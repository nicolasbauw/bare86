// SPDX-License-Identifier: GPL-3.0-or-later
//! Entrées/sorties hôte derrière des traits : SDL3 (étape 5) ou headless
//! (tests automatisés). Le thread vCPU ne dépend que de ces traits.

// Branchés aux étapes 4 (clavier, souris) et 5 (vidéo, son).
#![allow(dead_code)]

use std::collections::VecDeque;

/// Image produite par la carte vidéo émulée, pixels 0x00RRGGBB.
pub struct Frame<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a [u32],
}

pub trait Display {
    fn present(&mut self, frame: &Frame<'_>);
}

pub trait AudioSink {
    fn sample_rate(&self) -> u32;
    /// Échantillons stéréo entrelacés.
    fn push(&mut self, samples: &[i16]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Scancode PC (jeu 1), relâchement = bit 7.
    Key(u8),
    Mouse { dx: i32, dy: i32, buttons: u8 },
    Quit,
}

pub trait InputSource {
    fn poll(&mut self) -> Option<InputEvent>;
}

/// Sortie caractère de DOS en attendant le mode texte (étape 2).
pub trait TextSink {
    fn write(&mut self, bytes: &[u8]);
}

pub struct Host {
    pub display: Box<dyn Display>,
    pub audio: Box<dyn AudioSink>,
    pub input: Box<dyn InputSource>,
    pub text: Box<dyn TextSink>,
}

/// Dernière image conservée, pour les captures des tests.
#[derive(Default)]
pub struct HeadlessDisplay {
    pub last: Option<(usize, usize, Vec<u32>)>,
    pub frames: u64,
}

impl Display for HeadlessDisplay {
    fn present(&mut self, frame: &Frame<'_>) {
        self.last = Some((frame.width, frame.height, frame.pixels.to_vec()));
        self.frames += 1;
    }
}

/// Son compté puis jeté.
#[derive(Default)]
pub struct NullAudio {
    pub samples: u64,
}

impl AudioSink for NullAudio {
    fn sample_rate(&self) -> u32 {
        48_000
    }
    fn push(&mut self, samples: &[i16]) {
        self.samples += samples.len() as u64;
    }
}

/// Événements scriptés.
#[derive(Default)]
pub struct ScriptedInput(pub VecDeque<InputEvent>);

impl InputSource for ScriptedInput {
    fn poll(&mut self) -> Option<InputEvent> {
        self.0.pop_front()
    }
}

pub struct Stdout;

impl TextSink for Stdout {
    fn write(&mut self, bytes: &[u8]) {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}

/// Sortie texte capturée (tests).
#[derive(Clone, Default)]
pub struct Capture(pub std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl TextSink for Capture {
    fn write(&mut self, bytes: &[u8]) {
        self.0.borrow_mut().extend_from_slice(bytes);
    }
}

impl Host {
    /// Sans fenêtre ni son ; texte vers `text`.
    pub fn headless(text: Box<dyn TextSink>) -> Self {
        Self {
            display: Box::new(HeadlessDisplay::default()),
            audio: Box::new(NullAudio::default()),
            input: Box::new(ScriptedInput::default()),
            text,
        }
    }
}
