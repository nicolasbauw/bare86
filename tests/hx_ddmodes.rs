// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : DirectDraw (DDDRAW du fork de HX de Bare86) décrit les modes d'affichage comme Windows.
//!
//! `c/tests/win32/ddmodes.exe` énumère les modes : les modes 8 bits doivent avoir `DDPF_PALETTEINDEXED8`, aucun mode ne doit
//! annoncer 15 bits par pixel (les modes 5-5-5 de la carte VESA sont annoncés en 16 bits avec leurs masques). Le DDDRAW de
//! HX 2.23 annonçait 15 bits et des modes 8 bits « RGB » : OpenPTC (≈ 10 démos) s'arrêtait sur « unsupported bits per pixel ».
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
fn display_modes_are_described_as_windows_does() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-ddmodes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/ddmodes.exe"), dir.join("ddmodes.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "ddmodes.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("DDMODES.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "modes=1 pal8=1 bits15=0 rgb555=1 bad=0", "modes de DirectDraw");
}
