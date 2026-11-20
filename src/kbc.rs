// SPDX-License-Identifier: GPL-3.0-or-later
//! Contrôleur clavier 8042 (ports 60h, 64h) et clavier attaché : tampon de
//! sortie, IRQ 1, commandes du contrôleur et du clavier. Les scancodes
//! (jeu 1, déjà traduits) arrivent de l'hôte par `feed`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::io::IoDevice;

pub struct Kbc {
    /// Octets prêts à être lus sur le port 60h, le premier dans le tampon.
    out: VecDeque<u8>,
    /// Octet de commande (20h/60h) : bit 0 IRQ 1, bit 4 clavier désactivé.
    command_byte: u8,
    /// Port de sortie (D0h/D1h) : bit 1 = porte A20.
    pub output_port: u8,
    /// Commande en attente de son octet de données (60h, D1h, ou clavier).
    pending: Option<Pending>,
    /// Nouvel octet dans le tampon : IRQ 1 à lever.
    irq: bool,
    /// Dernier octet lu : le registre de données le garde après la lecture
    /// (tampon vide). Un gestionnaire d'IRQ 1 qui lit le port 60h puis passe
    /// la main à l'INT 09h du BIOS (HX, beaucoup de jeux) compte dessus.
    last: u8,
    /// Réinitialisation du processeur demandée (impulsion FEh, bit 0 du
    /// port de sortie ou du port 92h).
    pub reset: bool,
}

#[derive(Clone, Copy)]
enum Pending {
    CommandByte,
    OutputPort,
    /// Commande clavier qui attend son paramètre (LED, répétition, jeu).
    KeyboardParam,
}

/// Dernier octet lu au port 60h avant toute frappe : le relâchement
/// d'Entrée qui a validé la ligne de commande, comme sur un vrai PC. Avec
/// 00h (code d'appui, bit 7 à zéro), les démos qui guettent une touche au
/// port 60h quittaient aussitôt (Fukwit Daddy).
const ENTER_RELEASED: u8 = 0x9C;

impl Kbc {
    pub fn new() -> Self {
        Self { out: VecDeque::new(), command_byte: 0x45, output_port: 0xCD, pending: None, irq: false, last: ENTER_RELEASED, reset: false }
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    fn push(&mut self, b: u8) {
        let was_empty = self.out.is_empty();
        self.out.push_back(b);
        if was_empty {
            self.irq = true;
        }
    }

    /// Tampon de sortie libre : un nouveau scancode peut être envoyé.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Scancode venu du clavier (ignoré si le clavier est désactivé).
    pub fn feed(&mut self, scancode: u8) {
        if self.command_byte & 0x10 == 0 {
            self.push(scancode);
        }
    }

    /// IRQ 1 à lever (octet arrivé dans un tampon vide, IRQ autorisée).
    pub fn take_irq(&mut self) -> bool {
        let irq = self.irq && self.command_byte & 1 != 0;
        self.irq = false;
        irq
    }

    /// Porte A20 (bit 1 du port de sortie), fermée au démarrage comme
    /// après le POST d'un AT. Le port 92h, XMS et l'INT 15h passent par là.
    pub fn a20(&self) -> bool {
        self.output_port & 2 != 0
    }

    pub fn set_a20(&mut self, open: bool) {
        self.output_port = (self.output_port & !2) | if open { 2 } else { 0 };
    }

    fn status(&self) -> u8 {
        // Bit 0 : tampon plein ; bit 2 : système ; bit 3 : dernier octet = commande ;
        // bit 4 : clavier non verrouillé.
        u8::from(!self.out.is_empty()) | 0x04 | 0x10
    }

    fn read_data(&mut self) -> u8 {
        let Some(b) = self.out.pop_front() else { return self.last };
        self.last = b;
        if !self.out.is_empty() {
            self.irq = true;
        }
        b
    }

    fn controller_command(&mut self, cmd: u8) {
        match cmd {
            0x20 => self.push(self.command_byte),
            0x60 => self.pending = Some(Pending::CommandByte),
            0xA7 | 0xA8 => {}
            // Tests du contrôleur et de l'interface clavier : réussis.
            0xAA => self.push(0x55),
            0xA9 | 0xAB => self.push(0x00),
            0xAD => self.command_byte |= 0x10,
            0xAE => self.command_byte &= !0x10,
            0xC0 => self.push(0xBF),
            0xD0 => self.push(self.output_port),
            0xD1 => self.pending = Some(Pending::OutputPort),
            // A20 rapide (contrôleurs récents).
            0xDD => self.output_port &= !2,
            0xDF => self.output_port |= 2,
            // Impulsions sur le port de sortie : bit 0 = réinitialisation.
            0xF0..=0xFF if cmd & 1 == 0 => self.reset = true,
            _ => {}
        }
    }

    fn keyboard_command(&mut self, b: u8) {
        if let Some(Pending::KeyboardParam) = self.pending.take() {
            self.push(0xFA);
            return;
        }
        match b {
            // LED, répétition, jeu de scancodes : ACK puis paramètre.
            0xED | 0xF3 | 0xF0 => {
                self.push(0xFA);
                self.pending = Some(Pending::KeyboardParam);
            }
            // Identification : clavier MF2.
            0xF2 => {
                self.push(0xFA);
                self.push(0xAB);
                self.push(0x41);
            }
            0xEE => self.push(0xEE),
            // Réinitialisation : ACK, test réussi.
            0xFF => {
                self.push(0xFA);
                self.push(0xAA);
            }
            _ => self.push(0xFA),
        }
    }

    fn write_data(&mut self, b: u8) {
        match self.pending {
            Some(Pending::CommandByte) => {
                self.pending = None;
                self.command_byte = b;
            }
            Some(Pending::OutputPort) => {
                self.pending = None;
                self.output_port = b;
                self.reset |= b & 1 == 0;
            }
            _ => self.keyboard_command(b),
        }
    }
}

/// Ports 60h et 64h.
pub struct KbcPorts(pub Rc<RefCell<Kbc>>);

impl IoDevice for KbcPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        let mut k = self.0.borrow_mut();
        u32::from(match port {
            0x60 => k.read_data(),
            // Port de contrôle système A (« fast A20 ») : bit 1 = A20.
            0x92 => k.output_port & 2,
            _ => k.status(),
        })
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let mut k = self.0.borrow_mut();
        match port {
            0x60 => k.write_data(value as u8),
            // Bit 0 : réinitialisation rapide.
            0x92 => {
                k.output_port = (k.output_port & !2) | (value as u8 & 2);
                k.reset |= value & 1 != 0;
            }
            _ => k.controller_command(value as u8),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_and_commands() {
        let mut k = Kbc::new();
        k.feed(0x1E);
        k.feed(0x9E);
        assert!(k.take_irq());
        assert_eq!(k.status() & 1, 1);
        assert_eq!(k.read_data(), 0x1E);
        assert!(k.take_irq());
        assert_eq!(k.read_data(), 0x9E);
        assert!(!k.take_irq());
        k.controller_command(0xAA);
        assert_eq!(k.read_data(), 0x55);
        // A20 par le port de sortie.
        k.controller_command(0xD1);
        k.write_data(0xCD);
        assert!(!k.a20());
        // LED : ACK pour la commande et pour le paramètre.
        k.write_data(0xED);
        k.write_data(0x02);
        assert_eq!((k.read_data(), k.read_data()), (0xFA, 0xFA));
    }

    /// Le registre de données garde le dernier octet : un gestionnaire d'IRQ 1
    /// qui lit le port 60h puis chaîne sur l'INT 09h du BIOS (HX) voit le
    /// même scancode deux fois, préfixe E0h compris.
    #[test]
    fn data_survives_read() {
        let mut k = Kbc::new();
        k.feed(0xE0);
        assert_eq!((k.read_data(), k.status() & 1), (0xE0, 0));
        assert_eq!(k.read_data(), 0xE0);
        k.feed(0x48);
        assert_eq!((k.read_data(), k.read_data()), (0x48, 0x48));
    }
}
