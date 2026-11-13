// SPDX-License-Identifier: GPL-3.0-or-later
//! Fonctions fichier par FCB (héritées de CP/M) : ouverture, création,
//! lectures et écritures séquentielles ou aléatoires, recherche, suppression,
//! renommage. Un FCB ouvert renvoie à une entrée de la SFT, notée dans sa
//! zone réservée.
//!
//! FCB : lecteur (0 = courant), nom (8), extension (3), bloc courant (+0Ch),
//! taille d'enregistrement (+0Eh), taille du fichier (+10h), date (+14h),
//! heure (+16h), réservé (+18h), enregistrement courant (+20h), n°
//! d'enregistrement aléatoire (+21h). Un FCB étendu le précède de 7 octets :
//! FFh, 5 réservés, attributs.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::hle::{AX, CX, DS, DX, Regs, lin};
use crate::interp::Cpu;
use crate::mem::GuestMem;

use super::fs::{self, Target};
use super::handles::Kind;
use super::{Dos, Found};

/// Marque d'un FCB ouvert par nous, en +18h ; n° d'entrée SFT en +1Ah.
const OPEN_MAGIC: u16 = 0xFCB0;

struct Fcb {
    /// Adresse du FCB proprement dit (après l'en-tête étendu).
    at: usize,
    attr: u8,
}

fn fcb_at(mem: &GuestMem, cpu: &Cpu) -> Fcb {
    let at = lin(cpu.sreg(DS), cpu.w(DX));
    if mem.read_u8(at) == 0xFF { Fcb { at: at + 7, attr: mem.read_u8(at + 6) } } else { Fcb { at, attr: 0 } }
}

/// « X:NOM.EXT » d'après les champs du FCB (avec ses « ? » éventuels).
fn fcb_path(mem: &GuestMem, at: usize, current: u8) -> Vec<u8> {
    let mut raw = [0u8; 11];
    mem.read(at + 1, &mut raw);
    let drive = match mem.read_u8(at) {
        0 => current,
        d => d - 1,
    };
    let mut p = vec![b'A' + drive, b':'];
    p.extend(raw[..8].iter().copied().take_while(|&c| c != b' '));
    let ext: Vec<u8> = raw[8..].iter().copied().take_while(|&c| c != b' ').collect();
    if !ext.is_empty() {
        p.push(b'.');
        p.extend(ext);
    }
    p
}

impl Dos {
    fn fcb_file(&mut self, mem: &GuestMem, at: usize) -> Option<u8> {
        if mem.read_u16(at + 0x18) != OPEN_MAGIC {
            return None;
        }
        let idx = mem.read_u8(at + 0x1A);
        matches!(self.sft.get(idx).map(|e| &e.kind), Some(Kind::File(_))).then_some(idx)
    }

    fn record_size(mem: &GuestMem, at: usize) -> u64 {
        match mem.read_u16(at + 0x0E) {
            0 => 128,
            n => u64::from(n),
        }
    }

    /// Position séquentielle : bloc × 128 + enregistrement courant.
    fn seq_record(mem: &GuestMem, at: usize) -> u64 {
        u64::from(mem.read_u16(at + 0x0C)) * 128 + u64::from(mem.read_u8(at + 0x20))
    }

    fn set_seq_record(mem: &GuestMem, at: usize, rec: u64) {
        mem.write_u16(at + 0x0C, (rec / 128) as u16);
        mem.write_u8(at + 0x20, (rec % 128) as u8);
    }

    fn random_record(mem: &GuestMem, at: usize) -> u64 {
        let r = mem.read_u32(at + 0x21);
        u64::from(if Self::record_size(mem, at) >= 64 { r & 0x00FF_FFFF } else { r })
    }

    fn set_random_record(mem: &GuestMem, at: usize, rec: u64) {
        if Self::record_size(mem, at) >= 64 {
            let b = (rec as u32).to_le_bytes();
            mem.write(at + 0x21, &b[..3]);
        } else {
            mem.write_u32(at + 0x21, rec as u32);
        }
    }

    /// 0Fh ouvrir, 16h créer : AL = 0 ou FFh.
    fn fcb_open(&mut self, mem: &GuestMem, f: &Fcb, create: bool) -> bool {
        let path = fcb_path(mem, f.at, self.drives.current);
        let Ok(Target::Path { host, exists, .. }) = self.drives.resolve(&path) else { return false };
        if (!exists && !create) || host.is_dir() {
            return false;
        }
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true);
        if create {
            o.create(true).truncate(true);
        }
        let Ok(file) = o.open(&host).or_else(|_| std::fs::File::open(&host)) else { return false };
        let meta = file.metadata().ok();
        let Some(idx) = self.sft.insert(Kind::File(fs::OpenFile { file }), 2) else { return false };
        if mem.read_u8(f.at) == 0 {
            mem.write_u8(f.at, self.drives.current + 1);
        }
        mem.write_u16(f.at + 0x0C, 0);
        mem.write_u16(f.at + 0x0E, 128);
        let size = meta.as_ref().map_or(0, |m| m.len().min(u64::from(u32::MAX)) as u32);
        mem.write_u32(f.at + 0x10, size);
        let (time, date) =
            fs::dos_time(meta.and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH));
        mem.write_u16(f.at + 0x14, date);
        mem.write_u16(f.at + 0x16, time);
        mem.write_u16(f.at + 0x18, OPEN_MAGIC);
        mem.write_u8(f.at + 0x1A, idx);
        mem.write_u8(f.at + 0x20, 0);
        true
    }

    /// Lit ou écrit `count` enregistrements à partir de `rec`, via la DTA.
    /// Renvoie (AL, enregistrements traités) : 0 complet, 1 fin de fichier
    /// sans données (ou disque plein), 2 dépassement de segment, 3 partiel.
    fn fcb_transfer(&mut self, mem: &GuestMem, f: &Fcb, rec: u64, count: u64, write: bool) -> (u8, u64) {
        let Some(idx) = self.fcb_file(mem, f.at) else { return (1, 0) };
        let size = Self::record_size(mem, f.at);
        let total = (size * count) as usize;
        if usize::from(self.dta.1) + total > 0x1_0000 {
            return (2, 0);
        }
        let dta = lin(self.dta.0, self.dta.1);
        let Some(e) = self.sft.get(idx) else { return (1, 0) };
        let Kind::File(file) = &mut e.kind else { return (1, 0) };
        if file.file.seek(SeekFrom::Start(rec * size)).is_err() {
            return (1, 0);
        }
        if write {
            let mut buf = vec![0u8; total];
            mem.read(dta, &mut buf);
            if total == 0 {
                // Écriture de 0 enregistrement : fixe la taille du fichier.
                let _ = file.file.set_len(rec * size);
            } else if file.file.write_all(&buf).is_err() {
                return (1, 0);
            }
            let len = file.file.metadata().map_or(0, |m| m.len());
            mem.write_u32(f.at + 0x10, len.min(u64::from(u32::MAX)) as u32);
            return (0, count);
        }
        let mut buf = vec![0u8; total];
        let mut n = 0;
        while n < total {
            match file.file.read(&mut buf[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => n += k,
            }
        }
        if n == 0 {
            return (1, 0);
        }
        // Enregistrement incomplet complété de zéros.
        mem.write(dta, &buf);
        let done = (n as u64).div_ceil(size);
        (if n == total { 0 } else { 3 }, done)
    }

    fn fcb_find(&mut self, cpu: &mut Cpu, mem: &GuestMem, first: bool) -> u8 {
        let f = fcb_at(mem, cpu);
        let dta = lin(self.dta.0, self.dta.1);
        if first {
            let path = fcb_path(mem, f.at, self.drives.current);
            let Ok((dir, pattern)) = self.drives.search(&path) else { return 0xFF };
            let mut results: Vec<Found> = fs::list(&dir)
                .into_iter()
                .filter(|e| fs::fcb_matches(&pattern, &e.short))
                .filter(|e| !e.meta.is_dir() || f.attr & fs::ATTR_DIR != 0)
                .map(|e| {
                    let (time, date) = fs::dos_time(e.meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
                    let size = e.meta.len().min(u64::from(u32::MAX)) as u32;
                    Found { name: e.short.clone(), attr: fs::attributes(&e.meta), time, date, size }
                })
                .collect();
            if results.is_empty() {
                return 0xFF;
            }
            results.reverse();
            let id = self.new_search(results);
            // Notre n° de recherche, dans la zone réservée du FCB de l'appelant.
            mem.write_u16(f.at + 0x0C, id);
        }
        let id = mem.read_u16(f.at + 0x0C);
        let Some(found) = self.searches.iter_mut().find(|s| s.id == id).and_then(|s| s.results.pop()) else {
            return 0xFF;
        };
        // Résultat : un FCB non ouvert dans la DTA (étendu si la demande l'était).
        let mut out = dta;
        if f.at != lin(cpu.sreg(DS), cpu.w(DX)) {
            mem.write(dta, &[0xFF, 0, 0, 0, 0, 0, found.attr]);
            out += 7;
        }
        mem.fill(out, 0x25, 0);
        let drive = match mem.read_u8(f.at) {
            0 => self.drives.current + 1,
            d => d,
        };
        mem.write_u8(out, drive);
        mem.write(out + 1, &fs::fcb_pattern(&found.name));
        mem.write_u8(out + 0x0C, found.attr);
        mem.write_u16(out + 0x17, found.time);
        mem.write_u16(out + 0x19, found.date);
        mem.write_u32(out + 0x1D, found.size);
        0
    }

    /// Fonctions FCB de l'INT 21h ; `None` pour les autres.
    pub(super) fn int21_fcb(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Option<()> {
        let ah = cpu.hi(AX);
        let f = fcb_at(mem, cpu);
        let al = match ah {
            0x0F => {
                if self.fcb_open(mem, &f, false) {
                    0
                } else {
                    0xFF
                }
            }
            0x16 => {
                if self.fcb_open(mem, &f, true) {
                    0
                } else {
                    0xFF
                }
            }
            0x10 => match self.fcb_file(mem, f.at) {
                Some(idx) => {
                    self.sft.release(idx);
                    mem.write_u16(f.at + 0x18, 0);
                    0
                }
                None => 0xFF,
            },
            0x11 | 0x12 => self.fcb_find(cpu, mem, ah == 0x11),
            0x13 => {
                let path = fcb_path(mem, f.at, self.drives.current);
                match self.drives.search(&path) {
                    Ok((dir, pattern)) => {
                        let victims: Vec<_> = fs::list(&dir)
                            .into_iter()
                            .filter(|e| e.meta.is_file() && fs::fcb_matches(&pattern, &e.short))
                            .collect();
                        let deleted = victims.iter().filter(|e| std::fs::remove_file(&e.host).is_ok()).count();
                        if deleted > 0 { 0 } else { 0xFF }
                    }
                    Err(_) => 0xFF,
                }
            }
            0x14 | 0x15 => {
                let rec = Self::seq_record(mem, f.at);
                let (r, n) = self.fcb_transfer(mem, &f, rec, 1, ah == 0x15);
                Self::set_seq_record(mem, f.at, rec + n);
                r
            }
            // Renommer : nouveau nom en +11h du FCB.
            0x17 => {
                let from = fcb_path(mem, f.at, self.drives.current);
                let to = fcb_path(mem, f.at + 0x10, self.drives.current);
                let to = [&from[..2], &to[2..]].concat();
                match (self.drives.resolve(&from), self.drives.resolve(&to)) {
                    (Ok(Target::Path { host: a, exists: true, .. }), Ok(Target::Path { host: b, exists: false, .. }))
                        if std::fs::rename(&a, &b).is_ok() =>
                    {
                        0
                    }
                    _ => 0xFF,
                }
            }
            0x21 | 0x22 => {
                let rec = Self::random_record(mem, f.at);
                Self::set_seq_record(mem, f.at, rec);
                self.fcb_transfer(mem, &f, rec, 1, ah == 0x22).0
            }
            0x23 => {
                let path = fcb_path(mem, f.at, self.drives.current);
                match self.drives.resolve(&path) {
                    Ok(Target::Path { host, exists: true, .. }) => {
                        let len = std::fs::metadata(host).map_or(0, |m| m.len());
                        let size = Self::record_size(mem, f.at);
                        Self::set_random_record(mem, f.at, len.div_ceil(size));
                        0
                    }
                    _ => 0xFF,
                }
            }
            0x24 => {
                Self::set_random_record(mem, f.at, Self::seq_record(mem, f.at));
                return Some(());
            }
            0x27 | 0x28 => {
                let rec = Self::random_record(mem, f.at);
                let count = u64::from(cpu.w(CX));
                let (r, n) = self.fcb_transfer(mem, &f, rec, count, ah == 0x28);
                Self::set_random_record(mem, f.at, rec + n);
                Self::set_seq_record(mem, f.at, rec + n);
                cpu.set_w(CX, n as u16);
                r
            }
            _ => return None,
        };
        cpu.set_lo(AX, al);
        Some(())
    }
}
