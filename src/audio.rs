// SPDX-License-Identifier: GPL-3.0-or-later
//! Mixeur, sur le thread du vCPU : à chaque passage (`advance`), produit les
//! trames dues d'après l'horloge hôte (48 kHz, stéréo) — Sound Blaster
//! (DMA), haut-parleur PC — et les confie à l'hôte (`AudioSink`). Sorties :
//! file vers la fenêtre SDL, fichier WAV (`--wav`), ou rien.

use std::time::{Duration, Instant};

use crate::dma::Dma;
use crate::host::AudioSink;
use crate::mem::GuestMem;
use crate::sb16::Sb16;

pub const RATE: u32 = 48_000;
/// Au-delà de ce retard (pause, débogueur), le temps perdu est sauté.
const MAX_LAG: Duration = Duration::from_millis(200);
const SPEAKER_LEVEL: f32 = 0.15;

pub struct Mixer {
    start: Instant,
    /// Trames produites depuis `start`.
    produced: u64,
    speaker_phase: f64,
    buf: Vec<f32>,
    out: Vec<i16>,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    pub fn new() -> Self {
        Self { start: Instant::now(), produced: 0, speaker_phase: 0.0, buf: Vec::new(), out: Vec::new() }
    }

    /// Produit les trames dues à `now`.
    pub fn advance(&mut self, now: Instant, sb: &mut Sb16, dma: &mut Dma, mem: &GuestMem, speaker: Option<f64>, sink: &mut dyn AudioSink) {
        let elapsed = now.saturating_duration_since(self.start);
        let target = (elapsed.as_nanos() * u128::from(RATE) / 1_000_000_000) as u64;
        let lag = target.saturating_sub(self.produced);
        if lag == 0 {
            return;
        }
        let max = MAX_LAG.as_millis() as u64 * u64::from(RATE) / 1000;
        if lag > max {
            self.produced = target - max / 4;
        }
        let n = (target - self.produced) as usize;
        self.produced = target;
        self.buf.clear();
        self.buf.resize(n * 2, 0.0);
        sb.render(dma, mem, RATE, &mut self.buf);
        // Haut-parleur : onde carrée ; au-delà de 20 kHz (modulation de
        // largeur d'impulsion), silence.
        if let Some(f) = speaker.filter(|f| *f < 20_000.0) {
            for frame in self.buf.chunks_exact_mut(2) {
                let s = if self.speaker_phase < 0.5 { SPEAKER_LEVEL } else { -SPEAKER_LEVEL };
                frame[0] += s;
                frame[1] += s;
                self.speaker_phase = (self.speaker_phase + f / f64::from(RATE)).fract();
            }
        }
        self.out.clear();
        self.out.extend(self.buf.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16));
        sink.push(&self.out);
    }
}

/// Fichier WAV (PCM 16 bits stéréo, 48 kHz), écrit à la fin de la session.
pub struct WavSink {
    path: std::path::PathBuf,
    samples: Vec<i16>,
}

impl WavSink {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into(), samples: Vec::new() }
    }
}

impl AudioSink for WavSink {
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn push(&mut self, samples: &[i16]) {
        self.samples.extend_from_slice(samples);
    }
}

impl Drop for WavSink {
    fn drop(&mut self) {
        let data = (self.samples.len() * 2) as u32;
        let mut f = Vec::with_capacity(44 + data as usize);
        f.extend(b"RIFF");
        f.extend((36 + data).to_le_bytes());
        f.extend(b"WAVEfmt ");
        f.extend(16u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(2u16.to_le_bytes());
        f.extend(RATE.to_le_bytes());
        f.extend((RATE * 4).to_le_bytes());
        f.extend(4u16.to_le_bytes());
        f.extend(16u16.to_le_bytes());
        f.extend(b"data");
        f.extend(data.to_le_bytes());
        for s in &self.samples {
            f.extend(s.to_le_bytes());
        }
        if let Err(e) = std::fs::write(&self.path, f) {
            eprintln!("{} : {e}", self.path.display());
        }
    }
}

/// File partagée avec le thread de la fenêtre, qui la verse dans le flux SDL.
pub struct QueueSink(pub std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<i16>>>);

/// Au plus 250 ms en attente : au-delà, les plus anciennes sont jetées.
const QUEUE_MAX: usize = RATE as usize / 2;

impl AudioSink for QueueSink {
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn push(&mut self, samples: &[i16]) {
        if let Ok(mut q) = self.0.lock() {
            q.extend(samples);
            let excess = q.len().saturating_sub(QUEUE_MAX);
            q.drain(..excess);
        }
    }
}
