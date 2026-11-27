// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : une boîte de message qui attend n'occupe pas un cœur de l'hôte.
//!
//! La boucle d'attente de HX (boîte de message, écran d'erreur d'une démo) appelle sans cesse `INT 2Fh AX=1680h` (libérer la
//! tranche de temps) en alternance avec `INT 21h AX=4400h` ; Bare86 y répondait aussitôt et le processus consommait 100 % d'un
//! cœur (une démo sur trois du balayage Win32 attendait ainsi, des minutes). `c/tests/win32/msgbox.exe` ouvre une boîte et
//! attend ; on mesure le temps CPU du processus Bare86 sur 8 secondes : sous 0,35 cœur (il était à 1,0 ; il est à ≈ 0,1).
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

/// Temps CPU (utilisateur + système) des fils terminés de ce processus, en secondes (`/proc/self/stat`, champs 16 et 17).
fn children_cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after = &stat[stat.rfind(')').unwrap() + 2..]; // après « pid (comm) » : le champ 3 est le premier
    let f: Vec<&str> = after.split_whitespace().collect();
    (f[13].parse::<f64>().unwrap() + f[14].parse::<f64>().unwrap()) / 100.0 // cutime, cstime (champs 16, 17), en 1/100 s
}

#[test]
fn waiting_message_box_does_not_burn_a_core() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-idle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/msgbox.exe"), dir.join("msgbox.exe")).unwrap();
    let before = children_cpu_seconds();
    let started = std::time::Instant::now();
    // L'entrée standard reste OUVERTE (un tube que personne n'écrit) : fermée ou /dev/null, la fin de fichier validerait la boîte.
    let mut child = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "8", "msgbox.exe"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("bare86");
    let _stdin = child.stdin.take();
    let _ = child.wait();
    let wall = started.elapsed().as_secs_f64();
    let cpu = children_cpu_seconds() - before;
    // Le texte de la boîte est aussi écrit sur la sortie d'erreur de HX (hxstderr.log) : diagnostic des démos qui finissent dans une boîte.
    let log = std::fs::read_to_string(dir.join("hxstderr.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(log.contains("[MessageBox] msgbox: waiting"), "hxstderr.log : {log:?}");
    assert!(wall > 6.0, "le programme s'est arrêté trop tôt ({wall:.1} s) : la boîte n'a pas attendu");
    assert!(cpu / wall < 0.35, "une boîte de message en attente occupe {:.2} cœur ({cpu:.1} s de CPU en {wall:.1} s)", cpu / wall);
}
