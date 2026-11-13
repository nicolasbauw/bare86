// SPDX-License-Identifier: GPL-3.0-or-later
//! Contrôleurs DMA 8237 : canaux 0–3 (8 bits, ports 00h–0Fh) et 4–7
//! (16 bits, ports C0h–DFh), registres de page (81h–8Fh). Les transferts se
//! font à la demande du périphérique (`read_mem`), qui lit la mémoire de
//! l'invité ; mode auto-init, sens décroissant, fin de comptage (TC).

use std::cell::RefCell;
use std::rc::Rc;

use crate::io::IoDevice;
use crate::mem::GuestMem;

#[derive(Clone, Copy, Default)]
struct Channel {
    base_addr: u16,
    base_count: u16,
    addr: u16,
    count: u16,
    page: u8,
    mode: u8,
    masked: bool,
}

impl Channel {
    fn auto_init(&self) -> bool {
        self.mode & 0x10 != 0
    }
    fn decrement(&self) -> bool {
        self.mode & 0x20 != 0
    }
}

pub struct Dma {
    ch: [Channel; 8],
    /// Bascule octet bas / haut, par contrôleur.
    flip: [bool; 2],
    /// Fins de comptage pas encore lues (registre d'état), par contrôleur.
    tc: [u8; 2],
}

impl Default for Dma {
    fn default() -> Self {
        Self::new()
    }
}

impl Dma {
    pub fn new() -> Self {
        let ch = [Channel { masked: true, ..Default::default() }; 8];
        Self { ch, flip: [false; 2], tc: [0; 2] }
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    #[cfg(test)]
    pub fn masked(&self, n: usize) -> bool {
        self.ch[n].masked
    }

    /// Lit en mémoire, pour le périphérique du canal `n`, au plus `out.len()`
    /// octets (un nombre pair pour un canal 16 bits). Rend le nombre
    /// d'octets lus et si la fin de comptage a été atteinte.
    pub fn read_mem(&mut self, n: usize, mem: &GuestMem, out: &mut [u8]) -> (usize, bool) {
        let wide = n >= 4;
        let unit = if wide { 2 } else { 1 };
        let c = &mut self.ch[n];
        if c.masked {
            return (0, false);
        }
        let mut done = 0;
        let mut tc = false;
        while done + unit <= out.len() {
            let phys = if wide {
                (usize::from(c.page & 0xFE) << 16) | (usize::from(c.addr) << 1)
            } else {
                (usize::from(c.page) << 16) | usize::from(c.addr)
            };
            for i in 0..unit {
                out[done + i] = if phys + i < mem.size() { mem.read_u8(phys + i) } else { 0xFF };
            }
            done += unit;
            c.addr = if c.decrement() { c.addr.wrapping_sub(1) } else { c.addr.wrapping_add(1) };
            let (count, ended) = c.count.overflowing_sub(1);
            c.count = count;
            if ended {
                tc = true;
                self.tc[n / 4] |= 1 << (n % 4);
                if c.auto_init() {
                    c.addr = c.base_addr;
                    c.count = c.base_count;
                } else {
                    c.masked = true;
                }
                break;
            }
        }
        (done, tc)
    }

    fn write_port(&mut self, port: u16, v: u8) {
        let (ctl, reg) = match port {
            0x00..=0x0F => (0, port),
            0xC0..=0xDF => (1, (port - 0xC0) >> 1),
            _ => {
                self.write_page(port, v);
                return;
            }
        };
        let base = ctl * 4;
        match reg {
            0..=7 => {
                let c = &mut self.ch[base + usize::from(reg >> 1)];
                let hi = self.flip[ctl];
                self.flip[ctl] = !hi;
                let set = |x: &mut u16| *x = if hi { (*x & 0x00FF) | (u16::from(v) << 8) } else { (*x & 0xFF00) | u16::from(v) };
                if reg & 1 == 0 {
                    set(&mut c.base_addr);
                    c.addr = c.base_addr;
                } else {
                    set(&mut c.base_count);
                    c.count = c.base_count;
                }
            }
            // Commande, requête logicielle : sans effet ici.
            8 | 9 => {}
            0x0A => self.ch[base + usize::from(v & 3)].masked = v & 4 != 0,
            0x0B => self.ch[base + usize::from(v & 3)].mode = v,
            0x0C => self.flip[ctl] = false,
            // Réinitialisation : tout masqué, bascule à zéro.
            0x0D => {
                for c in &mut self.ch[base..base + 4] {
                    c.masked = true;
                }
                self.flip[ctl] = false;
                self.tc[ctl] = 0;
            }
            0x0E => {
                for c in &mut self.ch[base..base + 4] {
                    c.masked = false;
                }
            }
            _ => {
                for (i, c) in self.ch[base..base + 4].iter_mut().enumerate() {
                    c.masked = v & (1 << i) != 0;
                }
            }
        }
    }

    fn write_page(&mut self, port: u16, v: u8) {
        let n = match port {
            0x87 => 0,
            0x83 => 1,
            0x81 => 2,
            0x82 => 3,
            0x8F => 4,
            0x8B => 5,
            0x89 => 6,
            0x8A => 7,
            _ => return,
        };
        self.ch[n].page = v;
    }

    fn read_port(&mut self, port: u16) -> u8 {
        let (ctl, reg) = match port {
            0x00..=0x0F => (0, port),
            0xC0..=0xDF => (1, (port - 0xC0) >> 1),
            _ => {
                let n = [0x87, 0x83, 0x81, 0x82, 0x8F, 0x8B, 0x89, 0x8A].iter().position(|&p| p == port);
                return n.map_or(0xFF, |n| self.ch[n].page);
            }
        };
        match reg {
            0..=7 => {
                let c = &self.ch[ctl * 4 + usize::from(reg >> 1)];
                let v = if reg & 1 == 0 { c.addr } else { c.count };
                let hi = self.flip[ctl];
                self.flip[ctl] = !hi;
                if hi { (v >> 8) as u8 } else { v as u8 }
            }
            // État : fins de comptage (lues une fois).
            8 => std::mem::take(&mut self.tc[ctl]),
            _ => 0xFF,
        }
    }
}

/// Ports 00h–0Fh, 80h–8Fh, C0h–DFh.
pub struct DmaPorts(pub Rc<RefCell<Dma>>);

impl IoDevice for DmaPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        u32::from(self.0.borrow_mut().read_port(port))
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        self.0.borrow_mut().write_port(port, value as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(d: &mut Dma, n: usize, addr: u32, count: u16, mode: u8) {
        let (ctl, (ap, cp), page) = if n < 4 {
            (0, (n as u16 * 2, n as u16 * 2 + 1), [0x87, 0x83, 0x81, 0x82][n])
        } else {
            (1, (0xC0 + (n as u16 - 4) * 4, 0xC2 + (n as u16 - 4) * 4), [0x8F, 0x8B, 0x89, 0x8A][n - 4])
        };
        let a = if n < 4 { addr as u16 } else { (addr >> 1) as u16 };
        let mask = if ctl == 0 { 0x0A } else { 0xD4 };
        d.write_port(mask, 4 | (n as u8 & 3));
        d.write_port(if ctl == 0 { 0x0C } else { 0xD8 }, 0);
        d.write_port(ap, a as u8);
        d.write_port(ap, (a >> 8) as u8);
        d.write_port(cp, count as u8);
        d.write_port(cp, (count >> 8) as u8);
        d.write_port(page, (addr >> 16) as u8);
        d.write_port(if ctl == 0 { 0x0B } else { 0xD6 }, mode | (n as u8 & 3));
        d.write_port(mask, n as u8 & 3);
    }

    #[test]
    fn auto_init_8_and_16_bits() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        mem.write(0x2_1000, &[1, 2, 3, 4]);
        let mut d = Dma::new();
        // Canal 1, 4 octets (compte 3), lecture, auto-init.
        program(&mut d, 1, 0x2_1000, 3, 0x58);
        let mut buf = [0u8; 6];
        assert_eq!(d.read_mem(1, &mem, &mut buf), (4, true));
        assert_eq!(&buf[..4], &[1, 2, 3, 4]);
        assert_eq!(d.read_mem(1, &mem, &mut buf[..2]), (2, false));
        assert_eq!(&buf[..2], &[1, 2]);
        assert_eq!(d.read_port(8) & 2, 2);
        // Canal 5 : adresse et compte en mots ; cycle unique puis masqué.
        program(&mut d, 5, 0x2_1000, 1, 0x48);
        assert_eq!(d.read_mem(5, &mem, &mut buf), (4, true));
        assert_eq!(&buf[..4], &[1, 2, 3, 4]);
        assert!(d.masked(5));
    }
}
