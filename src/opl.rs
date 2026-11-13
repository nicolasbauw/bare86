// SPDX-License-Identifier: GPL-3.0-or-later
//! FM OPL3 (YMF262) : synthèse par Nuked-OPL3 (third_party, LGPL-2.1+),
//! registres d'adresse des deux bancs, minuteries et registre d'état
//! (détection AdLib). Ports 388h–38Bh, et 220h–223h, 228h–229h de la
//! Sound Blaster. Chaque écriture est horodatée et appliquée à l'échantillon
//! correspondant lors du rendu, pour que la musique garde son rythme.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::io::IoDevice;

#[repr(C)]
struct Chip {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn dosbox_opl3_new(samplerate: u32) -> *mut Chip;
    fn dosbox_opl3_free(chip: *mut Chip);
    fn OPL3_WriteRegBuffered(chip: *mut Chip, reg: u16, v: u8);
    fn OPL3_GenerateResampled(chip: *mut Chip, buf: *mut i16);
}

#[derive(Clone, Copy, Default)]
struct Timer {
    /// Valeur de rechargement, période d'un pas (80 ou 320 µs).
    value: u8,
    step: Duration,
    /// Départ du comptage, si la minuterie tourne.
    started: Option<Instant>,
    masked: bool,
    expired: bool,
}

impl Timer {
    fn update(&mut self, now: Instant) {
        if let Some(t) = self.started
            && !self.masked
            && now >= t + self.step * (256 - u32::from(self.value))
        {
            self.expired = true;
        }
    }
}

pub struct Opl {
    chip: *mut Chip,
    rate: u32,
    address: u16,
    timers: [Timer; 2],
    /// Écritures en attente de rendu : instant, registre, valeur.
    pending: VecDeque<(Instant, u16, u8)>,
    /// Puce sollicitée au moins une fois (sinon le rendu est sauté).
    used: bool,
}

impl Opl {
    pub fn new(rate: u32) -> Self {
        // SAFETY: allocation C vérifiée ; libérée par Drop.
        let chip = unsafe { dosbox_opl3_new(rate) };
        assert!(!chip.is_null(), "OPL3 : allocation impossible");
        let t = |step| Timer { step: Duration::from_micros(step), ..Default::default() };
        Self { chip, rate, address: 0, timers: [t(80), t(320)], pending: VecDeque::new(), used: false }
    }

    fn status(&mut self, now: Instant) -> u8 {
        for t in &mut self.timers {
            t.update(now);
        }
        let (t1, t2) = (self.timers[0].expired, self.timers[1].expired);
        // Bits 2 et 1 à 0 : OPL3 (une OPL2 y rend 1 et 1).
        (u8::from(t1 || t2) << 7) | (u8::from(t1) << 6) | (u8::from(t2) << 5)
    }

    fn write_data(&mut self, v: u8, now: Instant) {
        match self.address {
            0x02 => self.timers[0].value = v,
            0x03 => self.timers[1].value = v,
            0x04 => {
                if v & 0x80 != 0 {
                    for t in &mut self.timers {
                        t.expired = false;
                    }
                } else {
                    for (i, t) in self.timers.iter_mut().enumerate() {
                        t.update(now);
                        t.masked = v & (0x40 >> i) != 0;
                        let run = v & (1 << i) != 0;
                        if run && t.started.is_none() {
                            t.started = Some(now);
                        } else if !run {
                            t.started = None;
                        }
                    }
                }
            }
            reg => {
                self.used = true;
                self.pending.push_back((now, reg, v));
            }
        }
    }

    /// Ajoute à `out` (stéréo entrelacée) le rendu des trames qui
    /// commencent à `t0`, en appliquant chaque écriture à son instant.
    pub fn render(&mut self, t0: Instant, gain: [f32; 2], out: &mut [f32]) {
        if !self.used {
            return;
        }
        let ns_per_frame = 1_000_000_000 / u64::from(self.rate);
        let mut buf = [0i16; 2];
        for (i, frame) in out.chunks_exact_mut(2).enumerate() {
            let t = t0 + Duration::from_nanos(i as u64 * ns_per_frame);
            while let Some(&(when, reg, v)) = self.pending.front()
                && when <= t
            {
                // SAFETY: puce valide.
                unsafe { OPL3_WriteRegBuffered(self.chip, reg, v) };
                self.pending.pop_front();
            }
            // SAFETY: puce valide, tampon de 2 échantillons.
            unsafe { OPL3_GenerateResampled(self.chip, buf.as_mut_ptr()) };
            frame[0] += f32::from(buf[0]) / 32768.0 * gain[0];
            frame[1] += f32::from(buf[1]) / 32768.0 * gain[1];
        }
    }
}

impl Drop for Opl {
    fn drop(&mut self) {
        // SAFETY: allouée par dosbox_opl3_new.
        unsafe { dosbox_opl3_free(self.chip) };
    }
}

/// Ports de la FM : adresse (pair) et donnée (impair), banc 1 en 38Ah/222h.
pub struct OplPorts(pub std::rc::Rc<std::cell::RefCell<Opl>>);

impl IoDevice for OplPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        if port & 1 == 0 { u32::from(self.0.borrow_mut().status(Instant::now())) } else { 0xFF }
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let mut o = self.0.borrow_mut();
        let v = value as u8;
        let bank1 = matches!(port, 0x38A | 0x38B | 0x222 | 0x223);
        if port & 1 == 0 {
            o.address = u16::from(v) | if bank1 { 0x100 } else { 0 };
        } else {
            o.write_data(v, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adlib_detection_and_a_note() {
        let mut o = Opl::new(48_000);
        let t = Instant::now();
        let w = |o: &mut Opl, reg: u16, v: u8| {
            o.address = reg;
            o.write_data(v, t);
        };
        // Détection classique : remise à zéro des minuteries, état nul ;
        // minuterie 1 à FFh lancée : expirée après 80 µs.
        w(&mut o, 4, 0x60);
        w(&mut o, 4, 0x80);
        assert_eq!(o.status(t) & 0xE0, 0);
        w(&mut o, 2, 0xFF);
        w(&mut o, 4, 0x21);
        assert_eq!(o.status(t + Duration::from_micros(100)) & 0xE0, 0xC0);
        // Une note : opérateurs de la voie 0, la 440 Hz environ, touche enfoncée.
        for (reg, v) in [(0x20, 0x01), (0x23, 0x01), (0x40, 0x10), (0x43, 0x00), (0x60, 0xF0), (0x63, 0xF0), (0x80, 0x77), (0x83, 0x77), (0xC0, 0x30), (0xA0, 0x98), (0xB0, 0x31)] {
            w(&mut o, reg, v);
        }
        let mut out = vec![0f32; 2 * 4800];
        o.render(t, [1.0, 1.0], &mut out);
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "crête {peak}");
    }
}
