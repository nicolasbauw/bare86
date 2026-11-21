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

const PATH_KEYS: &[&str] = &["mount", "program", "font", "shot", "wav", "fda", "fdb", "hda", "cdrom", "midi-out"];

/// Fichier désigné ou trouvé, lu et analysé (vide s'il n'y en a pas).
pub fn load(cli: &[String]) -> Result<Config, String> {
    let explicit = cli.iter().position(|a| a == "--config").map(|i| cli.get(i + 1).cloned().ok_or("--config: file expected"));
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
        let (key, value) = line.split_once('=').ok_or(format!("line {}: \"option = value\" expected", n + 1))?;
        let (key, value) = (key.trim().trim_start_matches("--").to_ascii_lowercase(), value.trim());
        let value = if PATH_KEYS.contains(&key.as_str()) && !value.is_empty() {
            let p = Path::new(value);
            if p.is_relative() { dir.join(p).to_string_lossy().into_owned() } else { value.to_string() }
        } else {
            value.to_string()
        };
        match key.as_str() {
            "program" => c.program = Some(value),
            // Le traducteur est actif par défaut : « jit = off » le coupe.
            "jit" if matches!(value.to_ascii_lowercase().as_str(), "false" | "no" | "off") => c.options.push("--no-jit".into()),
            // Priorité temps réel d'office aussi : « realtime = off » la coupe.
            "realtime" if matches!(value.to_ascii_lowercase().as_str(), "false" | "no" | "off") => c.options.push("--no-realtime".into()),
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

/// Fichier de configuration modifiable (lanceur) : les lignes sont gardées
/// telles quelles (commentaires, options non gérées) ; `set` remplace,
/// ajoute ou retire une option.
pub struct Document {
    lines: Vec<String>,
}

/// Clé d'une ligne « option = valeur » (sans « -- », en minuscules).
fn line_key(line: &str) -> Option<String> {
    let body = line.split(['#', ';']).next().unwrap_or("");
    let (k, _) = body.split_once('=')?;
    Some(k.trim().trim_start_matches("--").to_ascii_lowercase())
}

impl Document {
    pub fn parse(text: &str) -> Self {
        Self { lines: text.lines().map(str::to_string).collect() }
    }

    /// Fichier lu, ou document vide s'il n'existe pas encore.
    pub fn load(path: &Path) -> Self {
        Self::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    /// Valeur de l'option (la dernière l'emporte, comme à la lecture).
    pub fn get(&self, key: &str) -> Option<String> {
        self.lines.iter().rev().find(|l| line_key(l).as_deref() == Some(key)).map(|l| {
            let body = l.split(['#', ';']).next().unwrap_or("");
            body.split_once('=').map(|(_, v)| v.trim().to_string()).unwrap_or_default()
        })
    }

    /// Option posée (en place de la première occurrence, les autres
    /// retirées), ou retirée (`None`).
    pub fn set(&mut self, key: &str, value: Option<&str>) {
        let mut first = None;
        let mut i = 0;
        while i < self.lines.len() {
            if line_key(&self.lines[i]).as_deref() == Some(key) {
                if first.is_none() {
                    first = Some(i);
                    i += 1;
                } else {
                    self.lines.remove(i);
                }
            } else {
                i += 1;
            }
        }
        match (first, value) {
            (Some(i), Some(v)) => self.lines[i] = format!("{key} = {v}"),
            (Some(i), None) => {
                self.lines.remove(i);
            }
            (None, Some(v)) => self.lines.push(format!("{key} = {v}")),
            (None, None) => {}
        }
    }

    pub fn text(&self) -> String {
        let mut t = self.lines.join("\n");
        t.push('\n');
        t
    }
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

    #[test]
    fn jit_off_turns_the_translator_off() {
        let c = parse("jit = off\n", Path::new(".")).unwrap();
        assert_eq!(c.options, ["--no-jit"]);
        let c = parse("jit = on\n", Path::new(".")).unwrap();
        assert_eq!(c.options, ["--jit"]);
    }

    #[test]
    fn document_keeps_comments_and_unknown_options() {
        let mut d = Document::parse("# Doom\nmips = 50 ; rapide\n--sb = pro\nvbe1 = true\nmips = 60\n");
        assert_eq!(d.get("mips").as_deref(), Some("60"));
        assert_eq!(d.get("sb").as_deref(), Some("pro"));
        d.set("mips", Some("100"));
        d.set("sb", None);
        d.set("cdrom", Some("DOOM.CUE"));
        assert_eq!(d.text(), "# Doom\nmips = 100\nvbe1 = true\ncdrom = DOOM.CUE\n");
        // Relu par l'analyseur ordinaire.
        let c = parse(&d.text(), Path::new("/j")).unwrap();
        assert_eq!(c.options, ["--mips", "100", "--vbe1", "--cdrom", "/j/DOOM.CUE"]);
    }
}
