// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : le bureau graphique de HX DOS Extender se dessine.
//!
//! `GDITEST.EXE` (fourni avec HX, dans `HX/TEST`) ouvre une fenêtre grise de 512×288 à partir du point (64, 64)
//! sur le bureau de HX (VESA 800×600, 256 couleurs). Le test lance Bare86 sur ce programme, prend une capture et
//! vérifie qu'un pixel de l'intérieur de la fenêtre est gris clair. Il est ignoré (sans échec) si KVM ou HX ne sont pas
//! installés : HX est téléchargé par Bare86 au premier programme Windows, pas par le test.
//!
//! Il aurait attrapé la régression du 2026-11-22 (interface VESA protégée : début d'affichage, puis largeur du DAC) :
//! le bureau restait noir et plus aucune démo Windows ne s'affichait, sans message d'erreur.

use std::path::{Path, PathBuf};
use std::process::Command;

fn hx_gditest() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d).join("bare86"),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/share/bare86"),
    };
    let exe = base.join("HX/TEST/GDITEST.EXE");
    exe.is_file().then_some(exe)
}

/// Pixel (x, y) d'un PNG RGB 8 bits non entrelacé : décompression (flate2) et défiltrage des cinq filtres de PNG.
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
fn hx_gui_desktop_draws_its_window() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    let Some(gditest) = hx_gditest() else {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("bare86-hx-desktop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&gditest, dir.join("GDITEST.EXE")).unwrap();
    let shot = dir.join("shot.png");
    let status = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "10", "--keys", "{wait:3000}{mouse:40,30,0}{wait:5000}", "--shot"])
        .arg(&shot)
        .arg("GDITEST.EXE")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let _ = status;
    let png = std::fs::read(&shot).expect("capture absente");
    let inside = png_pixel(&png, 100, 100); // dans la fenêtre
    let outside = png_pixel(&png, 700, 500); // bureau
    let _ = std::fs::remove_dir_all(&dir);
    assert!(inside[0] > 150 && inside[0] == inside[1] && inside[1] == inside[2], "fenêtre non dessinée ou mauvaise couleur : {inside:?}");
    assert_eq!(outside, [0, 0, 0], "le bureau devrait être noir : {outside:?}");
}
