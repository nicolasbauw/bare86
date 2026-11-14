// SPDX-License-Identifier: GPL-3.0-or-later
//! Extensions CD-ROM de Microsoft (MSCDEX 2.23) en HLE, pour le DOS
//! intégré : INT 2Fh AH=15h, et requêtes au pilote de périphérique
//! (AX=1510h). Le disque est l'image ISO montée par `vfs` ; les fichiers
//! passent par les fonctions ordinaires du DOS.
//!
//! L'en-tête de pilote (« MSCD001 ») rendu par 1501h n'a pas de code
//! derrière ses points d'entrée (RETF) : les programmes qui appellent le
//! pilote directement au lieu de passer par 1510h ne sont pas servis.

use std::path::PathBuf;
use std::sync::Arc;

use crate::cdrom::SECTOR;
use crate::hle::{AX, BX, CF, CX, DI, DX, ES, Flow, Regs, SI, lin, set_ret_flag};
use crate::interp::Cpu;
use crate::iso9660::IsoFs;
use crate::mem::GuestMem;

/// En-tête du pilote, dans la zone de données du DOS (0060:00C0).
pub const DEVICE_SEG: u16 = 0x0060;
pub const DEVICE_OFF: u16 = 0x00C0;

const ERR_INVALID_FUNCTION: u16 = 0x01;
const ERR_INVALID_DRIVE: u16 = 0x0F;

/// État des requêtes au pilote : fait, erreur (+ code), occupé.
const DONE: u16 = 0x0100;
const ERROR: u16 = 0x8000;
const ERR_UNKNOWN_UNIT: u16 = 0x01;
const ERR_UNKNOWN_COMMAND: u16 = 0x03;
const ERR_SECTOR_NOT_FOUND: u16 = 0x08;
const ERR_GENERAL: u16 = 0x0C;

pub struct Mscdex {
    /// Lettre du lecteur (0 = A:).
    pub drive: u8,
    pub root: PathBuf,
    /// Le disque a changé depuis la dernière question (IOCTL 9).
    changed: bool,
}

/// Adresse « Red Book » (trame, seconde, minute, 0) d'un secteur.
fn red_book(lba: u32) -> u32 {
    let f = lba + 150;
    u32::from_le_bytes([(f % 75) as u8, (f / 75 % 60) as u8, (f / 75 / 60) as u8, 0])
}

/// Secteur d'une adresse HSG (0) ou Red Book (1).
fn to_lba(mode: u8, addr: u32) -> u32 {
    if mode == 1 {
        let [f, s, m, _] = addr.to_le_bytes();
        (u32::from(m) * 60 + u32::from(s)) * 75 + u32::from(f) - 150
    } else {
        addr
    }
}

impl Mscdex {
    pub fn new(drive: u8, root: PathBuf) -> Self {
        Self { drive, root, changed: false }
    }

    fn iso(&self) -> Option<Arc<IsoFs>> {
        super::vfs::iso_at(&self.root)
    }

    /// En-tête du pilote : caractère, IOCTL ; nom « MSCD001 », une unité.
    pub fn install(&self, mem: &GuestMem) {
        let at = lin(DEVICE_SEG, DEVICE_OFF);
        mem.write_u32(at, 0xFFFF_FFFF);
        mem.write_u16(at + 4, 0xC800);
        // Stratégie et interruption : un RETF en fin d'en-tête.
        mem.write_u16(at + 6, DEVICE_OFF + 22);
        mem.write_u16(at + 8, DEVICE_OFF + 22);
        mem.write(at + 10, b"MSCD001 ");
        mem.write_u16(at + 18, 0);
        mem.write_u8(at + 20, self.drive + 1);
        mem.write_u8(at + 21, 1);
        mem.write_u8(at + 22, 0xCB);
    }

    /// Lit `count` secteurs de données à partir de `lba` dans la mémoire de l'invité.
    fn read_sectors(&self, mem: &GuestMem, lba: u32, count: u32, at: usize) -> Result<(), u16> {
        let iso = self.iso().ok_or(ERR_GENERAL)?;
        let mut buf = vec![0u8; SECTOR];
        for i in 0..count {
            iso.disc.read_data(lba + i, &mut buf).map_err(|_| ERR_SECTOR_NOT_FOUND)?;
            mem.write(at + i as usize * SECTOR, &buf);
        }
        Ok(())
    }

    /// INT 2Fh, AH=15h ; `None` pour les autres fonctions.
    pub fn int2f(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Option<Flow> {
        let ax = cpu.w(AX);
        if ax >> 8 != 0x15 {
            return None;
        }
        let es_bx = lin(cpu.sreg(ES), cpu.w(BX));
        let ours = cpu.w(CX) == u16::from(self.drive);
        let fail = |cpu: &mut Cpu, code: u16| {
            cpu.set_w(AX, code);
            Err(())
        };
        let r: Result<(), ()> = match ax {
            0x1500 => {
                cpu.set_w(BX, 1);
                cpu.set_w(CX, u16::from(self.drive));
                Ok(())
            }
            0x1501 => {
                mem.write_u8(es_bx, 0);
                mem.write_u16(es_bx + 1, DEVICE_OFF);
                mem.write_u16(es_bx + 3, DEVICE_SEG);
                Ok(())
            }
            // Fichiers de copyright, de résumé, bibliographique.
            0x1502..=0x1504 if ours => match self.iso() {
                Some(iso) => {
                    mem.write(es_bx, &iso.pvd_file_id(usize::from(ax - 0x1502)));
                    mem.write_u8(es_bx + 37, 0);
                    Ok(())
                }
                None => fail(cpu, ERR_INVALID_DRIVE),
            },
            // Descripteur de volume n° DX : AX = son type (FFh : terminal).
            0x1505 if ours => match self.read_sectors(mem, 16 + u32::from(cpu.w(DX)), 1, es_bx) {
                Ok(()) => {
                    cpu.set_w(AX, u16::from(mem.read_u8(es_bx)));
                    Ok(())
                }
                Err(_) => fail(cpu, ERR_INVALID_DRIVE),
            },
            // Lecture absolue : DX secteurs à partir de SI:DI.
            0x1508 if ours => {
                let lba = (u32::from(cpu.w(SI)) << 16) | u32::from(cpu.w(DI));
                match self.read_sectors(mem, lba, u32::from(cpu.w(DX)), es_bx) {
                    Ok(()) => Ok(()),
                    Err(_) => fail(cpu, ERR_INVALID_DRIVE),
                }
            }
            0x1509 if ours => fail(cpu, ERR_INVALID_FUNCTION),
            0x1502..=0x1509 => fail(cpu, ERR_INVALID_DRIVE),
            0x150B => {
                cpu.set_w(AX, if ours { 0x5AD8 } else { 0 });
                cpu.set_w(BX, 0xADAD);
                Ok(())
            }
            // Version 2.23.
            0x150C => {
                cpu.set_w(BX, 0x0217);
                Ok(())
            }
            0x150D => {
                mem.write_u8(es_bx, self.drive);
                Ok(())
            }
            // Préférence de descripteur : primaire.
            0x150E => {
                if cpu.w(BX) == 0 {
                    cpu.set_w(DX, 0x0100);
                }
                Ok(())
            }
            0x1510 if ours => {
                self.request(mem, es_bx);
                Ok(())
            }
            0x1510 => {
                mem.write_u16(es_bx + 3, DONE | ERROR | ERR_UNKNOWN_UNIT);
                Ok(())
            }
            _ => fail(cpu, ERR_INVALID_FUNCTION),
        };
        set_ret_flag(cpu, mem, CF, r.is_err());
        Some(Flow::Continue)
    }

    /// Requête au pilote (en-tête en `req`) : état écrit à req + 3.
    fn request(&mut self, mem: &GuestMem, req: usize) {
        let far = |at: usize| lin(mem.read_u16(at + 2), mem.read_u16(at));
        let status = match mem.read_u8(req + 2) {
            // IOCTL en entrée : bloc de contrôle à l'adresse de transfert.
            3 => self.ioctl_input(mem, far(req + 14)),
            // IOCTL en sortie (éjecter, verrouiller, réinitialiser, canaux
            // audio, fermer le tiroir), ouverture, fermeture, recherche,
            // préchargement : rien à faire.
            12 | 13 | 14 | 130 | 131 => Ok(()),
            // READ LONG : mode d'adressage, adresse, nombre, début, mode de lecture.
            128 => {
                let mode = mem.read_u8(req + 13);
                let count = u32::from(mem.read_u16(req + 18));
                let lba = to_lba(mode, mem.read_u32(req + 20));
                if mem.read_u8(req + 24) != 0 {
                    Err(ERR_UNKNOWN_COMMAND)
                } else {
                    self.read_sectors(mem, lba, count, far(req + 14))
                }
            }
            // Lecture audio : pas de piste audio (palier suivant).
            132 => Err(ERR_GENERAL),
            133 | 136 => Ok(()),
            _ => Err(ERR_UNKNOWN_COMMAND),
        };
        mem.write_u16(req + 3, match status {
            Ok(()) => DONE,
            Err(code) => DONE | ERROR | code,
        });
    }

    fn ioctl_input(&mut self, mem: &GuestMem, cb: usize) -> Result<(), u16> {
        let iso = self.iso();
        let sectors = iso.as_ref().map_or(0, |i| i.disc.sectors);
        match mem.read_u8(cb) {
            0 => {
                mem.write_u16(cb + 1, DEVICE_OFF);
                mem.write_u16(cb + 3, DEVICE_SEG);
            }
            // Position de la tête (HSG).
            1 => mem.write_u32(cb + 2, 0),
            // Canaux audio : entrée et volume des quatre sorties.
            4 => mem.write(cb + 1, &[0, 0xFF, 1, 0xFF, 2, 0, 3, 0]),
            // État : porte fermée, déverrouillée, lecture brute et cuite,
            // données et audio, HSG et Red Book ; bit 11 : pas de disque.
            6 => mem.write_u32(cb + 1, 0x0216 | if iso.is_none() { 0x800 } else { 0 }),
            7 => mem.write_u16(cb + 2, if mem.read_u8(cb + 1) == 1 { 2352 } else { SECTOR as u16 }),
            8 => mem.write_u32(cb + 1, sectors),
            9 => mem.write_u8(cb + 1, if std::mem::take(&mut self.changed) { 0xFF } else { 1 }),
            // Disque audio : première et dernière piste, début de la zone de fin.
            10 => {
                let iso = iso.ok_or(ERR_GENERAL)?;
                let t = &iso.disc.tracks;
                mem.write_u8(cb + 1, t.first().map_or(1, |t| t.number));
                mem.write_u8(cb + 2, t.last().map_or(1, |t| t.number));
                mem.write_u32(cb + 3, red_book(sectors));
            }
            // Piste : début (Red Book) et contrôle (40h : données).
            11 => {
                let iso = iso.ok_or(ERR_GENERAL)?;
                let n = mem.read_u8(cb + 1);
                let t = iso.disc.tracks.iter().find(|t| t.number == n).ok_or(ERR_SECTOR_NOT_FOUND)?;
                mem.write_u32(cb + 2, red_book(t.start));
                mem.write_u8(cb + 6, if t.audio { 0x00 } else { 0x40 });
            }
            // Sous-canal Q : position courante (aucune lecture audio).
            12 => mem.write(cb + 1, &[0x41, 1, 1, 0, 0, 0, 0, 0, 2, 0]),
            // État audio : ni pause, ni lecture.
            15 => {
                mem.write_u16(cb + 1, 0);
                mem.write_u32(cb + 3, 0);
                mem.write_u32(cb + 7, 0);
            }
            _ => return Err(ERR_UNKNOWN_COMMAND),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_book_addresses() {
        // Secteur 0 = 00:02:00 ; aller-retour.
        assert_eq!(red_book(0), 0x0000_0200);
        assert_eq!(to_lba(1, red_book(123_456)), 123_456);
        assert_eq!(to_lba(0, 77), 77);
    }
}
