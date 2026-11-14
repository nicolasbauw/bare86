// SPDX-License-Identifier: GPL-3.0-or-later
//! Musique General MIDI jouée sur OPL3 : libADLMIDI (third_party/libadlmidi,
//! émulateur Nuked OPL3, banques de timbres embarquées) en temps réel. Les
//! messages de la MPU-401 arrivent par `MidiSink` ; le mixeur demande les
//! échantillons par `AudioSource`. Puces séparées de l'OPL3 de la carte son,
//! comme un synthétiseur branché sur la sortie MIDI.

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int, c_long};
use std::rc::Rc;
use std::time::Instant;

use crate::audio::AudioSource;
use crate::mpu401::MidiSink;

#[repr(C)]
struct AdlPlayer {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn adl_init(sample_rate: c_long) -> *mut AdlPlayer;
    fn adl_close(dev: *mut AdlPlayer);
    fn adl_setBank(dev: *mut AdlPlayer, bank: c_int) -> c_int;
    fn adl_getBanksCount() -> c_int;
    fn adl_getBankNames() -> *const *const c_char;
    fn adl_setNumChips(dev: *mut AdlPlayer, chips: c_int) -> c_int;
    fn adl_rt_resetState(dev: *mut AdlPlayer);
    fn adl_rt_noteOn(dev: *mut AdlPlayer, channel: u8, note: u8, velocity: u8) -> c_int;
    fn adl_rt_noteOff(dev: *mut AdlPlayer, channel: u8, note: u8);
    fn adl_rt_noteAfterTouch(dev: *mut AdlPlayer, channel: u8, note: u8, value: u8);
    fn adl_rt_channelAfterTouch(dev: *mut AdlPlayer, channel: u8, value: u8);
    fn adl_rt_controllerChange(dev: *mut AdlPlayer, channel: u8, kind: u8, value: u8);
    fn adl_rt_patchChange(dev: *mut AdlPlayer, channel: u8, patch: u8);
    fn adl_rt_pitchBend(dev: *mut AdlPlayer, channel: u8, pitch: u16);
    fn adl_rt_systemExclusive(dev: *mut AdlPlayer, msg: *const u8, size: usize) -> c_int;
    fn adl_generate(dev: *mut AdlPlayer, sample_count: c_int, out: *mut i16) -> c_int;
}

/// Banque par défaut : The Fat Man 2op, celle du synthétiseur FM de
/// Windows 9x (« WOPL (The Fat Man 2op set; Win9x) »).
pub const DEFAULT_BANK: u32 = 58;

/// Noms des banques embarquées, dans l'ordre de leur numéro.
pub fn bank_names() -> Vec<String> {
    // SAFETY: tableau statique de `adl_getBanksCount()` chaînes C.
    unsafe {
        let n = adl_getBanksCount().max(0) as usize;
        let names = adl_getBankNames();
        (0..n).map(|i| CStr::from_ptr(*names.add(i)).to_string_lossy().into_owned()).collect()
    }
}

pub struct Synth {
    dev: *mut AdlPlayer,
    buf: Vec<i16>,
    /// Silence depuis la dernière note : le rendu continue un moment (fin
    /// des enveloppes), puis s'arrête pour ne rien coûter.
    idle_since: Option<Instant>,
    pub messages: u64,
}

pub type SharedSynth = Rc<RefCell<Synth>>;

impl Synth {
    pub fn new(rate: u32, bank: u32) -> Result<Self, String> {
        // SAFETY: appels de l'API C ; `dev` vérifié non nul avant usage.
        unsafe {
            let dev = adl_init(c_long::from(rate as i32));
            if dev.is_null() {
                return Err("libADLMIDI : initialisation impossible".into());
            }
            let s = Self { dev, buf: Vec::new(), idle_since: Some(Instant::now()), messages: 0 };
            if adl_setBank(dev, bank as c_int) != 0 {
                return Err(format!("banque MIDI {bank} inconnue (0 à {})", adl_getBanksCount() - 1));
            }
            // Quatre puces : polyphonie de 72 voix à deux opérateurs.
            adl_setNumChips(dev, 4);
            Ok(s)
        }
    }

    pub fn shared(self) -> SharedSynth {
        Rc::new(RefCell::new(self))
    }

    fn apply(&mut self, m: &[u8]) {
        let (status, ch) = (m[0] & 0xF0, m[0] & 0x0F);
        let d = |i: usize| m.get(i).copied().unwrap_or(0);
        // SAFETY: `dev` valide pendant toute la vie de `Synth`.
        unsafe {
            match status {
                0x80 => adl_rt_noteOff(self.dev, ch, d(1)),
                0x90 if d(2) == 0 => adl_rt_noteOff(self.dev, ch, d(1)),
                0x90 => {
                    adl_rt_noteOn(self.dev, ch, d(1), d(2));
                }
                0xA0 => adl_rt_noteAfterTouch(self.dev, ch, d(1), d(2)),
                0xB0 => adl_rt_controllerChange(self.dev, ch, d(1), d(2)),
                0xC0 => adl_rt_patchChange(self.dev, ch, d(1)),
                0xD0 => adl_rt_channelAfterTouch(self.dev, ch, d(1)),
                0xE0 => adl_rt_pitchBend(self.dev, ch, u16::from(d(1)) | (u16::from(d(2)) << 7)),
                _ if m[0] == 0xF0 => {
                    adl_rt_systemExclusive(self.dev, m.as_ptr(), m.len());
                }
                // Réinitialisation système (FFh, filtrée par la MPU) : rien.
                _ => {}
            }
        }
    }

}

/// Durée de rendu après la dernière note : fin des relâchements.
const TAIL: std::time::Duration = std::time::Duration::from_secs(5);

impl MidiSink for Synth {
    fn message(&mut self, _: Instant, msg: &[u8]) {
        self.messages += 1;
        if msg[0] & 0xF0 == 0x90 && msg.get(2).is_some_and(|&v| v > 0) {
            self.idle_since = None;
        } else if self.idle_since.is_none() && matches!(msg[0] & 0xF0, 0x80 | 0x90) {
            self.idle_since = Some(Instant::now());
        }
        self.apply(msg);
    }

    /// Toutes les notes coupées (réinitialisation de la MPU-401).
    fn reset(&mut self) {
        // SAFETY: `dev` valide.
        unsafe { adl_rt_resetState(self.dev) };
    }
}

impl AudioSource for Synth {
    fn render(&mut self, _rate: u32, out: &mut [f32]) {
        if self.idle_since.is_some_and(|t| t.elapsed() > TAIL) {
            return;
        }
        self.buf.resize(out.len(), 0);
        // SAFETY: `buf` contient `out.len()` échantillons (stéréo entrelacée).
        let n = unsafe { adl_generate(self.dev, out.len() as c_int, self.buf.as_mut_ptr()) }.max(0) as usize;
        let n = n.min(out.len());
        for (o, &s) in out.iter_mut().zip(&self.buf[..n]) {
            *o += f32::from(s) / 32768.0;
        }
    }
}

impl Drop for Synth {
    fn drop(&mut self) {
        // SAFETY: `dev` obtenu par adl_init, libéré une fois.
        unsafe { adl_close(self.dev) };
    }
}

/// Adaptateur : la MPU-401 parle à un synthétiseur partagé avec le mixeur.
pub struct SynthSink(pub SharedSynth);

impl MidiSink for SynthSink {
    fn message(&mut self, at: Instant, msg: &[u8]) {
        self.0.borrow_mut().message(at, msg);
    }

    fn reset(&mut self) {
        self.0.borrow_mut().reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banks_and_a_note() {
        let names = bank_names();
        assert!(names.len() > 70, "{} banques", names.len());
        assert!(names[DEFAULT_BANK as usize].contains("Win9x"), "{}", names[DEFAULT_BANK as usize]);
        let mut s = Synth::new(48_000, DEFAULT_BANK).unwrap();
        let mut out = vec![0f32; 2 * 4800];
        s.render(48_000, &mut out);
        assert!(out.iter().all(|&x| x == 0.0), "silence avant la première note");
        // Piano, do central, fort : du son ; puis note coupée.
        s.message(Instant::now(), &[0xC0, 0]);
        s.message(Instant::now(), &[0x90, 60, 127]);
        let mut out = vec![0f32; 2 * 4800];
        s.render(48_000, &mut out);
        let peak = out.iter().fold(0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.01, "crête {peak}");
        s.message(Instant::now(), &[0x80, 60, 0]);
        assert!(Synth::new(48_000, 10_000).is_err());
    }
}
