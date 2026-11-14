// SPDX-License-Identifier: GPL-3.0-or-later
//! Disque compact : image (ISO 9660, ou CUE/BIN avec pistes audio),
//! commandes SCSI/MMC d'un lecteur ATAPI (paquets de 12 octets) et lecture
//! des pistes audio (`Player`, mixé avec la SB16). Indépendant du
//! transport : le contrôleur IDE (`ide.rs`) achemine paquets et données,
//! MSCDEX (`dos/mscdex.rs`) partage le disque et le lecteur audio.

use std::cell::RefCell;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

pub const SECTOR: usize = 2048;
/// Secteur brut (synchronisation, en-tête, données, EDC/ECC ; ou 588
/// trames audio stéréo de 16 bits à 44,1 kHz).
pub const RAW_SECTOR: usize = 2352;
/// Décalage de l'adressage MSF : 2 s de pré-espace (150 trames).
const MSF_OFFSET: u32 = 150;

#[derive(Clone, Debug)]
pub struct Track {
    pub number: u8,
    pub start: u32,
    pub sectors: u32,
    pub audio: bool,
    /// Fichier de l'image, taille d'un secteur dans le fichier, décalage
    /// des données utiles dans le secteur, position du premier secteur.
    file: usize,
    sector_size: u64,
    data_offset: u64,
    file_offset: u64,
}

pub struct Disc {
    files: Vec<File>,
    pub tracks: Vec<Track>,
    /// Secteurs du disque (début de la zone de fin, « lead-out »).
    pub sectors: u32,
}

/// Image partagée par le lecteur ATAPI, MSCDEX et la lecture audio.
pub type SharedDisc = Arc<Disc>;

/// « mm:ss:ff » en trames (75 par seconde).
fn parse_msf(s: &str) -> Option<u32> {
    let mut it = s.split(':').map(|x| x.parse::<u32>().ok());
    let (m, sec, f) = (it.next()??, it.next()??, it.next()??);
    Some((m * 60 + sec) * 75 + f)
}

/// Fichier nommé dans une CUE, cherché aussi sans tenir compte de la casse
/// (CUE venues de Windows).
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let p = dir.join(name);
    if p.is_file() {
        return Some(p);
    }
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

impl Disc {
    /// Image ISO (secteurs de 2048 octets) ou feuille CUE.
    pub fn open(path: &Path) -> Result<Self, String> {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue")) {
            return Self::open_cue(path);
        }
        let err = |e: std::io::Error| format!("{} : {e}", path.display());
        let file = File::open(path).map_err(err)?;
        let size = file.metadata().map_err(err)?.len();
        if size < 17 * SECTOR as u64 {
            return Err(format!("{} : image trop petite pour un CD", path.display()));
        }
        let mut probe = [0u8; 6];
        file.read_exact_at(&mut probe, 16 * SECTOR as u64).map_err(err)?;
        if &probe[1..6] != b"CD001" {
            return Err(format!("{} : pas d'ISO 9660 (descripteur de volume absent)", path.display()));
        }
        let sectors = (size / SECTOR as u64) as u32;
        let track = Track { number: 1, start: 0, sectors, audio: false, file: 0, sector_size: SECTOR as u64, data_offset: 0, file_offset: 0 };
        Ok(Self { files: vec![file], tracks: vec![track], sectors })
    }

    /// Feuille CUE : FILE, TRACK (MODE1/2048, MODE1/2352, MODE2/2336,
    /// MODE2/2352, AUDIO), INDEX 01 (début de la piste ; INDEX 00 et les
    /// secteurs de prégap présents dans le fichier restent à la piste
    /// précédente), PREGAP (silence absent du fichier).
    fn open_cue(path: &Path) -> Result<Self, String> {
        let name = path.display();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{name} : {e}"))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        // Pistes lues : (n°, fichier, taille de secteur, décalage, audio,
        // INDEX 01 dans le fichier, prégap hors fichier).
        let mut files: Vec<(File, u64)> = Vec::new();
        let mut raw: Vec<(u8, usize, u64, u64, bool, Option<u32>, u32)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            let bad = || format!("{name}, ligne {} : « {line} »", n + 1);
            let (cmd, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            match cmd.to_ascii_uppercase().as_str() {
                "FILE" => {
                    let rest = rest.trim();
                    let fname = match rest.strip_prefix('"') {
                        Some(r) => r.split('"').next().unwrap_or(""),
                        None => rest.split_whitespace().next().unwrap_or(""),
                    };
                    let p = find_file(dir, fname).ok_or_else(|| format!("{name} : fichier « {fname} » introuvable"))?;
                    let f = File::open(&p).map_err(|e| format!("{} : {e}", p.display()))?;
                    let size = f.metadata().map_err(|e| format!("{} : {e}", p.display()))?.len();
                    files.push((f, size));
                }
                "TRACK" => {
                    let mut it = rest.split_whitespace();
                    let num: u8 = it.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
                    let (size, offset, audio) = match it.next().unwrap_or("").to_ascii_uppercase().as_str() {
                        "AUDIO" => (2352, 0, true),
                        "MODE1/2048" => (2048, 0, false),
                        "MODE1/2352" => (2352, 16, false),
                        "MODE2/2336" => (2336, 8, false),
                        "MODE2/2352" => (2352, 24, false),
                        _ => return Err(format!("{}, type de piste non pris en charge", bad())),
                    };
                    if files.is_empty() {
                        return Err(format!("{} : TRACK avant FILE", bad()));
                    }
                    raw.push((num, files.len() - 1, size, offset, audio, None, 0));
                }
                "INDEX" => {
                    let mut it = rest.split_whitespace();
                    let idx: u8 = it.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
                    let at = it.next().and_then(parse_msf).ok_or_else(bad)?;
                    let t = raw.last_mut().ok_or_else(bad)?;
                    if idx == 1 {
                        t.5 = Some(at);
                    }
                }
                "PREGAP" => {
                    let t = raw.last_mut().ok_or_else(bad)?;
                    t.6 = parse_msf(rest.trim()).ok_or_else(bad)?;
                }
                _ => {}
            }
        }
        if raw.is_empty() {
            return Err(format!("{name} : aucune piste"));
        }
        // Positions absolues : chaque fichier suit le précédent, les
        // prégaps hors fichier décalent tout ce qui suit.
        let mut tracks: Vec<Track> = Vec::new();
        let mut file_base = 0u32;
        let mut gaps = 0u32;
        for (i, &(number, file, size, offset, audio, index1, pregap)) in raw.iter().enumerate() {
            if i > 0 && raw[i - 1].1 != file {
                let (prev_file, prev_size) = (raw[i - 1].1, raw[i - 1].2);
                file_base += (files[prev_file].1 / prev_size) as u32;
            }
            gaps += pregap;
            let index1 = index1.ok_or_else(|| format!("{name} : piste {number} sans INDEX 01"))?;
            let file_sectors = (files[file].1 / size) as u32;
            let end_in_file = raw.get(i + 1).filter(|n| n.1 == file).and_then(|n| n.5).unwrap_or(file_sectors);
            tracks.push(Track {
                number,
                start: file_base + gaps + index1,
                sectors: end_in_file.saturating_sub(index1),
                audio,
                file,
                sector_size: size,
                data_offset: offset,
                file_offset: u64::from(index1) * size,
            });
        }
        let last = tracks.last().map_or(0, |t| t.start + t.sectors);
        Ok(Self { files: files.into_iter().map(|(f, _)| f).collect(), tracks, sectors: last })
    }

    fn track_at(&self, lba: u32) -> Option<&Track> {
        self.tracks.iter().find(|t| (t.start..t.start + t.sectors).contains(&lba))
    }

    /// Piste contenant le secteur (ou la dernière avant lui, dans un prégap).
    pub fn track_of(&self, lba: u32) -> Option<&Track> {
        self.tracks.iter().rev().find(|t| t.start <= lba).or(self.tracks.first())
    }

    /// Données utiles (2048 octets) d'un secteur d'une piste de données.
    pub fn read_data(&self, lba: u32, out: &mut [u8]) -> Result<(), Sense> {
        let t = self.track_at(lba).ok_or(Sense::LBA_OUT_OF_RANGE)?;
        if t.audio {
            return Err(Sense::ILLEGAL_MODE);
        }
        let at = t.file_offset + u64::from(lba - t.start) * t.sector_size + t.data_offset;
        self.files[t.file].read_exact_at(&mut out[..SECTOR], at).map_err(|_| Sense::READ_ERROR)
    }

    /// Secteur brut : tel quel pour une piste de 2352 octets, sinon
    /// reconstruit (synchronisation, en-tête mode 1, EDC/ECC à zéro).
    /// Hors piste (prégap) : silence.
    pub fn read_raw(&self, lba: u32, out: &mut [u8]) -> Result<(), Sense> {
        if lba >= self.sectors {
            return Err(Sense::LBA_OUT_OF_RANGE);
        }
        let Some(t) = self.track_at(lba) else {
            out[..RAW_SECTOR].fill(0);
            return Ok(());
        };
        if t.sector_size == RAW_SECTOR as u64 {
            let at = t.file_offset + u64::from(lba - t.start) * t.sector_size;
            return self.files[t.file].read_exact_at(&mut out[..RAW_SECTOR], at).map_err(|_| Sense::READ_ERROR);
        }
        out[..RAW_SECTOR].fill(0);
        out[1..11].fill(0xFF);
        let (m, s, f) = msf(lba + MSF_OFFSET);
        out[12..16].copy_from_slice(&[bcd(m), bcd(s), bcd(f), 1]);
        self.read_data(lba, &mut out[16..16 + SECTOR])
    }
}

/// État de la lecture audio (codes de READ SUB-CHANNEL).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioState {
    /// Aucune lecture demandée (15h).
    Idle,
    Playing,
    Paused,
    /// Lecture menée à son terme (13h).
    Completed,
}

/// Lecture des pistes audio, rendue au mixeur.
pub struct Player {
    pub disc: Option<SharedDisc>,
    pub state: AudioState,
    /// Secteur en cours, fin (exclue) et début de la dernière lecture.
    pub pos: u32,
    pub end: u32,
    pub start: u32,
    /// Trames du secteur en cours, position dans ce secteur.
    frames: Vec<[f32; 2]>,
    idx: usize,
    /// Rééchantillonnage : trames source encadrantes, phase.
    prev: [f32; 2],
    cur: [f32; 2],
    phase: f64,
}

pub type SharedPlayer = Rc<RefCell<Player>>;

const CD_RATE: f64 = 44_100.0;

impl Player {
    pub fn new(disc: Option<SharedDisc>) -> Self {
        Self { disc, state: AudioState::Idle, pos: 0, end: 0, start: 0, frames: Vec::new(), idx: 0, prev: [0.0; 2], cur: [0.0; 2], phase: 1.0 }
    }

    pub fn shared(self) -> SharedPlayer {
        Rc::new(RefCell::new(self))
    }

    /// Lecture de `start` à `end` (exclu) ; la piste de départ doit être audio.
    pub fn play(&mut self, start: u32, end: u32) -> Result<(), Sense> {
        let d = self.disc.as_ref().ok_or(Sense::NOT_READY_NO_MEDIUM)?;
        if start >= d.sectors {
            return Err(Sense::LBA_OUT_OF_RANGE);
        }
        // Fin au-delà du disque : bornée (indulgence pour les jeux qui
        // comptent mal la dernière piste), au lieu d'une erreur.
        let end = end.min(d.sectors);
        if d.track_of(start).is_some_and(|t| !t.audio && start >= t.start) {
            return Err(Sense::ILLEGAL_MODE);
        }
        if end <= start {
            self.stop();
            return Ok(());
        }
        (self.start, self.pos, self.end) = (start, start, end);
        self.frames.clear();
        self.idx = 0;
        self.state = AudioState::Playing;
        Ok(())
    }

    pub fn pause(&mut self) {
        if self.state == AudioState::Playing {
            self.state = AudioState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == AudioState::Paused {
            self.state = AudioState::Playing;
        }
    }

    pub fn stop(&mut self) {
        self.state = AudioState::Idle;
        self.frames.clear();
    }

    /// Trame source suivante ; `None` en fin de lecture.
    fn next_frame(&mut self) -> Option<[f32; 2]> {
        if self.idx >= self.frames.len() {
            if self.pos >= self.end {
                self.state = AudioState::Completed;
                return None;
            }
            let mut raw = vec![0u8; RAW_SECTOR];
            let d = self.disc.as_ref()?;
            // Secteur de données au milieu d'une lecture : silence.
            if d.track_of(self.pos).is_some_and(|t| t.audio) {
                d.read_raw(self.pos, &mut raw).ok()?;
            } else {
                raw.fill(0);
            }
            self.frames = raw
                .chunks_exact(4)
                .map(|c| [f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0, f32::from(i16::from_le_bytes([c[2], c[3]])) / 32768.0])
                .collect();
            self.idx = 0;
            self.pos += 1;
        }
        let f = self.frames[self.idx];
        self.idx += 1;
        Some(f)
    }

    /// Ajoute la musique à `out` (trames stéréo entrelacées à `rate` Hz).
    pub fn render(&mut self, rate: u32, out: &mut [f32]) {
        if self.state != AudioState::Playing {
            return;
        }
        let step = CD_RATE / f64::from(rate);
        for frame in out.chunks_exact_mut(2) {
            while self.phase >= 1.0 {
                self.prev = self.cur;
                match self.next_frame() {
                    Some(f) => self.cur = f,
                    None => return,
                }
                self.phase -= 1.0;
            }
            let t = self.phase as f32;
            frame[0] += self.prev[0] + (self.cur[0] - self.prev[0]) * t;
            frame[1] += self.prev[1] + (self.cur[1] - self.prev[1]) * t;
            self.phase += step;
        }
    }

    /// Avance de `frames` trames de sortie sans les rendre (retard du
    /// mixeur rattrapé : la musique continue pendant le temps sauté).
    pub fn skip(&mut self, frames: u64, rate: u32) {
        if self.state != AudioState::Playing {
            return;
        }
        let src = (frames as f64 * CD_RATE / f64::from(rate)) as u64;
        let in_sector = (self.frames.len() - self.idx.min(self.frames.len())) as u64;
        if src < in_sector {
            self.idx += src as usize;
            return;
        }
        let sectors = ((src - in_sector) / (RAW_SECTOR as u64 / 4)) as u32;
        self.frames.clear();
        self.idx = 0;
        self.pos = self.pos.saturating_add(sectors).min(self.end);
        if self.pos >= self.end {
            self.state = AudioState::Completed;
        }
    }

    /// Secteur joué, pour le sous-canal (le début avant toute lecture).
    pub fn position(&self) -> u32 {
        match self.state {
            AudioState::Playing | AudioState::Paused | AudioState::Completed => self.pos.saturating_sub(1).max(self.start),
            AudioState::Idle => 0,
        }
    }
}

fn msf(frames: u32) -> (u8, u8, u8) {
    ((frames / 75 / 60) as u8, (frames / 75 % 60) as u8, (frames % 75) as u8)
}

fn bcd(v: u8) -> u8 {
    (v / 10) << 4 | (v % 10)
}

/// Adresse sur 4 octets, en LBA ou en MSF (0, M, S, F).
fn address(lba: u32, use_msf: bool) -> [u8; 4] {
    if use_msf {
        let (m, s, f) = msf(lba + MSF_OFFSET);
        [0, m, s, f]
    } else {
        lba.to_be_bytes()
    }
}

/// Erreur SCSI : clé, code et qualificatif de détail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sense {
    pub key: u8,
    pub asc: u8,
    pub ascq: u8,
}

impl Sense {
    pub const NONE: Sense = Sense { key: 0, asc: 0, ascq: 0 };
    pub const NOT_READY_NO_MEDIUM: Sense = Sense { key: 2, asc: 0x3A, ascq: 0 };
    pub const READ_ERROR: Sense = Sense { key: 3, asc: 0x11, ascq: 0 };
    pub const INVALID_COMMAND: Sense = Sense { key: 5, asc: 0x20, ascq: 0 };
    pub const LBA_OUT_OF_RANGE: Sense = Sense { key: 5, asc: 0x21, ascq: 0 };
    pub const INVALID_FIELD: Sense = Sense { key: 5, asc: 0x24, ascq: 0 };
    pub const ILLEGAL_MODE: Sense = Sense { key: 5, asc: 0x64, ascq: 0 };
    pub const MEDIUM_CHANGED: Sense = Sense { key: 6, asc: 0x28, ascq: 0 };
}

/// Suite d'un paquet : données à rendre à l'hôte, données attendues de
/// l'hôte (MODE SELECT), ou rien.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Done,
    DataIn(Vec<u8>),
    /// Octets attendus de l'hôte (ignorés).
    DataOut(usize),
}

/// Lecteur : disque inséré, erreur en attente (REQUEST SENSE).
pub struct Drive {
    pub disc: Option<SharedDisc>,
    pub player: SharedPlayer,
    sense: Sense,
    /// Disque changé : signalé une fois (UNIT ATTENTION).
    changed: bool,
    pub sectors_read: u64,
}

const VENDOR: &[u8; 8] = b"BARE86  ";
const PRODUCT: &[u8; 16] = b"CD-ROM          ";
const REVISION: &[u8; 4] = b"1.0 ";

impl Drive {
    pub fn new(disc: Option<SharedDisc>, player: SharedPlayer) -> Self {
        Self { disc, player, sense: Sense::NONE, changed: false, sectors_read: 0 }
    }

    /// Change de disque (ou l'éjecte) : le pilote le saura au prochain paquet.
    #[allow(dead_code)] // changement de disque à chaud, à venir
    pub fn insert(&mut self, disc: Option<SharedDisc>) {
        let mut p = self.player.borrow_mut();
        p.stop();
        p.disc = disc.clone();
        drop(p);
        self.disc = disc;
        self.changed = true;
    }

    pub fn model() -> &'static str {
        "BARE86 CD-ROM"
    }

    /// Exécute un paquet ; une erreur est gardée pour REQUEST SENSE.
    pub fn command(&mut self, cmd: &[u8; 12]) -> Result<Reply, Sense> {
        let r = self.execute(cmd);
        self.sense = match &r {
            Ok(_) => Sense::NONE,
            Err(s) => *s,
        };
        r
    }

    fn disc(&self) -> Result<&Disc, Sense> {
        self.disc.as_deref().ok_or(Sense::NOT_READY_NO_MEDIUM)
    }

    fn execute(&mut self, c: &[u8; 12]) -> Result<Reply, Sense> {
        let be16 = |i: usize| usize::from(u16::from_be_bytes([c[i], c[i + 1]]));
        let be32 = |i: usize| u32::from_be_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
        let op = c[0];
        // Commandes servies même avec un changement de disque en attente.
        if !matches!(op, 0x03 | 0x12 | 0x4A) && std::mem::take(&mut self.changed) {
            return Err(Sense::MEDIUM_CHANGED);
        }
        let reply = |mut v: Vec<u8>, alloc: usize| {
            v.truncate(alloc);
            Ok(if v.is_empty() { Reply::Done } else { Reply::DataIn(v) })
        };
        match op {
            // TEST UNIT READY, REZERO, SEEK(10), SET CD SPEED, START STOP,
            // PREVENT ALLOW MEDIUM REMOVAL.
            0x00 | 0x01 | 0x2B | 0xBB => self.disc().map(|_| Reply::Done),
            // START STOP UNIT : l'éjection (LoEj sans Start) arrête la musique.
            0x1B => {
                if c[4] & 3 == 2 {
                    self.player.borrow_mut().stop();
                }
                Ok(Reply::Done)
            }
            0x1E => Ok(Reply::Done),
            0x03 => {
                let s = std::mem::replace(&mut self.sense, Sense::NONE);
                let mut v = vec![0u8; 18];
                v[0] = 0x70;
                v[2] = s.key;
                v[7] = 10;
                v[12] = s.asc;
                v[13] = s.ascq;
                reply(v, usize::from(c[4]))
            }
            0x12 => {
                let mut v = vec![0x05, 0x80, 0x00, 0x21, 31, 0, 0, 0];
                v.extend_from_slice(VENDOR);
                v.extend_from_slice(PRODUCT);
                v.extend_from_slice(REVISION);
                reply(v, usize::from(c[4]))
            }
            0x1A | 0x5A => {
                let ten = op == 0x5A;
                let page = c[2] & 0x3F;
                let alloc = if ten { be16(7) } else { usize::from(c[4]) };
                let mut pages = Vec::new();
                if matches!(page, 0x01 | 0x3F) {
                    pages.extend_from_slice(&[0x01, 0x06, 0, 5, 0, 0, 0, 0]);
                }
                if matches!(page, 0x0E | 0x3F) {
                    // Contrôle audio : volumes des quatre sorties au maximum.
                    pages.extend_from_slice(&[0x0E, 0x0E, 0x04, 0, 0, 0, 0, 0x4B, 1, 0xFF, 2, 0xFF, 0, 0, 0, 0]);
                }
                if matches!(page, 0x2A | 0x3F) {
                    // Capacités : lecture CD, audio, verrouillage, éjection ;
                    // 24× (4 224 Ko/s).
                    pages.extend_from_slice(&[0x2A, 0x12, 0x00, 0x00, 0x71, 0x7F, 0x29, 0x03, 0x10, 0x80, 0x01, 0x00, 0x00, 0x00, 0x10, 0x80, 0, 0, 0, 0]);
                }
                if pages.is_empty() {
                    return Err(Sense::INVALID_FIELD);
                }
                let medium = if self.disc.is_some() { 0x01 } else { 0x70 };
                let mut v = if ten {
                    let len = (pages.len() + 6) as u16;
                    let [h, l] = len.to_be_bytes();
                    vec![h, l, medium, 0, 0, 0, 0, 0]
                } else {
                    vec![(pages.len() + 3) as u8, medium, 0, 0]
                };
                v.extend_from_slice(&pages);
                reply(v, alloc)
            }
            0x15 | 0x55 => {
                let len = if op == 0x55 { be16(7) } else { usize::from(c[4]) };
                Ok(if len == 0 { Reply::Done } else { Reply::DataOut(len) })
            }
            0x25 => {
                let d = self.disc()?;
                let mut v = (d.sectors - 1).to_be_bytes().to_vec();
                v.extend_from_slice(&(SECTOR as u32).to_be_bytes());
                Ok(Reply::DataIn(v))
            }
            0x28 | 0xA8 => {
                let lba = be32(2);
                let count = if op == 0x28 { be16(7) as u32 } else { be32(6) };
                self.read_sectors(lba, count, false)
            }
            0xBE => {
                let lba = be32(2);
                let count = u32::from_be_bytes([0, c[6], c[7], c[8]]);
                // Données utiles seules (10h) ou secteur complet (F8h).
                match c[9] & 0xF8 {
                    0x10 => self.read_sectors(lba, count, false),
                    0xF8 => self.read_sectors(lba, count, true),
                    0x00 => Ok(Reply::Done),
                    _ => Err(Sense::INVALID_FIELD),
                }
            }
            0x43 => {
                let d = self.disc()?;
                let use_msf = c[1] & 2 != 0;
                // Format dans l'octet 2, ou à l'ancienne (SFF-8020) dans l'octet 9.
                let format = if c[2] & 0x0F != 0 { c[2] & 0x0F } else { c[9] >> 6 };
                let v = match format {
                    0 => toc(d, use_msf, c[6]).ok_or(Sense::INVALID_FIELD)?,
                    1 => {
                        let first = &d.tracks[0];
                        let mut v = vec![0, 10, 1, 1, 0, control(first), first.number, 0];
                        v.extend_from_slice(&address(first.start, use_msf));
                        v
                    }
                    _ => return Err(Sense::INVALID_FIELD),
                };
                reply(v, be16(7))
            }
            0x42 => {
                let d = self.disc()?;
                let p = self.player.borrow();
                let use_msf = c[1] & 2 != 0;
                let status = match p.state {
                    AudioState::Playing => 0x11,
                    AudioState::Paused => 0x12,
                    AudioState::Completed => 0x13,
                    AudioState::Idle => 0x15,
                };
                let mut v = vec![0, status, 0, 0];
                // Format 1 : position courante (piste, index, adresses
                // absolue et relative à la piste).
                if c[2] & 0x40 != 0 && c[3] <= 1 {
                    let pos = p.position();
                    let t = d.track_of(pos).ok_or(Sense::INVALID_FIELD)?;
                    v[3] = 12;
                    v.extend_from_slice(&[1, control(t), t.number, 1]);
                    v.extend_from_slice(&address(pos, use_msf));
                    let rel = pos.saturating_sub(t.start);
                    v.extend_from_slice(&if use_msf { address(rel.wrapping_sub(MSF_OFFSET), true) } else { rel.to_be_bytes() });
                }
                // Lue une fois, la fin de lecture redevient « aucun état ».
                drop(p);
                let mut p = self.player.borrow_mut();
                if p.state == AudioState::Completed {
                    p.state = AudioState::Idle;
                }
                reply(v, be16(7))
            }
            0x46 => {
                // GET CONFIGURATION : profil courant CD-ROM (0008h).
                let profile = if self.disc.is_some() { 8 } else { 0 };
                reply(vec![0, 0, 0, 4, 0, 0, 0, profile], be16(7))
            }
            0x4A => {
                // Événements du support : rien de neuf, disque présent ou non.
                let present = if self.disc.is_some() { 2 } else { 0 };
                reply(vec![0, 6, 4, 0x10, 0, present, 0, 0], be16(7))
            }
            0xBD => reply(vec![0, 0, 0, 0, 0, 1, 0, 0], be16(8)),
            // PLAY AUDIO (10), (12) : début, nombre de secteurs ; début
            // FFFFFFFFh : position courante.
            0x45 | 0xA5 => {
                self.disc()?;
                let len = if op == 0x45 { be16(7) as u32 } else { be32(6) };
                let mut p = self.player.borrow_mut();
                let start = match be32(2) {
                    0xFFFF_FFFF => p.position(),
                    lba => lba,
                };
                p.play(start, start.saturating_add(len))?;
                Ok(Reply::Done)
            }
            // PLAY AUDIO MSF : début et fin en minutes, secondes, trames.
            0x47 => {
                self.disc()?;
                let at = |i: usize| ((u32::from(c[i]) * 60 + u32::from(c[i + 1])) * 75 + u32::from(c[i + 2])).saturating_sub(MSF_OFFSET);
                let mut p = self.player.borrow_mut();
                let start = if c[3..6] == [0xFF; 3] { p.position() } else { at(3) };
                p.play(start, at(6))?;
                Ok(Reply::Done)
            }
            // PAUSE/RESUME : bit 0 de l'octet 8.
            0x4B => {
                self.disc()?;
                let mut p = self.player.borrow_mut();
                if c[8] & 1 != 0 { p.resume() } else { p.pause() }
                Ok(Reply::Done)
            }
            0x4E => {
                self.player.borrow_mut().stop();
                Ok(Reply::Done)
            }
            _ => Err(Sense::INVALID_COMMAND),
        }
    }

    fn read_sectors(&mut self, lba: u32, count: u32, raw: bool) -> Result<Reply, Sense> {
        let d = self.disc()?;
        if count == 0 {
            return Ok(Reply::Done);
        }
        if u64::from(lba) + u64::from(count) > u64::from(d.sectors) {
            return Err(Sense::LBA_OUT_OF_RANGE);
        }
        let size = if raw { RAW_SECTOR } else { SECTOR };
        let mut v = vec![0u8; size * count as usize];
        for (i, chunk) in v.chunks_mut(size).enumerate() {
            let lba = lba + i as u32;
            if raw { d.read_raw(lba, chunk)? } else { d.read_data(lba, chunk)? }
        }
        self.sectors_read += u64::from(count);
        Ok(Reply::DataIn(v))
    }
}

fn control(t: &Track) -> u8 {
    // ADR 1 (position), contrôle : 4 = piste de données, 0 = audio.
    if t.audio { 0x10 } else { 0x14 }
}

/// Table des matières, format 0 : pistes à partir de `from`, puis la zone
/// de fin (piste AAh).
fn toc(d: &Disc, use_msf: bool, from: u8) -> Option<Vec<u8>> {
    let last = d.tracks.last()?.number;
    if from > last && from != 0xAA {
        return None;
    }
    let mut v = vec![0, 0, d.tracks.first()?.number, last];
    for t in d.tracks.iter().filter(|t| t.number >= from && from != 0xAA) {
        v.extend_from_slice(&[0, control(t), t.number, 0]);
        v.extend_from_slice(&address(t.start, use_msf));
    }
    v.extend_from_slice(&[0, 0x14, 0xAA, 0]);
    v.extend_from_slice(&address(d.sectors, use_msf));
    let len = (v.len() - 2) as u16;
    v[..2].copy_from_slice(&len.to_be_bytes());
    Some(v)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// ISO minimale : descripteur de volume en secteur 16, `n` secteurs
    /// numérotés (premier octet = numéro de secteur).
    pub(crate) fn iso(n: u32) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let k = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("bare86-test-{}-{k}.iso", std::process::id()));
        let mut data = vec![0u8; n as usize * SECTOR];
        for (i, s) in data.chunks_mut(SECTOR).enumerate() {
            s[0] = i as u8;
        }
        data[16 * SECTOR + 1..16 * SECTOR + 6].copy_from_slice(b"CD001");
        std::fs::write(&path, data).unwrap();
        path
    }

    pub(crate) fn drive(disc: Option<Disc>) -> Drive {
        let disc = disc.map(Arc::new);
        Drive::new(disc.clone(), Player::new(disc).shared())
    }

    fn cmd(bytes: &[u8]) -> [u8; 12] {
        let mut c = [0u8; 12];
        c[..bytes.len()].copy_from_slice(bytes);
        c
    }

    #[test]
    fn read_capacity_toc_and_sectors() {
        let path = iso(40);
        let mut d = drive(Some(Disc::open(&path).unwrap()));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(d.command(&cmd(&[0x00])), Ok(Reply::Done));
        assert_eq!(d.command(&cmd(&[0x25])), Ok(Reply::DataIn(vec![0, 0, 0, 39, 0, 0, 8, 0])));
        let Ok(Reply::DataIn(v)) = d.command(&cmd(&[0x28, 0, 0, 0, 0, 20, 0, 0, 2])) else { panic!() };
        assert_eq!((v.len(), v[0], v[SECTOR]), (2 * SECTOR, 20, 21));
        assert_eq!(d.command(&cmd(&[0x28, 0, 0, 0, 0, 39, 0, 0, 2])), Err(Sense::LBA_OUT_OF_RANGE));
        let Ok(Reply::DataIn(s)) = d.command(&cmd(&[0x03, 0, 0, 0, 18])) else { panic!() };
        assert_eq!((s[2], s[12]), (5, 0x21));
        // TOC en MSF : piste 1 en 00:02:00, fin en 00:02:40.
        let Ok(Reply::DataIn(t)) = d.command(&cmd(&[0x43, 2, 0, 0, 0, 0, 0, 0, 100])) else { panic!() };
        assert_eq!(t, [0, 18, 1, 1, 0, 0x14, 1, 0, 0, 0, 2, 0, 0, 0x14, 0xAA, 0, 0, 0, 2, 40]);
        // Secteur brut : synchronisation, en-tête BCD, données.
        let Ok(Reply::DataIn(r)) = d.command(&cmd(&[0xBE, 0, 0, 0, 0, 3, 0, 0, 1, 0xF8])) else { panic!() };
        assert_eq!((&r[..2], &r[12..17]), (&[0x00, 0xFF][..], &[0x00, 0x02, 0x03, 0x01, 3][..]));
    }

    #[test]
    fn no_disc_and_medium_change() {
        let mut d = drive(None);
        assert_eq!(d.command(&cmd(&[0x00])), Err(Sense::NOT_READY_NO_MEDIUM));
        let path = iso(20);
        d.insert(Some(Arc::new(Disc::open(&path).unwrap())));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(d.command(&cmd(&[0x00])), Err(Sense::MEDIUM_CHANGED));
        assert_eq!(d.command(&cmd(&[0x00])), Ok(Reply::Done));
    }

    /// CUE/BIN : piste de données MODE1/2352 (« data.bin »), puis deux
    /// pistes audio dans « audio.bin » (nommé « AUDIO.BIN » dans la CUE) :
    /// piste 2 après un PREGAP de 2 s hors fichier, 100 secteurs à
    /// (8000, −8000) puis 10 de silence (INDEX 00 de la piste 3), piste 3 :
    /// 50 secteurs à 16000.
    fn cue_image() -> (std::path::PathBuf, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let k = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bare86-cue-{}-{k}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut data = vec![0u8; 40 * RAW_SECTOR];
        for (i, s) in data.chunks_mut(RAW_SECTOR).enumerate() {
            s[16] = i as u8;
        }
        data[16 * RAW_SECTOR + 17..16 * RAW_SECTOR + 22].copy_from_slice(b"CD001");
        std::fs::write(dir.join("data.bin"), data).unwrap();
        let frames = |n: usize, l: i16, r: i16| -> Vec<u8> {
            std::iter::repeat_n([l.to_le_bytes(), r.to_le_bytes()].concat(), n * 588).flatten().collect()
        };
        let mut audio = frames(100, 8000, -8000);
        audio.extend(frames(10, 0, 0));
        audio.extend(frames(50, 16000, 16000));
        std::fs::write(dir.join("audio.bin"), audio).unwrap();
        let cue = "FILE \"data.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n\
                   FILE \"AUDIO.BIN\" BINARY\n  TRACK 02 AUDIO\n    PREGAP 00:02:00\n    INDEX 01 00:00:00\n\
                   \x20 TRACK 03 AUDIO\n    INDEX 00 00:01:25\n    INDEX 01 00:01:35\n";
        std::fs::write(dir.join("disc.cue"), cue).unwrap();
        (dir.join("disc.cue"), dir)
    }

    #[test]
    fn cue_tracks_and_audio() {
        let (cue, dir) = cue_image();
        let disc = Disc::open(&cue).unwrap();
        let t: Vec<(u8, u32, u32, bool)> = disc.tracks.iter().map(|t| (t.number, t.start, t.sectors, t.audio)).collect();
        assert_eq!(t, [(1, 0, 40, false), (2, 190, 110, true), (3, 300, 50, true)]);
        assert_eq!(disc.sectors, 350);
        let mut d = drive(Some(disc));
        std::fs::remove_dir_all(&dir).unwrap();
        let Ok(Reply::DataIn(v)) = d.command(&cmd(&[0x28, 0, 0, 0, 0, 7, 0, 0, 1])) else { panic!() };
        assert_eq!(v[0], 7);
        assert_eq!(d.command(&cmd(&[0x28, 0, 0, 0, 0, 200, 0, 0, 1])), Err(Sense::ILLEGAL_MODE));
        // TOC : trois pistes, contrôle audio 10h, fin en 350.
        let Ok(Reply::DataIn(toc)) = d.command(&cmd(&[0x43, 0, 0, 0, 0, 0, 0, 0, 100])) else { panic!() };
        assert_eq!((toc[3], toc[13], &toc[16..20], &toc[32..36]), (3, 0x10, &[0, 0, 0, 190][..], &[0, 0, 1, 94][..]));
        // Lecture de la piste de données : refusée.
        assert_eq!(d.command(&cmd(&[0x45, 0, 0, 0, 0, 5, 0, 0, 10])), Err(Sense::ILLEGAL_MODE));
        // Piste 2, 5 secteurs (2940 trames à 44,1 kHz ≈ 3200 à 48 kHz).
        assert_eq!(d.command(&cmd(&[0x45, 0, 0, 0, 0, 190, 0, 0, 5])), Ok(Reply::Done));
        let mut out = vec![0f32; 2 * 1000];
        d.player.borrow_mut().render(48_000, &mut out);
        assert!((out[1000] - 8000.0 / 32768.0).abs() < 1e-3 && (out[1001] + 8000.0 / 32768.0).abs() < 1e-3);
        let sub = cmd(&[0x42, 0, 0x40, 1, 0, 0, 0, 0, 16]);
        let Ok(Reply::DataIn(q)) = d.command(&sub) else { panic!() };
        assert_eq!((q[1], q[6]), (0x11, 2));
        // Pause : rien n'est ajouté.
        d.command(&cmd(&[0x4B, 0, 0, 0, 0, 0, 0, 0, 0])).unwrap();
        let mut silent = vec![0f32; 200];
        d.player.borrow_mut().render(48_000, &mut silent);
        assert!(silent.iter().all(|&x| x == 0.0));
        let Ok(Reply::DataIn(q)) = d.command(&sub) else { panic!() };
        assert_eq!(q[1], 0x12);
        // Reprise jusqu'au bout : terminé (13h) une fois, puis aucun état.
        d.command(&cmd(&[0x4B, 0, 0, 0, 0, 0, 0, 0, 1])).unwrap();
        let mut rest = vec![0f32; 2 * 4000];
        d.player.borrow_mut().render(48_000, &mut rest);
        let Ok(Reply::DataIn(q)) = d.command(&sub) else { panic!() };
        assert_eq!(q[1], 0x13);
        let Ok(Reply::DataIn(q)) = d.command(&sub) else { panic!() };
        assert_eq!(q[1], 0x15);
        // Retard du mixeur rattrapé : 1 s sautée = 75 secteurs de plus.
        d.command(&cmd(&[0x45, 0, 0, 0, 0, 190, 0, 0, 100])).unwrap();
        d.player.borrow_mut().skip(48_000, 48_000);
        assert_eq!(d.player.borrow().pos, 265);
        // PLAY AUDIO MSF : piste 3 (00:06:00 = 450 − 150 = 300).
        d.command(&cmd(&[0x47, 0, 0, 0, 6, 0, 0, 6, 50])).unwrap();
        let mut out = vec![0f32; 200];
        d.player.borrow_mut().render(48_000, &mut out);
        assert!((out[100] - 16000.0 / 32768.0).abs() < 1e-3);
    }

}
