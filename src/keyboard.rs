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

/// Scancodes (appui, relâchement) d'un caractère tapé au terminal, avec Maj
/// ou Ctrl autour si le caractère l'exige.
pub fn text_scancodes(c: u8) -> Vec<u8> {
    let Some(w) = char_word(c) else { return Vec::new() };
    let scan = (w >> 8) as u8;
    let ascii = w as u8;
    let modifier = if ascii != 0 && NORMAL.get(usize::from(scan)) != Some(&ascii) && SHIFT.get(usize::from(scan)) == Some(&ascii) {
        Some(0x2A)
    } else if (1..=26).contains(&ascii) && !matches!(ascii, 0x08 | 0x09 | 0x0D) {
        Some(0x1D)
    } else {
        None
    };
    let mut v = Vec::new();
    v.extend(modifier);
    v.extend([scan, scan | 0x80]);
    v.extend(modifier.map(|m| m | 0x80));
    v
}

/// Script de touches pour `--keys` : texte tapé tel quel, touches nommées
/// entre accolades (`{enter}`, `{esc}`, `{f1}`, `{up}`, `{alt+f}`,
/// `{ctrl+c}`…), pauses (`{wait:500}`, en ms), vitesse (`{speed:25}` en
/// MIPS, `{speed:max}`), `{{` pour une accolade. Une
/// fin de session suit le script.
pub fn parse_script(spec: &str) -> Result<Vec<crate::host::InputEvent>, String> {
    use crate::host::InputEvent::{Key, Quit, Text};
    let mut out = Vec::new();
    let mut rest = spec.as_bytes();
    while let Some((&c, tail)) = rest.split_first() {
        rest = tail;
        if c != b'{' || rest.first() == Some(&b'{') {
            if c == b'{' {
                rest = &rest[1..];
            }
            out.push(Text(if c == b'\n' { b'\r' } else { c }));
            continue;
        }
        let end = rest.iter().position(|&c| c == b'}').ok_or("« { » sans « } »")?;
        let name = String::from_utf8_lossy(&rest[..end]).to_ascii_lowercase();
        rest = &rest[end + 1..];
        if let Some(ms) = name.strip_prefix("wait:") {
            out.push(crate::host::InputEvent::Wait(ms.parse().map_err(|_| format!("durée invalide : {ms}"))?));
            continue;
        }
        if let Some(v) = name.strip_prefix("speed:") {
            let mips = if v == "max" { None } else { Some(v.parse().map_err(|_| format!("vitesse invalide : {v}"))?) };
            out.push(crate::host::InputEvent::Speed(mips));
            continue;
        }
        let (mods, key) = match name.rsplit_once('+') {
            Some((m, k)) => (m.to_string(), k.to_string()),
            None => (String::new(), name.clone()),
        };
        let scan = match key.as_str() {
            "enter" => 0x1C,
            "esc" => 0x01,
            "tab" => 0x0F,
            "bs" | "backspace" => 0x0E,
            "space" => 0x39,
            "up" => 0x48,
            "down" => 0x50,
            "left" => 0x4B,
            "right" => 0x4D,
            "home" => 0x47,
            "end" => 0x4F,
            "pgup" => 0x49,
            "pgdn" => 0x51,
            "ins" => 0x52,
            "del" => 0x53,
            k if k.len() == 1 => {
                let ch = k.as_bytes()[0];
                NORMAL.iter().position(|&x| x == ch && ch != 0).ok_or(format!("touche inconnue : {k}"))? as u8
            }
            k if k.starts_with('f') => match k[1..].parse::<u8>() {
                Ok(n @ 1..=10) => 0x3A + n,
                Ok(11) => 0x57,
                Ok(12) => 0x58,
                _ => return Err(format!("touche inconnue : {k}")),
            },
            k => return Err(format!("touche inconnue : {k}")),
        };
        let mods: Vec<u8> = mods
            .split('+')
            .filter(|m| !m.is_empty())
            .map(|m| match m {
                "alt" => Ok(0x38),
                "ctrl" => Ok(0x1D),
                "shift" => Ok(0x2A),
                _ => Err(format!("modificateur inconnu : {m}")),
            })
            .collect::<Result<_, _>>()?;
        out.extend(mods.iter().map(|&m| Key(m)));
        out.push(Key(scan));
        out.push(Key(scan | 0x80));
        out.extend(mods.iter().rev().map(|&m| Key(m | 0x80)));
    }
    out.push(Quit);
    Ok(out)
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

    #[test]
    fn script() {
        use crate::host::InputEvent::{Key, Quit, Text};
        let ev = parse_script("a{alt+f}{{{enter}").unwrap();
        assert_eq!(ev, [Text(b'a'), Key(0x38), Key(0x21), Key(0xA1), Key(0xB8), Text(b'{'), Key(0x1C), Key(0x9C), Quit]);
        assert!(parse_script("{nope}").is_err());
        assert_eq!(parse_script("{wait:250}").unwrap(), [crate::host::InputEvent::Wait(250), Quit]);
    }

    #[test]
    fn text_to_scancodes() {
        assert_eq!(text_scancodes(b'a'), [0x1E, 0x9E]);
        assert_eq!(text_scancodes(b'A'), [0x2A, 0x1E, 0x9E, 0xAA]);
        assert_eq!(text_scancodes(b'\n'), [0x1C, 0x9C]);
        assert_eq!(text_scancodes(3), [0x1D, 0x2E, 0xAE, 0x9D]);
        assert_eq!(text_scancodes(b'*'), [0x37, 0xB7]);
    }
}
