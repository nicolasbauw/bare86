// SPDX-License-Identifier: GPL-3.0-or-later
//! Registres VGA minimaux en attendant l'étape 5 : état d'entrée 1
//! (3DAh / 3BAh : retours de ligne et de trame cadencés sur l'horloge hôte,
//! 70 Hz, 31,5 kHz), registres du CRTC (3D4h/3D5h, 3B4h/3B5h) et de la CGA
//! (3D8h, 3D9h) conservés.

use std::time::Instant;

use crate::io::IoDevice;

/// Durée d'une trame à 70 Hz et du retour de trame (2 lignes de 31,778 µs… arrondi).
const FRAME_NS: u128 = 14_285_714;
const VRETRACE_NS: u128 = 64 * 31_778 / 32;
const LINE_NS: u128 = 31_778;
const HBLANK_NS: u128 = 6_356;

pub struct VgaPorts {
    start: Instant,
    crtc_index: u8,
    crtc: [u8; 32],
    cga_mode: u8,
    cga_color: u8,
}

impl VgaPorts {
    pub fn new() -> Self {
        Self { start: Instant::now(), crtc_index: 0, crtc: [0; 32], cga_mode: 0x29, cga_color: 0x30 }
    }

    fn status(&self) -> u8 {
        let t = self.start.elapsed().as_nanos();
        let in_frame = t % FRAME_NS;
        let vretrace = in_frame >= FRAME_NS - VRETRACE_NS;
        let hblank = t % LINE_NS >= LINE_NS - HBLANK_NS;
        // Bit 0 : affichage inactif (retour de ligne ou de trame) ; bit 3 : retour de trame.
        u8::from(vretrace || hblank) | if vretrace { 0x08 } else { 0 }
    }
}

impl IoDevice for VgaPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        u32::from(match port & 0xFF {
            0xBA | 0xDA => self.status(),
            0xB4 | 0xD4 => self.crtc_index,
            0xB5 | 0xD5 => self.crtc[usize::from(self.crtc_index & 31)],
            0xD8 => self.cga_mode,
            0xD9 => self.cga_color,
            _ => 0xFF,
        })
    }

    fn write(&mut self, port: u16, size: u8, value: u32) {
        let v = value as u8;
        match port & 0xFF {
            // Écriture 16 bits sur l'index : index puis donnée.
            0xB4 | 0xD4 => {
                self.crtc_index = v;
                if size == 2 {
                    self.crtc[usize::from(v & 31)] = (value >> 8) as u8;
                }
            }
            0xB5 | 0xD5 => self.crtc[usize::from(self.crtc_index & 31)] = v,
            0xD8 => self.cga_mode = v,
            0xD9 => self.cga_color = v,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrace_toggles() {
        let v = VgaPorts::new();
        let deadline = Instant::now() + std::time::Duration::from_millis(100);
        let (mut on, mut off) = (false, false);
        while Instant::now() < deadline && !(on && off) {
            if v.status() & 8 != 0 { on = true } else { off = true }
        }
        assert!(on && off);
    }
}
