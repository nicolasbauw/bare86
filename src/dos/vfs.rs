// SPDX-License-Identifier: GPL-3.0-or-later
//! Accès aux fichiers des lecteurs : arborescence de l'hôte, ou image ISO
//! montée en lecture seule (CD-ROM). Les chemins restent des `PathBuf` :
//! une image a une racine virtuelle (« \0cd:D », que l'hôte ne peut pas
//! ouvrir), reconnue ici. `Meta` et `VFile` reprennent ce que le DOS
//! utilisait de `fs::Metadata` et `fs::File`.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::iso9660::{Entry, IsoFs};

static ISOS: Mutex<Vec<(PathBuf, Arc<IsoFs>)>> = Mutex::new(Vec::new());

/// Racine virtuelle de l'image montée sur `drive` (0 = A:).
pub fn mount_iso(drive: u8, iso: IsoFs) -> PathBuf {
    let root = PathBuf::from(format!("\0cd:{}", char::from(b'A' + drive)));
    let mut isos = ISOS.lock().unwrap_or_else(|e| e.into_inner());
    isos.retain(|(r, _)| *r != root);
    isos.push((root.clone(), Arc::new(iso)));
    root
}

/// Image d'une racine virtuelle.
pub fn iso_at(root: &Path) -> Option<Arc<IsoFs>> {
    ISOS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(r, _)| r == root).map(|(_, i)| i.clone())
}

/// Image et composants d'un chemin sous une racine virtuelle.
fn iso_path(p: &Path) -> Option<(Arc<IsoFs>, Vec<String>)> {
    let isos = ISOS.lock().unwrap_or_else(|e| e.into_inner());
    let (root, iso) = isos.iter().find(|(r, _)| p.starts_with(r))?;
    let comps = p
        .strip_prefix(root)
        .ok()?
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    Some((iso.clone(), comps))
}

fn iso_entry(p: &Path) -> Option<(Arc<IsoFs>, Entry)> {
    let (iso, comps) = iso_path(p)?;
    let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
    let e = iso.lookup(&refs)?;
    Some((iso, e))
}

fn read_only() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
}

fn not_found() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

/// Ce que le DOS sait d'un fichier.
#[derive(Clone, Debug)]
pub struct Meta {
    dir: bool,
    len: u64,
    modified: SystemTime,
    readonly: bool,
}

impl Meta {
    fn host(m: &fs::Metadata) -> Self {
        Self { dir: m.is_dir(), len: m.len(), modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH), readonly: m.permissions().readonly() }
    }

    fn iso(e: &Entry) -> Self {
        Self { dir: e.dir, len: u64::from(e.len), modified: e.modified, readonly: true }
    }

    pub fn is_dir(&self) -> bool {
        self.dir
    }

    pub fn is_file(&self) -> bool {
        !self.dir
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn modified(&self) -> io::Result<SystemTime> {
        Ok(self.modified)
    }

    pub fn readonly(&self) -> bool {
        self.readonly
    }
}

pub fn is_virtual(p: &Path) -> bool {
    iso_path(p).is_some()
}

pub fn metadata(p: &Path) -> io::Result<Meta> {
    if is_virtual(p) {
        return iso_entry(p).map(|(_, e)| Meta::iso(&e)).ok_or_else(not_found);
    }
    fs::metadata(p).map(|m| Meta::host(&m))
}

pub fn is_dir(p: &Path) -> bool {
    metadata(p).is_ok_and(|m| m.is_dir())
}

pub fn is_file(p: &Path) -> bool {
    metadata(p).is_ok_and(|m| m.is_file())
}

pub fn exists(p: &Path) -> bool {
    metadata(p).is_ok()
}

/// Chemin absolu sans lien (les racines virtuelles le sont déjà).
pub fn canonicalize(p: &Path) -> Option<PathBuf> {
    if is_virtual(p) { Some(p.to_path_buf()) } else { p.canonicalize().ok() }
}

/// Noms et chemins des entrées d'un répertoire.
pub fn read_dir(dir: &Path) -> io::Result<Vec<(String, PathBuf, Meta)>> {
    if is_virtual(dir) {
        let (iso, d) = iso_entry(dir).ok_or_else(not_found)?;
        if !d.dir {
            return Err(not_found());
        }
        return Ok(iso.read_dir(&d).into_iter().filter(|e| !e.hidden).map(|e| (e.name.clone(), dir.join(&e.name), Meta::iso(&e))).collect());
    }
    Ok(fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let meta = fs::metadata(e.path()).ok()?;
            Some((name, e.path(), Meta::host(&meta)))
        })
        .collect())
}

pub fn read(p: &Path) -> io::Result<Vec<u8>> {
    if is_virtual(p) {
        let (iso, e) = iso_entry(p).ok_or_else(not_found)?;
        let mut buf = vec![0u8; e.len as usize];
        let n = iso.read(&e, 0, &mut buf);
        buf.truncate(n);
        return Ok(buf);
    }
    fs::read(p)
}

/// Mode d'ouverture.
#[derive(Clone, Copy)]
pub struct Access {
    pub read: bool,
    pub write: bool,
    /// Créer ou tronquer.
    pub create: bool,
}

pub fn open(p: &Path, a: Access) -> io::Result<VFile> {
    if is_virtual(p) {
        if a.write || a.create {
            return Err(read_only());
        }
        let (iso, e) = iso_entry(p).ok_or_else(not_found)?;
        return Ok(VFile::Iso { iso, entry: e, pos: 0 });
    }
    let mut o = fs::OpenOptions::new();
    o.read(a.read).write(a.write || a.create);
    if a.create {
        o.create(true).truncate(true);
    }
    o.open(p).map(VFile::Host)
}

pub fn create_dir(p: &Path) -> io::Result<()> {
    if is_virtual(p) { Err(read_only()) } else { fs::create_dir(p) }
}

pub fn remove_dir(p: &Path) -> io::Result<()> {
    if is_virtual(p) { Err(read_only()) } else { fs::remove_dir(p) }
}

pub fn remove_file(p: &Path) -> io::Result<()> {
    if is_virtual(p) { Err(read_only()) } else { fs::remove_file(p) }
}

pub fn rename(a: &Path, b: &Path) -> io::Result<()> {
    if is_virtual(a) || is_virtual(b) { Err(read_only()) } else { fs::rename(a, b) }
}

pub fn set_readonly(p: &Path, on: bool) -> io::Result<()> {
    if is_virtual(p) {
        return Err(read_only());
    }
    let mut perm = fs::metadata(p)?.permissions();
    perm.set_readonly(on);
    fs::set_permissions(p, perm)
}

/// Fichier ouvert : de l'hôte, ou d'une image (lecture seule).
pub enum VFile {
    Host(fs::File),
    Iso { iso: Arc<IsoFs>, entry: Entry, pos: u64 },
}

impl VFile {
    pub fn metadata(&self) -> io::Result<Meta> {
        match self {
            VFile::Host(f) => f.metadata().map(|m| Meta::host(&m)),
            VFile::Iso { entry, .. } => Ok(Meta::iso(entry)),
        }
    }

    pub fn set_len(&self, len: u64) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.set_len(len),
            VFile::Iso { .. } => Err(read_only()),
        }
    }

    pub fn set_modified(&self, t: SystemTime) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.set_modified(t),
            VFile::Iso { .. } => Err(read_only()),
        }
    }
}

impl Read for VFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            VFile::Host(f) => f.read(buf),
            VFile::Iso { iso, entry, pos } => {
                let n = iso.read(entry, *pos, buf);
                *pos += n as u64;
                Ok(n)
            }
        }
    }
}

impl Write for VFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            VFile::Host(f) => f.write(buf),
            VFile::Iso { .. } => Err(read_only()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.flush(),
            VFile::Iso { .. } => Ok(()),
        }
    }
}

impl Seek for VFile {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        match self {
            VFile::Host(f) => f.seek(from),
            VFile::Iso { entry, pos, .. } => {
                let base = match from {
                    SeekFrom::Start(o) => {
                        *pos = o;
                        return Ok(o);
                    }
                    SeekFrom::Current(d) => (*pos as i64).checked_add(d),
                    SeekFrom::End(d) => i64::from(entry.len).checked_add(d),
                };
                match base {
                    Some(n) if n >= 0 => {
                        *pos = n as u64;
                        Ok(*pos)
                    }
                    _ => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                }
            }
        }
    }
}
