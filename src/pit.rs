// SPDX-License-Identifier: GPL-3.0-or-later
//! Minuterie 8254 (ports 40h–43h) et port 61h (haut-parleur, bascule de
//! rafraîchissement). Le temps est celui de l'hôte : les compteurs se lisent
//! d'après l'instant de leur chargement. Le canal 0 cadence l'IRQ 0, le
//! canal 2 le haut-parleur.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::io::IoDevice;

pub const PIT_HZ: u64 = 1_193_182;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Latch,
    Lo,
    Hi,
    LoHi,
}

#[derive(Clone, Copy)]
struct Channel {
    mode: u8,
    access: Access,
    /// Valeur de rechargement (0 = 65 536).
    reload: u32,
    loaded_at: Instant,
    /// Octet de poids faible reçu, en attente du fort (accès LoHi).
    write_lo: Option<u8>,
    /// Compte figé par une commande de verrouillage.
    latch: Option<u16>,
    /// Prochain octet lu en accès LoHi : le fort.
    read_hi: bool,
    /// Octet d'état verrouillé par une commande de relecture.
    status: Option<u8>,
    /// Comptage arrêté (mode 0 avant chargement, ou porte basse du canal 2).
    armed: bool,
}

impl Channel {
    fn new(now: Instant) -> Self {
        Self {
            mode: 3,
            access: Access::LoHi,
            reload: 0x1_0000,
            loaded_at: now,
            write_lo: None,
            latch: None,
            read_hi: false,
            status: None,
            armed: true,
        }
    }

    fn elapsed_ticks(&self, now: Instant) -> u64 {
        let ns = now.saturating_duration_since(self.loaded_at).as_nanos();
        (ns * u128::from(PIT_HZ) / 1_000_000_000) as u64
    }

    /// Compte courant.
    fn count(&self, now: Instant) -> u16 {
        if !self.armed {
            return self.reload as u16;
        }
        let r = u64::from(self.reload);
        let t = self.elapsed_ticks(now);
        let v = match self.mode {
            // Onde carrée : décrément de 2, deux fois par période.
            3 | 7 => (r - (2 * t) % r) & !1,
            2 | 6 => r - t % r,
            // Modes 0, 1, 4, 5 : décompte unique puis repli sur 65 536.
            _ => (r + 0x1_0000 - t % 0x1_0000) % 0x1_0000,
        };
        v as u16
    }

    /// Sortie OUT (canal 2 : bit 5 du port 61h).
    fn output(&self, now: Instant) -> bool {
        if !self.armed {
            return self.mode != 0;
        }
        let r = u64::from(self.reload);
        let t = self.elapsed_ticks(now);
        match self.mode {
            3 | 7 => t % r < r.div_ceil(2),
            2 | 6 => t % r != r - 1,
            _ => t >= r,
        }
    }

    fn period(&self) -> Duration {
        Duration::from_nanos(u64::from(self.reload) * 1_000_000_000 / PIT_HZ)
    }
}

pub struct Pit {
    ch: [Channel; 3],
    /// Port 61h : bits 0 (porte du canal 2) et 1 (haut-parleur).
    port61: u8,
    /// Prochaine IRQ 0 due.
    next_irq0: Option<Instant>,
}

impl Pit {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut p = Self { ch: [Channel::new(now); 3], port61: 0, next_irq0: None };
        p.ch[2].armed = false;
        p.next_irq0 = Some(now + p.ch[0].period());
        p
    }

    pub fn shared(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }

    /// Période de l'IRQ 0 (canal 0 périodique), pour cadencer les sorties de VM.
    pub fn irq0_period(&self) -> Duration {
        self.ch[0].period()
    }

    /// Nombre d'IRQ 0 échues depuis le dernier appel (au plus 1 : un invité
    /// en retard ne reçoit pas une rafale, comme un 8259 qui n'a qu'un bit).
    pub fn take_irq0(&mut self, now: Instant) -> bool {
        let Some(due) = self.next_irq0 else { return false };
        if now < due {
            return false;
        }
        let c = &self.ch[0];
        self.next_irq0 = match c.mode {
            2 | 3 | 6 | 7 => {
                let p = c.period().max(Duration::from_micros(10));
                let mut next = due + p;
                if next <= now {
                    // Retard de plusieurs périodes : on rattrape sans rafale.
                    next = now + p;
                }
                Some(next)
            }
            _ => None,
        };
        true
    }

    /// Prochaine échéance de l'IRQ 0.
    pub fn next_irq0(&self) -> Option<Instant> {
        self.next_irq0
    }

    fn reload(&mut self, i: usize, v: u32, now: Instant) {
        let c = &mut self.ch[i];
        c.reload = if v == 0 { 0x1_0000 } else { v };
        c.loaded_at = now;
        c.armed = i != 2 || self.port61 & 1 != 0;
        if i == 0 {
            self.next_irq0 = Some(now + c.period());
        }
    }

    fn write_control(&mut self, v: u8, now: Instant) {
        let sel = usize::from(v >> 6);
        if sel == 3 {
            // Relecture : bits 1–3 = canaux, bit 5 = pas le compte, bit 4 = pas l'état.
            for i in 0..3 {
                if v & (2 << i) != 0 {
                    let count = self.ch[i].count(now);
                    let out = self.ch[i].output(now);
                    let c = &mut self.ch[i];
                    if v & 0x20 == 0 && c.latch.is_none() {
                        c.latch = Some(count);
                    }
                    if v & 0x10 == 0 {
                        let access = match c.access {
                            Access::Latch => 0,
                            Access::Lo => 1,
                            Access::Hi => 2,
                            Access::LoHi => 3,
                        };
                        c.status = Some((u8::from(out) << 7) | (access << 4) | (c.mode << 1));
                    }
                }
            }
            return;
        }
        let access = match (v >> 4) & 3 {
            0 => Access::Latch,
            1 => Access::Lo,
            2 => Access::Hi,
            _ => Access::LoHi,
        };
        if access == Access::Latch {
            let count = self.ch[sel].count(now);
            let c = &mut self.ch[sel];
            if c.latch.is_none() {
                c.latch = Some(count);
                c.read_hi = false;
            }
            return;
        }
        let c = &mut self.ch[sel];
        c.access = access;
        c.mode = (v >> 1) & 7;
        c.write_lo = None;
        c.read_hi = false;
        c.latch = None;
        // Mode 0 : sortie basse jusqu'au chargement du compte.
        if c.mode == 0 {
            c.armed = false;
            if sel == 0 {
                self.next_irq0 = None;
            }
        }
    }

    fn write_counter(&mut self, i: usize, v: u8, now: Instant) {
        let c = &mut self.ch[i];
        match c.access {
            Access::Lo => self.reload(i, u32::from(v), now),
            Access::Hi => self.reload(i, u32::from(v) << 8, now),
            Access::LoHi | Access::Latch => match c.write_lo.take() {
                None => c.write_lo = Some(v),
                Some(lo) => self.reload(i, u32::from(lo) | (u32::from(v) << 8), now),
            },
        }
    }

    fn read_counter(&mut self, i: usize, now: Instant) -> u8 {
        let count = self.ch[i].count(now);
        let c = &mut self.ch[i];
        if let Some(s) = c.status.take() {
            return s;
        }
        let value = c.latch.unwrap_or(count);
        let (byte, done) = match c.access {
            Access::Lo => (value as u8, true),
            Access::Hi => ((value >> 8) as u8, true),
            Access::LoHi | Access::Latch => {
                let hi = c.read_hi;
                c.read_hi = !hi;
                (if hi { (value >> 8) as u8 } else { value as u8 }, hi)
            }
        };
        if done {
            c.latch = None;
        }
        byte
    }

    fn read_61(&self, now: Instant) -> u8 {
        // Bascule de rafraîchissement mémoire : toutes les 15,085 µs.
        let refresh = (now.duration_since(self.ch[0].loaded_at).as_nanos() / 15_085) & 1 == 1;
        let out2 = self.ch[2].output(now);
        (self.port61 & 0x0F) | (u8::from(refresh) << 4) | (u8::from(out2) << 5)
    }

    fn write_61(&mut self, v: u8, now: Instant) {
        let gate_rising = v & 1 != 0 && self.port61 & 1 == 0;
        self.port61 = v;
        let c = &mut self.ch[2];
        if v & 1 == 0 {
            c.armed = false;
        } else if gate_rising {
            c.armed = true;
            c.loaded_at = now;
        }
    }

    /// Haut-parleur : fréquence en Hz s'il sonne (porte, données, onde carrée).
    #[allow(dead_code)] // branché à la sortie son (étape 5)
    pub fn speaker(&self) -> Option<f64> {
        let c = &self.ch[2];
        (self.port61 & 3 == 3 && c.armed && matches!(c.mode, 3 | 7)).then(|| PIT_HZ as f64 / f64::from(c.reload))
    }
}

/// Ports 40h–43h et 61h.
pub struct PitPorts(pub Rc<RefCell<Pit>>);

impl IoDevice for PitPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        let now = Instant::now();
        let mut p = self.0.borrow_mut();
        u32::from(match port {
            0x40..=0x42 => p.read_counter(usize::from(port - 0x40), now),
            0x61 => p.read_61(now),
            _ => 0xFF,
        })
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let now = Instant::now();
        let mut p = self.0.borrow_mut();
        let v = value as u8;
        match port {
            0x40..=0x42 => p.write_counter(usize::from(port - 0x40), v, now),
            0x43 => p.write_control(v, now),
            0x61 => p.write_61(v, now),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_and_reprogram() {
        let mut p = Pit::new();
        let t0 = Instant::now();
        // Canal 0, mode 2, 1193 (1 kHz).
        p.write_control(0x34, t0);
        p.write_counter(0, (1193 & 0xFF) as u8, t0);
        p.write_counter(0, (1193 >> 8) as u8, t0);
        assert_eq!(p.irq0_period(), Duration::from_nanos(1193 * 1_000_000_000 / PIT_HZ));
        let later = t0 + Duration::from_micros(500);
        p.write_control(0x00, later);
        let lo = p.read_counter(0, later + Duration::from_micros(300));
        let hi = p.read_counter(0, later + Duration::from_micros(300));
        let v = u16::from(lo) | (u16::from(hi) << 8);
        // 500 µs écoulés au verrouillage : environ 597 tics restants.
        assert!((590..=600).contains(&v), "{v}");
        assert!(!p.take_irq0(t0 + Duration::from_micros(900)));
        assert!(p.take_irq0(t0 + Duration::from_micros(1001)));
        assert!(!p.take_irq0(t0 + Duration::from_micros(1500)));
        assert!(p.take_irq0(t0 + Duration::from_micros(2100)));
    }

    #[test]
    fn speaker() {
        let mut p = Pit::new();
        let t = Instant::now();
        p.write_control(0xB6, t);
        p.write_counter(2, 0xA9, t);
        p.write_counter(2, 0x04, t);
        assert_eq!(p.speaker(), None);
        p.write_61(0x03, t);
        let f = p.speaker().unwrap();
        assert!((f - 1000.0).abs() < 1.0, "{f}");
    }
}
