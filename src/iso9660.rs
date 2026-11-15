// SPDX-License-Identifier: GPL-3.0-or-later
//! Système de fichiers ISO 9660 (niveau 1, noms primaires 8.3, comme les lit
//! MSCDEX) : descripteur de volume primaire, répertoires, extensions.
//! Joliet et Rock Ridge sont ignorés, comme sous DOS.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cdrom::{SECTOR, SharedDisc};
#[cfg(test)]
use crate::cdrom::Disc;

#[derive(Clone, Debug)]
pub struct Entry {
    /// Nom DOS en majuscules (« FICHIER.TXT », « SOUS »), sans « ;1 ».
    pub name: String,
    pub dir: bool,
    pub lba: u32,
    pub len: u32,
    pub modified: SystemTime,
    pub hidden: bool,
}

pub struct IsoFs {
    pub disc: SharedDisc,
    root: Entry,
    /// Descripteur de volume primaire (secteur 16).
    pvd: Vec<u8>,
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Date d'un enregistrement de répertoire (7 octets : année − 1900, mois,
/// jour, heure, minute, seconde, décalage en quarts d'heure).
fn record_time(b: &[u8]) -> SystemTime {
    let (y, m, d) = (i64::from(b[0]) + 1900, i64::from(b[1]).clamp(1, 12), i64::from(b[2]).clamp(1, 31));
    // Jours depuis 1970 (algorithme de Howard Hinnant).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let offset = i64::from(b[6] as i8) * 15 * 60;
    let secs = days * 86_400 + i64::from(b[3]) * 3600 + i64::from(b[4]) * 60 + i64::from(b[5]) - offset;
    UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)
}

fn parse_record(r: &[u8]) -> Option<Entry> {
    let name_len = usize::from(*r.get(32)?);
    let raw = r.get(33..33 + name_len)?;
    // « . » et « .. » (00h, 01h) : pas des entrées pour DOS.
    if raw == [0] || raw == [1] {
        return None;
    }
    let mut name = String::from_utf8_lossy(raw).to_ascii_uppercase();
    if let Some(i) = name.find(';') {
        name.truncate(i);
    }
    if name.ends_with('.') {
        name.pop();
    }
    Some(Entry {
        name,
        dir: r[25] & 2 != 0,
        lba: le32(&r[2..6]),
        len: le32(&r[10..14]),
        modified: record_time(&r[18..25]),
        hidden: r[25] & 1 != 0,
    })
}

impl IsoFs {
    pub fn new(disc: SharedDisc) -> Result<Self, String> {
        let mut pvd = vec![0u8; SECTOR];
        // Descripteurs à partir du secteur 16 : le primaire est de type 1.
        let mut found = false;
        for lba in 16..32 {
            disc.read_data(lba, &mut pvd).map_err(|_| "ISO 9660: unreadable descriptor")?;
            if &pvd[1..6] != b"CD001" || pvd[0] == 0xFF {
                break;
            }
            if pvd[0] == 1 {
                found = true;
                break;
            }
        }
        if !found {
            return Err("ISO 9660: no primary volume descriptor".into());
        }
        let mut root = parse_record(&pvd[156..190]).unwrap_or(Entry {
            name: String::new(),
            dir: true,
            lba: le32(&pvd[158..162]),
            len: le32(&pvd[166..170]),
            modified: UNIX_EPOCH,
            hidden: false,
        });
        root.dir = true;
        Ok(Self { disc, root, pvd })
    }

    /// Champ texte du descripteur primaire, sans les espaces de fin.
    fn pvd_text(&self, at: usize, len: usize) -> String {
        String::from_utf8_lossy(&self.pvd[at..at + len]).trim_end().to_string()
    }

    pub fn volume_label(&self) -> String {
        self.pvd_text(40, 32)
    }

    /// Fichiers de copyright, de résumé et bibliographique (37 octets chacun).
    pub fn pvd_file_id(&self, which: usize) -> Vec<u8> {
        let at = [702, 739, 776][which];
        self.pvd[at..at + 37].to_vec()
    }

    #[cfg(test)]
    pub fn root(&self) -> &Entry {
        &self.root
    }

    /// Entrées d'un répertoire, dans l'ordre du disque.
    pub fn read_dir(&self, dir: &Entry) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut sector = vec![0u8; SECTOR];
        let sectors = dir.len.div_ceil(SECTOR as u32);
        for i in 0..sectors {
            if self.disc.read_data(dir.lba + i, &mut sector).is_err() {
                break;
            }
            let mut at = 0;
            // Un enregistrement ne franchit pas une limite de secteur : un
            // octet nul marque la fin des enregistrements de ce secteur.
            while at < SECTOR && sector[at] != 0 {
                let len = usize::from(sector[at]);
                if at + len > SECTOR || len < 34 {
                    break;
                }
                // Fichier en plusieurs extensions (bit 7) : la première seule.
                if let Some(e) = parse_record(&sector[at..at + len])
                    && !out.iter().any(|o: &Entry| o.name == e.name)
                {
                    out.push(e);
                }
                at += len;
            }
        }
        out
    }

    /// Entrée désignée par un chemin (composants en majuscules).
    pub fn lookup(&self, path: &[&str]) -> Option<Entry> {
        let mut cur = self.root.clone();
        for comp in path.iter().filter(|c| !c.is_empty()) {
            if !cur.dir {
                return None;
            }
            cur = self.read_dir(&cur).into_iter().find(|e| e.name.eq_ignore_ascii_case(comp))?;
        }
        Some(cur)
    }

    /// Lit à partir de `pos` dans le fichier ; rend le nombre d'octets lus.
    pub fn read(&self, file: &Entry, pos: u64, buf: &mut [u8]) -> usize {
        let len = u64::from(file.len);
        if pos >= len {
            return 0;
        }
        let want = buf.len().min((len - pos) as usize);
        let mut sector = vec![0u8; SECTOR];
        let mut done = 0;
        while done < want {
            let p = pos + done as u64;
            let lba = file.lba + (p / SECTOR as u64) as u32;
            let off = (p % SECTOR as u64) as usize;
            if self.disc.read_data(lba, &mut sector).is_err() {
                break;
            }
            let n = (SECTOR - off).min(want - done);
            buf[done..done + n].copy_from_slice(&sector[off..off + n]);
            done += n;
        }
        done
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn dates() {
        // 14 novembre 2026, 10:08:00 UTC+1.
        let t = record_time(&[126, 11, 14, 10, 8, 0, 4]);
        assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 1_794_647_280);
    }

    /// Image ISO fabriquée par xorriso à partir de fichiers (chemin
    /// relatif, contenu), étiquette `label` ; `None` si xorriso manque.
    /// Le répertoire rendu est à effacer par l'appelant.
    pub(crate) fn xorriso_iso(label: &str, files: &[(&str, &[u8])]) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let k = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bare86-iso-{}-{k}", std::process::id()));
        let src = dir.join("src");
        for (name, data) in files {
            let p = src.join(name);
            std::fs::create_dir_all(p.parent()?).ok()?;
            std::fs::write(p, data).ok()?;
        }
        let iso = dir.join("t.iso");
        let ok = std::process::Command::new("xorriso")
            .args(["-as", "mkisofs", "-quiet", "-V", label, "-o"])
            .arg(&iso)
            .arg(&src)
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("xorriso missing: test skipped");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        Some((iso, dir))
    }

    /// Répertoires et lecture d'une image fabriquée par xorriso.
    #[test]
    fn xorriso_image() {
        let big: Vec<u8> = (0..5000u32).map(|i| (i * 7) as u8).collect();
        let Some((iso, dir)) = xorriso_iso("ESSAI", &[("gros.bin", &big), ("sous/lisez.txt", b"profond"), ("README", b"x")]) else { return };
        let fs = IsoFs::new(std::sync::Arc::new(Disc::open(&iso).unwrap())).unwrap();
        assert_eq!(fs.volume_label(), "ESSAI");
        let mut names: Vec<String> = fs.read_dir(fs.root()).into_iter().map(|e| e.name).collect();
        names.sort();
        assert_eq!(names, ["GROS.BIN", "README", "SOUS"]);
        let f = fs.lookup(&["SOUS", "LISEZ.TXT"]).unwrap();
        let mut b = [0u8; 16];
        assert_eq!(fs.read(&f, 0, &mut b), 7);
        assert_eq!(&b[..7], b"profond");
        let g = fs.lookup(&["gros.bin"]).unwrap();
        let mut b = vec![0u8; 3000];
        assert_eq!(fs.read(&g, 2040, &mut b), 2960);
        assert_eq!(&b[..2960], &big[2040..]);
        assert!(fs.lookup(&["SOUS", "ABSENT"]).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
