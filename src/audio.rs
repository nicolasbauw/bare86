// SPDX-License-Identifier: GPL-3.0-or-later
//! Son à 48 kHz, stéréo. Le mixeur, sur le thread du vCPU, produit d'après
//! l'horloge hôte la Sound Blaster (DMA) et le haut-parleur PC ; un thread
//! audio y ajoute la FM OPL3 (synthèse coûteuse) et livre à l'hôte
//! (`AudioSink`) : file vers la fenêtre SDL, fichier WAV (`--wav`), ou rien.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::dma::Dma;
use crate::host::AudioSink;
use crate::mem::GuestMem;
use crate::opl::{FmSynth, Opl, Write};
use crate::sb16::Sb16;

pub const RATE: u32 = 48_000;
/// Au-delà de ce retard (pause, débogueur), le temps perdu est sauté.
const MAX_LAG: Duration = Duration::from_millis(200);
/// Rendu par tranches d'au moins 1 ms, blocs confiés au thread audio par
/// 5 ms : pas un message par sortie de VM.
const MIN_RENDER: u64 = RATE as u64 / 1000;
const BLOCK: usize = RATE as usize / 200;
const SPEAKER_LEVEL: f32 = 0.15;
/// Nuked-OPL3 sort environ 6 dB sous le son numérisé d'une vraie carte.
const FM_LEVEL: f32 = 2.0;
/// Marge de sortie (−6 dB) : un jeu qui mixe à pleine échelle (Quake) ne
/// sature pas à la somme des sources ; la chaîne analogique d'une vraie
/// carte en avait aussi. Au-delà de `KNEE`, limiteur doux.
const OUTPUT_GAIN: f32 = 0.5;
const KNEE: f32 = 0.8;

/// Limiteur doux : linéaire jusqu'à `KNEE`, puis tangente hyperbolique
/// vers la pleine échelle (pas d'écrêtage brutal).
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let room = 1.0 - KNEE;
        x.signum() * (KNEE + room * ((a - KNEE) / room).tanh())
    }
}

/// Bloc confié au thread audio : Sound Blaster et haut-parleur déjà mixés
/// (stéréo entrelacée) à partir de `t0`, écritures OPL jusque-là, gain FM.
struct Block {
    t0: Instant,
    frames: Vec<f32>,
    writes: Vec<Write>,
    fm_gain: [f32; 2],
}

/// Mixeur, sur le thread du vCPU : produit d'après l'horloge hôte le son de
/// la Sound Blaster (qui lit la mémoire invitée par DMA) et du haut-parleur,
/// puis le confie au thread audio, qui y ajoute la FM et le livre à l'hôte.
pub struct Mixer {
    start: Instant,
    /// Trames produites depuis `start`.
    produced: u64,
    speaker_phase: f64,
    /// Bloc en cours de remplissage et son instant de départ.
    buf: Vec<f32>,
    buf_t0: Instant,
    /// Avant cet instant, moins d'1 ms est due : rien à faire (appelé à
    /// chaque sortie de VM, le cas courant doit coûter une comparaison).
    next_due: Instant,
    tx: Option<Sender<Block>>,
    thread: Option<JoinHandle<()>>,
}

impl Mixer {
    /// Lance le thread audio, qui livre à `sink`.
    pub fn new(sink: Box<dyn AudioSink + Send>) -> Self {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new().name("audio".into()).spawn(move || audio_thread(rx, sink)).ok();
        let now = Instant::now();
        Self { start: now, produced: 0, speaker_phase: 0.0, buf: Vec::new(), buf_t0: now, next_due: now, tx: Some(tx), thread }
    }

    fn instant_of(&self, frame: u64) -> Instant {
        self.start + Duration::from_nanos(frame * 1_000_000_000 / u64::from(RATE))
    }

    /// Produit les trames dues à `now`.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn advance(&mut self, now: Instant, sb: &mut Sb16, opl: &mut Opl, dma: &mut Dma, mem: &GuestMem, speaker: Option<f64>, cd: Option<&mut crate::cdrom::Player>) {
        if now < self.next_due {
            return;
        }
        let elapsed = now.saturating_duration_since(self.start);
        let target = (elapsed.as_nanos() * u128::from(RATE) / 1_000_000_000) as u64;
        let lag = target.saturating_sub(self.produced);
        if lag < MIN_RENDER {
            self.next_due = self.instant_of(self.produced + MIN_RENDER);
            return;
        }
        let max = MAX_LAG.as_millis() as u64 * u64::from(RATE) / 1000;
        let mut cd = cd;
        if lag > max {
            self.flush(sb, opl);
            // Le disque, lui, continue de tourner pendant le temps sauté.
            if let Some(cd) = cd.as_deref_mut() {
                cd.skip(lag - max / 4, RATE);
            }
            self.produced = target - max / 4;
        }
        if self.buf.is_empty() {
            self.buf_t0 = self.instant_of(self.produced);
        }
        let n = (target - self.produced) as usize;
        self.produced = target;
        self.next_due = self.instant_of(target + MIN_RENDER);
        let at = self.buf.len();
        self.buf.resize(at + n * 2, 0.0);
        let out = &mut self.buf[at..];
        sb.render(dma, mem, RATE, out);
        if let Some(cd) = cd {
            cd.render(RATE, out);
        }
        // Haut-parleur : onde carrée ; au-delà de 20 kHz (modulation de
        // largeur d'impulsion), silence.
        if let Some(f) = speaker.filter(|f| *f < 20_000.0) {
            for frame in out.chunks_exact_mut(2) {
                let s = if self.speaker_phase < 0.5 { SPEAKER_LEVEL } else { -SPEAKER_LEVEL };
                frame[0] += s;
                frame[1] += s;
                self.speaker_phase = (self.speaker_phase + f / f64::from(RATE)).fract();
            }
        }
        if self.buf.len() >= BLOCK * 2 {
            self.flush(sb, opl);
        }
    }

    /// Confie le bloc en cours au thread audio.
    fn flush(&mut self, sb: &Sb16, opl: &mut Opl) {
        let block = Block {
            t0: self.buf_t0,
            frames: std::mem::take(&mut self.buf),
            writes: opl.take_writes(),
            fm_gain: sb.fm_gain().map(|g| g * FM_LEVEL),
        };
        if let Some(tx) = &self.tx {
            let _ = tx.send(block);
        }
    }
}

impl Drop for Mixer {
    /// Ferme le canal et attend le thread audio (fichier WAV écrit).
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Thread audio : FM ajoutée à chaque bloc, conversion, livraison.
fn audio_thread(rx: Receiver<Block>, mut sink: Box<dyn AudioSink + Send>) {
    let mut fm = FmSynth::new(RATE);
    let mut out = Vec::new();
    while let Ok(mut b) = rx.recv() {
        fm.queue(b.writes);
        fm.render(b.t0, b.fm_gain, &mut b.frames);
        out.clear();
        out.extend(b.frames.iter().map(|s| (soft_limit(s * OUTPUT_GAIN) * 32767.0) as i16));
        sink.push(&out);
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

#[cfg(test)]
mod tests {
    use super::soft_limit;

    #[test]
    fn soft_limiter() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.8), -0.8);
        assert!(soft_limit(0.9) < 0.9 && soft_limit(0.9) > 0.8);
        assert!(soft_limit(5.0) <= 1.0 && soft_limit(-5.0) >= -1.0);
    }
}
