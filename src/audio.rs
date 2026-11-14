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
use crate::opl::{FmSynth, Opl, OplModel, Write};
use crate::sb16::Sb16;

pub const RATE: u32 = 48_000;

/// Source ajoutée au mélange (pistes audio du CD, synthétiseur MIDI).
pub trait AudioSource {
    /// Ajoute `out.len() / 2` trames stéréo entrelacées à `rate` Hz.
    fn render(&mut self, rate: u32, out: &mut [f32]);
    /// Avance de `frames` trames sans les rendre (retard du mixeur rattrapé).
    fn skip(&mut self, _frames: u64, _rate: u32) {}
}
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
    /// Filtre analogique de la sortie FM (SB Pro).
    fm_filter: bool,
}

/// Filtres analogiques de la Sound Blaster Pro, d'après les mesures de
/// DOSBox Staging sur du vrai matériel (src/hardware/sblaster.cpp, v0.82.2) :
/// FM passe-bas du 1er ordre à 8 kHz, DSP passe-bas du 2e ordre à 3,2 kHz.
/// La SB16 n'a pas de tels filtres.
const PRO_FM_CUTOFF: f32 = 8_000.0;
const PRO_DSP_CUTOFF: f32 = 3_200.0;

/// Passe-bas du 1er ordre (6 dB/octave), stéréo.
pub struct OnePole {
    a: f32,
    y: [f32; 2],
}

impl OnePole {
    pub fn new(cutoff: f32, rate: u32) -> Self {
        Self { a: 1.0 - (-2.0 * std::f32::consts::PI * cutoff / rate as f32).exp(), y: [0.0; 2] }
    }

    pub fn process(&mut self, frames: &mut [f32]) {
        for f in frames.chunks_exact_mut(2) {
            for c in 0..2 {
                self.y[c] += self.a * (f[c] - self.y[c]);
                f[c] = self.y[c];
            }
        }
    }
}

/// Passe-bas de Butterworth du 2e ordre (12 dB/octave), stéréo (biquad,
/// formules de R. Bristow-Johnson).
pub struct Butterworth2 {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

impl Butterworth2 {
    pub fn new(cutoff: f32, rate: u32) -> Self {
        let w = 2.0 * std::f32::consts::PI * cutoff / rate as f32;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let (cw, a0) = (w.cos(), 1.0 + alpha);
        let b1 = (1.0 - cw) / a0;
        Self { b: [b1 / 2.0, b1, b1 / 2.0], a: [-2.0 * cw / a0, (1.0 - alpha) / a0], z: [[0.0; 2]; 2] }
    }

    pub fn process(&mut self, frames: &mut [f32]) {
        for f in frames.chunks_exact_mut(2) {
            for c in 0..2 {
                // Forme directe transposée II.
                let x = f[c];
                let y = self.b[0] * x + self.z[0][c];
                self.z[0][c] = self.b[1] * x - self.a[0] * y + self.z[1][c];
                self.z[1][c] = self.b[2] * x - self.a[1] * y;
                f[c] = y;
            }
        }
    }
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
    /// Filtre de la sortie numérique de la SB Pro, et tampon de rendu du DSP.
    dsp_filter: Butterworth2,
    dsp_buf: Vec<f32>,
}

impl Mixer {
    /// Lance le thread audio, qui livre à `sink`.
    pub fn new(sink: Box<dyn AudioSink + Send>, fm_model: OplModel) -> Self {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new().name("audio".into()).spawn(move || audio_thread(rx, sink, fm_model)).ok();
        let now = Instant::now();
        Self {
            start: now,
            produced: 0,
            speaker_phase: 0.0,
            buf: Vec::new(),
            buf_t0: now,
            next_due: now,
            tx: Some(tx),
            thread,
            dsp_filter: Butterworth2::new(PRO_DSP_CUTOFF, RATE),
            dsp_buf: Vec::new(),
        }
    }

    fn instant_of(&self, frame: u64) -> Instant {
        self.start + Duration::from_nanos(frame * 1_000_000_000 / u64::from(RATE))
    }

    /// Produit les trames dues à `now`.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn advance(&mut self, now: Instant, sb: &mut Sb16, opl: &mut Opl, dma: &mut Dma, mem: &GuestMem, speaker: Option<f64>, sources: &mut [&mut dyn AudioSource]) {
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
        if lag > max {
            self.flush(sb, opl);
            // Le disque, lui, continue de tourner pendant le temps sauté.
            for s in sources.iter_mut() {
                s.skip(lag - max / 4, RATE);
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
        if sb.pro_dsp_filter() {
            // SB Pro : le DSP passe par son filtre avant le reste du mélange.
            self.dsp_buf.clear();
            self.dsp_buf.resize(out.len(), 0.0);
            sb.render(dma, mem, RATE, &mut self.dsp_buf);
            self.dsp_filter.process(&mut self.dsp_buf);
            for (o, d) in out.iter_mut().zip(&self.dsp_buf) {
                *o += d;
            }
        } else {
            sb.render(dma, mem, RATE, out);
        }
        for s in sources.iter_mut() {
            s.render(RATE, out);
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
            fm_filter: sb.pro_fm_filter(),
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
fn audio_thread(rx: Receiver<Block>, mut sink: Box<dyn AudioSink + Send>, fm_model: OplModel) {
    let mut fm = FmSynth::new(RATE, fm_model);
    let mut fm_filter = OnePole::new(PRO_FM_CUTOFF, RATE);
    let mut fm_buf = Vec::new();
    let mut out = Vec::new();
    while let Ok(mut b) = rx.recv() {
        fm.queue(b.writes);
        if b.fm_filter {
            fm_buf.clear();
            fm_buf.resize(b.frames.len(), 0.0);
            fm.render(b.t0, b.fm_gain, &mut fm_buf);
            fm_filter.process(&mut fm_buf);
            for (o, f) in b.frames.iter_mut().zip(&fm_buf) {
                *o += f;
            }
        } else {
            fm.render(b.t0, b.fm_gain, &mut b.frames);
        }
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
    use super::*;

    #[test]
    fn soft_limiter() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.8), -0.8);
        assert!(soft_limit(0.9) < 0.9 && soft_limit(0.9) > 0.8);
        assert!(soft_limit(5.0) <= 1.0 && soft_limit(-5.0) >= -1.0);
    }

    /// Gain (dB) d'une sinusoïde à `freq` à travers `filter`.
    fn gain_db(freq: f32, mut filter: impl FnMut(&mut [f32])) -> f32 {
        let n = RATE as usize;
        let mut x: Vec<f32> = (0..n).flat_map(|i| {
            let v = (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin();
            [v, v]
        }).collect();
        filter(&mut x);
        // Seconde moitié (régime établi), voie gauche.
        let rms = (x[n..].iter().step_by(2).map(|v| v * v).sum::<f32>() / (n / 2) as f32).sqrt();
        20.0 * (rms * std::f32::consts::SQRT_2).log10()
    }

    #[test]
    fn sound_blaster_pro_filters() {
        let g = |f| gain_db(f, |x| OnePole::new(PRO_FM_CUTOFF, RATE).process(x));
        assert!(g(500.0) > -0.2, "{}", g(500.0));
        assert!((g(8_000.0) + 3.0).abs() < 1.0, "{}", g(8_000.0));
        let d = |f| gain_db(f, |x| Butterworth2::new(PRO_DSP_CUTOFF, RATE).process(x));
        assert!(d(500.0) > -0.2, "{}", d(500.0));
        assert!((d(3_200.0) + 3.0).abs() < 0.5, "{}", d(3_200.0));
        // Deux octaves au-dessus : au moins −24 dB (12 dB/octave ; un peu plus
        // en numérique, la transformation bilinéaire ayant un zéro à Nyquist).
        assert!((-31.0..-23.0).contains(&d(12_800.0)), "{}", d(12_800.0));
    }
}
