// SPDX-License-Identifier: GPL-3.0-or-later
//! Port de jeu (201h) : deux manches de deux axes, quatre boutons. Une
//! écriture déclenche les quatre monostables ; le bit de chaque axe reste
//! à 1 pendant une durée qui croît avec la position (24 µs à 1,1 ms, comme
//! une résistance de 0 à 100 kΩ). Sans manche, il reste à 1 : c'est ainsi
//! que les jeux concluent à l'absence de joystick. Bits 4 à 7 : boutons,
//! à 0 quand ils sont enfoncés. Les positions viennent de l'hôte (SDL).

use std::time::{Duration, Instant};

use crate::io::IoDevice;

const MIN: Duration = Duration::from_nanos(24_200);
const SPAN: Duration = Duration::from_micros(1_100);

#[derive(Default)]
pub struct Gameport {
    /// Manche branché (axes 0–1 : manche A ; 2–3 : manche B).
    present: [bool; 2],
    /// Positions, de 0 (gauche, haut) à 1.
    axes: [f32; 4],
    /// Boutons enfoncés (bit 0 : A1, 1 : A2, 2 : B1, 3 : B2).
    buttons: u8,
    fired: Option<Instant>,
}

impl Gameport {
    pub fn new() -> Self {
        Self { axes: [0.5; 4], ..Default::default() }
    }

    /// Manche branché ou débranché ; `sticks` : 1 (axes 0–1) ou 2.
    pub fn set_present(&mut self, sticks: u8) {
        self.present = [sticks >= 1, sticks >= 2];
        if sticks == 0 {
            self.buttons = 0;
        }
    }

    /// Axe `i` à la valeur SDL `v` (−32768 à 32767).
    pub fn set_axis(&mut self, i: usize, v: i16) {
        if let Some(a) = self.axes.get_mut(i) {
            *a = (f32::from(v) + 32768.0) / 65535.0;
        }
    }

    pub fn set_button(&mut self, i: usize, down: bool) {
        if i < 4 {
            let bit = 1 << i;
            self.buttons = if down { self.buttons | bit } else { self.buttons & !bit };
        }
    }

    fn duration(&self, axis: usize) -> Duration {
        MIN + SPAN.mul_f32(self.axes[axis])
    }

    pub fn read(&self, now: Instant) -> u8 {
        let mut v = (!self.buttons & 0x0F) << 4;
        for axis in 0..4 {
            let timing = match self.fired {
                Some(t) if self.present[axis / 2] => now < t + self.duration(axis),
                // Sans manche : le monostable ne retombe jamais.
                _ => !self.present[axis / 2],
            };
            v |= u8::from(timing) << axis;
        }
        v
    }

    /// INT 15h AH=84h : boutons (bits 4–7, à 0 enfoncés) ou positions
    /// (comptes de 0 à ≈ 255 ; 0 sans manche).
    pub fn bios_buttons(&self) -> u8 {
        (!self.buttons & 0x0F) << 4
    }

    pub fn bios_positions(&self) -> [u16; 4] {
        let mut out = [0u16; 4];
        for (i, o) in out.iter_mut().enumerate() {
            if self.present[i / 2] {
                *o = (self.axes[i] * 250.0) as u16 + 5;
            }
        }
        out
    }
}

pub struct GameportPorts(pub std::rc::Rc<std::cell::RefCell<Gameport>>);

impl IoDevice for GameportPorts {
    fn read(&mut self, _: u16, _: u8) -> u32 {
        u32::from(self.0.borrow().read(Instant::now()))
    }

    fn write(&mut self, _: u16, _: u8, _: u32) {
        self.0.borrow_mut().fired = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shots_and_buttons() {
        let mut g = Gameport::new();
        let t = Instant::now();
        // Sans manche : axes à 1, boutons relâchés.
        assert_eq!(g.read(t), 0xFF);
        g.set_present(1);
        g.set_axis(0, -32768); // à gauche : 24 µs
        g.set_axis(1, 32767); // en bas : ≈ 1,1 ms
        g.set_button(0, true);
        g.fired = Some(t);
        let at = |us| t + Duration::from_micros(us);
        // Manche B absent : bits 2 et 3 à 1 ; bouton A1 enfoncé : bit 4 à 0.
        assert_eq!(g.read(at(10)), 0b1110_1111);
        assert_eq!(g.read(at(100)), 0b1110_1110);
        assert_eq!(g.read(at(2000)), 0b1110_1100);
        assert_eq!(g.bios_positions()[0], 5);
    }
}
