// SPDX-License-Identifier: GPL-3.0-or-later
//! `CRTDLL.DLL` (la bibliothèque C de Windows 95/NT, absente de HX et d'aucun paquet Microsoft téléchargeable) fabriquée
//! à partir de `MSVCRT.DLL` de Visual C++ 6.0, qui en est la suite : même code, mêmes structures (`_iob`, `_ctype`…).
//! Il manque à MSVCRT les noms propres à CRTDLL : les variables `xxx_dll` (`_fmode_dll`, `__argc_dll`…, la même
//! variable que `xxx` de MSVCRT), `__GetMainArgs` (l'ancienne `__getmainargs`, sans le cinquième paramètre), `_matherr`
//! et quelques fonctions `_strxxx` des chaînes à un octet. On ajoute à une copie de MSVCRT une section qui porte une
//! nouvelle table d'exports (celle de MSVCRT, plus ces noms) et le petit code et les variables nécessaires. Le chargeur
//! de HX (DPMILD32) ne suit pas les exports transférés (`MSVCRT.xxx`) : d'où une copie complète plutôt qu'un relais.

/// Noms de CRTDLL qui désignent une variable de MSVCRT sous un autre nom.
const ALIASES: &[(&str, &str)] = &[
    ("_HUGE_dll", "_HUGE"),
    ("__mb_cur_max_dll", "__mb_cur_max"),
    ("_acmdln_dll", "_acmdln"),
    ("_aexit_rtn_dll", "_aexit_rtn"),
    ("_commode_dll", "_commode"),
    ("_daylight_dll", "_daylight"),
    ("_fileinfo_dll", "_fileinfo"),
    ("_fmode_dll", "_fmode"),
    ("_osver_dll", "_osver"),
    ("_pctype_dll", "_pctype"),
    ("_pwctype_dll", "_pwctype"),
    ("_sys_nerr_dll", "_sys_nerr"),
    ("_timezone_dll", "_timezone"),
    ("_winmajor_dll", "_winmajor"),
    ("_winminor_dll", "_winminor"),
    ("_winver_dll", "_winver"),
    ("__argc_dll", "__argc"),
    ("__argv_dll", "__argv"),
    ("_environ_dll", "_environ"),
    ("_pgmptr_dll", "_pgmptr"),
    ("_strspnp", "_mbsspnp"),
];

/// Variables de CRTDLL sans équivalent dans MSVCRT (versions de DOS/OS2 des premières bibliothèques C) : valeur fixe.
const VARIABLES: &[(&str, u32)] = &[
    ("_osmajor_dll", 4),
    ("_osminor_dll", 0),
    ("_osversion_dll", 4),
    ("_osmode_dll", 0),
    ("_cpumode_dll", 0),
    ("_basemajor_dll", 0),
    ("_baseminor_dll", 0),
    ("_baseversion_dll", 0),
];

/// Petites fonctions (code i386 indépendant de la position, convention cdecl).
const FUNCTIONS: &[(&str, &[u8])] = &[
    // int _matherr(struct _exception *) : 0 (erreur traitée par défaut).
    ("_matherr", &[0x31, 0xC0, 0xC3]),
    // char *_strinc(const char *p) : p + 1.
    ("_strinc", &[0x8B, 0x44, 0x24, 0x04, 0x40, 0xC3]),
    // char *_strdec(const char *start, const char *p) : p - 1.
    ("_strdec", &[0x8B, 0x44, 0x24, 0x08, 0x48, 0xC3]),
    // unsigned _strnextc(const char *p) : *p (non signé).
    ("_strnextc", &[0x8B, 0x44, 0x24, 0x04, 0x0F, 0xB6, 0x00, 0xC3]),
    // char *_strninc(const char *p, size_t n) : p + n.
    ("_strninc", &[0x8B, 0x44, 0x24, 0x04, 0x03, 0x44, 0x24, 0x08, 0xC3]),
    // size_t _strncnt(const char *s, size_t n) : nombre de caractères avant le zéro, au plus n.
    ("_strncnt", &[0x8B, 0x54, 0x24, 0x04, 0x8B, 0x4C, 0x24, 0x08, 0x31, 0xC0, 0x39, 0xC8, 0x73, 0x09, 0x80, 0x3C, 0x02, 0x00, 0x74, 0x03, 0x40, 0xEB, 0xF3, 0xC3]),
];

/// `void __GetMainArgs(int *argc, char ***argv, char ***envp, int expand)` : appelle
/// `__getmainargs(argc, argv, envp, expand, &startinfo)` avec un `_startupinfo` nul sur la pile. Les quatre derniers
/// octets (`call rel32`) sont complétés avec l'adresse relative de `__getmainargs`.
const GETMAINARGS: [u8; 30] = [
    0x6A, 0x00, // push 0                (_startupinfo.newmode)
    0x89, 0xE0, // mov eax, esp
    0x50, // push eax              (&_startupinfo)
    0xFF, 0x74, 0x24, 0x18, // push [esp+24]         (expand)
    0xFF, 0x74, 0x24, 0x18, // push [esp+24]         (envp)
    0xFF, 0x74, 0x24, 0x18, // push [esp+24]         (argv)
    0xFF, 0x74, 0x24, 0x18, // push [esp+24]         (argc)
    0xE8, 0, 0, 0, 0, // call __getmainargs
    0x83, 0xC4, 0x18, // add esp, 24
    0xC3, // ret
];
const GETMAINARGS_CALL: usize = 22; // déplacement du `call` (octet 21 : E8)

fn u16_at(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| "MSVCRT: truncated".to_string())
}
fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "MSVCRT: truncated".to_string())
}
fn put32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn align(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

struct Section {
    va: u32,
    vsize: u32,
    raw: u32,
    rsize: u32,
}

/// `CRTDLL.DLL` construite à partir du contenu de `MSVCRT.DLL`.
pub fn from_msvcrt(msvcrt: &[u8]) -> Result<Vec<u8>, String> {
    let d = msvcrt;
    let pe = u32_at(d, 0x3C)? as usize;
    if d.get(pe..pe + 4) != Some(b"PE\0\0") || u16_at(d, pe + 4)? != 0x14C {
        return Err("MSVCRT: not an i386 PE file".into());
    }
    let nsec = u16_at(d, pe + 6)? as usize;
    let opt = pe + 24;
    let sect_table = opt + u16_at(d, pe + 20)? as usize;
    let sect_align = u32_at(d, opt + 32)?;
    let file_align = u32_at(d, opt + 36)?;
    let headers_size = u32_at(d, opt + 60)? as usize;
    let secs: Vec<Section> = (0..nsec)
        .map(|i| {
            let s = sect_table + 40 * i;
            Ok(Section { va: u32_at(d, s + 12)?, vsize: u32_at(d, s + 8)?, raw: u32_at(d, s + 20)?, rsize: u32_at(d, s + 16)? })
        })
        .collect::<Result<_, String>>()?;
    // Place pour un en-tête de section de plus avant les données de la première section.
    let first_raw = secs.iter().filter(|s| s.rsize > 0).map(|s| s.raw as usize).min().unwrap_or(headers_size);
    if sect_table + 40 * (nsec + 1) > first_raw.min(headers_size) {
        return Err("MSVCRT: no room for one more section header".into());
    }
    let off = |rva: u32| -> Result<usize, String> {
        secs.iter()
            .find(|s| rva >= s.va && rva < s.va + s.vsize.max(s.rsize))
            .map(|s| (rva - s.va + s.raw) as usize)
            .ok_or_else(|| format!("MSVCRT: RVA {rva:#x} outside the sections"))
    };
    let cstr = |o: usize| -> Result<&[u8], String> {
        let end = d[o..].iter().position(|&b| b == 0).ok_or("MSVCRT: unterminated string")?;
        Ok(&d[o..o + end])
    };

    // Table d'exports de MSVCRT.
    let exp = off(u32_at(d, opt + 96)?)?;
    let base = u32_at(d, exp + 16)?;
    let nfun = u32_at(d, exp + 20)? as usize;
    let nnames = u32_at(d, exp + 24)? as usize;
    let (afun, anames, aords) = (off(u32_at(d, exp + 28)?)?, off(u32_at(d, exp + 32)?)?, off(u32_at(d, exp + 36)?)?);
    let mut funcs: Vec<u32> = (0..nfun).map(|i| u32_at(d, afun + 4 * i)).collect::<Result<_, _>>()?;
    let mut names: Vec<(Vec<u8>, u16)> = (0..nnames)
        .map(|i| Ok((cstr(off(u32_at(d, anames + 4 * i)?)?)?.to_vec(), u16_at(d, aords + 2 * i)?)))
        .collect::<Result<_, String>>()?;
    let rva_of = |names: &[(Vec<u8>, u16)], funcs: &[u32], n: &str| -> Result<u32, String> {
        names.iter().find(|(m, _)| m == n.as_bytes()).map(|(_, o)| funcs[*o as usize]).ok_or_else(|| format!("MSVCRT: no export {n}"))
    };

    // Nouvelle section : code, variables, puis table d'exports.
    let last_end = secs.iter().map(|s| s.va + s.vsize.max(s.rsize)).max().unwrap_or(0);
    let new_va = align(last_end, sect_align);
    let mut body: Vec<u8> = Vec::new();
    let add = |names: &mut Vec<(Vec<u8>, u16)>, funcs: &mut Vec<u32>, name: &str, rva: u32| {
        funcs.push(rva);
        names.push((name.as_bytes().to_vec(), (funcs.len() - 1) as u16));
    };
    for (alias, target) in ALIASES {
        let rva = rva_of(&names, &funcs, target)?;
        add(&mut names, &mut funcs, alias, rva);
    }
    let getmainargs = rva_of(&names, &funcs, "__getmainargs")?;
    let mut code = GETMAINARGS;
    let call_next = new_va + (GETMAINARGS_CALL + 4) as u32;
    code[GETMAINARGS_CALL..GETMAINARGS_CALL + 4].copy_from_slice(&getmainargs.wrapping_sub(call_next).to_le_bytes());
    body.extend_from_slice(&code);
    add(&mut names, &mut funcs, "__GetMainArgs", new_va);
    for (name, bytes) in FUNCTIONS {
        while body.len() % 4 != 0 {
            body.push(0xCC);
        }
        let rva = new_va + body.len() as u32;
        body.extend_from_slice(bytes);
        add(&mut names, &mut funcs, name, rva);
    }
    while body.len() % 4 != 0 {
        body.push(0);
    }
    for (name, value) in VARIABLES {
        let rva = new_va + body.len() as u32;
        body.extend_from_slice(&value.to_le_bytes());
        add(&mut names, &mut funcs, name, rva);
    }
    if funcs.len() > u16::MAX as usize {
        return Err("MSVCRT: too many exports".into());
    }
    names.sort_by(|a, b| a.0.cmp(&b.0));

    // Répertoire d'exports : en-tête (40 octets), fonctions, noms, ordinaux, nom de la DLL, chaînes.
    let dir = body.len();
    let dir_va = new_va + dir as u32;
    let fun_at = dir + 40;
    let names_at = fun_at + 4 * funcs.len();
    let ords_at = names_at + 4 * names.len();
    body.resize(ords_at + 2 * names.len(), 0);
    let rva = |o: usize| new_va + o as u32;
    let dll_name = rva(body.len());
    body.extend_from_slice(b"CRTDLL.dll\0");
    let mut name_rvas = Vec::with_capacity(names.len());
    for (n, _) in &names {
        name_rvas.push(rva(body.len()));
        body.extend_from_slice(n);
        body.push(0);
    }
    put32(&mut body, dir + 12, dll_name);
    put32(&mut body, dir + 16, base);
    put32(&mut body, dir + 20, funcs.len() as u32);
    put32(&mut body, dir + 24, names.len() as u32);
    put32(&mut body, dir + 28, rva(fun_at));
    put32(&mut body, dir + 32, rva(names_at));
    put32(&mut body, dir + 36, rva(ords_at));
    for (i, f) in funcs.iter().enumerate() {
        put32(&mut body, fun_at + 4 * i, *f);
    }
    for (i, ((_, ord), nr)) in names.iter().zip(&name_rvas).enumerate() {
        put32(&mut body, names_at + 4 * i, *nr);
        body[ords_at + 2 * i..ords_at + 2 * i + 2].copy_from_slice(&ord.to_le_bytes());
    }
    let dir_size = (body.len() - dir) as u32;

    // Fichier : copie de MSVCRT, nouvelle section à la fin (alignée), en-têtes mis à jour.
    let mut out = d.to_vec();
    let raw = align(out.len() as u32, file_align);
    out.resize(raw as usize, 0);
    let vsize = body.len() as u32;
    let rsize = align(vsize, file_align);
    out.extend_from_slice(&body);
    out.resize((raw + rsize) as usize, 0);
    let s = sect_table + 40 * nsec;
    out[s..s + 40].fill(0);
    out[s..s + 8].copy_from_slice(b".crtdll\0");
    put32(&mut out, s + 8, vsize);
    put32(&mut out, s + 12, new_va);
    put32(&mut out, s + 16, rsize);
    put32(&mut out, s + 20, raw);
    put32(&mut out, s + 36, 0xE000_0060); // code, données initialisées, lecture, écriture, exécution
    out[pe + 6..pe + 8].copy_from_slice(&((nsec + 1) as u16).to_le_bytes());
    put32(&mut out, opt + 56, align(new_va + vsize, sect_align)); // SizeOfImage
    put32(&mut out, opt + 64, 0); // CheckSum
    put32(&mut out, opt + 96, dir_va);
    put32(&mut out, opt + 100, dir_size);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Petit PE i386 : une section qui contient du code et la table d'exports d'une « MSVCRT » réduite aux noms
    /// dont la fabrication a besoin.
    fn fake_msvcrt() -> Vec<u8> {
        let mut needed: Vec<&str> = ALIASES.iter().map(|(_, t)| *t).collect();
        needed.push("__getmainargs");
        needed.push("printf");
        needed.sort();
        needed.dedup();
        let mut d = vec![0u8; 0x1000];
        d[0..2].copy_from_slice(b"MZ");
        put32(&mut d, 0x3C, 0x80);
        d[0x80..0x84].copy_from_slice(b"PE\0\0");
        d[0x84..0x86].copy_from_slice(&0x14Cu16.to_le_bytes());
        d[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        d[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes());
        let opt = 0x98;
        put32(&mut d, opt + 32, 0x1000);
        put32(&mut d, opt + 36, 0x200);
        put32(&mut d, opt + 56, 0x2000);
        put32(&mut d, opt + 60, 0x400);
        let s = opt + 0xE0;
        d[s..s + 8].copy_from_slice(b".text\0\0\0");
        put32(&mut d, s + 8, 0xC00);
        put32(&mut d, s + 12, 0x1000);
        put32(&mut d, s + 16, 0xC00);
        put32(&mut d, s + 20, 0x400);
        // Exports à RVA 0x1000 (fichier 0x400) : fonction i à RVA 0x1800 + 4 i.
        let e = 0x400;
        let n = needed.len();
        put32(&mut d, e + 16, 1);
        put32(&mut d, e + 20, n as u32);
        put32(&mut d, e + 24, n as u32);
        put32(&mut d, e + 28, 0x1028);
        put32(&mut d, e + 32, 0x1028 + 4 * n as u32);
        put32(&mut d, e + 36, 0x1028 + 8 * n as u32);
        let mut str_at = 0x1028 + 10 * n as u32;
        for (i, name) in needed.iter().enumerate() {
            put32(&mut d, e + 0x28 + 4 * i, 0x1800 + 4 * i as u32);
            put32(&mut d, e + 0x28 + 4 * n + 4 * i, str_at);
            d[e + 0x28 + 8 * n + 2 * i..][..2].copy_from_slice(&(i as u16).to_le_bytes());
            let o = (str_at - 0x1000 + 0x400) as usize;
            d[o..o + name.len()].copy_from_slice(name.as_bytes());
            str_at += name.len() as u32 + 1;
        }
        put32(&mut d, opt + 96, 0x1000);
        d
    }

    /// Exports (nom -> RVA) d'un PE fabriqué.
    fn exports(d: &[u8]) -> Vec<(String, u32)> {
        let pe = u32_at(d, 0x3C).unwrap() as usize;
        let opt = pe + 24;
        let nsec = u16_at(d, pe + 6).unwrap() as usize;
        let st = opt + u16_at(d, pe + 20).unwrap() as usize;
        let off = |rva: u32| {
            (0..nsec)
                .map(|i| st + 40 * i)
                .find_map(|s| {
                    let (va, vs, raw) = (u32_at(d, s + 12).unwrap(), u32_at(d, s + 8).unwrap(), u32_at(d, s + 20).unwrap());
                    (rva >= va && rva < va + vs).then(|| (rva - va + raw) as usize)
                })
                .unwrap()
        };
        let e = off(u32_at(d, opt + 96).unwrap());
        let n = u32_at(d, e + 24).unwrap() as usize;
        let (f, nm, o) = (off(u32_at(d, e + 28).unwrap()), off(u32_at(d, e + 32).unwrap()), off(u32_at(d, e + 36).unwrap()));
        (0..n)
            .map(|i| {
                let s = off(u32_at(d, nm + 4 * i).unwrap());
                let end = d[s..].iter().position(|&b| b == 0).unwrap();
                let ord = u16_at(d, o + 2 * i).unwrap() as usize;
                (String::from_utf8(d[s..s + end].to_vec()).unwrap(), u32_at(d, f + 4 * ord).unwrap())
            })
            .collect()
    }

    #[test]
    fn crtdll_exports_msvcrt_and_its_own_names() {
        let src = fake_msvcrt();
        let out = from_msvcrt(&src).unwrap();
        let ex = exports(&out);
        let get = |n: &str| ex.iter().find(|(m, _)| m == n).map(|(_, r)| *r);
        // Noms triés (recherche dichotomique du chargeur).
        assert!(ex.windows(2).all(|w| w[0].0.as_bytes() < w[1].0.as_bytes()), "noms non triés");
        // Les noms de MSVCRT restent, les alias désignent la même variable.
        assert_eq!(get("printf"), exports(&src).iter().find(|(m, _)| m == "printf").map(|(_, r)| *r));
        assert_eq!(get("_fmode_dll"), get("_fmode"));
        assert_eq!(get("__argv_dll"), get("__argv"));
        // Code et variables dans la nouvelle section, dont __GetMainArgs qui appelle __getmainargs.
        let gma = get("__GetMainArgs").unwrap();
        assert_eq!(gma, 0x2000);
        let pe = 0x80;
        assert_eq!(u16_at(&out, pe + 6).unwrap(), 2);
        let raw = u32_at(&out, 0x98 + 0xE0 + 40 + 20).unwrap() as usize;
        assert_eq!(out[raw + GETMAINARGS_CALL - 1], 0xE8, "opcode du call");
        let rel = u32_at(&out, raw + GETMAINARGS_CALL).unwrap();
        assert_eq!(gma + GETMAINARGS_CALL as u32 + 4 + rel, get("__getmainargs").unwrap());
        let osmajor = get("_osmajor_dll").unwrap();
        assert_eq!(u32_at(&out, raw + (osmajor - 0x2000) as usize).unwrap(), 4);
        for name in ["_matherr", "_strinc", "_strdec", "_strnextc", "_strninc", "_strncnt", "_strspnp"] {
            assert!(get(name).is_some(), "{name} manquant");
        }
        assert_eq!(u32_at(&out, 0x98 + 56).unwrap(), 0x3000, "SizeOfImage");
    }
}
