// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : `TextOutA` de DGDI32 (fork de HX de Bare86) dans un bitmap de bas en haut et hors du bitmap.
//!
//! `c/tests/win32/textout.exe` écrit du texte en haut d'une DIB section 8 bits de bas en haut : il doit se trouver dans les
//! lignes du haut de l'image (la fin des bits ; le DGDI32 de HX 2.23 l'écrivait en bas, à l'envers), puis du texte en partie
//! ou entièrement hors du bitmap : rien ne doit être écrit dehors ni planter (la démo `flush` plantait dans `TextOutA`,
//! accès hors du bitmap). Ignoré sans KVM ni HX.

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

#[test]
fn text_lands_inside_the_bitmap() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-textout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/textout.exe"), dir.join("textout.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "textout.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("TEXTOUT.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "top=1 bottom=0 outside=1", "texte de TextOutA");
}
