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

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// Scancode PC (jeu 1), relâchement = bit 7.
    Key(u8),
    /// Caractère tapé au terminal (sans scancode d'origine).
    Text(u8),
    Mouse { dx: i32, dy: i32, buttons: u8 },
    /// Pause d'un script (millisecondes) : l'entrée suivante attend.
    Wait(u32),
    /// Vitesse réglée à chaud (MIPS ; `None` : sans bridage).
    Speed(Option<f64>),
    /// Disquette suivante dans A: (Ctrl+F4, `{disk}`).
    NextDisk,
    /// Script : l'entrée suivante attend que l'écran texte affiche ce texte.
    Until(String),
    /// Joystick : manches branchés (0, 1 ou 2), axe (n°, valeur SDL), bouton.
    JoyPresent(u8),
    JoyAxis(u8, i16),
    JoyButton(u8, bool),
    /// Fin de la session : fenêtre fermée, entrée épuisée, fin d'un script.
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
    pub audio: Box<dyn AudioSink + Send>,
    pub input: Box<dyn InputSource>,
    pub text: Box<dyn TextSink>,
    /// Écran texte affiché en direct dans le terminal (`--term`).
    pub term: Option<crate::term::TermScreen>,
    /// Fin de session demandée (`InputEvent::Quit` reçu).
    pub quit: bool,
    /// L'affichage veut des images (fenêtre) : la machine en produit à 70 Hz.
    pub wants_frames: bool,
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

/// Script épuisé : fin de session.
impl InputSource for ScriptedInput {
    fn poll(&mut self) -> Option<InputEvent> {
        Some(self.0.pop_front().unwrap_or(InputEvent::Quit))
    }
}

/// Clavier du terminal : caractères tapés, sans écho ni attente de ligne ;
/// flèches, touches de fonction et Alt+lettre décodées de leurs séquences
/// d'échappement. Le terminal est rétabli à la fin, ou sur Ctrl-C.
pub struct StdinInput {
    pending: VecDeque<InputEvent>,
}

static SAVED_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

extern "C" fn restore_and_exit(_: libc::c_int) {
    if crate::term::ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
        let s = crate::term::LEAVE;
        // SAFETY: write est utilisable dans un gestionnaire de signal.
        unsafe { libc::write(1, s.as_ptr().cast(), s.len()) };
    }
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
        Self { pending: VecDeque::new() }
    }

    /// Un octet ; `Some(None)` à la fin de l'entrée, `None` si rien n'arrive.
    fn byte(timeout_ms: i32) -> Option<Option<u8>> {
        let mut fd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let mut b = 0u8;
        // SAFETY: lecture d'un octet dans `b`.
        unsafe {
            if libc::poll(&mut fd, 1, timeout_ms) <= 0 {
                return None;
            }
            if libc::read(0, (&raw mut b).cast(), 1) != 1 {
                return Some(None);
            }
        }
        Some(Some(b))
    }

    /// Appui puis relâchement (le terminal ne signale pas les relâchements).
    fn tap(&mut self, scan: u8, with: Option<u8>) {
        use InputEvent::Key;
        self.pending.extend(with.map(Key));
        self.pending.extend([Key(scan), Key(scan | 0x80)]);
        self.pending.extend(with.map(|m| Key(m | 0x80)));
    }

    /// Séquence après ESC : flèches, Origine/Fin, Inser/Suppr, Pg, F1–F12,
    /// Alt+touche ; ESC seul si rien ne suit.
    fn escape(&mut self) {
        let Some(Some(b)) = Self::byte(20) else {
            self.tap(0x01, None);
            return;
        };
        if b != b'[' && b != b'O' {
            // Alt+caractère.
            if let Some(w) = crate::keyboard::char_word(b.to_ascii_lowercase()) {
                self.tap((w >> 8) as u8, Some(0x38));
            }
            return;
        }
        let mut seq = Vec::new();
        while let Some(Some(c)) = Self::byte(20) {
            seq.push(c);
            if c.is_ascii_alphabetic() || c == b'~' || seq.len() > 6 {
                break;
            }
        }
        let scan = match seq.as_slice() {
            b"A" => 0x48,
            b"B" => 0x50,
            b"C" => 0x4D,
            b"D" => 0x4B,
            b"H" | b"1~" | b"7~" => 0x47,
            b"F" | b"4~" | b"8~" => 0x4F,
            b"2~" => 0x52,
            b"3~" => 0x53,
            b"5~" => 0x49,
            b"6~" => 0x51,
            b"P" | b"11~" => 0x3B,
            b"Q" | b"12~" => 0x3C,
            b"R" | b"13~" => 0x3D,
            b"S" | b"14~" => 0x3E,
            b"15~" => 0x3F,
            b"17~" => 0x40,
            b"18~" => 0x41,
            b"19~" => 0x42,
            b"20~" => 0x43,
            b"21~" => 0x44,
            b"23~" => 0x57,
            b"24~" => 0x58,
            _ => return,
        };
        self.tap(scan, None);
    }

    fn read(&mut self, timeout_ms: i32) -> Option<InputEvent> {
        if let Some(ev) = self.pending.pop_front() {
            return Some(ev);
        }
        match Self::byte(timeout_ms)? {
            // Fin de l'entrée (tube vidé, Ctrl-D) : fin de session.
            None => Some(InputEvent::Quit),
            Some(0x1B) if libc_isatty() => {
                self.escape();
                self.pending.pop_front()
            }
            Some(b) => Some(InputEvent::Text(b)),
        }
    }
}

fn libc_isatty() -> bool {
    // SAFETY: simple test sur l'entrée standard.
    unsafe { libc::isatty(0) == 1 }
}

impl InputSource for StdinInput {
    fn poll(&mut self) -> Option<InputEvent> {
        self.read(0)
    }
    fn wait(&mut self) -> Option<InputEvent> {
        loop {
            if let Some(ev) = self.read(-1) {
                return Some(ev);
            }
        }
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

/// Sortie texte ignorée (l'écran est affiché par ailleurs).
pub struct Discard;

impl TextSink for Discard {
    fn write(&mut self, _: &[u8]) {}
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
            term: None,
            quit: false,
            wants_frames: false,
        }
    }
}
