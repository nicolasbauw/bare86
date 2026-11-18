// SPDX-License-Identifier: GPL-3.0-or-later
//! Logiciels tiers que Bare86 va chercher lui-même, à leur source officielle,
//! et range dans son répertoire de données (`~/.local/share/bare86`, ou
//! `$XDG_DATA_HOME/bare86`) : HX DOS Extender (Japheth, freeware, « may be
//! used for any purpose », librement redistribuable), pour les programmes
//! Win32. Téléchargé par `curl` au premier programme Windows lancé sans HX,
//! version fixée et empreintes SHA-256 vérifiées.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Version de HX et archives de sa page de publication (GitHub de Japheth).
const HX_VERSION: &str = "2.23";
const HX_RELEASE: &str = "https://github.com/Baron-von-Riedesel/HX/releases/download/v2.23";
const HX_ARCHIVES: [(&str, &str); 2] = [
    ("HXRT223.zip", "20510cf66d6c8704f66f908f8fbf9783198195c1e043a6ca9fb6adeadf8785c9"),
    ("HXGUI223.zip", "d9f1962369d609c492ed534d886e697fc6bb0f3f94152674d64ab5f06e663b50"),
];

/// Répertoire de données de Bare86.
fn data_dir() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(d).join("bare86")),
        None => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/bare86")),
    }
}

/// Répertoire de HX s'il est installé (son chargeur présent).
pub fn hx_dir() -> Option<PathBuf> {
    let dir = data_dir()?.join("HX");
    dir.join("BIN").join("DPMILD32.EXE").is_file().then_some(dir)
}

/// Corrections ponctuelles des binaires de HX 2.23 : fichier, décalage,
/// octets d'origine, octets corrigés. Appliquées seulement si les octets
/// d'origine sont trouvés à leur place (idempotent ; sans effet sur une
/// autre version).
const HX_PATCHES: [(&str, usize, &[u8], &[u8]); 1] = [
    // GetDeviceCaps(RASTERCAPS) de DGDI32 (DEVCAPS.ASM) : `mov eax, 0C2B9h`
    // ne déclare ni RC_STRETCHBLT (800h) ni RC_STRETCHDIB (2000h), alors
    // que StretchBlt et StretchDIBits sont implémentés (STRETCH.ASM). Le
    // lanceur de Half-Life exige les deux (« Your graphics card does not
    // support a necessary raster operation ») : 0C2B9h → 0EAB9h.
    ("BIN/DGDI32.DLL", 0x3648, &[0xB8, 0xB9, 0xC2, 0x00, 0x00], &[0xB8, 0xB9, 0xEA, 0x00, 0x00]),
];

/// Applique `HX_PATCHES` à l'installation de HX `dir`.
fn patch_hx(dir: &Path) -> Result<(), String> {
    for (file, at, old, new) in HX_PATCHES {
        let path = dir.join(file);
        let Ok(mut data) = std::fs::read(&path) else { continue };
        if data.get(at..at + old.len()) == Some(old) {
            data[at..at + new.len()].copy_from_slice(new);
            std::fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Répertoire de HX, téléchargé et installé s'il manque.
pub fn hx_dir_or_install() -> Result<PathBuf, String> {
    if let Some(d) = hx_dir() {
        patch_hx(&d)?;
        return Ok(d);
    }
    let base = data_dir().ok_or("no home directory")?;
    eprintln!("[win32] downloading HX DOS Extender {HX_VERSION} (Japheth, freeware) from {HX_RELEASE} into {}", base.join("HX").display());
    install_hx(&base)?;
    hx_dir().ok_or_else(|| "HX: DPMILD32.EXE missing after installation".to_string())
}

/// Télécharge, vérifie et décompresse les archives de HX dans `base/HX`
/// (dans un répertoire temporaire d'abord : une installation interrompue ne
/// laisse pas de HX à moitié).
fn install_hx(base: &Path) -> Result<(), String> {
    std::fs::create_dir_all(base).map_err(|e| format!("{}: {e}", base.display()))?;
    let tmp = base.join(format!("HX.tmp{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let result = (|| {
        for (name, sha) in HX_ARCHIVES {
            let zip = tmp.join(name);
            let status = std::process::Command::new("curl")
                .args(["-fsSL", "--retry", "2", "-o"])
                .arg(&zip)
                .arg(format!("{HX_RELEASE}/{name}"))
                .status()
                .map_err(|e| format!("curl: {e}"))?;
            if !status.success() {
                return Err(format!("{name}: download failed ({status})"));
            }
            let data = std::fs::read(&zip).map_err(|e| format!("{name}: {e}"))?;
            let got = format!("{:x}", Sha256::digest(&data));
            if got != sha {
                return Err(format!("{name}: unexpected SHA-256 {got}"));
            }
            for (path, content) in unzip(&data)? {
                let out = tmp.join("HX").join(&path);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(&out, content).map_err(|e| format!("{}: {e}", out.display()))?;
            }
            let _ = std::fs::remove_file(&zip);
        }
        patch_hx(&tmp.join("HX"))?;
        let dest = base.join("HX");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(tmp.join("HX"), &dest).map_err(|e| format!("{}: {e}", dest.display()))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Fichiers d'une archive zip (stockés ou « deflate »), d'après son
/// répertoire central. Les chemins sont vérifiés : relatifs, sans « .. ».
fn unzip(data: &[u8]) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let bad = || "zip: corrupt archive".to_string();
    // Fin du répertoire central : signature 06054B50, cherchée depuis la fin.
    let eocd = (0..data.len().saturating_sub(21)).rev().find(|&o| u32_at(o) == Some(0x0605_4B50)).ok_or_else(bad)?;
    let count = u16_at(eocd + 10).ok_or_else(bad)?;
    let mut at = u32_at(eocd + 16).ok_or_else(bad)?;
    let mut files = Vec::new();
    for _ in 0..count {
        if u32_at(at) != Some(0x0201_4B50) {
            return Err(bad());
        }
        let method = u16_at(at + 10).ok_or_else(bad)?;
        let csize = u32_at(at + 20).ok_or_else(bad)?;
        let size = u32_at(at + 24).ok_or_else(bad)?;
        let (nlen, xlen, clen) = (u16_at(at + 28).ok_or_else(bad)?, u16_at(at + 30).ok_or_else(bad)?, u16_at(at + 32).ok_or_else(bad)?);
        let local = u32_at(at + 42).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(data.get(at + 46..at + 46 + nlen).ok_or_else(bad)?).replace('\\', "/");
        at += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        let path = PathBuf::from(&name);
        if path.is_absolute() || path.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("zip: unsafe path {name}"));
        }
        let start = local + 30 + u16_at(local + 26).ok_or_else(bad)? + u16_at(local + 28).ok_or_else(bad)?;
        let raw = data.get(start..start + csize).ok_or_else(bad)?;
        let content = match method {
            0 => raw.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size);
                std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(raw), &mut out).map_err(|e| format!("zip: {name}: {e}"))?;
                out
            }
            m => return Err(format!("zip: {name}: unsupported method {m}")),
        };
        if content.len() != size {
            return Err(format!("zip: {name}: wrong size"));
        }
        files.push((path, content));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Archive faite à la main : un fichier stocké, un compressé.
    #[test]
    fn unzip_stored_and_deflated() {
        use std::io::Write;
        let mut deflated = Vec::new();
        let mut enc = flate2::write::DeflateEncoder::new(&mut deflated, flate2::Compression::default());
        enc.write_all(b"hello hello hello hello").unwrap();
        enc.finish().unwrap();
        let entries: [(&str, u16, &[u8], usize); 2] = [("BIN/A.TXT", 0, b"abc", 3), ("DOC/B.TXT", 8, &deflated, 23)];
        let mut zip = Vec::new();
        let mut central = Vec::new();
        for (name, method, data, size) in entries {
            let local = zip.len() as u32;
            zip.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
            zip.extend_from_slice(&[0; 4]);
            zip.extend_from_slice(&method.to_le_bytes());
            zip.extend_from_slice(&[0; 8]);
            zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
            zip.extend_from_slice(&(size as u32).to_le_bytes());
            zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
            zip.extend_from_slice(&[0; 2]);
            zip.extend_from_slice(name.as_bytes());
            zip.extend_from_slice(data);
            central.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            central.extend_from_slice(&[0; 6]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(size as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&local.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd = zip.len() as u32;
        zip.extend_from_slice(&central);
        zip.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        zip.extend_from_slice(&[0; 6]);
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&(central.len() as u32).to_le_bytes());
        zip.extend_from_slice(&cd.to_le_bytes());
        zip.extend_from_slice(&[0; 2]);
        let files = unzip(&zip).unwrap();
        assert_eq!(files[0], (PathBuf::from("BIN/A.TXT"), b"abc".to_vec()));
        assert_eq!(files[1].1, b"hello hello hello hello");
    }
}
