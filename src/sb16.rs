// SPDX-License-Identifier: GPL-3.0-or-later
//! Sound Blaster 16 (DSP 4.05) en 220h, IRQ 5, DMA 1 (8 bits) et 5 (16 bits) :
//! réinitialisation, version, sortie directe (10h), transferts DMA 8 et
//! 16 bits en cycle unique ou auto-init (14h, 1Ch, 90h, 91h, B0h–CFh), taux
//! (40h, 41h), pause et reprise, silence (80h), IRQ forcées, mixeur
//! (sélection d'IRQ et de DMA, état des IRQ, volumes maître et voix).
//! Les échantillons sont tirés du DMA au fil du rendu (`render`), qui lève
//! l'IRQ en fin de bloc. La FM (OPL) est un périphérique à part.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::dma::Dma;
use crate::io::IoDevice;
use crate::mem::GuestMem;

pub const BASE: u16 = 0x220;
pub const IRQ: u8 = 5;
const DMA8: usize = 1;
const DMA16: usize = 5;
/// Taille (unités DMA) sous laquelle un transfert en cycle unique est achevé aussitôt.
const SHORT_TRANSFER: u32 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bits {
    Eight,
    Sixteen,
}

/// Transfert DMA en cours.
#[derive(Clone, Copy, Debug)]
struct Transfer {
    bits: Bits,
    stereo: bool,
    signed: bool,
    auto_init: bool,
    /// Taille du bloc, en unités DMA (octets ou mots), et reste à lire.
    block: u32,
    left: u32,
    paused: bool,
    started: Instant,
}

pub struct Sb16 {
    /// Sound Blaster Pro (DSP 3.02) : commandes de la SB16 ignorées.
    pub pro: bool,
    out: VecDeque<u8>,
    /// Commande en cours et ses arguments reçus.
    cmd: Option<u8>,
    args: Vec<u8>,
    reset_high: bool,
    rate: u32,
    block_size: u32,
    transfer: Option<Transfer>,
    /// Silence programmé (80h) : échantillons restants avant l'IRQ.
    silence: u32,
    /// IRQ à lever (machine), et état lu au mixeur (82h) : bit 0 = 8 bits,
    /// bit 1 = 16 bits.
    raise: bool,
    irq_status: u8,
    test_reg: u8,
    mixer_index: u8,
    mixer: [u8; 256],
    /// Rééchantillonnage : position fractionnaire et dernières trames.
    phase: f64,
    prev: [f32; 2],
    cur: [f32; 2],
    /// Sortie directe (10h) : dernier échantillon.
    direct: f32,
}

impl Default for Sb16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sb16 {
    pub fn new() -> Self {
        let mut sb = Self {
            pro: false,
            out: VecDeque::new(),
            cmd: None,
            args: Vec::new(),
            reset_high: false,
            rate: 22_050,
            block_size: 0x800,
            transfer: None,
            silence: 0,
            raise: false,
            irq_status: 0,
            test_reg: 0,
            mixer_index: 0,
            mixer: [0; 256],
            phase: 0.0,
            prev: [0.0; 2],
            cur: [0.0; 2],
            direct: 0.0,
        };
        sb.reset_mixer();
        sb
    }

    fn reset_mixer(&mut self) {
        self.mixer = [0; 256];
        // Volumes au maximum (SB16 : 5 bits en position haute ; SB Pro :
        // 2 × 4 bits) : les jeux règlent eux-mêmes le mixeur s'ils le veulent.
        for r in 0x30..=0x35 {
            self.mixer[r] = 0xF8;
        }
        self.mixer[0x22] = 0xFF;
        self.mixer[0x04] = 0xFF;
        self.mixer[0x26] = 0xFF;
        self.mixer[0x80] = 0x02; // IRQ 5
        self.mixer[0x81] = 0x22; // DMA 1 et 5
    }

    /// Filtre de sortie de la SB Pro : passe-bas sur le DSP, que le bit 5 du
    /// registre 0Eh du mixeur coupe (SB Pro 2). Sans objet sur une SB16.
    pub fn pro_dsp_filter(&self) -> bool {
        self.pro && self.mixer[0x0E] & 0x20 == 0
    }

    /// Gain de la FM : volumes maître et FM du mixeur (gauche, droite).
    pub fn fm_gain(&self) -> [f32; 2] {
        [volume(self.mixer[0x30]) * volume(self.mixer[0x34]), volume(self.mixer[0x31]) * volume(self.mixer[0x35])]
    }

    /// IRQ demandée depuis le dernier appel.
    pub fn take_irq(&mut self) -> bool {
        std::mem::take(&mut self.raise)
    }

    fn irq(&mut self, bits: Bits) {
        self.irq_status |= if bits == Bits::Eight { 1 } else { 2 };
        self.raise = true;
    }

    fn reset(&mut self) {
        self.out.clear();
        self.out.push_back(0xAA);
        self.cmd = None;
        self.args.clear();
        self.transfer = None;
        self.silence = 0;
        self.irq_status = 0;
        self.direct = 0.0;
    }

    fn arg_count(cmd: u8) -> usize {
        match cmd {
            0x10 | 0x40 | 0xE0 | 0xE4 => 1,
            0x14 | 0x16 | 0x17 | 0x24 | 0x41 | 0x42 | 0x48 | 0x80 => 2,
            0xB0..=0xCF => 3,
            _ => 0,
        }
    }

    /// Transfert en cycle unique assez court pour être une détection d'IRQ ;
    /// rend l'instant où il s'achèverait sur la carte (taille ÷ taux). Les
    /// pilotes d'époque l'attendent par une boucle calibrée pour un 386
    /// (≈ 10 µs à la vitesse native) : la machine l'achève donc après
    /// quelques instructions (`complete_short`). Livrée tout de suite,
    /// l'IRQ précéderait la remise à zéro du drapeau attendu ; au passage
    /// suivant du mixeur, elle arriverait après la fin de la boucle.
    pub fn short_transfer_end(&self) -> Option<Instant> {
        let t = self.transfer.filter(|t| !t.auto_init && !t.paused && t.block <= SHORT_TRANSFER)?;
        let ns = u64::from(t.block) * 1_000_000_000 / u64::from(self.rate.max(1000));
        Some(t.started + Duration::from_nanos(ns))
    }

    /// Lit d'un coup le transfert court en cours, puis lève l'IRQ.
    pub fn complete_short(&mut self, dma: &mut Dma, mem: &GuestMem) {
        while self.short_transfer_end().is_some() {
            if self.fetch(dma, mem).is_none() {
                break;
            }
        }
    }

    fn start(&mut self, bits: Bits, mode: u8, auto_init: bool, units: u32) {
        let t = Transfer {
            bits,
            stereo: mode & 0x20 != 0,
            signed: mode & 0x10 != 0,
            auto_init,
            block: units,
            left: units,
            paused: false,
            started: Instant::now(),
        };
        self.transfer = Some(t);
    }

    fn command(&mut self, cmd: u8, a: &[u8]) {
        let word = |lo: u8, hi: u8| u32::from(lo) | (u32::from(hi) << 8);
        // Taux des commandes SB / SB Pro : mono, sauf mixeur 0Eh bit 1 (stéréo).
        let pro_stereo = self.mixer[0x0E] & 2 != 0;
        let pro_mode = if pro_stereo { 0x20 } else { 0 };
        match cmd {
            0x10 => self.direct = (f32::from(a[0]) - 128.0) / 128.0,
            0x14 | 0x91 => {
                let n = if cmd == 0x14 { word(a[0], a[1]) } else { self.block_size };
                self.start(Bits::Eight, pro_mode, false, n + 1);
            }
            0x1C | 0x90 => self.start(Bits::Eight, pro_mode, true, self.block_size + 1),
            0x40 => {
                let channels = if pro_stereo { 2 } else { 1 };
                self.rate = 1_000_000 / (256 - u32::from(a[0])) / channels;
            }
            // Commandes de la SB16 : inconnues d'une SB Pro.
            0x41 | 0x42 | 0xB0..=0xCF | 0xD5 | 0xD6 | 0xD9 if self.pro => {}
            0x41 | 0x42 => self.rate = (u32::from(a[0]) << 8) | u32::from(a[1]),
            0x48 => self.block_size = word(a[0], a[1]),
            0x80 => self.silence = word(a[0], a[1]) + 1,
            // 16 bits (B0h–BFh) ou 8 bits (C0h–CFh) ; bit 3 : entrée (non gérée).
            0xB0..=0xCF if cmd & 0x08 == 0 => {
                let bits = if cmd < 0xC0 { Bits::Sixteen } else { Bits::Eight };
                self.start(bits, a[0], cmd & 0x04 != 0, word(a[1], a[2]) + 1);
            }
            0xD0 | 0xD5 => {
                if let Some(t) = &mut self.transfer {
                    t.paused = true;
                }
            }
            0xD4 | 0xD6 => {
                if let Some(t) = &mut self.transfer {
                    t.paused = false;
                }
            }
            // Fin de l'auto-init : le bloc en cours se termine.
            0xD9 | 0xDA => {
                if let Some(t) = &mut self.transfer {
                    t.auto_init = false;
                }
            }
            0xD8 => self.out.push_back(0xFF),
            0xE0 => self.out.push_back(!a[0]),
            0xE1 if self.pro => self.out.extend([3, 2]),
            0xE1 => self.out.extend([4, 5]),
            0xE3 => self.out.extend(b"COPYRIGHT (C) CREATIVE TECHNOLOGY LTD, 1992.\0"),
            0xE4 => self.test_reg = a[0],
            0xE8 => self.out.push_back(self.test_reg),
            0xF2 => self.irq(Bits::Eight),
            0xF3 => self.irq(Bits::Sixteen),
            // Haut-parleur (sans effet sur un SB16), commandes inconnues.
            _ => {}
        }
    }

    fn write_dsp(&mut self, v: u8) {
        let cmd = match self.cmd {
            Some(c) => {
                self.args.push(v);
                c
            }
            None => {
                self.args.clear();
                v
            }
        };
        if self.args.len() >= Self::arg_count(cmd) {
            self.cmd = None;
            let args = std::mem::take(&mut self.args);
            self.command(cmd, &args);
        } else {
            self.cmd = Some(cmd);
        }
    }

    /// Une trame source tirée du DMA ; `None` si le DMA ne fournit rien.
    fn fetch(&mut self, dma: &mut Dma, mem: &GuestMem) -> Option<[f32; 2]> {
        let t = self.transfer.as_mut()?;
        let channels = if t.stereo { 2 } else { 1 };
        let mut frame = [0f32; 2];
        for c in frame.iter_mut().take(channels) {
            let v = match t.bits {
                Bits::Eight => {
                    let mut b = [0u8; 1];
                    if dma.read_mem(DMA8, mem, &mut b).0 == 0 {
                        return None;
                    }
                    if t.signed { f32::from(b[0] as i8) / 128.0 } else { (f32::from(b[0]) - 128.0) / 128.0 }
                }
                Bits::Sixteen => {
                    let mut b = [0u8; 2];
                    if dma.read_mem(DMA16, mem, &mut b).0 == 0 {
                        return None;
                    }
                    let raw = u16::from_le_bytes(b);
                    if t.signed { f32::from(raw as i16) / 32768.0 } else { (f32::from(raw) - 32768.0) / 32768.0 }
                }
            };
            *c = v;
            t.left = t.left.saturating_sub(1);
        }
        if !t.stereo {
            frame[1] = frame[0];
        }
        if t.left == 0 {
            let bits = t.bits;
            if t.auto_init {
                t.left = t.block;
            } else {
                self.transfer = None;
            }
            self.irq(bits);
        }
        Some(frame)
    }

    /// Ajoute à `out` (stéréo entrelacée, 32 bits flottants) `out.len() / 2`
    /// trames au taux `rate_out`, tirées du DMA.
    pub fn render(&mut self, dma: &mut Dma, mem: &GuestMem, rate_out: u32, out: &mut [f32]) {
        let step = f64::from(self.rate.max(1000)) / f64::from(rate_out);
        let gain = [volume(self.mixer[0x30]) * volume(self.mixer[0x32]), volume(self.mixer[0x31]) * volume(self.mixer[0x33])];
        for frame in out.chunks_exact_mut(2) {
            if self.silence > 0 {
                self.phase += step;
                while self.phase >= 1.0 {
                    self.phase -= 1.0;
                    self.silence -= 1;
                    if self.silence == 0 {
                        self.irq(Bits::Eight);
                        break;
                    }
                }
                continue;
            }
            let playing = self.transfer.is_some_and(|t| !t.paused);
            if !playing {
                frame[0] += self.direct * gain[0];
                frame[1] += self.direct * gain[1];
                continue;
            }
            self.phase += step;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                match self.fetch(dma, mem) {
                    Some(f) => {
                        self.prev = self.cur;
                        self.cur = f;
                    }
                    None => {
                        self.phase = 0.0;
                        break;
                    }
                }
                if self.transfer.is_none() {
                    break;
                }
            }
            let x = self.phase as f32;
            for c in 0..2 {
                frame[c] += (self.prev[c] + (self.cur[c] - self.prev[c]) * x) * gain[c];
            }
        }
    }

    fn read_port(&mut self, port: u16) -> u8 {
        match port - BASE {
            0x05 => match self.mixer_index {
                0x82 => self.irq_status | 0x40,
                i => self.mixer[usize::from(i)],
            },
            0x0A => self.out.pop_front().unwrap_or(0xAA),
            // Prêt à recevoir (bit 7 à 0).
            0x0C => 0x7F,
            // Donnée disponible (bit 7) ; acquitte l'IRQ 8 bits.
            0x0E => {
                self.irq_status &= !1;
                if self.out.is_empty() { 0x7F } else { 0xFF }
            }
            0x0F => {
                self.irq_status &= !2;
                0xFF
            }
            _ => 0xFF,
        }
    }

    fn write_port(&mut self, port: u16, v: u8) {
        match port - BASE {
            0x04 => self.mixer_index = v,
            0x05 => match self.mixer_index {
                0x00 => self.reset_mixer(),
                // Sélections d'IRQ et de DMA : figées sur IRQ 5, DMA 1 et 5.
                0x80 | 0x81 | 0x82 => {}
                // Volumes SB Pro (gauche, droite sur 4 bits) : reportés sur
                // les registres SB16 de 5 bits, comme le fait la carte
                // (n → 2n + 1). Maître 22h, voix 04h, FM 26h.
                i @ (0x22 | 0x04 | 0x26) => {
                    self.mixer[usize::from(i)] = v;
                    let base = match i {
                        0x22 => 0x30,
                        0x04 => 0x32,
                        _ => 0x34,
                    };
                    let five = |n: u8| ((n << 1) | 1) << 3;
                    self.mixer[base] = five(v >> 4);
                    self.mixer[base + 1] = five(v & 0x0F);
                }
                i => self.mixer[usize::from(i)] = v,
            },
            0x06 => {
                if v & 1 != 0 {
                    self.reset_high = true;
                } else if self.reset_high {
                    self.reset_high = false;
                    self.reset();
                }
            }
            0x0C => self.write_dsp(v),
            _ => {}
        }
    }
}

/// Volume d'un registre SB16 (5 bits hauts) : pas de 2 dB, 31 = 0 dB, 0 = muet.
fn volume(r: u8) -> f32 {
    let step = r >> 3;
    if step == 0 { 0.0 } else { 10f32.powf((f32::from(step) - 31.0) * 2.0 / 20.0) }
}

/// Ports 224h–22Fh (la FM en 220h–223h, 228h–229h est à part).
pub struct Sb16Ports(pub std::rc::Rc<std::cell::RefCell<Sb16>>);

impl IoDevice for Sb16Ports {
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

    fn dsp(sb: &mut Sb16, bytes: &[u8]) {
        for &b in bytes {
            sb.write_port(BASE + 0x0C, b);
        }
    }

    #[test]
    fn reset_version_and_auto_init_8_bits() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        let mut sb = Sb16::new();
        sb.write_port(BASE + 6, 1);
        sb.write_port(BASE + 6, 0);
        assert_eq!(sb.read_port(BASE + 0x0E) & 0x80, 0x80);
        assert_eq!(sb.read_port(BASE + 0x0A), 0xAA);
        dsp(&mut sb, &[0xE1]);
        assert_eq!((sb.read_port(BASE + 0x0A), sb.read_port(BASE + 0x0A)), (4, 5));
        // 8 octets à 0xFF en 3000h, DMA 1 auto-init ; bloc de 4 octets à 24 kHz.
        mem.fill(0x3000, 8, 0xFF);
        let dma = Dma::new().shared();
        let mut d = crate::dma::DmaPorts(dma.clone());
        for (p, v) in [(0x0A, 5), (0x0C, 0), (0x02, 0x00), (0x02, 0x30), (0x03, 7), (0x03, 0), (0x83, 0), (0x0B, 0x59), (0x0A, 1)] {
            d.write(p, 1, v);
        }
        // Taux 24 000 Hz (41h), 8 bits auto-init (C6h), mono non signé, bloc de 4.
        dsp(&mut sb, &[0x41, 0x5D, 0xC0, 0xC6, 0x00, 3, 0]);
        let mut out = vec![0f32; 2 * 8];
        sb.render(&mut dma.borrow_mut(), &mem, 48_000, &mut out);
        // 8 trames de sortie = 4 trames source : un bloc, une IRQ.
        assert!(sb.take_irq());
        assert!(out[14] > 0.5);
        sb.write_port(BASE + 4, 0x82);
        assert_eq!(sb.read_port(BASE + 5) & 1, 1);
        sb.read_port(BASE + 0x0E);
        assert_eq!(sb.read_port(BASE + 5) & 1, 0);
    }

    #[test]
    fn sound_blaster_pro_identity() {
        let mut sb = Sb16::new();
        sb.pro = true;
        dsp(&mut sb, &[0xE1]);
        assert_eq!((sb.read_port(BASE + 0x0A), sb.read_port(BASE + 0x0A)), (3, 2));
        // Taux de la SB16 (41h) : ignoré, ses arguments consommés.
        dsp(&mut sb, &[0x41, 0x5D, 0xC0, 0xE1]);
        assert_eq!(sb.rate, Sb16::new().rate);
        assert_eq!(sb.read_port(BASE + 0x0A), 3);
    }
}
