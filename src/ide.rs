// SPDX-License-Identifier: GPL-3.0-or-later
//! Canal IDE secondaire (170h–177h, 376h, IRQ 15) avec un lecteur de CD
//! ATAPI en maître ; pas d'esclave. Transferts en PIO seulement (le
//! lecteur n'annonce pas de DMA). Les commandes s'achèvent tout de suite :
//! BSY n'est jamais vu levé.
//!
//! Protocole PACKET : la commande A0h met DRQ avec « commande » (CoD) dans
//! la raison d'interruption ; l'hôte écrit les 12 octets du paquet, puis
//! lit les données par morceaux d'au plus la limite d'octets donnée dans
//! les registres de cylindre (une IRQ par morceau), et l'état final.

use crate::cdrom::{Drive, Reply, Sense};
use crate::io::IoDevice;
use std::cell::RefCell;
use std::rc::Rc;

pub const BASE: u16 = 0x170;
pub const CONTROL: u16 = 0x376;
pub const IRQ: u8 = 15;

const BSY: u8 = 0x80;
const DRDY: u8 = 0x40;
const DSC: u8 = 0x10;
const DRQ: u8 = 0x08;
const ERR: u8 = 0x01;
const ABRT: u8 = 0x04;
/// Raison d'interruption (registre de nombre de secteurs) : CoD, IO.
const COD: u8 = 0x01;
const IO: u8 = 0x02;
/// Registre de contrôle : nIEN, SRST.
const NIEN: u8 = 0x02;
const SRST: u8 = 0x04;

#[derive(PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    /// Attente des 12 octets du paquet.
    Packet,
    /// Données vers l'hôte : morceau en cours, puis le reste.
    DataIn,
    /// Données venant de l'hôte (ignorées).
    DataOut,
    /// IDENTIFY PACKET DEVICE (une seule transmission de 512 octets).
    Identify,
}

pub struct Ide {
    drive: Drive,
    error: u8,
    /// Registre des fonctions (écrit au port 1, distinct de l'erreur lue).
    features: u8,
    count: u8,
    sector: u8,
    cyl: u16,
    select: u8,
    status: u8,
    control: u8,
    phase: Phase,
    /// Tampon du morceau en cours, et position.
    buf: Vec<u8>,
    pos: usize,
    /// Données encore à transmettre après ce morceau.
    rest: Vec<u8>,
    /// Limite d'octets par morceau (registres de cylindre au paquet).
    limit: usize,
    irq_line: bool,
    edge: bool,
}

impl Ide {
    pub fn new(drive: Drive) -> Self {
        let mut ide = Self {
            drive,
            error: 0,
            features: 0,
            count: 0,
            sector: 0,
            cyl: 0,
            select: 0,
            status: 0,
            control: 0,
            phase: Phase::Idle,
            buf: Vec::new(),
            pos: 0,
            rest: Vec::new(),
            limit: 0,
            irq_line: false,
            edge: false,
        };
        ide.reset();
        ide
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    pub fn drive_mut(&mut self) -> &mut Drive {
        &mut self.drive
    }

    /// Signature d'un périphérique ATAPI (cylindre EB14h), diagnostic réussi.
    fn signature(&mut self) {
        self.count = 1;
        self.sector = 1;
        self.cyl = 0xEB14;
        self.select &= 0x10;
        self.error = 0x01;
    }

    fn reset(&mut self) {
        self.signature();
        self.status = 0;
        self.phase = Phase::Idle;
        self.buf.clear();
        self.rest.clear();
        self.set_irq(false);
    }

    fn slave_selected(&self) -> bool {
        self.select & 0x10 != 0
    }

    fn set_irq(&mut self, on: bool) {
        let line = on && self.control & NIEN == 0;
        if line && !self.irq_line {
            self.edge = true;
        }
        self.irq_line = on;
    }

    /// Front montant de INTRQ depuis le dernier appel.
    pub fn take_irq(&mut self) -> bool {
        std::mem::take(&mut self.edge)
    }

    fn abort(&mut self) {
        self.error = ABRT;
        self.status = DRDY | ERR;
        self.phase = Phase::Idle;
        self.set_irq(true);
    }

    fn command(&mut self, cmd: u8) {
        if self.slave_selected() {
            return;
        }
        self.set_irq(false);
        match cmd {
            // DEVICE RESET
            0x08 => {
                self.reset();
            }
            // EXECUTE DEVICE DIAGNOSTIC
            0x90 => {
                self.signature();
                self.status = DRDY | DSC;
                self.set_irq(true);
            }
            // IDENTIFY DEVICE : refusé avec la signature ATAPI, comme il se doit.
            0xEC => {
                self.signature();
                self.abort();
            }
            0xA1 => {
                self.buf = identify();
                self.pos = 0;
                self.phase = Phase::Identify;
                self.status = DRDY | DSC | DRQ;
                self.count = IO;
                self.set_irq(true);
            }
            0xA0 => {
                // DMA demandé (bit 0 des fonctions) : non pris en charge.
                if self.features & 1 != 0 {
                    return self.abort();
                }
                self.limit = match usize::from(self.cyl) & !1 {
                    0 => 0xFFFE,
                    n => n,
                };
                self.buf.clear();
                self.pos = 0;
                self.phase = Phase::Packet;
                self.count = COD;
                self.status = DRDY | DSC | DRQ;
            }
            // SET FEATURES, IDLE, STANDBY, SLEEP, FLUSH : sans effet.
            0xEF | 0xE0 | 0xE1 | 0xE3 | 0xE6 | 0xE7 => {
                self.status = DRDY | DSC;
                self.set_irq(true);
            }
            // CHECK POWER MODE : actif.
            0xE5 => {
                self.count = 0xFF;
                self.status = DRDY | DSC;
                self.set_irq(true);
            }
            _ => self.abort(),
        }
    }

    /// Paquet complet : exécution, puis données ou état final.
    fn packet(&mut self) {
        let mut cmd = [0u8; 12];
        cmd.copy_from_slice(&self.buf[..12]);
        match self.drive.command(&cmd) {
            Ok(Reply::DataIn(data)) => {
                self.rest = data;
                self.next_chunk();
            }
            Ok(Reply::DataOut(n)) => {
                self.buf = vec![0; n];
                self.pos = 0;
                self.phase = Phase::DataOut;
                self.set_byte_count(n.min(self.limit));
                self.count = 0;
                self.status = DRDY | DSC | DRQ;
                self.set_irq(true);
            }
            Ok(Reply::Done) => self.complete(Sense::NONE),
            Err(s) => self.complete(s),
        }
    }

    fn set_byte_count(&mut self, n: usize) {
        self.cyl = n as u16;
    }

    /// Morceau suivant des données vers l'hôte, ou état final.
    fn next_chunk(&mut self) {
        if self.rest.is_empty() {
            return self.complete(Sense::NONE);
        }
        let n = self.rest.len().min(self.limit);
        self.buf = self.rest.drain(..n).collect();
        self.pos = 0;
        self.phase = Phase::DataIn;
        self.set_byte_count(n);
        self.count = IO;
        self.status = DRDY | DSC | DRQ;
        // Une impulsion par bloc, même si l'état n'a pas été lu.
        self.set_irq(false);
        self.set_irq(true);
    }

    fn complete(&mut self, sense: Sense) {
        self.phase = Phase::Idle;
        self.buf.clear();
        self.rest.clear();
        self.count = IO | COD;
        if sense == Sense::NONE {
            self.error = 0;
            self.status = DRDY | DSC;
        } else {
            self.error = sense.key << 4;
            self.status = DRDY | DSC | ERR;
        }
        self.set_irq(false);
        self.set_irq(true);
    }

    fn read_data(&mut self, size: u8) -> u32 {
        if self.slave_selected() || !matches!(self.phase, Phase::DataIn | Phase::Identify) {
            return 0xFFFF_FFFF >> (32 - 8 * u32::from(size));
        }
        let mut v = 0u32;
        for i in 0..usize::from(size) {
            v |= u32::from(self.buf.get(self.pos + i).copied().unwrap_or(0)) << (8 * i);
        }
        self.pos += usize::from(size);
        if self.pos >= self.buf.len() {
            match self.phase {
                Phase::Identify => {
                    self.phase = Phase::Idle;
                    self.status = DRDY | DSC;
                }
                _ => self.next_chunk(),
            }
        }
        v
    }

    fn write_data(&mut self, size: u8, value: u32) {
        if self.slave_selected() {
            return;
        }
        match self.phase {
            Phase::Packet => {
                for i in 0..usize::from(size) {
                    self.buf.push((value >> (8 * i)) as u8);
                }
                if self.buf.len() >= 12 {
                    self.status = DRDY | DSC | BSY;
                    self.packet();
                }
            }
            Phase::DataOut => {
                self.pos += usize::from(size);
                if self.pos >= self.buf.len() {
                    self.complete(Sense::NONE);
                }
            }
            _ => {}
        }
    }

    pub fn read_port(&mut self, port: u16, size: u8) -> u32 {
        if port == CONTROL {
            return u32::from(if self.slave_selected() { 0 } else { self.status });
        }
        let reg = port - BASE;
        if reg == 0 {
            return self.read_data(size);
        }
        // Esclave absent : registres à 0 (le maître ne répond pas pour lui).
        if self.slave_selected() && reg != 6 {
            return 0;
        }
        u32::from(match reg {
            1 => self.error,
            2 => self.count,
            3 => self.sector,
            4 => self.cyl as u8,
            5 => (self.cyl >> 8) as u8,
            6 => self.select | 0xA0,
            _ => {
                // Lire l'état acquitte l'interruption.
                self.set_irq(false);
                self.status
            }
        })
    }

    pub fn write_port(&mut self, port: u16, size: u8, value: u32) {
        let v = value as u8;
        if port == CONTROL {
            let was_reset = self.control & SRST != 0;
            self.control = v;
            if v & SRST != 0 {
                self.status = BSY;
            } else if was_reset {
                self.reset();
            }
            return;
        }
        match port - BASE {
            0 => self.write_data(size, value),
            1 => self.features = v,
            2 => self.count = v,
            3 => self.sector = v,
            4 => self.cyl = (self.cyl & 0xFF00) | u16::from(v),
            5 => self.cyl = (self.cyl & 0x00FF) | (u16::from(v) << 8),
            6 => self.select = v & 0x5F,
            _ => self.command(v),
        }
    }
}

/// IDENTIFY PACKET DEVICE : CD-ROM ATAPI amovible, paquets de 12 octets,
/// DRQ rapide (50 µs, sans IRQ avant le paquet), LBA, PIO 0–4, sans DMA.
fn identify() -> Vec<u8> {
    let mut w = [0u16; 256];
    w[0] = (2 << 14) | (5 << 8) | (1 << 7) | (2 << 5);
    let mut text = |at: usize, len: usize, s: &str| {
        let mut b = s.as_bytes().to_vec();
        b.resize(len * 2, b' ');
        for i in 0..len {
            w[at + i] = u16::from_be_bytes([b[2 * i], b[2 * i + 1]]);
        }
    };
    text(10, 10, "BARE86-0001");
    text(23, 4, "1.0");
    text(27, 20, Drive::model());
    w[49] = 1 << 9;
    w[53] = 0x0006;
    w[64] = 0x0003;
    w[65..=68].copy_from_slice(&[120, 120, 120, 120]);
    w[80] = 0x007E;
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Ports 170h–177h et 376h.
pub struct IdePorts(pub Rc<RefCell<Ide>>);

impl IoDevice for IdePorts {
    fn read(&mut self, port: u16, size: u8) -> u32 {
        self.0.borrow_mut().read_port(port, size)
    }

    fn write(&mut self, port: u16, size: u8, value: u32) {
        self.0.borrow_mut().write_port(port, size, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdrom::{Disc, SECTOR};

    fn ide() -> Ide {
        let path = crate::cdrom::tests::iso(40);
        let disc = Disc::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        Ide::new(crate::cdrom::tests::drive(Some(disc)))
    }

    fn send_packet(ide: &mut Ide, limit: u16, pkt: &[u8]) {
        ide.write_port(BASE + 4, 1, u32::from(limit & 0xFF));
        ide.write_port(BASE + 5, 1, u32::from(limit >> 8));
        ide.write_port(BASE + 7, 1, 0xA0);
        assert_eq!(ide.read_port(CONTROL, 1) as u8 & DRQ, DRQ);
        assert_eq!(ide.read_port(BASE + 2, 1), u32::from(COD));
        let mut p = [0u8; 12];
        p[..pkt.len()].copy_from_slice(pkt);
        for w in p.chunks(2) {
            ide.write_port(BASE, 2, u32::from(u16::from_le_bytes([w[0], w[1]])));
        }
    }

    fn read_chunk(ide: &mut Ide) -> Vec<u8> {
        let n = usize::from(ide.cyl);
        (0..n / 2).flat_map(|_| (ide.read_port(BASE, 2) as u16).to_le_bytes()).collect()
    }

    #[test]
    fn signature_and_identify() {
        let mut ide = ide();
        assert_eq!((ide.read_port(BASE + 4, 1), ide.read_port(BASE + 5, 1)), (0x14, 0xEB));
        ide.write_port(BASE + 7, 1, 0xEC);
        assert_eq!(ide.read_port(BASE + 7, 1) as u8, DRDY | ERR);
        ide.write_port(BASE + 7, 1, 0xA1);
        assert!(ide.take_irq());
        let id: Vec<u8> = (0..256).flat_map(|_| (ide.read_port(BASE, 2) as u16).to_le_bytes()).collect();
        assert_eq!(u16::from_le_bytes([id[0], id[1]]), 0x85C0);
        assert_eq!(&id[54..60], b"ABER68"); // « BARE86 », octets permutés par mot
        assert_eq!(ide.read_port(BASE + 7, 1) as u8 & DRQ, 0);
        // Esclave absent : état à 0.
        ide.write_port(BASE + 6, 1, 0xB0);
        assert_eq!(ide.read_port(BASE + 7, 1), 0);
    }

    #[test]
    fn read10_in_chunks() {
        let mut ide = ide();
        // Trois secteurs, morceaux de 4 Ko au plus : 4096 puis 2048.
        send_packet(&mut ide, 4096, &[0x28, 0, 0, 0, 0, 17, 0, 0, 3]);
        assert!(ide.take_irq());
        assert_eq!(ide.read_port(BASE + 2, 1), u32::from(IO));
        let a = read_chunk(&mut ide);
        assert!(ide.take_irq());
        let b = read_chunk(&mut ide);
        assert_eq!((a.len(), b.len()), (2 * SECTOR, SECTOR));
        assert_eq!((a[0], a[SECTOR], b[0]), (17, 18, 19));
        assert_eq!(ide.read_port(BASE + 2, 1), u32::from(IO | COD));
        assert_eq!(ide.read_port(BASE + 7, 1) as u8, DRDY | DSC);
        // Hors du disque : erreur, clé 5 dans le registre d'erreur.
        send_packet(&mut ide, 2048, &[0x28, 0, 0, 0, 0, 50, 0, 0, 1]);
        assert_eq!(ide.read_port(BASE + 7, 1) as u8 & ERR, ERR);
        assert_eq!(ide.read_port(BASE + 1, 1), 0x50);
    }
}
