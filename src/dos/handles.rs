// SPDX-License-Identifier: GPL-3.0-or-later
//! Handles : la JFT de chaque PSP (en mémoire invitée, comme sous DOS)
//! renvoie à une entrée de la table système des fichiers ouverts (SFT),
//! tenue côté hôte avec un compteur de références.

use crate::mem::GuestMem;

use super::process::{JFT_SIZE, psp, psp_addr};

pub const ERR_TOO_MANY: u16 = 0x04;
pub const ERR_BAD_HANDLE: u16 = 0x06;

/// Entrée libre de la JFT.
pub const UNUSED: u8 = 0xFF;
/// Indices des périphériques standard dans la SFT (ordre de MS-DOS).
pub const SFT_AUX: u8 = 0;
pub const SFT_CON: u8 = 1;
pub const SFT_PRN: u8 = 2;
/// JFT du shell racine : stdin, stdout, stderr = CON, stdaux, stdprn.
pub const ROOT_JFT: [u8; JFT_SIZE] = {
    let mut j = [UNUSED; JFT_SIZE];
    (j[0], j[1], j[2], j[3], j[4]) = (SFT_CON, SFT_CON, SFT_CON, SFT_AUX, SFT_PRN);
    j
};

pub enum Kind {
    Con,
    Aux,
    Prn,
    Nul,
    File(super::fs::OpenFile),
}

impl Kind {
    pub fn is_device(&self) -> bool {
        !matches!(self, Kind::File(_))
    }
}

pub struct Entry {
    pub kind: Kind,
    pub refs: u16,
    /// Mode d'ouverture (bits 0-2 : accès ; bit 7 : non hérité).
    pub mode: u8,
}

pub struct Sft(Vec<Option<Entry>>);

impl Sft {
    pub fn new() -> Self {
        let dev = |kind| Some(Entry { kind, refs: 0, mode: 2 });
        Self(vec![dev(Kind::Aux), dev(Kind::Con), dev(Kind::Prn)])
    }

    pub fn get(&mut self, idx: u8) -> Option<&mut Entry> {
        self.0.get_mut(usize::from(idx))?.as_mut()
    }

    pub fn insert(&mut self, kind: Kind, mode: u8) -> Option<u8> {
        let e = Entry { kind, refs: 1, mode };
        if let Some(i) = self.0.iter().position(Option::is_none) {
            self.0[i] = Some(e);
            return Some(i as u8);
        }
        if self.0.len() >= usize::from(UNUSED) {
            return None;
        }
        self.0.push(Some(e));
        Some((self.0.len() - 1) as u8)
    }

    pub fn add_ref(&mut self, idx: u8) {
        if let Some(e) = self.get(idx) {
            e.refs += 1;
        }
    }

    /// Retire une référence ; l'entrée disparaît à zéro (sauf périphériques
    /// standard, toujours présents).
    pub fn release(&mut self, idx: u8) {
        let i = usize::from(idx);
        if let Some(Some(e)) = self.0.get_mut(i) {
            e.refs = e.refs.saturating_sub(1);
            if e.refs == 0 && i > usize::from(SFT_PRN) {
                self.0[i] = None;
            }
        }
    }
}

/// JFT du PSP : adresse linéaire et nombre d'entrées (pointeur en PSP:34h,
/// qu'un programme peut déplacer avec AH=67h).
fn jft(mem: &GuestMem, seg: u16) -> (usize, u16) {
    let off = mem.read_u16(psp_addr(seg, psp::JFT_PTR));
    let sg = mem.read_u16(psp_addr(seg, psp::JFT_PTR + 2));
    ((usize::from(sg) << 4) + usize::from(off), mem.read_u16(psp_addr(seg, psp::JFT_COUNT)))
}

/// Entrée SFT d'un handle du processus `seg`.
pub fn lookup(mem: &GuestMem, seg: u16, handle: u16) -> Option<u8> {
    let (at, count) = jft(mem, seg);
    if handle >= count {
        return None;
    }
    let idx = mem.read_u8(at + usize::from(handle));
    (idx != UNUSED).then_some(idx)
}

/// Premier handle libre du processus, associé à `idx`.
pub fn assign(mem: &GuestMem, seg: u16, idx: u8) -> Option<u16> {
    let (at, count) = jft(mem, seg);
    let h = (0..count).find(|&h| mem.read_u8(at + usize::from(h)) == UNUSED)?;
    mem.write_u8(at + usize::from(h), idx);
    Some(h)
}

pub fn set(mem: &GuestMem, seg: u16, handle: u16, idx: u8) {
    let (at, count) = jft(mem, seg);
    if handle < count {
        mem.write_u8(at + usize::from(handle), idx);
    }
}

/// Toutes les entrées de la JFT (handle, entrée SFT) utilisées.
pub fn all(mem: &GuestMem, seg: u16) -> Vec<(u16, u8)> {
    let (at, count) = jft(mem, seg);
    (0..count).filter_map(|h| {
        let idx = mem.read_u8(at + usize::from(h));
        (idx != UNUSED).then_some((h, idx))
    })
    .collect()
}
