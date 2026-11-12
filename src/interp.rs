// SPDX-License-Identifier: GPL-3.0-or-later
//! Interpréteur x86 côté hôte pour les rafales d'accès à la mémoire vidéo.
//!
//! Après une sortie MMIO, l'hôte exécute lui-même les instructions suivantes
//! tant qu'elles touchent la VRAM, puis rend la main à la VM. Une rafale de
//! milliers d'accès ne coûte ainsi qu'une sortie de VM.
//!
//! Incrément 1 : mode réel, instructions entières de base. Toute instruction
//! inconnue termine la rafale ; la VM l'exécute alors nativement.

use iced_x86::{Code, ConditionCode, Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};
use kvm_bindings::{kvm_regs, kvm_sregs};

use crate::vm::GuestMem;

const CF: u32 = 1 << 0;
const PF: u32 = 1 << 2;
const AF: u32 = 1 << 4;
const ZF: u32 = 1 << 6;
const SF: u32 = 1 << 7;
const OF: u32 = 1 << 11;
const ARITH_FLAGS: u32 = CF | PF | AF | ZF | SF | OF;

/// Accès à la mémoire non mappée (fenêtre VGA) depuis l'interpréteur.
pub trait Mmio {
    /// Vrai si l'adresse linéaire est gérée ici plutôt qu'en RAM.
    fn claims(&self, addr: u32) -> bool;
    fn read8(&mut self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, v: u8);
}

#[derive(Clone, Copy)]
struct Seg {
    selector: u16,
    base: u32,
}

/// État du CPU invité pendant une rafale.
pub struct Cpu {
    /// AX, CX, DX, BX, SP, BP, SI, DI (ordre de l'encodage x86).
    gpr: [u32; 8],
    ip: u32,
    flags: u32,
    /// ES, CS, SS, DS, FS, GS.
    seg: [Seg; 6],
    seg_dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Trop d'instructions de suite sans accès VRAM : la VM reprend la main.
    Idle,
    /// Plafond d'instructions par rafale atteint (interruptions, bridage).
    Budget,
    /// Instruction non gérée : la VM l'exécutera nativement.
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
        let seg = |x: &kvm_bindings::kvm_segment| Seg { selector: x.selector, base: x.base as u32 };
        Self {
            gpr: [r.rax, r.rcx, r.rdx, r.rbx, r.rsp, r.rbp, r.rsi, r.rdi].map(|v| v as u32),
            ip: r.rip as u32,
            flags: r.rflags as u32,
            seg: [seg(&s.es), seg(&s.cs), seg(&s.ss), seg(&s.ds), seg(&s.fs), seg(&s.gs)],
            seg_dirty: false,
        }
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
            }
        }
        self.seg_dirty
    }

    fn reg(&self, r: Register) -> u32 {
        if r.is_gpr8() {
            let n = r as usize - Register::AL as usize;
            if n < 4 { self.gpr[n] & 0xFF } else { (self.gpr[n - 4] >> 8) & 0xFF }
        } else if r.is_segment_register() {
            self.seg[r as usize - Register::ES as usize].selector.into()
        } else {
            let v = self.gpr[r.number()];
            if r.size() == 2 { v & 0xFFFF } else { v }
        }
    }

    fn set_reg(&mut self, r: Register, v: u32) {
        if r.is_gpr8() {
            let n = r as usize - Register::AL as usize;
            if n < 4 {
                self.gpr[n] = (self.gpr[n] & !0xFF) | (v & 0xFF);
            } else {
                self.gpr[n - 4] = (self.gpr[n - 4] & !0xFF00) | ((v & 0xFF) << 8);
            }
        } else if r.is_segment_register() {
            // Mode réel : base = sélecteur × 16.
            let s = &mut self.seg[r as usize - Register::ES as usize];
            s.selector = v as u16;
            s.base = (v & 0xFFFF) << 4;
            self.seg_dirty = true;
        } else if r.size() == 2 {
            let n = r.number();
            self.gpr[n] = (self.gpr[n] & !0xFFFF) | (v & 0xFFFF);
        } else {
            self.gpr[r.number()] = v;
        }
    }

    fn seg_base(&self, r: Register) -> u32 {
        self.seg[r as usize - Register::ES as usize].base
    }
}

/// Adresse ou valeur d'un opérande.
#[derive(Clone, Copy)]
enum Loc {
    Reg(Register),
    Mem(u32),
    Imm(u32),
}

struct Exec<'a, M: Mmio> {
    cpu: &'a mut Cpu,
    mem: &'a GuestMem,
    mmio: &'a mut M,
    mmio_hits: u64,
}

impl<M: Mmio> Exec<'_, M> {
    fn read8(&mut self, addr: u32) -> u8 {
        if self.mmio.claims(addr) {
            self.mmio_hits += 1;
            self.mmio.read8(addr)
        } else if (addr as usize) < self.mem.size() {
            self.mem.read_u8(addr as usize)
        } else {
            0xFF
        }
    }

    fn write8(&mut self, addr: u32, v: u8) {
        if self.mmio.claims(addr) {
            self.mmio_hits += 1;
            self.mmio.write8(addr, v);
        } else if (addr as usize) < self.mem.size() {
            self.mem.write(addr as usize, &[v]);
        }
    }

    fn read_mem(&mut self, addr: u32, size: usize) -> u32 {
        (0..size).fold(0, |acc, i| acc | u32::from(self.read8(addr.wrapping_add(i as u32))) << (8 * i))
    }

    fn write_mem(&mut self, addr: u32, size: usize, v: u32) {
        for i in 0..size {
            self.write8(addr.wrapping_add(i as u32), (v >> (8 * i)) as u8);
        }
    }

    /// Adresse linéaire de l'opérande mémoire.
    fn effective_address(&self, ins: &Instruction) -> u32 {
        let mut ea = ins.memory_displacement32();
        if ins.memory_base() != Register::None {
            ea = ea.wrapping_add(self.cpu.reg(ins.memory_base()));
        }
        if ins.memory_index() != Register::None {
            ea = ea.wrapping_add(self.cpu.reg(ins.memory_index()) * ins.memory_index_scale());
        }
        // Taille d'adresse 16 bits : décalage sur 16 bits.
        if ins.memory_base().size() != 4 && ins.memory_index().size() != 4 {
            ea &= 0xFFFF;
        }
        self.cpu.seg_base(ins.memory_segment()).wrapping_add(ea)
    }

    fn loc(&self, ins: &Instruction, n: u32) -> Option<Loc> {
        Some(match ins.op_kind(n) {
            OpKind::Register => Loc::Reg(ins.op_register(n)),
            OpKind::Memory => Loc::Mem(self.effective_address(ins)),
            OpKind::Immediate8
            | OpKind::Immediate16
            | OpKind::Immediate32
            | OpKind::Immediate8to16
            | OpKind::Immediate8to32 => Loc::Imm(ins.immediate(n) as u32),
            _ => return None,
        })
    }

    fn get(&mut self, l: Loc, size: usize) -> u32 {
        match l {
            Loc::Reg(r) => self.cpu.reg(r),
            Loc::Mem(a) => self.read_mem(a, size),
            Loc::Imm(v) => v & mask(size),
        }
    }

    fn set(&mut self, l: Loc, size: usize, v: u32) {
        match l {
            Loc::Reg(r) => self.cpu.set_reg(r, v),
            Loc::Mem(a) => self.write_mem(a, size, v),
            Loc::Imm(_) => unreachable!("écriture dans un immédiat"),
        }
    }

    fn set_flags(&mut self, which: u32, values: u32) {
        self.cpu.flags = (self.cpu.flags & !which) | (values & which);
    }

    fn condition(&self, cc: ConditionCode) -> bool {
        let f = self.cpu.flags;
        let (c, z, s, o, p) = (f & CF != 0, f & ZF != 0, f & SF != 0, f & OF != 0, f & PF != 0);
        match cc {
            ConditionCode::o => o,
            ConditionCode::no => !o,
            ConditionCode::b => c,
            ConditionCode::ae => !c,
            ConditionCode::e => z,
            ConditionCode::ne => !z,
            ConditionCode::be => c || z,
            ConditionCode::a => !c && !z,
            ConditionCode::s => s,
            ConditionCode::ns => !s,
            ConditionCode::p => p,
            ConditionCode::np => !p,
            ConditionCode::l => s != o,
            ConditionCode::ge => s == o,
            ConditionCode::le => z || s != o,
            ConditionCode::g => !z && s == o,
            ConditionCode::None => true,
        }
    }

    /// Exécute `ins` ; faux si l'instruction n'est pas gérée (état inchangé).
    fn step(&mut self, ins: &Instruction) -> bool {
        let next = ins.next_ip() as u32;
        let size = match ins.op_kind(0) {
            OpKind::Register => ins.op_register(0).size(),
            OpKind::Memory => ins.memory_size().size(),
            _ => 0,
        };
        match ins.mnemonic() {
            Mnemonic::Nop => {}
            Mnemonic::Mov => {
                let (Some(d), Some(s)) = (self.loc(ins, 0), self.loc(ins, 1)) else { return false };
                if matches!(d, Loc::Reg(r) if r == Register::CS) {
                    return false;
                }
                let v = self.get(s, size);
                self.set(d, size, v);
            }
            m @ (Mnemonic::Add
            | Mnemonic::Adc
            | Mnemonic::Sub
            | Mnemonic::Sbb
            | Mnemonic::Cmp
            | Mnemonic::And
            | Mnemonic::Or
            | Mnemonic::Xor
            | Mnemonic::Test) => {
                let (Some(d), Some(s)) = (self.loc(ins, 0), self.loc(ins, 1)) else { return false };
                let (a, b) = (self.get(d, size), self.get(s, size));
                let carry = self.cpu.flags & CF;
                let (res, flags) = match m {
                    Mnemonic::Add => add(a, b, 0, size),
                    Mnemonic::Adc => add(a, b, carry, size),
                    Mnemonic::Sub | Mnemonic::Cmp => sub(a, b, 0, size),
                    Mnemonic::Sbb => sub(a, b, carry, size),
                    Mnemonic::And | Mnemonic::Test => logic(a & b, size),
                    Mnemonic::Or => logic(a | b, size),
                    _ => logic(a ^ b, size),
                };
                self.set_flags(ARITH_FLAGS, flags);
                if !matches!(m, Mnemonic::Cmp | Mnemonic::Test) {
                    self.set(d, size, res);
                }
            }
            m @ (Mnemonic::Inc | Mnemonic::Dec) => {
                let Some(d) = self.loc(ins, 0) else { return false };
                let a = self.get(d, size);
                let (res, flags) = if m == Mnemonic::Inc { add(a, 1, 0, size) } else { sub(a, 1, 0, size) };
                self.set_flags(ARITH_FLAGS & !CF, flags);
                self.set(d, size, res);
            }
            Mnemonic::Jmp if ins.op_kind(0) == OpKind::NearBranch16 => {
                self.cpu.ip = ins.near_branch_target() as u32;
                return true;
            }
            _ if ins.is_jcc_short_or_near() => {
                if self.condition(ins.condition_code()) {
                    self.cpu.ip = ins.near_branch_target() as u32;
                    return true;
                }
            }
            Mnemonic::Loop | Mnemonic::Loope | Mnemonic::Loopne | Mnemonic::Jcxz | Mnemonic::Jecxz => {
                let cx = match ins.code() {
                    Code::Loop_rel8_16_ECX | Code::Loope_rel8_16_ECX | Code::Loopne_rel8_16_ECX | Code::Jecxz_rel8_16 => {
                        Register::ECX
                    }
                    Code::Loop_rel8_16_CX | Code::Loope_rel8_16_CX | Code::Loopne_rel8_16_CX | Code::Jcxz_rel8_16 => {
                        Register::CX
                    }
                    _ => return false,
                };
                let taken = if matches!(ins.mnemonic(), Mnemonic::Jcxz | Mnemonic::Jecxz) {
                    self.cpu.reg(cx) == 0
                } else {
                    let count = self.cpu.reg(cx).wrapping_sub(1);
                    self.cpu.set_reg(cx, count);
                    let z = self.cpu.flags & ZF != 0;
                    count & mask(cx.size()) != 0
                        && match ins.mnemonic() {
                            Mnemonic::Loope => z,
                            Mnemonic::Loopne => !z,
                            _ => true,
                        }
                };
                if taken {
                    self.cpu.ip = ins.near_branch_target() as u32;
                    return true;
                }
            }
            _ => return false,
        }
        self.cpu.ip = next;
        true
    }
}

fn mask(size: usize) -> u32 {
    if size >= 4 { u32::MAX } else { (1u32 << (8 * size)) - 1 }
}

fn sign(size: usize) -> u32 {
    1u32 << (8 * size - 1)
}

/// ZF, SF, PF du résultat.
fn szp(res: u32, size: usize) -> u32 {
    let mut f = 0;
    if res == 0 {
        f |= ZF;
    }
    if res & sign(size) != 0 {
        f |= SF;
    }
    if (res as u8).count_ones().is_multiple_of(2) {
        f |= PF;
    }
    f
}

fn add(a: u32, b: u32, c: u32, size: usize) -> (u32, u32) {
    let wide = u64::from(a) + u64::from(b) + u64::from(c);
    let res = wide as u32 & mask(size);
    let mut f = szp(res, size);
    if wide > u64::from(mask(size)) {
        f |= CF;
    }
    if (a ^ res) & (b ^ res) & sign(size) != 0 {
        f |= OF;
    }
    if (a ^ b ^ res) & 0x10 != 0 {
        f |= AF;
    }
    (res, f)
}

fn sub(a: u32, b: u32, c: u32, size: usize) -> (u32, u32) {
    let res = a.wrapping_sub(b).wrapping_sub(c) & mask(size);
    let mut f = szp(res, size);
    if u64::from(a) < u64::from(b) + u64::from(c) {
        f |= CF;
    }
    if (a ^ b) & (a ^ res) & sign(size) != 0 {
        f |= OF;
    }
    if (a ^ b ^ res) & 0x10 != 0 {
        f |= AF;
    }
    (res, f)
}

fn logic(res: u32, size: usize) -> (u32, u32) {
    let res = res & mask(size);
    (res, szp(res, size))
}

/// Exécute une rafale à partir de l'état courant.
/// `idle_limit` : instructions de suite sans MMIO avant de rendre la main ;
/// `budget` : plafond d'instructions de la rafale.
pub fn run_burst<M: Mmio>(cpu: &mut Cpu, mem: &GuestMem, mmio: &mut M, idle_limit: u64, budget: u64) -> Burst {
    let mut ex = Exec { cpu, mem, mmio, mmio_hits: 0 };
    let mut instructions = 0;
    let mut idle = 0;
    let mut bytes = [0u8; 15];
    let reason = loop {
        if instructions >= budget {
            break StopReason::Budget;
        }
        if idle >= idle_limit {
            break StopReason::Idle;
        }
        // Mode réel uniquement pour l'instant : code 16 bits.
        let ip = ex.cpu.ip & 0xFFFF;
        let at = ex.cpu.seg[1].base.wrapping_add(ip) as usize;
        let n = bytes.len().min(mem.size().saturating_sub(at));
        mem.read(at, &mut bytes[..n]);
        let ins = Decoder::with_ip(16, &bytes[..n], u64::from(ip), DecoderOptions::NONE).decode();
        if ins.is_invalid() || ins.has_rep_prefix() || ins.has_repne_prefix() {
            break StopReason::Unsupported;
        }
        let before = ex.mmio_hits;
        if !ex.step(&ins) {
            break StopReason::Unsupported;
        }
        instructions += 1;
        idle = if ex.mmio_hits == before { idle + 1 } else { 0 };
    };
    Burst { instructions, mmio_accesses: ex.mmio_hits, reason }
}

/// Après une sortie MMIO déjà servie : termine l'instruction en cours dans
/// KVM sans relancer l'invité (`immediate_exit`), puis exécute une rafale
/// côté hôte et recopie l'état dans le vCPU.
pub fn continue_in_host<M: Mmio>(
    m: &mut crate::vm::Machine,
    mmio: &mut M,
    idle_limit: u64,
    budget: u64,
) -> Result<Burst, kvm_ioctls::Error> {
    m.vcpu.set_kvm_immediate_exit(1);
    let completed = m.vcpu.run().map(|_| ());
    m.vcpu.set_kvm_immediate_exit(0);
    match completed {
        Err(e) if e.errno() == libc::EINTR => {}
        Err(e) => return Err(e),
        Ok(()) => {}
    }
    let mut regs = m.vcpu.get_regs()?;
    let mut sregs = m.vcpu.get_sregs()?;
    let mut cpu = Cpu::from_kvm(&regs, &sregs);
    let burst = run_burst(&mut cpu, &m.mem, mmio, idle_limit, budget);
    if cpu.to_kvm(&mut regs, &mut sregs) {
        m.vcpu.set_sregs(&sregs)?;
    }
    m.vcpu.set_regs(&regs)?;
    Ok(burst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_add_sub() {
        assert_eq!(add(0xFF, 1, 0, 1), (0, ZF | CF | AF | PF));
        assert_eq!(add(0x7F, 1, 0, 1), (0x80, SF | OF | AF));
        assert_eq!(sub(0, 1, 0, 2), (0xFFFF, SF | CF | AF | PF));
        assert_eq!(sub(0x8000, 1, 0, 2), (0x7FFF, OF | AF | PF));
        assert_eq!(logic(0x0F0, 1), (0xF0, SF | PF));
    }
}
