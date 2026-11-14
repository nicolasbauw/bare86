// SPDX-License-Identifier: GPL-3.0-or-later
//! Disque compact : image (ISO 9660, secteurs de 2048 octets) et commandes
//! SCSI/MMC d'un lecteur ATAPI (paquets de 12 octets). Indépendant du
//! transport : le contrôleur IDE (`ide.rs`) achemine paquets et données.
//!
//! Pistes : une piste de données pour une ISO. La structure prévoit les
//! images CUE/BIN (secteurs bruts de 2352 octets, pistes audio).

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub const SECTOR: usize = 2048;
/// Secteur brut (synchronisation, en-tête, données, EDC/ECC).
pub const RAW_SECTOR: usize = 2352;
/// Décalage de l'adressage MSF : 2 s de pré-espace (150 trames).
const MSF_OFFSET: u32 = 150;

#[derive(Clone, Debug)]
pub struct Track {
    pub number: u8,
    pub start: u32,
    pub sectors: u32,
    pub audio: bool,
    /// Taille d'un secteur dans le fichier, et décalage des données utiles.
    sector_size: u64,
    data_offset: u64,
    file_offset: u64,
}

pub struct Disc {
    file: File,
    pub tracks: Vec<Track>,
    /// Secteurs du disque (début de la zone de fin, « lead-out »).
    pub sectors: u32,
}

impl Disc {
    pub fn open(path: &Path) -> Result<Self, String> {
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
        let track = Track { number: 1, start: 0, sectors, audio: false, sector_size: SECTOR as u64, data_offset: 0, file_offset: 0 };
        Ok(Self { file, tracks: vec![track], sectors })
    }

    fn track_at(&self, lba: u32) -> Option<&Track> {
        self.tracks.iter().find(|t| (t.start..t.start + t.sectors).contains(&lba))
    }

    /// Données utiles (2048 octets) d'un secteur d'une piste de données.
    pub fn read_data(&self, lba: u32, out: &mut [u8]) -> Result<(), Sense> {
        let t = self.track_at(lba).ok_or(Sense::LBA_OUT_OF_RANGE)?;
        if t.audio {
            return Err(Sense::ILLEGAL_MODE);
        }
        let at = t.file_offset + u64::from(lba - t.start) * t.sector_size + t.data_offset;
        self.file.read_exact_at(&mut out[..SECTOR], at).map_err(|_| Sense::READ_ERROR)
    }

    /// Secteur brut : reconstruit (synchronisation, en-tête mode 1, EDC/ECC à
    /// zéro) pour une piste de 2048 octets.
    pub fn read_raw(&self, lba: u32, out: &mut [u8]) -> Result<(), Sense> {
        let t = self.track_at(lba).ok_or(Sense::LBA_OUT_OF_RANGE)?;
        if t.sector_size == RAW_SECTOR as u64 {
            let at = t.file_offset + u64::from(lba - t.start) * t.sector_size;
            return self.file.read_exact_at(&mut out[..RAW_SECTOR], at).map_err(|_| Sense::READ_ERROR);
        }
        out[..RAW_SECTOR].fill(0);
        out[1..11].fill(0xFF);
        let (m, s, f) = msf(lba + MSF_OFFSET);
        out[12..16].copy_from_slice(&[bcd(m), bcd(s), bcd(f), 1]);
        self.read_data(lba, &mut out[16..16 + SECTOR])
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
    pub disc: Option<Disc>,
    sense: Sense,
    /// Disque changé : signalé une fois (UNIT ATTENTION).
    changed: bool,
    pub sectors_read: u64,
}

const VENDOR: &[u8; 8] = b"BARE86  ";
const PRODUCT: &[u8; 16] = b"CD-ROM          ";
const REVISION: &[u8; 4] = b"1.0 ";

impl Drive {
    pub fn new(disc: Option<Disc>) -> Self {
        Self { disc, sense: Sense::NONE, changed: false, sectors_read: 0 }
    }

    /// Change de disque (ou l'éjecte) : le pilote le saura au prochain paquet.
    #[allow(dead_code)] // changement de disque à chaud, à venir
    pub fn insert(&mut self, disc: Option<Disc>) {
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
        self.disc.as_ref().ok_or(Sense::NOT_READY_NO_MEDIUM)
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
            0x1B | 0x1E => Ok(Reply::Done),
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
                // Position courante : aucune lecture audio (état 15h).
                let t = &d.tracks[0];
                let mut v = vec![0, 0x15, 0, 0];
                if c[2] & 0x40 != 0 {
                    v[3] = 12;
                    v.extend_from_slice(&[1, control(t), t.number, 1]);
                    // Adresses absolue et relative.
                    v.extend_from_slice(&address(0, c[1] & 2 != 0));
                    v.extend_from_slice(&[0; 4]);
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
            // Lecture audio : pas de piste audio sur une ISO.
            0x45 | 0x47 | 0x4B | 0x4E | 0xA5 => {
                self.disc()?;
                Err(Sense::ILLEGAL_MODE)
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
    let mut v = vec![0, 0, d.tracks[0].number, last];
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

    fn cmd(bytes: &[u8]) -> [u8; 12] {
        let mut c = [0u8; 12];
        c[..bytes.len()].copy_from_slice(bytes);
        c
    }

    #[test]
    fn read_capacity_toc_and_sectors() {
        let path = iso(40);
        let mut d = Drive::new(Some(Disc::open(&path).unwrap()));
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
        let mut d = Drive::new(None);
        assert_eq!(d.command(&cmd(&[0x00])), Err(Sense::NOT_READY_NO_MEDIUM));
        let path = iso(20);
        d.insert(Some(Disc::open(&path).unwrap()));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(d.command(&cmd(&[0x00])), Err(Sense::MEDIUM_CHANGED));
        assert_eq!(d.command(&cmd(&[0x00])), Ok(Reply::Done));
    }
}
