// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : `StretchDIBits` de DGDI32 met une image à l'échelle (fork de HX de Bare86).
//!
//! `c/tests/win32/stretch.exe` ouvre une fenêtre plein écran sur le bureau de HX (VESA 800×600, 256 couleurs)
//! et étire une image de 4×4 pixels en 32 bits, de bas en haut, aux coins colorés (haut-gauche rouge, haut-droite
//! vert, bas-gauche bleu, bas-droite blanc) : en 160×160 en (100, 100), puis son carré 2×2 inférieur gauche en
//! 160×160 en (400, 100). Le test vérifie l'échelle (blocs de 40 pixels), l'orientation, le sous-rectangle et les
//! couleurs : sur le bureau de 256 couleurs, le rouge, le vert, le bleu et le blanc purs doivent sortir purs (avec le
//! DGDI32 et le VESA32 de HX 2.23, la palette système était fausse et la recherche de la couleur la plus proche
//! cherchait dans 20 entrées : le rouge sortait presque noir). Ignoré (sans échec) sans KVM ni HX.

use std::path::{Path, PathBuf};
use std::process::Command;

fn hx_installed() -> bool {
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d).join("bare86"),
        None => match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(".local/share/bare86"),
            None => return false,
        },
    };
    base.join("HX/BIN/DPMILD32.EXE").is_file()
}

/// Pixel (x, y) d'un PNG RGB 8 bits non entrelacé (voir `hx_desktop.rs`).
fn png_pixel(data: &[u8], x: usize, y: usize) -> [u8; 3] {
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "pas un PNG");
    let (mut at, mut idat, mut ihdr) = (8, Vec::new(), (0usize, 0usize));
    while at + 8 <= data.len() {
        let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let kind = &data[at + 4..at + 8];
        let body = &data[at + 8..at + 8 + len];
        match kind {
            b"IHDR" => {
                ihdr = (u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize, u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize);
                assert_eq!((body[8], body[9], body[12]), (8, 2, 0), "PNG RGB 8 bits non entrelacé attendu");
            }
            b"IDAT" => idat.extend_from_slice(body),
            _ => {}
        }
        at += 12 + len;
    }
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&idat[..]), &mut raw).expect("zlib");
    let (w, h) = ihdr;
    let stride = w * 3;
    let mut rows: Vec<Vec<u8>> = Vec::with_capacity(h);
    for r in 0..h {
        let line = &raw[r * (stride + 1)..(r + 1) * (stride + 1)];
        let mut cur = line[1..].to_vec();
        for i in 0..stride {
            let a = if i >= 3 { cur[i - 3] as i32 } else { 0 };
            let b = if r > 0 { rows[r - 1][i] as i32 } else { 0 };
            let c = if r > 0 && i >= 3 { rows[r - 1][i - 3] as i32 } else { 0 };
            let add = match line[0] {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                _ => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                }
            };
            cur[i] = (cur[i] as i32 + add) as u8;
        }
        rows.push(cur);
    }
    [rows[y][x * 3], rows[y][x * 3 + 1], rows[y][x * 3 + 2]]
}

#[test]
fn stretch_dibits_scales_and_keeps_orientation() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-stretch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/stretch.exe"), dir.join("stretch.exe")).unwrap();
    let shot = dir.join("shot.png");
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "9", "--keys", "{wait:900000}", "--shot"])
        .arg(&shot)
        .arg("stretch.exe")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let png = std::fs::read(&shot).expect("capture absente");
    let px = |x, y| png_pixel(&png, x, y);
    let (tl, tr, bl, br) = (px(120, 120), px(240, 120), px(120, 240), px(240, 240));
    let (mid, outside) = (px(180, 180), px(300, 300));
    let (sub_bl, sub_top, sub_right) = (px(440, 220), px(440, 120), px(520, 220));
    let _ = std::fs::remove_dir_all(&dir);
    let near = |c: [u8; 3], want: [u8; 3]| c.iter().zip(want).all(|(&a, b)| (i32::from(a) - i32::from(b)).abs() <= 8);
    assert!(near(tl, [255, 0, 0]), "haut-gauche devrait être rouge : {tl:?}");
    assert!(near(tr, [0, 255, 0]), "haut-droite devrait être vert : {tr:?}");
    assert!(near(bl, [0, 0, 255]), "bas-gauche devrait être bleu : {bl:?}");
    assert!(near(br, [255, 255, 255]), "bas-droite devrait être blanc : {br:?}");
    assert_eq!(mid, [0, 0, 0], "le milieu (pixels noirs de l'image) : {mid:?}");
    assert_eq!(outside, [0, 0, 0], "hors du rectangle : {outside:?}");
    assert!(near(sub_bl, [0, 0, 255]), "le carré 2×2 inférieur gauche étiré devrait avoir son coin bas-gauche bleu : {sub_bl:?}");
    assert_eq!(sub_top, [0, 0, 0], "partie haute du sous-rectangle : {sub_top:?}");
    assert_eq!(sub_right, [0, 0, 0], "partie droite du sous-rectangle : {sub_right:?}");
}
