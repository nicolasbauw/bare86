// SPDX-License-Identifier: GPL-3.0-or-later
//! Interface MIDI MPU-401 en mode UART (330h données, 331h état et
//! commandes), celle de la Sound Blaster 16 : réinitialisation (FFh) et
//! passage en UART (3Fh) acquittés par FEh ; les octets écrits ensuite sont
//! la musique General MIDI du programme. Pas de mode intelligent ni d'IRQ
//! (les jeux s'en passent en UART).
//!
//! Sorties (`MidiSink`) : le synthétiseur General MIDI sur OPL3
//! (`midisynth.rs`) et, sur demande, un enregistrement en fichier MIDI
//! standard (`--midi-out`), lisible par n'importe quel lecteur.

use crate::io::IoDevice;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

pub const BASE: u16 = 0x330;
const ACK: u8 = 0xFE;

/// Messages MIDI complets, avec l'instant de leur dernier octet.
pub trait MidiSink {
    fn message(&mut self, at: Instant, msg: &[u8]);
    /// Réinitialisation de l'interface (commande FFh) : notes coupées.
    fn reset(&mut self) {}
}

/// Découpe le flux d'octets en messages (statut courant, SysEx).
#[derive(Default)]
struct Parser {
    status: u8,
    msg: Vec<u8>,
    sysex: bool,
}

impl Parser {
    fn data_len(status: u8) -> usize {
        match status & 0xF0 {
            0xC0 | 0xD0 => 1,
            0x80..=0xE0 => 2,
            _ => match status {
                0xF1 | 0xF3 => 1,
                0xF2 => 2,
                _ => 0,
            },
        }
    }

    /// Octet suivant ; rend un message quand il est complet.
    fn feed(&mut self, b: u8) -> Option<Vec<u8>> {
        // Temps réel (horloge, start…) : ignoré, sans casser le message en cours.
        if b >= 0xF8 {
            return None;
        }
        if self.sysex {
            self.msg.push(b);
            if b == 0xF7 {
                self.sysex = false;
                return Some(std::mem::take(&mut self.msg));
            }
            return None;
        }
        if b == 0xF0 {
            self.sysex = true;
            self.msg = vec![b];
            return None;
        }
        if b & 0x80 != 0 {
            self.status = b;
            self.msg = vec![b];
        } else if self.status == 0 {
            return None;
        } else {
            if self.msg.is_empty() {
                // Statut courant (running status).
                self.msg.push(self.status);
            }
            self.msg.push(b);
        }
        if self.msg.len() > Self::data_len(self.status) {
            let m = std::mem::take(&mut self.msg);
            if self.status >= 0xF0 {
                self.status = 0;
            }
            return Some(m);
        }
        None
    }
}

/// Enregistrement en fichier MIDI standard (format 0, une milliseconde par
/// tic). Le fichier reste valide après chaque message (longueur de piste et
/// fin de piste réécrites) : rien n'est perdu si la session s'arrête net.
pub struct MidiFile {
    file: File,
    start: Option<Instant>,
    last_ms: u64,
    track_len: u32,
}

/// Quantité de longueur variable.
fn varlen(mut v: u64, out: &mut Vec<u8>) {
    let mut bytes = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    out.extend(bytes.iter().rev());
}

/// En-tête (22 octets) puis tempo : 1 000 000 µs par noire, 1000 tics par noire.
const TEMPO: [u8; 7] = [0x00, 0xFF, 0x51, 0x03, 0x0F, 0x42, 0x40];
const END: [u8; 4] = [0x00, 0xFF, 0x2F, 0x00];

impl MidiFile {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let mut file = File::create(path)?;
        file.write_all(b"MThd\0\0\0\x06\0\0\0\x01\x03\xE8MTrk\0\0\0\0")?;
        let mut f = Self { file, start: None, last_ms: 0, track_len: 0 };
        f.append(&TEMPO)?;
        Ok(f)
    }

    /// Ajoute des événements, puis la fin de piste et la longueur à jour.
    fn append(&mut self, events: &[u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(22 + u64::from(self.track_len)))?;
        self.file.write_all(events)?;
        self.track_len += events.len() as u32;
        self.file.write_all(&END)?;
        self.file.seek(SeekFrom::Start(18))?;
        self.file.write_all(&(self.track_len + END.len() as u32).to_be_bytes())
    }
}

impl MidiSink for MidiFile {
    fn message(&mut self, at: Instant, msg: &[u8]) {
        let start = *self.start.get_or_insert(at);
        let ms = at.saturating_duration_since(start).as_millis() as u64;
        let mut ev = Vec::new();
        varlen(ms.saturating_sub(self.last_ms), &mut ev);
        self.last_ms = ms.max(self.last_ms);
        if msg[0] == 0xF0 {
            ev.push(0xF0);
            varlen(msg.len() as u64 - 1, &mut ev);
            ev.extend_from_slice(&msg[1..]);
        } else {
            ev.extend_from_slice(msg);
        }
        if let Err(e) = self.append(&ev) {
            eprintln!("[midi] {e}");
        }
    }
}

pub struct Mpu401 {
    uart: bool,
    out: VecDeque<u8>,
    parser: Parser,
    sinks: Vec<Box<dyn MidiSink>>,
    pub bytes: u64,
}

impl Mpu401 {
    /// Sorties : synthétiseur, enregistrement… (aucune : octets perdus).
    pub fn new(sinks: Vec<Box<dyn MidiSink>>) -> Self {
        Self { uart: false, out: VecDeque::new(), parser: Parser::default(), sinks, bytes: 0 }
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    pub fn read_port(&mut self, port: u16) -> u8 {
        if port == BASE {
            return self.out.pop_front().unwrap_or(ACK);
        }
        // État : bit 7 à 0 = un octet à lire ; bit 6 à 0 = prêt à recevoir.
        if self.out.is_empty() { 0x80 } else { 0x00 }
    }

    pub fn write_port(&mut self, port: u16, v: u8) {
        if port == BASE {
            self.bytes += 1;
            if let Some(msg) = self.parser.feed(v) {
                let now = Instant::now();
                for s in &mut self.sinks {
                    s.message(now, &msg);
                }
            }
            return;
        }
        match v {
            0xFF => {
                self.uart = false;
                self.parser = Parser::default();
                for s in &mut self.sinks {
                    s.reset();
                }
                self.out.clear();
                self.out.push_back(ACK);
            }
            0x3F => {
                self.uart = true;
                self.out.push_back(ACK);
            }
            // Mode intelligent : commandes acquittées sans effet.
            _ if !self.uart => self.out.push_back(ACK),
            _ => {}
        }
    }
}

/// Ports 330h–331h.
pub struct MpuPorts(pub Rc<RefCell<Mpu401>>);

impl IoDevice for MpuPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        u32::from(self.0.borrow_mut().read_port(port))
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        self.0.borrow_mut().write_port(port, value as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone)]
    struct Log(Rc<RefCell<Vec<Vec<u8>>>>);

    impl MidiSink for Log {
        fn message(&mut self, _: Instant, msg: &[u8]) {
            self.0.borrow_mut().push(msg.to_vec());
        }
    }

    #[test]
    fn uart_handshake_and_messages() {
        let log = Log::default();
        let mut m = Mpu401::new(vec![Box::new(log.clone())]);
        assert_eq!(m.read_port(BASE + 1), 0x80);
        m.write_port(BASE + 1, 0xFF);
        assert_eq!(m.read_port(BASE + 1), 0x00);
        assert_eq!((m.read_port(BASE), m.read_port(BASE + 1)), (ACK, 0x80));
        m.write_port(BASE + 1, 0x3F);
        assert_eq!(m.read_port(BASE), ACK);
        // Note on, statut courant, horloge au milieu, programme, SysEx.
        for b in [0x90, 60, 100, 0xF8, 64, 100, 0xC0, 5, 0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7] {
            m.write_port(BASE, b);
        }
        assert_eq!(*log.0.borrow(), [vec![0x90, 60, 100], vec![0x90, 64, 100], vec![0xC0, 5], vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]]);
    }

    #[test]
    fn midi_file_stays_valid() {
        let path = std::env::temp_dir().join(format!("bare86-midi-{}.mid", std::process::id()));
        let mut f = MidiFile::create(&path).unwrap();
        let t0 = Instant::now();
        f.message(t0, &[0x90, 60, 100]);
        f.message(t0 + std::time::Duration::from_millis(300), &[0x80, 60, 0]);
        let data = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&data[..14], b"MThd\0\0\0\x06\0\0\0\x01\x03\xE8");
        let len = u32::from_be_bytes([data[18], data[19], data[20], data[21]]) as usize;
        assert_eq!(data.len(), 22 + len);
        // Tempo, note on (Δ 0), note off (Δ 300 = 82h 2Ch), fin de piste.
        assert_eq!(&data[22..], [&TEMPO[..], &[0x00, 0x90, 60, 100], &[0x82, 0x2C, 0x80, 60, 0], &END[..]].concat());
    }
}
