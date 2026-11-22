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

/// Racine virtuelle de l'image montée sur `drive` (0 = A:), unique : deux
/// machines du même processus (tests) ne se partagent pas leurs images.
pub fn mount_iso(drive: u8, iso: Arc<IsoFs>) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = PathBuf::from(format!("\0cd:{}:{n}", char::from(b'A' + drive)));
    ISOS.lock().unwrap_or_else(|e| e.into_inner()).push((root.clone(), iso));
    root
}

/// Oublie l'image montée sur cette racine (disque changé).
pub fn unmount(root: &Path) {
    ISOS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(r, _)| r != root);
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

/// Fichiers fournis par Bare86 à l'invité : vus dans tout répertoire où
/// aucun fichier de ce nom n'existe (un vrai fichier reste prioritaire), en
/// lecture seule, absents des listes de répertoire. La `glide2x.dll` de
/// Bare86 est ainsi trouvée par le chargeur de HX à côté de chaque jeu (de
/// même que son `opengl32.dll`), et sa `glide2x.ovl` par DOS/4GW.
const BUILTIN: &[(&str, &[u8])] = &[
    ("glide2x.dll", include_bytes!("../../guest/glide2x/glide2x.dll")),
    // Jeux DOS sous DOS/4GW (Carmageddon…) : même Glide, DLL au format LE.
    ("glide2x.ovl", include_bytes!("../../guest/glide2x/glide2x.ovl")),
    // OpenGL 1.1 servi par l'hôte (Quake II, GLQuake…).
    ("opengl32.dll", include_bytes!("../../guest/opengl32/opengl32.dll")),
];

/// Contenu fourni par Bare86 pour ce chemin (fichier absent de l'hôte, dans
/// un répertoire existant).
fn builtin(p: &Path) -> Option<&'static [u8]> {
    let name = p.file_name()?.to_str()?;
    let absent = || !is_virtual(p) && fs::symlink_metadata(p).is_err() && p.parent().is_some_and(Path::is_dir);
    // DOS/4GW n'est pas redistribuable : DOS/32A, compatible, sous son nom
    // (téléchargé au premier besoin), pour les programmes liés à son amorce.
    if name.eq_ignore_ascii_case("DOS4GW.EXE") {
        return if absent() { crate::runtime::dos32a() } else { None };
    }
    let (_, data) = BUILTIN.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    absent().then_some(*data)
}

/// Fichier fourni par Bare86 (absent de l'hôte), et non un vrai fichier.
pub fn is_builtin(p: &Path) -> bool {
    fs::symlink_metadata(p).is_err() && builtin(p).is_some()
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

    fn builtin(data: &[u8]) -> Self {
        // Date fixe : 1er janvier 1998.
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(883_612_800);
        Self { dir: false, len: data.len() as u64, modified, readonly: true }
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
    match fs::metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => builtin(p).map(Meta::builtin).ok_or(e),
        r => r.map(|m| Meta::host(&m)),
    }
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
    if let Some(data) = builtin(p) {
        return Ok(data.to_vec());
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
    if !a.create
        && let Some(data) = builtin(p)
    {
        if a.write {
            return Err(read_only());
        }
        return Ok(VFile::Builtin { data, pos: 0 });
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
    /// Fichier fourni par Bare86 (`BUILTIN`).
    Builtin { data: &'static [u8], pos: u64 },
}

impl VFile {
    pub fn metadata(&self) -> io::Result<Meta> {
        match self {
            VFile::Host(f) => f.metadata().map(|m| Meta::host(&m)),
            VFile::Iso { entry, .. } => Ok(Meta::iso(entry)),
            VFile::Builtin { data, .. } => Ok(Meta::builtin(data)),
        }
    }

    pub fn set_len(&self, len: u64) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.set_len(len),
            VFile::Iso { .. } | VFile::Builtin { .. } => Err(read_only()),
        }
    }

    pub fn set_modified(&self, t: SystemTime) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.set_modified(t),
            VFile::Iso { .. } | VFile::Builtin { .. } => Err(read_only()),
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
            VFile::Builtin { data, pos } => {
                let rest = data.get(*pos as usize..).unwrap_or(&[]);
                let n = rest.len().min(buf.len());
                buf[..n].copy_from_slice(&rest[..n]);
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
            VFile::Iso { .. } | VFile::Builtin { .. } => Err(read_only()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            VFile::Host(f) => f.flush(),
            VFile::Iso { .. } | VFile::Builtin { .. } => Ok(()),
        }
    }
}

impl Seek for VFile {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        match self {
            VFile::Host(f) => f.seek(from),
            VFile::Iso { .. } | VFile::Builtin { .. } => {
                let (pos, len) = match self {
                    VFile::Iso { entry, pos, .. } => (pos, u64::from(entry.len)),
                    VFile::Builtin { data, pos } => (pos, data.len() as u64),
                    VFile::Host(_) => unreachable!(),
                };
                let base = match from {
                    SeekFrom::Start(o) => {
                        *pos = o;
                        return Ok(o);
                    }
                    SeekFrom::Current(d) => (*pos as i64).checked_add(d),
                    SeekFrom::End(d) => (len as i64).checked_add(d),
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

#[cfg(test)]
mod builtin_tests {
    use super::*;

    /// La glide2x.dll de Bare86 est vue là où l'hôte n'en a pas, en lecture
    /// seule ; un vrai fichier du même nom reste prioritaire.
    #[test]
    fn builtin_file_where_missing() {
        let dir = std::env::temp_dir().join(format!("bare86-builtin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("GLIDE2X.DLL");
        let size = include_bytes!("../../guest/glide2x/glide2x.dll").len() as u64;
        assert_eq!(metadata(&p).unwrap().len(), size);
        let mut f = open(&p, Access { read: true, write: false, create: false }).unwrap();
        let mut head = [0u8; 2];
        f.read_exact(&mut head).unwrap();
        assert_eq!(&head, b"MZ");
        assert!(open(&p, Access { read: true, write: true, create: false }).is_err());
        assert!(!exists(&dir.join("absent").join("glide2x.dll")));
        assert!(read_dir(&dir).unwrap().is_empty());
        fs::write(dir.join("glide2x.dll"), b"x").unwrap();
        assert_eq!(metadata(&dir.join("glide2x.dll")).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
