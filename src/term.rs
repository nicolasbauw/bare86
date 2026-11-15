// SPDX-License-Identifier: GPL-3.0-or-later
//! Écran texte de l'invité affiché en direct dans le terminal (`--term`),
//! en attendant la fenêtre SDL de l'étape 5 : écran alternatif, couleurs
//! CGA exactes (24 bits), CP437 converti, curseur du BIOS. Seules les
//! cellules modifiées sont réécrites.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bios;
use crate::mem::GuestMem;

/// Palette CGA / VGA texte par défaut.
const PALETTE: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0x00, 0x00, 0xAA),
    (0x00, 0xAA, 0x00),
    (0x00, 0xAA, 0xAA),
    (0xAA, 0x00, 0x00),
    (0xAA, 0x00, 0xAA),
    (0xAA, 0x55, 0x00),
    (0xAA, 0xAA, 0xAA),
    (0x55, 0x55, 0x55),
    (0x55, 0x55, 0xFF),
    (0x55, 0xFF, 0x55),
    (0x55, 0xFF, 0xFF),
    (0xFF, 0x55, 0x55),
    (0xFF, 0x55, 0xFF),
    (0xFF, 0xFF, 0x55),
    (0xFF, 0xFF, 0xFF),
];

/// Écran alternatif actif : le gestionnaire de Ctrl-C doit le quitter.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Séquence de sortie de l'écran alternatif.
pub const LEAVE: &[u8] = b"\x1b[0m\x1b[?25h\x1b[?1049l";

pub struct TermScreen {
    shown: Option<bios::TextSnapshot>,
    /// Mode graphique déjà signalé.
    graphics: Option<u8>,
}

impl TermScreen {
    pub fn new() -> Self {
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[?1049h\x1b[2J\x1b[H");
        let _ = out.flush();
        ACTIVE.store(true, Ordering::SeqCst);
        Self { shown: None, graphics: None }
    }

    pub fn render(&mut self, mem: &GuestMem) {
        let mut s = String::new();
        let Some(snap) = bios::text_snapshot(mem) else {
            let mode = mem.read_u8(0x449);
            if self.graphics != Some(mode) {
                self.graphics = Some(mode);
                self.shown = None;
                s.push_str(&format!("\x1b[0m\x1b[2J\x1b[H[graphics mode {mode:02X}h: not shown in the terminal]"));
                write_out(&s);
            }
            return;
        };
        self.graphics = None;
        let full = self.shown.as_ref().is_none_or(|p| p.cols != snap.cols || p.rows != snap.rows);
        if full {
            s.push_str("\x1b[0m\x1b[2J");
        }
        let mut attr = None;
        let mut pos = None;
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                let i = row * snap.cols + col;
                let cell = snap.cells[i];
                if !full && self.shown.as_ref().is_some_and(|p| p.cells[i] == cell) {
                    continue;
                }
                if pos != Some((row, col)) {
                    s.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
                }
                if attr != Some(cell.1) {
                    let (f, b) = (PALETTE[usize::from(cell.1 & 15)], PALETTE[usize::from(cell.1 >> 4 & 7)]);
                    s.push_str(&format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", f.0, f.1, f.2, b.0, b.1, b.2));
                    attr = Some(cell.1);
                }
                s.push(bios::cp437(cell.0));
                pos = Some((row, col + 1));
            }
        }
        let cursor_changed = self.shown.as_ref().is_none_or(|p| p.cursor != snap.cursor);
        if !s.is_empty() || cursor_changed {
            match snap.cursor {
                Some((r, c)) => s.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1)),
                None => s.push_str("\x1b[?25l"),
            }
            write_out(&s);
        }
        self.shown = Some(snap);
    }
}

fn write_out(s: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

impl Drop for TermScreen {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(LEAVE);
        let _ = out.flush();
        ACTIVE.store(false, Ordering::SeqCst);
    }
}
