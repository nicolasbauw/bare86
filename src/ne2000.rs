// SPDX-License-Identifier: GPL-3.0-or-later
//! Carte réseau NE2000 ISA (contrôleur DP8390) en 300h, IRQ 3 : 32 Ko de
//! mémoire tampon (pages 40h–BFh), PROM de l'adresse MAC, DMA distant par le
//! port de données (base + 10h), remise à zéro en lisant base + 1Fh.
//! Comportement calqué sur celui de la NE2000 de QEMU, servie par les mêmes
//! pilotes (packet driver Crynwr `NE2000.COM`).
//!
//! La carte ne connaît pas le réseau : les trames émises s'accumulent dans
//! `outbox`, les trames à recevoir attendent dans `inbox` que l'anneau de
//! réception ait la place.

use crate::io::IoDevice;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

pub const BASE: u16 = 0x300;
pub const IRQ: u8 = 3;
/// Adresse MAC (préfixe 52:54:00 de QEMU, réservé aux machines virtuelles).
pub const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

const MEM_SIZE: usize = 0xC000;
const RAM_START: usize = 0x4000;
/// Trame Ethernet la plus longue, sans CRC.
const MAX_FRAME: usize = 1514;
const MIN_FRAME: usize = 60;
/// Trames en attente au plus (au-delà, les plus anciennes sont perdues).
const INBOX_MAX: usize = 256;

// Registre de commande (CR).
const CR_STOP: u8 = 0x01;
const CR_TRANS: u8 = 0x04;
const CR_RREAD: u8 = 0x08;
const CR_RWRITE: u8 = 0x10;
const CR_NODMA: u8 = 0x20;

// Registre d'état des interruptions (ISR).
const ISR_RX: u8 = 0x01;
const ISR_TX: u8 = 0x02;
const ISR_RDC: u8 = 0x40;
const ISR_RESET: u8 = 0x80;

const RSR_RXOK: u8 = 0x01;
const RSR_PHY: u8 = 0x20;
const TSR_PTX: u8 = 0x01;
const DCR_WTS: u8 = 0x01;
const RCR_AB: u8 = 0x04;
const RCR_AM: u8 = 0x08;
const RCR_PRO: u8 = 0x10;

pub struct Ne2000 {
    cmd: u8,
    /// Début et fin de l'anneau de réception (octets).
    start: usize,
    stop: usize,
    boundary: u8,
    tsr: u8,
    tpsr: u8,
    tcnt: u16,
    rcnt: u16,
    rsar: u16,
    rsr: u8,
    rxcr: u8,
    txcr: u8,
    isr: u8,
    imr: u8,
    dcr: u8,
    phys: [u8; 6],
    curpag: u8,
    mult: [u8; 8],
    mem: Box<[u8; MEM_SIZE]>,
    line: bool,
    edge: bool,
    /// Trames émises par l'invité, à envoyer.
    pub outbox: Vec<Vec<u8>>,
    /// Trames arrivées, à placer dans l'anneau.
    inbox: VecDeque<Vec<u8>>,
    pub sent: u64,
    pub received: u64,
}

impl Ne2000 {
    pub fn new() -> Self {
        let mut n = Self {
            cmd: CR_STOP | CR_NODMA,
            start: 0,
            stop: 0,
            boundary: 0,
            tsr: 0,
            tpsr: 0,
            tcnt: 0,
            rcnt: 0,
            rsar: 0,
            rsr: 0,
            rxcr: 0,
            txcr: 0,
            isr: 0,
            imr: 0,
            dcr: 0,
            phys: [0; 6],
            curpag: 0,
            mult: [0; 8],
            mem: Box::new([0; MEM_SIZE]),
            line: false,
            edge: false,
            outbox: Vec::new(),
            inbox: VecDeque::new(),
            sent: 0,
            received: 0,
        };
        n.reset();
        n
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    /// PROM : l'adresse MAC puis la signature « WW » (carte 16 bits), chaque
    /// octet doublé comme le lit un pilote en mode mot.
    fn reset(&mut self) {
        self.isr = ISR_RESET;
        self.cmd = CR_STOP | CR_NODMA;
        let mut prom = [0u8; 16];
        prom[..6].copy_from_slice(&MAC);
        prom[14] = 0x57;
        prom[15] = 0x57;
        for (i, b) in prom.iter().enumerate() {
            self.mem[2 * i] = *b;
            self.mem[2 * i + 1] = *b;
        }
        self.update_irq();
    }

    fn update_irq(&mut self) {
        let line = self.isr & self.imr & 0x7F != 0;
        if line && !self.line {
            self.edge = true;
        }
        self.line = line;
    }

    /// Front montant de la ligne d'interruption depuis le dernier appel.
    pub fn take_irq(&mut self) -> bool {
        std::mem::take(&mut self.edge)
    }

    /// Trame venue du réseau.
    pub fn deliver(&mut self, frame: Vec<u8>) {
        if self.inbox.len() >= INBOX_MAX {
            self.inbox.pop_front();
        }
        self.inbox.push_back(frame);
        self.drain_inbox();
    }

    /// Place les trames en attente tant que l'anneau a la place.
    pub fn drain_inbox(&mut self) {
        while !self.inbox.is_empty() {
            if self.cmd & CR_STOP != 0 || self.buffer_full() {
                return;
            }
            let f = self.inbox.pop_front().unwrap_or_default();
            self.receive(&f);
        }
    }

    fn buffer_full(&self) -> bool {
        if self.stop <= self.start {
            return true;
        }
        let index = usize::from(self.curpag) << 8;
        let boundary = usize::from(self.boundary) << 8;
        let avail = if index < boundary { boundary - index } else { (self.stop - self.start).saturating_sub(index - boundary) };
        avail < MAX_FRAME + 4
    }

    fn accepts(&self, frame: &[u8]) -> bool {
        if self.rxcr & RCR_PRO != 0 {
            return true;
        }
        let dest = &frame[..6];
        if dest == [0xFF; 6] {
            self.rxcr & RCR_AB != 0
        } else if dest[0] & 1 != 0 {
            let h = multicast_hash(dest);
            self.rxcr & RCR_AM != 0 && self.mult[usize::from(h >> 3)] & (1 << (h & 7)) != 0
        } else {
            dest == self.phys
        }
    }

    fn receive(&mut self, frame: &[u8]) {
        if frame.len() < 6 || frame.len() > MAX_FRAME || !self.accepts(frame) {
            return;
        }
        let mut buf = frame.to_vec();
        buf.resize(buf.len().max(MIN_FRAME), 0);
        let mut index = usize::from(self.curpag) << 8;
        if index >= MEM_SIZE || index < self.start || index >= self.stop {
            index = self.start;
        }
        let total = buf.len() + 4;
        let mut next = index + ((total + 4 + 255) & !0xFF);
        if next >= self.stop {
            next -= self.stop - self.start;
        }
        self.rsr = RSR_RXOK;
        if buf[0] & 1 != 0 {
            self.rsr |= RSR_PHY;
        }
        let header = [self.rsr, (next >> 8) as u8, total as u8, (total >> 8) as u8];
        self.mem[index..index + 4].copy_from_slice(&header);
        index += 4;
        let mut data = &buf[..];
        while !data.is_empty() {
            let n = data.len().min(self.stop.saturating_sub(index));
            self.mem[index..index + n].copy_from_slice(&data[..n]);
            data = &data[n..];
            index += n;
            if index >= self.stop {
                index = self.start;
            }
        }
        self.curpag = (next >> 8) as u8;
        self.received += 1;
        self.isr |= ISR_RX;
        self.update_irq();
    }

    fn write_cmd(&mut self, val: u8) {
        self.cmd = val;
        if val & CR_STOP != 0 {
            return;
        }
        self.isr &= !ISR_RESET;
        if val & (CR_RREAD | CR_RWRITE) != 0 && self.rcnt == 0 {
            self.isr |= ISR_RDC;
            self.update_irq();
        }
        if val & CR_TRANS != 0 {
            let at = usize::from(self.tpsr) << 8;
            let len = usize::from(self.tcnt).min(MAX_FRAME);
            if at >= RAM_START && at + len <= MEM_SIZE {
                self.outbox.push(self.mem[at..at + len].to_vec());
                self.sent += 1;
            }
            self.tsr = TSR_PTX;
            self.isr |= ISR_TX;
            self.cmd &= !CR_TRANS;
            self.update_irq();
        }
        self.drain_inbox();
    }

    fn write_reg(&mut self, offset: u16, val: u8) {
        if offset == 0 {
            return self.write_cmd(val);
        }
        let page = self.cmd >> 6;
        match (page, offset) {
            (0, 0x01) => self.start = usize::from(val) << 8,
            (0, 0x02) => self.stop = usize::from(val) << 8,
            (0, 0x03) => {
                self.boundary = val;
                self.drain_inbox();
            }
            (0, 0x04) => self.tpsr = val,
            (0, 0x05) => self.tcnt = (self.tcnt & 0xFF00) | u16::from(val),
            (0, 0x06) => self.tcnt = (self.tcnt & 0x00FF) | (u16::from(val) << 8),
            (0, 0x07) => {
                self.isr &= !(val & 0x7F);
                self.update_irq();
            }
            (0, 0x08) => self.rsar = (self.rsar & 0xFF00) | u16::from(val),
            (0, 0x09) => self.rsar = (self.rsar & 0x00FF) | (u16::from(val) << 8),
            (0, 0x0A) => self.rcnt = (self.rcnt & 0xFF00) | u16::from(val),
            (0, 0x0B) => self.rcnt = (self.rcnt & 0x00FF) | (u16::from(val) << 8),
            (0, 0x0C) => self.rxcr = val,
            (0, 0x0D) => self.txcr = val,
            (0, 0x0E) => self.dcr = val,
            (0, 0x0F) => {
                self.imr = val;
                self.update_irq();
            }
            (1, 0x01..=0x06) => self.phys[usize::from(offset - 1)] = val,
            (1, 0x07) => {
                self.curpag = val;
                self.drain_inbox();
            }
            (1, 0x08..=0x0F) => self.mult[usize::from(offset - 8)] = val,
            _ => {}
        }
    }

    fn read_reg(&mut self, offset: u16) -> u8 {
        if offset == 0 {
            return self.cmd;
        }
        let page = self.cmd >> 6;
        match (page, offset) {
            (0, 0x03) => self.boundary,
            (0, 0x04) => self.tsr,
            (0, 0x07) => self.isr,
            (0, 0x08) => self.rsar as u8,
            (0, 0x09) => (self.rsar >> 8) as u8,
            // Identifiant RTL8029, comme la carte de QEMU.
            (0, 0x0A) => 0x50,
            (0, 0x0B) => 0x43,
            (0, 0x0C) => self.rsr,
            (1, 0x01..=0x06) => self.phys[usize::from(offset - 1)],
            (1, 0x07) => self.curpag,
            (1, 0x08..=0x0F) => self.mult[usize::from(offset - 8)],
            (2, 0x01) => (self.start >> 8) as u8,
            (2, 0x02) => (self.stop >> 8) as u8,
            (2, 0x04) => self.tpsr,
            (2, 0x0C) => self.rxcr,
            (2, 0x0D) => self.txcr,
            (2, 0x0E) => self.dcr,
            (2, 0x0F) => self.imr,
            _ => 0,
        }
    }

    /// Avance du DMA distant de `n` octets.
    fn dma_advance(&mut self, n: u16) {
        self.rsar = self.rsar.wrapping_add(n);
        if usize::from(self.rsar) == self.stop && self.stop > self.start {
            self.rsar = self.start as u16;
        }
        if self.rcnt <= n {
            self.rcnt = 0;
            self.isr |= ISR_RDC;
            self.update_irq();
        } else {
            self.rcnt -= n;
        }
    }

    fn mem_read(&self, addr: u16) -> u8 {
        let a = usize::from(addr);
        if a < 32 || (RAM_START..MEM_SIZE).contains(&a) { self.mem[a] } else { 0xFF }
    }

    fn mem_write(&mut self, addr: u16, val: u8) {
        let a = usize::from(addr);
        if (RAM_START..MEM_SIZE).contains(&a) {
            self.mem[a] = val;
        }
    }

    /// Unités transférées par accès au port de données : mot ou octet selon
    /// DCR, double mot pour un accès de 4 octets.
    fn data_width(&self, size: u8) -> u16 {
        match size {
            4 => 4,
            _ if self.dcr & DCR_WTS != 0 => 2,
            _ => 1,
        }
    }

    fn data_read(&mut self, size: u8) -> u32 {
        let width = self.data_width(size);
        let mut v = 0u32;
        for i in 0..width {
            v |= u32::from(self.mem_read(self.rsar.wrapping_add(i))) << (8 * i);
        }
        self.dma_advance(width);
        v
    }

    fn data_write(&mut self, size: u8, value: u32) {
        let width = self.data_width(size);
        for i in 0..width {
            self.mem_write(self.rsar.wrapping_add(i), (value >> (8 * i)) as u8);
        }
        self.dma_advance(width);
    }

    pub fn read_port(&mut self, port: u16, size: u8) -> u32 {
        match port.wrapping_sub(BASE) {
            o @ 0x00..=0x0F => u32::from(self.read_reg(o)),
            0x10..=0x17 => self.data_read(size),
            _ => {
                self.reset();
                0
            }
        }
    }

    pub fn write_port(&mut self, port: u16, size: u8, value: u32) {
        match port.wrapping_sub(BASE) {
            o @ 0x00..=0x0F => self.write_reg(o, value as u8),
            0x10..=0x17 => self.data_write(size, value),
            _ => {}
        }
    }
}

/// Index (0–63) du filtre multicast : 6 bits de poids fort du CRC-32
/// (calcul « big-endian » du DP8390).
fn multicast_hash(addr: &[u8]) -> u8 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in addr {
        let mut b = byte;
        for _ in 0..8 {
            let carry = (crc >> 31) ^ u32::from(b & 1);
            crc <<= 1;
            b >>= 1;
            if carry != 0 {
                crc = (crc ^ 0x04C1_1DB6) | carry;
            }
        }
    }
    (crc >> 26) as u8
}

/// Ports base–base + 1Fh.
pub struct Ne2000Ports(pub Rc<RefCell<Ne2000>>);

impl IoDevice for Ne2000Ports {
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

    fn w(n: &mut Ne2000, off: u16, v: u8) {
        n.write_port(BASE + off, 1, u32::from(v));
    }

    fn r(n: &mut Ne2000, off: u16) -> u8 {
        n.read_port(BASE + off, 1) as u8
    }

    /// Initialisation à la manière d'un packet driver : mode mot, anneau
    /// 46h–80h, émission en 40h, adresse physique, démarrage.
    fn init(n: &mut Ne2000) {
        r(n, 0x1F);
        w(n, 0x00, 0x21);
        w(n, 0x0E, 0x49);
        w(n, 0x0A, 0);
        w(n, 0x0B, 0);
        w(n, 0x0C, RCR_AB);
        w(n, 0x0D, 0x02);
        w(n, 0x01, 0x46);
        w(n, 0x03, 0x46);
        w(n, 0x02, 0x80);
        w(n, 0x07, 0xFF);
        w(n, 0x0F, 0x3F);
        w(n, 0x00, 0x61);
        for (i, b) in MAC.iter().enumerate() {
            w(n, 1 + i as u16, *b);
        }
        w(n, 0x07, 0x47);
        w(n, 0x00, 0x22);
        w(n, 0x0D, 0x00);
    }

    fn remote(n: &mut Ne2000, addr: u16, count: u16, cmd: u8) {
        w(n, 0x08, addr as u8);
        w(n, 0x09, (addr >> 8) as u8);
        w(n, 0x0A, count as u8);
        w(n, 0x0B, (count >> 8) as u8);
        w(n, 0x00, cmd);
    }

    #[test]
    fn prom_reports_mac_and_word_signature() {
        let mut n = Ne2000::new();
        assert_eq!(r(&mut n, 0x1F), 0);
        assert_eq!(r(&mut n, 0x07) & ISR_RESET, ISR_RESET);
        w(&mut n, 0x0E, 0x49);
        remote(&mut n, 0, 32, 0x0A);
        let words: Vec<u16> = (0..16).map(|_| n.read_port(BASE + 0x10, 2) as u16).collect();
        let mac: Vec<u8> = words[..6].iter().map(|&x| x as u8).collect();
        assert_eq!(mac, MAC);
        assert_eq!(words[14], 0x5757);
        assert_eq!(r(&mut n, 0x07) & ISR_RDC, ISR_RDC);
    }

    #[test]
    fn transmit_and_receive_through_ring() {
        let mut n = Ne2000::new();
        init(&mut n);
        // Émission : 60 octets écrits en 4000h par DMA distant, puis TXP.
        let frame: Vec<u8> = (0..60u8).collect();
        remote(&mut n, 0x4000, 60, 0x12);
        for pair in frame.chunks(2) {
            n.write_port(BASE + 0x10, 2, u32::from(u16::from_le_bytes([pair[0], pair[1]])));
        }
        w(&mut n, 0x04, 0x40);
        w(&mut n, 0x05, 60);
        w(&mut n, 0x06, 0);
        w(&mut n, 0x00, 0x26);
        assert_eq!(n.outbox, [frame]);
        assert!(n.take_irq());
        assert_eq!(r(&mut n, 0x07) & ISR_TX, ISR_TX);
        w(&mut n, 0x07, 0xFF);
        assert!(!n.take_irq());

        // Réception : une trame pour nous, une pour un autre (ignorée), une
        // diffusion.
        let mut ours = MAC.to_vec();
        ours.extend_from_slice(&[0xAA; 70]);
        let mut other = vec![2, 0, 0, 0, 0, 1];
        other.extend_from_slice(&[0; 60]);
        let mut bcast = vec![0xFF; 6];
        bcast.extend_from_slice(&[0xBB; 300]);
        n.deliver(ours.clone());
        n.deliver(other);
        n.deliver(bcast.clone());
        assert!(n.take_irq());
        assert_eq!(n.received, 2);
        // En-tête en 4700h (CURR) : état, page suivante, longueur (+ 4).
        remote(&mut n, 0x4700, 4, 0x0A);
        let h: Vec<u8> = (0..2).flat_map(|_| (n.read_port(BASE + 0x10, 2) as u16).to_le_bytes()).collect();
        assert_eq!(h, [RSR_RXOK, 0x48, 80, 0]);
        remote(&mut n, 0x4804, 2, 0x0A);
        assert_eq!(n.read_port(BASE + 0x10, 2) as u16, 0xFFFF);
        w(&mut n, 0x00, 0x62);
        assert_eq!(r(&mut n, 0x07), 0x4A);
        w(&mut n, 0x00, 0x22);
    }

    #[test]
    fn ring_full_keeps_frames_waiting() {
        let mut n = Ne2000::new();
        init(&mut n);
        let mut f = MAC.to_vec();
        f.extend_from_slice(&[0; 1508]);
        for _ in 0..20 {
            n.deliver(f.clone());
        }
        // 3Ah pages (46h–80h), 6 pages par trame : l'anneau se remplit.
        let placed = n.received;
        assert!((5..20).contains(&placed));
        // Le pilote consomme : BNRY avance, les trames en attente entrent.
        w(&mut n, 0x00, 0x62);
        let curr = r(&mut n, 0x07);
        w(&mut n, 0x00, 0x22);
        w(&mut n, 0x03, curr.wrapping_sub(1).max(0x46));
        assert!(n.received > placed);
    }
}
