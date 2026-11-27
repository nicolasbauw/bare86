// SPDX-License-Identifier: GPL-3.0-or-later
//! INT 10h en modes graphiques : pixels (AH=0Ch/0Dh), caractères (AH=09h/0Ah/0Eh/13h) et défilement.
//!
//! Le BIOS d'origine dessine dans la mémoire vidéo ; ici l'hôte écrit directement dans les plans (modes 0Dh, 0Eh, 10h,
//! 12h), le tampon linéaire (mode 13h) ou la RAM B8000 (CGA 04h–06h). Les caractères viennent de la police pointée par
//! INT 43h (8×8 en ROM pour les modes de 200 lignes), sur `CHAR_HEIGHT` lignes.

use super::{GFX_VGA, bda};
use crate::mem::GuestMem;
use crate::vga::{LINEAR, Vga};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Mode 13h : un octet par pixel.
    Linear,
    /// Modes 16 couleurs : 4 plans, `stride` octets par ligne.
    Planar(usize),
    /// CGA 320×200, 2 bits par pixel (lignes paires/impaires séparées).
    Cga4,
    /// CGA 640×200, 1 bit par pixel.
    Cga2,
}

/// (largeur, hauteur, type) d'un mode graphique que ce module sait dessiner.
pub fn geometry(mode: u8) -> Option<(usize, usize, Kind)> {
    Some(match mode {
        4 | 5 => (320, 200, Kind::Cga4),
        6 => (640, 200, Kind::Cga2),
        0x0D => (320, 200, Kind::Planar(40)),
        0x0E => (640, 200, Kind::Planar(80)),
        0x10 => (640, 350, Kind::Planar(80)),
        0x12 => (640, 480, Kind::Planar(80)),
        0x13 => (320, 200, Kind::Linear),
        _ => return None,
    })
}

/// Exécute `f` sur la VGA de la machine (sans effet si elle n'est pas enregistrée ou déjà empruntée).
pub fn with_vga<R>(f: impl FnOnce(&mut Vga) -> R) -> Option<R> {
    GFX_VGA.with(|g| {
        let g = g.borrow();
        let mut v = g.as_ref()?.try_borrow_mut().ok()?;
        Some(f(&mut v))
    })
}

const CGA: usize = 0xB_8000;

pub fn put_pixel(vga: &mut Vga, mem: &GuestMem, kind: Kind, x: usize, y: usize, color: u8, xor: bool) {
    match kind {
        Kind::Linear => {
            let off = y * 320 + x;
            let old = vga.raw_get(LINEAR, off);
            vga.raw_set(LINEAR, off, if xor { old ^ color } else { color });
        }
        Kind::Planar(stride) => {
            let off = y * stride + x / 8;
            let bit = 0x80u8 >> (x & 7);
            for p in 0..4 {
                let v = vga.raw_get(p, off);
                let on = color >> p & 1 != 0;
                let n = match (xor, on) {
                    (true, true) => v ^ bit,
                    (true, false) => v,
                    (false, true) => v | bit,
                    (false, false) => v & !bit,
                };
                vga.raw_set(p, off, n);
            }
        }
        Kind::Cga4 | Kind::Cga2 => {
            let (per, mask) = if kind == Kind::Cga4 { (4, 3u8) } else { (8, 1u8) };
            let at = CGA + (y & 1) * 0x2000 + (y >> 1) * 80 + x / per;
            let sh = (per - 1 - (x % per)) * (8 / per);
            let old = mem.read_u8(at);
            let c = color & mask;
            mem.write_u8(at, if xor { old ^ (c << sh) } else { (old & !(mask << sh)) | (c << sh) });
        }
    }
    vga.touch();
}

pub fn get_pixel(vga: &Vga, mem: &GuestMem, kind: Kind, x: usize, y: usize) -> u8 {
    match kind {
        Kind::Linear => vga.raw_get(LINEAR, y * 320 + x),
        Kind::Planar(stride) => {
            let off = y * stride + x / 8;
            let bit = 0x80u8 >> (x & 7);
            (0..4).fold(0, |c, p| c | u8::from(vga.raw_get(p, off) & bit != 0) << p)
        }
        Kind::Cga4 | Kind::Cga2 => {
            let (per, mask) = if kind == Kind::Cga4 { (4, 3u8) } else { (8, 1u8) };
            let at = CGA + (y & 1) * 0x2000 + (y >> 1) * 80 + x / per;
            mem.read_u8(at) >> ((per - 1 - (x % per)) * (8 / per)) & mask
        }
    }
}

/// AH=0Ch : écrit un pixel. En mode 13h, les 8 bits de AL sont la couleur ; ailleurs, le bit 7 demande un OU exclusif.
pub fn write_pixel(mem: &GuestMem, x: u16, y: u16, al: u8) {
    let mode = mem.read_u8(bda::VIDEO_MODE);
    let Some((w, h, kind)) = geometry(mode) else { return };
    let (x, y) = (usize::from(x), usize::from(y));
    if x >= w || y >= h {
        return;
    }
    let xor = al & 0x80 != 0 && mode != 0x13;
    with_vga(|v| put_pixel(v, mem, kind, x, y, if xor { al & 0x7F } else { al }, xor));
}

/// AH=0Dh : lit un pixel (0 hors de l'écran ou mode non graphique).
pub fn read_pixel(mem: &GuestMem, x: u16, y: u16) -> u8 {
    let mode = mem.read_u8(bda::VIDEO_MODE);
    let Some((w, h, kind)) = geometry(mode) else { return 0 };
    let (x, y) = (usize::from(x), usize::from(y));
    if x >= w || y >= h {
        return 0;
    }
    with_vga(|v| get_pixel(v, mem, kind, x, y)).unwrap_or(0)
}

/// Dessine le caractère `ch` dans la cellule (`row`, `col`) : `fg` pour les points allumés, 0 sinon (sauf OU exclusif).
pub fn put_char(mem: &GuestMem, ch: u8, row: usize, col: usize, fg: u8, xor: bool) {
    let mode = mem.read_u8(bda::VIDEO_MODE);
    let Some((w, h, kind)) = geometry(mode) else { return };
    let ch_h = usize::from(mem.read_u8(bda::CHAR_HEIGHT)).clamp(1, 32);
    let font = (usize::from(mem.read_u16(0x43 * 4 + 2)) << 4) + usize::from(mem.read_u16(0x43 * 4));
    let mut rows = [0u8; 32];
    mem.read(font + usize::from(ch) * ch_h, &mut rows[..ch_h]);
    with_vga(|v| {
        for (r, bits) in rows[..ch_h].iter().enumerate() {
            let y = row * ch_h + r;
            if y >= h {
                break;
            }
            for c in 0..8 {
                let x = col * 8 + c;
                if x >= w {
                    break;
                }
                if bits >> (7 - c) & 1 != 0 {
                    put_pixel(v, mem, kind, x, y, fg, xor);
                } else if !xor {
                    put_pixel(v, mem, kind, x, y, 0, false);
                }
            }
        }
    });
}

/// Défile la fenêtre (haut, gauche, bas, droite en cellules) de `n` lignes de caractères (0 : efface) ; `fill` = couleur du vide.
pub fn scroll(mem: &GuestMem, n: u8, fill: u8, win: (u8, u8, u8, u8), up: bool) {
    let mode = mem.read_u8(bda::VIDEO_MODE);
    let Some((w, h, kind)) = geometry(mode) else { return };
    let ch_h = usize::from(mem.read_u8(bda::CHAR_HEIGHT)).clamp(1, 32);
    let x0 = usize::from(win.1) * 8;
    let x1 = ((usize::from(win.3) + 1) * 8).min(w);
    let y0 = usize::from(win.0) * ch_h;
    let y1 = ((usize::from(win.2) + 1) * ch_h).min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let rows = y1 - y0;
    let n = if n == 0 { rows } else { (usize::from(n) * ch_h).min(rows) };
    with_vga(|v| {
        let moved = rows - n;
        for i in 0..moved {
            let (dst, src) = if up { (y0 + i, y0 + i + n) } else { (y1 - 1 - i, y1 - 1 - i - n) };
            for x in x0..x1 {
                let c = get_pixel(v, mem, kind, x, src);
                put_pixel(v, mem, kind, x, dst, c, false);
            }
        }
        let cleared = if up { y1 - n..y1 } else { y0..y0 + n };
        for y in cleared {
            for x in x0..x1 {
                put_pixel(v, mem, kind, x, y, fill, false);
            }
        }
    });
}
