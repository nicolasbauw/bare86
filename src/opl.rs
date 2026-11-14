// SPDX-License-Identifier: GPL-3.0-or-later
//! FM : OPL3 (YMF262, Nuked-OPL3, third_party/nuked-opl3), OPL2 (YM3812,
//! Nuked OPL2 Lite de third_party/libadlmidi), double OPL2 (SB Pro 1 :
//! une puce par voie) ou CQM de Creative (Nuked CQM) ; LGPL-2.1+. Registres d'adresse, minuteries et
//! registre d'état (détection AdLib ; une OPL2 y rend 06h dans les bits
//! bas). Ports 388h–38Bh, et 220h–223h, 228h–229h de la Sound Blaster. Côté vCPU (`Opl`) : adresse, minuteries, état, et
//! écritures horodatées ; côté thread audio (`FmSynth`) : la puce, qui
//! applique chaque écriture à l'échantillon de son instant, pour que la
//! musique garde son rythme.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::io::IoDevice;

#[repr(C)]
struct Chip {
    _opaque: [u8; 0],
}

#[repr(C)]
struct Chip2 {
    _opaque: [u8; 0],
}

#[repr(C)]
struct ChipCqm {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn bare86_cqm_new(samplerate: u32) -> *mut ChipCqm;
    fn bare86_cqm_free(chip: *mut ChipCqm);
    fn CQM_WriteRegBuffered(chip: *mut ChipCqm, reg: u16, v: u8);
    fn CQM_GenerateResampled(chip: *mut ChipCqm, buf: *mut i16);
    fn bare86_opl2_new(samplerate: u32) -> *mut Chip2;
    fn bare86_opl2_free(chip: *mut Chip2);
    fn OPL2_WriteRegBuffered(chip: *mut Chip2, reg: u8, v: u8);
    fn OPL2_GenerateResampled(chip: *mut Chip2, buf: *mut i16);
    fn bare86_opl3_new(samplerate: u32) -> *mut Chip;
    fn bare86_opl3_free(chip: *mut Chip);
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

/// Écriture de registre horodatée : instant, registre, valeur. OPL3 : banc
/// 1 = 100h. OPL2 : puces visées dans les bits 8–9 (1 gauche, 2 droite, 3
/// les deux).
pub type Write = (Instant, u16, u8);

/// Puce FM de la carte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OplModel {
    /// YMF262 (SB Pro 2, SB16).
    Opl3,
    /// YM3812 seule, mono (AdLib, SB 1.x/2.0).
    Opl2,
    /// Deux YM3812, une par voie (SB Pro 1).
    DualOpl2,
    /// Synthèse « CQM » de Creative, compatible OPL3 (SB16 Vibra, AWE64) :
    /// le son métallique de nombreuses cartes Creative tardives.
    Cqm,
}

impl std::str::FromStr for OplModel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "opl3" => OplModel::Opl3,
            "opl2" => OplModel::Opl2,
            "dualopl2" => OplModel::DualOpl2,
            "cqm" => OplModel::Cqm,
            _ => return Err(format!("« {s} » : opl2, dualopl2, opl3 ou cqm attendu")),
        })
    }
}

/// Partie de la puce vue par l'invité, sur le thread du vCPU.
pub struct Opl {
    pub model: OplModel,
    /// Horloge de la puce : au moins l'heure réelle, et 1 µs de plus par
    /// accès (cycle du bus ISA). Les boucles d'attente des pilotes, qui
    /// comptent des lectures de l'état, durent ainsi ce qu'elles duraient
    /// sur une vraie carte, même servies en rafale (≈ 70 ns l'accès).
    clock: Instant,
    address: u16,
    timers: [Timer; 2],
    /// Écritures pas encore confiées au thread audio.
    writes: Vec<Write>,
}

impl Default for Opl {
    fn default() -> Self {
        Self::new()
    }
}

impl Opl {
    pub fn new() -> Self {
        let t = |step| Timer { step: Duration::from_micros(step), ..Default::default() };
        Self { model: OplModel::Opl3, clock: Instant::now(), address: 0, timers: [t(80), t(320)], writes: Vec::new() }
    }

    /// Heure de la puce pour un accès (voir `clock`).
    fn tick(&mut self) -> Instant {
        const ISA_CYCLE: Duration = Duration::from_micros(1);
        self.clock = (self.clock + ISA_CYCLE).max(Instant::now());
        self.clock
    }

    /// Écritures en attente, pour le thread audio.
    pub fn take_writes(&mut self) -> Vec<Write> {
        std::mem::take(&mut self.writes)
    }

    fn status(&mut self, now: Instant) -> u8 {
        for t in &mut self.timers {
            t.update(now);
        }
        let (t1, t2) = (self.timers[0].expired, self.timers[1].expired);
        // Bits 2 et 1 : 0 sur une OPL3, 1 et 1 sur une OPL2 (détection).
        let opl2 = if matches!(self.model, OplModel::Opl3 | OplModel::Cqm) { 0 } else { 0x06 };
        (u8::from(t1 || t2) << 7) | (u8::from(t1) << 6) | (u8::from(t2) << 5) | opl2
    }

    fn write_data(&mut self, v: u8, now: Instant, real: Instant) {
        // Minuteries : registres 02h–04h du premier banc (OPL3), de toute
        // puce visée (OPL2 : bits 8–9 = puces).
        let timer_reg = if matches!(self.model, OplModel::Opl3 | OplModel::Cqm) { self.address } else { self.address & 0xFF };
        match timer_reg {
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
            _ => self.writes.push((real, self.address, v)),
        }
    }
}

/// Puces de la carte, sur le thread audio.
enum Chips {
    Opl3(*mut Chip),
    Cqm(*mut ChipCqm),
    /// Une OPL2 (mono) ou deux (gauche, droite).
    Opl2(Vec<*mut Chip2>),
}

/// La synthèse FM (Nuked), sur le thread audio.
pub struct FmSynth {
    chips: Chips,
    rate: u32,
    /// Écritures reçues, pas encore appliquées.
    pending: VecDeque<Write>,
    /// Puce sollicitée au moins une fois (sinon le rendu est sauté).
    used: bool,
}

// SAFETY: les puces ne sont accédées que par leur propriétaire, sur un seul thread à la fois.
unsafe impl Send for FmSynth {}

impl FmSynth {
    pub fn new(rate: u32, model: OplModel) -> Self {
        // SAFETY: allocations C vérifiées ; libérées par Drop.
        let chips = unsafe {
            match model {
                OplModel::Opl3 => {
                    let c = bare86_opl3_new(rate);
                    assert!(!c.is_null(), "OPL3 : allocation impossible");
                    Chips::Opl3(c)
                }
                OplModel::Cqm => {
                    let c = bare86_cqm_new(rate);
                    assert!(!c.is_null(), "CQM : allocation impossible");
                    Chips::Cqm(c)
                }
                OplModel::Opl2 | OplModel::DualOpl2 => {
                    let n = if model == OplModel::Opl2 { 1 } else { 2 };
                    let v: Vec<*mut Chip2> = (0..n).map(|_| bare86_opl2_new(rate)).collect();
                    assert!(v.iter().all(|c| !c.is_null()), "OPL2 : allocation impossible");
                    Chips::Opl2(v)
                }
            }
        };
        Self { chips, rate, pending: VecDeque::new(), used: false }
    }

    pub fn queue(&mut self, writes: Vec<Write>) {
        self.used |= !writes.is_empty();
        self.pending.extend(writes);
    }

    fn apply(&mut self, reg: u16, v: u8) {
        // SAFETY: puces valides.
        unsafe {
            match &self.chips {
                Chips::Opl3(c) => OPL3_WriteRegBuffered(*c, reg, v),
                Chips::Cqm(c) => CQM_WriteRegBuffered(*c, reg, v),
                Chips::Opl2(v2) => {
                    for (i, c) in v2.iter().enumerate() {
                        // Une seule puce : elle reçoit tout ce qui lui est destiné.
                        let target = if v2.len() == 1 { 1 } else { 1 << i };
                        if (reg >> 8) & target != 0 {
                            OPL2_WriteRegBuffered(*c, reg as u8, v);
                        }
                    }
                }
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
                self.apply(reg, v);
                self.pending.pop_front();
            }
            // SAFETY: puces valides, tampon de 2 échantillons.
            unsafe {
                match &self.chips {
                    Chips::Opl3(c) => OPL3_GenerateResampled(*c, buf.as_mut_ptr()),
                    // Niveau tel quel : proche de l'OPL3 dans notre mélange
                    // (Doom : efficace ≈ 640 contre 540 ; libADLMIDI le divise
                    // par deux pour son propre mélange).
                    Chips::Cqm(c) => CQM_GenerateResampled(*c, buf.as_mut_ptr()),
                    Chips::Opl2(v2) => {
                        OPL2_GenerateResampled(v2[0], buf.as_mut_ptr());
                        if let Some(&right) = v2.get(1) {
                            OPL2_GenerateResampled(right, buf.as_mut_ptr().add(1));
                        } else {
                            buf[1] = buf[0];
                        }
                    }
                }
            }
            frame[0] += f32::from(buf[0]) / 32768.0 * gain[0];
            frame[1] += f32::from(buf[1]) / 32768.0 * gain[1];
        }
    }
}

impl Drop for FmSynth {
    fn drop(&mut self) {
        // SAFETY: puces allouées par bare86_opl3_new / bare86_opl2_new.
        unsafe {
            match &self.chips {
                Chips::Opl3(c) => bare86_opl3_free(*c),
                Chips::Cqm(c) => bare86_cqm_free(*c),
                Chips::Opl2(v) => v.iter().for_each(|c| bare86_opl2_free(*c)),
            }
        }
    }
}

/// Ports de la FM : adresse (pair) et donnée (impair), banc 1 en 38Ah/222h.
pub struct OplPorts(pub std::rc::Rc<std::cell::RefCell<Opl>>);

impl IoDevice for OplPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        let mut o = self.0.borrow_mut();
        let now = o.tick();
        if port & 1 == 0 { u32::from(o.status(now)) } else { 0xFF }
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let mut o = self.0.borrow_mut();
        let v = value as u8;
        let bank1 = matches!(port, 0x38A | 0x38B | 0x222 | 0x223);
        let now = o.tick();
        if port & 1 == 0 {
            o.address = u16::from(v)
                | match o.model {
                    OplModel::Opl3 | OplModel::Cqm => u16::from(bank1) << 8,
                    // Une seule puce : le second banc n'existe pas (écritures
                    // perdues, faute de puce visée).
                    OplModel::Opl2 => if bank1 { 0 } else { 0x100 },
                    // SB Pro 1 : 220h gauche, 222h (et 38Ah) droite, 228h et
                    // 388h les deux.
                    OplModel::DualOpl2 => match port {
                        0x220 => 0x100,
                        0x222 | 0x38A => 0x200,
                        _ => 0x300,
                    },
                };
        } else {
            // Minuteries à l'heure de la puce ; musique à l'heure réelle.
            o.write_data(v, now, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adlib_detection_and_a_note() {
        let mut o = Opl::new();
        let t = Instant::now();
        let w = |o: &mut Opl, reg: u16, v: u8| {
            o.address = reg;
            o.write_data(v, t, t);
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
        let mut fm = FmSynth::new(48_000, OplModel::Opl3);
        fm.queue(o.take_writes());
        let mut out = vec![0f32; 2 * 4800];
        fm.render(t, [1.0, 1.0], &mut out);
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "crête {peak}");
    }

    /// Hauteur juste à 48 kHz (la puce tourne à 49 716 Hz) : sinusoïde pure
    /// (modulateur éteint), F-number 408, bloc 4 → 408 × 49 716 / 2^16 ≈ 309,5 Hz.
    #[test]
    fn pitch_at_48_khz() {
        let t = Instant::now();
        let mut o = Opl::new();
        let w = |o: &mut Opl, reg: u16, v: u8| {
            o.address = reg;
            o.write_data(v, t, t);
        };
        for (reg, v) in [(0x20, 0x01), (0x23, 0x01), (0x40, 0x3F), (0x43, 0x00), (0x60, 0xF0), (0x63, 0xF0), (0x80, 0x0F), (0x83, 0x0F), (0xC0, 0x30), (0xA0, 0x98), (0xB0, 0x31)] {
            w(&mut o, reg, v);
        }
        let mut fm = FmSynth::new(48_000, OplModel::Opl3);
        fm.queue(o.take_writes());
        let mut out = vec![0f32; 2 * 48_000];
        fm.render(t, [1.0, 1.0], &mut out);
        // Passages par zéro montants sur la dernière demi-seconde (après l'attaque).
        let left: Vec<f32> = out.iter().step_by(2).copied().skip(24_000).collect();
        let ups = left.windows(2).filter(|p| p[0] < 0.0 && p[1] >= 0.0).count();
        let freq = ups as f64 / 0.5;
        assert!((freq - 309.5).abs() < 3.0, "{freq} Hz");
    }

    /// Écrit une sinusoïde tenue (F-number 408, bloc 4) par les ports `addr` et
    /// `addr + 1`, rend une seconde : (fréquence à gauche, crête droite).
    fn opl2_note(model: OplModel, addr: u16) -> (f64, f32) {
        let o = std::rc::Rc::new(std::cell::RefCell::new(Opl::new()));
        o.borrow_mut().model = model;
        let mut ports = OplPorts(o.clone());
        for (reg, v) in [(0x20, 0x21), (0x23, 0x21), (0x40, 0x3F), (0x43, 0x00), (0x60, 0xF0), (0x63, 0xF0), (0x80, 0x0F), (0x83, 0x0F), (0xC0, 0x30), (0xA0, 0x98), (0xB0, 0x31)] {
            ports.write(addr, 1, reg);
            ports.write(addr + 1, 1, v);
        }
        let t = Instant::now();
        let mut fm = FmSynth::new(48_000, model);
        fm.queue(o.borrow_mut().take_writes());
        let mut out = vec![0f32; 2 * 48_000];
        fm.render(t, [1.0, 1.0], &mut out);
        let left: Vec<f32> = out.iter().step_by(2).copied().skip(24_000).collect();
        let ups = left.windows(2).filter(|p| p[0] < 0.0 && p[1] >= 0.0).count();
        let right_peak = out.iter().skip(1).step_by(2).fold(0f32, |m, x| m.max(x.abs()));
        (ups as f64 / 0.5, right_peak)
    }

    #[test]
    fn opl2_and_dual_opl2() {
        let mut o = Opl::new();
        o.model = OplModel::Opl2;
        assert_eq!(o.status(Instant::now()) & 0x06, 0x06, "signature OPL2");
        o.model = OplModel::Opl3;
        assert_eq!(o.status(Instant::now()) & 0x06, 0);
        // OPL2 seule : même hauteur que l'OPL3, mono (droite = gauche).
        let (f, right) = opl2_note(OplModel::Opl2, 0x388);
        assert!((f - 309.5).abs() < 3.0 && right > 0.05, "{f} Hz, droite {right}");
        // Double OPL2 : 220h → puce de gauche seule ; 228h → les deux.
        let (f, right) = opl2_note(OplModel::DualOpl2, 0x220);
        assert!((f - 309.5).abs() < 3.0 && right == 0.0, "{f} Hz, droite {right}");
        let (_, right) = opl2_note(OplModel::DualOpl2, 0x228);
        assert!(right > 0.05, "droite {right}");
        // CQM de Creative : se présente en OPL3, joue la même note.
        o.model = OplModel::Cqm;
        assert_eq!(o.status(Instant::now()) & 0x06, 0);
        let (f, right) = opl2_note(OplModel::Cqm, 0x388);
        assert!((f - 309.5).abs() < 3.0 && right > 0.01, "CQM : {f} Hz, droite {right}");
    }

    /// Détection AdLib servie en rafale (lectures de l'état enchaînées en
    /// quelques nanosecondes) : la minuterie 1 (80 µs) doit expirer dans le
    /// temps de la puce, ≈ 1 µs par accès, comme sur un vrai bus ISA.
    #[test]
    fn adlib_detection_with_fast_reads() {
        let o = std::rc::Rc::new(std::cell::RefCell::new(Opl::new()));
        let mut p = OplPorts(o);
        let mut w = |reg: u32, v: u32| {
            p.write(0x388, 1, reg);
            p.write(0x389, 1, v);
        };
        w(4, 0x60);
        w(4, 0x80);
        w(2, 0xFF);
        w(4, 0x21);
        let reads = (1..=200).find(|_| p.read(0x388, 1) & 0xE0 == 0xC0);
        assert!(reads.is_some_and(|n| (60..=100).contains(&n)), "{reads:?} lectures");
    }
}
