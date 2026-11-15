// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire de texture d'une TMU, rangée comme sur la Voodoo : les niveaux
//! d'une texture se suivent du plus grand au plus petit, chacun occupant une
//! taille arrondie (au moins 4 texels). Le programme gère lui-même ses
//! adresses (`grTexDownloadMipMap`, `grTexSource`) : il faut donc les mêmes
//! décalages que la Glide de 3dfx. Décodage des formats en RGBA8.

/// Taille de la mémoire de texture annoncée (4 Mo).
pub const TMU_RAM: usize = 4 << 20;

/// Texels occupés en mémoire par chaque LOD (0 = 256), selon le rapport
/// ramené à 8:1, 4:1, 2:1, 1:1 (index 0 à 3).
const MEM_SIZE: [[u32; 9]; 4] = [
    [0x2000, 0x800, 0x200, 0x80, 0x20, 0x10, 0x8, 0x4, 0x4],
    [0x4000, 0x1000, 0x400, 0x100, 0x40, 0x10, 0x8, 0x4, 0x4],
    [0x8000, 0x2000, 0x800, 0x200, 0x80, 0x20, 0x8, 0x4, 0x4],
    [0x10000, 0x4000, 0x1000, 0x400, 0x100, 0x40, 0x10, 0x4, 0x4],
];

pub const MIPMAP_EVEN: u32 = 1;
pub const MIPMAP_ODD: u32 = 2;
pub const MIPMAP_BOTH: u32 = 3;

/// Formats de 16 bits par texel à partir de celui-ci.
const FMT_16BIT: u32 = 8;

/// Index du rapport largeur:hauteur, symétrique (8:1 et 1:8 → 0).
fn aspect_index(aspect: u32) -> usize {
    let a = aspect.min(6);
    (if a > 3 { 6 - a } else { a }) as usize
}

pub fn bytes_per_texel(format: u32) -> usize {
    if format >= FMT_16BIT { 2 } else { 1 }
}

/// Dimensions d'un LOD (0 = 256 sur le grand côté) selon le rapport (0 = 8x1,
/// 3 = 1x1, 6 = 1x8).
pub fn level_size(lod: u32, aspect: u32) -> (usize, usize) {
    let big = 256usize >> lod.min(8);
    let a = aspect.min(6);
    if a <= 3 { (big, (big >> (3 - a)).max(1)) } else { ((big >> (a - 3)).max(1), big) }
}

/// Décalage (en texels) du LOD `lod` depuis le début de la chaîne (LOD 0),
/// tous niveaux confondus, ou un sur deux (TMU paire ou impaire).
fn offset(aspect: usize, lod: u32, split: bool) -> u32 {
    if split {
        (0..lod).filter(|l| (lod - l).is_multiple_of(2)).map(|l| MEM_SIZE[aspect][l as usize]).sum()
    } else {
        (0..lod).map(|l| MEM_SIZE[aspect][l as usize]).sum()
    }
}

/// Adresse en mémoire de texture du LOD `lod` d'une texture commençant à
/// `start` (son plus grand niveau `large`) ; `None` si ce niveau n'est pas
/// sur cette TMU (pair/impair).
pub fn level_address(start: u32, large: u32, lod: u32, aspect: u32, format: u32, even_odd: u32) -> Option<u32> {
    let a = aspect_index(aspect);
    let bpp = bytes_per_texel(format) as u32;
    if even_odd == MIPMAP_BOTH {
        return Some(start + (offset(a, lod, false) - offset(a, large, false)) * bpp);
    }
    // Les niveaux pairs (0, 2…) ou impairs d'une chaîne partagée entre deux TMU.
    let wanted_odd = even_odd == MIPMAP_ODD;
    if (lod & 1 == 1) != wanted_odd {
        return None;
    }
    let first = if (large & 1 == 1) == wanted_odd { large } else { large + 1 };
    Some(start + (offset(a, lod, true) - offset(a, first, true)) * bpp)
}

/// Mémoire occupée (octets, arrondie à 8) : `grTexTextureMemRequired`.
pub fn mem_required(small: u32, large: u32, aspect: u32, format: u32, even_odd: u32) -> u32 {
    let a = aspect_index(aspect);
    let texels: u32 = (large..=small.min(8))
        .filter(|l| match even_odd {
            MIPMAP_EVEN => l & 1 == 0,
            MIPMAP_ODD => l & 1 == 1,
            _ => true,
        })
        .map(|l| MEM_SIZE[a][l as usize])
        .sum();
    (texels * bytes_per_texel(format) as u32 + 7) & !7
}

/// Texel en RGBA8 (r | g<<8 | b<<16 | a<<24) selon le format.
fn texel(format: u32, v: u16, palette: &[u32; 256]) -> u32 {
    let rgba = |r: u32, g: u32, b: u32, a: u32| r | g << 8 | b << 16 | a << 24;
    let ex = |x: u32, bits: u32| x * 255 / ((1 << bits) - 1);
    let pal = |i: u16| {
        let p = palette[usize::from(i & 0xFF)];
        rgba(p >> 16 & 0xFF, p >> 8 & 0xFF, p & 0xFF, 0xFF)
    };
    let v32 = u32::from(v);
    match format {
        // RGB 332
        0 => rgba(ex(v32 >> 5 & 7, 3), ex(v32 >> 2 & 7, 3), ex(v32 & 3, 2), 0xFF),
        // alpha 8 : blanc, transparence
        2 => rgba(0xFF, 0xFF, 0xFF, v32 & 0xFF),
        // intensité 8
        3 => rgba(v32 & 0xFF, v32 & 0xFF, v32 & 0xFF, 0xFF),
        // alpha + intensité 4/4
        4 => {
            let i = ex(v32 & 15, 4);
            rgba(i, i, i, ex(v32 >> 4 & 15, 4))
        }
        // palette 8 bits
        5 => pal(v),
        // ARGB 8332
        8 => rgba(ex(v32 >> 5 & 7, 3), ex(v32 >> 2 & 7, 3), ex(v32 & 3, 2), v32 >> 8),
        // RGB 565
        10 => rgba(ex(v32 >> 11, 5), ex(v32 >> 5 & 63, 6), ex(v32 & 31, 5), 0xFF),
        // ARGB 1555
        11 => rgba(ex(v32 >> 10 & 31, 5), ex(v32 >> 5 & 31, 5), ex(v32 & 31, 5), if v32 & 0x8000 != 0 { 0xFF } else { 0 }),
        // ARGB 4444
        12 => rgba(ex(v32 >> 8 & 15, 4), ex(v32 >> 4 & 15, 4), ex(v32 & 15, 4), ex(v32 >> 12, 4)),
        // alpha + intensité 8/8
        13 => rgba(v32 & 0xFF, v32 & 0xFF, v32 & 0xFF, v32 >> 8),
        // alpha + palette 8/8
        14 => (pal(v) & 0x00FF_FFFF) | (v32 >> 8) << 24,
        // YIQ (NCC) et réservés : gris neutre, rarement utilisés.
        _ => rgba(0x80, 0x80, 0x80, 0xFF),
    }
}

/// Un niveau décodé en RGBA8, lu en mémoire de texture à `addr`.
pub fn decode_level(mem: &[u8], addr: u32, w: usize, h: usize, format: u32, palette: &[u32; 256]) -> Vec<u32> {
    let bpp = bytes_per_texel(format);
    let at = addr as usize;
    (0..w * h)
        .map(|i| {
            let o = at + i * bpp;
            let v = if bpp == 2 {
                u16::from_le_bytes([*mem.get(o).unwrap_or(&0), *mem.get(o + 1).unwrap_or(&0)])
            } else {
                u16::from(*mem.get(o).unwrap_or(&0))
            };
            texel(format, v, palette)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_glide() {
        // grTexCalcMemRequired(GR_LOD_1, GR_LOD_256, 1x1, 565) : 2 × 87 396 texels.
        assert_eq!(mem_required(8, 0, 3, 10, MIPMAP_BOTH), (0x10000 + 0x4000 + 0x1000 + 0x400 + 0x100 + 0x40 + 0x10 + 4 + 4) * 2);
        // 64×64 en 8 bits : 4096 + 1024 + … arrondi à 8.
        assert_eq!(mem_required(8, 2, 3, 3, MIPMAP_BOTH), 0x1000 + 0x400 + 0x100 + 0x40 + 0x10 + 4 + 4);
        // Le niveau 32×32 d'une texture 64×64 en 16 bits : 4096 texels plus loin.
        assert_eq!(level_address(0x1000, 2, 3, 3, 10, MIPMAP_BOTH), Some(0x1000 + 0x2000));
        // Rapports symétriques : 2x1 et 1x2.
        assert_eq!(level_size(0, 2), (256, 128));
        assert_eq!(level_size(0, 4), (128, 256));
        assert_eq!(level_size(8, 0), (1, 1));
        // Pair/impair : seuls les niveaux pairs sur la TMU paire.
        assert_eq!(level_address(0, 0, 1, 3, 3, MIPMAP_EVEN), None);
        assert_eq!(level_address(0, 0, 2, 3, 3, MIPMAP_EVEN), Some(0x10000));
    }

    #[test]
    fn formats() {
        let pal = [0x00FF_8000u32; 256];
        assert_eq!(texel(10, 0xF800, &pal), 0xFF00_00FF);
        assert_eq!(texel(11, 0x801F, &pal), 0xFFFF_0000);
        assert_eq!(texel(12, 0x0F00, &pal), 0x0000_00FF);
        assert_eq!(texel(5, 3, &pal), 0xFF00_80FF);
        assert_eq!(texel(13, 0x8040, &pal), 0x8040_4040);
    }
}
