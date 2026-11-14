// SPDX-License-Identifier: GPL-3.0-or-later
//! Fichier de configuration : une option par ligne, `option = valeur`, comme
//! sur la ligne de commande (`--option valeur`) ; `option = true` donne
//! l'option seule, `false` l'omet. Commentaires après `#` ou `;`.
//! `program = …` et `args = …` désignent le programme à lancer quand la
//! ligne de commande n'en donne pas. Les chemins relatifs (`mount`,
//! `program`, `font`, `shot`, `wav`) partent du répertoire du fichier.
//!
//! Recherche : `--config FICHIER`, sinon `./bare86.conf`, sinon
//! `~/.config/bare86/bare86.conf` ; un seul fichier est lu.

use std::path::{Path, PathBuf};

#[derive(Default, Debug, PartialEq)]
pub struct Config {
    /// Options, sous la forme de la ligne de commande.
    pub options: Vec<String>,
    pub program: Option<String>,
    pub args: Option<String>,
}

const PATH_KEYS: &[&str] = &["mount", "program", "font", "shot", "wav", "fda", "fdb", "hda", "cdrom"];

/// Fichier désigné ou trouvé, lu et analysé (vide s'il n'y en a pas).
pub fn load(cli: &[String]) -> Result<Config, String> {
    let explicit = cli.iter().position(|a| a == "--config").map(|i| cli.get(i + 1).cloned().ok_or("--config : fichier attendu"));
    let path = match explicit {
        Some(p) => Some(PathBuf::from(p?)),
        None => {
            let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/bare86/bare86.conf"));
            [Some(PathBuf::from("bare86.conf")), home].into_iter().flatten().find(|p| p.is_file())
        }
    };
    let Some(path) = path else { return Ok(Config::default()) };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    parse(&text, &dir).map_err(|e| format!("{} : {e}", path.display()))
}

pub fn parse(text: &str, dir: &Path) -> Result<Config, String> {
    let mut c = Config::default();
    for (n, line) in text.lines().enumerate() {
        let line = line.split(['#', ';']).next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(format!("ligne {} : « option = valeur » attendu", n + 1))?;
        let (key, value) = (key.trim().trim_start_matches("--").to_ascii_lowercase(), value.trim());
        let value = if PATH_KEYS.contains(&key.as_str()) && !value.is_empty() {
            let p = Path::new(value);
            if p.is_relative() { dir.join(p).to_string_lossy().into_owned() } else { value.to_string() }
        } else {
            value.to_string()
        };
        match key.as_str() {
            "program" => c.program = Some(value),
            "args" => c.args = Some(value),
            _ => match value.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" => c.options.push(format!("--{key}")),
                "false" | "no" | "off" => {}
                _ => c.options.extend([format!("--{key}"), value]),
            },
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_paths_and_program() {
        let text = "# Doom\nmips = 50\nfullscreen = true\nvbe1 = false\nmount = c ; lecteur C:\nprogram = c/DOOM.EXE\nargs = -nosound\n";
        let c = parse(text, Path::new("/jeux/doom")).unwrap();
        assert_eq!(c.options, ["--mips", "50", "--fullscreen", "--mount", "/jeux/doom/c"]);
        assert_eq!(c.program.as_deref(), Some("/jeux/doom/c/DOOM.EXE"));
        assert_eq!(c.args.as_deref(), Some("-nosound"));
        assert!(parse("mips 50", Path::new(".")).is_err());
    }
}
