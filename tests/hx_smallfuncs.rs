// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : des petites fonctions que HX n'avait pas (fork de HX de Bare86) sont importées par nom et marchent.
//!
//! `c/tests/win32/smallfuncs.exe` les IMPORTE statiquement (sans elles, HX refuse de charger le programme : « import not
//! found ») : `GetClipCursor`, `FrameRect`, `GetBkMode`, `CreatePenIndirect`, `GetBrushOrgEx`/`SetBrushOrgEx`, `GetScrollPos`,
//! `HideCaret`/`ShowCaret`, `ActivateKeyboardLayout`, `joySetThreshold`, `waveOutSetPlaybackRate`, `timeGetSystemTime`. Il écrit
//! les résultats dans SMALL.TXT et dessine un cadre rouge d'un pixel de (100,100) à (200,160). Ignoré sans KVM ni HX.

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
fn small_functions_are_imported_and_work() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-smallfuncs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/smallfuncs.exe"), dir.join("smallfuncs.exe")).unwrap();
    let shot = dir.join("shot.png");
    let out = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "9", "--keys", "{wait:900000}", "--shot"])
        .arg(&shot)
        .arg("smallfuncs.exe")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let text = std::fs::read_to_string(dir.join("SMALL.TXT")).unwrap_or_default().replace("\r\n", "\n");
    let png = std::fs::read(&shot).unwrap_or_default();
    let px = |x, y| if png.is_empty() { [9, 9, 9] } else { png_pixel(&png, x, y) };
    let (corner, top, left, right, bottom) = (px(100, 100), px(150, 100), px(100, 130), px(199, 130), px(150, 159));
    let (inside, above, outside) = (px(150, 130), px(150, 99), px(200, 130));
    let _ = std::fs::remove_dir_all(&dir);
    let expected = "clip0=0,0,800,600\nclip1=10,20,300,200\nclip2=0,0,800,600\nbkmode=2,1\npen=1\nbrushorg=0,0\nscroll=0 caret_kl=1\njoy=6 wave=8 tgst=0 type=1\nmore2=wininet:1 imagelist:1\npoly=1,1\ngdi2=bmi:1 h:64 size:512 font:1\nmore=stretch:3 hatch:1 dibpat:1 rrgn:1 ps:1 guid:1,0 cbv:1702bc icce:1\n";
    assert_eq!(text, expected, "résultats des fonctions (le programme n'a peut-être pas démarré : {log})");
    for (what, c) in [("coin", corner), ("bord haut", top), ("bord gauche", left), ("bord droit", right), ("bord bas", bottom)] {
        assert_eq!(c, [255, 0, 0], "le cadre de FrameRect devrait être rouge ({what}) : {c:?}");
    }
    assert_eq!(inside, [0, 0, 0], "l'intérieur du cadre : {inside:?}");
    assert_eq!(above, [0, 0, 0], "au-dessus du cadre : {above:?}");
    assert_eq!(outside, [0, 0, 0], "à droite du cadre : {outside:?}");
}
