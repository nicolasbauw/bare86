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
    /// Caractère tapé au terminal (sans scancode d'origine).
    Text(u8),
    Mouse { dx: i32, dy: i32, buttons: u8 },
    Quit,
}

pub trait InputSource {
    /// Événement en attente, sans bloquer.
    fn poll(&mut self) -> Option<InputEvent>;
    /// Attend le prochain événement ; `None` : plus aucune entrée possible.
    fn wait(&mut self) -> Option<InputEvent> {
        self.poll()
    }
}

/// Recopie de la sortie télétype du BIOS, en attendant l'affichage (étape 5).
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

/// Clavier du terminal : caractères tapés, sans écho ni attente de ligne.
/// Le terminal est rétabli à la fin, ou sur Ctrl-C.
pub struct StdinInput;

static SAVED_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

extern "C" fn restore_and_exit(_: libc::c_int) {
    if let Some(t) = SAVED_TERMIOS.get() {
        // SAFETY: tcsetattr et _exit sont utilisables dans un gestionnaire de signal.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, t) };
    }
    unsafe { libc::_exit(130) };
}

impl StdinInput {
    pub fn new() -> Self {
        // SAFETY: appels termios sur l'entrée standard, état sauvegardé avant.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::isatty(0) == 1 && libc::tcgetattr(0, &mut t) == 0 && SAVED_TERMIOS.set(t).is_ok() {
                let mut raw = t;
                raw.c_lflag &= !(libc::ICANON | libc::ECHO);
                raw.c_cc[libc::VMIN] = 1;
                raw.c_cc[libc::VTIME] = 0;
                libc::tcsetattr(0, libc::TCSANOW, &raw);
                libc::signal(libc::SIGINT, restore_and_exit as extern "C" fn(libc::c_int) as libc::sighandler_t);
            }
        }
        Self
    }

    fn read(&mut self, timeout_ms: i32) -> Option<InputEvent> {
        let mut fd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let mut b = 0u8;
        // SAFETY: lecture d'un octet dans `b`.
        unsafe {
            if libc::poll(&mut fd, 1, timeout_ms) <= 0 || libc::read(0, (&raw mut b).cast(), 1) != 1 {
                return None;
            }
        }
        Some(InputEvent::Text(b))
    }
}

impl InputSource for StdinInput {
    fn poll(&mut self) -> Option<InputEvent> {
        self.read(0)
    }
    fn wait(&mut self) -> Option<InputEvent> {
        self.read(-1)
    }
}

impl Drop for StdinInput {
    fn drop(&mut self) {
        if let Some(t) = SAVED_TERMIOS.get() {
            // SAFETY: rétablit l'état sauvegardé par `new`.
            unsafe { libc::tcsetattr(0, libc::TCSANOW, t) };
        }
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
    /// Sans fenêtre ni son ; clavier du terminal, texte vers la sortie standard.
    pub fn terminal() -> Self {
        Self { input: Box::new(StdinInput::new()), ..Self::headless(Box::new(Stdout)) }
    }

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
