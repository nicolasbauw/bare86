// SPDX-License-Identifier: GPL-3.0-or-later
//! Gravis UltraSound (GF1, « Classic ») : 1 Mo de mémoire sur la carte,
//! 32 voix lues dans cette mémoire (8 ou 16 bits, boucles, sens inverse),
//! rampes de volume logarithmiques, panoramique sur 16 positions, deux
//! minuteries façon AdLib, chargement de la mémoire par DMA, IRQ des voix,
//! des minuteries et du DMA.
//!
//! D'après l'émulation de DOSBox Staging (src/hardware/audio/gus.cpp,
//! GPL-2.0-or-later, The DOSBox Staging Team et The DOSBox Team), elle-même
//! fondée sur l'UltraSound Software Development Kit 2.22 de Gravis.
//! Différences : rendu tiré par le mixeur de Bare86 à son débit (chaque
//! échantillon GF1 maintenu jusqu'au suivant, comme DOSBox Staging avant son
//! rééchantillonnage), DMA servi d'un coup au tour de boucle suivant. IRQ :
//! un front par événement (échéance d'une minuterie, voix, fin de DMA),
//! comme les appels à `CheckIrq` de DOSBox ; la demande est retirée si la
//! ligne retombe avant d'être servie. Relever l'IRQ tant que la ligne est
//! active la ré-imbriquait sans fin dans le gestionnaire (Cyboman 2 : pile
//! débordée).
//!
//! Ports : 2x0 (mélangeur), 2x6 (état des IRQ), 2x8–2x9 (minuteries),
//! 2xA, 2xB (choix IRQ/DMA), 3x2–3x5 (voix, registre, données), 3x7
//! (lecture/écriture directe de la mémoire de la carte), avec x = 4.

use std::time::{Duration, Instant};

use crate::dma::Dma;
use crate::io::IoDevice;
use crate::mem::GuestMem;

/// Adresse de base (2x0 = 240h), DMA et IRQ annoncés par `ULTRASND`.
pub const BASE: u16 = 0x240;
pub const DMA: usize = 3;
pub const IRQ: u8 = 7;
/// Valeur de la variable `ULTRASND` (base, DMA de lecture et
/// d'enregistrement, IRQ de la carte et du MIDI).
pub const ULTRASND: &str = "240,3,3,7,7";

const RAM_SIZE: usize = 1024 * 1024;
const VOICES: usize = 32;
const MIN_VOICES: u8 = 14;
/// Positions des voix : 9 bits de fraction.
const WAVE_WIDTH: i32 = 1 << 9;
/// Volumes : 4 096 niveaux de 0,0235 dB, indices multipliés par 512 pour
/// les incréments fractionnaires des rampes.
const VOLUME_LEVELS: usize = 4096;
const VOLUME_INC_SCALAR: i32 = 512;
const DELTA_DB: f64 = 0.002_709_201;
const PAN_DEFAULT: u8 = 7;
/// Choix d'IRQ et de DMA par 2xB (SDK, 2.14 et 2.15).
const IRQ_ADDRESSES: [u8; 8] = [0, 2, 5, 3, 7, 11, 12, 15];
const DMA_ADDRESSES: [usize; 6] = [0, 1, 3, 5, 6, 7];
/// Minuteries : pas de 80 et 320 µs.
const TIMER_STEP: [Duration; 2] = [Duration::from_micros(80), Duration::from_micros(320)];
/// Sortie analogique de la GUS Classic : passe-bas du premier ordre à 8 kHz
/// (mesures de DOSBox Staging sur une vraie carte).
const LOWPASS_HZ: f32 = 8000.0;

// États d'une commande de voix (onde ou volume).
const RESET: u8 = 0x01;
const STOPPED: u8 = 0x02;
const DISABLED: u8 = RESET | STOPPED;
const BIT16: u8 = 0x04;
const LOOP: u8 = 0x08;
const BIDIRECTIONAL: u8 = 0x10;
const RAISEIRQ: u8 = 0x20;
const DECREASING: u8 = 0x40;

/// Commande d'onde ou de volume d'une voix : bornes, position, pas.
#[derive(Clone, Copy)]
struct Ctrl {
    start: i32,
    end: i32,
    pos: i32,
    inc: i32,
    state: u8,
}

impl Default for Ctrl {
    fn default() -> Self {
        Self { start: 0, end: 0, pos: 0, inc: 0, state: DISABLED }
    }
}

/// Avance d'un pas ; vrai si la borne est atteinte avec demande d'IRQ.
fn step(c: &mut Ctrl, dont_loop: bool) -> bool {
    if c.state & DISABLED != 0 {
        return false;
    }
    let remaining = if c.state & DECREASING != 0 {
        c.pos -= c.inc;
        c.start - c.pos
    } else {
        c.pos += c.inc;
        c.pos - c.end
    };
    if remaining < 0 {
        return false;
    }
    let irq = c.state & RAISEIRQ != 0;
    if dont_loop {
        return irq;
    }
    if c.state & LOOP != 0 {
        if c.state & BIDIRECTIONAL != 0 {
            c.state ^= DECREASING;
        }
        c.pos = if c.state & DECREASING != 0 { c.end - remaining } else { c.start + remaining };
    } else {
        c.state |= 1;
        c.pos = if c.state & DECREASING != 0 { c.start } else { c.end };
    }
    irq
}

#[derive(Clone, Copy, Default)]
struct Voice {
    wave: Ctrl,
    vol: Ctrl,
    pan: u8,
}

#[derive(Clone, Copy)]
struct Timer {
    value: u8,
    expired: bool,
    counting: bool,
    masked: bool,
    raise: bool,
    due: Option<Instant>,
}

impl Timer {
    fn delay(&self, t: usize) -> Duration {
        TIMER_STEP[t] * (0x100 - u32::from(self.value))
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self { value: 0xFF, expired: true, counting: false, masked: false, raise: false, due: None }
    }
}

pub struct Gus {
    ram: Vec<u8>,
    voices: [Voice; VOICES],
    /// Voix qui demandent une IRQ d'onde ou de volume (un bit par voix), et
    /// voix rapportée par le registre 8Fh.
    wave_irq: u32,
    vol_irq: u32,
    irq_voice: u8,
    active: u8,
    active_mask: u32,
    /// Débit de la puce : 44,1 kHz jusqu'à 14 voix, moins au-delà.
    rate_hz: f64,
    voice: Option<usize>,
    voice_index: u8,
    selected: u8,
    data: u16,
    dram_addr: u32,
    dma_ctrl: u8,
    dma_addr: u16,
    dma_nibble: u8,
    dma_pending: bool,
    pub dma1: usize,
    pub irq1: u8,
    adlib_cmd: u8,
    timer_ctrl: u8,
    timers: [Timer; 2],
    irq_status: u8,
    /// Événement d'IRQ depuis le dernier relevé (`take_edge`).
    edge: bool,
    reset: u8,
    mix: u8,
    latch_select: bool,
    vol_table: Vec<f32>,
    pan_table: [[f32; 2]; 16],
    /// Rendu : avance dans la trame GF1 en cours, trame en cours, filtre.
    phase: f64,
    cur: [f32; 2],
    lowpass: [f32; 2],
}

impl Default for Gus {
    fn default() -> Self {
        Self::new()
    }
}

impl Gus {
    pub fn new() -> Self {
        let mut vol_table = vec![0f32; VOLUME_LEVELS];
        let mut scalar = 1.0f64;
        for v in vol_table.iter_mut().rev() {
            *v = scalar as f32;
            scalar /= 1.0 + DELTA_DB;
        }
        vol_table[0] = 0.0;
        // Panoramique à puissance constante (0 : gauche, 7 : centre, 15 : droite).
        let pan_table = std::array::from_fn(|i| {
            let norm = (i as f64 - 7.0) / if i < 7 { 7.0 } else { 8.0 };
            let angle = (norm + 1.0) * std::f64::consts::PI / 4.0;
            [angle.cos() as f32, angle.sin() as f32]
        });
        let mut g = Self {
            ram: vec![0; RAM_SIZE],
            voices: [Voice::default(); VOICES],
            wave_irq: 0,
            vol_irq: 0,
            irq_voice: 0,
            active: 0,
            active_mask: 0,
            rate_hz: 44_100.0,
            voice: None,
            voice_index: 0,
            selected: 0,
            data: 0,
            dram_addr: 0,
            dma_ctrl: 0,
            dma_addr: 0,
            dma_nibble: 0,
            dma_pending: false,
            dma1: DMA,
            irq1: IRQ,
            adlib_cmd: 85,
            timer_ctrl: 0,
            timers: [Timer::default(); 2],
            irq_status: 0,
            edge: false,
            reset: 0,
            mix: 0x0B,
            latch_select: false,
            vol_table,
            pan_table,
            phase: 0.0,
            cur: [0.0; 2],
            lowpass: [0.0; 2],
        };
        g.reset_card();
        g
    }

    fn reset_card(&mut self) {
        self.irq_status = 0;
        self.adlib_cmd = 85;
        self.dma_ctrl = 0;
        self.dma_pending = false;
        self.timer_ctrl = 0;
        self.timers = [Timer::default(); 2];
        for v in &mut self.voices {
            v.vol.pos = 0;
            v.vol.state = DISABLED;
            v.wave.state = DISABLED;
            v.pan = PAN_DEFAULT;
        }
        self.wave_irq = 0;
        self.vol_irq = 0;
        self.irq_voice = 0;
        self.voice = None;
        self.voice_index = 0;
        self.active = 0;
        self.active_mask = 0;
        self.set_dma_addr(0);
        self.dram_addr = 0;
        self.data = 0;
        self.selected = 0;
        self.latch_select = false;
        self.reset = 0;
        self.mix = 0x0B;
    }

    fn activate_voices(&mut self, n: u8) {
        let n = n.clamp(MIN_VOICES, VOICES as u8);
        if n != self.active {
            self.active = n;
            self.active_mask = u32::MAX >> (VOICES as u8 - n);
            // Débit de la puce selon le nombre de voix (Gravis, Lowlevel
            // ToolKit 2.22).
            self.rate_hz = 1_000_000.0 / (1.619_695_497 * f64::from(n));
        }
    }

    /// La ligne d'IRQ de la carte est active.
    pub fn irq_line(&self) -> bool {
        let mask = if self.reset & 0x04 != 0 { 0xFF } else { 0x9F };
        self.irq_status & mask != 0 && self.mix & 0x08 != 0
    }

    /// Nouvel événement d'IRQ depuis le dernier appel.
    pub fn take_edge(&mut self) -> bool {
        std::mem::take(&mut self.edge)
    }

    /// État des IRQ des voix (bits 5 et 6) et voix rapportée par 8Fh.
    fn check_voice_irq(&mut self) {
        self.irq_status &= 0x9F;
        let total = (self.vol_irq | self.wave_irq) & self.active_mask;
        if total == 0 {
            return;
        }
        self.edge = true;
        if self.vol_irq != 0 {
            self.irq_status |= 0x40;
        }
        if self.wave_irq != 0 {
            self.irq_status |= 0x20;
        }
        while total & (1 << self.irq_voice) == 0 {
            self.irq_voice += 1;
            if self.irq_voice >= self.active {
                self.irq_voice = 0;
            }
        }
    }

    // ---- Minuteries ----

    /// Prochaine échéance d'une minuterie qui compte.
    pub fn next_due(&self) -> Option<Instant> {
        self.timers.iter().filter_map(|t| t.due).min()
    }

    /// Plus courte période des minuteries qui comptent (battement).
    pub fn timer_period(&self) -> Option<Duration> {
        (0..2).filter(|&t| self.timers[t].counting).map(|t| self.timers[t].delay(t)).min()
    }

    /// Minuteries échues jusqu'à `now`.
    pub fn tick(&mut self, now: Instant) {
        for t in 0..2 {
            while let Some(due) = self.timers[t].due.filter(|&d| d <= now) {
                let timer = &mut self.timers[t];
                if !timer.masked {
                    timer.expired = true;
                }
                if timer.raise {
                    self.irq_status |= 0x04 << t;
                    self.edge = true;
                }
                let timer = &mut self.timers[t];
                timer.due = timer.counting.then(|| due + timer.delay(t));
                // Long arrêt (pause) : pas de rattrapage tic par tic.
                if timer.due.is_some_and(|d| d + Duration::from_millis(100) < now) {
                    timer.due = Some(now + timer.delay(t));
                }
            }
        }
    }

    // ---- DMA ----

    fn dma_16bit(&self) -> bool {
        self.dma_ctrl & 0x04 != 0 && self.dma1 >= 4
    }

    fn dma_offset(&self) -> usize {
        let a = u32::from(self.dma_addr);
        let adjusted = if self.dma_16bit() { (a & 0xC000) | ((a & 0x1FFF) << 1) } else { a };
        ((adjusted << 4) + u32::from(self.dma_nibble)) as usize
    }

    fn set_dma_addr(&mut self, offset: usize) {
        let offset = offset as u32;
        let adjusted = if self.dma_16bit() { (offset & 0xC0000) | ((offset & 0x3FFFE) >> 1) } else { offset & 0xFFFF0 };
        self.dma_addr = (adjusted >> 4) as u16;
        self.dma_nibble = (adjusted & 0xF) as u8;
    }

    /// Transfert DMA demandé (registre 41h) : servi d'un coup jusqu'à la
    /// fin de comptage du canal, s'il est démasqué.
    pub fn service_dma(&mut self, dma: &mut Dma, mem: &GuestMem) {
        if !self.dma_pending {
            return;
        }
        if self.dma_ctrl & 0x01 == 0 || self.dma_ctrl & 0x02 != 0 {
            // Arrêté, ou de la carte vers l'hôte (enregistrement) : non géré.
            self.dma_pending = false;
            return;
        }
        if dma.masked(self.dma1) {
            return;
        }
        let offset = self.dma_offset().min(RAM_SIZE);
        let mut buf = vec![0u8; RAM_SIZE - offset];
        let (n, tc) = dma.read_mem(self.dma1, mem, &mut buf);
        let ram = &mut self.ram[offset..offset + n];
        ram.copy_from_slice(&buf[..n]);
        // Bit de poids fort inversé (échantillons non signés).
        if self.dma_ctrl & 0x80 != 0 {
            let (first, skip) = if self.dma_ctrl & 0x40 != 0 { (1, 2) } else { (0, 1) };
            for b in ram.iter_mut().skip(first).step_by(skip) {
                *b ^= 0x80;
            }
        }
        self.set_dma_addr(offset + n);
        if tc || n == buf.len() {
            self.dma_pending = false;
            self.dma_ctrl |= 0x40;
            if self.dma_ctrl & 0x20 != 0 {
                self.irq_status |= 0x80;
                self.edge = true;
            }
        }
    }

    // ---- Registres ----

    fn read_register(&mut self) -> u16 {
        match self.selected {
            0x41 => {
                let r = self.dma_ctrl;
                self.dma_ctrl &= !0x40;
                self.irq_status &= 0x7F;
                return u16::from(r) << 8;
            }
            0x42 => return self.dma_addr,
            0x45 => return u16::from(self.timer_ctrl) << 8,
            0x49 => return u16::from(self.dma_ctrl) << 8,
            0x4C => return u16::from(self.reset) << 8,
            0x8F => {
                let mut r = self.irq_voice | 0x20;
                let mask = 1 << self.irq_voice;
                if self.vol_irq & mask == 0 {
                    r |= 0x40;
                }
                if self.wave_irq & mask == 0 {
                    r |= 0x80;
                }
                self.vol_irq &= !mask;
                self.wave_irq &= !mask;
                self.check_voice_irq();
                return u16::from(r) << 8;
            }
            _ => {}
        }
        let Some(i) = self.voice else {
            return if matches!(self.selected, 0x80 | 0x8D) { 0x0300 } else { 0 };
        };
        let bit = 1u32 << i;
        let v = &self.voices[i];
        let state = |c: &Ctrl, irq: u32| u16::from(c.state | if irq & bit != 0 { 0x80 } else { 0 }) << 8;
        match self.selected {
            0x80 => state(&v.wave, self.wave_irq),
            0x82 => (v.wave.start >> 16) as u16,
            0x83 => v.wave.start as u16,
            0x89 => ((vol_index(v.vol.pos)) << 4) as u16,
            0x8A => (v.wave.pos >> 16) as u16,
            0x8B => v.wave.pos as u16,
            0x8D => state(&v.vol, self.vol_irq),
            _ => self.data,
        }
    }

    fn write_register(&mut self) {
        let hi = (self.data >> 8) as u8;
        match self.selected {
            0x0E => {
                // Jazz Jackrabbit relit ce registre comme numéro de registre.
                self.selected = hi;
                self.activate_voices(1 + (hi & 31));
                return;
            }
            0x10 => return,
            0x41 => {
                self.dma_ctrl = hi;
                self.dma_pending = hi & 0x01 != 0;
                return;
            }
            0x42 => {
                self.dma_addr = self.data;
                self.dma_nibble = 0;
                return;
            }
            0x43 => {
                self.dram_addr = (self.dram_addr & 0xF0000) | u32::from(self.data);
                return;
            }
            0x44 => {
                self.dram_addr = (self.dram_addr & 0x0FFFF) | ((u32::from(self.data) & 0x0F00) << 8);
                return;
            }
            0x45 => {
                self.timer_ctrl = hi;
                for t in 0..2 {
                    self.timers[t].raise = hi & (0x04 << t) != 0;
                    if !self.timers[t].raise {
                        self.irq_status &= !(0x04 << t);
                    }
                }
                return;
            }
            0x46 | 0x47 => {
                self.timers[usize::from(self.selected - 0x46)].value = hi;
                return;
            }
            0x49 => return,
            0x4C => {
                self.reset = hi;
                if hi & 0x01 == 0 {
                    self.reset_card();
                    self.reset = hi;
                }
                return;
            }
            _ => {}
        }
        let Some(i) = self.voice else { return };
        let bit = 1u32 << i;
        let data = self.data;
        let v = &mut self.voices[i];
        let msw = |a: &mut i32| *a = (*a & 0xFFFF) | (i32::from(data & 0x1FFF) << 16);
        let lsw = |a: &mut i32| *a = (*a & !0xFFFF) | i32::from(data);
        match self.selected {
            0x00 | 0x0D => {
                let (c, irq) = if self.selected == 0 { (&mut v.wave, &mut self.wave_irq) } else { (&mut v.vol, &mut self.vol_irq) };
                let before = *irq;
                if hi & 0xA0 == 0xA0 {
                    *irq |= bit;
                } else {
                    *irq &= !bit;
                }
                c.state = hi & 0x7F;
                if before != *irq {
                    self.check_voice_irq();
                }
            }
            0x01 => v.wave.inc = i32::from(data.div_ceil(2)),
            0x02 => msw(&mut v.wave.start),
            0x03 => lsw(&mut v.wave.start),
            0x04 => msw(&mut v.wave.end),
            0x05 => lsw(&mut v.wave.end),
            0x06 => {
                // Quatre bancs : pas de 1, 1/8, 1/64, 1/512 d'indice.
                let in_bank = i32::from(hi & 63);
                let decimator = 1 << (3 * (hi >> 6));
                v.vol.inc = (in_bank * VOLUME_INC_SCALAR + decimator - 1) / decimator;
            }
            0x07 => v.vol.start = (i32::from(hi) << 4) * VOLUME_INC_SCALAR,
            0x08 => v.vol.end = (i32::from(hi) << 4) * VOLUME_INC_SCALAR,
            0x09 => v.vol.pos = i32::from(data >> 4) * VOLUME_INC_SCALAR,
            0x0A => msw(&mut v.wave.pos),
            0x0B => lsw(&mut v.wave.pos),
            0x0C => v.pan = hi.min(15),
            _ => {}
        }
    }

    fn read_port(&mut self, port: u16, word: bool) -> u16 {
        match port.wrapping_sub(BASE) {
            0x006 => u16::from(self.irq_status),
            0x008 => {
                let mut t = 0u8;
                if self.timers[0].expired {
                    t |= 0x40;
                }
                if self.timers[1].expired {
                    t |= 0x20;
                }
                if t & 0x60 != 0 {
                    t |= 0x80;
                }
                if self.irq_status & 0x04 != 0 {
                    t |= 0x04;
                }
                if self.irq_status & 0x08 != 0 {
                    t |= 0x02;
                }
                u16::from(t)
            }
            0x00A => u16::from(self.adlib_cmd),
            0x102 => u16::from(self.voice_index),
            0x103 => u16::from(self.selected),
            0x104 => {
                let r = self.read_register();
                if word { r } else { r & 0xFF }
            }
            0x105 => self.read_register() >> 8,
            0x107 => self.ram.get(self.dram_addr as usize).copied().map_or(0, u16::from),
            _ => 0xFF,
        }
    }

    fn write_port(&mut self, port: u16, val: u16, word: bool) {
        match port.wrapping_sub(BASE) {
            0x000 => {
                self.mix = val as u8;
                self.latch_select = true;
                return;
            }
            0x008 => self.adlib_cmd = val as u8,
            0x009 => {
                let now = Instant::now();
                if val & 0x80 != 0 {
                    self.timers[0].expired = false;
                    self.timers[1].expired = false;
                    return;
                }
                self.timers[0].masked = val & 0x40 != 0;
                self.timers[1].masked = val & 0x20 != 0;
                for t in 0..2 {
                    if val & (1 << t) != 0 {
                        if !self.timers[t].counting {
                            self.timers[t].counting = true;
                            self.timers[t].due = Some(now + self.timers[t].delay(t));
                        }
                    } else {
                        self.timers[t].counting = false;
                        self.timers[t].due = None;
                    }
                }
            }
            0x00B if self.latch_select => {
                // Doit suivre immédiatement l'écriture de 2x0 (SDK, 2.13).
                let ch1 = usize::from(val & 7);
                if self.mix & 0x40 != 0 {
                    if (1..IRQ_ADDRESSES.len()).contains(&ch1) {
                        self.irq1 = match IRQ_ADDRESSES[ch1] {
                            2 => 9,
                            n => n,
                        };
                    }
                } else if (1..DMA_ADDRESSES.len()).contains(&ch1) {
                    self.dma1 = DMA_ADDRESSES[ch1];
                }
            }
            0x102 => {
                self.voice_index = (val & 31) as u8;
                self.voice = Some(usize::from(self.voice_index));
            }
            0x103 => {
                self.selected = val as u8;
                self.data = 0;
            }
            0x104 => {
                self.data = val;
                if word {
                    self.write_register();
                }
            }
            0x105 => {
                self.data = (self.data & 0x00FF) | (val << 8);
                self.write_register();
            }
            0x107 => {
                if let Some(b) = self.ram.get_mut(self.dram_addr as usize) {
                    *b = val as u8;
                }
            }
            _ => {}
        }
        self.latch_select = false;
    }

    // ---- Rendu ----

    /// Une trame de la puce : somme des voix actives (échelle 16 bits).
    fn gf1_frame(&mut self) -> [f32; 2] {
        let mut out = [0f32; 2];
        if self.reset & 0x03 != 0x03 {
            return out;
        }
        for i in 0..usize::from(self.active) {
            let v = &mut self.voices[i];
            if v.vol.state & v.wave.state & DISABLED != 0 {
                continue;
            }
            // Échantillon (interpolé si la voix avance de moins d'un
            // échantillon par trame), puis avance.
            let pos = v.wave.pos;
            let addr = pos / WAVE_WIDTH;
            let frac = pos & (WAVE_WIDTH - 1);
            let wide = v.wave.state & BIT16 != 0;
            let read = |a: i32| -> f32 {
                if wide {
                    let a = a as u32;
                    let i = ((a & 0xC0000) | ((a & 0x1FFFF) << 1)) as usize & (RAM_SIZE - 1);
                    f32::from(i16::from_le_bytes([self.ram[i], self.ram[(i + 1) & (RAM_SIZE - 1)]]))
                } else {
                    f32::from(self.ram[(a as usize) & (RAM_SIZE - 1)] as i8) * 256.0
                }
            };
            let mut s = read(addr);
            if v.wave.inc < WAVE_WIDTH && frac != 0 {
                let next = read(addr + 1);
                s += (next - s) * frac as f32 / WAVE_WIDTH as f32;
            }
            // Bascule (rollover) : IRQ sans boucle ni arrêt (SDK, 3.11).
            let rollover = v.vol.state & BIT16 != 0 && v.wave.state & LOOP == 0;
            if step(&mut v.wave, rollover) {
                self.wave_irq |= 1 << i;
            }
            let level = self.vol_table[vol_index(v.vol.pos) as usize];
            if step(&mut v.vol, false) {
                self.vol_irq |= 1 << i;
            }
            let pan = self.pan_table[usize::from(v.pan)];
            out[0] += s * level * pan[0];
            out[1] += s * level * pan[1];
        }
        out
    }
}

/// Indice dans la table des volumes d'une position de rampe.
fn vol_index(pos: i32) -> i32 {
    ((pos + VOLUME_INC_SCALAR - 1).div_euclid(VOLUME_INC_SCALAR)).clamp(0, VOLUME_LEVELS as i32 - 1)
}

impl crate::audio::AudioSource for Gus {
    fn render(&mut self, rate: u32, out: &mut [f32]) {
        if self.active == 0 {
            return;
        }
        let step = self.rate_hz / f64::from(rate);
        let alpha = 1.0 - (-2.0 * std::f32::consts::PI * LOWPASS_HZ / rate as f32).exp();
        for frame in out.chunks_exact_mut(2) {
            self.phase += step;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                self.cur = self.gf1_frame();
            }
            for c in 0..2 {
                self.lowpass[c] += alpha * (self.cur[c] / 32768.0 - self.lowpass[c]);
                frame[c] += self.lowpass[c];
            }
        }
        self.check_voice_irq();
    }

    fn skip(&mut self, frames: u64, rate: u32) {
        let mut n = (frames as f64 * self.rate_hz / f64::from(rate)) as u64;
        while n > 0 && self.active > 0 {
            self.gf1_frame();
            n -= 1;
        }
        self.check_voice_irq();
    }
}

/// Ports de la carte : 2x0, 2x6, 2x8–2xB, 3x2–3x7. Un accès 16 bits ailleurs
/// qu'en 3x4 est fait de deux accès 8 bits (carte ISA 8 bits).
pub struct GusPorts(pub std::rc::Rc<std::cell::RefCell<Gus>>);

impl IoDevice for GusPorts {
    fn read(&mut self, port: u16, size: u8) -> u32 {
        let mut g = self.0.borrow_mut();
        if size >= 2 && port != BASE + 0x104 {
            let lo = g.read_port(port, false) & 0xFF;
            let hi = g.read_port(port + 1, false) & 0xFF;
            return u32::from(lo | (hi << 8));
        }
        u32::from(g.read_port(port, size >= 2))
    }

    fn write(&mut self, port: u16, size: u8, value: u32) {
        let mut g = self.0.borrow_mut();
        if size >= 2 && port != BASE + 0x104 {
            g.write_port(port, (value & 0xFF) as u16, false);
            g.write_port(port + 1, ((value >> 8) & 0xFF) as u16, false);
            return;
        }
        g.write_port(port, value as u16, size >= 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioSource;

    fn reg(g: &mut Gus, r: u8, v: u16) {
        g.write_port(BASE + 0x103, u16::from(r), false);
        g.write_port(BASE + 0x104, v, true);
    }

    /// Détection classique : écriture puis relecture de la mémoire de la
    /// carte par 3x7 après remise à zéro.
    #[test]
    fn dram_peek_poke() {
        let mut g = Gus::new();
        reg(&mut g, 0x4C, 0x0000);
        reg(&mut g, 0x4C, 0x0100);
        reg(&mut g, 0x43, 0x0000);
        reg(&mut g, 0x44, 0x0100); // octet 10000h
        g.write_port(BASE + 0x107, 0x55, false);
        reg(&mut g, 0x44, 0x0000);
        g.write_port(BASE + 0x107, 0xAA, false);
        reg(&mut g, 0x44, 0x0100);
        assert_eq!(g.read_port(BASE + 0x107, false), 0x55);
    }

    /// Une voix 8 bits jouée sans boucle : du son, puis l'IRQ d'onde en fin
    /// d'échantillon, relevée par 8Fh.
    #[test]
    fn voice_plays_and_raises_wave_irq() {
        let mut g = Gus::new();
        reg(&mut g, 0x4C, 0x0700);
        reg(&mut g, 0x0E, 13 << 8); // 14 voix
        for i in 0..1000 {
            g.ram[i] = if i % 20 < 10 { 0x60 } else { 0xA0 };
        }
        g.write_port(BASE + 0x102, 0, false);
        reg(&mut g, 0x02, 0);
        reg(&mut g, 0x03, 0);
        reg(&mut g, 0x04, 0);
        reg(&mut g, 0x05, 999 << 9);
        reg(&mut g, 0x01, 1024); // un échantillon par trame
        reg(&mut g, 0x09, 0xFFF0); // volume maximal
        reg(&mut g, 0x0D, 0x0300); // pas de rampe
        reg(&mut g, 0x00, 0x2000); // en route, IRQ en fin
        let mut out = vec![0f32; 2 * 4800];
        g.render(48_000, &mut out);
        assert!(out.iter().any(|s| s.abs() > 0.3));
        assert!(g.irq_line());
        g.write_port(BASE + 0x103, 0x8F, false);
        let r = g.read_port(BASE + 0x105, false) as u8;
        assert_eq!(r & 0x1F, 0);
        assert_eq!(r & 0x80, 0, "IRQ d'onde de la voix 0");
        assert!(!g.irq_line());
    }

    /// Minuterie 1 : IRQ (bit 2 de 2x6) à l'échéance.
    #[test]
    fn timer_one_raises_irq() {
        let mut g = Gus::new();
        reg(&mut g, 0x4C, 0x0700);
        reg(&mut g, 0x46, 0xF0 << 8); // 16 × 80 µs
        reg(&mut g, 0x45, 0x04 << 8);
        g.write_port(BASE + 0x008, 4, false);
        g.write_port(BASE + 0x009, 1, false);
        let due = g.next_due().unwrap();
        g.tick(due);
        assert_eq!(g.irq_status & 0x04, 0x04);
        assert!(g.irq_line());
    }
}
