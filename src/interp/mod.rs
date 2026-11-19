// SPDX-License-Identifier: GPL-3.0-or-later
//! Interpréteur x86 côté hôte pour les rafales d'accès à la mémoire vidéo.
//!
//! Après une sortie MMIO, l'hôte exécute lui-même les instructions suivantes
//! tant qu'elles touchent la VRAM, puis rend la main à la VM. Une rafale de
//! milliers d'accès ne coûte ainsi qu'une sortie de VM.
//!
//! Mode réel uniquement pour l'instant. Toute instruction non gérée, ou qui
//! fauterait, termine la rafale : la VM l'exécute alors nativement.

mod alu;
#[cfg(test)]
pub(crate) mod difftest;
mod exec;

use iced_x86::{Decoder, DecoderOptions, Instruction};
use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::backend::Backend;
use crate::mem::GuestMem;
use exec::{Bail, Exec};

/// Accès à la mémoire non mappée (fenêtre VGA) depuis l'interpréteur.
pub trait Mmio {
    /// Vrai si cet accès à l'adresse linéaire est géré ici plutôt qu'en RAM
    /// (fenêtre VGA ; écritures dans la ROM, ignorées).
    fn claims(&self, addr: u32, write: bool) -> bool;
    fn read8(&mut self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, v: u8);
    /// Vrai si les IN/OUT sur ce port sont servis pendant la rafale (sinon
    /// la rafale s'arrête et le processeur exécute l'instruction).
    fn port_claims(&self, _port: u16) -> bool {
        false
    }
    fn port_in(&mut self, _port: u16, _size: u8) -> u32 {
        u32::MAX
    }
    fn port_out(&mut self, _port: u16, _size: u8, _v: u32) {}
    /// Adresse hôte du plan lu pour les lectures simples de la fenêtre
    /// (traducteur dynamique) ; nulle sinon.
    fn direct_read_window(&mut self) -> u64 {
        0
    }
    /// Lecture faite directement à ce décalage de la fenêtre : la carte en
    /// chargera ses latches avant leur prochain usage.
    fn direct_read_at(&mut self, _off: u32) {}
    /// Compteur d'activité de la carte (accès émulés, ports servis) : le
    /// traducteur rend la main quand il cesse d'avancer.
    fn activity(&self) -> u64 {
        0
    }
    /// Adresse hôte où les écritures simples dans la fenêtre A0000 peuvent
    /// se faire directement (traducteur dynamique) ; nulle sinon.
    fn direct_write_window(&mut self) -> u64 {
        0
    }
    /// memfd de la VRAM, adresse hôte de son mappage complet et nombre de
    /// régions de 64 Ko (plans) que les fenêtres directes peuvent viser :
    /// le traducteur en fait des vues (`jit::fastmem::VgaViews`).
    fn vram_mapping(&self) -> Option<(i32, u64, usize)> {
        None
    }
    /// Écriture du masque de plans servie par le code traduit
    /// (`Vga::plane_mask_fast`) ; `None` si ce bus ne sert pas 3C5h.
    fn plane_mask_fast(&mut self) -> Option<crate::vga::PlaneMaskFast> {
        None
    }
    /// Vrai si la rafale doit s'arrêter après l'instruction courante.
    fn stop(&self) -> bool {
        false
    }
}

/// Registre de segment avec son descripteur en cache (comme le processeur).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    pub selector: u16,
    pub base: u32,
    /// Limite en octets (granularité déjà appliquée).
    pub limit: u32,
    /// Type du descripteur (4 bits) : code = bit 3 ; données : E = bit 2, W = bit 1 ;
    /// code : R = bit 1.
    pub typ: u8,
    /// Descripteur de code ou de données (S = 1).
    pub s: bool,
    pub dpl: u8,
    pub present: bool,
    /// D/B : code ou pile 32 bits.
    pub db: bool,
    pub g: bool,
    pub unusable: bool,
}

impl Seg {
    /// Segment de données lecture-écriture du mode réel.
    pub fn real(selector: u16) -> Self {
        Self {
            selector,
            base: u32::from(selector) << 4,
            limit: 0xFFFF,
            typ: 3,
            s: true,
            dpl: 0,
            present: true,
            db: false,
            g: false,
            unusable: false,
        }
    }
}

/// État du CPU invité pendant une rafale.
#[derive(Clone, Debug)]
pub struct Cpu {
    /// AX, CX, DX, BX, SP, BP, SI, DI (ordre de l'encodage x86).
    pub gpr: [u32; 8],
    pub ip: u32,
    pub flags: u32,
    /// ES, CS, SS, DS, FS, GS.
    pub seg: [Seg; 6],
    /// SS.B : pile 32 bits.
    pub stack32: bool,
    pub idt_base: u32,
    pub cr0: u32,
    /// Répertoire de pages (CR3) et CR4 (PSE : pages de 4 Mo).
    pub cr3: u32,
    pub cr4: u32,
    /// TSS (base linéaire, limite) : bitmap des ports en mode protégé et V86.
    pub tr_base: u32,
    pub tr_limit: u32,
    pub(crate) seg_dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Trop d'instructions de suite sans accès VRAM ni port servi : la VM
    /// reprend la main.
    Idle,
    /// Plafond d'instructions par rafale atteint (interruptions, bridage).
    Budget,
    /// Instruction non gérée ou fautive : la VM l'exécutera nativement.
    Unsupported,
    /// Arrêt demandé par le bus (`Mmio::stop`).
    Stopped,
}

#[derive(Debug)]
#[allow(dead_code)] // mmio_accesses et reason : pour le réglage des rafales
pub struct Burst {
    pub instructions: u64,
    pub mmio_accesses: u64,
    pub reason: StopReason,
}

impl Cpu {
    pub fn from_kvm(r: &kvm_regs, s: &kvm_sregs) -> Self {
        let seg = |x: &kvm_segment| Seg {
            selector: x.selector,
            base: x.base as u32,
            limit: x.limit,
            typ: x.type_,
            s: x.s != 0,
            dpl: x.dpl,
            present: x.present != 0,
            db: x.db != 0,
            g: x.g != 0,
            unusable: x.unusable != 0,
        };
        Self {
            gpr: [r.rax, r.rcx, r.rdx, r.rbx, r.rsp, r.rbp, r.rsi, r.rdi].map(|v| v as u32),
            ip: r.rip as u32,
            flags: r.rflags as u32,
            seg: [seg(&s.es), seg(&s.cs), seg(&s.ss), seg(&s.ds), seg(&s.fs), seg(&s.gs)],
            stack32: s.ss.db != 0,
            idt_base: s.idt.base as u32,
            cr0: s.cr0 as u32,
            cr3: s.cr3 as u32,
            cr4: s.cr4 as u32,
            tr_base: s.tr.base as u32,
            tr_limit: s.tr.limit,
            seg_dirty: false,
        }
    }

    /// Mode réel, tous les segments à `seg`, IF levé.
    pub fn real_mode(seg: u16, ip: u16, sp: u16) -> Self {
        let mut gpr = [0; 8];
        gpr[4] = sp.into();
        let s = Seg::real(seg);
        Self { gpr, ip: ip.into(), flags: 0x202, seg: [s; 6], stack32: false, idt_base: 0, cr0: 0, cr3: 0, cr4: 0, tr_base: 0, tr_limit: 0, seg_dirty: true }
    }

    /// Mode protégé (CR0.PE).
    pub fn protected(&self) -> bool {
        self.cr0 & 1 != 0
    }

    /// Mode virtuel 8086 (EFLAGS.VM) : adressage du mode réel, CPL 3, sous
    /// un superviseur (JEMMEX, EMM386).
    pub fn v86(&self) -> bool {
        self.flags & (1 << 17) != 0
    }

    /// Mode protégé avec descripteurs (pas V86).
    pub fn pm(&self) -> bool {
        self.protected() && !self.v86()
    }

    /// Code 32 bits (CS.D en mode protégé).
    pub fn code32(&self) -> bool {
        self.pm() && self.seg[1].db
    }

    /// Niveau de privilège courant (DPL de SS, comme le fait SVM ; 3 en V86).
    pub fn cpl(&self) -> u8 {
        if self.v86() {
            3
        } else if self.protected() {
            self.seg[2].dpl
        } else {
            0
        }
    }

    pub fn linear(&self, seg: usize, off: u32) -> u32 {
        self.seg[seg].base.wrapping_add(off)
    }

    /// Recopie l'état ; renvoie vrai si les registres de segment ont changé.
    pub fn to_kvm(&self, r: &mut kvm_regs, s: &mut kvm_sregs) -> bool {
        let g = self.gpr.map(u64::from);
        (r.rax, r.rcx, r.rdx, r.rbx, r.rsp, r.rbp, r.rsi, r.rdi) = (g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7]);
        r.rip = self.ip.into();
        r.rflags = self.flags.into();
        if self.seg_dirty {
            for (dst, src) in [&mut s.es, &mut s.cs, &mut s.ss, &mut s.ds, &mut s.fs, &mut s.gs].into_iter().zip(self.seg) {
                dst.selector = src.selector;
                dst.base = src.base.into();
                dst.limit = src.limit;
            }
        }
        self.seg_dirty
    }
}

/// Lit `buf.len()` octets à l'adresse linéaire `lin` de l'invité, page par
/// page à travers sa pagination ; `false` si une page est absente.
pub fn read_linear(cpu: &Cpu, mem: &GuestMem, lin: u32, buf: &mut [u8]) -> bool {
    let mut done = 0;
    while done < buf.len() {
        let at = lin.wrapping_add(done as u32);
        let n = (0x1000 - (at & 0xFFF) as usize).min(buf.len() - done);
        match exec::translate(cpu, mem, at, false, false) {
            Some(p) if p as usize + n <= mem.size() => mem.read(p as usize, &mut buf[done..done + n]),
            _ => return false,
        }
        done += n;
    }
    true
}

/// Écrit `data` à l'adresse linéaire `lin` de l'invité (pagination, bits A
/// et D posés comme par le processeur) ; `false` si une page est absente.
pub fn write_linear(cpu: &Cpu, mem: &GuestMem, lin: u32, data: &[u8]) -> bool {
    let mut done = 0;
    while done < data.len() {
        let at = lin.wrapping_add(done as u32);
        let n = (0x1000 - (at & 0xFFF) as usize).min(data.len() - done);
        match exec::translate(cpu, mem, at, true, true) {
            Some(p) if p as usize + n <= mem.size() => mem.write(p as usize, &data[done..done + n]),
            _ => return false,
        }
        done += n;
    }
    true
}

/// Instruction décodée à CS:IP, avec l'indicateur d'adressage 32 bits
/// (taille par défaut du segment de code, inversée par 67h).
fn fetch(cpu: &Cpu, mem: &GuestMem) -> Option<(Instruction, bool)> {
    let mut bytes = [0u8; 15];
    let code32 = cpu.code32();
    let ip = if code32 { cpu.ip } else { cpu.ip & 0xFFFF };
    let at = cpu.seg[1].base.wrapping_add(ip);
    // Octet par octet à travers la pagination : on s'arrête à la première
    // page absente ; le décodage échoue si l'instruction la franchit.
    let mut n = 0;
    while n < bytes.len() {
        match exec::translate(cpu, mem, at.wrapping_add(n as u32), false, false) {
            Some(p) if (p as usize) < mem.size() => bytes[n] = mem.read_u8(p as usize),
            _ => break,
        }
        n += 1;
    }
    let bitness = if code32 { 32 } else { 16 };
    let ins = Decoder::with_ip(bitness, &bytes[..n], u64::from(ip), DecoderOptions::NONE).decode();
    if ins.is_invalid() {
        return None;
    }
    let prefix67 = bytes[..ins.len()]
        .iter()
        .take_while(|b| matches!(b, 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 | 0x66 | 0x67 | 0xF0 | 0xF2 | 0xF3))
        .any(|&b| b == 0x67);
    Some((ins, code32 != prefix67))
}

/// États que l'interpréteur laisse entièrement au processeur : pas à pas.
fn unsupported_mode(cpu: &Cpu) -> bool {
    cpu.flags & alu::TF != 0
}

/// Résultat d'un pas isolé (tests différentiels).
#[cfg(test)]
pub(crate) struct Stepped {
    pub ins: Instruction,
    pub flags_push: Option<u32>,
}

/// Exécute une instruction (une seule itération de REP, comme le pas à pas
/// du processeur). `None` : l'interpréteur renonce, état inchangé.
#[cfg(test)]
pub(crate) fn step_once<M: Mmio>(cpu: &mut Cpu, mem: &GuestMem, mmio: &mut M) -> Option<Stepped> {
    if unsupported_mode(cpu) {
        return None;
    }
    let (ins, addr32) = fetch(cpu, mem)?;
    let mut ex = Exec { cpu, mem, mmio, mmio_hits: 0, rep_budget: 1, flags_push: None, addr32, dry: None };
    ex.step(&ins).ok()?;
    Some(Stepped { ins, flags_push: ex.flags_push })
}

/// Instruction à CS:EIP (sonde BARE86_JITPROBE).
pub fn describe_at(cpu: &Cpu, mem: &GuestMem) -> String {
    match fetch(cpu, mem) {
        Some((ins, _)) => format!("{:08X} {ins}", cpu.seg[1].base.wrapping_add(cpu.ip)),
        None => format!("{:08X} (fetch failed, mode?)", cpu.seg[1].base.wrapping_add(cpu.ip)),
    }
}

/// Exécute une rafale à partir de l'état courant.
/// `idle_limit` : instructions de suite sans MMIO avant de rendre la main ;
/// `budget` : plafond d'instructions (et d'itérations de REP) de la rafale.
pub fn run_burst<M: Mmio>(cpu: &mut Cpu, mem: &GuestMem, mmio: &mut M, idle_limit: u64, budget: u64) -> Burst {
    let mut instructions = 0;
    let mut mmio_accesses = 0;
    let mut idle = 0;
    let reason = loop {
        if instructions >= budget {
            break StopReason::Budget;
        }
        if idle >= idle_limit {
            break StopReason::Idle;
        }
        // Pas à pas : laissé au processeur.
        if unsupported_mode(cpu) {
            break StopReason::Unsupported;
        }
        let Some((ins, addr32)) = fetch(cpu, mem) else { break StopReason::Unsupported };
        let rep_budget = budget - instructions;
        let mut ex = Exec { cpu: &mut *cpu, mem, mmio: &mut *mmio, mmio_hits: 0, rep_budget, flags_push: None, addr32, dry: None };
        let result = ex.step(&ins);
        let (hits, used) = (ex.mmio_hits, rep_budget - ex.rep_budget);
        mmio_accesses += hits;
        // Une par itération d'un `rep` (au moins une : ECX = 0, ou
        // instruction simple).
        instructions += used.max(1);
        idle = if hits == 0 { idle + 1 } else { 0 };
        if let Err(Bail) = result {
            break StopReason::Unsupported;
        }
        if mmio.stop() {
            break StopReason::Stopped;
        }
    };
    Burst { instructions, mmio_accesses, reason }
}

/// Attente active pure à partir de `start` ? L'interpréteur exécute à blanc
/// (écritures dans une mémoire superposée, état de l'invité intact) au plus
/// `max` instructions. Vrai si, après un premier tour qui peut écrire
/// (pile, variables recopiées), un tour complet ramène exactement au même
/// état : mêmes registres au point de départ, même mémoire superposée (un
/// emplacement de pile peut changer au cours du tour, deux appels y laissant
/// des adresses de retour différentes), ni port ni accès claimé par `mmio`.
/// La boucle est alors un point fixe : sans interruption, l'invité ne
/// progresse plus.
pub fn probe_spin<M: Mmio>(start: &Cpu, mem: &GuestMem, mmio: &mut M, max: u64) -> bool {
    if unsupported_mode(start) {
        return false;
    }
    let same = |a: &Cpu, b: &Cpu| a.gpr == b.gpr && a.flags == b.flags && a.seg == b.seg;
    let mut cpu = start.clone();
    let mut overlay = exec::Overlay::default();
    // État au début du tour en cours (après le premier passage au départ).
    let mut lap: Option<(Cpu, std::collections::HashMap<u32, u8>)> = None;
    let mut laps = 0;
    for _ in 0..max {
        let Some((ins, addr32)) = fetch(&cpu, mem) else { return false };
        let mut ex = Exec { cpu: &mut cpu, mem, mmio: &mut *mmio, mmio_hits: 0, rep_budget: 1, flags_push: None, addr32, dry: Some(&mut overlay) };
        if ex.step(&ins).is_err() || ex.mmio_hits > 0 {
            return false;
        }
        if cpu.ip == start.ip && cpu.seg[1] == start.seg[1] {
            if let Some((c, bytes)) = &lap
                && same(&cpu, c)
                && overlay.bytes == *bytes
            {
                return true;
            }
            laps += 1;
            if laps > 2 {
                return false;
            }
            lap = Some((cpu.clone(), overlay.bytes.clone()));
        }
    }
    false
}

/// Après une sortie MMIO déjà servie : termine l'instruction en cours sans
/// relancer l'invité, puis exécute une rafale côté hôte et recopie l'état.
pub fn continue_in_host<B: Backend, M: Mmio>(
    vm: &mut B,
    mmio: &mut M,
    idle_limit: u64,
    budget: u64,
) -> crate::backend::Result<Burst> {
    vm.complete_pending(&mut |addr, data, write| {
        for (i, b) in data.iter_mut().enumerate() {
            let a = addr as u32 + i as u32;
            if write {
                mmio.write8(a, *b);
            } else {
                *b = if mmio.claims(a, false) { mmio.read8(a) } else { 0xFF };
            }
        }
    })?;
    let mut cpu = vm.state()?;
    let burst = run_burst(&mut cpu, vm.mem(), mmio, idle_limit, budget);
    vm.set_state(&cpu)?;
    Ok(burst)
}

#[cfg(test)]
mod spin_tests {
    use super::*;

    struct NoMmio;
    impl Mmio for NoMmio {
        fn claims(&self, _: u32, _: bool) -> bool {
            false
        }
        fn read8(&mut self, _: u32) -> u8 {
            0xFF
        }
        fn write8(&mut self, _: u32, _: u8) {}
    }

    /// Code en 1000:0000, pile en 2000:0100, IF levé.
    fn probe(code: &[u8], cx: u32) -> bool {
        let mem = GuestMem::new(0x40000).unwrap();
        mem.write(0x10000, code);
        let mut seg = [Seg::real(0); 6];
        seg[1] = Seg::real(0x1000);
        seg[1].typ = 0xB;
        seg[2] = Seg::real(0x2000);
        let mut gpr = [0u32; 8];
        gpr[1] = cx;
        gpr[4] = 0x100;
        let cpu = Cpu {
            gpr,
            ip: 0,
            flags: 0x202,
            seg,
            stack32: false,
            idt_base: 0,
            cr0: 0,
            cr3: 0,
            cr4: 0,
            tr_base: 0,
            tr_limit: 0,
            seg_dirty: false,
        };
        let before = mem.read_u32(0x20000 + 0xFC);
        let spin = probe_spin(&cpu, &mem, &mut NoMmio, 1024);
        // À blanc : la pile de l'invité n'a pas été écrite.
        assert_eq!(mem.read_u32(0x20000 + 0xFC), before);
        spin
    }

    #[test]
    fn memory_wait_is_a_spin() {
        // l: mov ax,[0500h] ; cmp ax,1 ; jne l
        assert!(probe(&[0xA1, 0x00, 0x05, 0x3D, 0x01, 0x00, 0x75, 0xF8], 0));
    }

    #[test]
    fn calls_from_two_sites_reach_a_fixed_point() {
        // 0: call 9 ; 3: call 9 ; 6: jmp 0 ; 8: nop ; 9: ret
        // (le même mot de pile reçoit 0003h puis 0006h à chaque tour)
        assert!(probe(&[0xE8, 0x06, 0x00, 0xE8, 0x03, 0x00, 0xEB, 0xF8, 0x90, 0xC3], 0));
    }

    #[test]
    fn progress_is_not_a_spin() {
        // l: inc word [0500h] ; jmp l
        assert!(!probe(&[0xFF, 0x06, 0x00, 0x05, 0xEB, 0xFA], 0));
        // l: dec cx ; jnz l (boucle de temporisation)
        assert!(!probe(&[0x49, 0x75, 0xFD], 0x100));
        // l: in al,dx ; test al,8 ; jz l (scrutation d'un port)
        assert!(!probe(&[0xEC, 0xA8, 0x08, 0x74, 0xFB], 0));
    }
}
