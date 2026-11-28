// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : la `CRTDLL.DLL` que Bare86 fabrique à partir de `MSVCRT.DLL` (`src/crtdll.rs`) sous HX.
//!
//! `c/tests/win32/crtdll.exe` la charge par `LoadLibrary` : `__GetMainArgs` rend les arguments, `_fmode_dll` est la
//! variable `_fmode`, `_osmajor_dll` vaut 4, `_strncnt`/`_strinc`/`_strnextc` répondent, et un fichier binaire (0x1A,
//! CR LF) écrit puis relu par ses `fopen`/`fwrite`/`fseek`/`getc`/`fread` revient intact, et `_open`/`_fstat`/`_read` en donnent la taille et le contenu. La CRTDLL d'avant (simple
//! copie de MSVCRT) n'avait aucun de ces noms : `load=1 gma=0 alias=0 osmajor=-1 str=0 bin=1`. Ignoré sans KVM ni HX.

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
fn crtdll_has_its_own_names_and_reads_binary_files() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-crtdll-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/crtdll.exe"), dir.join("crtdll.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "crtdll.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("CRTDLL.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "load=1 gma=1 alias=1 osmajor=4 str=2211 bin=1 low=88", "résultats de CRTDLL");
}
