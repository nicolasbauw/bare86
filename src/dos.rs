// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS en HLE, version minimale (étape 1) : chargeur .COM et quelques
//! services INT 21h. Le DOS complet arrive à l'étape 3.

use crate::bios;
use crate::hle::{DS, Flow, Regs, lin};
use crate::host::Host;
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Segment du PSP du programme chargé.
pub const PSP_SEG: u16 = 0x1000;

/// Charge un .COM derrière un PSP minimal ; renvoie l'état initial du CPU.
pub fn load_com(mem: &GuestMem, image: &[u8]) -> Result<Cpu, String> {
    if image.len() > 0xFF00 - 2 {
        return Err("fichier .COM trop gros".into());
    }
    let psp = lin(PSP_SEG, 0);
    mem.write(psp, &[0xCD, 0x20]); // INT 20h en PSP:0000
    mem.write(psp + 0x80, &[0, 0x0D]); // ligne de commande vide
    mem.write(psp + 0x100, image);
    // Mot nul en sommet de pile : un RET final saute en PSP:0 (INT 20h).
    mem.write_u16(psp + 0xFFFE, 0);
    Ok(Cpu::real_mode(PSP_SEG, 0x100, 0xFFFE))
}

/// Sortie console : télétype du BIOS sur la page active.
fn con_out(mem: &GuestMem, host: &mut Host, bytes: &[u8]) {
    let page = bios::active_page(mem);
    for &c in bytes {
        bios::tty(mem, host.text.as_mut(), page, c, None);
    }
}

pub fn int21(cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Flow {
    let ax = cpu.gpr[0];
    let ah = (ax >> 8) as u8;
    let al = ax as u8;
    let ds = cpu.sreg(DS);
    let dx = cpu.gpr[2] as u16;
    match ah {
        0x02 => con_out(mem, host, &[cpu.gpr[2] as u8]),
        0x09 => {
            let mut s = Vec::new();
            let mut off = dx;
            loop {
                let c = mem.read_u8(lin(ds, off));
                if c == b'$' || s.len() > 0xFFFF {
                    break;
                }
                s.push(c);
                off = off.wrapping_add(1);
            }
            con_out(mem, host, &s);
        }
        // Détourner un vecteur : AL = n°, DS:DX = gestionnaire.
        0x25 => {
            let at = usize::from(al) * 4;
            mem.write_u16(at, dx);
            mem.write_u16(at + 2, ds);
        }
        // Version : 5.00, OEM IBM.
        0x30 => {
            cpu.gpr[0] = (cpu.gpr[0] & !0xFFFF) | 0x0005;
            cpu.gpr[3] &= !0xFFFF;
            cpu.gpr[1] &= !0xFFFF;
        }
        // Lire un vecteur : ES:BX.
        0x35 => {
            let at = usize::from(al) * 4;
            cpu.gpr[3] = (cpu.gpr[3] & !0xFFFF) | u32::from(mem.read_u16(at));
            let es = mem.read_u16(at + 2);
            cpu.seg[0].selector = es;
            cpu.seg[0].base = u32::from(es) << 4;
            cpu.seg_dirty = true;
        }
        0x4C => return Flow::Exit(al),
        _ => return Flow::Unhandled(ah),
    }
    Flow::Continue
}
