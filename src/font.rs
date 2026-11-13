// SPDX-License-Identifier: GPL-3.0-or-later
//! Police du mode texte, chargée à l'exécution depuis un fichier PSF1 ou
//! PSF2 (éventuellement compressé gzip), par exemple une police de la
//! console Linux. Rien n'est embarqué : le choix d'une police libre à
//! intégrer reste une question ouverte (FAISABILITE.md). Les 256 caractères
//! CP437 sont retrouvés par la table Unicode de la police.

use std::io::Read;
use std::path::Path;

/// Police installée par le paquet kbd, essayée par défaut.
pub const DEFAULT_PATHS: &[&str] = &["/usr/share/kbd/consolefonts/default8x16.psfu.gz"];

pub struct Font {
    pub width: usize,
    pub height: usize,
    /// Glyphe de chaque caractère CP437 : `height` lignes, 1 bit par point
    /// (bit 7 = point de gauche), largeur ≤ 8.
    glyphs: Vec<[u8; 32]>,
}

impl Font {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read(path).map_err(|e| format!("{} : {e}", path.display()))?;
        let data = if raw.starts_with(&[0x1F, 0x8B]) {
            let mut d = Vec::new();
            flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut d).map_err(|e| format!("{} : {e}", path.display()))?;
            d
        } else {
            raw
        };
        Self::parse(&data).ok_or_else(|| format!("{} : police PSF illisible", path.display()))
    }

    /// Première police par défaut disponible.
    pub fn default_font() -> Option<Self> {
        DEFAULT_PATHS.iter().find_map(|p| Self::load(Path::new(p)).ok())
    }

    fn parse(d: &[u8]) -> Option<Self> {
        let le32 = |at: usize| Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize);
        // (glyphes, nombre, octets par glyphe, hauteur, largeur, table Unicode : glyphe → caractères)
        let (start, count, size, height, width, table): (usize, usize, usize, usize, usize, Vec<Vec<char>>);
        if d.starts_with(&[0x36, 0x04]) {
            let mode = *d.get(2)?;
            count = if mode & 1 != 0 { 512 } else { 256 };
            height = usize::from(*d.get(3)?);
            (start, size, width) = (4, height, 8);
            let mut t = vec![Vec::new(); count];
            if mode & 0x06 != 0 {
                let mut at = start + count * size;
                let mut g = 0;
                while g < count && at + 1 < d.len() {
                    let u = u16::from_le_bytes([d[at], d[at + 1]]);
                    at += 2;
                    match u {
                        0xFFFF => g += 1,
                        0xFFFE => {}
                        _ => t[g].extend(char::from_u32(u32::from(u))),
                    }
                }
            }
            table = t;
        } else if d.starts_with(&[0x72, 0xB5, 0x4A, 0x86]) {
            start = le32(8)?;
            let flags = le32(12)?;
            count = le32(16)?;
            size = le32(20)?;
            height = le32(24)?;
            width = le32(28)?;
            let mut t = vec![Vec::new(); count];
            if flags & 1 != 0 {
                let mut at = start + count * size;
                let mut g = 0;
                let mut seq = false;
                while g < count && at < d.len() {
                    match d[at] {
                        0xFF => {
                            g += 1;
                            seq = false;
                            at += 1;
                        }
                        0xFE => {
                            seq = true;
                            at += 1;
                        }
                        b => {
                            let len = match b {
                                0x00..=0x7F => 1,
                                0xC0..=0xDF => 2,
                                0xE0..=0xEF => 3,
                                _ => 4,
                            };
                            if let Some(c) = d.get(at..at + len).and_then(|s| std::str::from_utf8(s).ok()).and_then(|s| s.chars().next())
                                && !seq
                            {
                                t[g].push(c);
                            }
                            at += len;
                        }
                    }
                }
            }
            table = t;
        } else {
            return None;
        }
        if width > 8 || height > 32 || height == 0 || d.len() < start + count * size {
            return None;
        }
        let glyph = |g: usize| {
            let mut out = [0u8; 32];
            let row_bytes = width.div_ceil(8);
            for (y, row) in out.iter_mut().enumerate().take(height) {
                *row = d[start + g * size + y * row_bytes];
            }
            out
        };
        let has_table = table.iter().any(|t| !t.is_empty());
        let glyphs = (0..=255u8)
            .map(|b| {
                let want = crate::bios::cp437(b);
                let g = if b == 0 || !has_table {
                    usize::from(b).min(count - 1)
                } else {
                    table.iter().position(|t| t.contains(&want)).unwrap_or(usize::from(b).min(count - 1))
                };
                glyph(g)
            })
            .collect();
        Some(Self { width, height, glyphs })
    }

    /// Ligne `y` du glyphe du caractère CP437 `c`.
    pub fn row(&self, c: u8, y: usize) -> u8 {
        self.glyphs[usize::from(c)][y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psf1_with_unicode_table() {
        // Police de 256 glyphes de 2 lignes : le glyphe 7 porte « █ », le 3 « A ».
        let mut d = vec![0x36, 0x04, 0x02, 2];
        for g in 0..256u16 {
            d.extend([g as u8, !(g as u8)]);
        }
        for g in 0..256u16 {
            match g {
                3 => d.extend(0x41u16.to_le_bytes()),
                7 => d.extend(0x2588u16.to_le_bytes()),
                _ => {}
            }
            d.extend(0xFFFFu16.to_le_bytes());
        }
        let f = Font::parse(&d).unwrap();
        assert_eq!((f.width, f.height), (8, 2));
        assert_eq!(f.row(b'A', 0), 3);
        assert_eq!(f.row(0xDB, 1), !7);
    }

    #[test]
    fn system_font_if_present() {
        if let Some(f) = Font::default_font() {
            assert_eq!((f.width, f.height), (8, 16));
            // « █ » (DBh) est plein, l'espace est vide.
            assert_eq!(f.row(0xDB, 8), 0xFF);
            assert_eq!(f.row(b' ', 8), 0x00);
        }
    }
}
