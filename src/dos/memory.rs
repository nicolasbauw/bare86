// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire conventionnelle : chaîne de MCB (Memory Control Blocks) comme
//! sous MS-DOS, en mémoire invitée, pour les programmes qui la parcourent.
//!
//! Un MCB occupe le paragraphe qui précède son bloc : signature `M` (ou `Z`
//! pour le dernier), propriétaire (PSP, 0 = libre, 8 = DOS), taille en
//! paragraphes, puis le nom du programme (8 octets, en +8).

use crate::mem::GuestMem;

/// Segment du premier MCB.
pub const FIRST_MCB: u16 = 0x0100;
/// Fin de la mémoire conventionnelle (segment).
pub const MEM_TOP: u16 = 0xA000;
pub const OWNER_FREE: u16 = 0;

pub const ERR_MCB_DESTROYED: u16 = 0x07;
pub const ERR_NO_MEMORY: u16 = 0x08;
pub const ERR_BAD_BLOCK: u16 = 0x09;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mcb {
    /// Segment du MCB (le bloc commence au paragraphe suivant).
    pub seg: u16,
    pub last: bool,
    pub owner: u16,
    pub size: u16,
}

impl Mcb {
    fn next(&self) -> u16 {
        self.seg + 1 + self.size
    }
}

fn addr(seg: u16) -> usize {
    usize::from(seg) << 4
}

fn read(mem: &GuestMem, seg: u16) -> Option<Mcb> {
    let sig = mem.read_u8(addr(seg));
    if sig != b'M' && sig != b'Z' {
        return None;
    }
    Some(Mcb { seg, last: sig == b'Z', owner: mem.read_u16(addr(seg) + 1), size: mem.read_u16(addr(seg) + 3) })
}

fn write(mem: &GuestMem, m: &Mcb) {
    mem.write_u8(addr(m.seg), if m.last { b'Z' } else { b'M' });
    mem.write_u16(addr(m.seg) + 1, m.owner);
    mem.write_u16(addr(m.seg) + 3, m.size);
}

/// Nouveau MCB (nom effacé).
fn create(mem: &GuestMem, m: &Mcb) {
    mem.fill(addr(m.seg), 16, 0);
    write(mem, m);
}

/// Une seule zone libre, du premier MCB à 640 Ko.
pub fn init(mem: &GuestMem) {
    create(mem, &Mcb { seg: FIRST_MCB, last: true, owner: OWNER_FREE, size: MEM_TOP - FIRST_MCB - 1 });
}

/// Toute la chaîne ; `Err` si elle est corrompue.
pub fn chain(mem: &GuestMem) -> Result<Vec<Mcb>, u16> {
    let mut out = Vec::new();
    let mut seg = FIRST_MCB;
    loop {
        let m = read(mem, seg).ok_or(ERR_MCB_DESTROYED)?;
        out.push(m);
        if m.last {
            return Ok(out);
        }
        if out.len() > 0x1_0000 {
            return Err(ERR_MCB_DESTROYED);
        }
        seg = m.next();
    }
}

/// Fusionne chaque bloc libre avec les blocs libres qui le suivent.
fn coalesce(mem: &GuestMem) -> Result<(), u16> {
    let mut seg = FIRST_MCB;
    loop {
        let mut m = read(mem, seg).ok_or(ERR_MCB_DESTROYED)?;
        if m.owner == OWNER_FREE {
            while !m.last {
                let n = read(mem, m.next()).ok_or(ERR_MCB_DESTROYED)?;
                if n.owner != OWNER_FREE {
                    break;
                }
                m.size += n.size + 1;
                m.last = n.last;
            }
            write(mem, &m);
        }
        if m.last {
            return Ok(());
        }
        seg = m.next();
    }
}

/// Coupe `m` à `paras` paragraphes ; le reste devient un bloc libre.
fn split(mem: &GuestMem, m: &mut Mcb, paras: u16) {
    if paras < m.size {
        let rest = Mcb { seg: m.seg + 1 + paras, last: m.last, owner: OWNER_FREE, size: m.size - paras - 1 };
        create(mem, &rest);
        m.size = paras;
        m.last = false;
    }
    write(mem, m);
}

/// Plus grand bloc libre, en paragraphes.
pub fn largest(mem: &GuestMem) -> Result<u16, u16> {
    coalesce(mem)?;
    Ok(chain(mem)?.iter().filter(|m| m.owner == OWNER_FREE).map(|m| m.size).max().unwrap_or(0))
}

/// Alloue `paras` paragraphes ; renvoie le segment du bloc, ou l'erreur
/// et la taille du plus grand bloc libre. Stratégie : 0 premier, 1 meilleur,
/// 2 dernier bloc convenable (alloué en haut du bloc libre).
pub fn alloc(mem: &GuestMem, paras: u16, owner: u16, strategy: u16) -> Result<u16, (u16, u16)> {
    coalesce(mem).map_err(|e| (e, 0))?;
    let blocks = chain(mem).map_err(|e| (e, 0))?;
    let fits = blocks.iter().filter(|m| m.owner == OWNER_FREE && m.size >= paras);
    let pick = match strategy & 3 {
        1 => fits.min_by_key(|m| m.size),
        2 => fits.max_by_key(|m| m.seg),
        _ => fits.min_by_key(|m| m.seg),
    };
    let Some(&free) = pick else {
        let max = blocks.iter().filter(|m| m.owner == OWNER_FREE).map(|m| m.size).max().unwrap_or(0);
        return Err((ERR_NO_MEMORY, max));
    };
    let mut m = free;
    if strategy & 3 == 2 && paras < free.size {
        // Haut du bloc : la partie basse reste libre.
        let mut low = free;
        low.size = free.size - paras - 1;
        low.last = false;
        write(mem, &low);
        m = Mcb { seg: low.next(), last: free.last, owner, size: paras };
        create(mem, &m);
    } else {
        m.owner = owner;
        mem.fill(addr(m.seg) + 5, 11, 0);
        split(mem, &mut m, paras);
    }
    Ok(m.seg + 1)
}

fn block_mcb(mem: &GuestMem, block: u16) -> Result<Mcb, u16> {
    let m = read(mem, block.wrapping_sub(1)).ok_or(ERR_BAD_BLOCK)?;
    chain(mem)?.contains(&m).then_some(m).ok_or(ERR_BAD_BLOCK)
}

pub fn free(mem: &GuestMem, block: u16) -> Result<(), u16> {
    let mut m = block_mcb(mem, block)?;
    m.owner = OWNER_FREE;
    write(mem, &m);
    Ok(())
}

/// Redimensionne le bloc ; en cas d'échec, renvoie l'erreur et la plus
/// grande taille possible pour ce bloc.
pub fn resize(mem: &GuestMem, block: u16, paras: u16) -> Result<(), (u16, u16)> {
    let mut m = block_mcb(mem, block).map_err(|e| (e, 0))?;
    // Absorbe les blocs libres qui suivent pour connaître la place disponible.
    let mut avail = m;
    while !avail.last {
        let Some(n) = read(mem, avail.next()) else { return Err((ERR_MCB_DESTROYED, 0)) };
        if n.owner != OWNER_FREE {
            break;
        }
        avail.size += n.size + 1;
        avail.last = n.last;
    }
    if paras > avail.size {
        return Err((ERR_NO_MEMORY, avail.size));
    }
    if paras > m.size {
        m = avail;
    }
    split(mem, &mut m, paras);
    Ok(())
}

/// Libère tous les blocs d'un propriétaire (fin de programme).
pub fn free_owned(mem: &GuestMem, owner: u16) {
    if let Ok(blocks) = chain(mem) {
        for mut m in blocks.into_iter().filter(|m| m.owner == owner) {
            m.owner = OWNER_FREE;
            write(mem, &m);
        }
    }
    let _ = coalesce(mem);
}

/// Taille d'un bloc alloué, en paragraphes.
#[cfg(test)]
pub fn size_of(mem: &GuestMem, block: u16) -> Option<u16> {
    block_mcb(mem, block).ok().map(|m| m.size)
}

pub fn set_owner(mem: &GuestMem, block: u16, owner: u16) {
    if let Ok(mut m) = block_mcb(mem, block) {
        m.owner = owner;
        write(mem, &m);
    }
}

/// Nom du programme dans le MCB (DOS 4+), 8 caractères au plus.
pub fn set_name(mem: &GuestMem, block: u16, name: &[u8]) {
    let mut n = [0u8; 8];
    let len = name.len().min(8);
    n[..len].copy_from_slice(&name[..len]);
    mem.write(addr(block - 1) + 8, &n);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> GuestMem {
        let m = GuestMem::new(0x10_0000).unwrap();
        init(&m);
        m
    }

    #[test]
    fn alloc_free_resize() {
        let m = mem();
        let total = largest(&m).unwrap();
        let a = alloc(&m, 0x100, 0x50, 0).unwrap();
        assert_eq!(a, FIRST_MCB + 1);
        let b = alloc(&m, 0x200, 0x50, 0).unwrap();
        assert_eq!(b, a + 0x101);
        // Dernier bloc convenable : en haut de la mémoire.
        let c = alloc(&m, 0x10, 0x50, 2).unwrap();
        assert_eq!(c + 0x10, MEM_TOP);
        assert_eq!(largest(&m).unwrap(), total - 0x100 - 0x200 - 0x10 - 3);
        // A ne peut pas grandir (B le suit) ; après libération de B, si.
        assert_eq!(resize(&m, a, 0x200), Err((ERR_NO_MEMORY, 0x100)));
        free(&m, b).unwrap();
        resize(&m, a, 0x200).unwrap();
        assert_eq!(size_of(&m, a), Some(0x200));
        resize(&m, a, 0x80).unwrap();
        assert_eq!(free(&m, 0x1234), Err(ERR_BAD_BLOCK));
        free_owned(&m, 0x50);
        assert_eq!(largest(&m).unwrap(), total);
        assert_eq!(chain(&m).unwrap().len(), 1);
        assert_eq!(alloc(&m, 0xFFFF, 1, 0), Err((ERR_NO_MEMORY, total)));
    }
}
