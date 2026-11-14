// SPDX-License-Identifier: GPL-3.0-or-later
//! Lecteurs : répertoires hôtes montés (comme `mount c` de DOSBox), chemins
//! DOS, noms 8.3 et recherche par motif.
//!
//! Un nom hôte valide en 8.3 est vu en majuscules ; les autres reçoivent
//! un nom court généré (`LONGNO~1.TXT`), stable tant que le répertoire ne
//! change pas. Les fichiers cachés hôtes (`.git`…) sont ignorés.

use std::path::{Path, PathBuf};

use super::vfs::{self, Meta};
use std::time::SystemTime;

pub const ERR_FILE_NOT_FOUND: u16 = 0x02;
pub const ERR_PATH_NOT_FOUND: u16 = 0x03;
pub const ERR_ACCESS_DENIED: u16 = 0x05;
pub const ERR_INVALID_DRIVE: u16 = 0x0F;
pub const ERR_NO_MORE_FILES: u16 = 0x12;

pub const ATTR_READONLY: u8 = 0x01;
pub const ATTR_VOLUME: u8 = 0x08;
pub const ATTR_DIR: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;

pub struct OpenFile {
    pub file: vfs::VFile,
}

/// Périphériques reconnus dans les noms de fichiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Con,
    Nul,
    Aux,
    Prn,
}

fn device(name: &[u8]) -> Option<Device> {
    let base = name.split(|&c| c == b'.').next().unwrap_or(name);
    Some(match base {
        b"CON" => Device::Con,
        b"NUL" => Device::Nul,
        b"AUX" | b"COM1" | b"COM2" | b"COM3" | b"COM4" => Device::Aux,
        b"PRN" | b"LPT1" | b"LPT2" | b"LPT3" => Device::Prn,
        _ => return None,
    })
}

fn valid_char(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c >= 0x80 || b"!#$%&'()-@^_`{}~".contains(&c)
}

/// Composant DOS en majuscules, tronqué en 8.3 comme le fait DOS.
fn to_83(comp: &[u8]) -> Result<Vec<u8>, u16> {
    let up = comp.to_ascii_uppercase();
    let (base, ext) = match up.iter().position(|&c| c == b'.') {
        Some(i) => (&up[..i], &up[i + 1..]),
        None => (&up[..], &[][..]),
    };
    let ok = |s: &[u8]| s.iter().all(|&c| valid_char(c) || c == b'?' || c == b'*');
    if base.is_empty() || !ok(base) || !ok(ext) || ext.contains(&b'.') {
        return Err(ERR_PATH_NOT_FOUND);
    }
    let mut out = base[..base.len().min(8)].to_vec();
    if !ext.is_empty() {
        out.push(b'.');
        out.extend_from_slice(&ext[..ext.len().min(3)]);
    }
    Ok(out)
}

/// Nom hôte vu par DOS s'il est déjà valide en 8.3.
fn native_83(name: &str) -> Option<Vec<u8>> {
    let up = name.as_bytes().to_ascii_uppercase();
    if name == "." || name == ".." {
        return None;
    }
    let (base, ext) = match up.iter().rposition(|&c| c == b'.') {
        Some(i) => (&up[..i], &up[i + 1..]),
        None => (&up[..], &[][..]),
    };
    let fits = !base.is_empty() && base.len() <= 8 && ext.len() <= 3 && !(up.ends_with(b"."));
    (fits && base.iter().chain(ext).all(|&c| valid_char(c) && c < 0x80)).then_some(up)
}

/// Une entrée de répertoire hôte, avec son nom court.
pub struct DirEntry {
    pub short: Vec<u8>,
    pub host: PathBuf,
    pub meta: Meta,
}

/// Entrées d'un répertoire hôte, triées, avec leurs noms courts.
pub fn list(dir: &Path) -> Vec<DirEntry> {
    let Ok(rd) = vfs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<(String, PathBuf, Meta)> = rd.into_iter().filter(|(n, _, _)| !n.starts_with('.')).collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    let mut taken: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();
    // Les noms natifs d'abord, pour qu'un nom généré ne leur prenne pas leur place.
    let mut pending = Vec::new();
    for (name, path, meta) in names {
        match native_83(&name) {
            Some(s) if !taken.contains(&s) => {
                taken.push(s.clone());
                out.push(DirEntry { short: s, host: path, meta });
            }
            _ => pending.push((name, path, meta)),
        }
    }
    for (name, path, meta) in pending {
        let up = name.as_bytes().to_ascii_uppercase();
        let (stem, ext) = match up.iter().rposition(|&c| c == b'.') {
            Some(i) if i > 0 => (&up[..i], &up[i + 1..]),
            _ => (&up[..], &[][..]),
        };
        let clean = |s: &[u8]| s.iter().copied().filter(|&c| valid_char(c) && c < 0x80).collect::<Vec<u8>>();
        let (stem, ext) = (clean(stem), clean(ext));
        let ext = &ext[..ext.len().min(3)];
        let short = (1..).find_map(|n: u32| {
            let tag = format!("~{n}");
            let keep = 8 - tag.len();
            let mut s = stem[..stem.len().min(keep)].to_vec();
            s.extend_from_slice(tag.as_bytes());
            if !ext.is_empty() {
                s.push(b'.');
                s.extend_from_slice(ext);
            }
            (!taken.contains(&s)).then_some(s)
        });
        let Some(short) = short else { continue };
        taken.push(short.clone());
        out.push(DirEntry { short, host: path, meta });
    }
    out
}

/// Chemin DOS résolu.
pub enum Target {
    Device(Device),
    /// Fichier ou répertoire : chemin hôte (existant ou à créer), existence,
    /// chemin DOS canonique (« C:\JEUX\DOOM.EXE »).
    Path { host: PathBuf, exists: bool, dos: Vec<u8> },
}

pub struct Drives {
    roots: [Option<PathBuf>; 26],
    /// Répertoire courant par lecteur, sans lecteur ni « \ » initial.
    cwd: [Vec<u8>; 26],
    pub current: u8,
}

impl Drives {
    pub fn new() -> Self {
        Self { roots: Default::default(), cwd: Default::default(), current: 2 }
    }

    pub fn mount(&mut self, drive: u8, root: PathBuf) {
        self.roots[usize::from(drive)] = Some(root);
        self.cwd[usize::from(drive)].clear();
    }

    /// Chemin DOS d'un fichier hôte situé sous un lecteur monté.
    pub fn dos_path_of(&self, host: &Path) -> Option<Vec<u8>> {
        let host = vfs::canonicalize(host)?;
        for (drive, root) in self.roots.iter().enumerate() {
            let Some(root) = root.as_ref().and_then(|r| vfs::canonicalize(r)) else { continue };
            let Ok(rel) = host.strip_prefix(&root) else { continue };
            let mut dir = root.clone();
            let mut comps = Vec::new();
            for c in rel.components() {
                let next = dir.join(c);
                let e = list(&dir).into_iter().find(|e| e.host == next)?;
                comps.push(e.short);
                dir = next;
            }
            return Some(canonical(drive as u8, &comps));
        }
        None
    }

    pub fn is_mounted(&self, drive: u8) -> bool {
        self.roots.get(usize::from(drive)).is_some_and(Option::is_some)
    }

    /// Répertoire courant du lecteur, sans « X:\ ».
    pub fn cwd(&self, drive: u8) -> &[u8] {
        &self.cwd[usize::from(drive)]
    }

    /// Lecteur et composants canoniques d'un chemin DOS.
    pub fn split(&self, path: &[u8]) -> Result<(u8, Vec<Vec<u8>>), u16> {
        let (drive, rest) = if path.len() >= 2 && path[1] == b':' {
            let d = path[0].to_ascii_uppercase();
            if !d.is_ascii_uppercase() {
                return Err(ERR_INVALID_DRIVE);
            }
            (d - b'A', &path[2..])
        } else {
            (self.current, path)
        };
        if !self.is_mounted(drive) {
            return Err(ERR_INVALID_DRIVE);
        }
        let mut comps: Vec<Vec<u8>> = Vec::new();
        let absolute = matches!(rest.first(), Some(b'\\' | b'/'));
        if !absolute {
            for c in self.cwd[usize::from(drive)].split(|&c| c == b'\\').filter(|c| !c.is_empty()) {
                comps.push(c.to_vec());
            }
        }
        for c in rest.split(|&c| c == b'\\' || c == b'/') {
            match c {
                b"" | b"." => {}
                b".." => {
                    if comps.pop().is_none() {
                        return Err(ERR_PATH_NOT_FOUND);
                    }
                }
                _ => comps.push(to_83(c)?),
            }
        }
        Ok((drive, comps))
    }

    /// Répertoire hôte correspondant aux composants (qui doivent exister).
    fn host_dir(&self, drive: u8, comps: &[Vec<u8>]) -> Result<PathBuf, u16> {
        let mut dir = self.roots[usize::from(drive)].clone().ok_or(ERR_INVALID_DRIVE)?;
        for c in comps {
            let e = list(&dir).into_iter().find(|e| &e.short == c && e.meta.is_dir()).ok_or(ERR_PATH_NOT_FOUND)?;
            dir = e.host;
        }
        Ok(dir)
    }

    pub fn resolve(&self, path: &[u8]) -> Result<Target, u16> {
        let (drive, comps) = self.split(path)?;
        let dos = canonical(drive, &comps);
        let Some((last, parents)) = comps.split_last() else {
            // Racine du lecteur.
            return Ok(Target::Path { host: self.host_dir(drive, &[])?, exists: true, dos });
        };
        if let Some(d) = device(last) {
            return Ok(Target::Device(d));
        }
        if last.contains(&b'?') || last.contains(&b'*') {
            return Err(ERR_PATH_NOT_FOUND);
        }
        let dir = self.host_dir(drive, parents)?;
        Ok(match list(&dir).into_iter().find(|e| &e.short == last) {
            Some(e) => Target::Path { host: e.host, exists: true, dos },
            None => Target::Path { host: dir.join(String::from_utf8_lossy(last).as_ref()), exists: false, dos },
        })
    }

    /// Répertoire hôte et motif FCB d'une recherche (« C:\JEUX\*.EXE »).
    pub fn search(&self, path: &[u8]) -> Result<(PathBuf, [u8; 11]), u16> {
        let (drive, comps) = self.split(path)?;
        let (pattern, parents) = match comps.split_last() {
            Some((last, parents)) => (fcb_pattern(last), parents),
            None => (fcb_pattern(b"*.*"), &comps[..]),
        };
        Ok((self.host_dir(drive, parents)?, pattern))
    }

    pub fn chdir(&mut self, path: &[u8]) -> Result<(), u16> {
        let (drive, comps) = self.split(path)?;
        self.host_dir(drive, &comps)?;
        self.cwd[usize::from(drive)] = comps.join(&b'\\');
        Ok(())
    }
}

/// « C:\A\B » à partir des composants.
pub fn canonical(drive: u8, comps: &[Vec<u8>]) -> Vec<u8> {
    let mut s = vec![b'A' + drive, b':', b'\\'];
    s.extend_from_slice(&comps.join(&b'\\'));
    s
}

/// Nom 8.3 en champs FCB (8 + 3, complétés de blancs), « * » développé en « ? ».
pub fn fcb_pattern(name: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let (base, ext) = match name.iter().position(|&c| c == b'.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, &[][..]),
    };
    let fill = |field: &mut [u8], src: &[u8]| {
        for (i, slot) in field.iter_mut().enumerate() {
            match src.get(i) {
                Some(b'*') => {
                    field[i..].fill(b'?');
                    return;
                }
                Some(&c) => *slot = c.to_ascii_uppercase(),
                None => return,
            }
        }
    };
    let (b, e) = out.split_at_mut(8);
    fill(b, base);
    fill(e, ext);
    out
}

pub fn fcb_matches(pattern: &[u8; 11], name: &[u8]) -> bool {
    let n = fcb_pattern(name);
    pattern.iter().zip(n).all(|(&p, c)| p == b'?' || p == c)
}

/// Attributs DOS d'un fichier.
pub fn attributes(meta: &Meta) -> u8 {
    let mut a = if meta.is_dir() { ATTR_DIR } else { ATTR_ARCHIVE };
    if meta.readonly() {
        a |= ATTR_READONLY;
    }
    a
}

/// Date et heure DOS (heure locale) : (heure, date).
pub fn dos_time(t: SystemTime) -> (u16, u16) {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    // SAFETY: localtime_r remplit `tm`, initialisé à zéro.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    let year = (tm.tm_year + 1900 - 1980).clamp(0, 127) as u16;
    let time = ((tm.tm_hour as u16) << 11) | ((tm.tm_min as u16) << 5) | (tm.tm_sec as u16 / 2);
    let date = (year << 9) | (((tm.tm_mon + 1) as u16) << 5) | tm.tm_mday as u16;
    (time, date)
}

/// Instant correspondant à une date et heure DOS (heure locale).
pub fn from_dos_time(time: u16, date: u16) -> SystemTime {
    // SAFETY: mktime lit `tm`, entièrement initialisé.
    let secs = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = i32::from(date >> 9) + 80;
        tm.tm_mon = i32::from((date >> 5) & 15) - 1;
        tm.tm_mday = i32::from(date & 31);
        tm.tm_hour = i32::from(time >> 11);
        tm.tm_min = i32::from((time >> 5) & 63);
        tm.tm_sec = i32::from(time & 31) * 2;
        tm.tm_isdst = -1;
        libc::mktime(&mut tm)
    };
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn names() {
        assert_eq!(to_83(b"readme.txt").unwrap(), b"README.TXT");
        assert_eq!(to_83(b"longfilename.text").unwrap(), b"LONGFILE.TEX");
        assert_eq!(native_83("Doom.wad").unwrap(), b"DOOM.WAD");
        assert!(native_83("long name.txt").is_none());
        assert_eq!(&fcb_pattern(b"*.EXE"), b"????????EXE");
        assert_eq!(&fcb_pattern(b"A?C"), b"A?C        ");
        assert!(fcb_matches(&fcb_pattern(b"*.*"), b"README"));
        assert!(fcb_matches(&fcb_pattern(b"R*.T?T"), b"README.TXT"));
        assert!(!fcb_matches(&fcb_pattern(b"*.EXE"), b"README.TXT"));
    }

    #[test]
    fn short_names_and_resolution() {
        let dir = std::env::temp_dir().join(format!("bare86-fs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Sub Dir")).unwrap();
        for f in ["long file name.txt", "longfi~1.txt", "readme.txt", ".hidden"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<_> = list(&dir).into_iter().map(|e| String::from_utf8(e.short).unwrap()).collect();
        assert_eq!(names, ["LONGFI~1.TXT", "README.TXT", "SUBDIR~1", "LONGFI~2.TXT"]);
        let mut d = Drives::new();
        d.mount(2, dir.clone());
        d.chdir(b"subdir~1").unwrap();
        let Target::Path { host, exists, dos } = d.resolve(b"..\\LONGFI~2.TXT").unwrap() else { panic!() };
        assert!(exists);
        assert_eq!(host, dir.join("long file name.txt"));
        assert_eq!(dos, b"C:\\LONGFI~2.TXT");
        assert!(matches!(d.resolve(b"c:\\nul.txt").unwrap(), Target::Device(Device::Nul)));
        assert_eq!(d.resolve(b"D:\\X").err(), Some(ERR_INVALID_DRIVE));
        assert_eq!(d.resolve(b"\\NOPE\\X").err(), Some(ERR_PATH_NOT_FOUND));
        fs::remove_dir_all(&dir).unwrap();
    }
}
