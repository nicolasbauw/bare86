// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : `MulDiv` de DKRNL32 (fork de HX de Bare86) se comporte comme celui de Windows.
//!
//! `c/tests/win32/muldiv.exe` : arrondi au plus proche, nombres négatifs, diviseur nul et débordements (-1, sans exception),
//! produit sur 64 bits. Le `MulDiv` de HX 2.23 (mul/div non signés) levait une erreur de division et se trompait de signe.
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
fn muldiv_rounds_and_reports_overflow() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-muldiv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/muldiv.exe"), dir.join("muldiv.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "muldiv.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("MULDIV.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "3 -3 -1 -1 -11 600000 -33 -1 -2147483648", "résultats de MulDiv");
}
