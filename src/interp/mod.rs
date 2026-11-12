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
mod difftest;
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    pub selector: u16,
    pub base: u32,
    pub limit: u32,
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
    pub(crate) seg_dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Trop d'instructions de suite sans accès VRAM : la VM reprend la main.
    Idle,
    /// Plafond d'instructions par rafale atteint (interruptions, bridage).
    Budget,
    /// Instruction non gérée ou fautive : la VM l'exécutera nativement.
    Unsupported,
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
        let seg = |x: &kvm_segment| Seg { selector: x.selector, base: x.base as u32, limit: x.limit };
        Self {
            gpr: [r.rax, r.rcx, r.rdx, r.rbx, r.rsp, r.rbp, r.rsi, r.rdi].map(|v| v as u32),
            ip: r.rip as u32,
            flags: r.rflags as u32,
            seg: [seg(&s.es), seg(&s.cs), seg(&s.ss), seg(&s.ds), seg(&s.fs), seg(&s.gs)],
            stack32: s.ss.db != 0,
            idt_base: s.idt.base as u32,
            cr0: s.cr0 as u32,
            seg_dirty: false,
        }
    }

    /// Mode réel, tous les segments à `seg`, IF levé.
    pub fn real_mode(seg: u16, ip: u16, sp: u16) -> Self {
        let s = Seg { selector: seg, base: u32::from(seg) << 4, limit: 0xFFFF };
        let mut gpr = [0; 8];
        gpr[4] = sp.into();
        Self { gpr, ip: ip.into(), flags: 0x202, seg: [s; 6], stack32: false, idt_base: 0, cr0: 0, seg_dirty: true }
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

/// Instruction décodée à CS:IP, avec l'indicateur d'adressage 32 bits (67h).
fn fetch(cpu: &Cpu, mem: &GuestMem) -> Option<(Instruction, bool)> {
    let mut bytes = [0u8; 15];
    let ip = cpu.ip & 0xFFFF;
    let at = cpu.seg[1].base.wrapping_add(ip) as usize;
    let n = bytes.len().min(mem.size().saturating_sub(at));
    mem.read(at, &mut bytes[..n]);
    let ins = Decoder::with_ip(16, &bytes[..n], u64::from(ip), DecoderOptions::NONE).decode();
    if ins.is_invalid() {
        return None;
    }
    let addr32 = bytes[..ins.len()]
        .iter()
        .take_while(|b| matches!(b, 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 | 0x66 | 0x67 | 0xF0 | 0xF2 | 0xF3))
        .any(|&b| b == 0x67);
    Some((ins, addr32))
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
    let (ins, addr32) = fetch(cpu, mem)?;
    let mut ex = Exec { cpu, mem, mmio, mmio_hits: 0, rep_budget: 1, flags_push: None, addr32 };
    ex.step(&ins).ok()?;
    Some(Stepped { ins, flags_push: ex.flags_push })
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
        // Pas à pas demandé par l'invité : laisser le processeur s'en charger.
        if cpu.flags & alu::TF != 0 {
            break StopReason::Unsupported;
        }
        let Some((ins, addr32)) = fetch(cpu, mem) else { break StopReason::Unsupported };
        let rep_budget = budget - instructions;
        let mut ex = Exec { cpu: &mut *cpu, mem, mmio: &mut *mmio, mmio_hits: 0, rep_budget, flags_push: None, addr32 };
        let result = ex.step(&ins);
        let (hits, used) = (ex.mmio_hits, rep_budget - ex.rep_budget);
        mmio_accesses += hits;
        instructions += 1 + used;
        idle = if hits == 0 { idle + 1 } else { 0 };
        if let Err(Bail) = result {
            break StopReason::Unsupported;
        }
    };
    Burst { instructions, mmio_accesses, reason }
}

/// Après une sortie MMIO déjà servie : termine l'instruction en cours sans
/// relancer l'invité, puis exécute une rafale côté hôte et recopie l'état.
pub fn continue_in_host<B: Backend, M: Mmio>(
    vm: &mut B,
    mmio: &mut M,
    idle_limit: u64,
    budget: u64,
) -> crate::backend::Result<Burst> {
    vm.complete_pending()?;
    let mut cpu = vm.state()?;
    // Mode protégé : pas encore géré par l'interpréteur.
    if cpu.cr0 & 1 != 0 {
        return Ok(Burst { instructions: 0, mmio_accesses: 0, reason: StopReason::Unsupported });
    }
    let burst = run_burst(&mut cpu, vm.mem(), mmio, idle_limit, budget);
    vm.set_state(&cpu)?;
    Ok(burst)
}
