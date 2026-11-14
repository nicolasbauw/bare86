// SPDX-License-Identifier: GPL-3.0-or-later
//! Menu de Bare86 (Ctrl+Alt+M dans la fenêtre, lettre réglable) : changement de disquette et
//! de CD, vitesse, plein écran, capture, rappel des raccourcis. Jamais
//! affiché sans être demandé ; l'invité continue de tourner dessous (il ne
//! reçoit plus le clavier tant que le menu est ouvert).
//!
//! Indépendant de SDL : la fenêtre traduit ses touches en `Key`, exécute
//! les `Action` rendues et affiche les pixels de `rasterize` par-dessus
//! l'image. Rendu en mode texte façon DOS (cellules CP437, palette CGA,
//! police intégrée).

use std::path::{Path, PathBuf};

use crate::font::Font;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    PageUp,
    PageDown,
    Home,
    End,
    Backspace,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Close,
    Speed(Option<f64>),
    Fullscreen,
    Screenshot,
    Floppy(PathBuf),
    /// Nouveau disque (ou éjection).
    Cd(Option<PathBuf>),
    Quit,
}

/// Ce que montre le menu ; tenu à jour par la fenêtre.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub floppy: Option<PathBuf>,
    /// Lecteur de CD présent (`--cdrom` au lancement), et son disque.
    pub cd_drive: bool,
    pub cd: Option<PathBuf>,
    pub speed: Option<f64>,
    pub fullscreen: bool,
    /// Images de A: données au lancement (`--fda` répété, Ctrl+Alt+D).
    pub floppies: Vec<PathBuf>,
    /// Lettre du raccourci du menu (Ctrl+Alt+lettre).
    pub menu_letter: char,
    /// Message affiché en bas du menu (erreur d'ouverture d'une image…).
    pub message: Option<String>,
}

pub const COLS: usize = 64;
pub const ROWS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Floppy,
    Cd,
    Eject,
    Speed,
    Fullscreen,
    Screenshot,
    Keys,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Floppy,
    Cd,
}

impl Target {
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Target::Floppy => &["img", "ima", "vfd", "flp", "dsk"],
            Target::Cd => &["iso", "cue"],
        }
    }
}

struct Browser {
    target: Target,
    dir: PathBuf,
    /// Entrées : nom, répertoire.
    entries: Vec<(String, bool)>,
    sel: usize,
    top: usize,
}

impl Browser {
    /// Liste de `dir`, l'image `current` présélectionnée si elle y est.
    fn open(target: Target, dir: PathBuf, current: Option<&Path>) -> Self {
        let mut b = Self { target, dir, entries: Vec::new(), sel: 0, top: 0 };
        b.load();
        let name = current.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        if let Some(i) = name.and_then(|n| b.entries.iter().position(|(e, d)| !d && *e == n)) {
            b.sel = i;
            b.top = i.saturating_sub(LIST_ROWS - 1);
        }
        b
    }

    /// Répertoires puis images du bon type, triés ; « .. » en tête.
    fn load(&mut self) {
        let exts = self.target.extensions();
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let Ok(name) = e.file_name().into_string() else { continue };
                if name.starts_with('.') {
                    continue;
                }
                let path = e.path();
                if path.is_dir() {
                    dirs.push(name);
                } else if path.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e))) {
                    files.push(name);
                }
            }
        }
        dirs.sort_by_key(|n| n.to_lowercase());
        files.sort_by_key(|n| n.to_lowercase());
        self.entries = Vec::new();
        if self.dir.parent().is_some() {
            self.entries.push(("..".into(), true));
        }
        self.entries.extend(dirs.into_iter().map(|d| (d, true)));
        self.entries.extend(files.into_iter().map(|f| (f, false)));
        self.sel = 0;
        self.top = 0;
    }

    fn enter(&mut self) -> Option<PathBuf> {
        let (name, is_dir) = self.entries.get(self.sel)?.clone();
        if !is_dir {
            return Some(self.dir.join(name));
        }
        let next = if name == ".." { self.dir.parent()?.to_path_buf() } else { self.dir.join(name) };
        let from = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
        self.dir = next;
        self.load();
        // En remontant, le répertoire quitté reste sélectionné.
        if let Some(i) = from.and_then(|f| self.entries.iter().position(|(n, _)| *n == f)) {
            self.sel = i;
        }
        None
    }
}

enum Page {
    Main,
    Keys,
    Browse(Browser),
}

pub struct Menu {
    sel: usize,
    page: Page,
}

/// Lignes visibles de la liste de fichiers.
const LIST_ROWS: usize = ROWS - 6;

fn start_dir(current: Option<&Path>) -> PathBuf {
    current
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn name_of(p: Option<&Path>) -> String {
    p.and_then(Path::file_name).map_or("(aucun)".into(), |n| n.to_string_lossy().into_owned())
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    pub fn new() -> Self {
        Self { sel: 0, page: Page::Main }
    }

    fn items(st: &State) -> Vec<Item> {
        let mut v = vec![Item::Floppy, Item::Cd];
        if st.cd_drive && st.cd.is_some() {
            v.push(Item::Eject);
        }
        v.extend([Item::Speed, Item::Fullscreen, Item::Screenshot, Item::Keys, Item::Quit]);
        v
    }

    pub fn key(&mut self, key: Key, st: &State) -> Action {
        if key == Key::Esc && matches!(self.page, Page::Browse(_)) {
            self.page = Page::Main;
            return Action::None;
        }
        match &mut self.page {
            Page::Keys => {
                if matches!(key, Key::Esc | Key::Enter | Key::Backspace) {
                    self.page = Page::Main;
                }
                Action::None
            }
            Page::Browse(b) => {
                let n = b.entries.len();
                match key {
                    Key::Esc => {}
                    Key::Up => b.sel = b.sel.saturating_sub(1),
                    Key::Down => b.sel = (b.sel + 1).min(n.saturating_sub(1)),
                    Key::PageUp => b.sel = b.sel.saturating_sub(LIST_ROWS),
                    Key::PageDown => b.sel = (b.sel + LIST_ROWS).min(n.saturating_sub(1)),
                    Key::Home => b.sel = 0,
                    Key::End => b.sel = n.saturating_sub(1),
                    Key::Backspace | Key::Left => {
                        if b.entries.first().is_some_and(|(n, _)| n == "..") {
                            b.sel = 0;
                            b.enter();
                        }
                    }
                    Key::Enter | Key::Right => {
                        if let Some(path) = b.enter().filter(|_| key == Key::Enter) {
                            let target = b.target;
                            self.page = Page::Main;
                            return match target {
                                Target::Floppy => Action::Floppy(path),
                                Target::Cd => Action::Cd(Some(path)),
                            };
                        }
                    }
                }
                if b.sel < b.top {
                    b.top = b.sel;
                } else if b.sel >= b.top + LIST_ROWS {
                    b.top = b.sel + 1 - LIST_ROWS;
                }
                Action::None
            }
            Page::Main => {
                let items = Self::items(st);
                self.sel = self.sel.min(items.len() - 1);
                let item = items[self.sel];
                match key {
                    Key::Esc => Action::Close,
                    Key::Up => {
                        self.sel = (self.sel + items.len() - 1) % items.len();
                        Action::None
                    }
                    Key::Down => {
                        self.sel = (self.sel + 1) % items.len();
                        Action::None
                    }
                    Key::Home => {
                        self.sel = 0;
                        Action::None
                    }
                    Key::End => {
                        self.sel = items.len() - 1;
                        Action::None
                    }
                    Key::Left | Key::Right if item == Item::Speed => Action::Speed(crate::sdl::step_speed(st.speed, key == Key::Right)),
                    Key::Enter => match item {
                        Item::Floppy => {
                            self.page = Page::Browse(Browser::open(Target::Floppy, start_dir(st.floppy.as_deref()), st.floppy.as_deref()));
                            Action::None
                        }
                        Item::Cd if st.cd_drive => {
                            self.page = Page::Browse(Browser::open(Target::Cd, start_dir(st.cd.as_deref()), st.cd.as_deref()));
                            Action::None
                        }
                        Item::Cd => Action::None,
                        Item::Eject => Action::Cd(None),
                        Item::Speed => Action::Speed(crate::sdl::step_speed(st.speed, true)),
                        Item::Fullscreen => Action::Fullscreen,
                        Item::Screenshot => Action::Screenshot,
                        Item::Keys => {
                            self.page = Page::Keys;
                            Action::None
                        }
                        Item::Quit => Action::Quit,
                    },
                    _ => Action::None,
                }
            }
        }
    }

    /// Cellules (caractère CP437, attribut) de la boîte, `COLS` × `ROWS`.
    pub fn render(&self, st: &State) -> Vec<(u8, u8)> {
        let mut s = Screen::new();
        match &self.page {
            Page::Main => {
                s.frame("Bare86");
                let speed = st.speed.map_or("sans bridage".into(), |m| format!("{m} MIPS"));
                for (i, item) in Self::items(st).into_iter().enumerate() {
                    let text = match item {
                        Item::Floppy => format!("Disquette A:      {}", name_of(st.floppy.as_deref())),
                        Item::Cd if st.cd_drive => format!("CD-ROM            {}", name_of(st.cd.as_deref())),
                        Item::Cd => "CD-ROM            (pas de lecteur : option --cdrom)".into(),
                        Item::Eject => "Éjecter le CD".into(),
                        Item::Speed => format!("Vitesse           ◄ {speed} ►"),
                        Item::Fullscreen => format!("Plein écran       {}", if st.fullscreen { "oui" } else { "non" }),
                        Item::Screenshot => "Capture d'écran".into(),
                        Item::Keys => "Raccourcis clavier".into(),
                        Item::Quit => "Quitter Bare86".into(),
                    };
                    s.item(2 + i, &text, i == self.sel);
                }
                s.footer("↑↓ choisir   Entrée valider   ←→ régler   Échap fermer");
            }
            Page::Keys => {
                s.frame("Raccourcis clavier");
                let menu = format!("Ctrl+Alt+{}        ce menu", st.menu_letter);
                let lines = [
                    menu.as_str(),
                    "Alt+Entrée        plein écran (aussi Ctrl+Alt+F)",
                    "Ctrl+Alt+D        disquette suivante (--fda répété)",
                    "Ctrl+Alt+S        capture d'écran (bare86_NNN.png)",
                    "Ctrl+Alt+G        libérer la souris (ou bouton central)",
                    "Ctrl+Alt+−        plus lent",
                    "Ctrl+Alt++        plus vite",
                    "",
                    "Un clic dans la fenêtre capture la souris.",
                ];
                for (i, l) in lines.iter().enumerate() {
                    s.text(2 + i, 3, l, ATTR_TEXT);
                }
                s.footer("Échap retour");
            }
            Page::Browse(b) => {
                s.frame(match b.target {
                    Target::Floppy => "Image de disquette pour A:",
                    Target::Cd => "Image de CD (.iso, .cue)",
                });
                let dir = b.dir.to_string_lossy();
                let dir: String = if dir.chars().count() > COLS - 6 {
                    let tail: String = dir.chars().rev().take(COLS - 9).collect::<Vec<_>>().into_iter().rev().collect();
                    format!("...{tail}")
                } else {
                    dir.into_owned()
                };
                s.text(2, 3, &dir, ATTR_TITLE);
                if b.entries.is_empty() {
                    s.text(4, 3, "(rien ici)", ATTR_TEXT);
                }
                for (row, (i, (name, is_dir))) in b.entries.iter().enumerate().skip(b.top).take(LIST_ROWS).enumerate() {
                    let text = if *is_dir { format!("{name}/") } else { name.clone() };
                    s.item(4 + row, &text, i == b.sel);
                }
                s.footer("Entrée choisir   ← dossier parent   Échap retour");
            }
        }
        if let Some(m) = &st.message {
            s.text(ROWS - 3, 3, m, ATTR_MESSAGE);
        }
        s.cells
    }
}

/// Liste plein cadre (lanceur) : titre, lignes, ligne choisie, pied ;
/// défile pour garder la ligne choisie visible.
pub fn list_page(title: &str, lines: &[String], sel: usize, empty: &str, footer: &str) -> Vec<(u8, u8)> {
    let mut s = Screen::new();
    s.frame(title);
    let rows = ROWS - 5;
    let top = sel.saturating_sub(rows - 1);
    if lines.is_empty() {
        s.text(2, 3, empty, ATTR_TEXT);
    }
    for (row, (i, l)) in lines.iter().enumerate().skip(top).take(rows).enumerate() {
        s.item(2 + row, l, i == sel);
    }
    s.footer(footer);
    s.cells
}

const ATTR_BOX: u8 = 0x1F;
const ATTR_TEXT: u8 = 0x17;
const ATTR_TITLE: u8 = 0x1E;
const ATTR_SEL: u8 = 0x70;
const ATTR_MESSAGE: u8 = 0x1C;

/// CP437 d'un caractère ; lettre sans accent à défaut.
fn cp437_of(c: char) -> u8 {
    if c.is_ascii() && !c.is_ascii_control() {
        return c as u8;
    }
    if let Some(b) = (0x80..=0xFFu8).chain(1..0x20).find(|&b| crate::bios::cp437(b) == c) {
        return b;
    }
    let plain = match c {
        'À' | 'Â' => 'A',
        'È' | 'Ê' | 'Ë' => 'E',
        'Î' | 'Ï' => 'I',
        'Ô' => 'O',
        'Ù' | 'Û' => 'U',
        'œ' => 'o',
        _ => '?',
    };
    plain as u8
}

struct Screen {
    cells: Vec<(u8, u8)>,
}

impl Screen {
    fn new() -> Self {
        Self { cells: vec![(b' ', ATTR_TEXT); COLS * ROWS] }
    }

    fn put(&mut self, row: usize, col: usize, c: u8, attr: u8) {
        if row < ROWS && col < COLS {
            self.cells[row * COLS + col] = (c, attr);
        }
    }

    fn text(&mut self, row: usize, col: usize, text: &str, attr: u8) {
        for (i, ch) in text.chars().enumerate() {
            if col + i >= COLS - 1 {
                break;
            }
            self.put(row, col + i, cp437_of(ch), attr);
        }
    }

    /// Cadre double, titre centré.
    fn frame(&mut self, title: &str) {
        for col in 0..COLS {
            self.put(0, col, 0xCD, ATTR_BOX);
            self.put(ROWS - 1, col, 0xCD, ATTR_BOX);
        }
        for row in 0..ROWS {
            self.put(row, 0, 0xBA, ATTR_BOX);
            self.put(row, COLS - 1, 0xBA, ATTR_BOX);
        }
        self.put(0, 0, 0xC9, ATTR_BOX);
        self.put(0, COLS - 1, 0xBB, ATTR_BOX);
        self.put(ROWS - 1, 0, 0xC8, ATTR_BOX);
        self.put(ROWS - 1, COLS - 1, 0xBC, ATTR_BOX);
        let t = format!(" {title} ");
        let col = (COLS - t.chars().count()) / 2;
        self.text(0, col, &t, ATTR_TITLE);
    }

    /// Ligne de menu, en vidéo inverse si choisie.
    fn item(&mut self, row: usize, text: &str, selected: bool) {
        let attr = if selected { ATTR_SEL } else { ATTR_TEXT };
        for col in 2..COLS - 2 {
            self.put(row, col, b' ', attr);
        }
        self.text(row, 3, text, attr);
    }

    fn footer(&mut self, text: &str) {
        let col = (COLS - text.chars().count()) / 2;
        self.text(ROWS - 2, col, text, ATTR_BOX);
    }
}

/// Palette CGA (XRGB).
const PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555, 0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555,
    0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// Pixels XRGB des cellules : (`COLS` × largeur de police) × (`ROWS` × hauteur).
pub fn rasterize(cells: &[(u8, u8)], font: &Font) -> (usize, usize, Vec<u32>) {
    let (fw, fh) = (font.width, font.height);
    let (w, h) = (COLS * fw, ROWS * fh);
    let mut px = vec![0u32; w * h];
    for (i, &(c, attr)) in cells.iter().enumerate() {
        let (row, col) = (i / COLS, i % COLS);
        let (fg, bg) = (PALETTE[usize::from(attr & 0x0F)], PALETTE[usize::from(attr >> 4 & 0x07)]);
        for y in 0..fh {
            let bits = font.row(c, y);
            let line = &mut px[(row * fh + y) * w + col * fw..][..fw];
            for (x, p) in line.iter_mut().enumerate() {
                *p = if bits & (0x80 >> x) != 0 { fg } else { bg };
            }
        }
    }
    (w, h, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(cells: &[(u8, u8)]) -> String {
        cells.chunks(COLS).map(|r| r.iter().map(|&(c, _)| crate::bios::cp437(c)).collect::<String>() + "\n").collect()
    }

    #[test]
    fn main_page_actions() {
        let st = State { cd_drive: true, cd: Some("/jeux/a.iso".into()), speed: Some(50.0), ..State::default() };
        let mut m = Menu::new();
        let t = text_of(&m.render(&st));
        assert!(t.contains("a.iso") && t.contains("Éjecter le CD") && t.contains("◄ 50 MIPS ►"), "{t}");
        // Ordre : disquette, CD, éjecter, vitesse…
        assert_eq!(m.key(Key::Down, &st), Action::None);
        assert_eq!(m.key(Key::Down, &st), Action::None);
        assert_eq!(m.key(Key::Enter, &st), Action::Cd(None));
        m.key(Key::Down, &st);
        assert_eq!(m.key(Key::Right, &st), Action::Speed(Some(100.0)));
        assert_eq!(m.key(Key::Up, &st), Action::None);
        assert_eq!(m.key(Key::Esc, &st), Action::Close);
        // Remonter depuis le haut : dernière ligne (Quitter).
        let mut m = Menu::new();
        m.key(Key::Up, &st);
        assert_eq!(m.key(Key::Enter, &st), Action::Quit);
    }

    #[test]
    fn browser_filters_and_navigates() {
        let dir = std::env::temp_dir().join(format!("bare86-menu-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Jeux")).unwrap();
        for f in ["b.ISO", "a.cue", "a.bin", "notes.txt", ".cache.iso"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        std::fs::write(dir.join("Jeux/c.iso"), b"").unwrap();
        let st = State { cd_drive: true, cd: Some(dir.join("b.ISO")), ..State::default() };
        let mut m = Menu::new();
        m.key(Key::Down, &st);
        m.key(Key::Enter, &st);
        let t = text_of(&m.render(&st));
        assert!(t.contains("Jeux/") && t.contains("a.cue") && t.contains("b.ISO"), "{t}");
        assert!(!t.contains("a.bin") && !t.contains("notes.txt") && !t.contains(".cache"), "{t}");
        // « .. », Jeux/, a.cue, b.ISO : le disque en cours est présélectionné.
        let base = dir.canonicalize().unwrap();
        assert_eq!(m.key(Key::Enter, &st), Action::Cd(Some(base.join("b.ISO"))));
        // De retour au menu (sur « CD-ROM ») : rouvrir, entrer dans Jeux, choisir c.iso.
        m.key(Key::Enter, &st);
        m.key(Key::Home, &st);
        m.key(Key::Down, &st);
        m.key(Key::Enter, &st);
        m.key(Key::Down, &st);
        assert_eq!(m.key(Key::Enter, &st), Action::Cd(Some(base.join("Jeux/c.iso"))));
        // Retour au menu principal après le choix.
        assert!(text_of(&m.render(&st)).contains("Quitter Bare86"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rasterize_size_and_colours() {
        let st = State::default();
        let cells = Menu::new().render(&st);
        let font = Font::builtin();
        let (w, h, px) = rasterize(&cells, &font);
        assert_eq!((w, h, px.len()), (COLS * 8, ROWS * 16, COLS * 8 * ROWS * 16));
        // Fond bleu, ligne choisie sur fond gris.
        assert!(px.contains(&0x0000AA) && px.contains(&0xAAAAAA));
        assert_eq!(cp437_of('é'), 0x82);
        assert_eq!(cp437_of('È'), b'E');
        assert_eq!(cp437_of('◄'), 0x11);
    }
}
