// SPDX-License-Identifier: GPL-3.0-or-later
//! Programmes Win32 sous le DOS intégré, par HX DOS Extender (Japheth,
//! freeware) : un exécutable PE est lancé par le chargeur PE de HX,
//! `DPMILD32.EXE`, comme le ferait son TSR `HXLDR32` (que notre shell ne
//! déclenche pas : il lance les programmes côté hôte, sans INT 21h AH=4Bh).
//! Les PE dont le stub charge déjà HX (outil PESTUB) passent aussi par là :
//! leur stub ferait de même, mais sans pouvoir compléter le PATH. Un PE à
//! stub inconnu (programme DOS à extendeur intégré, WDOSX) tourne tel quel.

/// Répertoire de HX essayé après ceux du PATH.
const DEFAULT_DIR: &[u8] = b"C:\\HX\\BIN";
const LOADER: &[u8] = b"DPMILD32.EXE";

/// Chargeur à lancer à la place du programme.
pub struct Loader {
    pub image: Vec<u8>,
    /// Chemin DOS de DPMILD32.EXE.
    pub path: Vec<u8>,
    /// Ligne de commande : le programme, puis ses arguments.
    pub tail: Vec<u8>,
    /// Répertoire de HX (ses DLL), à mettre dans le PATH du programme.
    pub dir: Vec<u8>,
}

/// Stubs des PE pour Windows : Microsoft, Borland, HX (PESTUB).
const WINDOWS_STUBS: [&[u8]; 3] = [b"cannot be run in DOS mode", b"must be run under Win32", b"DPMILD32"];

/// Exécutable PE pour Windows, au stub connu.
pub fn is_win32(image: &[u8]) -> bool {
    let word = |at: usize| image.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let dword = |at: usize| image.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if !image.starts_with(b"MZ") || word(0x18).is_none_or(|r| r < 0x40) {
        return false;
    }
    let Some(pe) = dword(0x3C).map(|o| o as usize) else { return false };
    image.get(pe..pe + 4) == Some(b"PE\0\0")
        && WINDOWS_STUBS.iter().any(|s| image[..pe].windows(s.len()).any(|w| w == *s))
}

/// Chargeur de HX pour `image` (chemin DOS `path`, arguments `tail`), cherché
/// dans les répertoires de `path_var` puis dans `C:\HX\BIN` ; `read` rend le
/// contenu et le chemin canonique d'un fichier. `None` : pas un PE pour
/// Windows, ou HX introuvable (le stub du programme s'exécute alors).
pub fn loader_for(
    image: &[u8],
    path: &[u8],
    tail: &[u8],
    path_var: &[u8],
    read: impl Fn(&[u8]) -> Option<(Vec<u8>, Vec<u8>)>,
) -> Option<Loader> {
    if !is_win32(image) {
        return None;
    }
    let dirs = path_var.split(|&c| c == b';').map(<[u8]>::trim_ascii).filter(|d| !d.is_empty()).chain([DEFAULT_DIR]);
    for dir in dirs {
        let dir = dir.strip_suffix(b"\\").unwrap_or(dir);
        if let Some((image, path_found)) = read(&[dir, b"\\", LOADER].concat()) {
            let dir = path_found[..path_found.len() - LOADER.len() - 1].to_vec();
            let args = tail.trim_ascii_start();
            let mut line = [&b" "[..], path].concat();
            if !args.is_empty() {
                line.push(b' ');
                line.extend_from_slice(args);
            }
            if line.len() > 126 {
                // Limite du PSP, que DPMILD32 reprend pour le programme.
                eprintln!("[win32] command line longer than 126 characters: truncated");
            }
            return Some(Loader { image, path: path_found, tail: line, dir });
        }
    }
    eprintln!("[win32] {}: Windows program; HX DOS Extender not found (PATH, C:\\HX\\BIN)", String::from_utf8_lossy(path));
    None
}

/// Variables d'un bloc d'environnement (`env_block`), et la suite du bloc
/// (compte et chemin du programme).
fn split_env(env: &[u8]) -> (Vec<Vec<u8>>, &[u8]) {
    // Fin des variables : le premier NUL qui suit un NUL (ou en tête).
    let end = (0..env.len()).find(|&i| env[i] == 0 && (i == 0 || env[i - 1] == 0)).unwrap_or(env.len());
    let vars = env[..end].split(|&c| c == 0).filter(|v| !v.is_empty()).map(<[u8]>::to_vec).collect();
    (vars, env.get(end + 1..).unwrap_or(&[]))
}

fn join_env(vars: &[Vec<u8>], rest: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = vars.iter().flat_map(|v| v.iter().copied().chain([0])).collect();
    out.push(0);
    out.extend_from_slice(rest);
    out
}

/// Bloc d'environnement (`env_block`) dont le PATH contient `dir`.
pub fn with_path_dir(env: &[u8], dir: &[u8]) -> Vec<u8> {
    let (mut vars, rest) = split_env(env);
    let same = |d: &[u8]| {
        let d = d.trim_ascii();
        d.strip_suffix(b"\\").unwrap_or(d).eq_ignore_ascii_case(dir)
    };
    match vars.iter_mut().find(|v| v.len() >= 5 && v[..5].eq_ignore_ascii_case(b"PATH=")) {
        Some(v) if v[5..].split(|&c| c == b';').any(same) => {}
        Some(v) => {
            if v.len() > 5 && !v.ends_with(b";") {
                v.push(b';');
            }
            v.extend_from_slice(dir);
        }
        None => vars.push([&b"PATH="[..], dir].concat()),
    }
    join_env(&vars, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STUB: &[u8] = b"This program cannot be run in DOS mode.";

    /// PE minimal : en-tête MZ, stub `stub`, signature PE en 80h.
    fn pe(stub: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; 0x84];
        d[..2].copy_from_slice(b"MZ");
        d[0x18] = 0x40;
        d[0x3C] = 0x80;
        d[0x40..0x40 + stub.len()].copy_from_slice(stub);
        d[0x80..0x84].copy_from_slice(b"PE\0\0");
        d
    }

    #[test]
    fn windows_pe_is_recognized() {
        assert!(is_win32(&pe(b"This program cannot be run in DOS mode.")));
        assert!(is_win32(&pe(b"This program must be run under Win32")));
        assert!(is_win32(&pe(b"DPMILD32.EXE")));
        // PE à extendeur DOS intégré, exécutable DOS ordinaire.
        assert!(!is_win32(&pe(b"WDOSX")));
        let mut mz = pe(b"cannot be run in DOS mode");
        mz[0x18] = 0x1E;
        assert!(!is_win32(&mz));
        assert!(!is_win32(b"MZ"));
    }

    #[test]
    fn loader_from_path_or_default_dir() {
        let read = |p: &[u8]| (p == b"C:\\HX\\BIN\\DPMILD32.EXE").then(|| (b"MZ".to_vec(), p.to_vec()));
        let l = loader_for(&pe(STUB), b"C:\\Q2\\QUAKE2.EXE", b" +map base1", b"C:\\;D:\\TOOLS", read).unwrap();
        assert_eq!(l.path, b"C:\\HX\\BIN\\DPMILD32.EXE");
        assert_eq!(l.tail, b" C:\\Q2\\QUAKE2.EXE +map base1");
        assert_eq!(l.dir, b"C:\\HX\\BIN");
        let read = |p: &[u8]| (p == b"D:\\HX\\DPMILD32.EXE").then(|| (b"MZ".to_vec(), p.to_vec()));
        let l = loader_for(&pe(STUB), b"C:\\A.EXE", b"", b"D:\\HX\\", read).unwrap();
        assert_eq!((l.dir.as_slice(), l.tail.as_slice()), (&b"D:\\HX"[..], &b" C:\\A.EXE"[..]));
        assert!(loader_for(&pe(STUB), b"C:\\A.EXE", b"", b"", |_: &[u8]| None).is_none());
    }

    #[test]
    fn path_gets_the_hx_directory_once() {
        let env = b"COMSPEC=C:\\COMMAND.COM\0PATH=C:\\\0\0\x01\0C:\\HX\\BIN\\DPMILD32.EXE\0";
        let out = with_path_dir(env, b"C:\\HX\\BIN");
        assert_eq!(out, b"COMSPEC=C:\\COMMAND.COM\0PATH=C:\\;C:\\HX\\BIN\0\0\x01\0C:\\HX\\BIN\\DPMILD32.EXE\0");
        assert_eq!(with_path_dir(&out, b"C:\\HX\\BIN"), out);
        assert_eq!(with_path_dir(b"\0\x01\0X\0", b"C:\\HX"), b"PATH=C:\\HX\0\0\x01\0X\0");
    }
}
