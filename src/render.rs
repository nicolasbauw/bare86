// SPDX-License-Identifier: GPL-3.0-or-later
//! Rendu des modes graphiques en image RVB (0x00RRGGBB) : 256 couleurs
//! (chain4 et non chaîné), planaires 16 couleurs, CGA 4 et 2 couleurs. La
//! géométrie vient du CRTC (largeur, fin d'affichage, répétition des lignes,
//! pas de ligne, adresse de départ). Le mode texte demande une police
//! (question ouverte) : il est rendu par le terminal en attendant.
//!
//! Non gérés pour l'instant : décalage horizontal (panning), écran partagé
//! (line compare), modes 200 lignes CGA hors 4, 5 et 6.

use crate::mem::GuestMem;
use crate::vga::Vga;

/// Couleur 0x00RRGGBB d'une entrée du DAC (composantes sur 6 bits).
fn dac_rgb(vga: &Vga, i: u8) -> u32 {
    let [r, g, b] = vga.dac[usize::from(i & vga.dac_mask)];
    let x = |c: u8| u32::from(c & 0x3F) * 255 / 63;
    (x(r) << 16) | (x(g) << 8) | x(b)
}

/// Index du DAC d'une couleur 4 bits, par la palette du contrôleur d'attributs.
fn attr_color(vga: &Vga, c: u8) -> u8 {
    let c = c & vga.attr[0x12] & 0x0F;
    let pal = vga.attr[usize::from(c)];
    let sel = vga.attr[0x14];
    if vga.attr[0x10] & 0x80 != 0 {
        (pal & 0x0F) | ((sel & 0x0F) << 4)
    } else {
        (pal & 0x3F) | ((sel & 0x0C) << 4)
    }
}

pub struct Geometry {
    pub width: usize,
    pub height: usize,
    /// Lignes de balayage par ligne affichée (doublement), pour le rapport d'aspect.
    #[allow(dead_code)] // fenêtre SDL : point 4 de l'étape 5
    pub repeat: usize,
    /// Octets par ligne (dans la mémoire du mode).
    pitch: usize,
    start: usize,
}

fn geometry(vga: &Vga, pixels_per_char: usize) -> Geometry {
    let c = &vga.crtc;
    let chars = usize::from(c[1]) + 1;
    let half = vga.seq[1] & 0x08 != 0;
    let width = chars * pixels_per_char / if half { 2 } else { 1 };
    let vde = usize::from(c[0x12]) | (usize::from(c[7] & 0x02) << 7) | (usize::from(c[7] & 0x40) << 3);
    let mut repeat = usize::from(c[9] & 0x1F) + 1;
    if c[9] & 0x80 != 0 {
        repeat *= 2;
    }
    let height = (vde + 1) / repeat;
    let start = (usize::from(c[0x0C]) << 8) | usize::from(c[0x0D]);
    Geometry { width, height, repeat, pitch: usize::from(c[0x13]) * 2, start }
}

/// Rend l'image du mode graphique courant dans `out` ; `None` en mode texte.
pub fn render(vga: &Vga, mem: &GuestMem, out: &mut Vec<u32>) -> Option<(usize, usize)> {
    let bios_mode = mem.read_u8(0x449);
    if matches!(bios_mode, 4..=6) {
        return Some(render_cga(vga, mem, bios_mode, out));
    }
    if vga.gc[6] & 1 == 0 {
        return None;
    }
    if vga.gc[5] & 0x40 != 0 {
        // 256 couleurs : 4 pixels par caractère d'horloge (8 points / 2).
        let g = geometry(vga, 4);
        out.clear();
        out.reserve(g.width * g.height);
        for y in 0..g.height {
            for x in 0..g.width {
                let idx = if vga.chain4() {
                    // Adressage en doubles mots : 4 octets par adresse du CRTC.
                    vga.linear_byte((g.start * 4 + y * g.pitch * 4 + x) & 0xFFFF)
                } else {
                    vga.plane_byte(x & 3, (g.start + y * g.pitch + x / 4) & 0xFFFF)
                };
                out.push(dac_rgb(vga, idx));
            }
        }
        return Some((g.width, g.height));
    }
    // Planaire 16 couleurs : 8 pixels par octet de plan.
    let g = geometry(vga, 8);
    out.clear();
    out.reserve(g.width * g.height);
    for y in 0..g.height {
        let line = g.start + y * g.pitch;
        for x in 0..g.width {
            let off = (line + x / 8) & 0xFFFF;
            let bit = 7 - (x & 7);
            let c = (0..4).fold(0u8, |acc, p| acc | (((vga.plane_byte(p, off) >> bit) & 1) << p));
            out.push(dac_rgb(vga, attr_color(vga, c)));
        }
    }
    Some((g.width, g.height))
}

/// CGA 320 × 200 × 4 (modes 4, 5) et 640 × 200 × 2 (mode 6), en RAM
/// B8000 : lignes paires à B8000, impaires à BA000.
fn render_cga(vga: &Vga, mem: &GuestMem, mode: u8, out: &mut Vec<u32>) -> (usize, usize) {
    const CGA: [u32; 16] = [
        0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555, 0x5555FF, 0x55FF55,
        0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
    ];
    let color = vga.cga_color();
    let w = if mode == 6 { 640 } else { 320 };
    out.clear();
    for y in 0..200 {
        let line = 0xB_8000 + (y & 1) * 0x2000 + (y / 2) * 80;
        for x in 0..w {
            let px = if mode == 6 {
                let b = mem.read_u8(line + x / 8);
                if b >> (7 - (x & 7)) & 1 != 0 { CGA[usize::from(color & 15)] } else { 0 }
            } else {
                let b = mem.read_u8(line + x / 4);
                let v = (b >> (6 - 2 * (x & 3))) & 3;
                if v == 0 {
                    CGA[usize::from(color & 15)]
                } else {
                    // Palette 1 (cyan, magenta, blanc) ou 0 (vert, rouge, brun), intensité bit 4.
                    let pal1 = color & 0x20 != 0 || mode == 5;
                    let base: [usize; 3] = if pal1 { [3, 5, 7] } else { [2, 4, 6] };
                    CGA[base[usize::from(v - 1)] + if color & 0x10 != 0 { 8 } else { 0 }]
                }
            };
            out.push(px);
        }
    }
    (w, 200)
}

/// Image en PNG (RVB, sans compression : blocs « stored » de deflate).
pub fn png(width: usize, height: usize, pixels: &[u32]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for row in pixels.chunks(width).take(height) {
        raw.push(0);
        for &p in row {
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(0xFFFF).peekable();
    while let Some(b) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
