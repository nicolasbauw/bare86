// SPDX-License-Identifier: GPL-3.0-or-later
//! Outils communs aux services en HLE (BIOS, DOS) : registres 16 bits,
//! drapeaux rendus par l'IRET du stub.

use crate::interp::Cpu;
use crate::mem::GuestMem;

pub enum Flow {
    Continue,
    Exit(u8),
    /// Service non géré : AH du service.
    Unhandled(u8),
    /// En attente d'une touche : l'appel sera repris tel quel après un HLT
    /// (registres d'entrée rétablis), pour laisser passer l'IRQ 1.
    Wait,
}

/// Registres généraux, dans l'ordre de `Cpu::gpr`.
pub const AX: usize = 0;
pub const CX: usize = 1;
pub const DX: usize = 2;
pub const BX: usize = 3;
pub const SP: usize = 4;
pub const BP: usize = 5;
pub const SI: usize = 6;
pub const DI: usize = 7;
/// Registres de segment, dans l'ordre de `Cpu::seg`.
pub const ES: usize = 0;
pub const SS: usize = 2;
pub const DS: usize = 3;

pub const CF: u16 = 1 << 0;
pub const ZF: u16 = 1 << 6;

pub fn lin(seg: u16, off: u16) -> usize {
    (usize::from(seg) << 4) + usize::from(off)
}

pub trait Regs {
    fn w(&self, r: usize) -> u16;
    fn set_w(&mut self, r: usize, v: u16);
    fn sreg(&self, s: usize) -> u16;
    /// Charge un registre de segment en mode réel (base = sélecteur × 16).
    fn set_sreg(&mut self, s: usize, sel: u16);

    fn hi(&self, r: usize) -> u8 {
        (self.w(r) >> 8) as u8
    }
    fn lo(&self, r: usize) -> u8 {
        self.w(r) as u8
    }
    fn set_hi(&mut self, r: usize, v: u8) {
        let w = self.w(r);
        self.set_w(r, (w & 0x00FF) | (u16::from(v) << 8));
    }
    fn set_lo(&mut self, r: usize, v: u8) {
        let w = self.w(r);
        self.set_w(r, (w & 0xFF00) | u16::from(v));
    }
}

impl Regs for Cpu {
    fn w(&self, r: usize) -> u16 {
        self.gpr[r] as u16
    }
    fn set_w(&mut self, r: usize, v: u16) {
        self.gpr[r] = (self.gpr[r] & !0xFFFF) | u32::from(v);
    }
    fn sreg(&self, s: usize) -> u16 {
        self.seg[s].selector
    }
    fn set_sreg(&mut self, s: usize, sel: u16) {
        self.seg[s].selector = sel;
        self.seg[s].base = u32::from(sel) << 4;
        self.seg_dirty = true;
    }
}

/// Lève ou baisse un drapeau dans le FLAGS empilé par l'INT : l'IRET du
/// stub le restaure, ce qui transmet CF / ZF à l'appelant sans toucher IF.
pub fn set_ret_flag(cpu: &Cpu, mem: &GuestMem, mask: u16, on: bool) {
    let at = lin(cpu.sreg(SS), cpu.w(SP).wrapping_add(4));
    let f = mem.read_u16(at);
    mem.write_u16(at, if on { f | mask } else { f & !mask });
}
