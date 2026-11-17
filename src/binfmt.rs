// SPDX-License-Identifier: GPL-3.0-or-later
//! Programmes DOS et Win32 lancés directement depuis Linux (`./JEU.EXE`,
//! double-clic) par binfmt_misc : le noyau exécute alors
//! `bare86 /chemin/JEU.EXE arguments…`.
//!
//! Bare86 n'écrit rien dans le système : `bare86 --binfmt` affiche les
//! règles, que l'utilisateur installe lui-même (droits root), par exemple
//! dans `/etc/binfmt.d/zz-bare86.conf`. Le nom passe après `mono.conf` :
//! systemd-binfmt charge les fichiers dans l'ordre de leurs noms et le noyau
//! essaie d'abord la règle la plus récente, or Mono réclame aussi la
//! signature MZ ; les assemblages .NET lui sont renvoyés (`dotnet_target`).
//!
//! Lancé ainsi (`invoked`), Bare86 cherche le profil du programme
//! (`profile_for`) et ouvre une fenêtre si un affichage graphique existe et
//! que la sortie standard est un terminal.

use std::path::{Path, PathBuf};

/// Règles binfmt_misc pour l'exécutable `exe` : `.EXE` par la signature MZ,
/// `.COM` par l'extension (sans signature ; une règle par casse). Sans le
/// drapeau F, le noyau ouvre l'interpréteur à chaque lancement : un Bare86
/// recompilé ou réinstallé (`cargo install`) sert aussitôt.
pub fn rules(exe: &Path) -> String {
    let exe = exe.display();
    format!(
        "# Bare86: DOS and Win32 programs run directly from Linux (binfmt_misc)\n\
         :bare86-exe:M::MZ::{exe}:\n\
         :bare86-com:E::com::{exe}:\n\
         :bare86-COM:E::COM::{exe}:\n"
    )
}

/// Instructions affichées par `--binfmt` (sur la sortie d'erreur ; les
/// règles seules vont sur la sortie standard, pour `| sudo tee`).
pub const HELP: &str = "\
To run DOS programs directly (./GAME.EXE), install these rules as root:
  bare86 --binfmt | sudo tee /etc/binfmt.d/zz-bare86.conf
  sudo systemctl restart systemd-binfmt
Programs need the execute permission (chmod +x GAME.EXE). To remove:
  sudo rm /etc/binfmt.d/zz-bare86.conf
  echo -1 | sudo tee /proc/sys/fs/binfmt_misc/bare86-{exe,com,COM}
The file name sorts after mono.conf so that these rules take precedence;
.NET programs are handed back to Mono.";

/// Bare86 lancé par le noyau pour `program` (premier argument) : le fichier
/// réellement exécuté (`AT_EXECFN`) est ce programme, et non Bare86.
pub fn invoked(cli: &[String]) -> Option<PathBuf> {
    let program = PathBuf::from(cli.first()?);
    // SAFETY: getauxval rend 0 ou l'adresse d'une chaîne terminée par NUL,
    // posée par le noyau dans la pile initiale du processus.
    let execfn = unsafe {
        let p = libc::getauxval(libc::AT_EXECFN) as *const libc::c_char;
        if p.is_null() {
            return None;
        }
        PathBuf::from(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    };
    let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
    let ours = std::env::current_exe().ok()?;
    (same(&execfn, &program) && !same(&execfn, &ours)).then_some(program)
}

/// Assemblage .NET (PE dont le répertoire de données « COM descriptor »,
/// le 15e, n'est pas vide) : il revient à Mono.
pub fn is_dotnet(bytes: &[u8]) -> bool {
    let u16_at = |o: usize| bytes.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let Some(pe) = u32_at(0x3C).map(|v| v as usize) else { return false };
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
        return false;
    }
    let opt = pe + 24;
    // Répertoires de données : décalage 96 (PE32) ou 112 (PE32+).
    let dirs = match u16_at(opt) {
        Some(0x10B) => opt + 96,
        Some(0x20B) => opt + 112,
        _ => return false,
    };
    let count = u32_at(opt + if u16_at(opt) == Some(0x10B) { 92 } else { 108 }).unwrap_or(0);
    count > 14 && u32_at(dirs + 14 * 8).is_some_and(|rva| rva != 0)
}

/// Mono, pour un assemblage .NET (le premier `mono` du PATH).
pub fn dotnet_target() -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join("mono")).find(|p| p.is_file())
}

/// Profil du programme : `bare86.conf` dans son répertoire, ou dans un
/// répertoire parent dont le profil monte (`mount`) un répertoire qui
/// contient le programme (`./quake2.exe` dans `c/QUAKE2` trouve le profil
/// de `quake2/`, qui monte `c`). Le dossier personnel borne la recherche.
pub fn profile_for(program: &Path) -> Option<PathBuf> {
    let program = program.canonicalize().ok()?;
    let home = std::env::var_os("HOME").map(PathBuf::from).and_then(|h| h.canonicalize().ok());
    let mut dir = program.parent()?;
    let first = dir.to_path_buf();
    loop {
        let conf = dir.join("bare86.conf");
        if let Ok(text) = std::fs::read_to_string(&conf)
            && let Ok(c) = crate::config::parse(&text, dir)
        {
            let mount = c.options.iter().position(|o| o == "--mount").and_then(|i| c.options.get(i + 1));
            let covers = match mount {
                Some(m) => Path::new(m).canonicalize().is_ok_and(|m| program.starts_with(m)),
                None => dir == first,
            };
            if covers {
                return Some(conf);
            }
        }
        if home.as_deref() == Some(dir) {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// Arguments du profil quand on lance le programme même du profil sans
/// argument (`./quake2.exe` reprend `+set vid_ref gl…`).
pub fn profile_args(conf: &Path, program: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(conf) else { return Vec::new() };
    let Ok(c) = crate::config::parse(&text, conf.parent().unwrap_or(Path::new("."))) else { return Vec::new() };
    let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
    match (c.program, c.args) {
        (Some(p), Some(a)) if same(Path::new(&p), program) => a.split_whitespace().map(String::from).collect(),
        _ => Vec::new(),
    }
}

/// Argument passé à la façon de Linux, traduit en chemin DOS. Règle étroite
/// (choix du propriétaire du projet) : seulement un fichier ou répertoire
/// existant (ou à créer dans un répertoire existant : son nom est gardé),
/// écrit avec une barre oblique ailleurs qu'en tête (`./f.zip`,
/// `sous/f.txt`, `/home/…/f.zip` ; pas `/W` ni `/bin`, options DOS
/// possibles), et **seulement sous le lecteur C:** : un programme DOS ne
/// reçoit ni ne voit rien d'autre de la machine. `dos_path_of` rend le
/// chemin DOS d'un chemin de l'hôte (liens résolus, noms courts du DOS).
/// `Err(())` : chemin Linux hors du lecteur C:, laissé tel quel.
pub fn dos_argument(arg: &str, dos_path_of: &dyn Fn(&Path) -> Option<Vec<u8>>) -> Result<Option<Vec<u8>>, ()> {
    if !arg.trim_start_matches('/').contains('/') {
        return Ok(None);
    }
    let path = Path::new(arg);
    let (host, created) = match path.canonicalize() {
        Ok(h) => (h, None),
        // Fichier à créer : son répertoire doit exister.
        Err(_) => match (path.parent().map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p }), path.file_name()) {
            (Some(dir), Some(name)) if !arg.ends_with('/') => match dir.canonicalize() {
                Ok(d) => (d, Some(name.to_string_lossy().into_owned())),
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        },
    };
    match dos_path_of(&host) {
        Some(mut dos) if dos.starts_with(b"C:") => {
            if let Some(name) = created {
                if !dos.ends_with(b"\\") {
                    dos.push(b'\\');
                }
                dos.extend_from_slice(name.as_bytes());
            }
            Ok(Some(dos))
        }
        _ => Err(()),
    }
}

/// Affichage graphique disponible et sortie vers un terminal : fenêtre.
pub fn wants_window() -> bool {
    let display = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    // SAFETY: isatty sur le descripteur 1.
    display && unsafe { libc::isatty(1) } == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_name_the_interpreter() {
        let r = rules(Path::new("/opt/bare86"));
        assert!(r.contains(":bare86-exe:M::MZ::/opt/bare86:\n"));
        assert!(r.contains(":bare86-COM:E::COM::/opt/bare86:\n"));
    }

    /// En-tête PE minimal : PE32, 16 répertoires, « COM descriptor » à `clr`.
    fn pe(clr: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let opt = 0x80 + 24;
        b[opt..opt + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        b[opt + 92..opt + 96].copy_from_slice(&16u32.to_le_bytes());
        b[opt + 96 + 14 * 8..opt + 96 + 14 * 8 + 4].copy_from_slice(&clr.to_le_bytes());
        b
    }

    #[test]
    fn dotnet_detection() {
        assert!(is_dotnet(&pe(0x2008)));
        assert!(!is_dotnet(&pe(0)));
        // Programme DOS (pas d'en-tête PE).
        let mut dos = vec![0u8; 64];
        dos[0..2].copy_from_slice(b"MZ");
        assert!(!is_dotnet(&dos));
    }

    #[test]
    fn arguments_translated_only_under_c() {
        let base = std::env::temp_dir().join(format!("bare86-args-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("c/sub")).unwrap();
        std::fs::write(base.join("c/sub/f.txt"), b"x").unwrap();
        std::fs::write(base.join("outside.txt"), b"x").unwrap();
        let root = base.join("c").canonicalize().unwrap();
        let dos = move |p: &Path| {
            p.strip_prefix(&root).ok().map(|r| {
                let mut v = b"C:\\".to_vec();
                v.extend(r.to_string_lossy().to_uppercase().replace('/', "\\").bytes());
                v
            })
        };
        let inside = base.join("c/sub/f.txt");
        assert_eq!(dos_argument(inside.to_str().unwrap(), &dos), Ok(Some(b"C:\\SUB\\F.TXT".to_vec())));
        // Hors du lecteur C: : laissé tel quel, signalé.
        assert_eq!(dos_argument(base.join("outside.txt").to_str().unwrap(), &dos), Err(()));
        // Fichier à créer dans un répertoire du lecteur : nom gardé.
        let new = base.join("c/sub/new.zip");
        assert_eq!(dos_argument(new.to_str().unwrap(), &dos), Ok(Some(b"C:\\SUB\\new.zip".to_vec())));
        assert_eq!(dos_argument(base.join("new.zip").to_str().unwrap(), &dos), Err(()));
        // Options DOS, noms sans barre, répertoires absents : intacts.
        for a in ["/W", "/h", "/bin", "-timedemo", "f.txt", "./absent/file"] {
            assert_eq!(dos_argument(a, &dos), Ok(None), "{a}");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn profile_lookup_follows_mount() {
        let base = std::env::temp_dir().join(format!("bare86-binfmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let game = base.join("q2/c/QUAKE2");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("quake2.exe"), b"MZ").unwrap();
        // Pas de profil : rien.
        assert_eq!(profile_for(&game.join("quake2.exe")), None);
        // Profil du jeu, deux niveaux plus haut, qui monte `c`.
        std::fs::write(base.join("q2/bare86.conf"), "mount = c\nprogram = c/QUAKE2/quake2.exe\nargs = +set vid_ref gl\n").unwrap();
        let found = profile_for(&game.join("quake2.exe")).unwrap();
        assert_eq!(found, base.join("q2/bare86.conf").canonicalize().unwrap());
        assert_eq!(profile_args(&found, &game.join("quake2.exe")), ["+set", "vid_ref", "gl"]);
        // Un profil plus haut qui ne monte pas le programme est ignoré.
        std::fs::write(base.join("q2/bare86.conf"), "mount = autre\n").unwrap();
        assert_eq!(profile_for(&game.join("quake2.exe")), None);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
