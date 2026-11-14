// SPDX-License-Identifier: GPL-3.0-or-later
//! Lanceur (`--launcher [RÉPERTOIRE]`), seulement sur demande : `bare86`
//! sans argument reste un lancement direct. Un profil est le `bare86.conf`
//! du répertoire d'un jeu (format de `config.rs`, chemins relatifs à son
//! répertoire) ; le lanceur liste les sous-répertoires du répertoire des
//! jeux qui en ont un, lance celui qu'on choisit, et crée ou modifie un
//! profil (`Ui`, logique sans SDL ; écran dans `sdl::launcher`).

use std::path::{Path, PathBuf};

use crate::config::Document;
use crate::menu::{Browsed, Browser, COLS, Key, ROWS, Screen, Target};

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

/// Réglage d'un profil, dans l'ordre de l'éditeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Program,
    Boot,
    HardDisk,
    Cdrom,
    Floppy,
    Sound,
    Fm,
    Midi,
    MidiBank,
    Speed,
    Memory,
    Network,
    Fullscreen,
    Save,
    SaveAndRun,
}

const FIELDS: [Field; 15] = [
    Field::Program,
    Field::Boot,
    Field::HardDisk,
    Field::Cdrom,
    Field::Floppy,
    Field::Sound,
    Field::Fm,
    Field::Midi,
    Field::MidiBank,
    Field::Speed,
    Field::Memory,
    Field::Network,
    Field::Fullscreen,
    Field::Save,
    Field::SaveAndRun,
];

/// Choix d'un réglage à liste : clé du fichier et valeurs (`None` : option
/// absente, valeur par défaut), avec leurs libellés.
fn choices(f: Field) -> Option<(&'static str, &'static [(Option<&'static str>, &'static str)])> {
    Some(match f {
        Field::Sound => ("sb", &[(None, "Sound Blaster 16"), (Some("pro2"), "Sound Blaster Pro 2"), (Some("pro1"), "Sound Blaster Pro 1"), (Some("ess"), "ESS AudioDrive")]),
        Field::Fm => ("opl", &[(None, "celle de la carte"), (Some("opl2"), "OPL2"), (Some("dualopl2"), "deux OPL2"), (Some("opl3"), "OPL3"), (Some("cqm"), "Creative CQM"), (Some("esfm"), "ESS ESFM")]),
        Field::Midi => ("midi", &[(None, "synthèse FM"), (Some("off"), "aucun")]),
        // Choix restreint de timbres parlants (numéros de `--midi-bank list`) ;
        // une autre valeur écrite à la main reste affichée telle quelle.
        Field::MidiBank => (
            "midi-bank",
            &[
                (None, "Windows 95/98"),
                (Some("77"), "Windows 3.x"),
                (Some("14"), "Doom (DMX, Bobby Prince)"),
                (Some("0"), "Miles (Origin, Sierra...)"),
                (Some("2"), "HMI (Descent...)"),
                (Some("62"), "Apogee (Duke Nukem 3D)"),
            ],
        ),
        Field::Speed => ("mips", &[(None, "sans bridage"), (Some("3"), "3 MIPS"), (Some("6"), "6 MIPS"), (Some("12"), "12 MIPS"), (Some("25"), "25 MIPS"), (Some("50"), "50 MIPS"), (Some("100"), "100 MIPS"), (Some("200"), "200 MIPS"), (Some("400"), "400 MIPS")]),
        Field::Memory => ("mem", &[(Some("4"), "4 Mo"), (Some("8"), "8 Mo"), (None, "16 Mo"), (Some("32"), "32 Mo"), (Some("64"), "64 Mo")]),
        _ => return None,
    })
}

/// Réglages oui/non : clé du fichier.
fn toggle_key(f: Field) -> Option<&'static str> {
    Some(match f {
        Field::Boot => "boot",
        Field::Network => "net",
        Field::Fullscreen => "fullscreen",
        _ => return None,
    })
}

/// Réglages fichier : clé et type d'image.
fn file_field(f: Field) -> Option<(&'static str, Target)> {
    Some(match f {
        Field::Program => ("program", Target::Program),
        Field::HardDisk => ("hda", Target::HardDisk),
        Field::Cdrom => ("cdrom", Target::Cd),
        Field::Floppy => ("fda", Target::Floppy),
        _ => return None,
    })
}

fn is_true(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on"))
}

/// Dossier du jeu d'un programme : le sous-dossier du répertoire des jeux
/// qui le contient (`jeux/doom` pour `jeux/doom/c/DOOMS/DOOM.EXE`), ou son
/// propre dossier s'il est ailleurs.
fn game_dir(root: &Path, program: &Path) -> PathBuf {
    match program.strip_prefix(root).ok().and_then(|r| r.components().next()) {
        Some(first) if program.parent() != Some(root) => root.join(first),
        _ => program.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf),
    }
}

/// Profil en cours de création ou de modification.
struct Editor {
    name: String,
    dir: PathBuf,
    conf: PathBuf,
    doc: Document,
    sel: usize,
}

impl Editor {
    fn open(dir: PathBuf) -> Self {
        let conf = dir.join(PROFILE);
        let name = dir.file_name().map_or("?".into(), |n| n.to_string_lossy().into_owned());
        Self { name, doc: Document::load(&conf), conf, dir, sel: 0 }
    }

    /// Chemin écrit dans le profil : relatif au dossier du jeu s'il y est.
    fn store_path(&self, p: &Path) -> String {
        p.strip_prefix(&self.dir).map_or_else(|_| p.to_string_lossy().into_owned(), |r| r.to_string_lossy().into_owned())
    }

    fn resolve(&self, v: &str) -> PathBuf {
        let p = Path::new(v);
        if p.is_relative() { self.dir.join(p) } else { p.to_path_buf() }
    }

    fn value(&self, f: Field) -> String {
        if let Some((key, list)) = choices(f) {
            let cur = self.doc.get(key);
            if let Some((_, l)) = list.iter().find(|(v, _)| v.map(str::to_string) == cur) {
                return l.to_string();
            }
            // Valeur hors liste (fichier écrit à la main) : telle quelle,
            // avec le nom de la banque MIDI.
            let cur = cur.unwrap_or_default();
            if f == Field::MidiBank {
                let names = crate::midisynth::bank_names();
                let name = cur.parse::<usize>().ok().and_then(|n| names.get(n).cloned()).unwrap_or_default();
                return format!("{cur} {name}");
            }
            return cur;
        }
        if let Some(key) = toggle_key(f) {
            return if is_true(self.doc.get(key)) { "oui".into() } else { "non".into() };
        }
        if let Some((key, _)) = file_field(f) {
            return self.doc.get(key).unwrap_or_else(|| "(aucun)".into());
        }
        match f {
            _ => String::new(),
        }
    }

    fn label(f: Field) -> &'static str {
        match f {
            Field::Program => "Programme",
            Field::Boot => "Démarrer un vrai DOS",
            Field::HardDisk => "Disque dur C:",
            Field::Cdrom => "CD-ROM",
            Field::Floppy => "Disquette A:",
            Field::Sound => "Carte son",
            Field::Fm => "Puce FM",
            Field::Midi => "Synthé General MIDI",
            Field::MidiBank => "Timbres General MIDI",
            Field::Speed => "Vitesse",
            Field::Memory => "Mémoire",
            Field::Network => "Réseau",
            Field::Fullscreen => "Plein écran",
            Field::Save => "[ Enregistrer ]",
            Field::SaveAndRun => "[ Enregistrer et lancer ]",
        }
    }

    /// Aide d'un réglage (ligne du bas de l'éditeur).
    fn hint(f: Field) -> &'static str {
        match f {
            Field::Program => "Programme lancé par le DOS intégré",
            Field::Boot => "Démarre le disque dur (FreeDOS...) au lieu du DOS intégré",
            Field::HardDisk => "Image de disque dur pour C:, avec un vrai DOS",
            Field::Cdrom => "Image .iso ou .cue du CD du jeu (lecteur D:)",
            Field::Floppy => "Image de disquette pour A:",
            Field::Sound => "Son numérique et musique FM (réglage Sound Blaster du jeu)",
            Field::Fm => "Puce FM : « celle de la carte » convient presque toujours",
            Field::Midi => "Seulement si le jeu est réglé sur General MIDI",
            Field::MidiBank => "Sonorité du synthé General MIDI (sans matériel d'époque)",
            Field::Speed => "Bride le processeur pour les jeux trop rapides",
            Field::Memory => "Mémoire totale de la machine",
            Field::Network => "Carte réseau NE2000, accès à Internet",
            Field::Fullscreen => "Démarre en plein écran (Alt+Entrée bascule)",
            Field::Save => "Écrit bare86.conf dans le dossier du jeu",
            Field::SaveAndRun => "Écrit bare86.conf et lance le jeu",
        }
    }

    /// Valeur suivante (ou précédente) d'un réglage à liste ou oui/non.
    fn step(&mut self, f: Field, forward: bool) {
        if let Some((key, list)) = choices(f) {
            let cur = self.doc.get(key);
            let i = list.iter().position(|(v, _)| v.map(str::to_string) == cur).unwrap_or(0);
            let n = list.len();
            let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
            self.doc.set(key, list[j].0);
        } else if let Some(key) = toggle_key(f) {
            let on = is_true(self.doc.get(key));
            self.doc.set(key, (!on).then_some("true"));
        }
    }

    fn save(&self) -> Result<(), String> {
        std::fs::write(&self.conf, self.doc.text()).map_err(|e| format!("{} : {e}", self.conf.display()))
    }
}

/// Ce que l'écran du lanceur doit faire après une touche.
#[derive(Debug, PartialEq)]
pub enum UiAction {
    None,
    Launch(PathBuf),
    Quit,
}

enum Page {
    List,
    /// Explorateur : dossier d'un nouveau jeu, ou fichier d'un réglage.
    Browse(Browser, Option<Field>),
    Edit(Editor),
}

/// Lanceur : liste des jeux, création et modification des profils.
pub struct Ui {
    root: PathBuf,
    profiles: Vec<Profile>,
    sel: usize,
    page: Page,
    /// Éditeur mis de côté pendant le choix d'un fichier pour un réglage.
    parked: Option<Editor>,
    message: Option<String>,
}

const NEW_GAME: &str = "+ Nouveau jeu...";

impl Ui {
    pub fn new(root: PathBuf) -> Self {
        let root = root.canonicalize().unwrap_or(root);
        Self { profiles: scan(&root), root, sel: 0, page: Page::List, parked: None, message: None }
    }

    fn refresh(&mut self, select: Option<&Path>) {
        self.profiles = scan(&self.root);
        if let Some(i) = select.and_then(|c| self.profiles.iter().position(|p| p.conf == c)) {
            self.sel = i;
        }
        self.sel = self.sel.min(self.profiles.len());
    }

    pub fn key(&mut self, key: Key) -> UiAction {
        self.message = None;
        match &mut self.page {
            Page::List => {
                let last = self.profiles.len(); // la ligne « Nouveau jeu »
                match key {
                    Key::Esc => return UiAction::Quit,
                    Key::Up => self.sel = self.sel.saturating_sub(1),
                    Key::Down => self.sel = (self.sel + 1).min(last),
                    Key::PageUp => self.sel = self.sel.saturating_sub(10),
                    Key::PageDown => self.sel = (self.sel + 10).min(last),
                    Key::Home => self.sel = 0,
                    Key::End => self.sel = last,
                    Key::Enter if self.sel < last => return UiAction::Launch(self.profiles[self.sel].conf.clone()),
                    // Nouveau jeu : choisir directement son programme.
                    Key::Enter => self.page = Page::Browse(Browser::open(Target::Program, self.root.clone(), None), None),
                    Key::Edit if self.sel < last => {
                        let dir = self.profiles[self.sel].conf.parent().map(Path::to_path_buf).unwrap_or_default();
                        self.page = Page::Edit(Editor::open(dir));
                    }
                    _ => {}
                }
            }
            Page::Browse(b, field) => {
                let field = *field;
                match b.key(key) {
                    Browsed::Pending => {}
                    // Retour à l'éditeur qui avait ouvert l'explorateur, ou à la liste.
                    Browsed::Cancelled => {
                        self.page = match self.parked.take() {
                            Some(ed) => Page::Edit(ed),
                            None => Page::List,
                        }
                    }
                    Browsed::Chosen(path) => match (field, self.parked.take()) {
                        (Some(f), Some(mut ed)) => {
                            if let Some((k, _)) = file_field(f) {
                                let v = ed.store_path(&path);
                                ed.doc.set(k, Some(&v));
                            }
                            self.page = Page::Edit(ed);
                        }
                        // Nouveau jeu : son dossier est celui, sous le répertoire
                        // des jeux, qui contient le programme choisi.
                        _ => {
                            let dir = game_dir(&self.root, &path);
                            let mut ed = Editor::open(dir);
                            let v = ed.store_path(&path);
                            ed.doc.set("program", Some(&v));
                            self.page = Page::Edit(ed);
                        }
                    },
                }
            }
            Page::Edit(ed) => {
                let f = FIELDS[ed.sel];
                match key {
                    Key::Esc => self.page = Page::List,
                    Key::Up => ed.sel = (ed.sel + FIELDS.len() - 1) % FIELDS.len(),
                    Key::Down => ed.sel = (ed.sel + 1) % FIELDS.len(),
                    Key::Home => ed.sel = 0,
                    Key::End => ed.sel = FIELDS.len() - 1,
                    Key::Left | Key::Right => ed.step(f, key == Key::Right),
                    Key::Delete | Key::Backspace => {
                        if let Some(k) = file_field(f).map(|(k, _)| k).or(choices(f).map(|(k, _)| k)) {
                            ed.doc.set(k, None);
                        } else if let Some(k) = toggle_key(f) {
                            ed.doc.set(k, None);
                        }
                    }
                    Key::Enter => match f {
                        Field::Save | Field::SaveAndRun => {
                            let conf = ed.conf.clone();
                            match ed.save() {
                                Ok(()) => {
                                    self.page = Page::List;
                                    self.refresh(Some(&conf));
                                    if f == Field::SaveAndRun {
                                        return UiAction::Launch(conf);
                                    }
                                    if !conf.starts_with(&self.root) || !self.profiles.iter().any(|p| p.conf == conf) {
                                        self.message = Some("Enregistré (hors du répertoire des jeux : absent de la liste)".into());
                                    }
                                }
                                Err(e) => self.message = Some(e),
                            }
                        }
                        _ if file_field(f).is_some() => {
                            let (key, target) = file_field(f).unwrap_or(("", Target::Program));
                            let current = ed.doc.get(key).map(|v| ed.resolve(&v));
                            let start = current.as_deref().and_then(Path::parent).map_or_else(|| ed.dir.clone(), Path::to_path_buf);
                            let browser = Browser::open(target, start, current.as_deref());
                            let Page::Edit(ed) = std::mem::replace(&mut self.page, Page::List) else { unreachable!() };
                            self.page = Page::Browse(browser, Some(f));
                            self.parked = Some(ed);
                        }
                        _ => ed.step(f, true),
                    },
                    _ => {}
                }
            }
        }
        UiAction::None
    }

    /// Cellules de l'écran.
    pub fn render(&self) -> Vec<(u8, u8)> {
        let mut s = Screen::new();
        match &self.page {
            Page::List => {
                let mut lines: Vec<String> = self.profiles.iter().map(line).collect();
                lines.push(NEW_GAME.into());
                // Titre : nom du répertoire des jeux (le chemin complet ne
                // tiendrait pas dans le cadre).
                let name = self.root.file_name().map_or_else(|| self.root.display().to_string(), |n| n.to_string_lossy().into_owned());
                let name: String = name.chars().take(COLS - 20).collect();
                return crate::menu::list_page(
                    &format!("Bare86 : {name}"),
                    &lines,
                    self.sel,
                    "",
                    "Entrée lancer   E modifier   Échap quitter",
                )
                .into_iter()
                .enumerate()
                .map(|(i, c)| self.overlay_message(i, c))
                .collect();
            }
            Page::Browse(b, _) => b.draw(&mut s),
            Page::Edit(ed) => {
                s.frame(&format!("Profil : {}", ed.name));
                for (i, f) in FIELDS.iter().enumerate() {
                    let text = match f {
                        Field::Save | Field::SaveAndRun => Editor::label(*f).to_string(),
                        _ => format!("{:<22}{}", Editor::label(*f), ed.value(*f)),
                    };
                    s.item(2 + i, &text, i == ed.sel);
                }
                s.text(ROWS - 3, 3, Editor::hint(FIELDS[ed.sel]), 0x1B);
                s.footer("←→ changer  Entrée choisir  Suppr effacer  Échap annuler");
            }
        }
        let mut cells = s.cells;
        for (i, c) in cells.iter_mut().enumerate() {
            *c = self.overlay_message(i, *c);
        }
        cells
    }

    /// Message (enregistrement, erreur) sur l'avant-dernière ligne.
    fn overlay_message(&self, i: usize, c: (u8, u8)) -> (u8, u8) {
        let Some(m) = &self.message else { return c };
        let (row, col) = (i / COLS, i % COLS);
        if row != ROWS - 3 || col < 3 {
            return c;
        }
        match m.chars().nth(col - 3) {
            Some(ch) if col < COLS - 2 => (crate::menu::cp437_of(ch), 0x1C),
            _ => c,
        }
    }
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

    fn keys(ui: &mut Ui, keys: &[Key]) -> UiAction {
        let mut last = UiAction::None;
        for &k in keys {
            last = ui.key(k);
        }
        last
    }

    fn screen(ui: &Ui) -> String {
        ui.render().chunks(COLS).map(|r| r.iter().map(|&(c, _)| crate::bios::cp437(c)).collect::<String>() + "\n").collect()
    }

    #[test]
    fn edit_and_create_profiles() {
        let root = std::env::temp_dir().join(format!("bare86-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("doom")).unwrap();
        std::fs::create_dir_all(root.join("wolf")).unwrap();
        std::fs::write(root.join("doom/DOOM.EXE"), b"").unwrap();
        std::fs::write(root.join("wolf/WOLF3D.EXE"), b"").unwrap();
        std::fs::write(root.join("doom").join(PROFILE), "# mon Doom\nvbe1 = true\n").unwrap();
        let mut ui = Ui::new(root.clone());
        let t = screen(&ui);
        assert!(t.contains("doom") && t.contains("Nouveau jeu") && !t.contains("wolf"), "{t}");

        // Modifier Doom : carte son → SB Pro 2 (Sound : 6e réglage), vitesse
        // → 3 MIPS, réseau → oui ; programme choisi dans l'explorateur.
        ui.key(Key::Edit);
        assert!(screen(&ui).contains("Profil : doom"));
        keys(&mut ui, &[Key::Down; 5]);
        ui.key(Key::Right);
        keys(&mut ui, &[Key::Down; 4]);
        ui.key(Key::Right);
        keys(&mut ui, &[Key::Down; 2]);
        ui.key(Key::Enter);
        let t = screen(&ui);
        assert!(t.contains("Sound Blaster Pro 2") && t.contains("3 MIPS"), "{t}");
        // Programme : explorateur dans le dossier du jeu, DOOM.EXE (après « .. »).
        ui.key(Key::Home);
        ui.key(Key::Enter);
        assert!(screen(&ui).contains("DOOM.EXE"));
        // Échap : retour à l'éditeur, rien de changé ; puis choix réel.
        ui.key(Key::Esc);
        assert!(screen(&ui).contains("Profil : doom"));
        ui.key(Key::Enter);
        keys(&mut ui, &[Key::Down, Key::Enter]);
        assert!(screen(&ui).contains("DOOM.EXE"), "{}", screen(&ui));
        // Enregistrer (avant-dernière ligne).
        ui.key(Key::End);
        ui.key(Key::Up);
        assert_eq!(ui.key(Key::Enter), UiAction::None);
        let text = std::fs::read_to_string(root.join("doom").join(PROFILE)).unwrap();
        assert_eq!(text, "# mon Doom\nvbe1 = true\nsb = pro2\nmips = 3\nnet = true\nprogram = DOOM.EXE\n");

        // Nouveau jeu : on choisit directement son programme (« .. », doom/,
        // wolf/ ; puis « .. », WOLF3D.EXE) ; le profil va dans wolf/.
        keys(&mut ui, &[Key::End, Key::Enter]);
        let t = screen(&ui);
        assert!(t.contains("Programme à lancer") && t.contains("wolf/"), "{t}");
        keys(&mut ui, &[Key::Down, Key::Down, Key::Down, Key::Enter]);
        assert!(screen(&ui).contains("WOLF3D.EXE"), "{}", screen(&ui));
        keys(&mut ui, &[Key::Down, Key::Enter]);
        let t = screen(&ui);
        assert!(t.contains("Profil : wolf") && t.contains("WOLF3D.EXE"), "{t}");
        // Enregistrer et lancer (dernière ligne).
        ui.key(Key::End);
        assert_eq!(ui.key(Key::Enter), UiAction::Launch(root.canonicalize().unwrap().join("wolf").join(PROFILE)));
        assert!(screen(&ui).contains("wolf"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn game_directory_of_a_program() {
        let root = Path::new("/jeux");
        assert_eq!(game_dir(root, Path::new("/jeux/doom/c/DOOMS/DOOM.EXE")), Path::new("/jeux/doom"));
        assert_eq!(game_dir(root, Path::new("/jeux/dune/DUNE.BAT")), Path::new("/jeux/dune"));
        // Programme posé à la racine, ou ailleurs : son propre dossier.
        assert_eq!(game_dir(root, Path::new("/jeux/JEU.EXE")), Path::new("/jeux"));
        assert_eq!(game_dir(root, Path::new("/autre/x/JEU.EXE")), Path::new("/autre/x"));
    }

    #[test]
    fn midi_timbres_are_a_short_list() {
        let dir = std::env::temp_dir().join(format!("bare86-timbres-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PROFILE), "midi-bank = 72\n").unwrap();
        let mut ed = Editor::open(dir.clone());
        // Valeur écrite à la main : gardée, affichée avec le nom de la banque.
        assert!(ed.value(Field::MidiBank).starts_with("72 WOPL (DMXOPL3"), "{}", ed.value(Field::MidiBank));
        ed.doc.set("midi-bank", None);
        assert_eq!(ed.value(Field::MidiBank), "Windows 95/98");
        ed.step(Field::MidiBank, true);
        assert_eq!((ed.value(Field::MidiBank), ed.doc.get("midi-bank")), ("Windows 3.x".to_string(), Some("77".to_string())));
        ed.step(Field::MidiBank, false);
        ed.step(Field::MidiBank, false);
        assert_eq!(ed.doc.get("midi-bank").as_deref(), Some("62"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
