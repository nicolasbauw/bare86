// SPDX-License-Identifier: GPL-3.0-or-later
//! Clavier US : scancodes du jeu 1 et mots clavier du BIOS
//! (scancode << 8 | ASCII), tels que les rend INT 16h.

/// Caractère sans Maj, par scancode (00h–39h) ; 0 : pas de caractère.
const NORMAL: &[u8; 0x3A] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\r\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
/// Caractère avec Maj.
const SHIFT: &[u8; 0x3A] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

/// Bits de l'état des touches de modification (BDA 0417h).
pub const RSHIFT: u8 = 1 << 0;
pub const LSHIFT: u8 = 1 << 1;
pub const CTRL: u8 = 1 << 2;
pub const ALT: u8 = 1 << 3;
pub const CAPS: u8 = 1 << 6;

/// Bit d'état d'une touche de modification (appui / relâchement suivis).
pub fn modifier(scan: u8) -> Option<u8> {
    match scan {
        0x2A => Some(LSHIFT),
        0x36 => Some(RSHIFT),
        0x1D => Some(CTRL),
        0x38 => Some(ALT),
        _ => None,
    }
}

/// Mot clavier pour l'appui sur `scan` selon l'état `shift` ; `None` pour
/// les touches sans caractère ni code étendu (verrouillages, modificateurs).
pub fn key_word(scan: u8, shift: u8) -> Option<u16> {
    let s = u16::from(scan) << 8;
    let shifted = shift & (LSHIFT | RSHIFT) != 0;
    match scan {
        // F1–F10 : codes étendus 3Bh, 54h (Maj), 5Eh (Ctrl), 68h (Alt).
        0x3B..=0x44 => {
            let base = if shift & ALT != 0 {
                0x68
            } else if shift & CTRL != 0 {
                0x5E
            } else if shifted {
                0x54
            } else {
                0x3B
            };
            Some(u16::from(base + scan - 0x3B) << 8)
        }
        // F11, F12.
        0x57 | 0x58 => Some(u16::from(scan + 0x2E) << 8),
        0x4A => Some(s | u16::from(b'-')),
        0x4E => Some(s | u16::from(b'+')),
        // Pavé : flèches, Origine, Fin, Pg, Inser, Suppr (sans Verr Num).
        0x47..=0x53 => Some(s),
        0x0F if shifted => Some(s),
        _ => {
            let ch = *NORMAL.get(usize::from(scan))?;
            if ch == 0 {
                return None;
            }
            if shift & ALT != 0 {
                return Some(s);
            }
            if shift & CTRL != 0 {
                return match ch {
                    b'a'..=b'z' => Some(s | u16::from(ch & 0x1F)),
                    b'\r' => Some(s | 0x0A),
                    0x08 => Some(s | 0x7F),
                    0x1B => Some(s | 0x1B),
                    _ => None,
                };
            }
            let caps = shift & CAPS != 0 && ch.is_ascii_alphabetic();
            let table = if shifted != caps { SHIFT } else { NORMAL };
            Some(s | u16::from(table[usize::from(scan)]))
        }
    }
}

/// Mot clavier pour un caractère tapé au terminal (recherche inverse).
pub fn char_word(c: u8) -> Option<u16> {
    let at = |i: usize| Some(((i as u16) << 8) | u16::from(c));
    match c {
        b'\n' => return Some(0x1C0D),
        0x7F => return Some(0x0E08),
        0 => return None,
        _ => {}
    }
    if let Some(i) = NORMAL.iter().position(|&x| x == c) {
        return at(i);
    }
    if let Some(i) = SHIFT.iter().position(|&x| x == c) {
        return at(i);
    }
    if (1..=26).contains(&c) {
        return NORMAL.iter().position(|&x| x == b'a' + c - 1).and_then(at);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(key_word(0x1E, 0), Some(0x1E61));
        assert_eq!(key_word(0x1E, LSHIFT), Some(0x1E41));
        assert_eq!(key_word(0x1E, CAPS | RSHIFT), Some(0x1E61));
        assert_eq!(key_word(0x1E, CTRL), Some(0x1E01));
        assert_eq!(key_word(0x1E, ALT), Some(0x1E00));
        assert_eq!(key_word(0x3B, 0), Some(0x3B00));
        assert_eq!(key_word(0x3C, ALT), Some(0x6900));
        assert_eq!(key_word(0x2A, 0), None);
        assert_eq!(char_word(b'H'), Some(0x2348));
        assert_eq!(char_word(b'\n'), Some(0x1C0D));
        assert_eq!(char_word(3), Some(0x2E03));
        assert_eq!(char_word(b'~'), Some(0x297E));
    }
}
