// SPDX-License-Identifier: GPL-3.0-or-later
//! Lanceur (`--launcher [RÉPERTOIRE]`), seulement sur demande : `bare86`
//! sans argument reste un lancement direct. Un profil est le `bare86.conf`
//! du répertoire d'un jeu (format de `config.rs`, chemins relatifs à son
//! répertoire) ; le lanceur liste les sous-répertoires du répertoire des
//! jeux qui en ont un, et lance celui qu'on choisit.

use std::path::{Path, PathBuf};

pub const PROFILE: &str = "bare86.conf";

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Nom affiché : celui du répertoire du jeu.
    pub name: String,
    pub conf: PathBuf,
    /// Programme lancé (`program = …`), CD (`cdrom = …`), vrai DOS (`boot`).
    pub program: Option<String>,
    pub cdrom: Option<String>,
    pub boot: bool,
}

/// Lit ce que le lanceur montre d'un profil (sans valider le reste).
fn describe(name: String, conf: PathBuf) -> Profile {
    let text = std::fs::read_to_string(&conf).unwrap_or_default();
    let mut p = Profile { name, conf, program: None, cdrom: None, boot: false };
    for line in text.lines() {
        let line = line.split(['#', ';']).next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim().trim_start_matches("--").to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "program" => p.program = Some(v.to_string()),
            "cdrom" => p.cdrom = Some(v.to_string()),
            "boot" => p.boot = matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on"),
            _ => {}
        }
    }
    p
}

/// Profils du répertoire des jeux : chaque sous-répertoire qui a un
/// `bare86.conf`, triés par nom (sans tenir compte de la casse).
pub fn scan(root: &Path) -> Vec<Profile> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<Profile> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(PROFILE).is_file())
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            (!name.starts_with('.')).then(|| describe(name, p.join(PROFILE)))
        })
        .collect();
    v.sort_by_key(|p| p.name.to_lowercase());
    v
}

/// Ligne du lanceur : nom, puis programme ou mode de démarrage, et le CD.
pub fn line(p: &Profile) -> String {
    let what = match (&p.program, p.boot) {
        (_, true) => "vrai DOS".to_string(),
        (Some(prog), _) => Path::new(prog).file_name().map_or(prog.clone(), |n| n.to_string_lossy().into_owned()),
        (None, false) => "shell".to_string(),
    };
    let cd = p.cdrom.as_ref().map(|c| format!("  [CD {}]", Path::new(c).file_name().map_or(c.clone(), |n| n.to_string_lossy().into_owned()))).unwrap_or_default();
    format!("{:<24} {what}{cd}", p.name)
}

/// Ligne de commande d'un profil choisi : `--launcher` et son répertoire
/// retirés, `--config PROFIL --window` en tête, le reste gardé.
pub fn command_line(cli: &[String], conf: &Path) -> Vec<String> {
    let mut out = vec!["--config".to_string(), conf.to_string_lossy().into_owned(), "--window".to_string()];
    let mut it = cli.iter().peekable();
    while let Some(a) = it.next() {
        if a == "--launcher" {
            if it.peek().is_some_and(|n| !n.starts_with("--")) {
                it.next();
            }
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// Répertoire des jeux donné après `--launcher` (le courant sinon).
pub fn root_of(cli: &[String]) -> Option<PathBuf> {
    let i = cli.iter().position(|a| a == "--launcher")?;
    Some(match cli.get(i + 1).filter(|n| !n.starts_with("--")) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from("."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_profiles_and_command_line() {
        let root = std::env::temp_dir().join(format!("bare86-launcher-{}", std::process::id()));
        for d in ["doom", "Dune", "vide", ".cache"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("doom").join(PROFILE), "mips = 100\nprogram = c/DOOM.EXE # jeu\n").unwrap();
        std::fs::write(root.join("Dune").join(PROFILE), "cdrom = DUNE.CUE\nboot = true\n").unwrap();
        std::fs::write(root.join(".cache").join(PROFILE), "").unwrap();
        let p = scan(&root);
        let names: Vec<&str> = p.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["doom", "Dune"]);
        assert_eq!(p[0].program.as_deref(), Some("c/DOOM.EXE"));
        assert!(line(&p[0]).ends_with("DOOM.EXE"));
        assert!(p[1].boot && line(&p[1]).contains("vrai DOS  [CD DUNE.CUE]"));
        std::fs::remove_dir_all(&root).unwrap();

        let cli: Vec<String> = ["--launcher", "/jeux", "--mips", "50"].iter().map(|s| s.to_string()).collect();
        assert_eq!(root_of(&cli), Some(PathBuf::from("/jeux")));
        assert_eq!(command_line(&cli, Path::new("/jeux/doom/bare86.conf")), ["--config", "/jeux/doom/bare86.conf", "--window", "--mips", "50"]);
        let bare: Vec<String> = vec!["--launcher".into()];
        assert_eq!(root_of(&bare), Some(PathBuf::from(".")));
        assert_eq!(root_of(&[]), None);
    }
}
