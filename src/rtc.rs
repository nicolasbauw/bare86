// SPDX-License-Identifier: GPL-3.0-or-later
//! Horloge temps réel et CMOS MC146818 (ports 70h, 71h) : heure locale de
//! l'hôte en BCD, registres d'état, configuration (mémoire, équipement)
//! avec sa somme de contrôle. Les écritures dans les registres d'heure sont
//! ignorées (l'horloge hôte fait foi) ; les autres sont conservées.
//!
//! Interruption périodique (IRQ 8) : cadence du registre A (1 024 Hz au
//! démarrage), activée par le bit PIE du registre B ; le registre C porte
//! IRQF et PF, remis à zéro par sa lecture. Comme sur la puce, la ligne reste
//! levée tant que C n'a pas été lu : une nouvelle IRQ n'est signalée qu'après
//! cette lecture. Le DOS de HX (DKRNL32) y cadence `Sleep`, ses minuteries et
//! celles de WINMM (`timeSetEvent`).

use crate::io::IoDevice;
use std::time::{Duration, Instant};

/// Retard au-delà duquel les périodes manquées sont abandonnées (vCPU arrêté
/// par le débogueur, machine hôte suspendue) plutôt que rattrapées.
const CATCH_UP_MAX: Duration = Duration::from_millis(100);

pub struct Rtc {
    index: u8,
    cmos: [u8; 128],
    /// Prochaine période échue (interruption périodique active).
    next_periodic: Option<Instant>,
}

/// IRQ de la RTC.
pub const IRQ: u8 = 8;

/// Période de l'interruption périodique pour le choix de cadence RS (bits 0
/// à 3 du registre A) : 256 et 128 Hz pour 1 et 2, 32 768 Hz / 2^(RS−1)
/// ensuite ; aucune pour 0.
fn rate_period(rs: u8) -> Option<Duration> {
    let hz = match rs & 0x0F {
        0 => return None,
        1 => 256,
        2 => 128,
        r => 32768u32 >> (r - 1),
    };
    Some(Duration::from_nanos(1_000_000_000 / u64::from(hz)))
}

fn bcd(v: i32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

impl Rtc {
    pub fn new(extended_kb: usize) -> Self {
        let mut cmos = [0u8; 128];
        cmos[0x0A] = 0x26;
        // 24 h, BCD.
        cmos[0x0B] = 0x02;
        cmos[0x0D] = 0x80;
        // Équipement : coprocesseur, VGA (0 = EGA/VGA), pas de lecteur.
        cmos[0x14] = 0x02;
        cmos[0x15..0x17].copy_from_slice(&640u16.to_le_bytes());
        let ext = (extended_kb.min(0xFFFF) as u16).to_le_bytes();
        cmos[0x17..0x19].copy_from_slice(&ext);
        cmos[0x30..0x32].copy_from_slice(&ext);
        let sum: u16 = cmos[0x10..0x2E].iter().map(|&b| u16::from(b)).sum();
        cmos[0x2E..0x30].copy_from_slice(&sum.to_be_bytes());
        Self { index: 0, cmos, next_periodic: None }
    }

    /// Redémarrage : interruptions de la RTC coupées (comme au POST) ;
    /// heure et configuration conservées.
    pub fn reset(&mut self) {
        self.cmos[0x0B] &= !0x70;
        self.cmos[0x0C] = 0;
        self.next_periodic = None;
    }

    /// Période de l'interruption périodique, si elle est active.
    pub fn periodic(&self) -> Option<Duration> {
        if self.cmos[0x0B] & 0x40 == 0 {
            return None;
        }
        rate_period(self.cmos[0x0A])
    }

    /// Échéance de la prochaine interruption périodique.
    pub fn next_irq(&self) -> Option<Instant> {
        self.next_periodic
    }

    /// (Ré)arme la période après un changement des registres A ou B.
    fn rearm(&mut self, now: Instant) {
        self.next_periodic = self.periodic().map(|p| now + p);
    }

    /// Période échue : PF (et IRQF) levés ; vrai si l'IRQ 8 doit être
    /// signalée (IRQF était baissé : registre C lu depuis la précédente).
    pub fn take_irq(&mut self, now: Instant) -> bool {
        let (Some(due), Some(period)) = (self.next_periodic, self.periodic()) else { return false };
        if now < due {
            return false;
        }
        self.next_periodic = Some(if now - due > CATCH_UP_MAX { now + period } else { due + period });
        let was = self.cmos[0x0C] & 0x80 != 0;
        self.cmos[0x0C] |= 0xC0;
        !was
    }

    fn read_reg(&mut self, reg: u8) -> u8 {
        let t = crate::bios::local_time();
        match reg {
            0x00 => bcd(t.tm_sec),
            0x02 => bcd(t.tm_min),
            0x04 => bcd(t.tm_hour),
            0x06 => bcd(t.tm_wday + 1),
            0x07 => bcd(t.tm_mday),
            0x08 => bcd(t.tm_mon + 1),
            0x09 => bcd(t.tm_year % 100),
            0x32 => bcd((t.tm_year + 1900) / 100),
            // Mise à jour en cours pendant les 244 dernières µs de chaque seconde.
            0x0A => {
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                self.cmos[0x0A] | if ns >= 999_756_000 { 0x80 } else { 0 }
            }
            // Indicateurs d'interruption : remis à zéro par la lecture.
            0x0C => std::mem::take(&mut self.cmos[0x0C]),
            r => self.cmos[usize::from(r)],
        }
    }
}

impl Rtc {
    fn read_port(&mut self, port: u16) -> u8 {
        if port == 0x71 { self.read_reg(self.index) } else { 0xFF }
    }

    fn write_port(&mut self, port: u16, v: u8, now: Instant) {
        if port == 0x70 {
            // Bit 7 : masque de la NMI, sans objet ici.
            self.index = v & 0x7F;
            return;
        }
        match self.index {
            0x00..=0x09 | 0x32 | 0x0C | 0x0D => {}
            r => {
                let old = self.periodic();
                self.cmos[usize::from(r)] = v;
                if matches!(r, 0x0A | 0x0B) && self.periodic() != old {
                    self.rearm(now);
                }
            }
        }
    }
}

/// Ports 70h–71h d'une RTC partagée avec la boucle de la machine (IRQ 8).
pub struct RtcPorts(pub std::rc::Rc<std::cell::RefCell<Rtc>>);

impl IoDevice for RtcPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        u32::from(self.0.borrow_mut().read_port(port))
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        self.0.borrow_mut().write_port(port, value as u8, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_interrupt() {
        let mut r = Rtc::new(0);
        let t0 = Instant::now();
        // Registre A = 26h au démarrage : 1 024 Hz ; PIE baissé.
        assert_eq!(r.periodic(), None);
        assert!(!r.take_irq(t0 + Duration::from_secs(1)));
        r.write_port(0x70, 0x0B, t0);
        r.write_port(0x71, 0x42, t0);
        assert_eq!(r.periodic(), Some(Duration::from_nanos(976_562)));
        let due = r.next_irq().unwrap();
        assert!(!r.take_irq(due - Duration::from_micros(1)));
        assert!(r.take_irq(due));
        // Registre C non lu : pas de nouvelle IRQ, mais PF reste levé.
        let next = r.next_irq().unwrap();
        assert_eq!(next - due, Duration::from_nanos(976_562));
        assert!(!r.take_irq(next));
        r.write_port(0x70, 0x0C, t0);
        assert_eq!(r.read_port(0x71), 0xC0);
        assert_eq!(r.read_port(0x71), 0x00);
        assert!(r.take_irq(r.next_irq().unwrap()));
        // Cadence changée (RS = 15 : 2 Hz), puis PIE baissé.
        r.write_port(0x70, 0x0A, t0);
        r.write_port(0x71, 0x2F, t0);
        assert_eq!(r.next_irq(), Some(t0 + Duration::from_millis(500)));
        r.write_port(0x70, 0x0B, t0);
        r.write_port(0x71, 0x02, t0);
        assert_eq!(r.next_irq(), None);
    }

    #[test]
    fn registers() {
        let mut r = Rtc::new(15 * 1024);
        r.write_port(0x70, 0x17, Instant::now());
        assert_eq!(u32::from(r.read_port(0x71)), 0x00);
        r.write_port(0x70, 0x18, Instant::now());
        assert_eq!(u32::from(r.read_port(0x71)), 0x3C);
        r.write_port(0x70, 0x0B, Instant::now());
        assert_eq!(u32::from(r.read_port(0x71)), 0x02);
        r.write_port(0x70, 0x04, Instant::now());
        let h = u32::from(r.read_port(0x71)) as u8;
        assert!(h >> 4 <= 2 && h & 15 <= 9, "{h:02X}");
        // Octet libre conservé, registre d'heure ignoré.
        r.write_port(0x70, 0x40, Instant::now());
        r.write_port(0x71, 0x5A, Instant::now());
        assert_eq!(u32::from(r.read_port(0x71)), 0x5A);
    }
}
