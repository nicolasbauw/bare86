// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS en HLE, version minimale : chargeur .COM et les services INT 21h
//! qu'utilise le runtime d'Open Watcom en modèle tiny. Le DOS complet
//! (MCB, fichiers, MZ) arrive à l'étape 3.

use crate::backend::Result;
use crate::bios;
use crate::hle::{AX, BX, CF, CX, DS, DX, Flow, Regs, SI, lin, set_ret_flag};
use crate::host::Host;
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Segment du PSP du programme chargé.
pub const PSP_SEG: u16 = 0x1000;
/// Fin de la mémoire conventionnelle (segment).
const MEM_TOP: u16 = 0xA000;

/// Codes d'erreur DOS.
const ERR_NO_MEMORY: u16 = 0x08;
const ERR_BAD_HANDLE: u16 = 0x06;

/// Charge un .COM derrière un PSP minimal ; renvoie l'état initial du CPU.
pub fn load_com(mem: &GuestMem, image: &[u8]) -> std::result::Result<Cpu, String> {
    if image.len() > 0xFF00 - 2 {
        return Err("fichier .COM trop gros".into());
    }
    let psp = lin(PSP_SEG, 0);
    mem.write(psp, &[0xCD, 0x20]); // INT 20h en PSP:0000
    mem.write_u16(psp + 2, MEM_TOP); // premier segment après le programme
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

/// État du DOS entre deux appels.
#[derive(Default)]
pub struct Dos {
    /// Scancode d'une touche étendue, rendu au prochain appel de lecture.
    pending_scan: Option<u8>,
}

impl Dos {
    /// Lecture console sans écho (01h, 07h, 08h) : les touches étendues
    /// donnent 0 puis leur scancode.
    fn con_in(&mut self, mem: &GuestMem, host: &mut Host) -> Result<u8> {
        if let Some(scan) = self.pending_scan.take() {
            return Ok(scan);
        }
        let w = bios::read_key(mem, host).map_err(|e| format!("INT 21h : {e}"))?;
        let ascii = w as u8;
        if ascii == 0 || ascii == 0xE0 {
            self.pending_scan = Some((w >> 8) as u8);
            return Ok(0);
        }
        Ok(ascii)
    }

    pub fn int21(&mut self, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Flow> {
        let ah = cpu.hi(AX);
        // Résultat CF : None laisse les drapeaux tels quels.
        let mut carry = None;
        match ah {
            0x01 => {
                let c = self.con_in(mem, host)?;
                con_out(mem, host, &[c]);
                cpu.set_lo(AX, c);
            }
            0x07 | 0x08 => {
                let c = self.con_in(mem, host)?;
                cpu.set_lo(AX, c);
            }
            // État de l'entrée : FFh si une touche attend.
            0x0B => {
                let ready = self.pending_scan.is_some() || bios::peek_key(mem, host).is_some();
                cpu.set_lo(AX, if ready { 0xFF } else { 0 });
            }
            // IOCTL, informations sur le périphérique : la console pour 0 à 2.
            0x44 if cpu.lo(AX) == 0x00 => {
                if cpu.w(BX) <= 2 {
                    cpu.set_w(DX, 0x80D3);
                    carry = Some(false);
                } else {
                    cpu.set_w(AX, ERR_BAD_HANDLE);
                    carry = Some(true);
                }
            }
            // Écriture sur un handle : console seulement avant l'étape 3.
            0x40 => {
                if matches!(cpu.w(BX), 1 | 2) {
                    let mut buf = vec![0u8; usize::from(cpu.w(CX))];
                    mem.read(lin(cpu.sreg(DS), cpu.w(DX)), &mut buf);
                    con_out(mem, host, &buf);
                    cpu.set_w(AX, cpu.w(CX));
                    carry = Some(false);
                } else {
                    cpu.set_w(AX, ERR_BAD_HANDLE);
                    carry = Some(true);
                }
            }
            // Redimensionner le bloc du programme (ES) : sans MCB, tout
            // l'espace jusqu'à A000h lui appartient.
            0x4A => {
                let max = MEM_TOP - PSP_SEG;
                if cpu.w(BX) <= max {
                    carry = Some(false);
                } else {
                    cpu.set_w(AX, ERR_NO_MEMORY);
                    cpu.set_w(BX, max);
                    carry = Some(true);
                }
            }
            // Page de codes globale : 437.
            0x66 if cpu.lo(AX) == 0x01 => {
                cpu.set_w(BX, 437);
                cpu.set_w(DX, 437);
                carry = Some(false);
            }
            // Table des octets de tête DBCS : vide.
            0x63 if cpu.lo(AX) == 0x00 => {
                cpu.set_sreg(DS, bios::ROM_SEG);
                cpu.set_w(SI, bios::DBCS_TABLE);
                cpu.set_lo(AX, 0);
                carry = Some(false);
            }
            _ => return Ok(int21_basic(cpu, mem, host)),
        }
        if let Some(c) = carry {
            set_ret_flag(cpu, mem, CF, c);
        }
        Ok(Flow::Continue)
    }
}

fn int21_basic(cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Flow {
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
