// SPDX-License-Identifier: GPL-3.0-or-later
//! Test d'intégration : les boîtes de dialogue modales de DUSER32 (fork de HX de Bare86) s'exécutent.
//!
//! `c/tests/win32/dialog.exe` ouvre cinq boîtes avec `DialogBoxIndirectParam` : fermée par IDOK (42), seulement par
//! IDCANCEL (7), seulement par WM_CLOSE (9), jamais fermée (le résultat doit être 1 et le programme ne doit pas rester
//! bloqué), et une boîte qui en ouvre une autre à l'initialisation, et une boîte qui envoie `WM_SETICON` à sa propre fenêtre
//! et lit son rectangle (comme beaucoup de procédures : la boîte doit être une vraie fenêtre, sinon HX saute à l'adresse 0x13).
//! Puis une boîte dont le modèle a un bouton par défaut (1008, comme « Start » dans la boîte de réglages d'une démo) : il doit
//! arriver avant IDOK, et les cases cochées à l'initialisation doivent se relire ; enfin une boîte non modale
//! (`CreateDialogIndirectParam`) dont le bouton par défaut arrive par la file de messages. Les éléments du modèle doivent être
//! des fenêtres (`GetDlgItem`, texte, combo box : chaînes, sélection, données : ce que lisent les fonctions DDX de MFC).
//! Enfin un crochet `WH_CBT` doit être appelé par `CreateWindowEx` (`HCBT_CREATEWND`) et pouvoir sous-classer la fenêtre
//! avant `WM_CREATE` (MFC : sinon `CWnd::CreateEx` échoue), et ne plus l'être une fois décroché. Puis une boîte sans bouton par
//! défaut, active, qui ignore IDOK : son premier bouton-poussoir (1003, « Begin Wire » de `wireworld`) doit arriver avant
//! IDCANCEL ; la fenêtre montrée ensuite doit recevoir `WM_ACTIVATEAPP` (la boîte est cachée par `EndDialog`), et
//! `LoadIcon(NULL, IDI_APPLICATION)` comme `SetClassLong(GCL_HICON)` doivent rendre une icône non nulle (MFC : `CWnd::SetIcon`).
//! Il écrit les résultats dans DIALOG.TXT.
//! Avec le DUSER32 de HX 2.23, la fonction n'affiche qu'une boîte « called » et attend. Ignoré sans KVM ni HX.

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
fn modal_dialogs_run_to_the_end() {
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipped: no /dev/kvm");
        return;
    }
    if !hx_installed() {
        eprintln!("skipped: HX is not installed (run any Windows program with Bare86 once)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bare86-hx-dialog-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("c/tests/win32/dialog.exe"), dir.join("dialog.exe")).unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_bare86"))
        .current_dir(&dir)
        .args(["--no-realtime", "--limit", "20", "dialog.exe"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("bare86");
    let text = std::fs::read_to_string(dir.join("DIALOG.TXT")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.trim(), "ok=42 flags=3 cancel=7 close=9 never=1 outer=5 inner=42 msg=3/1 def=77 checks=011 modeless=1 ctl=1/Q/2/1/800x600/55 cbt=1/11/1/1 first=33 icon=1 actapp=1", "résultats des boîtes de dialogue");
}
