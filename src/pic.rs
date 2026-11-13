// SPDX-License-Identifier: GPL-3.0-or-later
//! Contrôleur d'interruptions : deux 8259A en cascade (maître 20h/21h,
//! esclave A0h/A1h sur l'IRQ 2), déclenchement sur front, mode
//! entièrement imbriqué.

use std::cell::RefCell;
use std::rc::Rc;

use crate::io::IoDevice;

#[derive(Default)]
struct Chip {
    irr: u8,
    isr: u8,
    imr: u8,
    /// Vecteur de l'IRQ 0 de la puce (ICW2).
    base: u8,
    /// Étape d'initialisation : prochain ICW attendu (0 = initialisée).
    icw: u8,
    needs_icw4: bool,
    single: bool,
    auto_eoi: bool,
    read_isr: bool,
}

impl Chip {
    fn new(base: u8, imr: u8) -> Self {
        Self { base, imr, ..Self::default() }
    }

    fn write_cmd(&mut self, v: u8) {
        if v & 0x10 != 0 {
            // ICW1 : réinitialisation.
            self.icw = 2;
            self.needs_icw4 = v & 1 != 0;
            self.single = v & 2 != 0;
            self.imr = 0;
            self.isr = 0;
            self.irr = 0;
            self.read_isr = false;
            self.auto_eoi = false;
        } else if v & 0x08 != 0 {
            // OCW3 : registre lu sur le port de commande.
            if v & 2 != 0 {
                self.read_isr = v & 1 != 0;
            }
        } else {
            // OCW2 : fin d'interruption (les rotations sont traitées comme des EOI).
            match v >> 5 {
                0b011 | 0b111 => self.isr &= !(1 << (v & 7)),
                0b001 | 0b101 if self.isr != 0 => self.isr &= !(1 << self.isr.trailing_zeros()),
                _ => {}
            }
        }
    }

    fn write_data(&mut self, v: u8) {
        match self.icw {
            2 => {
                self.base = v & 0xF8;
                self.icw = if !self.single { 3 } else if self.needs_icw4 { 4 } else { 0 };
            }
            3 => self.icw = if self.needs_icw4 { 4 } else { 0 },
            4 => {
                self.auto_eoi = v & 2 != 0;
                self.icw = 0;
            }
            _ => self.imr = v,
        }
    }

    /// IRQ (0–7) à servir : demandée, non masquée, plus prioritaire que celles en service.
    fn pending(&self) -> Option<u8> {
        let req = self.irr & !self.imr;
        if req == 0 {
            return None;
        }
        let irq = req.trailing_zeros() as u8;
        let in_service = if self.isr == 0 { 8 } else { self.isr.trailing_zeros() as u8 };
        (irq < in_service).then_some(irq)
    }

    fn ack(&mut self, irq: u8) {
        self.irr &= !(1 << irq);
        if !self.auto_eoi {
            self.isr |= 1 << irq;
        }
    }
}

pub struct Pic {
    master: Chip,
    slave: Chip,
}

impl Pic {
    /// État laissé par le BIOS : vecteurs 08h et 70h, IRQ 0, 1 et 2 ouvertes.
    pub fn new() -> Self {
        Self { master: Chip::new(0x08, 0xF8), slave: Chip::new(0x70, 0xFF) }
    }

    /// Front montant sur l'IRQ 0–15.
    pub fn raise(&mut self, irq: u8) {
        if irq < 8 {
            self.master.irr |= 1 << irq;
        } else {
            self.slave.irr |= 1 << (irq - 8);
        }
    }

    fn slave_wants(&self) -> bool {
        self.slave.pending().is_some()
    }

    /// Vecteur de la prochaine interruption à servir, sans l'acquitter.
    pub fn pending(&self) -> Option<u8> {
        let m = self.master.clone_for_cascade(self.slave_wants());
        let irq = m.pending()?;
        if irq == 2 && self.slave_wants() {
            return self.slave.pending().map(|s| self.slave.base + s);
        }
        Some(self.master.base + irq)
    }

    /// Acquitte la prochaine interruption (cycle INTA) : son vecteur.
    pub fn acknowledge(&mut self) -> Option<u8> {
        let slave = self.slave_wants();
        let m = self.master.clone_for_cascade(slave);
        let irq = m.pending()?;
        if irq == 2 && slave {
            let s = self.slave.pending()?;
            self.slave.ack(s);
            self.master.ack(2);
            return Some(self.slave.base + s);
        }
        self.master.ack(irq);
        Some(self.master.base + irq)
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }
}

impl Chip {
    /// Copie dont l'IRQ 2 reflète la demande de l'esclave.
    fn clone_for_cascade(&self, slave: bool) -> Chip {
        let mut c = Chip { irr: self.irr, isr: self.isr, imr: self.imr, base: self.base, ..Chip::default() };
        if slave {
            c.irr |= 1 << 2;
        }
        c
    }
}

/// Ports 20h/21h et A0h/A1h.
pub struct PicPorts(pub Rc<RefCell<Pic>>);

impl IoDevice for PicPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        let p = self.0.borrow();
        let chip = if port & 0x80 != 0 { &p.slave } else { &p.master };
        u32::from(if port & 1 != 0 {
            chip.imr
        } else if chip.read_isr {
            chip.isr
        } else {
            chip.irr
        })
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let mut p = self.0.borrow_mut();
        let chip = if port & 0x80 != 0 { &mut p.slave } else { &mut p.master };
        if port & 1 != 0 { chip.write_data(value as u8) } else { chip.write_cmd(value as u8) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_cascade_and_eoi() {
        let mut p = Pic::new();
        p.slave.imr = 0;
        p.raise(1);
        p.raise(0);
        assert_eq!(p.acknowledge(), Some(0x08));
        // IRQ 1 attend l'EOI de l'IRQ 0, plus prioritaire.
        assert_eq!(p.pending(), None);
        p.master.write_cmd(0x20);
        assert_eq!(p.acknowledge(), Some(0x09));
        p.master.write_cmd(0x20);
        p.raise(12);
        assert_eq!(p.acknowledge(), Some(0x74));
        p.slave.write_cmd(0x20);
        p.master.write_cmd(0x20);
        // Masquage, puis reprogrammation (ICW1–ICW4) des vecteurs.
        p.master.write_data(0xFF);
        p.raise(0);
        assert_eq!(p.pending(), None);
        for (cmd, v) in [(true, 0x11), (false, 0x20), (false, 0x04), (false, 0x01)] {
            if cmd { p.master.write_cmd(v) } else { p.master.write_data(v) }
        }
        p.raise(0);
        assert_eq!(p.acknowledge(), Some(0x20));
    }
}
