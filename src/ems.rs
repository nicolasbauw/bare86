// SPDX-License-Identifier: GPL-3.0-or-later
//! Pilote EMS (LIM 4.0, fonctions courantes) en HLE. Page frame de 64 Ko en
//! E000h (4 pages physiques de 16 Ko, RAM ordinaire de l'invité) ; pages
//! logiques gardées côté hôte. Mapper une page copie la page physique vers
//! la page logique qu'elle tenait, puis la nouvelle page logique vers la
//! page physique (≈ 1 µs par page). Limite : une même page logique mappée
//! dans deux pages physiques n'est pas partagée (rare).
//!
//! Détection : le vecteur INT 67h pointe dans un segment dont l'offset 0Ah
//! porte « EMMXXXX0 » (en-tête de périphérique en ROM, asm/bios.S).

use std::collections::BTreeMap;

use crate::hle::{AX, BX, CX, DI, DS, DX, ES, Flow, Regs, SI, lin};
use crate::interp::Cpu;
use crate::mem::GuestMem;

pub const FRAME_SEG: u16 = 0xE000;
const PAGE: usize = 0x4000;
const PHYS: usize = 4;
/// Octets d'une carte de mappage sauvegardée (4 × handle, page logique).
const MAP_SIZE: usize = PHYS * 4;

type Mapping = [Option<(u16, u16)>; PHYS];

pub struct Ems {
    total_pages: usize,
    /// Pages logiques de chaque handle (le handle 0, du système, en a 0).
    handles: BTreeMap<u16, Vec<Box<[u8]>>>,
    names: BTreeMap<u16, [u8; 8]>,
    map: Mapping,
    /// Cartes sauvegardées par AH=47h, par handle.
    saved: BTreeMap<u16, Mapping>,
}

impl Ems {
    pub fn new(total_kb: usize) -> Self {
        let mut handles = BTreeMap::new();
        handles.insert(0, Vec::new());
        Self { total_pages: total_kb * 1024 / PAGE, handles, names: BTreeMap::new(), map: [None; PHYS], saved: BTreeMap::new() }
    }

    fn used_pages(&self) -> usize {
        self.handles.values().map(Vec::len).sum()
    }

    fn free_pages(&self) -> usize {
        self.total_pages - self.used_pages()
    }

    fn frame(p: usize) -> usize {
        lin(FRAME_SEG, 0) + p * PAGE
    }

    /// Recopie les pages physiques mappées vers leurs pages logiques.
    fn flush(&mut self, mem: &GuestMem) {
        for (p, m) in self.map.iter().enumerate() {
            if let Some((h, l)) = *m
                && let Some(page) = self.handles.get_mut(&h).and_then(|v| v.get_mut(usize::from(l)))
            {
                mem.read(Self::frame(p), page);
            }
        }
    }

    /// Installe une carte complète (après `flush`).
    fn load(&mut self, mem: &GuestMem, map: Mapping) {
        for (p, m) in map.iter().enumerate() {
            if let Some((h, l)) = *m
                && let Some(page) = self.handles.get(&h).and_then(|v| v.get(usize::from(l)))
            {
                mem.write(Self::frame(p), page);
            }
        }
        self.map = map;
    }

    fn map_page(&mut self, mem: &GuestMem, phys: usize, handle: u16, logical: u16) -> Result<(), u8> {
        let pages = self.handles.get(&handle).ok_or(0x83u8)?.len();
        if phys >= PHYS {
            return Err(0x8B);
        }
        let entry = if logical == 0xFFFF {
            None
        } else if usize::from(logical) < pages {
            Some((handle, logical))
        } else {
            return Err(0x8A);
        };
        self.flush(mem);
        let mut map = self.map;
        map[phys] = entry;
        self.load(mem, map);
        Ok(())
    }

    fn encode(map: &Mapping, mem: &GuestMem, at: usize) {
        for (i, m) in map.iter().enumerate() {
            let (h, l) = m.unwrap_or((0xFFFF, 0xFFFF));
            mem.write_u16(at + 4 * i, h);
            mem.write_u16(at + 4 * i + 2, l);
        }
    }

    fn decode(&self, mem: &GuestMem, at: usize) -> Mapping {
        let mut map = [None; PHYS];
        for (i, m) in map.iter_mut().enumerate() {
            let (h, l) = (mem.read_u16(at + 4 * i), mem.read_u16(at + 4 * i + 2));
            if self.handles.get(&h).is_some_and(|v| usize::from(l) < v.len()) {
                *m = Some((h, l));
            }
        }
        map
    }

    fn resize(&mut self, mem: &GuestMem, handle: u16, pages: usize) -> Result<(), u8> {
        let have = self.handles.get(&handle).ok_or(0x83u8)?.len();
        if pages > self.total_pages {
            return Err(0x87);
        }
        if pages > have + self.free_pages() {
            return Err(0x88);
        }
        self.flush(mem);
        let v = self.handles.get_mut(&handle).expect("handle");
        v.resize_with(pages, || vec![0u8; PAGE].into_boxed_slice());
        // Pages mappées qui n'existent plus : démappées.
        for m in &mut self.map {
            if m.is_some_and(|(h, l)| h == handle && usize::from(l) >= pages) {
                *m = None;
            }
        }
        Ok(())
    }

    pub fn int67(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Flow {
        let (ah, al) = (cpu.hi(AX), cpu.lo(AX));
        let dx = cpu.w(DX);
        let r: Result<(), u8> = match ah {
            0x40 => Ok(()),
            0x41 => {
                cpu.set_w(BX, FRAME_SEG);
                Ok(())
            }
            0x42 => {
                cpu.set_w(BX, self.free_pages() as u16);
                cpu.set_w(DX, self.total_pages as u16);
                Ok(())
            }
            0x43 | 0x5A => {
                let n = usize::from(cpu.w(BX));
                if ah == 0x43 && n == 0 {
                    Err(0x89)
                } else if n > self.total_pages {
                    Err(0x87)
                } else if n > self.free_pages() {
                    Err(0x88)
                } else if self.handles.len() >= 255 {
                    Err(0x85)
                } else {
                    let h = (1..=0xFE).find(|h| !self.handles.contains_key(h)).expect("free handle");
                    self.handles.insert(h, (0..n).map(|_| vec![0u8; PAGE].into_boxed_slice()).collect());
                    cpu.set_w(DX, h);
                    Ok(())
                }
            }
            0x44 => self.map_page(mem, usize::from(al), dx, cpu.w(BX)),
            0x45 => {
                if !self.handles.contains_key(&dx) {
                    Err(0x83)
                } else if self.saved.contains_key(&dx) {
                    Err(0x86)
                } else {
                    for m in &mut self.map {
                        if m.is_some_and(|(h, _)| h == dx) {
                            *m = None;
                        }
                    }
                    if dx == 0 {
                        self.handles.insert(0, Vec::new());
                    } else {
                        self.handles.remove(&dx);
                    }
                    self.names.remove(&dx);
                    Ok(())
                }
            }
            0x46 => {
                cpu.set_lo(AX, 0x40);
                Ok(())
            }
            0x47 => match () {
                _ if !self.handles.contains_key(&dx) => Err(0x83),
                _ if self.saved.contains_key(&dx) => Err(0x8D),
                _ => {
                    self.saved.insert(dx, self.map);
                    Ok(())
                }
            },
            0x48 => match self.saved.remove(&dx) {
                _ if !self.handles.contains_key(&dx) => Err(0x83),
                None => Err(0x8E),
                Some(map) => {
                    self.flush(mem);
                    self.load(mem, map);
                    Ok(())
                }
            },
            0x4B => {
                cpu.set_w(BX, self.handles.len() as u16);
                Ok(())
            }
            0x4C => match self.handles.get(&dx) {
                None => Err(0x83),
                Some(v) => {
                    cpu.set_w(BX, v.len() as u16);
                    Ok(())
                }
            },
            0x4D => {
                let at = lin(cpu.sreg(ES), cpu.w(DI));
                for (i, (h, v)) in self.handles.iter().enumerate() {
                    mem.write_u16(at + 4 * i, *h);
                    mem.write_u16(at + 4 * i + 2, v.len() as u16);
                }
                cpu.set_w(BX, self.handles.len() as u16);
                Ok(())
            }
            // Carte complète (4Eh) ou partielle (4Fh, sauvée en entier : la
            // zone de sauvegarde a la même taille).
            0x4E | 0x4F if (ah == 0x4E && al == 3) || (ah == 0x4F && al == 2) => {
                cpu.set_lo(AX, MAP_SIZE as u8);
                Ok(())
            }
            0x4E | 0x4F => match al {
                0..=2 if ah == 0x4E || al < 2 => {
                    if al == 0 || al == 2 {
                        self.flush(mem);
                        Self::encode(&self.map, mem, lin(cpu.sreg(ES), cpu.w(DI)));
                    }
                    if al == 1 || al == 2 {
                        let map = self.decode(mem, lin(cpu.sreg(DS), cpu.w(SI)));
                        self.flush(mem);
                        self.load(mem, map);
                    }
                    Ok(())
                }
                _ => Err(0x8F),
            },
            0x50 if al <= 1 => {
                let at = lin(cpu.sreg(DS), cpu.w(SI));
                let mut r = Ok(());
                for i in 0..usize::from(cpu.w(CX)) {
                    let logical = mem.read_u16(at + 4 * i);
                    let p = mem.read_u16(at + 4 * i + 2);
                    let phys = if al == 0 { usize::from(p) } else { usize::from(p.wrapping_sub(FRAME_SEG)) / (PAGE >> 4) };
                    r = self.map_page(mem, phys, dx, logical);
                    if r.is_err() {
                        break;
                    }
                }
                r
            }
            0x51 => {
                let r = self.resize(mem, dx, usize::from(cpu.w(BX)));
                if let Some(v) = self.handles.get(&dx) {
                    cpu.set_w(BX, v.len() as u16);
                }
                r
            }
            // Attributs : volatils seulement.
            0x52 => match al {
                0 => {
                    cpu.set_lo(AX, 0);
                    Ok(())
                }
                1 => Err(0x91),
                2 => {
                    cpu.set_lo(AX, 0);
                    Ok(())
                }
                _ => Err(0x8F),
            },
            0x53 => match (al, self.handles.contains_key(&dx)) {
                (_, false) => Err(0x83),
                (0, true) => {
                    mem.write(lin(cpu.sreg(ES), cpu.w(DI)), &self.names.get(&dx).copied().unwrap_or([0; 8]));
                    Ok(())
                }
                (1, true) => {
                    let mut name = [0u8; 8];
                    mem.read(lin(cpu.sreg(DS), cpu.w(SI)), &mut name);
                    self.names.insert(dx, name);
                    Ok(())
                }
                _ => Err(0x8F),
            },
            0x58 => {
                if al == 0 {
                    let at = lin(cpu.sreg(ES), cpu.w(DI));
                    for p in 0..PHYS {
                        mem.write_u16(at + 4 * p, FRAME_SEG + (p * PAGE >> 4) as u16);
                        mem.write_u16(at + 4 * p + 2, p as u16);
                    }
                }
                cpu.set_w(CX, PHYS as u16);
                if al <= 1 { Ok(()) } else { Err(0x8F) }
            }
            0x59 if al == 1 => {
                cpu.set_w(BX, self.free_pages() as u16);
                cpu.set_w(DX, self.total_pages as u16);
                Ok(())
            }
            0x59 if al == 0 => {
                let at = lin(cpu.sreg(ES), cpu.w(DI));
                for (i, w) in [PAGE as u16 / 16, 0, MAP_SIZE as u16, 0, 0].into_iter().enumerate() {
                    mem.write_u16(at + 2 * i, w);
                }
                Ok(())
            }
            // VCPI absent.
            0xDE => Err(0x84),
            _ => Err(0x84),
        };
        cpu.set_hi(AX, r.err().unwrap_or(0));
        if r == Err(0x84) && ah != 0xDE {
            return Flow::Unhandled(ah);
        }
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(e: &mut Ems, cpu: &mut Cpu, mem: &GuestMem, ax: u16) -> u8 {
        cpu.set_w(AX, ax);
        e.int67(cpu, mem);
        cpu.hi(AX)
    }

    #[test]
    fn allocate_map_and_swap() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        let mut e = Ems::new(1024);
        let mut cpu = Cpu::real_mode(0, 0, 0);
        cpu.set_w(BX, 3);
        assert_eq!(call(&mut e, &mut cpu, &mem, 0x4300), 0);
        let h = cpu.w(DX);
        // Page logique 2 en page physique 1 : écriture, puis page 0 à sa place.
        cpu.set_w(BX, 2);
        assert_eq!(call(&mut e, &mut cpu, &mem, 0x4401), 0);
        let p1 = Ems::frame(1);
        mem.write(p1 + 5, b"EMS");
        cpu.set_w(BX, 0);
        cpu.set_w(DX, h);
        call(&mut e, &mut cpu, &mem, 0x4401);
        assert_eq!(mem.read_u8(p1 + 5), 0);
        // Page logique 2 en page physique 3 : l'écriture l'a suivie.
        cpu.set_w(BX, 2);
        cpu.set_w(DX, h);
        call(&mut e, &mut cpu, &mem, 0x4403);
        let mut buf = [0u8; 3];
        mem.read(Ems::frame(3) + 5, &mut buf);
        assert_eq!(&buf, b"EMS");
        // Page hors du handle, handle inconnu.
        cpu.set_w(BX, 3);
        cpu.set_w(DX, h);
        assert_eq!(call(&mut e, &mut cpu, &mem, 0x4400), 0x8A);
        cpu.set_w(DX, 99);
        assert_eq!(call(&mut e, &mut cpu, &mem, 0x4400), 0x83);
        cpu.set_w(DX, h);
        call(&mut e, &mut cpu, &mem, 0x4200);
        assert_eq!((cpu.w(BX), cpu.w(DX)), (61, 64));
    }
}
