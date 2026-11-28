// SPDX-License-Identifier: GPL-3.0-or-later
//! Logiciels tiers que Bare86 va chercher lui-même, à leur source officielle,
//! et range dans son répertoire de données (`~/.local/share/bare86`, ou
//! `$XDG_DATA_HOME/bare86`) : HX DOS Extender (Japheth, freeware, « may be
//! used for any purpose », librement redistribuable), pour les programmes
//! Win32. Téléchargé par `curl` au premier programme Windows lancé sans HX,
//! version fixée et empreintes SHA-256 vérifiées. Et DOS/32A (Narech K.,
//! licence de type BSD), présenté sous le nom `DOS4GW.EXE` aux programmes
//! liés à l'amorce de DOS/4GW qui ne fournissent pas l'extendeur (démos :
//! Fukwit Daddy, 303) ; son site d'origine a disparu, il vient du dépôt
//! figé de FreeDOS 1.3 (ibiblio). Sa licence demande, en cas de
//! redistribution, la mention « This product uses DOS/32 Advanced DOS
//! Extender technology. » : Bare86 ne le redistribue pas, mais la porte
//! (README).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Version de HX et archives de sa page de publication (GitHub de Japheth).
const HX_VERSION: &str = "2.23";
const HX_RELEASE: &str = "https://github.com/Baron-von-Riedesel/HX/releases/download/v2.23";
const HX_ARCHIVES: [(&str, &str); 2] = [
    ("HXRT223.zip", "20510cf66d6c8704f66f908f8fbf9783198195c1e043a6ca9fb6adeadf8785c9"),
    ("HXGUI223.zip", "d9f1962369d609c492ed534d886e697fc6bb0f3f94152674d64ab5f06e663b50"),
];

/// Bibliothèques d'exécution de Visual C++ 6.0 (MSVCRT, MSVCP60, MFC42…), que les programmes Windows des années
/// 1998-2001 supposent présentes et que HX ne fournit pas. Microsoft les distribuait à part (`vc6redistsetup_enu.exe`,
/// SP4, novembre 2000, KB Q259403) ; l'adresse d'origine est morte, Bare86 prend la copie de la Wayback Machine
/// (fichier identique, octet pour octet, à ce que Microsoft publiait : empreinte vérifiée).
const MSRT_URL: &str = "https://web.archive.org/web/20120707155520id_/http://download.microsoft.com/download/vc60pro/update/1/w9xnt4/en-us/vc6redistsetup_enu.exe";
const MSRT_SHA256: &str = "cf9d0dd968e78d5e685a6ea6ab58ec438ac0ac24316b14d271b02cf3cced67d9";
/// DLL retenues du paquet (HX a déjà ses OLEAUT32, etc.).
const MSRT_FILES: [&str; 6] = ["msvcrt.dll", "msvcirt.dll", "msvcp60.dll", "mfc42.dll", "mfc42u.dll", "olepro32.dll"];

/// DOS/32A 9.1.2 : paquet du dépôt de FreeDOS 1.3 et son empreinte.
const DOS32A_URL: &str = "https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/repositories/1.3/util/dos32a.zip";
const DOS32A_SHA256: &str = "f9094975c61d03a5c3fffc778f49f46995d4a9ade0ec0c9b2e9750f987198f86";

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

/// Binaires du fork de HX de Bare86 (https://github.com/nicolasbauw/HX), qui remplacent ceux de HX 2.23 :
/// nom sous `BIN`, adresse, SHA-256 du binaire du fork, SHA-256 des binaires officiels qu'il remplace.
/// `DPMILD32.EXE` garde l'en-tête de l'exécutable tel qu'il est dans le fichier (les programmes empaquetés avec
/// Petite 2.3 le vérifient) ; `DGDI32.DLL` : `SetDIBits` copie `cScanLines` lignes, `RASTERCAPS` déclare
/// `StretchBlt` et `StretchDIBits`. On ne remplace un fichier que s'il est le binaire officiel (celui de
/// `DGDI32.DLL` a deux empreintes : brut, ou corrigé par `HX_PATCHES`) ; un fichier que HX n'a pas (`MSVFW32.DLL`, liste
/// vide) est installé s'il manque ; un échec n'est pas fatal.
const HX_FORK_RELEASE: &str = "https://github.com/nicolasbauw/HX/releases/download/v2.24-bare86.22";
const HX_FORK_FILES: [(&str, &str, &[&str]); 12] = [
    (
        "DPMILD32.EXE",
        "5db83c24b9a39b0213eb440c2f6f091ade109c015fa70e4e1a07752f18fba643",
        &[
            "3a460e8158bbde0e147a9ac1ae110c0603a8883aeed598a9279dc1efaf6fa8d4",
            "02a94dbbc79b05bd06f54845cb6d1b143337ecc7265a7c17bc1539c61c978ec3",
        ],
    ),
    (
        "DGDI32.DLL",
        "834bbbb9ff2e01c5c04cd00b72fc7306b6b65730511524c2879bc87d67af4913",
        &[
            "08a1c62160d177420ba9c74d3c2f20954d12d105e3e7509c4a8a4af8cdd8a2d3",
            "a5b4e8691d3aadaa34ba6956463fd8a57436fde786f4e4956a54a4ee4064d758",
            "1cdcc7785db46c93143912a6d47bb13a2efe94e1ddd5ca59ef7e13df459c7d62",
            "f6312813bac8bf3402f179b7965ad01b8d2b81f629b8a290fcb127a5deacb4d6",
            "fd10cb7497ad3605d573a5a9d0537e66fde205a7cfb743d370368f4768ee5d4b",
            "39ef30e57b498ea829eb10670d3164b11ecf805fc886e1a32fa3730733ec2a58",
            "d41ae9fcb7de5d693da0bf6b91649b889801ef3416e78ca12c2994c0c6e8c8c3",
            "19674b98e36ed5b87eac7fbed400179266a0687764ed4cd59631795706025af3",
            "1e17bac09e2f38655d7a90310cae5bf7a30483bde592cc97d23dc02d51bba924",
            "e298f7f5da1e678317b25a144b610739cc42fc285e05c17f28a29071f28bd6e0",
            "860841f1442cbf700049cb2177850a8e641b3e4aadca6223b4a3a9476688929c",
            "598ac51c1102ec9b542c7e1168d9f902c670aefd9bee959f918cdb854000722c",
        ],
    ),
    (
        "VESA32.DLL",
        "710540d6cd4b6566e6f3fbd5737c3c380ba9473e61fac2e829a70c8e531b29ce",
        &["78f3613fd291ca1446d8482103a5d61a01dab4803b1186b6ef57329b74010531"],
    ),
    (
        "DUSER32.DLL",
        "336fef50fe826d73ac26e7bd872b23736d8d6cde8117bc463ba5e7a6ab06e411",
        &[
            "9c95d23a3b3f06bb682416b89693b16e87931788076660fb48c50fee826f4f92",
            "74f4bd51669a1949caa992778a2d618ac1424407752348d71d8f20d925f31dd3",
            "c8a6cad73f0ca9d12f4f2ea2909741f909ea7d2774ac73bc9f9cbeca874917d9",
            "b65cae531c9252589442ad07c6e871cc8e4aac146414232e07964d88a13617e4",
            "4e9b790223526a37f3f3ed539f8c11faad89e114f612325b61b33886516938e7",
            "247c9b03ecc058ac8f5e9bf800a88e7f735e5c3b657735f1ca8c62e7a014f7ae",
            "9f8dd1357ae981d27ea564536977911cf8b830acbf915991ae68ef880f272082",
            "3cc6c4d516aa7723aa1aaa970d37356a53643c337271b3afb66c7e6848ec7cac",
            "5e28a471df2a623c399e5d61a02dc2181e4ec233b0d3fb3f70bd15c0711d1e0a",
            "7f09774905555f067936d68577249dac71e1cbd1efe6864a58453f772d7659f8",
        ],
    ),
    (
        "WINMM.DLL",
        "9d56c7d5501d6e3a85b78f028b6272d03fb950e4ffd40cb85499d7f9f2dce800",
        &[
            "feef20a07d4cff7201dc828a1e039e767756cb36a6cd6e16004785f316595b74",
            "569ecc69f6300dd646a14e28a920eb504e7a14ca0f8076693640ea913a9c6048",
        ],
    ),
    // OLE32 et COMCTL32 reconstruits (IsEqualGUID, CoBuildVersion ; InitCommonControlsEx)
    (
        "OLE32.DLL",
        "92d3cc2ef79cc8ab295866efd48f856628450f52b83302650de71abc23b1128a",
        &[
            "6ad368dd773087a3e936668176d73afa4fca880f5a2402edf555c9b55e674b18",
            "26583ff37329eb4c792476df1acf87f66f5002ff5f7bf17edde9aba2ff14b4d8",
        ],
    ),
    (
        "COMCTL32.DLL",
        "940e46fd80ffe6eb1c9a390933f882bdbb898978712881d9b57f5044d0453734",
        &[
            "3738907a38cd068780930452b10e86f759d559fcceff5dde519d253b1010f9f0",
            "969968782b79bab7c7c98074a815f5a5bfece24546466c47895da320e2108605",
        ],
    ),
    // VERSION : VerQueryValueA/W (bouchons dans HX 2.23) ; exigée par la DllMain de ijl11.dll (JPEG d'Intel)
    (
        "VERSION.DLL",
        "505ddfb11202c3af401e2b5f5d162737a83e6167843e86a2276c7e37f31af8c7",
        &["50291e9ed91fffd3bdc6867d2a8edefaf30157258490d00d1974587f9e0e96ed"],
    ),
    // DDDRAW : modes 8 bits palettisés, modes 15 bits annoncés en 16 bits 5-5-5 (OpenPTC : « unsupported bits per pixel »)
    (
        "DDDRAW.DLL",
        "f42cd62883939d453b41bee69ab3a54f4571a4ac192ebac7143b8a35decbf2aa",
        &["76e72561ce4a3c51bdf13f2f723550ff4b1da5de4a267be670246ffbb0310ca5"],
    ),
    // DKRNL32 : MulDiv comme Windows (erreur de division dans HX 2.23)
    (
        "DKRNL32.DLL",
        "21df5dcdb11bed38024a7a1b421a87a000f02d77caf96371aa015f640e63ac65",
        &["e49cc88f16d44e486693c88950454f012335a271252a2e8c3f97b61809691251"],
    ),
    // WININET.DLL : HX n'en a pas (accès Internet absent : les fonctions répondent « non connecté ») ; installée si elle manque
    (
        "WININET.DLL",
        "9cd285fbd9804584386f9736982175ca46097ec920149454d9882ab27be76bf2",
        &[],
    ),
    // sans équivalent dans HX : installée si elle manque (liste de binaires officiels vide)
    (
        "MSVFW32.DLL",
        "c97a7a5e0e1691d83e9873eaa41ca1c1168eaf8dc5202b2a1799ec1578253f7c",
        &[],
    ),
];

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

/// Remplace les binaires officiels de HX par ceux du fork (voir `HX_FORK_FILES`).
fn ensure_fork_files(hx: &Path) {
    for (name, fork_sha, official) in HX_FORK_FILES {
        let path = hx.join("BIN").join(name);
        let replace = match std::fs::read(&path) {
            Ok(current) => official.contains(&format!("{:x}", Sha256::digest(&current)).as_str()),
            Err(_) => official.is_empty(), // absent : seulement pour un fichier que HX n'a pas
        };
        if !replace {
            continue;
        }
        let url = format!("{HX_FORK_RELEASE}/{name}");
        eprintln!("[win32] downloading {name} of Bare86's HX fork from {url}");
        let tmp = hx.join(format!("BIN/{name}.tmp{}", std::process::id()));
        let result = (|| {
            let status = std::process::Command::new("curl")
                .args(["-fsSL", "--retry", "2", "-o"])
                .arg(&tmp)
                .arg(&url)
                .status()
                .map_err(|e| format!("curl: {e}"))?;
            if !status.success() {
                return Err(format!("download failed ({status})"));
            }
            let data = std::fs::read(&tmp).map_err(|e| e.to_string())?;
            let got = format!("{:x}", Sha256::digest(&data));
            if got != fork_sha {
                return Err(format!("unexpected SHA-256 {got}"));
            }
            std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
        })();
        if let Err(e) = result {
            eprintln!("[win32] {name} of the fork not installed ({e}); keeping HX's own");
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Répertoire de HX, téléchargé et installé s'il manque.
pub fn hx_dir_or_install() -> Result<PathBuf, String> {
    if let Some(d) = hx_dir() {
        patch_hx(&d)?;
        ensure_fork_files(&d);
        ensure_ms_runtime(&d);
        return Ok(d);
    }
    let base = data_dir().ok_or("no home directory")?;
    eprintln!("[win32] downloading HX DOS Extender {HX_VERSION} (Japheth, freeware) from {HX_RELEASE} into {}", base.join("HX").display());
    install_hx(&base)?;
    let dir = hx_dir().ok_or_else(|| "HX: DPMILD32.EXE missing after installation".to_string())?;
    ensure_fork_files(&dir);
    ensure_ms_runtime(&dir);
    Ok(dir)
}

/// Ajoute à `hx/BIN` les DLL d'exécution de Visual C++ 6.0 si `MSVCRT.DLL` y manque (un échec n'empêche pas
/// de lancer le programme : message seulement).
fn ensure_ms_runtime(hx: &Path) {
    let bin = hx.join("BIN");
    if bin.join("MSVCRT.DLL").exists() || bin.join("msvcrt.dll").exists() {
        ensure_crtdll(&bin);
        return;
    }
    eprintln!("[win32] downloading the Visual C++ 6.0 runtime (MSVCRT, MSVCP60, MFC42…, Microsoft) from {MSRT_URL}");
    if let Err(e) = install_ms_runtime(&bin) {
        eprintln!("[win32] Visual C++ runtime: {e} (programs that need MSVCRT.DLL will not start)");
    }
}

/// `CRTDLL.DLL` fabriquée à partir de `MSVCRT.DLL` (voir `crtdll`) si elle manque ou si elle n'est qu'une copie de
/// MSVCRT (installations antérieures : il lui manquait `_fmode_dll`, `__GetMainArgs`…).
fn ensure_crtdll(bin: &Path) {
    let Ok(msvcrt) = std::fs::read(bin.join("MSVCRT.DLL")) else { return };
    let path = bin.join("CRTDLL.DLL");
    match std::fs::read(&path) {
        Ok(current) if current != msvcrt => return,
        _ => {}
    }
    match crate::crtdll::from_msvcrt(&msvcrt) {
        Ok(dll) => {
            if let Err(e) = std::fs::write(&path, dll) {
                eprintln!("[win32] CRTDLL.DLL: {e}");
            }
        }
        Err(e) => eprintln!("[win32] CRTDLL.DLL: {e}"),
    }
}

/// Position d'un cabinet CAB (`MSCF`, réservé à zéro, taille plausible) dans `data`.
fn find_cab(data: &[u8]) -> Option<usize> {
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    (0..data.len().saturating_sub(16)).find(|&o| &data[o..o + 4] == b"MSCF" && u32_at(o + 4) == Some(0) && u32_at(o + 8).is_some_and(|n| o + n <= data.len()))
}

/// Contenu du fichier `name` (sans tenir compte de la casse) d'un cabinet.
fn cab_file(cab_data: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut cab = cab::Cabinet::new(std::io::Cursor::new(cab_data)).map_err(|e| format!("cab: {e}"))?;
    let found = cab
        .folder_entries()
        .flat_map(|f| f.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>())
        .find(|n| n.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("cab: {name} not found"))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut cab.read_file(&found).map_err(|e| format!("cab: {found}: {e}"))?, &mut out).map_err(|e| format!("cab: {found}: {e}"))?;
    Ok(out)
}

/// Télécharge `vc6redistsetup_enu.exe`, vérifie son empreinte, en extrait (deux cabinets imbriqués, compression LZX)
/// les DLL de `MSRT_FILES` et les écrit dans `bin`, sans écraser un fichier existant. `CRTDLL.DLL` (la bibliothèque C
/// de Windows 95, absente de HX) est fabriquée à partir de `MSVCRT.DLL` (voir `crtdll`).
fn install_ms_runtime(bin: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("bare86-msrt{}.exe", std::process::id()));
    let status = std::process::Command::new("curl").args(["-fsSL", "--retry", "2", "-o"]).arg(&tmp).arg(MSRT_URL).status().map_err(|e| format!("curl: {e}"))?;
    let data = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if !status.success() {
        return Err(format!("download failed ({status})"));
    }
    let data = data.map_err(|e| format!("{}: {e}", tmp.display()))?;
    let got = format!("{:x}", Sha256::digest(&data));
    if got != MSRT_SHA256 {
        return Err(format!("unexpected SHA-256 {got}"));
    }
    let outer = &data[find_cab(&data).ok_or("no cabinet in the package")?..];
    let inner = cab_file(outer, "vcredist.exe")?;
    let inner_cab = &inner[find_cab(&inner).ok_or("no inner cabinet in the package")?..];
    for name in MSRT_FILES {
        let content = cab_file(inner_cab, name)?;
        let out = bin.join(name.to_ascii_uppercase());
        if !out.exists() {
            std::fs::write(&out, &content).map_err(|e| format!("{}: {e}", out.display()))?;
        }
        if name == "msvcrt.dll" && !bin.join("CRTDLL.DLL").exists() {
            std::fs::write(bin.join("CRTDLL.DLL"), crate::crtdll::from_msvcrt(&content)?).map_err(|e| format!("CRTDLL.DLL: {e}"))?;
        }
    }
    Ok(())
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

/// Contenu de DOS32A.EXE (servi sous le nom DOS4GW.EXE), téléchargé et
/// installé dans `DOS32A/` au premier besoin ; `None` si c'est impossible
/// (message affiché une fois).
pub fn dos32a() -> Option<&'static [u8]> {
    static EXE: std::sync::OnceLock<Option<&'static [u8]>> = std::sync::OnceLock::new();
    *EXE.get_or_init(|| match dos32a_load() {
        Ok(data) => Some(Box::leak(data.into_boxed_slice())),
        Err(e) => {
            eprintln!("[dos4gw] DOS/32A: {e}");
            None
        }
    })
}

fn dos32a_load() -> Result<Vec<u8>, String> {
    let dir = data_dir().ok_or("no home directory")?.join("DOS32A");
    let exe = dir.join("DOS32A.EXE");
    if let Ok(data) = std::fs::read(&exe) {
        return Ok(data);
    }
    eprintln!("[dos4gw] DOS4GW.EXE missing: downloading DOS/32A 9.1.2 (Narech K., BSD-style licence) from {DOS32A_URL} into {}", dir.display());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let zip = dir.join(format!("dos32a.zip.tmp{}", std::process::id()));
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(&zip)
        .arg(DOS32A_URL)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    let data = std::fs::read(&zip);
    let _ = std::fs::remove_file(&zip);
    if !status.success() {
        return Err(format!("download failed ({status})"));
    }
    let data = data.map_err(|e| format!("{}: {e}", zip.display()))?;
    let got = format!("{:x}", Sha256::digest(&data));
    if got != DOS32A_SHA256 {
        return Err(format!("unexpected SHA-256 {got}"));
    }
    let files = unzip(&data)?;
    let get = |name: &str| files.iter().find(|(p, _)| p.to_string_lossy().eq_ignore_ascii_case(name)).map(|(_, d)| d.clone());
    let bin = get("BIN/DOS32A.EXE").ok_or("BIN/DOS32A.EXE missing from the package")?;
    if let Some(licence) = get("DOC/DOS32A/LICENSE") {
        let _ = std::fs::write(dir.join("LICENSE"), licence);
    }
    let tmp = dir.join(format!("DOS32A.EXE.tmp{}", std::process::id()));
    std::fs::write(&tmp, &bin).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    Ok(bin)
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
