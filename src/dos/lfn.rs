// SPDX-License-Identifier: GPL-3.0-or-later
//! Noms longs : l'API INT 21h AX=71xxh de Windows 95 (utilisée par les
//! programmes Win32 de HX DOS Extender, les outils DJGPP, etc.). Le nom long
//! d'un fichier est son nom sur l'hôte ; son nom court, celui que voit déjà
//! le DOS (`fs::list`). Une fonction absente rend AX=7100h et CF, comme un
//! DOS sans noms longs.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use super::fs::{self, Target};
use super::handles::{self, Kind};
use super::{Dos, ERR_FILE_EXISTS, ERR_INVALID_FUNCTION, Out, io_err, read_asciiz, vfs};
use crate::hle::{AX, BX, CX, DI, DS, DX, ES, Regs, SI, lin};
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Recherches à noms longs ouvertes en même temps, au plus.
const MAX_SEARCHES: usize = 32;
/// Système de fichiers annoncé (AX=71A0h) : noms longs, casse conservée.
const VOLUME_FLAGS: u16 = 0x4002;
const MAX_NAME: u16 = 255;
const MAX_PATH: u16 = 260;
/// FILETIME : centaines de nanosecondes depuis le 1er janvier 1601.
const FILETIME_UNIX: u64 = 11_644_473_600;

/// Résultat d'une recherche à noms longs.
pub struct Found {
    long: Vec<u8>,
    short: Vec<u8>,
    attr: u8,
    size: u64,
    modified: SystemTime,
}

fn filetime(t: SystemTime) -> u64 {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    (d.as_secs() + FILETIME_UNIX) * 10_000_000 + u64::from(d.subsec_nanos()) / 100
}

fn from_filetime(ft: u64) -> SystemTime {
    let secs = (ft / 10_000_000).saturating_sub(FILETIME_UNIX);
    SystemTime::UNIX_EPOCH + Duration::new(secs, (ft % 10_000_000) as u32 * 100)
}

/// Heure d'un enregistrement : FILETIME (format 0), ou heure et date DOS
/// dans le mot faible et le mot fort du double mot faible (format 1).
fn write_time(mem: &GuestMem, at: usize, t: SystemTime, dos_format: bool) {
    if dos_format {
        let (time, date) = fs::dos_time(t);
        mem.write_u32(at, u32::from(time) | u32::from(date) << 16);
        mem.write_u32(at + 4, 0);
    } else {
        let ft = filetime(t);
        mem.write_u32(at, ft as u32);
        mem.write_u32(at + 4, (ft >> 32) as u32);
    }
}

/// Nom court 8.3 proposé pour un nom long (AX=71A8h), sans tenir compte des
/// fichiers existants : « LONGFI~1.TXT ».
fn short_alias(long: &[u8]) -> Vec<u8> {
    let up = long.to_ascii_uppercase();
    let clean = |s: &[u8]| s.iter().copied().filter(|&c| c.is_ascii_alphanumeric() || b"!#$%&'()-@^_`{}~".contains(&c)).collect::<Vec<u8>>();
    let (stem, ext) = match up.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 => (clean(&up[..i]), clean(&up[i + 1..])),
        _ => (clean(&up), Vec::new()),
    };
    // Déjà un nom 8.3 : aucun caractère retiré, une partie de 8 et une de 3 au plus.
    let kept = stem.len() + ext.len() + usize::from(!ext.is_empty());
    let mut out = if !stem.is_empty() && stem.len() <= 8 && ext.len() <= 3 && kept == up.len() {
        stem
    } else {
        let mut s = stem[..stem.len().min(6)].to_vec();
        s.extend_from_slice(b"~1");
        s
    };
    if !ext.is_empty() {
        out.push(b'.');
        out.extend_from_slice(&ext[..ext.len().min(3)]);
    }
    out
}

impl Dos {
    pub(super) fn lfn(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let ds_dx = || read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
        let path_target = |dos: &Dos, path: &[u8]| -> Result<(std::path::PathBuf, bool), u16> {
            match dos.drives.resolve_long(path)? {
                Target::Path { host, exists, .. } => Ok((host, exists)),
                Target::Device(_) => Err(fs::ERR_ACCESS_DENIED),
            }
        };
        match cpu.lo(AX) {
            // Vidage des tampons d'un lecteur : rien en attente.
            0x0D => Out::Ok,
            0x39 => Out::from(path_target(self, &ds_dx()).and_then(|(h, exists)| {
                if exists {
                    return Err(fs::ERR_ACCESS_DENIED);
                }
                vfs::create_dir(&h).map_err(|e| io_err(&e))
            })),
            0x3A => Out::from(path_target(self, &ds_dx()).and_then(|(h, exists)| {
                if !exists || !vfs::is_dir(&h) {
                    return Err(fs::ERR_PATH_NOT_FOUND);
                }
                vfs::remove_dir(&h).map_err(|_| fs::ERR_ACCESS_DENIED)
            })),
            0x3B => {
                let r = match self.drives.resolve_long(&ds_dx()) {
                    Ok(Target::Path { exists: true, dos, host }) if vfs::is_dir(&host) => self.drives.chdir(&dos),
                    Ok(_) => Err(fs::ERR_PATH_NOT_FOUND),
                    Err(e) => Err(e),
                };
                Out::from(r)
            }
            0x41 => Out::from(self.lfn_delete(cpu, mem)),
            0x43 => self.lfn_attributes(cpu, mem),
            0x47 => {
                let drive = if cpu.lo(DX) == 0 { self.drives.current } else { cpu.lo(DX) - 1 };
                if !self.drives.is_mounted(drive) {
                    return Out::Err(fs::ERR_INVALID_DRIVE);
                }
                let mut short = vec![b'A' + drive, b':', b'\\'];
                short.extend_from_slice(self.drives.cwd(drive));
                match self.drives.long_path(&short) {
                    Ok(long) => {
                        let mut p = long[3..].to_vec();
                        p.push(0);
                        mem.write(lin(cpu.sreg(DS), cpu.w(SI)), &p);
                        Out::Ok
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0x4E | 0x4F => self.lfn_find(cpu, mem),
            0x56 => {
                let to = read_asciiz(mem, lin(cpu.sreg(ES), cpu.w(DI)));
                let r = path_target(self, &ds_dx()).and_then(|(from, exists)| {
                    if !exists {
                        return Err(fs::ERR_FILE_NOT_FOUND);
                    }
                    match path_target(self, &to)? {
                        (_, true) => Err(fs::ERR_ACCESS_DENIED),
                        (dest, false) => vfs::rename(&from, &dest).map_err(|e| io_err(&e)),
                    }
                });
                Out::from(r)
            }
            0x60 => {
                let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(SI)));
                let r = match cpu.lo(CX) {
                    // Nom court.
                    1 => match self.drives.resolve_long(&path) {
                        Ok(Target::Path { dos, .. }) => Ok(dos),
                        Ok(Target::Device(_)) => Err(fs::ERR_FILE_NOT_FOUND),
                        Err(e) => Err(e),
                    },
                    // Nom canonique ou nom long : chemin long, et pour un
                    // fichier à créer, celui de son répertoire.
                    0 | 2 => self.drives.long_path(&path).or_else(|e| {
                        let cut = path.iter().rposition(|&c| c == b'\\' || c == b'/').ok_or(e)?;
                        let mut dir = self.drives.long_path(&path[..cut.max(if path.get(1) == Some(&b':') { 3 } else { 1 })])?;
                        if !dir.ends_with(b"\\") {
                            dir.push(b'\\');
                        }
                        dir.extend_from_slice(&path[cut + 1..]);
                        Ok(dir)
                    }),
                    _ => Err(ERR_INVALID_FUNCTION),
                };
                match r {
                    Ok(mut p) => {
                        p.push(0);
                        mem.write(lin(cpu.sreg(ES), cpu.w(DI)), &p);
                        Out::Ok
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0x6C | 0xA9 => self.lfn_open(cpu, mem),
            0xA0 => {
                let size = usize::from(cpu.w(CX));
                if size >= 4 {
                    mem.write(lin(cpu.sreg(ES), cpu.w(DI)), b"FAT\0");
                }
                cpu.set_w(BX, VOLUME_FLAGS);
                cpu.set_w(CX, MAX_NAME);
                cpu.set_w(DX, MAX_PATH);
                Out::Ok
            }
            0xA1 => {
                let h = cpu.w(BX);
                match self.lfn_searches.iter().position(|(id, _)| *id == h) {
                    Some(i) => {
                        self.lfn_searches.remove(i);
                        Out::Ok
                    }
                    None => Out::Err(handles::ERR_BAD_HANDLE),
                }
            }
            0xA6 => {
                let r = self.psp_handle(mem, cpu.w(BX)).and_then(|idx| match self.sft.get(idx).map(|e| &e.kind) {
                    Some(Kind::File(f)) => f.file.metadata().map_err(|e| io_err(&e)),
                    _ => Err(handles::ERR_BAD_HANDLE),
                });
                match r {
                    Ok(meta) => {
                        let at = lin(cpu.sreg(DS), cpu.w(DX));
                        let t = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                        mem.fill(at, 0x34, 0);
                        mem.write_u32(at, u32::from(fs::attributes(&meta)));
                        for off in [0x04, 0x0C, 0x14] {
                            write_time(mem, at + off, t, false);
                        }
                        mem.write_u32(at + 0x20, (meta.len() >> 32) as u32);
                        mem.write_u32(at + 0x24, meta.len() as u32);
                        mem.write_u32(at + 0x28, 1);
                        Out::Ok
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0xA7 => match cpu.lo(BX) {
                0 => {
                    let at = lin(cpu.sreg(DS), cpu.w(SI));
                    let ft = u64::from(mem.read_u32(at)) | u64::from(mem.read_u32(at + 4)) << 32;
                    let (time, date) = fs::dos_time(from_filetime(ft));
                    cpu.set_w(CX, time);
                    cpu.set_w(DX, date);
                    cpu.set_hi(BX, 0);
                    Out::Ok
                }
                1 => {
                    let t = fs::from_dos_time(cpu.w(CX), cpu.w(DX)) + Duration::from_millis(u64::from(cpu.hi(BX)) * 10);
                    let ft = filetime(t);
                    let at = lin(cpu.sreg(ES), cpu.w(DI));
                    mem.write_u32(at, ft as u32);
                    mem.write_u32(at + 4, (ft >> 32) as u32);
                    Out::Ok
                }
                _ => Out::Err(ERR_INVALID_FUNCTION),
            },
            0xA8 => {
                let long = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(SI)));
                let short = short_alias(&long);
                let at = lin(cpu.sreg(ES), cpu.w(DI));
                if cpu.hi(DX) == 0 {
                    mem.write(at, &fs::fcb_pattern(&short));
                } else {
                    mem.write(at, &short);
                    mem.write_u8(at + short.len(), 0);
                }
                Out::Ok
            }
            // Fonction absente : AX=7100h et CF, comme un DOS sans noms longs.
            _ => Out::Err(0x7100),
        }
    }

    /// AX=716Ch : ouverture étendue (BX mode, DX action, DS:SI nom).
    fn lfn_open(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(SI)));
        let (action, mode) = (cpu.lo(DX), cpu.lo(BX));
        let target = match self.drives.resolve_long(&path) {
            Ok(t) => t,
            Err(e) => return Out::Err(e),
        };
        let exists = matches!(target, Target::Path { exists: true, .. } | Target::Device(_));
        let r = match (exists, action & 0x0F, action >> 4) {
            (true, 1, _) => self.open_target(mem, target, mode, 0).map(|h| (h, 1)),
            (true, 2, _) => self.open_target(mem, target, mode, 1).map(|h| (h, 3)),
            (true, _, _) => Err(ERR_FILE_EXISTS),
            (false, _, 1) => self.open_target(mem, target, mode, 1).map(|h| (h, 2)),
            (false, _, _) => Err(fs::ERR_FILE_NOT_FOUND),
        };
        match r {
            Ok((h, status)) => {
                cpu.set_w(AX, h);
                cpu.set_w(CX, status);
                Out::Ok
            }
            Err(e) => Out::Err(e),
        }
    }

    /// AX=7141h : suppression (SI=1 : motif et attributs CL/CH).
    fn lfn_delete(&mut self, cpu: &Cpu, mem: &GuestMem) -> Result<(), u16> {
        let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
        if cpu.w(SI) == 0 {
            return match self.drives.resolve_long(&path)? {
                Target::Path { host, exists: true, .. } if vfs::is_file(&host) => {
                    vfs::remove_file(&host).map_err(|_| fs::ERR_ACCESS_DENIED)
                }
                _ => Err(fs::ERR_FILE_NOT_FOUND),
            };
        }
        let fs::LongSearch { dir, pattern, .. } = self.drives.search_long(&path)?;
        let (allowed, required) = (cpu.lo(CX), cpu.hi(CX));
        let mut deleted = 0;
        for e in fs::list(&dir) {
            let a = fs::attributes(&e.meta);
            let name = fs::long_name(&e);
            if a & fs::ATTR_DIR == 0
                && a & !allowed & fs::ATTR_READONLY == 0
                && a & required == required
                && (fs::long_matches(&pattern, &name) || fs::long_matches(&pattern, &e.short))
            {
                vfs::remove_file(&e.host).map_err(|_| fs::ERR_ACCESS_DENIED)?;
                deleted += 1;
            }
        }
        if deleted == 0 { Err(fs::ERR_FILE_NOT_FOUND) } else { Ok(()) }
    }

    /// AX=7143h : attributs (BL=0/1) et dates (BL=2 à 8) d'un fichier.
    fn lfn_attributes(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
        let host = match self.drives.resolve_long(&path) {
            Ok(Target::Path { host, exists: true, .. }) => host,
            Ok(_) => return Out::Err(fs::ERR_FILE_NOT_FOUND),
            Err(e) => return Out::Err(e),
        };
        let meta = match vfs::metadata(&host) {
            Ok(m) => m,
            Err(e) => return Out::Err(io_err(&e)),
        };
        let (time, date) = fs::dos_time(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        match cpu.lo(BX) {
            0 => cpu.set_w(CX, u16::from(fs::attributes(&meta))),
            1 => {
                if let Err(e) = vfs::set_readonly(&host, cpu.w(CX) & u16::from(fs::ATTR_READONLY) != 0) {
                    return Out::Err(io_err(&e));
                }
            }
            // Taille sur le disque.
            2 => {
                cpu.set_w(AX, meta.len() as u16);
                cpu.set_w(DX, (meta.len() >> 16) as u16);
            }
            // Date de dernière écriture.
            3 => {
                let t = fs::from_dos_time(cpu.w(CX), cpu.w(DI));
                let r = vfs::open(&host, vfs::Access { read: false, write: true, create: false }).and_then(|f| f.set_modified(t));
                if let Err(e) = r {
                    return Out::Err(io_err(&e));
                }
            }
            4 => {
                cpu.set_w(CX, time);
                cpu.set_w(DI, date);
            }
            // Dernier accès, création : la date d'écriture tient lieu des deux.
            5 | 7 => {}
            6 => cpu.set_w(DI, date),
            8 => {
                cpu.set_w(CX, time);
                cpu.set_w(DI, date);
                cpu.set_w(SI, 0);
            }
            _ => return Out::Err(ERR_INVALID_FUNCTION),
        }
        Out::Ok
    }

    /// AX=714Eh (DS:DX motif, CL/CH attributs, BX handle rendu dans AX) et
    /// 714Fh (BX handle) ; ES:DI reçoit l'enregistrement, SI son format
    /// d'heure.
    fn lfn_find(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let handle = if cpu.lo(AX) == 0x4E {
            let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
            let fs::LongSearch { dir, parents, pattern } = match self.drives.search_long(&path) {
                Ok(r) => r,
                Err(e) => return Out::Err(e),
            };
            let (allowed, required) = (cpu.lo(CX), cpu.hi(CX));
            let hidden = fs::ATTR_DIR | 0x06;
            let keep = |a: u8| a & !allowed & hidden == 0 && a & required == required;
            let mut results = VecDeque::new();
            if !parents.is_empty() && keep(fs::ATTR_DIR) {
                for dot in [&b"."[..], b".."] {
                    if fs::long_matches(&pattern, dot) {
                        let t = SystemTime::now();
                        results.push_back(Found { long: dot.to_vec(), short: Vec::new(), attr: fs::ATTR_DIR, size: 0, modified: t });
                    }
                }
            }
            for e in fs::list(&dir) {
                let a = fs::attributes(&e.meta);
                let long = fs::long_name(&e);
                if keep(a) && (fs::long_matches(&pattern, &long) || fs::long_matches(&pattern, &e.short)) {
                    let modified = e.meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    // Pas de nom court distinct quand le nom long en est un.
                    let short = if long == e.short { Vec::new() } else { e.short };
                    results.push_back(Found { long, short, attr: a, size: e.meta.len(), modified });
                }
            }
            if results.is_empty() {
                return Out::Err(fs::ERR_FILE_NOT_FOUND);
            }
            let id = self.next_search;
            self.next_search = self.next_search.wrapping_add(1).max(1);
            if self.lfn_searches.len() >= MAX_SEARCHES {
                self.lfn_searches.pop_front();
            }
            self.lfn_searches.push_back((id, results));
            cpu.set_w(AX, id);
            cpu.set_w(CX, 0);
            id
        } else {
            cpu.w(BX)
        };
        let Some((_, results)) = self.lfn_searches.iter_mut().find(|(id, _)| *id == handle) else {
            return Out::Err(handles::ERR_BAD_HANDLE);
        };
        let Some(f) = results.pop_front() else { return Out::Err(fs::ERR_NO_MORE_FILES) };
        let at = lin(cpu.sreg(ES), cpu.w(DI));
        let dos_format = cpu.w(SI) == 1;
        mem.fill(at, 0x13E, 0);
        mem.write_u32(at, u32::from(f.attr));
        for off in [0x04, 0x0C, 0x14] {
            write_time(mem, at + off, f.modified, dos_format);
        }
        mem.write_u32(at + 0x1C, (f.size >> 32) as u32);
        mem.write_u32(at + 0x20, f.size as u32);
        mem.write(at + 0x2C, &f.long[..f.long.len().min(259)]);
        mem.write(at + 0x130, &f.short[..f.short.len().min(13)]);
        if cpu.lo(AX) != 0x4E {
            cpu.set_w(CX, 0);
        }
        Out::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_round_trip() {
        let t = SystemTime::UNIX_EPOCH + Duration::new(1_000_000_000, 500);
        assert_eq!(filetime(SystemTime::UNIX_EPOCH), FILETIME_UNIX * 10_000_000);
        assert_eq!(from_filetime(filetime(t)), SystemTime::UNIX_EPOCH + Duration::new(1_000_000_000, 500));
    }

    #[test]
    fn short_aliases() {
        assert_eq!(short_alias(b"readme.txt"), b"README.TXT");
        assert_eq!(short_alias(b"Long File Name.text"), b"LONGFI~1.TEX");
        assert_eq!(short_alias(b"Makefile"), b"MAKEFILE");
    }
}
