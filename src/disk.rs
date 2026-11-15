// SPDX-License-Identifier: GPL-3.0-or-later
//! Disques sur images (INT 13h en HLE) : disquettes A: et B: (géométrie
//! déduite de la taille de l'image) et disque dur C: (géométrie lue dans la
//! table de partitions, sinon 16 têtes et 63 secteurs). Accès CHS (02h,
//! 03h, 08h) et LBA étendu (41h, 42h, 43h, 48h). Les écritures vont
//! directement dans le fichier image.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::hle::{AX, BX, CF, CX, DI, DS, DX, ES, Regs, SI, lin, set_ret_flag};
use crate::interp::Cpu;
use crate::mem::GuestMem;

pub const SECTOR: usize = 512;

pub struct Disk {
    file: File,
    pub sectors: u64,
    pub cylinders: u32,
    pub heads: u32,
    pub spt: u32,
    pub floppy: bool,
    readonly: bool,
}

/// Formats de disquette : (taille en Ko, cylindres, têtes, secteurs, type BIOS).
const FLOPPIES: [(u64, u32, u32, u32, u8); 6] =
    [(160, 40, 1, 8, 1), (180, 40, 1, 9, 1), (360, 40, 2, 9, 1), (720, 80, 2, 9, 3), (1200, 80, 2, 15, 2), (1440, 80, 2, 18, 4)];
const FLOPPY_288: (u64, u32, u32, u32, u8) = (2880, 80, 2, 36, 5);

impl Disk {
    pub fn open(path: &Path, floppy: bool) -> Result<Self, String> {
        let err = |e: std::io::Error| format!("{} : {e}", path.display());
        let (file, readonly) = match File::options().read(true).write(true).open(path) {
            Ok(f) => (f, false),
            Err(_) => (File::open(path).map_err(err)?, true),
        };
        let size = file.metadata().map_err(err)?.len();
        let sectors = size / SECTOR as u64;
        let mut d = Self { file, sectors, cylinders: 0, heads: 0, spt: 0, floppy, readonly };
        if floppy {
            let kb = size / 1024;
            let &(_, c, h, s, _) = FLOPPIES
                .iter()
                .chain(std::iter::once(&FLOPPY_288))
                .find(|f| f.0 == kb)
                .ok_or(format!("{}: unknown floppy size ({kb} KB)", path.display()))?;
            (d.cylinders, d.heads, d.spt) = (c, h, s);
        } else {
            let (h, s) = d.mbr_geometry().unwrap_or((16, 63));
            d.heads = h;
            d.spt = s;
            d.cylinders = (sectors / u64::from(h * s)).clamp(1, 1024) as u32;
        }
        Ok(d)
    }

    /// Têtes et secteurs par piste d'après la fin CHS de la première
    /// partition (comme les BIOS qui « traduisent » selon le partitionnement).
    fn mbr_geometry(&mut self) -> Option<(u32, u32)> {
        let mut mbr = [0u8; SECTOR];
        self.read(0, &mut mbr).ok()?;
        if mbr[510..512] != [0x55, 0xAA] {
            return None;
        }
        (0..4).map(|i| 0x1BE + i * 16).find_map(|e| {
            let (end_head, end_sector) = (u32::from(mbr[e + 5]), u32::from(mbr[e + 6] & 0x3F));
            (mbr[e + 4] != 0 && end_sector != 0).then_some((end_head + 1, end_sector))
        })
    }

    /// Type BIOS de la disquette (INT 13h AH=08h, BL).
    fn floppy_type(&self) -> u8 {
        let kb = self.sectors / 2;
        FLOPPIES.iter().chain(std::iter::once(&FLOPPY_288)).find(|f| f.0 == kb).map_or(4, |f| f.4)
    }

    pub fn read(&mut self, lba: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(lba * SECTOR as u64))?;
        self.file.read_exact(buf)
    }

    fn write(&mut self, lba: u64, buf: &[u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(lba * SECTOR as u64))?;
        self.file.write_all(buf)
    }

    fn chs_to_lba(&self, c: u32, h: u32, s: u32) -> Option<u64> {
        (s >= 1 && s <= self.spt && h < self.heads).then(|| u64::from((c * self.heads + h) * self.spt + s - 1))
    }
}

/// Lecteurs : A:, B: (00h, 01h), C: (80h).
#[derive(Default)]
pub struct Disks {
    pub floppies: [Option<Disk>; 2],
    pub hard: Option<Disk>,
    last_status: u8,
    /// Images de A: à faire défiler (`--fda` répété), et la courante.
    a_images: Vec<std::path::PathBuf>,
    a_index: usize,
    /// Disquette changée depuis la dernière consultation (INT 13h AH=16h).
    changed: [bool; 2],
}

impl Disks {
    fn get(&mut self, drive: u8) -> Option<&mut Disk> {
        match drive {
            0 | 1 => self.floppies[usize::from(drive)].as_mut(),
            0x80 => self.hard.as_mut(),
            _ => None,
        }
    }

    /// Ouvre les images de A:, B: et C: (la première de chaque liste ;
    /// celles de A: défilent ensuite avec `next_floppy`).
    pub fn open(paths: &[Vec<std::path::PathBuf>; 3]) -> Result<Self, String> {
        let mut d = Self::default();
        for (i, list) in paths.iter().enumerate() {
            for p in list {
                Disk::open(p, i < 2)?;
            }
            if let Some(p) = list.first() {
                let disk = Disk::open(p, i < 2)?;
                if i < 2 { d.floppies[i] = Some(disk) } else { d.hard = Some(disk) }
            }
        }
        d.a_images = paths[0].clone();
        Ok(d)
    }

    /// Met l'image `path` dans A: (choisie dans le menu).
    pub fn insert_floppy(&mut self, path: &std::path::Path) -> Result<(), String> {
        let d = Disk::open(path, true)?;
        self.floppies[0] = Some(d);
        self.changed[0] = true;
        if let Some(i) = self.a_images.iter().position(|p| p == path) {
            self.a_index = i;
        }
        eprintln!("[disk] A: = {}", path.display());
        Ok(())
    }

    /// Change la disquette de A: pour l'image suivante de la liste.
    pub fn next_floppy(&mut self) {
        if self.a_images.len() < 2 {
            eprintln!("[disk] a single image for A: (repeat --fda to give several)");
            return;
        }
        self.a_index = (self.a_index + 1) % self.a_images.len();
        let path = &self.a_images[self.a_index];
        match Disk::open(path, true) {
            Ok(d) => {
                self.floppies[0] = Some(d);
                self.changed[0] = true;
                eprintln!("[disk] A: = {}", path.display());
            }
            Err(e) => eprintln!("[disk] {e}"),
        }
    }

    /// Secteur d'amorce du lecteur (signature 55AAh vérifiée).
    pub fn boot_sector(&mut self, drive: u8) -> Result<[u8; SECTOR], String> {
        let d = self.get(drive).ok_or(format!("drive {drive:02X}h absent"))?;
        let mut s = [0u8; SECTOR];
        d.read(0, &mut s).map_err(|e| format!("boot sector: {e}"))?;
        if s[510..] != [0x55, 0xAA] {
            return Err(format!("drive {drive:02X}h not bootable (no 55AAh signature)"));
        }
        Ok(s)
    }

    pub fn floppy_count(&self) -> u8 {
        self.floppies.iter().filter(|d| d.is_some()).count() as u8
    }

    /// Transfert de `count` secteurs entre l'image et la mémoire.
    fn transfer(&mut self, drive: u8, lba: u64, count: usize, addr: usize, write: bool, mem: &GuestMem) -> Result<(), u8> {
        let d = self.get(drive).ok_or(0x01u8)?;
        if lba + count as u64 > d.sectors {
            return Err(0x04);
        }
        if write && d.readonly {
            return Err(0x03);
        }
        let mut buf = vec![0u8; count * SECTOR];
        if write {
            mem.read(addr, &mut buf);
            d.write(lba, &buf).map_err(|_| 0x04u8)?;
        } else {
            d.read(lba, &mut buf).map_err(|_| 0x04u8)?;
            mem.write(addr, &buf);
        }
        Ok(())
    }

    /// INT 13h. Statut en AH, CF en cas d'erreur.
    pub fn int13(&mut self, cpu: &mut Cpu, mem: &GuestMem) {
        let (ah, al, drive) = (cpu.hi(AX), cpu.lo(AX), cpu.lo(DX));
        let r: Result<(), u8> = match ah {
            0x00 => Ok(()),
            0x01 => {
                cpu.set_lo(AX, self.last_status);
                Ok(())
            }
            0x02 | 0x03 | 0x04 => {
                let (cx, dh) = (cpu.w(CX), u32::from(cpu.hi(DX)));
                let c = u32::from(cx >> 8) | (u32::from(cx & 0xC0) << 2);
                let s = u32::from(cx & 0x3F);
                match self.get(drive).map(|d| d.chs_to_lba(c, dh, s)) {
                    None => Err(0x01),
                    Some(None) => Err(0x04),
                    Some(Some(lba)) if ah == 0x04 => {
                        let _ = lba;
                        Ok(())
                    }
                    Some(Some(lba)) => {
                        let addr = lin(cpu.sreg(ES), cpu.w(BX));
                        let r = self.transfer(drive, lba, usize::from(al), addr, ah == 0x03, mem);
                        if r.is_err() {
                            cpu.set_lo(AX, 0);
                        }
                        r
                    }
                }
            }
            0x05 => Ok(()),
            0x08 => {
                let floppies = self.floppy_count();
                let hard = u8::from(self.hard.is_some());
                match self.get(drive) {
                    Some(d) => {
                        let maxc = d.cylinders - 1;
                        cpu.set_w(CX, (((maxc & 0xFF) as u16) << 8) | (((maxc >> 2) & 0xC0) as u16) | d.spt as u16);
                        cpu.set_hi(DX, (d.heads - 1) as u8);
                        if d.floppy {
                            cpu.set_lo(BX, d.floppy_type());
                            cpu.set_lo(DX, floppies);
                            cpu.set_sreg(ES, crate::bios::ROM_SEG);
                            cpu.set_w(DI, crate::bios::DISKETTE_PARAMS);
                        } else {
                            cpu.set_lo(DX, hard);
                        }
                        cpu.set_lo(AX, 0);
                        Ok(())
                    }
                    None => Err(0x01),
                }
            }
            0x09..=0x14 => Ok(()),
            // Type de lecteur : disquette avec ligne de changement (2), disque dur (3, secteurs en CX:DX).
            0x15 => match self.get(drive) {
                Some(d) if d.floppy => {
                    cpu.set_hi(AX, 2);
                    set_ret_flag(cpu, mem, CF, false);
                    return;
                }
                Some(d) => {
                    let n = d.sectors.min(0xFFFF_FFFF) as u32;
                    cpu.set_w(CX, (n >> 16) as u16);
                    cpu.set_w(DX, n as u16);
                    cpu.set_hi(AX, 3);
                    set_ret_flag(cpu, mem, CF, false);
                    return;
                }
                None => {
                    cpu.set_hi(AX, 0);
                    set_ret_flag(cpu, mem, CF, false);
                    return;
                }
            },
            // Ligne de changement de disquette : 06h une fois après un changement.
            0x16 => match drive {
                0 | 1 if std::mem::take(&mut self.changed[usize::from(drive)]) => Err(0x06),
                _ => Ok(()),
            },
            // Extensions (disque dur) : version 2.1, accès étendus.
            0x41 if cpu.w(BX) == 0x55AA && drive >= 0x80 && self.hard.is_some() => {
                cpu.set_w(BX, 0xAA55);
                cpu.set_hi(AX, 0x21);
                cpu.set_w(CX, 0x0001);
                set_ret_flag(cpu, mem, CF, false);
                return;
            }
            0x42 | 0x43 => {
                let p = lin(cpu.sreg(DS), cpu.w(SI));
                let count = usize::from(mem.read_u16(p + 2));
                let addr = lin(mem.read_u16(p + 6), mem.read_u16(p + 4));
                let lba = u64::from(mem.read_u32(p + 8)) | (u64::from(mem.read_u32(p + 12)) << 32);
                self.transfer(drive, lba, count, addr, ah == 0x43, mem)
            }
            0x48 => match self.get(drive) {
                Some(d) => {
                    let p = lin(cpu.sreg(DS), cpu.w(SI));
                    mem.write_u16(p, 0x1A);
                    mem.write_u16(p + 2, 0x0002);
                    mem.write_u32(p + 4, d.cylinders);
                    mem.write_u32(p + 8, d.heads);
                    mem.write_u32(p + 12, d.spt);
                    mem.write_u32(p + 16, d.sectors as u32);
                    mem.write_u32(p + 20, (d.sectors >> 32) as u32);
                    mem.write_u16(p + 24, SECTOR as u16);
                    Ok(())
                }
                None => Err(0x01),
            },
            _ => Err(0x01),
        };
        let status = r.err().unwrap_or(0);
        self.last_status = status;
        cpu.set_hi(AX, status);
        set_ret_flag(cpu, mem, CF, status != 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floppy_chs_read_write() {
        let dir = std::env::temp_dir().join(format!("bare86-disk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.img");
        let mut img = vec![0u8; 1440 * 1024];
        img[512 * 19] = 0x42; // cylindre 0, tête 1, secteur 2 : LBA 19
        std::fs::write(&path, &img).unwrap();
        let mut disks = Disks::default();
        disks.floppies[0] = Some(Disk::open(&path, true).unwrap());
        let mem = GuestMem::new(0x10_0000).unwrap();
        let mut cpu = Cpu::real_mode(0, 0, 0);
        // AH=02h : 1 secteur, C=0 S=2 H=1 vers 0000:1000.
        cpu.set_w(AX, 0x0201);
        cpu.set_w(CX, 0x0002);
        cpu.set_w(DX, 0x0100);
        cpu.set_w(BX, 0x1000);
        disks.int13(&mut cpu, &mem);
        assert_eq!((cpu.hi(AX), mem.read_u8(0x1000)), (0, 0x42));
        // AH=08h : 80 cylindres, 18 secteurs, 2 têtes, type 4.
        cpu.set_w(AX, 0x0800);
        cpu.set_w(DX, 0x0000);
        disks.int13(&mut cpu, &mem);
        assert_eq!((cpu.w(CX), cpu.hi(DX), cpu.lo(BX)), (0x4F12, 1, 4));
        // Écriture puis relecture dans le fichier.
        mem.write_u8(0x2000, 0x99);
        cpu.set_sreg(ES, 0); // AH=08h l'a placé sur la table en ROM
        cpu.set_w(AX, 0x0301);
        cpu.set_w(CX, 0x0101);
        cpu.set_w(DX, 0x0000);
        cpu.set_w(BX, 0x2000);
        disks.int13(&mut cpu, &mem);
        assert_eq!(cpu.hi(AX), 0);
        drop(disks);
        assert_eq!(std::fs::read(&path).unwrap()[512 * 36], 0x99);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
