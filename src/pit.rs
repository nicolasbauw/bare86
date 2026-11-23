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
    /// Mot de commande reçu, compte initial attendu : le prochain compte
    /// écrit démarre le comptage, quel que soit le mode.
    fresh: bool,
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
            fresh: false,
        }
    }

    fn elapsed_ticks(&self, now: Instant) -> u64 {
        self.elapsed_ticks_from(self.loaded_at, now)
    }

    fn elapsed_ticks_from(&self, start: Instant, now: Instant) -> u64 {
        let ns = now.saturating_duration_since(start).as_nanos();
        (ns * u128::from(PIT_HZ) / 1_000_000_000) as u64
    }

    /// Compte après `t` impulsions depuis le chargement (ou le début de la
    /// période en cours, voir `Pit::ticks`).
    fn count(&self, t: u64) -> u16 {
        if !self.armed {
            return self.reload as u16;
        }
        let r = u64::from(self.reload);
        let v = match self.mode {
            // Onde carrée : décrément de 2, deux fois par période. Le compte
            // rechargé (r, 0 pour 65 536) n'est pas montré : la puce le
            // garde une impulsion, et un compte nul se lit comme une
            // demi-période de décalage (`GetTickCount` de HX le combine avec
            // OUT) ; on montre déjà le suivant.
            3 | 7 => match (2 * t) % r {
                0 => r.saturating_sub(2),
                d => (r - d) & !1,
            },
            2 | 6 => r - t % r,
            // Modes 0, 1, 4, 5 : décompte unique puis repli sur 65 536.
            _ => (r + 0x1_0000 - t % 0x1_0000) % 0x1_0000,
        };
        v as u16
    }

    /// Sortie OUT après `t` impulsions (canal 2 : bit 5 du port 61h).
    fn output(&self, t: u64) -> bool {
        if !self.armed {
            return self.mode != 0;
        }
        let r = u64::from(self.reload);
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
    /// Fin de la période du canal 0 que montre son compte : avance à
    /// l'acquittement de l'IRQ 0 par l'invité (voir `ticks`).
    shown_end: Option<Instant>,
    /// Compte écrit au canal 0 pendant qu'il tourne en mode périodique
    /// (2, 3), sans mot de commande : comme sur la puce, il n'est pris qu'au
    /// rechargement suivant, et la phase de l'IRQ 0 est gardée (un
    /// programme qui retouche le compte à chaque période pour se caler sur
    /// un événement en dépend). Après un mot de commande, le premier compte
    /// démarre aussitôt, comme en mode 0.
    pending0: Option<u32>,
}

impl Pit {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut p = Self { ch: [Channel::new(now); 3], port61: 0, next_irq0: None, shown_end: None, pending0: None };
        p.ch[2].armed = false;
        p.next_irq0 = Some(now + p.ch[0].period());
        p.shown_end = p.next_irq0;
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
    /// `masked` : IRQ 0 masquée au PIC (elle ne sera pas acquittée).
    pub fn take_irq0(&mut self, now: Instant, masked: bool) -> bool {
        let Some(due) = self.next_irq0 else { return false };
        if now < due {
            return false;
        }
        let c = &self.ch[0];
        // IRQ 0 masquée, donc jamais acquittée : le compte ne reste pas figé
        // plus d'une période. Non masquée, elle le sera tôt ou tard (vCPU
        // retenu dans un long appel à l'hôte, gestionnaire lent) : le compte
        // reste figé jusque-là, sans quoi la période montrée avancerait plus
        // vite que le compteur de tics et l'heure lue reculerait.
        let stale = masked && self.shown_end.is_some_and(|e| e < due);
        let periodic = matches!(c.mode, 2 | 3 | 6 | 7);
        // Rechargement : le compte en attente prend effet à cette échéance.
        if periodic && let Some(r) = self.pending0.take() {
            let c = &mut self.ch[0];
            c.reload = r;
            c.loaded_at = due;
        }
        let c = &self.ch[0];
        self.next_irq0 = match c.mode {
            2 | 3 | 6 | 7 => {
                let p = c.period().max(Duration::from_micros(10));
                let mut next = due + p;
                if next <= now {
                    // Retard de plusieurs périodes : une seule IRQ, et
                    // l'échéance suivante reste sur la grille des périodes
                    // (le compte garde sa phase).
                    let late = (now - due).as_nanos() / p.as_nanos();
                    next = due + p * (late as u32 + 1);
                }
                Some(next)
            }
            _ => None,
        };
        if stale {
            self.shown_end = self.next_irq0;
        }
        true
    }


    /// L'invité a acquitté l'IRQ 0 (cycle INTA) : le compte du canal 0
    /// passe à la période en cours.
    pub fn irq0_acknowledged(&mut self) {
        if let Some(next) = self.next_irq0 {
            self.shown_end = Some(next);
        }
    }

    /// Prochaine échéance de l'IRQ 0.
    pub fn next_irq0(&self) -> Option<Instant> {
        self.next_irq0
    }

    /// Impulsions comptées par le canal `i` à l'instant `now`. Canal 0
    /// périodique : depuis le début de la période montrée (`shown_end`), et
    /// figé sur sa fin tant que l'invité n'a pas acquitté l'IRQ 0 échue (au
    /// plus une période). Le compte ne repart donc qu'avec l'IRQ, comme sur
    /// la puce où les deux sont simultanés. Sans cela, l'IRQ levée au
    /// battement suivant, ou retenue par un invité aux interruptions
    /// masquées, laissait voir un compte reparti avec un compteur de tics
    /// pas encore avancé, et l'heure lue reculait (`GetTickCount` de HX,
    /// qui combine les deux : `Sleep` de seize jours dans Turok).
    fn ticks(&self, i: usize, now: Instant) -> u64 {
        let c = &self.ch[i];
        if i == 0
            && c.armed
            && matches!(c.mode, 2 | 3 | 6 | 7)
            && let Some(end) = self.shown_end
            && let Some(start) = end.checked_sub(c.period())
        {
            // Au moins une impulsion : en mode 3, un compte nul (65 536)
            // avec OUT haut se lit comme une demi-période écoulée.
            let t = c.elapsed_ticks_from(start, now);
            return t.clamp(1, u64::from(c.reload) - 1);
        }
        c.elapsed_ticks(now)
    }

    fn reload(&mut self, i: usize, v: u32, now: Instant) {
        let c = &mut self.ch[i];
        let v = if v == 0 { 0x1_0000 } else { v };
        // Canal 0 en marche périodique : pris au rechargement (`pending0`).
        if i == 0 && matches!(c.mode, 2 | 3 | 6 | 7) && !c.fresh && c.armed && self.next_irq0.is_some() {
            self.pending0 = Some(v);
            return;
        }
        c.fresh = false;
        if i == 0 {
            self.pending0 = None;
        }
        c.reload = v;
        c.loaded_at = now;
        c.armed = i != 2 || self.port61 & 1 != 0;
        if i == 0 {
            self.next_irq0 = Some(now + c.period());
            self.shown_end = self.next_irq0;
        }
    }

    fn write_control(&mut self, v: u8, now: Instant) {
        let sel = usize::from(v >> 6);
        if sel == 3 {
            // Relecture : bits 1–3 = canaux, bit 5 = pas le compte, bit 4 = pas l'état.
            for i in 0..3 {
                if v & (2 << i) != 0 {
                    let t = self.ticks(i, now);
                    let count = self.ch[i].count(t);
                    let out = self.ch[i].output(t);
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
            let count = self.ch[sel].count(self.ticks(sel, now));
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
        c.fresh = true;
        if sel == 0 {
            self.pending0 = None;
        }
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
        let count = self.ch[i].count(self.ticks(i, now));
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
        let out2 = self.ch[2].output(self.ticks(2, now));
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
    pub fn speaker(&self) -> Option<f64> {
        let c = &self.ch[2];
        (self.port61 & 3 == 3 && c.armed && matches!(c.mode, 3 | 7)).then(|| PIT_HZ as f64 / f64::from(c.reload))
    }
}

/// Ports 40h–43h et 61h.
pub struct PitPorts(pub Rc<RefCell<Pit>>);

impl IoDevice for PitPorts {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        let now = crate::vga::timer_now(Instant::now());
        let mut p = self.0.borrow_mut();
        u32::from(match port {
            0x40..=0x42 => p.read_counter(usize::from(port - 0x40), now),
            0x61 => p.read_61(now),
            _ => 0xFF,
        })
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let now = crate::vga::timer_now(Instant::now());
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

    /// Canal 0 : le compte ne repart qu'avec l'IRQ 0, même levée en retard
    /// ou après un rattrapage.
    #[test]
    fn channel0_count_follows_irq0() {
        let mut p = Pit::new();
        let due = p.next_irq0().unwrap();
        let period = p.irq0_period();
        // Juste avant, puis bien après l'échéance, IRQ pas encore acquittée :
        // compte au plus bas (mode 3 : 2), sans repartir.
        let low = p.ch[0].count(p.ticks(0, due - Duration::from_micros(2)));
        assert!(low <= 6, "{low}");
        assert_eq!(p.ch[0].count(p.ticks(0, due + Duration::from_millis(3))), 2);
        // IRQ levée 3 ms en retard : toujours figé jusqu'à l'acquittement.
        let t3 = due + Duration::from_millis(3);
        assert!(p.take_irq0(t3, false));
        assert_eq!(p.next_irq0(), Some(due + period));
        assert_eq!(p.ch[0].count(p.ticks(0, t3)), 2);
        // Acquittée : le compte repart du début de la nouvelle période,
        // alignée sur l'échéance (mode 3 : 2 par impulsion, 3 ms ≈ 3 580).
        p.irq0_acknowledged();
        let c = p.ch[0].count(p.ticks(0, t3));
        assert!((0xE400..=0xE414).contains(&c), "{c:04X}");
        // Retard de plusieurs périodes : échéance suivante sur la grille.
        let late = due + period * 5 + Duration::from_millis(7);
        assert!(p.take_irq0(late, false));
        assert_eq!(p.next_irq0(), Some(due + period * 6));
        p.irq0_acknowledged();
        let c = p.ch[0].count(p.ticks(0, late));
        assert!((0xBEB0..=0xBED0).contains(&c), "{c:04X}");
        // Non masquée mais pas encore acquittée après deux périodes : figé.
        let next = p.next_irq0().unwrap();
        assert!(p.take_irq0(next, false));
        assert!(p.take_irq0(next + period, false));
        assert_eq!(p.ch[0].count(p.ticks(0, next + period + Duration::from_millis(3))), 2);
        // Masquée, donc jamais acquittée : gel d'une période au plus.
        assert!(p.take_irq0(next + period * 2, true));
        let c = p.ch[0].count(p.ticks(0, next + period * 2 + Duration::from_millis(3)));
        assert!((0xE400..=0xE414).contains(&c), "{c:04X}");
    }

    /// Canal 0 en mode 3 qui tourne : un compte écrit sans mot de commande
    /// n'est pris qu'au rechargement, la phase de l'IRQ 0 est gardée ; après
    /// un mot de commande, le premier compte démarre aussitôt.
    #[test]
    fn periodic_count_rewrite_keeps_phase() {
        let mut p = Pit::new();
        let t0 = Instant::now();
        p.write_control(0x36, t0);
        p.write_counter(0, 0x00, t0);
        p.write_counter(0, 0x50, t0); // 5000h ≈ 17,2 ms
        let due = p.next_irq0().unwrap();
        assert_eq!(due, t0 + p.irq0_period());
        // Nouveau compte au milieu de la période : échéance inchangée.
        let mid = t0 + Duration::from_millis(8);
        p.write_counter(0, 0x00, mid);
        p.write_counter(0, 0x40, mid); // 4000h ≈ 13,7 ms
        assert_eq!(p.next_irq0(), Some(due));
        // À l'échéance, la nouvelle période part de là.
        assert!(p.take_irq0(due, false));
        assert_eq!(p.irq0_period(), Duration::from_nanos(0x4000 * 1_000_000_000 / PIT_HZ));
        assert_eq!(p.next_irq0(), Some(due + p.irq0_period()));
        // Mot de commande puis compte : redémarrage immédiat.
        let t1 = due + Duration::from_millis(2);
        p.write_control(0x36, t1);
        p.write_counter(0, 0x00, t1);
        p.write_counter(0, 0x20, t1);
        assert_eq!(p.next_irq0(), Some(t1 + p.irq0_period()));
    }

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
        assert!(!p.take_irq0(t0 + Duration::from_micros(900), false));
        assert!(p.take_irq0(t0 + Duration::from_micros(1001), false));
        assert!(!p.take_irq0(t0 + Duration::from_micros(1500), false));
        assert!(p.take_irq0(t0 + Duration::from_micros(2100), false));
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
