// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : classes de contrôles standard et modes d'affichage de DUSER32 (fork de HX de Bare86).
//!
//! `c/tests/win32/stdctl.exe` crée dans une fenêtre ordinaire des contrôles `BUTTON` et `COMBOBOX` (HX 2.23 n'a aucune
//! classe standard : `CreateWindowEx` échouait, et les démos qui construisent elles-mêmes leur fenêtre de réglages
//! attendaient un clic). Le bouton `BS_DEFPUSHBUTTON` doit « cliquer » de lui-même (une fois, venant de sa fenêtre), l'autre
//! non ; la combo box garde ses chaînes et sa sélection. `EnumDisplaySettings` doit donner 60 Hz pour chaque mode et aucun
//! mode à 15 bits (la démo `232` cherche « 800x600:32@60 »). Ignoré sans KVM ni HX.

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
fn standard_controls_and_display_settings() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-stdctl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/stdctl.exe"), dir.join("stdctl.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "stdctl.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("STDCTL.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "created=1 def=1/1 other=0 combo=2/1 modes=1 hz60=1 bpp15=0", "contrôles standard et modes");
}
