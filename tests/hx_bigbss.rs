// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : un programme Windows qui a besoin de plus de 64 Mo (32 Mo de BSS + 40 Mo de tas) démarre sans `--mem`.
//!
//! Sans `--mem`, Bare86 donne 256 Mo aux programmes Windows (sous HX) : à 64 Mo, `madeby.exe` (32 Mo de BSS puis des JPEG
//! décodés) finissait en triple faute. `c/tests/win32/bigbss.exe` écrit BIG.TXT « big ok » s'il a pu toucher toute la mémoire.
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

#[test]
fn win32_program_gets_enough_memory() {
    if !Path::new("/dev/kvm").exists() || !hx_installed() {
        eprintln!("ignoré : pas de KVM ou HX non installé");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-bigbss-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/bigbss.exe"), dir.join("bigbss.exe")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "10", "bigbss.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let text = std::fs::read_to_string(dir.join("BIG.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.starts_with("big ok"), "BIG.TXT = {text:?} ({log})");
}
