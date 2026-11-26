// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : un programme qui coupe à 16 bits le contexte d'affichage de `WM_ERASEBKGND` ne plante pas (DUSER32
//! du fork de HX de Bare86).
//!
//! `c/tests/win32/erase16.exe` ouvre une fenêtre à fond rouge dont la procédure passe `wParam & 0xFFFF` à `DefWindowProc` pour
//! `WM_ERASEBKGND` (comme la démo « haetten » : sous Windows 9x un handle tient sur 16 bits ; ici c'est une adresse de 32 bits
//! et `PatBlt` lisait l'objet à l'adresse 0x30). `DefWindowProc` retrouve le contexte complet parmi ceux que `GetDC` a donnés.
//! Ignoré sans KVM ni HX.

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
fn erase_background_survives_a_handle_cut_to_16_bits() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-erase16-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/erase16.exe"), dir.join("erase16.exe")).unwrap();
    let shot = dir.join("shot.png");
    let out = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "9", "--keys", "{wait:900000}", "--shot"])
        .arg(&shot)
        .arg("erase16.exe")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let png = std::fs::read(&shot).expect("capture absente (le programme n'a pas démarré ?)");
    let inside = png_pixel(&png, 200, 150);
    let outside = png_pixel(&png, 600, 450);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!text.contains("exception"), "le programme a planté : {text}");
    assert_eq!(inside, [255, 0, 0], "le fond rouge de la fenêtre devrait être peint : {inside:?}");
    assert_eq!(outside, [0, 0, 0], "hors de la fenêtre : {outside:?}");
}
