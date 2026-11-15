// SPDX-License-Identifier: GPL-3.0-or-later
//! Le PC émulé : carte mémoire, boucle vCPU, aiguillage des sorties de VM.
//!
//! Carte mémoire :
//! ```text
//! 00000–9FFFF  RAM conventionnelle (640 Ko)
//! A0000–AFFFF  fenêtre graphique VGA : non mappée, chaque accès sort (MMIO)
//! B0000–BFFFF  mémoire texte : RAM
//! C0000–EFFFF  ROM d'extension / UMB : RAM
//! F0000–FFFFF  ROM BIOS : lecture seule
//! 100000–…     mémoire étendue (HMA comprise, A20 active)
//! ```

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::backend::{Backend, Error, Exit, Result};
use crate::bios::{self, Bios, ROM, ROM_SEG};
use crate::dos;
use crate::hle::{AX, CF, Flow, Regs, set_ret_flag};
use crate::host::Host;
use crate::interp::{self, Mmio};
use crate::io::IoBus;
use crate::kvm::{KvmVm, Region, VGA_WINDOW};
use crate::host::InputEvent;
use crate::kbc::{Kbc, KbcPorts};
use crate::keyboard;
use crate::pic::{Pic, PicPorts};
use crate::pit::{Pit, PitPorts};
use crate::throttle::Throttle;
use crate::timer::Heartbeat;
use crate::vga::{Access, Vga, VgaPorts};

/// Attente maximale sur HLT sans échéance de minuterie (entrées à relire).
const HALT_MAX: Duration = Duration::from_millis(10);

/// Image de l'écran : mode graphique, ou texte si une police est fournie
/// (clignotement à environ 1,9 Hz depuis `started`).
fn frame_of(
    vga: &Vga,
    mem: &crate::mem::GuestMem,
    font: Option<&crate::font::Font>,
    started: Instant,
) -> Option<(usize, usize, Vec<u32>)> {
    let mut px = Vec::new();
    let blink_on = (started.elapsed().as_millis() / 267).is_multiple_of(2);
    let (w, h) = crate::render::render(vga, mem, &mut px)
        .or_else(|| crate::render::render_text(vga, mem, font?, blink_on, &mut px))?;
    Some((w, h, px))
}

/// Le programme a remplacé l'INT 09h de la ROM (il lit le clavier lui-même).
fn int09_hooked(mem: &crate::mem::GuestMem) -> bool {
    mem.read_u32(0x09 * 4) != (u32::from(ROM_SEG) << 16 | u32::from(bios::INT09))
}

/// Battement : la période de l'IRQ 0, entre 50 µs et 10 ms.
fn heartbeat_period(irq0: Duration) -> Duration {
    irq0.clamp(Duration::from_micros(50), Duration::from_millis(10))
}

/// Instructions exécutées (par l'interpréteur) entre la commande d'un
/// transfert Sound Blaster court et son IRQ : assez pour que le pilote ait
/// remis à zéro le drapeau qu'il attend, assez peu pour rester dans sa
/// boucle d'attente (≈ 10 µs à la vitesse native, calibrée pour un 386).
const SHORT_TRANSFER_STEPS: u32 = 64;

/// Port d'appel hôte utilisé par les stubs.
pub const HOST_CALL_PORT: u16 = 0xE0;
/// Port par lequel l'INT 09h de la ROM confie un scancode à l'hôte.
const HOST_KBD_PORT: u16 = 0xE1;
/// Intervalle entre deux scancodes livrés au 8042.
const KEY_SPACING: Duration = Duration::from_millis(2);
/// Durée d'appui d'une touche avant son relâchement.
const KEY_HOLD: Duration = Duration::from_millis(40);
/// Délai laissé à un programme qui scrute le clavier après la fin de l'entrée.
const QUIT_GRACE: Duration = Duration::from_millis(300);
/// Période des images envoyées à l'affichage (70 Hz, comme la VGA).
const FRAME_PERIOD: Duration = Duration::from_micros(14_286);
/// Période de rafraîchissement de l'écran du terminal.
const TERM_FRAME: Duration = Duration::from_millis(33);

/// Scrutation du PIT (`Pc::pit_poll`) : écart maximal entre deux lectures
/// d'une même série, durée de série avant de céder, temps cédé par lecture.
const PIT_POLL_GAP: Duration = Duration::from_micros(100);
const PIT_POLL_AFTER: Duration = Duration::from_millis(2);
const PIT_POLL_SLEEP: Duration = Duration::from_micros(100);
/// Sonde d'attente active (`Pc::spin_check`) : instructions exécutées à
/// blanc au plus ; sommeil maximal (borne aussi le retard des IRQ sans
/// échéance connue, comme la fin d'un bloc DMA de la Sound Blaster :
/// dormir jusqu'à l'IRQ 0 coûtait plus à Doom, 77 % d'un cœur contre 69 %) ;
/// délai de la sonde qui suit le réveil ; nouvelles tentatives après un
/// échec quand la boucle a été vue.
const SPIN_PROBE_MAX: u64 = 256;
const SPIN_SLEEP_MAX: Duration = Duration::from_millis(1);
const SPIN_REPROBE: Duration = Duration::from_micros(100);
const SPIN_RETRIES: u32 = 6;

/// Écart maximal entre deux tours complets du haut de la boucle quand les
/// sorties passent par le chemin court (`Pc::resume`).
const FAST_PATH_MAX: Duration = Duration::from_micros(500);


/// Réveil avant le blanc vertical (`Pc::vga_poll`) : une dizaine de lignes.
const VBLANK_MARGIN: Duration = Duration::from_micros(300);

/// Instructions sans accès VRAM avant que l'interpréteur rende la main.
const IDLE_LIMIT: u64 = 256;
/// Plafond d'instructions par rafale interprétée.
const BURST_BUDGET: u64 = 100_000;

#[derive(Clone)]
pub struct Config {
    /// Mémoire étendue au-delà du premier Mo, en Ko.
    pub extended_kb: usize,
    /// Mémoire EMS en Ko (gardée côté hôte, en plus de la mémoire étendue).
    pub ems_kb: usize,
    /// Débit visé en MIPS ; `None` : vitesse native.
    pub mips: Option<f64>,
    /// Journal des interruptions servies par l'hôte.
    pub trace_int: bool,
    /// Ports d'E/S sans périphérique signalés.
    pub trace_io: bool,
    /// Rafales de l'interpréteur après une sortie MMIO (sinon : une sortie
    /// de VM par accès à la VRAM émulée ; `--no-interp`, pour mesurer).
    pub interp: bool,
    /// VBE 2.0 avec framebuffer linéaire (sinon VBE 1.2, `--vbe1`).
    pub vbe2: bool,
    /// Durée maximale d'exécution (diagnostic : registres affichés à l'arrêt).
    pub limit: Option<Duration>,
    /// Images de A:, B: et C: (`--fda`, `--fdb`, `--hda`) ; plusieurs pour
    /// A: (changement de disquette).
    pub disks: [Vec<std::path::PathBuf>; 3],
    /// Carte NE2000 et NAT en espace utilisateur (`--net`), avec les ports
    /// de l'hôte redirigés vers l'invité.
    pub net: Option<Vec<crate::net::Forward>>,
    /// Image du CD (lecteur ATAPI en maître du canal IDE secondaire).
    pub cdrom: Option<std::path::PathBuf>,
    /// Sound Blaster Pro au lieu de la SB16 (`--sb pro`).
    pub sb_pro: bool,
    /// Variables d'environnement du DOS intégré (`--set`).
    pub env: Vec<(String, String)>,
    /// ESS AudioDrive ES1688 (`--sb ess`) : compatible SB Pro, FM ESFM.
    pub sb_ess: bool,
    /// Puce FM : OPL3 (SB Pro 2, SB16), OPL2, double OPL2 (SB Pro 1).
    pub opl: crate::opl::OplModel,
    /// Enregistrement de la musique MIDI (MPU-401) en fichier MIDI standard.
    pub midi_out: Option<std::path::PathBuf>,
    /// Banque du synthétiseur General MIDI sur OPL3 ; `None` : pas de synthèse.
    pub midi_bank: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self { extended_kb: 15 * 1024, ems_kb: 4096, mips: None, trace_int: false, trace_io: false, interp: true, vbe2: true, limit: None, disks: Default::default(), net: None, cdrom: None, sb_pro: false, env: Vec::new(), sb_ess: false, opl: crate::opl::OplModel::Opl3, midi_out: None, midi_bank: Some(crate::midisynth::DEFAULT_BANK) }
    }
}

/// Accès MMIO vus par l'interpréteur : fenêtre VGA (logique des plans),
/// écritures en ROM (ignorées).
struct MmioBus<'a> {
    vga: &'a mut Vga,
    accesses: u64,
    reads: u64,
    /// Ports servis pendant une rafale (`IoBus::serves_in_burst`).
    io: Option<&'a mut IoBus>,
    port_accesses: u64,
    /// Traversée d'un choix de plan de lecture (`Access::PlaneRead`) :
    /// registres VGA servis ici, arrêt dès que la configuration en sort.
    plane_read: bool,
}

/// Bus de la sonde d'attente active (`interp::probe_spin`) : la VRAM et la
/// ROM sont signalées (la sonde échoue) sans être lues, pour ne pas toucher
/// aux verrous de la carte.
struct SpinBus;

impl Mmio for SpinBus {
    fn claims(&self, addr: u32, _write: bool) -> bool {
        let a = addr as usize;
        VGA_WINDOW.contains(&a) || ROM.contains(&a)
    }
    fn read8(&mut self, _addr: u32) -> u8 {
        0xFF
    }
    fn write8(&mut self, _addr: u32, _v: u8) {}
}

/// Ports d'index et de données du séquenceur et du contrôleur graphique.
fn vga_plane_port(port: u16) -> bool {
    matches!(port, 0x3C4 | 0x3C5 | 0x3CE | 0x3CF)
}

/// Ports de la palette (DAC) : index d'écriture, de lecture, données.
fn vga_dac_port(port: u16) -> bool {
    matches!(port, 0x3C7..=0x3C9)
}

impl MmioBus<'_> {
    /// Port VGA servi directement pendant une rafale (la carte est déjà
    /// empruntée par le bus) : la palette en rafale ordinaire, les plans
    /// pendant la traversée d'un choix de plan de lecture.
    fn serves_vga(&self, port: u16) -> bool {
        (self.io.is_some() && vga_dac_port(port)) || (self.plane_read && vga_plane_port(port))
    }
}

impl Mmio for MmioBus<'_> {
    fn claims(&self, addr: u32, write: bool) -> bool {
        let a = addr as usize;
        VGA_WINDOW.contains(&a) || (write && ROM.contains(&a))
    }
    fn read8(&mut self, addr: u32) -> u8 {
        self.accesses += 1;
        self.reads += 1;
        self.vga.read(addr as usize - VGA_WINDOW.start)
    }
    fn write8(&mut self, addr: u32, v: u8) {
        if VGA_WINDOW.contains(&(addr as usize)) {
            self.accesses += 1;
            self.vga.write(addr as usize - VGA_WINDOW.start, v);
        }
    }
    fn port_claims(&self, port: u16) -> bool {
        self.serves_vga(port) || self.io.as_ref().is_some_and(|io| io.serves_in_burst(port))
    }
    fn port_in(&mut self, port: u16, size: u8) -> u32 {
        self.port_accesses += 1;
        if self.serves_vga(port) {
            let lo = u32::from(self.vga.port_read(port));
            return if size == 2 { lo | u32::from(self.vga.port_read(port + 1)) << 8 } else { lo };
        }
        let mut data = [0u8; 4];
        let n = usize::from(size);
        if let Some(io) = &mut self.io {
            io.read(port, size, &mut data[..n]);
        }
        u32::from_le_bytes(data)
    }
    fn port_out(&mut self, port: u16, size: u8, v: u32) {
        self.port_accesses += 1;
        if self.serves_vga(port) {
            self.vga.port_write(port, v as u8);
            if size == 2 {
                self.vga.port_write(port + 1, (v >> 8) as u8);
            }
            return;
        }
        if let Some(io) = &mut self.io {
            io.write(port, size, &v.to_le_bytes()[..usize::from(size)]);
        }
    }
    fn stop(&self) -> bool {
        self.plane_read && !matches!(self.vga.access(), Access::PlaneRead(_))
    }
}

/// Suivi du mappage de la fenêtre A0000 (docs/JOURNAL.md, « décision sur la
/// mémoire vidéo ») : linéaire tout de suite, plan unique après
/// `MAP_THRESHOLD` accès émulés sans quitter le mode « un plan » (les
/// changements de plan ne remettent pas le compte à zéro), puis
/// maintenu d'un plan à l'autre : un changement de plan ne coûte que deux
/// copies de 64 Ko (scène de `Vram`), moins qu'une sortie MMIO.
#[derive(Default)]
struct WindowState {
    mapped: bool,
    /// Mappée en lecture seule (`Access::PlaneRead`).
    readonly: bool,
    /// Écritures émulées depuis le dernier choix d'un plan de lecture : la
    /// lecture seule est gardée en deçà de `RO_WRITE_LIMIT`.
    writes_since_read: u64,
    epoch: u64,
    emulated_in_epoch: u64,
    /// Lectures émulées dans cette configuration (plan de lecture ≠ plan écrit).
    reads_in_epoch: u64,
    /// Configurations successives en mode « un plan » (`emulated_in_epoch`
    /// les cumule alors).
    plane_streak: bool,
}

const MAP_THRESHOLD: u64 = 600;

/// Écritures tolérées en lecture seule après un choix de plan de lecture,
/// quand la traversée par l'interpréteur n'a pas suffi : un motif « plan lu
/// = p » puis « plan écrit = p » avec quelques copies reste en lecture
/// seule (pas deux remappages par plan) ; le rendu repasse en écriture.
const RO_WRITE_LIMIT: u64 = 1024;

#[derive(Default)]
pub struct Stats {
    pub exits: u64,
    pub mmio_exits: u64,
    pub interpreted: u64,
    pub host_calls: u64,
    pub irqs: u64,
    /// Accès à la VRAM passés par la logique des plans (MMIO, interpréteur).
    pub vram_emulated: u64,
    /// Mappages et démappages de la fenêtre A0000.
    pub vram_maps: u64,
    pub vram_unmaps: u64,
    /// Changements de page de l'affichage (écritures du registre 0Ch du CRTC).
    pub page_flips: u64,
    /// Accès aux ports servis en rafale par l'interpréteur (sans sortie).
    pub burst_ports: u64,
    /// Temps cédés à l'hôte pendant une attente active sur le PIT.
    pub pit_yields: u64,
    /// Sommeils jusqu'au blanc vertical pendant une attente sur 3DAh.
    pub vga_yields: u64,
    /// Sommeils pendant une attente active sans sortie (`Pc::spin_check`).
    pub spin_sleeps: u64,
    /// Sorties servies par le chemin court (registres de plans).
    pub fast_exits: u64,
    /// Sorties d'E/S par port (lectures et écritures).
    pub io_ports: PortCounts,
}

/// Compteur par port, indexé directement : incrémenté à chaque sortie d'E/S
/// (une table de hachage y coûtait un SipHash, ≈ 0,6 % du temps de Doom).
pub struct PortCounts(Box<[u64]>);

impl Default for PortCounts {
    fn default() -> Self {
        Self(vec![0; 0x10000].into_boxed_slice())
    }
}

impl PortCounts {
    pub fn add(&mut self, port: u16) {
        self.0[usize::from(port)] += 1;
    }
}

impl std::fmt::Debug for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ports: Vec<String> = self.top_ports(20).iter().map(|(p, c)| format!("{p:03X}h: {c}")).collect();
        f.debug_struct("Stats")
            .field("exits", &self.exits)
            .field("mmio_exits", &self.mmio_exits)
            .field("interpreted", &self.interpreted)
            .field("host_calls", &self.host_calls)
            .field("irqs", &self.irqs)
            .field("vram_emulated", &self.vram_emulated)
            .field("vram_maps", &self.vram_maps)
            .field("vram_unmaps", &self.vram_unmaps)
            .field("page_flips", &self.page_flips)
            .field("burst_ports", &self.burst_ports)
            .field("pit_yields", &self.pit_yields)
            .field("vga_yields", &self.vga_yields)
            .field("spin_sleeps", &self.spin_sleeps)
            .field("fast_exits", &self.fast_exits)
            .field("io_ports", &ports)
            .finish()
    }
}

impl Stats {
    /// Les `n` ports les plus sollicités, du plus au moins fréquent.
    pub fn top_ports(&self, n: usize) -> Vec<(u16, u64)> {
        let mut v: Vec<_> = (0..=u16::MAX).map(|p| (p, self.io_ports.0[usize::from(p)])).filter(|&(_, c)| c > 0).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(n);
        v
    }
}

pub enum Event {
    /// Le programme s'est terminé (code de sortie).
    Exit(u8),
    /// Fin d'un pas (pas à pas).
    Step,
}

pub struct Pc<B: Backend = KvmVm> {
    pub vm: B,
    pub io: IoBus,
    pub host: Host,
    pub stats: Stats,
    bios: Bios,
    dos: dos::Dos,
    vga: Rc<RefCell<Vga>>,
    window: WindowState,
    throttle: Option<Throttle>,
    pic: Rc<RefCell<Pic>>,
    pit: Rc<RefCell<Pit>>,
    heartbeat: Heartbeat,
    kbc: Rc<RefCell<Kbc>>,
    xms: crate::xms::Xms,
    dma: Rc<RefCell<crate::dma::Dma>>,
    gameport: Rc<RefCell<crate::gameport::Gameport>>,
    sb: Rc<RefCell<crate::sb16::Sb16>>,
    opl: Rc<RefCell<crate::opl::Opl>>,
    mixer: crate::audio::Mixer,
    ems: crate::ems::Ems,
    /// Scancodes en route vers le 8042, livrés au rythme d'une frappe.
    keys: VecDeque<u8>,
    last_scancode: Option<Instant>,
    /// Dernier appui livré (son relâchement attend KEY_HOLD).
    last_made: Option<u8>,
    /// Le programme attend une touche (appel repris après HLT).
    waiting_key: bool,
    /// Fin de l'entrée constatée, tout étant lu : début du délai de grâce.
    drained_at: Option<Instant>,
    /// Le programme a scruté le clavier depuis (délai de grâce applicable).
    polled_after_drain: bool,
    /// Pause d'un script en cours : pas de nouvelle entrée avant.
    input_paused_until: Option<Instant>,
    /// Script en attente d'un texte à l'écran (`{until:…}`), et dernier examen.
    until_text: Option<(String, Instant)>,
    font: Option<crate::font::Font>,
    /// Glide (3dfx) servi par l'hôte, créé au premier appel de la DLL.
    glide: std::cell::RefCell<Option<Box<crate::glide::Glide>>>,
    started: Instant,
    limit: Option<Duration>,
    last_frame: Option<Instant>,
    /// Dernier rafraîchissement de l'écran du terminal.
    last_render: Option<Instant>,
    /// Pas à pas en cours : pas d'interruption injectée entre deux pas.
    stepping: bool,
    interp: bool,
    trace_int: bool,
    /// Services non gérés déjà signalés (vecteur, AH).
    unhandled: BTreeSet<(u8, u8)>,
    /// État de la porte A20 appliqué aux slots mémoire.
    a20: Option<bool>,
    /// Scrutation du clavier à vide (INT 16h AH=01h/11h sans touche) : début
    /// de la série et dernier appel.
    idle_polls: Option<(Instant, Instant)>,
    /// Scrutation du compteur du PIT : début de la série et dernière lecture.
    pit_polls: Option<(Instant, Instant)>,
    /// Scrutation du registre d'état VGA sans autre écriture sur un port :
    /// début de la série et dernière lecture.
    vga_polls: Option<(Instant, Instant)>,
    /// Sondes d'attente active encore permises après un échec (`spin_check`).
    spin_retries: u32,
    /// La sortie précédente permet le chemin court (`resume`), et dernier
    /// tour complet du haut de la boucle.
    fast_next: bool,
    last_full: Instant,
    /// Souris PS/2 de l'interface du BIOS (vrai DOS).
    ps2: crate::mouse::Ps2,
    /// DOS fourni par l'hôte ; faux après un démarrage sur image disque.
    dos_hle: bool,
    /// Lecteur de démarrage (redémarrages à chaud).
    boot_drive: u8,
    /// Tailles de la mémoire (octets) et de l'EMS (Ko), pour un redémarrage.
    mem_size: usize,
    ems_kb: usize,
    /// Synthétiseur General MIDI sur OPL3 (derrière la MPU-401).
    midi: Option<crate::midisynth::SharedSynth>,
    /// Disque compact et lecture de ses pistes audio.
    cd: Option<(crate::cdrom::SharedDisc, crate::cdrom::SharedPlayer)>,
    /// Canal IDE secondaire et son lecteur de CD.
    ide: Option<Rc<RefCell<crate::ide::Ide>>>,
    /// Carte réseau et lien avec le thread réseau.
    nic: Option<(Rc<RefCell<crate::ne2000::Ne2000>>, crate::net::NetLink)>,
}

impl Pc<KvmVm> {
    pub fn new(cfg: &Config, host: Host) -> Result<Self> {
        let size = 0x10_0000 + cfg.extended_kb * 1024;
        let r = |start, end, readonly| Region { start, end, readonly };
        let layout = [
            r(0, VGA_WINDOW.start, false),
            r(VGA_WINDOW.end, ROM.start, false),
            r(ROM.start, ROM.end, true),
            // La HMA a son propre slot (porte A20, `Backend::set_a20`).
            r(crate::kvm::HMA.end, size, false),
        ];
        let vm = KvmVm::with_layout(size, &layout)?;
        Self::with_backend(vm, cfg, host)
    }
}

impl<B: Backend> Pc<B> {
    pub fn with_backend(vm: B, cfg: &Config, mut host: Host) -> Result<Self> {
        let mixer = crate::audio::Mixer::new(std::mem::replace(&mut host.audio, Box::new(crate::host::NullAudio::default())), cfg.opl);
        let vga = Rc::new(RefCell::new(Vga::new()?));
        let mut bios = Bios::new(cfg.extended_kb, vga.clone());
        bios.vbe2 = cfg.vbe2;
        bios.disks = crate::disk::Disks::open(&cfg.disks)?;
        bios.install(vm.mem());
        let mut dos = dos::Dos::new();
        dos.sb_pro = cfg.sb_pro || cfg.sb_ess;
        dos.env = cfg.env.clone();
        dos.boot(vm.mem());
        let throttle = cfg.mips.map(Throttle::new).transpose()?;
        let mut io = IoBus::default();
        io.trace = cfg.trace_io;
        // Port de diagnostic POST, utilisé aussi comme temporisation.
        io.map(0x80..=0x80, Box::new(crate::io::Sink));
        let cd = match &cfg.cdrom {
            Some(path) => {
                let disc = std::sync::Arc::new(crate::cdrom::Disc::open(path)?);
                let player = crate::cdrom::Player::new(Some(disc.clone())).shared();
                Some((disc, player))
            }
            None => None,
        };
        let ide = match &cd {
            Some((disc, player)) => {
                let ide = crate::ide::Ide::new(crate::cdrom::Drive::new(Some(disc.clone()), player.clone())).shared();
                io.map(crate::ide::BASE..=crate::ide::BASE + 7, Box::new(crate::ide::IdePorts(ide.clone())));
                io.map(crate::ide::CONTROL..=crate::ide::CONTROL, Box::new(crate::ide::IdePorts(ide.clone())));
                Some(ide)
            }
            None => None,
        };
        for base in [0x1F0u16, 0x170, 0x1E8, 0x168] {
            if !(ide.is_some() && base == crate::ide::BASE) {
                io.map(base..=base + 7, Box::new(crate::io::NoIde));
            }
        }
        let pic = Pic::new().shared();
        io.map(0x20..=0x21, Box::new(PicPorts(pic.clone())));
        io.map(0xA0..=0xA1, Box::new(PicPorts(pic.clone())));
        let pit = Pit::new().shared();
        io.map(0x40..=0x43, Box::new(PitPorts(pit.clone())));
        io.map(0x61..=0x61, Box::new(PitPorts(pit.clone())));
        let heartbeat = Heartbeat::new(heartbeat_period(pit.borrow().irq0_period()))?;
        io.map(0x70..=0x71, Box::new(crate::rtc::Rtc::new(cfg.extended_kb)));
        io.map(0x3B4..=0x3BA, Box::new(VgaPorts(vga.clone())));
        io.map(0x3C0..=0x3DF, Box::new(VgaPorts(vga.clone())));
        let kbc = Kbc::new().shared();
        io.map(0x60..=0x60, Box::new(KbcPorts(kbc.clone())));
        io.map(0x64..=0x64, Box::new(KbcPorts(kbc.clone())));
        io.map(0x92..=0x92, Box::new(KbcPorts(kbc.clone())));
        let gameport = Rc::new(RefCell::new(crate::gameport::Gameport::new()));
        io.map(0x201..=0x201, Box::new(crate::gameport::GameportPorts(gameport.clone())));
        let dma = crate::dma::Dma::new().shared();
        io.map(0x00..=0x0F, Box::new(crate::dma::DmaPorts(dma.clone())));
        io.map(0x81..=0x8F, Box::new(crate::dma::DmaPorts(dma.clone())));
        io.map(0xC0..=0xDF, Box::new(crate::dma::DmaPorts(dma.clone())));
        let sb = Rc::new(RefCell::new(crate::sb16::Sb16::new()));
        sb.borrow_mut().pro = cfg.sb_pro || cfg.sb_ess;
        sb.borrow_mut().ess = cfg.sb_ess;
        // Interface MIDI MPU-401 (UART), comme sur la SB16.
        // Sorties : synthétiseur General MIDI sur OPL3, enregistrement.
        let mut sinks: Vec<Box<dyn crate::mpu401::MidiSink>> = Vec::new();
        let midi = match cfg.midi_bank {
            Some(bank) => {
                let synth = crate::midisynth::Synth::new(crate::audio::RATE, bank)?.shared();
                sinks.push(Box::new(crate::midisynth::SynthSink(synth.clone())));
                Some(synth)
            }
            None => None,
        };
        if let Some(p) = &cfg.midi_out {
            sinks.push(Box::new(crate::mpu401::MidiFile::create(p).map_err(|e| format!("{} : {e}", p.display()))?));
        }
        let mpu = crate::mpu401::Mpu401::new(sinks).shared();
        io.map(crate::mpu401::BASE..=crate::mpu401::BASE + 1, Box::new(crate::mpu401::MpuPorts(mpu)));
        // DSP et mixeur ; 228h–229h reviennent à la FM.
        io.map(crate::sb16::BASE + 4..=crate::sb16::BASE + 7, Box::new(crate::sb16::Sb16Ports(sb.clone())));
        io.map(crate::sb16::BASE + 0x0A..=crate::sb16::BASE + 0x0F, Box::new(crate::sb16::Sb16Ports(sb.clone())));
        // Servis pendant une rafale (sans en déclencher) : les boucles de
        // détection qui scrutent le DSP restent dans l'interpréteur.
        io.burst_on(crate::sb16::BASE + 4..=crate::sb16::BASE + 7, false);
        io.burst_on(crate::sb16::BASE + 0x0A..=crate::sb16::BASE + 0x0F, false);
        let opl = Rc::new(RefCell::new(crate::opl::Opl::new()));
        opl.borrow_mut().model = cfg.opl;
        for ports in [0x388..=0x38B, crate::sb16::BASE..=crate::sb16::BASE + 3, crate::sb16::BASE + 8..=crate::sb16::BASE + 9] {
            io.map(ports.clone(), Box::new(crate::opl::OplPorts(opl.clone())));
            // Les pilotes FM attendent après chaque écriture en relisant
            // l'état (≈ 35 lectures, temporisation AdLib) : une sortie de VM
            // chacune allongeait le gestionnaire de la musique au-delà de la
            // période de l'IRQ 0 (Doom : musique 1,3 % trop lente). Servis
            // en rafale par l'interpréteur.
            io.burst_on(ports, true);
        }
        let nic = match &cfg.net {
            Some(forwards) => {
                let nic = crate::ne2000::Ne2000::new().shared();
                let base = crate::ne2000::BASE;
                io.map(base..=base + 0x1F, Box::new(crate::ne2000::Ne2000Ports(nic.clone())));
                // Le pilote enchaîne les accès aux registres, entrecoupés de
                // lectures de 61h (temporisation du bus ISA).
                io.burst_on(base..=base + 0x1F, true);
                io.burst_on(0x61..=0x61, false);
                // Masque et fin d'interruption de son gestionnaire d'IRQ.
                io.burst_on(0x20..=0x21, false);
                io.burst_on(0xA0..=0xA1, false);
                Some((nic, crate::net::NetLink::start(forwards).map_err(|e| format!("network: {e}"))?))
            }
            None => None,
        };
        let mut pc = Self {
            midi,
            cd,
            ide,
            nic,
            vm,
            io,
            host,
            stats: Stats::default(),
            bios,
            dos,
            vga,
            window: WindowState::default(),
            throttle,
            pic,
            pit,
            heartbeat,
            kbc,
            xms: crate::xms::Xms::new(0x10_0000 + cfg.extended_kb * 1024),
            dma,
            gameport,
            sb,
            opl,
            mixer,
            ems: crate::ems::Ems::new(cfg.ems_kb),
            keys: VecDeque::new(),
            last_scancode: None,
            last_made: None,
            waiting_key: false,
            drained_at: None,
            polled_after_drain: false,
            last_render: None,
            font: None,
            glide: std::cell::RefCell::new(None),
            started: Instant::now(),
            limit: cfg.limit,
            last_frame: None,
            input_paused_until: None,
            until_text: None,
            stepping: false,
            trace_int: cfg.trace_int,
            interp: cfg.interp,
            unhandled: BTreeSet::new(),
            a20: None,
            idle_polls: None,
            pit_polls: None,
            vga_polls: None,
            spin_retries: 0,
            fast_next: false,
            last_full: Instant::now(),
            ps2: Default::default(),
            dos_hle: true,
            boot_drive: 0,
            mem_size: 0x10_0000 + cfg.extended_kb * 1024,
            ems_kb: cfg.ems_kb,
        };
        pc.sync_a20()?;
        let fb = pc.vga.borrow().fb_host_addr();
        pc.vm.map_lfb(fb, crate::vga::FB_SIZE)?;
        Ok(pc)
    }

    /// Monte un répertoire hôte comme lecteur (0 = A:).
    pub fn mount(&mut self, drive: u8, root: std::path::PathBuf) {
        self.dos.mount(drive, root);
    }

    /// Disquette ou CD choisi dans le menu.
    fn media_event(&mut self, e: InputEvent) {
        match e {
            InputEvent::InsertFloppy(path) => {
                if let Err(e) = self.bios.disks.insert_floppy(&path) {
                    eprintln!("[disk] {e}");
                }
            }
            InputEvent::InsertCd(path) => {
                if let Err(e) = self.change_cd(path.as_deref()) {
                    eprintln!("[cd] {e}");
                }
            }
            _ => {}
        }
    }

    /// Change le disque du lecteur (ou l'éjecte) : lecteur ATAPI, musique,
    /// et D: du DOS intégré s'il est monté. Le programme l'apprend comme sur
    /// un vrai lecteur (UNIT ATTENTION, MSCDEX IOCTL 9).
    pub fn change_cd(&mut self, image: Option<&std::path::Path>) -> Result<()> {
        let Some((_, player)) = &self.cd else { return Err("no CD drive (--cdrom option at startup)".into()) };
        let disc = image.map(crate::cdrom::Disc::open).transpose()?.map(std::sync::Arc::new);
        let player = player.clone();
        {
            let mut p = player.borrow_mut();
            p.stop();
            p.disc = disc.clone();
        }
        if let Some(ide) = &self.ide {
            ide.borrow_mut().drive_mut().insert(disc.clone());
        }
        self.dos.change_cd(disc.clone())?;
        if let (Some(d), Some(cd)) = (disc, self.cd.as_mut()) {
            cd.0 = d;
        }
        match image {
            Some(p) => eprintln!("[cd] {}", p.display()),
            None => eprintln!("[cd] disc ejected"),
        }
        Ok(())
    }

    /// Monte une image ISO comme lecteur de CD du DOS intégré (MSCDEX).
    pub fn mount_cd(&mut self, drive: u8) -> Result<()> {
        let (disc, player) = self.cd.clone().ok_or("no CD image (--cdrom)")?;
        self.dos.mount_cd(self.vm.mem(), drive, disc, player).map_err(Into::into)
    }

    /// Charge un programme en mémoire sous le nom DOS `path` (sans disque).
    #[cfg(test)]
    pub fn load_image(&mut self, image: &[u8], path: &[u8], tail: &[u8]) -> Result<()> {
        let cpu = self.dos.start_image(self.vm.mem(), image, path, tail)?;
        self.vm.set_state(&cpu)
    }

    #[cfg(test)]
    pub fn load_com(&mut self, image: &[u8]) -> Result<()> {
        self.load_image(image, b"C:\\PROGRAM.COM", b"")
    }

    /// Chemin DOS d'un fichier hôte situé sous un lecteur monté.
    pub fn dos_path_of(&self, host: &std::path::Path) -> Option<Vec<u8>> {
        self.dos.dos_path_of(host)
    }

    /// Répertoire courant du DOS intégré.
    pub fn chdir(&mut self, path: &[u8]) -> std::result::Result<(), u16> {
        self.dos.chdir(path)
    }

    /// Charge un programme d'un lecteur monté (« C:\\JEU.EXE »).
    pub fn load_program(&mut self, path: &[u8], tail: &[u8]) -> Result<()> {
        let cpu = self.dos.start(self.vm.mem(), path, tail)?;
        self.vm.set_state(&cpu)
    }

    /// Démarre sur l'image du lecteur `drive` (00h = A:, 80h = C:) comme un
    /// BIOS : secteur d'amorce en 0000:7C00, DL = lecteur. Le DOS de l'hôte
    /// s'efface ; XMS, EMS, souris, VBE et son restent servis par l'hôte.
    pub fn boot_disk(&mut self, drive: u8) -> Result<()> {
        let sector = self.bios.disks.boot_sector(drive)?;
        self.dos_hle = false;
        self.boot_drive = drive;
        // XMS et EMS de l'hôte retirées : le noyau d'un vrai DOS ne renvoie
        // pas l'INT 2Fh au BIOS, et JEMMEX refuse une EMS déjà présente.
        self.bios.host_memory = false;
        let mem = self.vm.mem();
        mem.write_u16(0x67 * 4, 0x67 * 16);
        mem.write_u16(0x67 * 4 + 2, ROM_SEG);
        // Souris PS/2 annoncée (bit 2 du mot d'équipement) : INT 15h AH=C2h.
        mem.write_u16(bios::bda::EQUIPMENT, mem.read_u16(bios::bda::EQUIPMENT) | 4);
        mem.fill(0x500, 0xA_0000 - 0x500, 0);
        mem.write(0x7C00, &sector);
        let mut cpu = crate::interp::Cpu::real_mode(0, 0x7C00, 0x7C00);
        cpu.set_lo(crate::hle::DX, drive);
        self.vm.set_state(&cpu)
    }

    /// Redémarrage (INT 19h, saut en FFFF:0000, 8042, port 92h, triple
    /// faute) : machine remise à neuf, puis amorçage sur le même lecteur.
    /// Sous le DOS de l'hôte, la session s'arrête.
    fn reboot(&mut self) -> Result<Option<u8>> {
        self.kbc.borrow_mut().reset = false;
        if self.dos_hle {
            return Ok(Some(0));
        }
        eprintln!("[boot] reboot");
        self.vm.reset_cpu()?;
        let mem = self.vm.mem();
        mem.fill(0, 0xA_0000, 0);
        self.bios.install(mem);
        self.xms = crate::xms::Xms::new(self.mem_size);
        self.ems = crate::ems::Ems::new(self.ems_kb);
        *self.pic.borrow_mut() = Pic::new();
        *self.pit.borrow_mut() = Pit::new();
        self.ps2 = Default::default();
        self.kbc.borrow_mut().set_a20(false);
        self.sync_a20()?;
        self.boot_disk(self.boot_drive)?;
        Ok(None)
    }

    /// Shell interne : invite, commandes, programmes lancés tour à tour.
    /// Rend la main sur EXIT ou quand l'entrée clavier est épuisée.
    pub fn shell(&mut self) -> Result<u8> {
        let mut code = 0;
        loop {
            let mem = self.vm.mem();
            self.dos.prompt(mem, &mut self.host);
            let (vga, font, started) = (&self.vga, &self.font, self.started);
            let mut redraw = |mem: &crate::mem::GuestMem, host: &mut Host| {
                if let Some(t) = &mut host.term {
                    t.render(mem);
                }
                if host.wants_frames
                    && let Some((width, height, pixels)) = frame_of(&vga.borrow(), mem, font.as_ref(), started)
                {
                    host.display.present(&crate::host::Frame { width, height, pixels: &pixels });
                }
            };
            let Some(line) = self.dos.shell_read(mem, &mut self.host, &mut redraw) else { return Ok(code) };
            for e in std::mem::take(&mut self.dos.pending_media) {
                self.media_event(e);
            }
            let mem = self.vm.mem();
            match self.dos.shell_command(mem, &mut self.host, &line)? {
                dos::Action::Continue => {}
                dos::Action::Exit => return Ok(code),
                dos::Action::Run(path, tail) => {
                    self.load_program(&path, &tail)?;
                    code = self.run()?;
                }
            }
        }
    }

    /// Exécute jusqu'à la fin du programme.
    pub fn run(&mut self) -> Result<u8> {
        loop {
            if let Event::Exit(code) = self.resume()? {
                return Ok(code);
            }
        }
    }

    pub fn set_single_step(&mut self, on: bool) -> Result<()> {
        self.stepping = on;
        self.vm.request_irq_window(false);
        self.vm.set_single_step(on)
    }

    /// Applique aux slots mémoire l'état de la porte A20 tenu par le 8042.
    fn sync_a20(&mut self) -> Result<()> {
        let open = self.kbc.borrow().a20();
        if self.a20 != Some(open) {
            self.vm.set_a20(open)?;
            self.a20 = Some(open);
        }
        Ok(())
    }

    /// Mappe, remappe ou démappe la fenêtre A0000 selon la configuration VGA.
    fn update_window(&mut self) -> Result<()> {
        let mut vga = self.vga.borrow_mut();
        let w = &mut self.window;
        let access = vga.access();
        if vga.epoch != w.epoch {
            w.epoch = vga.epoch;
            // D'un plan à l'autre, les accès s'additionnent : un flux de
            // colonnes (un plan chacune) finit par être mappé.
            let plane = matches!(access, Access::Plane(_) | Access::PlaneRead(_));
            if !(plane && w.plane_streak) {
                w.emulated_in_epoch = 0;
            }
            w.plane_streak = plane;
            w.reads_in_epoch = 0;
        }
        if matches!(access, Access::PlaneRead(_)) {
            w.writes_since_read = 0;
        }
        let want = match access {
            Access::Emulated => None,
            Access::Linear | Access::Bank(_) => Some(access),
            // Lectures vues alors qu'elles viseraient un autre plan que celui mappé.
            Access::Plane(p) if p != vga.read_plane() && w.reads_in_epoch > 0 => None,
            Access::Plane(_) | Access::PlaneRead(_) if w.mapped || w.emulated_in_epoch >= MAP_THRESHOLD => Some(access),
            Access::Plane(_) | Access::PlaneRead(_) => None,
        };
        match want {
            Some(a) => {
                vga.point_window(a)?;
                // Lecture seule pour un plan de lecture, gardée tant que peu
                // d'écritures suivent (elles sortent et sont émulées). Une
                // bascule recrée le slot : KVM ne change pas ce drapeau en place.
                let readonly = match a {
                    Access::PlaneRead(_) => true,
                    Access::Plane(_) => w.mapped && w.readonly && w.writes_since_read < RO_WRITE_LIMIT,
                    _ => false,
                };
                if w.mapped && w.readonly != readonly {
                    self.vm.map_vga(None, false)?;
                    w.mapped = false;
                }
                if !w.mapped {
                    self.vm.map_vga(Some(vga.window_host_addr()), readonly)?;
                    w.mapped = true;
                    w.readonly = readonly;
                    self.stats.vram_maps += 1;
                }
            }
            None if w.mapped => {
                self.vm.map_vga(None, false)?;
                w.mapped = false;
                self.stats.vram_unmaps += 1;
            }
            None => {}
        }
        Ok(())
    }

    /// Minuterie : IRQ 0 échue, cadence du battement.
    fn service_timers(&mut self) -> Result<()> {
        let now = Instant::now();
        let mut pit = self.pit.borrow_mut();
        if pit.take_irq0(now) {
            self.pic.borrow_mut().raise(0);
        }
        self.heartbeat.set_period(heartbeat_period(pit.irq0_period()))?;
        let speaker = pit.speaker();
        drop(pit);
        // Transfert court (détection d'IRQ) : achevé après quelques
        // instructions de l'invité (voir `Sb16::short_transfer_end`),
        // exécutées par l'interpréteur. Le pas à pas du processeur (TF) s'y
        // prêtait mal : un `pushf` de l'invité empilait TF, rétabli plus tard
        // par un `popf` → exception de débogage (DOS/4GW : « exception 01h »,
        // Doom sur SB Pro, 4 à 6 démarrages sur 8).
        if self.sb.borrow().short_transfer_end().is_some() && !self.stepping {
            self.short_transfer_burst()?;
        }
        let mut sb = self.sb.borrow_mut();
        let mut cd = self.cd.as_ref().map(|(_, p)| p.borrow_mut());
        let mut midi = self.midi.as_ref().map(|m| m.borrow_mut());
        let mut sources: Vec<&mut dyn crate::audio::AudioSource> = Vec::new();
        if let Some(c) = cd.as_deref_mut() {
            sources.push(c);
        }
        if let Some(m) = midi.as_deref_mut() {
            sources.push(m);
        }
        self.mixer.advance(now, &mut sb, &mut self.opl.borrow_mut(), &mut self.dma.borrow_mut(), self.vm.mem(), speaker, &mut sources);
        drop(sources);
        drop((cd, midi));
        if sb.take_irq() {
            self.pic.borrow_mut().raise(crate::sb16::IRQ);
        }
        drop(sb);
        self.service_net();
        if let Some(ide) = &self.ide
            && ide.borrow_mut().take_irq()
        {
            self.pic.borrow_mut().raise(crate::ide::IRQ);
        }
        Ok(())
    }

    /// Trames émises par la carte vers le thread réseau, et inversement.
    fn service_net(&mut self) {
        let Some((nic, link)) = &self.nic else { return };
        let mut nic = nic.borrow_mut();
        for frame in nic.outbox.drain(..) {
            link.send(frame);
        }
        link.receive(|frame| nic.deliver(frame));
        if nic.take_irq() {
            self.pic.borrow_mut().raise(crate::ne2000::IRQ);
        }
    }

    /// Fin d'un transfert Sound Blaster court : `SHORT_TRANSFER_STEPS`
    /// instructions de l'invité dans l'interpréteur (sans TF), puis le
    /// transfert est lu et son IRQ levée. Une instruction que l'interpréteur
    /// ne sert pas arrête la rafale plus tôt.
    fn short_transfer_burst(&mut self) -> Result<()> {
        if self.interp {
            let mut vga = self.vga.borrow_mut();
            let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0, io: Some(&mut self.io), port_accesses: 0, plane_read: false };
            let burst = interp::continue_in_host(&mut self.vm, &mut bus, u64::MAX, u64::from(SHORT_TRANSFER_STEPS))?;
            self.stats.interpreted += burst.instructions;
        }
        self.sb.borrow_mut().complete_short(&mut self.dma.borrow_mut(), self.vm.mem());
        Ok(())
    }

    /// Entrées de l'hôte → scancodes → 8042 → IRQ 1. Une frappe à la fois :
    /// un octet par intervalle, le tampon du 8042 vide, et l'appui sur une
    /// touche (hors modificateurs) attend que le programme ait lu la
    /// précédente dans la BDA, sauf s'il a détourné l'INT 09h. Les boucles
    /// d'événements (DFlat) gardent ainsi l'ordre des raccourcis et du texte,
    /// et lisent l'état des modificateurs de la frappe (AH=02h).
    fn service_input(&mut self) {
        let mut paused = self.input_paused_until.is_some_and(|t| Instant::now() < t);
        if let Some((text, last)) = &mut self.until_text {
            // L'écran est examiné toutes les 50 ms au plus.
            if last.elapsed() >= Duration::from_millis(50) {
                *last = Instant::now();
                if bios::screen_text(self.vm.mem()).iter().any(|l| l.contains(text.as_str())) {
                    self.until_text = None;
                }
            }
            paused |= self.until_text.is_some();
        }
        while self.keys.is_empty() && !self.host.quit && !paused {
            match self.host.input.poll() {
                Some(InputEvent::Wait(ms)) => {
                    self.input_paused_until = Some(Instant::now() + Duration::from_millis(u64::from(ms)));
                    break;
                }
                Some(InputEvent::Quit) => self.host.quit = true,
                // Le compteur d'instructions est lié au thread du vCPU : le
                // bridage est recréé ici.
                Some(InputEvent::JoyPresent(n)) => self.gameport.borrow_mut().set_present(n),
                Some(InputEvent::JoyAxis(i, v)) => self.gameport.borrow_mut().set_axis(usize::from(i), v),
                Some(InputEvent::JoyButton(i, down)) => self.gameport.borrow_mut().set_button(usize::from(i), down),
                Some(InputEvent::Speed(mips)) => match mips.map(Throttle::new).transpose() {
                    Ok(t) => self.throttle = t,
                    Err(e) => eprintln!("throttling: {e}"),
                },
                Some(InputEvent::NextDisk) => self.bios.disks.next_floppy(),
                Some(e @ (InputEvent::InsertFloppy(_) | InputEvent::InsertCd(_))) => self.media_event(e),
                Some(InputEvent::Until(text)) => {
                    self.until_text = Some((text, Instant::now() - Duration::from_secs(1)));
                    break;
                }
                Some(InputEvent::Key(code)) => self.keys.push_back(code),
                Some(InputEvent::Text(c)) => self.keys.extend(keyboard::text_scancodes(c)),
                Some(InputEvent::Mouse { dx, dy, buttons }) if !self.dos_hle => self.ps2.event(dx, dy, buttons),
                Some(InputEvent::Mouse { dx, dy, buttons }) => self.bios.mouse.event(self.vm.mem(), dx, dy, buttons),
                None => break,
            }
        }
        if self.ps2.take_irq() {
            self.pic.borrow_mut().raise(12);
        }
        let now = Instant::now();
        // Une touche reste enfoncée KEY_HOLD avant son relâchement : les jeux
        // qui lisent l'état du clavier à chaque image (Wolfenstein 3D) la voient.
        let gap = match (self.keys.front(), self.last_made) {
            (Some(&b), Some(m)) if b == m | 0x80 => KEY_HOLD,
            _ => KEY_SPACING,
        };
        let spaced = self.last_scancode.is_none_or(|t| now.duration_since(t) >= gap);
        if let Some(&b) = self.keys.front()
            && spaced
            && self.kbc.borrow().is_empty()
        {
            let mem = self.vm.mem();
            let key_press = b & 0x80 == 0 && b != 0xE0 && keyboard::modifier(b).is_none();
            if !key_press || int09_hooked(mem) || bios::peek_key(mem).is_none() {
                self.kbc.borrow_mut().feed(b);
                self.keys.pop_front();
                self.last_scancode = Some(now);
                self.last_made = (b & 0x80 == 0 && b != 0xE0).then_some(b);
            }
        }
        if self.kbc.borrow_mut().take_irq() {
            self.pic.borrow_mut().raise(1);
        }
    }

    /// Écran du terminal (`--term`), une trentaine de fois par seconde.
    pub fn render_term(&mut self, now: bool) {
        let due = now || self.last_render.is_none_or(|t| t.elapsed() >= TERM_FRAME);
        if let Some(term) = &mut self.host.term
            && due
        {
            term.render(self.vm.mem());
            self.last_render = Some(Instant::now());
        }
    }

    /// Fin de session après la fin de l'entrée (script épuisé, tube vidé),
    /// une fois tout ce qui a été tapé lu : tout de suite si le programme
    /// attend une touche ; après un délai de grâce s'il scrute le clavier ou
    /// a détourné l'INT 09h (boucles d'événements qui traitent la dernière
    /// touche au tour suivant). Un programme qui ne lit pas le clavier va
    /// jusqu'à sa fin.
    fn input_finished(&mut self) -> bool {
        let mem = self.vm.mem();
        let drained = self.host.quit && self.keys.is_empty() && self.kbc.borrow().is_empty() && bios::peek_key(mem).is_none();
        if !drained {
            self.drained_at = None;
            self.polled_after_drain = false;
            return false;
        }
        let since = *self.drained_at.get_or_insert_with(Instant::now);
        let reads_keys = self.polled_after_drain || int09_hooked(mem);
        self.waiting_key || (reads_keys && since.elapsed() >= QUIT_GRACE)
    }

    /// Injecte l'interruption en attente au PIC, ou demande une fenêtre.
    fn deliver_irq(&mut self) -> Result<()> {
        if self.stepping {
            return Ok(());
        }
        let pending = self.pic.borrow().pending().is_some();
        if pending && self.vm.can_inject() {
            if let Some(vector) = self.pic.borrow_mut().acknowledge() {
                self.vm.inject(vector)?;
                self.stats.irqs += 1;
            }
            self.vm.request_irq_window(false);
        } else {
            self.vm.request_irq_window(pending);
        }
        Ok(())
    }

    /// HLT : attend la prochaine interruption (IF doit être levé).
    fn halt(&mut self) -> Result<()> {
        if self.pic.borrow().pending().is_some() {
            return Ok(());
        }
        if self.vm.state()?.flags & (1 << 9) == 0 {
            return Err("HLT with interrupts disabled (IF = 0)".into());
        }
        let now = Instant::now();
        let until = self.pit.borrow().next_irq0().unwrap_or(now + HALT_MAX).min(now + HALT_MAX);
        self.wait_until(until);
        Ok(())
    }

    /// Attente active sans sortie de VM (Keen relit son compteur de tics
    /// jusqu'à l'image suivante : un cœur entier). Au battement, une sonde à
    /// blanc dans l'interpréteur vérifie que l'invité tourne dans une boucle
    /// qui se répète à l'identique (`interp::probe_spin`) : seule une
    /// interruption peut l'en sortir, et le vCPU dort jusqu'à la prochaine
    /// IRQ 0, au plus `SPIN_SLEEP_MAX`. Dormir revient à exécuter l'invité
    /// plus lentement : la sonde évite seulement de ralentir un calcul. Une sonde ponctuelle suit le réveil de
    /// `SPIN_REPROBE`, pour rattraper la boucle au retour du gestionnaire
    /// d'interruption sans attendre le battement suivant.
    fn spin_check(&mut self) -> Result<()> {
        if !self.interp || self.stepping || self.pic.borrow().pending().is_some() {
            return Ok(());
        }
        let cpu = self.vm.state()?;
        if cpu.flags & (1 << 9) == 0 || !interp::probe_spin(&cpu, self.vm.mem(), &mut SpinBus, SPIN_PROBE_MAX) {
            // Juste après le réveil, l'invité peut être encore dans le
            // gestionnaire d'interruption (Doom y mixe le son, DOS/4GW y
            // passe par le mode réel) : nouvelles tentatives, de plus en plus
            // espacées (≈ 6 ms en tout), tant que la boucle a été vue.
            if self.spin_retries > 0 {
                self.spin_retries -= 1;
                self.heartbeat.probe_in(SPIN_REPROBE * (1 << (SPIN_RETRIES - self.spin_retries)))?;
            }
            return Ok(());
        }
        self.spin_retries = SPIN_RETRIES;
        self.stats.spin_sleeps += 1;
        let now = Instant::now();
        let until = self.pit.borrow().next_irq0().unwrap_or(now + SPIN_SLEEP_MAX).min(now + SPIN_SLEEP_MAX);
        self.wait_until(until);
        self.heartbeat.probe_in(SPIN_REPROBE)?;
        Ok(())
    }

    /// Sommeil du vCPU jusqu'à `until`, écourté par l'arrivée d'une trame.
    fn wait_until(&mut self, until: Instant) {
        let now = Instant::now();
        if until > now {
            match &self.nic {
                // Attente écourtée par l'arrivée d'une trame.
                Some((_, link)) => {
                    let mut pfd = libc::pollfd { fd: link.event_fd(), events: libc::POLLIN, revents: 0 };
                    let d = until - now;
                    let ts = libc::timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ };
                    // SAFETY: un pollfd et un délai locaux ; EINTR (battement)
                    // écourte l'attente, comme un réveil.
                    unsafe { libc::ppoll(&mut pfd, 1, &ts, std::ptr::null()) };
                    link.clear_event();
                }
                None => std::thread::sleep(until - now),
            }
        }
    }

    /// Exécute jusqu'au prochain événement (fin du programme ou fin de pas).
    pub fn resume(&mut self) -> Result<Event> {
        loop {
            // Chemin court après une écriture sur un registre de plans (Doom
            // en fait une par colonne de mur, 8 millions par timedemo) : la
            // fenêtre et les IRQ seulement. Le reste (minuteries, son,
            // entrées, affichage) passe au plus tard `FAST_PATH_MAX` après le
            // dernier tour complet ; le battement interrompt de toute façon
            // l'invité à l'échéance de l'IRQ 0.
            let now = Instant::now();
            if std::mem::take(&mut self.fast_next) && now - self.last_full < FAST_PATH_MAX {
                self.update_window()?;
                self.deliver_irq()?;
                self.stats.fast_exits += 1;
            } else {
                self.last_full = now;
                self.loop_top()?;
            }
            self.stats.exits += 1;
            let mut host_call = false;
            let mut glide_call = false;
            let mut mmio = false;
            // Sortie sur un port de périphérique très sollicité (carte réseau) :
            // les accès suivants sont servis en rafale par l'interpréteur.
            let mut port_burst = false;
            // Choix d'un plan de lecture, fenêtre mappée en écriture : la
            // suite passe par l'interpréteur jusqu'à la sortie de cet état.
            let mut plane_read = false;
            match self.vm.run()? {
                Exit::IoOut { port: HOST_CALL_PORT, .. } => host_call = true,
                Exit::IoOut { port: crate::glide::proto::PORT, .. } => glide_call = true,
                Exit::IoOut { port: HOST_KBD_PORT, data, .. } => {
                    let code = data[0];
                    self.bios.scancode(self.vm.mem(), code);
                }
                Exit::IoOut { port, size, data } => {
                    self.stats.io_ports.add(port);
                    // Sauf l'acquittement des interruptions (PIC).
                    if !matches!(port, 0x20 | 0x21 | 0xA0 | 0xA1) {
                        self.vga_polls = None;
                    }
                    self.io.write(port, size, data);
                    // Palette : 768 écritures de suite (Quake en charge une à
                    // presque chaque image), servies en rafale.
                    port_burst = data.len() == usize::from(size) && (self.io.starts_burst(port) || vga_dac_port(port));
                    if matches!(port, 0x60 | 0x64 | 0x92) {
                        self.sync_a20()?;
                        if self.kbc.borrow().reset
                            && let Some(code) = self.reboot()?
                        {
                            return Ok(Event::Exit(code));
                        }
                    }
                    if matches!(port, 0x3D4 | 0x3D5) {
                        self.stats.page_flips = self.vga.borrow().start_writes;
                    }
                    // Registre de plans : la sortie suivante peut prendre le
                    // chemin court (sauf rafale, lancée plus bas).
                    self.fast_next = vga_plane_port(port);
                    // Doom copie sa barre d'état plan par plan : « plan lu =
                    // p » puis aussitôt « plan écrit = p », sans accès entre
                    // les deux. Traversé ici, l'état intermédiaire n'impose
                    // ni lecture seule ni remappage (deux changements de slot).
                    if vga_plane_port(port)
                        && self.window.mapped
                        && !self.window.readonly
                        && matches!(self.vga.borrow().access(), Access::PlaneRead(_))
                    {
                        plane_read = true;
                        port_burst = true;
                    }
                }
                Exit::IoIn { port, size, data } => {
                    self.stats.io_ports.add(port);
                    if matches!(port, 0x40..=0x42) && Self::pit_poll(&mut self.pit_polls) {
                        self.stats.pit_yields += 1;
                    }
                    if matches!(port, 0x3BA | 0x3DA)
                        && Self::vga_poll(&mut self.vga_polls, &self.vga.borrow(), &self.pit.borrow(), port)
                    {
                        self.stats.vga_yields += 1;
                    }
                    self.io.read(port, size, data);
                    port_burst = data.len() == usize::from(size) && self.io.starts_burst(port);
                }
                Exit::MmioRead { addr, data } => {
                    let mut vga = self.vga.borrow_mut();
                    let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0, io: None, port_accesses: 0, plane_read: false };
                    for (i, b) in data.iter_mut().enumerate() {
                        let a = addr as u32 + i as u32;
                        *b = if bus.claims(a, false) { bus.read8(a) } else { 0xFF };
                    }
                    self.window.emulated_in_epoch += bus.accesses;
                    self.window.reads_in_epoch += bus.reads;
                    self.window.writes_since_read += bus.accesses - bus.reads;
                    self.stats.vram_emulated += bus.accesses;
                    mmio = true;
                }
                Exit::MmioWrite { addr, data } => {
                    let mut vga = self.vga.borrow_mut();
                    let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0, io: None, port_accesses: 0, plane_read: false };
                    for (i, &b) in data.iter().enumerate() {
                        bus.write8(addr as u32 + i as u32, b);
                    }
                    self.window.emulated_in_epoch += bus.accesses;
                    self.window.reads_in_epoch += bus.reads;
                    self.window.writes_since_read += bus.accesses - bus.reads;
                    self.stats.vram_emulated += bus.accesses;
                    // Écriture en ROM : ignorée, pas de rafale.
                    mmio = VGA_WINDOW.contains(&(addr as usize));
                }
                // Signal (battement, débordement du compteur) : le bridage
                // est servi en tête de boucle.
                Exit::Interrupted => self.spin_check()?,
                Exit::IrqWindow => {}
                Exit::Step => return Ok(Event::Step),
                Exit::Hlt => self.halt()?,
                Exit::Shutdown if !self.dos_hle => {
                    self.vm.reset_cpu()?;
                    self.reboot()?;
                }
                Exit::Shutdown => return Err("processor shutdown (triple fault?)".into()),
                Exit::Other(e) => return Err(format!("unexpected VM exit: {e}").into()),
            }
            if mmio {
                self.stats.mmio_exits += 1;
            }
            if (mmio || port_burst) && self.interp {
                let mut vga = self.vga.borrow_mut();
                let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0, io: Some(&mut self.io), port_accesses: 0, plane_read };
                let burst = interp::continue_in_host(&mut self.vm, &mut bus, IDLE_LIMIT, BURST_BUDGET)?;
                self.stats.burst_ports += bus.port_accesses;
                self.window.emulated_in_epoch += bus.accesses;
                self.window.reads_in_epoch += bus.reads;
                self.window.writes_since_read += bus.accesses - bus.reads;
                self.stats.vram_emulated += bus.accesses;
                self.stats.interpreted += burst.instructions;
                if let Some(t) = &mut self.throttle {
                    t.credit(burst.instructions);
                }
            }
            if glide_call {
                self.glide_call()?;
            }
            if host_call && let Some(code) = self.host_call()? {
                return Ok(Event::Exit(code));
            }
        }
    }

    /// Tour complet du haut de la boucle : limite de temps, minuteries,
    /// fenêtre, entrées, affichage, bridage, IRQ.
    fn loop_top(&mut self) -> Result<()> {
        if self.limit.is_some_and(|l| self.started.elapsed() >= l) {
            let cpu = self.vm.state()?;
            let code = crate::debug::disassemble(self.vm.mem(), cpu.seg[1].selector, cpu.ip as u16, 4);
            let screen = bios::screen_text(self.vm.mem()).join("\n");
            return Err(format!(
                "time limit reached\n{}\n{}\n{:?}\nscreen:\n{}",
                crate::debug::format_regs(&cpu),
                code.join("\n"),
                self.stats,
                screen.trim_end()
            )
            .into());
        }
        self.service_timers()?;
        self.update_window()?;
        self.service_input();
        self.render_term(false);
        if self.host.wants_frames {
            self.present_frame();
        }
        if self.input_finished() {
            return Err("end of input: session stopped".into());
        }
        if let Some(t) = &mut self.throttle
            && t.take_overflow()
        {
            t.pace()?;
        }
        self.deliver_irq()?;
        Ok(())
    }

    /// Sortie sur le port d'appel hôte. Vrai appel seulement depuis un stub
    /// de la ROM ; sinon le programme a simplement écrit sur le port E0h.
    /// Appel de la `glide2x.dll` de Bare86 (`glide::proto`) : EBX = tampon de
    /// commandes (dans DS), ECX = sa longueur ; EAX = valeur rendue.
    fn glide_call(&mut self) -> Result<()> {
        let mut cpu = self.vm.state()?;
        let ds = cpu.seg[3].base;
        let len = (cpu.gpr[1] as usize).min(16 << 20) & !3;
        let mut bytes = vec![0u8; len];
        let mem = self.vm.mem();
        let ret = if interp::read_linear(&cpu, mem, ds.wrapping_add(cpu.gpr[3]), &mut bytes) {
            let words: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            let mut glide = self.glide.borrow_mut();
            let g = glide.get_or_insert_with(|| Box::new(crate::glide::Glide::new()));
            let mut read = |at: u32, buf: &mut [u8]| {
                interp::read_linear(&cpu, mem, ds.wrapping_add(at), buf);
            };
            let mut write = |at: u32, data: &[u8]| {
                interp::write_linear(&cpu, mem, ds.wrapping_add(at), data);
            };
            g.execute(&words, &mut read, &mut write)
        } else {
            0
        };
        cpu.gpr[0] = ret;
        self.vm.set_state(&cpu)?;
        Ok(())
    }

    fn host_call(&mut self) -> Result<Option<u8>> {
        let mut cpu = self.vm.state()?;
        let xms_entry = (u32::from(bios::XMS_ENTRY)..u32::from(bios::XMS_ENTRY) + 16).contains(&cpu.ip);
        let ps2_fetch = (u32::from(bios::PS2_FETCH)..u32::from(bios::PS2_FETCH) + 3).contains(&cpu.ip);
        if cpu.seg[1].selector != ROM_SEG || (cpu.ip >= 0x1000 && !xms_entry && !ps2_fetch) {
            return Ok(None);
        }
        if ps2_fetch {
            let mut cpu = cpu;
            let (ax, (bx, cx, dx)) = match self.ps2.packet() {
                Some(p) => (1, p),
                None => (0, (0, 0, 0)),
            };
            cpu.set_w(AX, ax);
            cpu.set_w(crate::hle::BX, bx);
            cpu.set_w(crate::hle::CX, cx);
            cpu.set_w(crate::hle::DX, dx);
            self.vm.set_state(&cpu)?;
            return Ok(None);
        }
        if xms_entry {
            self.stats.host_calls += 1;
            let mut cpu = cpu;
            let mut a20 = self.kbc.borrow().a20();
            self.xms.call(&mut cpu, self.vm.mem(), &mut a20);
            self.kbc.borrow_mut().set_a20(a20);
            self.sync_a20()?;
            self.vm.set_state(&cpu)?;
            return Ok(None);
        }
        self.stats.host_calls += 1;
        let vector = ((cpu.ip & 0xFFF) >> 4) as u8;
        if vector == 0x19 {
            return self.reboot();
        }
        if self.trace_int {
            let mem = self.vm.mem();
            let sp = cpu.linear(2, cpu.gpr[4] & 0xFFFF) as usize;
            let (ip, cs) = (mem.read_u16(sp), mem.read_u16(sp + 2));
            eprintln!(
                "[int] {vector:02X}h AX={:04X} BX={:04X} CX={:04X} DX={:04X} ES={:04X} from {cs:04X}:{ip:04X}",
                cpu.gpr[0] & 0xFFFF,
                cpu.gpr[3] & 0xFFFF,
                cpu.gpr[1] & 0xFFFF,
                cpu.gpr[2] & 0xFFFF,
                cpu.seg[0].selector
            );
            // Nom de fichier en DS:DX des fonctions DOS qui en prennent un.
            let ah = (cpu.gpr[0] >> 8) as u8;
            if vector == 0x21 && matches!(ah, 0x39..=0x3D | 0x41 | 0x43 | 0x4B | 0x4E | 0x56 | 0x5A | 0x5B | 0x6C) {
                let mem = self.vm.mem();
                let at = cpu.linear(3, if ah == 0x6C { cpu.gpr[6] } else { cpu.gpr[2] } & 0xFFFF) as usize;
                let name: Vec<u8> = (0..128).map(|i| mem.read_u8(at + i)).take_while(|&b| b != 0).collect();
                eprintln!("      « {} »", String::from_utf8_lossy(&name));
                if ah == 0x4B {
                    // Ligne de commande : bloc de paramètres en ES:BX, pointeur en +2.
                    let block = cpu.linear(0, cpu.gpr[3] & 0xFFFF) as usize;
                    let tail = (usize::from(mem.read_u16(block + 4)) << 4) + usize::from(mem.read_u16(block + 2));
                    let len = usize::from(mem.read_u8(tail)).min(126);
                    let t: Vec<u8> = (0..len).map(|i| mem.read_u8(tail + 1 + i)).collect();
                    eprintln!("      line \"{}\"", String::from_utf8_lossy(&t));
                }
            }
        }
        let entry = cpu.clone();
        self.waiting_key = false;
        let ah = (cpu.gpr[0] >> 8) as u8;
        match (vector, ah) {
            (0x16, 0x01 | 0x11) => self.idle_poll(),
            // Compatibles avec une boucle d'attente : curseur, horloge, veille.
            (0x10, 0x02 | 0x03) | (0x1A, 0x00) | (0x28, _) | (0x2F, 0x16) => {}
            // Autre travail (disque, texte…) : ce n'est pas une attente. Le
            // noyau FreeDOS scrute le clavier (Ctrl-C) à chaque appel DOS.
            _ => self.idle_polls = None,
        }
        let mem = self.vm.mem();
        if self.drained_at.is_some() && matches!((vector, ah), (0x16, 0x01 | 0x11) | (0x21, 0x06 | 0x0B)) {
            self.polled_after_drain = true;
        }
        let driver = match vector {
            0x2F if self.dos_hle => match self.xms.int2f(&mut cpu, (ROM_SEG, bios::XMS_ENTRY)) {
                Some(f) => Some(f),
                None => self.dos.int2f(&mut cpu, mem),
            },
            0x67 if self.dos_hle => Some(self.ems.int67(&mut cpu, mem)),
            // Pas à pas sans débogueur dans l'invité (TF resté empilé après
            // notre propre pas à pas) : TF effacé au retour.
            0x01 => {
                set_ret_flag(&cpu, mem, 1 << 8, false);
                Some(Flow::Continue)
            }
            // Joystick du BIOS : boutons (DX=0) ou positions (DX=1).
            0x15 if ah == 0x84 => {
                let g = self.gameport.borrow();
                match cpu.w(crate::hle::DX) {
                    0 => cpu.set_lo(AX, g.bios_buttons()),
                    _ => {
                        let [a, b, c, d] = g.bios_positions();
                        cpu.set_w(AX, a);
                        cpu.set_w(crate::hle::BX, b);
                        cpu.set_w(crate::hle::CX, c);
                        cpu.set_w(crate::hle::DX, d);
                    }
                }
                set_ret_flag(&cpu, mem, CF, false);
                Some(Flow::Continue)
            }
            // Souris PS/2 du BIOS (sous un vrai DOS ; le DOS de l'hôte a son INT 33h).
            0x15 if ah == 0xC2 && !self.dos_hle => {
                let r = ps2_bios(&mut self.ps2, &self.pic, &mut cpu, mem);
                cpu.set_hi(AX, r);
                set_ret_flag(&cpu, mem, CF, r != 0);
                Some(Flow::Continue)
            }
            0x15 if ah == 0x24 && cpu.lo(AX) <= 2 => {
                let mut kbc = self.kbc.borrow_mut();
                match cpu.lo(AX) {
                    2 => cpu.set_lo(AX, u8::from(kbc.a20())),
                    al => kbc.set_a20(al == 1),
                }
                cpu.set_hi(AX, 0);
                set_ret_flag(&cpu, mem, CF, false);
                Some(Flow::Continue)
            }
            _ => None,
        };
        let flow = match driver {
            Some(flow) => flow,
            None => match if self.dos_hle { self.dos.call(vector, &mut cpu, mem, &mut self.host)? } else { None } {
                Some(flow) => flow,
                None => self.bios.call(vector, &mut cpu, mem, &mut self.host)?,
            },
        };
        match flow {
            Flow::Exit(code) => return Ok(Some(code)),
            // Le service a pu poser un code d'erreur (AH, CF) : on le transmet.
            Flow::Unhandled(ah) => {
                if self.unhandled.insert((vector, ah)) {
                    eprintln!("[int] INT {vector:02X}h AH={ah:02X}h not handled");
                }
            }
            Flow::Continue => {}
            // Reprise après HLT : registres d'entrée, IP sur le tremplin du vecteur.
            Flow::Wait => {
                cpu = entry;
                cpu.ip = u32::from(if vector == 0x16 { bios::WAIT16 } else { bios::WAIT21 });
                self.waiting_key = true;
                self.vm.set_state(&cpu)?;
                return Ok(None);
            }
        }
        if self.trace_int {
            let mem = self.vm.mem();
            let flags = mem.read_u16(cpu.linear(2, (cpu.gpr[4] + 4) & 0xFFFF) as usize);
            eprintln!(
                "      → AX={:04X} BX={:04X} CX={:04X} DX={:04X} ES={:04X} CF={}",
                cpu.gpr[0] & 0xFFFF,
                cpu.gpr[3] & 0xFFFF,
                cpu.gpr[1] & 0xFFFF,
                cpu.gpr[2] & 0xFFFF,
                cpu.seg[0].selector,
                flags & 1
            );
        }
        self.sync_a20()?;
        self.vm.set_state(&cpu)?;
        Ok(None)
    }

    /// Attente active sur le compteur du PIT (Quake entre deux images, à
    /// 72 i/s : ≈ 70 000 lectures par seconde, trois sorties chacune) : au-delà
    /// de `PIT_POLL_AFTER` de lectures rapprochées, chaque lecture cède
    /// `PIT_POLL_SLEEP` à l'hôte. Le compteur suit le temps réel : le
    /// programme voit simplement le temps passer. Les temporisations courtes
    /// (pilotes, détection AdLib) ne sont pas touchées.
    fn pit_poll(polls: &mut Option<(Instant, Instant)>) -> bool {
        let now = Instant::now();
        let start = match *polls {
            Some((start, last)) if now - last < PIT_POLL_GAP => start,
            _ => now,
        };
        let yields = now - start > PIT_POLL_AFTER;
        if yields {
            std::thread::sleep(PIT_POLL_SLEEP);
        }
        *polls = Some((start, Instant::now()));
        yields
    }

    /// Attente du blanc vertical par scrutation de 3DAh (Keen, Wolfenstein :
    /// la routine d'id suit le bit 0 ligne à ligne, ≈ 47 000 lectures par
    /// seconde). Au-delà de `PIT_POLL_AFTER` de lectures rapprochées sans
    /// écriture sur un autre port que le PIC (un effet de raster écrirait la
    /// palette ou un registre entre deux attentes), le vCPU dort jusqu'à
    /// `VBLANK_MARGIN` avant le prochain blanc vertical, sans dépasser la
    /// prochaine IRQ 0 : le programme voit des échantillons plus espacés
    /// d'un signal exact.
    fn vga_poll(polls: &mut Option<(Instant, Instant)>, vga: &Vga, pit: &crate::pit::Pit, port: u16) -> bool {
        let now = Instant::now();
        if !vga.is_status_port(port) {
            return false;
        }
        let start = match *polls {
            Some((start, last)) if now - last < PIT_POLL_GAP => start,
            _ => now,
        };
        let mut slept = false;
        if now - start > PIT_POLL_AFTER {
            let mut d = vga.until_vblank().saturating_sub(VBLANK_MARGIN);
            if let Some(irq) = pit.next_irq0() {
                d = d.min(irq.saturating_duration_since(now));
            }
            if d >= PIT_POLL_SLEEP {
                std::thread::sleep(d);
                slept = true;
            }
        }
        *polls = Some((start, Instant::now()));
        slept
    }

    /// Boucle d'attente d'une touche par scrutation (FreeCOM à l'invite) :
    /// au-delà de 20 ms d'appels rapprochés (moins de 200 µs entre deux) sans
    /// touche ni autre service entre eux, chaque appel cède 1 ms à l'hôte au
    /// lieu d'occuper un cœur. Un jeu qui scrute une fois par image n'est pas
    /// concerné.
    fn idle_poll(&mut self) {
        let now = Instant::now();
        if !self.keys.is_empty() || bios::peek_key(self.vm.mem()).is_some() {
            self.idle_polls = None;
            return;
        }
        let start = match self.idle_polls {
            Some((start, last)) if now - last < Duration::from_micros(200) => start,
            _ => now,
        };
        if now - start > Duration::from_millis(20) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.idle_polls = Some((start, Instant::now()));
    }

    /// Services non gérés rencontrés (vecteur, AH).
    #[cfg(test)]
    pub fn unhandled(&self) -> &BTreeSet<(u8, u8)> {
        &self.unhandled
    }

    /// Image de l'écran : mode graphique, ou texte si une police est chargée.
    pub fn frame(&self) -> Option<(usize, usize, Vec<u32>)> {
        // Glide ouvert : son image remplace celle de la VGA (relais de la Voodoo).
        if let Some(g) = self.glide.borrow_mut().as_mut()
            && let Some(f) = g.frame()
        {
            return Some(f);
        }
        frame_of(&self.vga.borrow(), self.vm.mem(), self.font.as_ref(), self.started)
    }

    /// Police du mode texte (rendu en image : fenêtre, captures).
    pub fn set_font(&mut self, font: crate::font::Font) {
        self.font = Some(font);
    }

    /// Image envoyée à l'affichage de l'hôte, à 70 Hz.
    fn present_frame(&mut self) {
        if self.last_frame.is_some_and(|t| t.elapsed() < FRAME_PERIOD) {
            return;
        }
        self.last_frame = Some(Instant::now());
        if let Some((width, height, pixels)) = self.frame() {
            self.host.display.present(&crate::host::Frame { width, height, pixels: &pixels });
        }
    }

    /// Texte de la page active de l'écran.
    pub fn screen_text(&self) -> Vec<String> {
        bios::screen_text(self.vm.mem())
    }

    /// Octet de la fenêtre graphique : tampon linéaire en chain4, plan 0 sinon (tests).
    #[cfg(test)]
    pub fn vga_byte(&self, off: usize) -> u8 {
        let v = self.vga.borrow();
        if v.chain4() { v.linear_byte(off) } else { v.plane_byte(0, off) }
    }

    #[cfg(test)]
    pub fn vga(&self) -> std::cell::Ref<'_, Vga> {
        self.vga.borrow()
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
    }
}

/// INT 15h AH=C2h (souris PS/2) : AL = fonction ; rend le statut (AH).
fn ps2_bios(ps2: &mut crate::mouse::Ps2, pic: &RefCell<Pic>, cpu: &mut crate::interp::Cpu, mem: &crate::mem::GuestMem) -> u8 {
    use crate::hle::{BX, CX, DX, ES};
    match cpu.lo(AX) {
        // Activation (BH=1) ou arrêt : il faut une routine.
        0x00 => {
            let on = cpu.hi(BX) == 1;
            if on && !ps2.handler {
                return 0x05;
            }
            ps2.enabled = on;
            if on {
                pic.borrow_mut().unmask(12);
            }
            0
        }
        // Réinitialisation : identifiant 00h, arrêtée.
        0x01 => {
            ps2.enabled = false;
            cpu.set_w(BX, 0x00AA);
            0
        }
        // Fréquence, résolution, échelle : acceptées.
        0x02 | 0x03 => 0,
        0x04 => {
            cpu.set_hi(BX, 0);
            0
        }
        // Initialisation (BH = taille du paquet) : 3 octets seulement.
        0x05 => {
            ps2.enabled = false;
            if cpu.hi(BX) == 3 { 0 } else { 0x02 }
        }
        0x06 => {
            if cpu.hi(BX) == 0 {
                // État : activée, résolution 4 points/mm, 100 échantillons/s.
                cpu.set_lo(BX, if ps2.enabled { 0x20 } else { 0 });
                cpu.set_lo(CX, 2);
                cpu.set_lo(DX, 100);
            }
            0
        }
        // Routine du pilote en ES:BX (0:0 : retirée).
        0x07 => {
            let (seg, off) = (cpu.sreg(ES), cpu.w(BX));
            mem.write_u16(bios::PS2_HANDLER, off);
            mem.write_u16(bios::PS2_HANDLER + 2, seg);
            ps2.handler = seg != 0 || off != 0;
            if !ps2.handler {
                ps2.enabled = false;
            }
            0
        }
        _ => 0x01,
    }
}
