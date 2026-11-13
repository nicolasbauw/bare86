// SPDX-License-Identifier: GPL-3.0-or-later
//! Processus : PSP, environnement, chargeurs .COM et MZ (relocations).

use crate::interp::Cpu;
use crate::mem::GuestMem;

use super::memory;

pub const ERR_BAD_FORMAT: u16 = 0x0B;

/// Taille de la JFT du PSP (handles 0 à 19).
pub const JFT_SIZE: usize = 20;

/// Champs du PSP.
pub mod psp {
    pub const MEM_TOP: usize = 0x02;
    pub const INT22: usize = 0x0A;
    pub const PARENT: usize = 0x16;
    pub const JFT: usize = 0x18;
    pub const ENV: usize = 0x2C;
    pub const SAVED_SS_SP: usize = 0x2E;
    pub const JFT_COUNT: usize = 0x32;
    pub const JFT_PTR: usize = 0x34;
    pub const VERSION: usize = 0x40;
    pub const FCB1: usize = 0x5C;
    pub const FCB2: usize = 0x6C;
    pub const TAIL: usize = 0x80;
}

pub fn psp_addr(seg: u16, field: usize) -> usize {
    (usize::from(seg) << 4) + field
}

/// Un programme en cours ; le premier de la pile est le shell racine.
pub struct Process {
    pub psp: u16,
    /// État du parent au moment de son INT 21h AH=4Bh, rétabli à la fin.
    /// `None` : lancé par l'hôte, sa fin termine la session.
    pub parent_cpu: Option<Cpu>,
}

/// Programme analysé, prêt à être chargé.
pub struct Exe<'a> {
    pub com: bool,
    /// Module chargé en mémoire (sans l'en-tête MZ).
    pub data: &'a [u8],
    relocs: Vec<(u16, u16)>,
    /// Paragraphes après le module : minimum et maximum (MZ).
    min_extra: u16,
    max_extra: u16,
    cs: u16,
    ip: u16,
    ss: u16,
    sp: u16,
}

fn word(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

pub fn parse(image: &[u8]) -> Result<Exe<'_>, u16> {
    let mz = image.len() >= 0x1C && matches!(&image[..2], b"MZ" | b"ZM");
    if !mz {
        if image.len() > 0xFF00 {
            return Err(ERR_BAD_FORMAT);
        }
        return Ok(Exe {
            com: true,
            data: image,
            relocs: Vec::new(),
            min_extra: 0,
            max_extra: 0xFFFF,
            cs: 0,
            ip: 0x100,
            ss: 0,
            sp: 0,
        });
    }
    let (last, pages) = (usize::from(word(image, 2)), usize::from(word(image, 4)));
    let hdr = usize::from(word(image, 8)) * 16;
    let mut end = pages * 512;
    if last != 0 {
        end = end.saturating_sub(512 - last);
    }
    // Fichiers dont l'en-tête annonce plus que leur taille : on charge ce qui existe.
    let end = end.min(image.len());
    if hdr > end {
        return Err(ERR_BAD_FORMAT);
    }
    let (count, table) = (usize::from(word(image, 6)), usize::from(word(image, 0x18)));
    if table + count * 4 > image.len() {
        return Err(ERR_BAD_FORMAT);
    }
    let relocs = (0..count).map(|i| (word(image, table + i * 4 + 2), word(image, table + i * 4))).collect();
    Ok(Exe {
        com: false,
        data: &image[hdr..end],
        relocs,
        min_extra: word(image, 0x0A),
        max_extra: word(image, 0x0C),
        ss: word(image, 0x0E),
        sp: word(image, 0x10),
        ip: word(image, 0x14),
        cs: word(image, 0x16),
    })
}

impl Exe<'_> {
    fn image_paras(&self) -> u32 {
        (self.data.len() as u32).div_ceil(16)
    }

    /// Paragraphes nécessaires (PSP compris) : minimum et souhaités.
    pub fn paras(&self) -> (u32, u32) {
        if self.com {
            // PSP + image + au moins 256 octets de pile ; tout le reste souhaité.
            let min = (0x100 + self.data.len() as u32 + 0x100).div_ceil(16);
            return (min, 0xFFFF);
        }
        let base = 0x10 + self.image_paras();
        (base + u32::from(self.min_extra), (base + u32::from(self.max_extra)).min(0xFFFF))
    }

    /// Copie le module à `seg` et applique les relocations (facteur `reloc`).
    pub fn place(&self, mem: &GuestMem, seg: u16, reloc: u16) {
        let at = usize::from(seg) << 4;
        mem.write(at, self.data);
        for &(s, o) in &self.relocs {
            let a = at + (usize::from(s) << 4) + usize::from(o);
            if a + 2 <= mem.size() {
                mem.write_u16(a, mem.read_u16(a).wrapping_add(reloc));
            }
        }
    }

    /// Charge dans le bloc `psp` de `paras` paragraphes ; état initial du CPU.
    pub fn load(&self, mem: &GuestMem, psp: u16, paras: u16) -> Cpu {
        if self.com {
            let at = (usize::from(psp) << 4) + 0x100;
            mem.write(at, self.data);
            let top = (u32::from(paras) * 16).min(0x1_0000) - 2;
            // Mot nul en sommet de pile : un RET final saute en PSP:0 (INT 20h).
            mem.write_u16((usize::from(psp) << 4) + top as usize, 0);
            return Cpu::real_mode(psp, 0x100, top as u16);
        }
        // Programme « chargé en haut » (min = max = 0) : module en fin de bloc.
        let high = self.min_extra == 0 && self.max_extra == 0;
        let start = if high { psp + paras - self.image_paras() as u16 } else { psp + 0x10 };
        self.place(mem, start, start);
        let mut cpu = Cpu::real_mode(psp, self.ip, self.sp);
        let real = crate::interp::Seg::real;
        cpu.seg[1] = real(start.wrapping_add(self.cs));
        cpu.seg[2] = real(start.wrapping_add(self.ss));
        cpu
    }
}

/// Environnement : variables du parent (ou `default`), puis le chemin du
/// programme après le mot 0001h.
pub fn env_block(mem: &GuestMem, parent_env: u16, default: &[u8], path: &[u8]) -> Vec<u8> {
    let mut vars = Vec::new();
    if parent_env != 0 {
        let at = usize::from(parent_env) << 4;
        let mut i = 0;
        while i < 0x8000 {
            let c = mem.read_u8(at + i);
            if c == 0 && (vars.is_empty() || vars.last() == Some(&0)) {
                break;
            }
            vars.push(c);
            i += 1;
        }
    } else {
        vars.extend_from_slice(default);
    }
    if vars.last() != Some(&0) && !vars.is_empty() {
        vars.push(0);
    }
    vars.push(0);
    vars.extend_from_slice(&[1, 0]);
    vars.extend_from_slice(path);
    vars.push(0);
    vars
}

pub struct PspInit<'a> {
    pub parent: u16,
    pub env: u16,
    /// Fin du bloc du programme (segment).
    pub top: u16,
    pub jft: [u8; JFT_SIZE],
    /// Ligne de commande (sans la longueur ni le CR final).
    pub tail: &'a [u8],
    pub fcb1: [u8; 16],
    pub fcb2: [u8; 16],
}

/// Écrit un PSP complet à `seg`.
pub fn build_psp(mem: &GuestMem, seg: u16, p: &PspInit<'_>) {
    let a = |f| psp_addr(seg, f);
    mem.fill(a(0), 0x100, 0);
    mem.write(a(0), &[0xCD, 0x20]);
    mem.write_u16(a(psp::MEM_TOP), p.top);
    // Appel CP/M en PSP:0005 (FAR CALL vers le DOS) : réduit à un RETF.
    mem.write_u8(a(0x05), 0xCB);
    // Vecteurs 22h, 23h, 24h du moment, rétablis à la fin du programme.
    let mut v = [0u8; 12];
    mem.read(0x22 * 4, &mut v);
    mem.write(a(psp::INT22), &v);
    mem.write_u16(a(psp::PARENT), p.parent);
    mem.write(a(psp::JFT), &p.jft);
    mem.write_u16(a(psp::ENV), p.env);
    mem.write_u16(a(psp::JFT_COUNT), JFT_SIZE as u16);
    mem.write_u16(a(psp::JFT_PTR), psp::JFT as u16);
    mem.write_u16(a(psp::JFT_PTR + 2), seg);
    mem.write_u32(a(0x38), 0xFFFF_FFFF);
    mem.write(a(psp::VERSION), &[5, 0]);
    // PSP:0050 : INT 21h ; RETF (appel du DOS par CALL FAR PSP:0050).
    mem.write(a(0x50), &[0xCD, 0x21, 0xCB]);
    mem.write(a(psp::FCB1), &p.fcb1);
    mem.write(a(psp::FCB2), &p.fcb2);
    let tail = &p.tail[..p.tail.len().min(126)];
    mem.write_u8(a(psp::TAIL), tail.len() as u8);
    mem.write(a(psp::TAIL + 1), tail);
    mem.write_u8(a(psp::TAIL + 1 + tail.len()), 0x0D);
}

/// FCB vide : lecteur courant, nom en blancs.
pub const BLANK_FCB: [u8; 16] = [0, b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', 0, 0, 0, 0];

/// Nom de base du programme pour son MCB (« C:\JEUX\DOOM.EXE » → « DOOM »).
pub fn mcb_name(path: &[u8]) -> &[u8] {
    let base = path.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(path);
    base.split(|&c| c == b'.').next().unwrap_or(base)
}

/// Réserve le bloc d'un programme : `Ok((segment, paragraphes))`.
pub fn alloc_program(mem: &GuestMem, exe: &Exe<'_>, strategy: u16) -> Result<(u16, u16), u16> {
    let (min, want) = exe.paras();
    if min > 0xFFFF {
        return Err(memory::ERR_NO_MEMORY);
    }
    let largest = memory::largest(mem)?;
    if u32::from(largest) < min {
        return Err(memory::ERR_NO_MEMORY);
    }
    let paras = want.min(u32::from(largest)) as u16;
    let seg = memory::alloc(mem, paras, 0xFFFF, strategy).map_err(|(e, _)| e)?;
    memory::set_owner(mem, seg, seg);
    Ok((seg, paras))
}
