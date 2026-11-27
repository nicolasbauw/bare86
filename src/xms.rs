// SPDX-License-Identifier: GPL-3.0-or-later
//! Pilote XMS 3.0 en HLE (comme HIMEM.SYS) : détection par INT 2Fh
//! AX=4300h / 4310h, point d'entrée en ROM (`out E0h, al ; retf`, précédé du
//! saut court que l'on peut détourner). HMA, porte A20 (toujours ouverte),
//! blocs de mémoire étendue (EMB) au-dessus de la HMA, blocs de mémoire
//! supérieure (UMB) en C800h–DFFFh (le DOS ne les relie pas encore à sa
//! chaîne de MCB : ils servent aux programmes qui les demandent à XMS).

use crate::hle::{AX, BX, CX, DX, DS, ES, Flow, Regs, SI, lin};
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Début de la mémoire gérée : après la HMA (1 Mo + 64 Ko - 16).
const POOL_START: usize = 0x11_0000;
const MAX_HANDLES: usize = 64;
/// Mémoire supérieure, en paragraphes : après une éventuelle ROM VGA
/// (C000h–C7FFh), avant la page frame EMS (E000h).
const UMB: std::ops::Range<u16> = 0xC800..0xE000;

#[derive(Clone, Copy)]
struct Block {
    handle: u16,
    start: usize,
    len: usize,
    locks: u8,
}

pub struct Xms {
    pool_end: usize,
    /// Blocs triés par adresse.
    blocks: Vec<Block>,
    hma_used: bool,
    next_handle: u16,
    /// A20 : ouverture globale (03h/04h) et compteur local (05h/06h).
    global_a20: bool,
    local_a20: u32,
    /// UMB alloués : (segment, paragraphes), triés.
    umbs: Vec<(u16, u16)>,
}

impl Xms {
    pub fn new(mem_size: usize) -> Self {
        Self { pool_end: mem_size.max(POOL_START), blocks: Vec::new(), hma_used: false, next_handle: 1, global_a20: false, local_a20: 0, umbs: Vec::new() }
    }

    /// INT 2Fh, fonctions 43h ; `None` pour les autres.
    pub fn int2f(&mut self, cpu: &mut Cpu, entry: (u16, u16)) -> Option<Flow> {
        match cpu.w(AX) {
            0x4300 => cpu.set_lo(AX, 0x80),
            0x4310 => {
                cpu.set_sreg(ES, entry.0);
                cpu.set_w(BX, entry.1);
            }
            _ => return None,
        }
        Some(Flow::Continue)
    }

    /// Ko libres : (plus grand bloc, total).
    fn free_kb(&self) -> (usize, usize) {
        let (mut largest, mut total) = (0, 0);
        for (s, e) in self.gaps() {
            let kb = (e - s) / 1024;
            largest = largest.max(kb);
            total += kb;
        }
        (largest, total)
    }

    fn gaps(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = POOL_START;
        for b in &self.blocks {
            if b.start > at {
                out.push((at, b.start));
            }
            at = b.start + b.len;
        }
        if self.pool_end > at {
            out.push((at, self.pool_end));
        }
        out
    }

    /// Trous de la mémoire supérieure : (segment, paragraphes).
    fn umb_gaps(&self) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        let mut at = UMB.start;
        for &(seg, len) in &self.umbs {
            if seg > at {
                out.push((at, seg - at));
            }
            at = seg + len;
        }
        if UMB.end > at {
            out.push((at, UMB.end - at));
        }
        out
    }

    /// AH=10h–12h : (DX, BX) en sortie, ou (code d'erreur, plus grand bloc).
    fn umb(&mut self, ah: u8, dx: u16, bx: u16) -> Result<(u16, u16), (u8, u16)> {
        let largest = |x: &Self| x.umb_gaps().iter().map(|g| g.1).max().unwrap_or(0);
        match ah {
            0x10 => {
                let Some(&(seg, _)) = self.umb_gaps().iter().find(|g| g.1 >= dx && dx > 0) else {
                    let l = largest(self);
                    return Err((if l == 0 { 0xB1 } else { 0xB0 }, l));
                };
                let at = self.umbs.partition_point(|u| u.0 < seg);
                self.umbs.insert(at, (seg, dx));
                Ok((dx, seg))
            }
            0x11 => {
                let i = self.umbs.iter().position(|u| u.0 == dx).ok_or((0xB2, 0))?;
                self.umbs.remove(i);
                Ok((0, 0))
            }
            _ => {
                let i = self.umbs.iter().position(|u| u.0 == dx).ok_or((0xB2, 0))?;
                let limit = self.umbs.get(i + 1).map_or(UMB.end, |n| n.0);
                if dx + bx > limit {
                    return Err((0xB0, limit - dx));
                }
                self.umbs[i].1 = bx;
                Ok((dx, bx))
            }
        }
    }

    fn find(&self, handle: u16) -> Option<usize> {
        self.blocks.iter().position(|b| b.handle == handle)
    }

    /// Alloue `kb` Ko ; rend le numéro de handle ou le code d'erreur.
    fn alloc(&mut self, kb: usize) -> Result<u16, u8> {
        if self.blocks.len() >= MAX_HANDLES {
            return Err(0xA1);
        }
        let len = kb * 1024;
        let (start, _) = self.gaps().into_iter().find(|(s, e)| e - s >= len).ok_or(0xA0u8)?;
        let mut handle = self.next_handle;
        while handle == 0 || self.find(handle).is_some() {
            handle = handle.wrapping_add(1);
        }
        self.next_handle = handle.wrapping_add(1);
        let at = self.blocks.partition_point(|b| b.start < start);
        self.blocks.insert(at, Block { handle, start, len, locks: 0 });
        Ok(handle)
    }

    /// Redimensionne un bloc non verrouillé (sur place si possible, sinon
    /// en le déplaçant).
    fn realloc(&mut self, mem: &GuestMem, handle: u16, kb: usize) -> Result<(), u8> {
        let i = self.find(handle).ok_or(0xA2u8)?;
        let b = self.blocks[i];
        if b.locks > 0 {
            return Err(0xAB);
        }
        let len = kb * 1024;
        let limit = self.blocks.get(i + 1).map_or(self.pool_end, |n| n.start);
        if b.start + len <= limit {
            self.blocks[i].len = len;
            return Ok(());
        }
        self.blocks.remove(i);
        match self.gaps().into_iter().find(|(s, e)| e - s >= len) {
            Some((start, _)) => {
                let mut buf = vec![0u8; b.len.min(len)];
                mem.read(b.start, &mut buf);
                mem.write(start, &buf);
                let at = self.blocks.partition_point(|x| x.start < start);
                self.blocks.insert(at, Block { start, len, ..b });
                Ok(())
            }
            None => {
                let at = self.blocks.partition_point(|x| x.start < b.start);
                self.blocks.insert(at, b);
                Err(0xA0)
            }
        }
    }

    /// AH=0Bh : copie décrite par la structure en DS:SI.
    fn move_block(&self, cpu: &Cpu, mem: &GuestMem) -> Result<(), u8> {
        let s = lin(cpu.sreg(DS), cpu.w(SI));
        let len = mem.read_u32(s) as usize;
        let resolve = |handle: u16, off: u32, bad_handle: u8, bad_off: u8| -> Result<usize, u8> {
            if handle == 0 {
                let a = lin((off >> 16) as u16, off as u16);
                return if a + len <= 0x11_0000 { Ok(a) } else { Err(bad_off) };
            }
            let b = self.blocks[self.find(handle).ok_or(bad_handle)?];
            if off as usize + len <= b.len { Ok(b.start + off as usize) } else { Err(bad_off) }
        };
        let src = resolve(mem.read_u16(s + 4), mem.read_u32(s + 6), 0xA3, 0xA4)?;
        let dst = resolve(mem.read_u16(s + 10), mem.read_u32(s + 12), 0xA5, 0xA6)?;
        let mut buf = vec![0u8; len];
        mem.read(src, &mut buf);
        mem.write(dst, &buf);
        Ok(())
    }

    /// Point d'entrée XMS (appel lointain) : AX=1 en cas de succès, sinon
    /// AX=0 et BL = code d'erreur. `a20` : état réel de la porte, que les
    /// fonctions 03h à 06h modifient.
    pub fn call(&mut self, cpu: &mut Cpu, mem: &GuestMem, a20: &mut bool) -> Flow {
        if std::env::var_os("XMSDBG").is_some() { eprintln!("XMSDBG in  {:08X?}", cpu.gpr); } // XMSDBG
        let ah = cpu.hi(AX);
        let r: Result<(), u8> = match ah {
            // Version 3.00, révision 3.95, HMA présente.
            0x00 => {
                cpu.set_w(AX, 0x0300);
                cpu.set_w(BX, 0x0395);
                cpu.set_w(DX, 1);
                return Flow::Continue;
            }
            0x01 if self.hma_used => Err(0x91),
            0x01 => {
                self.hma_used = true;
                Ok(())
            }
            0x02 if !self.hma_used => Err(0x93),
            0x02 => {
                self.hma_used = false;
                Ok(())
            }
            // A20 : ouverte tant que l'ouverture globale ou locale tient.
            0x03..=0x06 => {
                match ah {
                    0x03 => self.global_a20 = true,
                    0x04 => self.global_a20 = false,
                    0x05 => self.local_a20 += 1,
                    _ => self.local_a20 = self.local_a20.saturating_sub(1),
                }
                *a20 = self.global_a20 || self.local_a20 > 0;
                Ok(())
            }
            0x07 => {
                cpu.set_w(AX, u16::from(*a20));
                cpu.set_lo(BX, 0);
                return Flow::Continue;
            }
            0x08 | 0x88 => {
                let (largest, total) = self.free_kb();
                if ah == 0x08 {
                    // Plafond de 32 767 Ko : des programmes (NFF, « Need For Fun », 1997) lisent ces mots comme des entiers SIGNÉS et
                    // refusent une machine de 64 Mo (« XMS Memory Free : -1152 Kb »). AH=88h (32 bits) rend la valeur entière.
                    cpu.set_w(AX, largest.min(0x7FFF) as u16);
                    cpu.set_w(DX, total.min(0x7FFF) as u16);
                } else {
                    cpu.gpr[AX] = largest as u32;
                    cpu.gpr[DX] = total as u32;
                    cpu.gpr[CX] = self.pool_end as u32 - 1;
                }
                cpu.set_lo(BX, if total == 0 { 0xA0 } else { 0 });
                return Flow::Continue;
            }
            0x09 | 0x89 => {
                let kb = if ah == 0x09 { usize::from(cpu.w(DX)) } else { cpu.gpr[DX] as usize };
                self.alloc(kb).map(|h| cpu.set_w(DX, h))
            }
            0x0A => match self.find(cpu.w(DX)) {
                None => Err(0xA2),
                Some(i) if self.blocks[i].locks > 0 => Err(0xAB),
                Some(i) => {
                    self.blocks.remove(i);
                    Ok(())
                }
            },
            0x0B => self.move_block(cpu, mem),
            0x0C => match self.find(cpu.w(DX)) {
                None => Err(0xA2),
                Some(i) if self.blocks[i].locks == u8::MAX => Err(0xAC),
                Some(i) => {
                    self.blocks[i].locks += 1;
                    let a = self.blocks[i].start as u32;
                    cpu.set_w(DX, (a >> 16) as u16);
                    cpu.set_w(BX, a as u16);
                    Ok(())
                }
            },
            0x0D => match self.find(cpu.w(DX)) {
                None => Err(0xA2),
                Some(i) if self.blocks[i].locks == 0 => Err(0xAA),
                Some(i) => {
                    self.blocks[i].locks -= 1;
                    Ok(())
                }
            },
            0x0E | 0x8E => match self.find(cpu.w(DX)) {
                None => Err(0xA2),
                Some(i) => {
                    let b = self.blocks[i];
                    let free = (MAX_HANDLES - self.blocks.len()) as u16;
                    cpu.set_hi(BX, b.locks);
                    if ah == 0x0E {
                        cpu.set_lo(BX, free.min(0xFF) as u8);
                        cpu.set_w(DX, (b.len / 1024).min(0xFFFF) as u16);
                    } else {
                        cpu.set_w(CX, free);
                        cpu.gpr[DX] = (b.len / 1024) as u32;
                    }
                    Ok(())
                }
            },
            0x0F | 0x8F => {
                let kb = if ah == 0x0F { usize::from(cpu.w(BX)) } else { cpu.gpr[BX] as usize };
                self.realloc(mem, cpu.w(DX), kb)
            }
            0x10..=0x12 => match self.umb(ah, cpu.w(DX), cpu.w(BX)) {
                Ok((dx, bx)) => {
                    if ah == 0x10 {
                        cpu.set_w(DX, dx);
                        cpu.set_w(BX, bx);
                    }
                    Ok(())
                }
                Err((e, largest)) => {
                    cpu.set_w(DX, largest);
                    Err(e)
                }
            },
            _ => Err(0x80),
        };
        match r {
            Ok(()) => cpu.set_w(AX, 1),
            Err(e) => {
                cpu.set_w(AX, 0);
                cpu.set_lo(BX, e);
            }
        }
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(x: &mut Xms, cpu: &mut Cpu, mem: &GuestMem, ax: u16) -> u16 {
        cpu.set_w(AX, ax);
        x.call(cpu, mem, &mut false);
        cpu.w(AX)
    }

    #[test]
    fn alloc_lock_move_realloc() {
        let mem = GuestMem::new(0x20_0000).unwrap();
        let mut x = Xms::new(mem.size());
        let mut cpu = Cpu::real_mode(0, 0, 0);
        call(&mut x, &mut cpu, &mem, 0x0800);
        assert_eq!(cpu.w(DX), 960);
        cpu.set_w(DX, 100);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x0900), 1);
        let h = cpu.w(DX);
        // Copie de 4 octets de 1000:0000 vers le bloc, décalage 8.
        mem.write(0x1_0000, b"XMS!");
        let s = 0x500;
        mem.write_u32(s, 4);
        mem.write_u16(s + 4, 0);
        mem.write_u32(s + 6, 0x1000_0000);
        mem.write_u16(s + 10, h);
        mem.write_u32(s + 12, 8);
        cpu.set_sreg(DS, 0);
        cpu.set_w(SI, s as u16);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x0B00), 1);
        cpu.set_w(DX, h);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x0C00), 1);
        let a = (usize::from(cpu.w(DX)) << 16) | usize::from(cpu.w(BX));
        assert_eq!(a, POOL_START);
        let mut buf = [0u8; 4];
        mem.read(a + 8, &mut buf);
        assert_eq!(&buf, b"XMS!");
        // Verrouillé : ni libération ni redimensionnement.
        cpu.set_w(DX, h);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x0A00), 0);
        assert_eq!(cpu.lo(BX), 0xAB);
        cpu.set_w(DX, h);
        call(&mut x, &mut cpu, &mem, 0x0D00);
        // Un second bloc juste après empêche l'agrandissement sur place.
        cpu.set_w(DX, 10);
        call(&mut x, &mut cpu, &mem, 0x0900);
        cpu.set_w(DX, h);
        cpu.set_w(BX, 200);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x0F00), 1);
        let b = x.blocks[x.find(h).unwrap()];
        assert_eq!((b.start, b.len), (POOL_START + 110 * 1024, 200 * 1024));
        mem.read(b.start + 8, &mut buf);
        assert_eq!(&buf, b"XMS!");
    }

    #[test]
    fn upper_memory_blocks() {
        let mem = GuestMem::new(0x20_0000).unwrap();
        let mut x = Xms::new(mem.size());
        let mut cpu = Cpu::real_mode(0, 0, 0);
        cpu.set_w(DX, 0x1000);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x1000), 1);
        assert_eq!((cpu.w(BX), cpu.w(DX)), (0xC800, 0x1000));
        // Trop grand : le plus grand bloc restant (0D800h–0DFFFh) est rendu.
        cpu.set_w(DX, 0x1000);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x1000), 0);
        assert_eq!((cpu.lo(BX), cpu.w(DX)), (0xB0, 0x800));
        cpu.set_w(DX, 0xC800);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x1100), 1);
        cpu.set_w(DX, 0x1800);
        assert_eq!(call(&mut x, &mut cpu, &mem, 0x1000), 1);
        assert_eq!(cpu.w(BX), 0xC800);
    }
}
