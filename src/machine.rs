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

use std::collections::BTreeSet;

use crate::backend::{Backend, Error, Exit, Result};
use crate::bios::{self, Bios, ROM, ROM_SEG};
use crate::dos;
use crate::hle::Flow;
use crate::host::Host;
use crate::interp::{self, Mmio};
use crate::io::IoBus;
use crate::kvm::{KvmVm, Region, VGA_WINDOW};
use crate::throttle::Throttle;

/// Port d'appel hôte utilisé par les stubs.
pub const HOST_CALL_PORT: u16 = 0xE0;

/// Instructions sans accès VRAM avant que l'interpréteur rende la main.
const IDLE_LIMIT: u64 = 256;
/// Plafond d'instructions par rafale interprétée.
const BURST_BUDGET: u64 = 100_000;

pub struct Config {
    /// Mémoire étendue au-delà du premier Mo, en Ko.
    pub extended_kb: usize,
    /// Débit visé en MIPS ; `None` : vitesse native.
    pub mips: Option<f64>,
    /// Journal des interruptions servies par l'hôte.
    pub trace_int: bool,
    /// Ports d'E/S sans périphérique signalés.
    pub trace_io: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { extended_kb: 15 * 1024, mips: None, trace_int: false, trace_io: false }
    }
}

/// Fenêtre VGA provisoire : 64 Ko linéaires, en attendant l'étape 5.
pub struct VgaWindow(Vec<u8>);

/// Accès MMIO vus par l'interpréteur : fenêtre VGA, écritures en ROM.
struct MmioBus<'a> {
    vga: &'a mut VgaWindow,
}

impl Mmio for MmioBus<'_> {
    fn claims(&self, addr: u32, write: bool) -> bool {
        let a = addr as usize;
        VGA_WINDOW.contains(&a) || (write && ROM.contains(&a))
    }
    fn read8(&mut self, addr: u32) -> u8 {
        self.vga.0[addr as usize - VGA_WINDOW.start]
    }
    fn write8(&mut self, addr: u32, v: u8) {
        if VGA_WINDOW.contains(&(addr as usize)) {
            self.vga.0[addr as usize - VGA_WINDOW.start] = v;
        }
    }
}

#[derive(Default, Debug)]
pub struct Stats {
    pub exits: u64,
    pub mmio_exits: u64,
    pub interpreted: u64,
    pub host_calls: u64,
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
    vga: VgaWindow,
    throttle: Option<Throttle>,
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
        let mut bios = Bios::new(cfg.extended_kb);
        bios.install(vm.mem());
        let mut dos = dos::Dos::new();
        dos.boot(vm.mem());
        let throttle = cfg.mips.map(Throttle::new).transpose()?;
        let mut io = IoBus::default();
        io.trace = cfg.trace_io;
        // Port de diagnostic POST, utilisé aussi comme temporisation.
        io.map(0x80..=0x80, Box::new(crate::io::Sink));
        Ok(Self {
            vm,
            io,
            host,
            stats: Stats::default(),
            bios,
            dos,
            vga: VgaWindow(vec![0; VGA_WINDOW.len()]),
            throttle,
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
            let Some(line) = self.dos.shell_read(mem, &mut self.host) else { return Ok(code) };
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

    /// Exécute jusqu'au prochain événement (fin du programme ou fin de pas).
    pub fn resume(&mut self) -> Result<Event> {
        loop {
            self.stats.exits += 1;
            let mut host_call = false;
            let mut mmio = false;
            match self.vm.run()? {
                Exit::IoOut { port: HOST_CALL_PORT, .. } => host_call = true,
                Exit::IoOut { port, data } => self.io.write(port, data),
                Exit::IoIn { port, data } => self.io.read(port, data),
                Exit::MmioRead { addr, data } => {
                    let mut bus = MmioBus { vga: &mut self.vga };
                    for (i, b) in data.iter_mut().enumerate() {
                        let a = addr as u32 + i as u32;
                        *b = if bus.claims(a, false) { bus.read8(a) } else { 0xFF };
                    }
                    mmio = true;
                }
                Exit::MmioWrite { addr, data } => {
                    let mut bus = MmioBus { vga: &mut self.vga };
                    for (i, &b) in data.iter().enumerate() {
                        bus.write8(addr as u32 + i as u32, b);
                    }
                    // Écriture en ROM : ignorée, pas de rafale.
                    mmio = VGA_WINDOW.contains(&(addr as usize));
                }
                Exit::Interrupted => {
                    self.bios.sync_ticks(self.vm.mem());
                    if let Some(t) = &mut self.throttle {
                        t.pace()?;
                    }
                }
                Exit::Step => return Ok(Event::Step),
                Exit::Hlt => return Err("HLT sans interruption à attendre (PIC : étape 4)".into()),
                Exit::Shutdown => return Err("arrêt du processeur (triple faute ?)".into()),
                Exit::Other(e) => return Err(format!("sortie de VM inattendue : {e}").into()),
            }
            if mmio {
                self.stats.mmio_exits += 1;
                let mut bus = MmioBus { vga: &mut self.vga };
                let burst = interp::continue_in_host(&mut self.vm, &mut bus, IDLE_LIMIT, BURST_BUDGET)?;
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
        if cpu.seg[1].selector != ROM_SEG || cpu.ip >= 0x1000 {
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
        self.bios.sync_ticks(mem);
        let flow = match self.dos.call(vector, &mut cpu, mem, &mut self.host)? {
            Some(flow) => flow,
            None => self.bios.call(vector, &mut cpu, mem, &mut self.host)?,
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
        if self.host.stop {
            return Err("fin de l'entrée : session arrêtée".into());
        }
        Ok(None)
    }

    /// Services non gérés rencontrés (vecteur, AH).
    #[cfg(test)]
    pub fn unhandled(&self) -> &BTreeSet<(u8, u8)> {
        &self.unhandled
    }

    /// Texte de la page active de l'écran.
    pub fn screen_text(&self) -> Vec<String> {
        bios::screen_text(self.vm.mem())
    }

    #[cfg(test)]
    pub fn vga_byte(&self, off: usize) -> u8 {
        self.vga.0[off]
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
    }
}
