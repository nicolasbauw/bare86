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
use crate::hle::Flow;
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
    /// Durée maximale d'exécution (diagnostic : registres affichés à l'arrêt).
    pub limit: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self { extended_kb: 15 * 1024, ems_kb: 4096, mips: None, trace_int: false, trace_io: false, limit: None }
    }
}

/// Accès MMIO vus par l'interpréteur : fenêtre VGA (logique des plans),
/// écritures en ROM (ignorées).
struct MmioBus<'a> {
    vga: &'a mut Vga,
    accesses: u64,
    reads: u64,
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
}

/// Suivi du mappage de la fenêtre A0000 (FAISABILITE, « décision sur la
/// mémoire vidéo ») : linéaire tout de suite, plan unique après
/// `MAP_THRESHOLD` accès émulés sans changement de configuration, puis
/// maintenu d'un plan à l'autre : un changement de plan ne coûte que deux
/// copies de 64 Ko (scène de `Vram`), moins qu'une sortie MMIO.
#[derive(Default)]
struct WindowState {
    mapped: bool,
    epoch: u64,
    emulated_in_epoch: u64,
    /// Lectures émulées dans cette configuration (plan de lecture ≠ plan écrit).
    reads_in_epoch: u64,
}

const MAP_THRESHOLD: u64 = 600;

#[derive(Default, Debug)]
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
    font: Option<crate::font::Font>,
    started: Instant,
    limit: Option<Duration>,
    last_frame: Option<Instant>,
    /// Dernier rafraîchissement de l'écran du terminal.
    last_render: Option<Instant>,
    /// Pas à pas en cours : pas d'interruption injectée entre deux pas.
    stepping: bool,
    trace_int: bool,
    /// Services non gérés déjà signalés (vecteur, AH).
    unhandled: BTreeSet<(u8, u8)>,
}

impl Pc<KvmVm> {
    pub fn new(cfg: &Config, host: Host) -> Result<Self> {
        let size = 0x10_0000 + cfg.extended_kb * 1024;
        let r = |start, end, readonly| Region { start, end, readonly };
        let layout = [
            r(0, VGA_WINDOW.start, false),
            r(VGA_WINDOW.end, ROM.start, false),
            r(ROM.start, ROM.end, true),
            r(ROM.end, size, false),
        ];
        let vm = KvmVm::with_layout(size, &layout)?;
        Self::with_backend(vm, cfg, host)
    }
}

impl<B: Backend> Pc<B> {
    pub fn with_backend(vm: B, cfg: &Config, host: Host) -> Result<Self> {
        let vga = Rc::new(RefCell::new(Vga::new()?));
        let mut bios = Bios::new(cfg.extended_kb, vga.clone());
        bios.install(vm.mem());
        let mut dos = dos::Dos::new();
        dos.boot(vm.mem());
        let throttle = cfg.mips.map(Throttle::new).transpose()?;
        let mut io = IoBus::default();
        io.trace = cfg.trace_io;
        // Port de diagnostic POST, utilisé aussi comme temporisation.
        io.map(0x80..=0x80, Box::new(crate::io::Sink));
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
        Ok(Self {
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
            ems: crate::ems::Ems::new(cfg.ems_kb),
            keys: VecDeque::new(),
            last_scancode: None,
            last_made: None,
            waiting_key: false,
            drained_at: None,
            polled_after_drain: false,
            last_render: None,
            font: None,
            started: Instant::now(),
            limit: cfg.limit,
            last_frame: None,
            input_paused_until: None,
            stepping: false,
            trace_int: cfg.trace_int,
            unhandled: BTreeSet::new(),
        })
    }

    /// Monte un répertoire hôte comme lecteur (0 = A:).
    pub fn mount(&mut self, drive: u8, root: std::path::PathBuf) {
        self.dos.mount(drive, root);
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

    /// Charge un programme d'un lecteur monté (« C:\\JEU.EXE »).
    pub fn load_program(&mut self, path: &[u8], tail: &[u8]) -> Result<()> {
        let cpu = self.dos.start(self.vm.mem(), path, tail)?;
        self.vm.set_state(&cpu)
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

    /// Mappe, remappe ou démappe la fenêtre A0000 selon la configuration VGA.
    fn update_window(&mut self) -> Result<()> {
        let mut vga = self.vga.borrow_mut();
        let w = &mut self.window;
        if vga.epoch != w.epoch {
            w.epoch = vga.epoch;
            w.emulated_in_epoch = 0;
            w.reads_in_epoch = 0;
        }
        let access = vga.access();
        let want = match access {
            Access::Emulated => None,
            Access::Linear => Some(access),
            // Lectures vues alors qu'elles viseraient un autre plan que celui mappé.
            Access::Plane(p) if p != vga.read_plane() && w.reads_in_epoch > 0 => None,
            Access::Plane(_) if w.mapped || w.emulated_in_epoch >= MAP_THRESHOLD => Some(access),
            Access::Plane(_) => None,
        };
        match want {
            Some(a) => {
                vga.point_window(a)?;
                if !w.mapped {
                    self.vm.map_vga(Some(vga.window_host_addr()))?;
                    w.mapped = true;
                    self.stats.vram_maps += 1;
                }
            }
            None if w.mapped => {
                self.vm.map_vga(None)?;
                w.mapped = false;
                self.stats.vram_unmaps += 1;
            }
            None => {}
        }
        Ok(())
    }

    /// Minuterie : IRQ 0 échue, cadence du battement.
    fn service_timers(&mut self) -> Result<()> {
        let mut pit = self.pit.borrow_mut();
        if pit.take_irq0(Instant::now()) {
            self.pic.borrow_mut().raise(0);
        }
        self.heartbeat.set_period(heartbeat_period(pit.irq0_period()))?;
        if let Some(sp) = &self.host.speaker {
            let f = pit.speaker().map_or(0, |f| (f * 100.0) as u32);
            sp.store(f, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    /// Entrées de l'hôte → scancodes → 8042 → IRQ 1. Une frappe à la fois :
    /// un octet par intervalle, le tampon du 8042 vide, et l'appui sur une
    /// touche (hors modificateurs) attend que le programme ait lu la
    /// précédente dans la BDA, sauf s'il a détourné l'INT 09h. Les boucles
    /// d'événements (DFlat) gardent ainsi l'ordre des raccourcis et du texte,
    /// et lisent l'état des modificateurs de la frappe (AH=02h).
    fn service_input(&mut self) {
        let paused = self.input_paused_until.is_some_and(|t| Instant::now() < t);
        while self.keys.is_empty() && !self.host.quit && !paused {
            match self.host.input.poll() {
                Some(InputEvent::Wait(ms)) => {
                    self.input_paused_until = Some(Instant::now() + Duration::from_millis(u64::from(ms)));
                    break;
                }
                Some(InputEvent::Quit) => self.host.quit = true,
                Some(InputEvent::Key(code)) => self.keys.push_back(code),
                Some(InputEvent::Text(c)) => self.keys.extend(keyboard::text_scancodes(c)),
                Some(InputEvent::Mouse { dx, dy, buttons }) => self.bios.mouse.event(self.vm.mem(), dx, dy, buttons),
                None => break,
            }
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
            return Err("HLT avec les interruptions masquées (IF = 0)".into());
        }
        let now = Instant::now();
        let until = self.pit.borrow().next_irq0().unwrap_or(now + HALT_MAX).min(now + HALT_MAX);
        if until > now {
            std::thread::sleep(until - now);
        }
        Ok(())
    }

    /// Exécute jusqu'au prochain événement (fin du programme ou fin de pas).
    pub fn resume(&mut self) -> Result<Event> {
        loop {
            if self.limit.is_some_and(|l| self.started.elapsed() >= l) {
                let cpu = self.vm.state()?;
                let code = crate::debug::disassemble(self.vm.mem(), cpu.seg[1].selector, cpu.ip as u16, 4);
                return Err(format!(
                    "limite de temps atteinte\n{}\n{}\n{:?}",
                    crate::debug::format_regs(&cpu),
                    code.join("\n"),
                    self.stats
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
                return Err("fin de l'entrée : session arrêtée".into());
            }
            self.deliver_irq()?;
            self.stats.exits += 1;
            let mut host_call = false;
            let mut mmio = false;
            match self.vm.run()? {
                Exit::IoOut { port: HOST_CALL_PORT, .. } => host_call = true,
                Exit::IoOut { port: HOST_KBD_PORT, data } => {
                    let code = data[0];
                    self.bios.scancode(self.vm.mem(), code);
                }
                Exit::IoOut { port, data } => self.io.write(port, data),
                Exit::IoIn { port, data } => self.io.read(port, data),
                Exit::MmioRead { addr, data } => {
                    let mut vga = self.vga.borrow_mut();
                    let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0 };
                    for (i, b) in data.iter_mut().enumerate() {
                        let a = addr as u32 + i as u32;
                        *b = if bus.claims(a, false) { bus.read8(a) } else { 0xFF };
                    }
                    self.window.emulated_in_epoch += bus.accesses;
                    self.window.reads_in_epoch += bus.reads;
                    self.stats.vram_emulated += bus.accesses;
                    mmio = true;
                }
                Exit::MmioWrite { addr, data } => {
                    let mut vga = self.vga.borrow_mut();
                    let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0 };
                    for (i, &b) in data.iter().enumerate() {
                        bus.write8(addr as u32 + i as u32, b);
                    }
                    self.window.emulated_in_epoch += bus.accesses;
                    self.window.reads_in_epoch += bus.reads;
                    self.stats.vram_emulated += bus.accesses;
                    // Écriture en ROM : ignorée, pas de rafale.
                    mmio = VGA_WINDOW.contains(&(addr as usize));
                }
                Exit::Interrupted => {
                    if let Some(t) = &mut self.throttle {
                        t.pace()?;
                    }
                }
                Exit::IrqWindow => {}
                Exit::Step => return Ok(Event::Step),
                Exit::Hlt => self.halt()?,
                Exit::Shutdown => return Err("arrêt du processeur (triple faute ?)".into()),
                Exit::Other(e) => return Err(format!("sortie de VM inattendue : {e}").into()),
            }
            if mmio {
                self.stats.mmio_exits += 1;
                let mut vga = self.vga.borrow_mut();
                let mut bus = MmioBus { vga: &mut vga, accesses: 0, reads: 0 };
                let burst = interp::continue_in_host(&mut self.vm, &mut bus, IDLE_LIMIT, BURST_BUDGET)?;
                self.window.emulated_in_epoch += bus.accesses;
                self.window.reads_in_epoch += bus.reads;
                self.stats.vram_emulated += bus.accesses;
                self.stats.interpreted += burst.instructions;
                if let Some(t) = &mut self.throttle {
                    t.credit(burst.instructions);
                }
            }
            if host_call && let Some(code) = self.host_call()? {
                return Ok(Event::Exit(code));
            }
        }
    }

    /// Sortie sur le port d'appel hôte. Vrai appel seulement depuis un stub
    /// de la ROM ; sinon le programme a simplement écrit sur le port E0h.
    fn host_call(&mut self) -> Result<Option<u8>> {
        let mut cpu = self.vm.state()?;
        let xms_entry = (u32::from(bios::XMS_ENTRY)..u32::from(bios::XMS_ENTRY) + 16).contains(&cpu.ip);
        if cpu.seg[1].selector != ROM_SEG || (cpu.ip >= 0x1000 && !xms_entry) {
            return Ok(None);
        }
        if xms_entry {
            self.stats.host_calls += 1;
            let mut cpu = cpu;
            self.xms.call(&mut cpu, self.vm.mem());
            self.vm.set_state(&cpu)?;
            return Ok(None);
        }
        self.stats.host_calls += 1;
        let vector = ((cpu.ip & 0xFFF) >> 4) as u8;
        if self.trace_int {
            let mem = self.vm.mem();
            let sp = cpu.linear(2, cpu.gpr[4] & 0xFFFF) as usize;
            let (ip, cs) = (mem.read_u16(sp), mem.read_u16(sp + 2));
            eprintln!(
                "[int] {vector:02X}h AX={:04X} BX={:04X} CX={:04X} DX={:04X} ES={:04X} depuis {cs:04X}:{ip:04X}",
                cpu.gpr[0] & 0xFFFF,
                cpu.gpr[3] & 0xFFFF,
                cpu.gpr[1] & 0xFFFF,
                cpu.gpr[2] & 0xFFFF,
                cpu.seg[0].selector
            );
        }
        let mem = self.vm.mem();
        let entry = cpu.clone();
        self.waiting_key = false;
        let ah = (cpu.gpr[0] >> 8) as u8;
        if self.drained_at.is_some() && matches!((vector, ah), (0x16, 0x01 | 0x11) | (0x21, 0x06 | 0x0B)) {
            self.polled_after_drain = true;
        }
        let driver = match vector {
            0x2F => self.xms.int2f(&mut cpu, (ROM_SEG, bios::XMS_ENTRY)),
            0x67 => Some(self.ems.int67(&mut cpu, mem)),
            _ => None,
        };
        let flow = match driver {
            Some(flow) => flow,
            None => match self.dos.call(vector, &mut cpu, mem, &mut self.host)? {
                Some(flow) => flow,
                None => self.bios.call(vector, &mut cpu, mem, &mut self.host)?,
            },
        };
        match flow {
            Flow::Exit(code) => return Ok(Some(code)),
            // Le service a pu poser un code d'erreur (AH, CF) : on le transmet.
            Flow::Unhandled(ah) => {
                if self.unhandled.insert((vector, ah)) {
                    eprintln!("[int] INT {vector:02X}h AH={ah:02X}h non géré");
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
        self.vm.set_state(&cpu)?;
        Ok(None)
    }

    /// Services non gérés rencontrés (vecteur, AH).
    #[cfg(test)]
    pub fn unhandled(&self) -> &BTreeSet<(u8, u8)> {
        &self.unhandled
    }

    /// Image de l'écran : mode graphique, ou texte si une police est chargée.
    pub fn frame(&self) -> Option<(usize, usize, Vec<u32>)> {
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
