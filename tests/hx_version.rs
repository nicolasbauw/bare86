// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : VERSION.DLL du fork de HX de Bare86 lit les ressources de version.
//!
//! `c/tests/win32/version.exe` (ressource `version.rc`) lit sa propre version : `GetFileVersionInfoSizeA`/`GetFileVersionInfoA`
//! sur le programme déjà chargé (comme `ijl11.dll`, la bibliothèque JPEG d'Intel, qui lit sa version dans sa DllMain et
//! refuse de se charger sinon), puis `VerQueryValueA` sur `\` (VS_FIXEDFILEINFO), deux chaînes rendues en ANSI (la même
//! deux fois : la conversion ne doit pas abîmer le bloc), `\VarFileInfo\Translation` et un chemin absent. Avec la VERSION.DLL
//! de HX 2.23, `VerQueryValueA` est un bouchon qui échoue toujours. Ignoré sans KVM ni HX.

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
fn version_resources_are_read() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-version-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/version.exe"), dir.join("version.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "version.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("VERSION.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        text.trim(),
        "ok=1 size=1 ms=00010002 pv=5.6.7.8/5.6.7.8 co=Bare86 tr=04B00409 absent=0",
        "lecture des ressources de version"
    );
}
