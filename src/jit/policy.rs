// SPDX-License-Identifier: GPL-3.0-or-later
//! Engagement automatique du traducteur dynamique, d'après les compteurs
//! de la machine (docs/JOURNAL.md, point 7).
//!
//! - **Engagement** : par fenêtres de `WINDOW`, le temps perdu hors de
//!   l'exécution native est estimé (sorties de VM liées à la VGA, rafales
//!   d'interpréteur) ; au-delà de `ENGAGE_SHARE`, le traducteur prend les
//!   rafales.
//! - **Rendement** : engagé, il se mesure (part des instructions qu'il rend
//!   à l'interpréteur) ; s'il n'apporte rien, il se retire pour un temps
//!   qui double à chaque échec (`BACKOFF_MIN` à `BACKOFF_MAX`), puis
//!   l'observation reprend.

use std::time::{Duration, Instant};

/// Durée d'une fenêtre d'observation.
pub const WINDOW: Duration = Duration::from_millis(250);
/// Coût estimé d'une sortie de VM servie (sortie, service, reprise).
const EXIT_COST_NS: u64 = 4_000;
/// Coût estimé d'une instruction interprétée.
const INTERP_COST_NS: u64 = 30;
/// Part du temps perdue hors du natif au-delà de laquelle on s'engage.
const ENGAGE_SHARE: f64 = 0.05;
/// Part des instructions rendues à l'interpréteur au-delà de laquelle le
/// traducteur n'apporte rien.
const FALLBACK_SHARE: f64 = 0.5;
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(32);

/// Compteurs cumulés de la machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Sorties de VM liées à la VGA (registres de plans, MMIO).
    pub vga_exits: u64,
    /// Instructions exécutées par l'interpréteur hors traducteur.
    pub interpreted: u64,
    /// Instructions des rafales traduites, et parmi elles celles rendues à
    /// l'interpréteur.
    pub jit_instructions: u64,
    pub jit_fallbacks: u64,
    /// Rafales confiées au traducteur.
    pub jit_bursts: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Jamais (`--no-jit`).
    Off,
    /// D'après les statistiques (défaut).
    Auto,
    /// Toujours (`--jit` : mesures, diagnostic).
    On,
}

pub struct Policy {
    mode: Mode,
    engaged: bool,
    window_start: Instant,
    base: Sample,
    backoff: Duration,
    retry_at: Option<Instant>,
    /// Engagements et retraits (diagnostic).
    pub engagements: u64,
    pub retreats: u64,
}

impl Policy {
    pub fn new(mode: Mode, now: Instant) -> Self {
        Self {
            mode,
            engaged: mode == Mode::On,
            window_start: now,
            base: Sample::default(),
            backoff: BACKOFF_MIN,
            retry_at: None,
            engagements: 0,
            retreats: 0,
        }
    }

    /// Le traducteur prend-il les rafales ?
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// À appeler souvent (haut de boucle) : décide à la fin de chaque fenêtre.
    pub fn tick(&mut self, now: Instant, s: Sample) {
        let dt = now.saturating_duration_since(self.window_start);
        if self.mode != Mode::Auto || dt < WINDOW {
            return;
        }
        let d = Sample {
            vga_exits: s.vga_exits - self.base.vga_exits,
            interpreted: s.interpreted - self.base.interpreted,
            jit_instructions: s.jit_instructions - self.base.jit_instructions,
            jit_fallbacks: s.jit_fallbacks - self.base.jit_fallbacks,
            jit_bursts: s.jit_bursts - self.base.jit_bursts,
        };
        self.window_start = now;
        self.base = s;
        if self.engaged {
            // Rendement : trop d'instructions rendues à l'interpréteur (le
            // traducteur ne fait que s'ajouter à lui), ou des rafales qui
            // n'exécutent rien (code 16 bits, mode non éligible).
            let useless = d.jit_instructions > 0 && d.jit_fallbacks as f64 > FALLBACK_SHARE * d.jit_instructions as f64;
            let ineligible = d.jit_bursts > 0 && d.jit_instructions < d.jit_bursts;
            if useless || ineligible {
                self.engaged = false;
                self.retreats += 1;
                self.retry_at = Some(now + self.backoff);
                self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
            } else if d.jit_instructions > 0 {
                self.backoff = BACKOFF_MIN;
            }
            return;
        }
        if self.retry_at.is_some_and(|t| now < t) {
            return;
        }
        let lost_ns = d.vga_exits * EXIT_COST_NS + d.interpreted * INTERP_COST_NS;
        if lost_ns as f64 > ENGAGE_SHARE * dt.as_nanos() as f64 {
            self.engaged = true;
            self.engagements += 1;
            self.retry_at = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn engages_on_vga_cost_only() {
        let t0 = Instant::now();
        let mut p = Policy::new(Mode::Auto, t0);
        // Jeu en mode 13h : presque rien hors du natif.
        p.tick(at(t0, 250), Sample { vga_exits: 100, interpreted: 10_000, ..Default::default() });
        assert!(!p.engaged());
        // Doom : ≈ 45 000 changements de plan par fenêtre (72 % du temps).
        p.tick(at(t0, 500), Sample { vga_exits: 45_100, interpreted: 10_000, ..Default::default() });
        assert!(p.engaged());
    }

    #[test]
    fn engages_on_interpreter_bursts() {
        let t0 = Instant::now();
        let mut p = Policy::new(Mode::Auto, t0);
        // Rise of the Triad : ≈ 2 M d'instructions interprétées par fenêtre.
        p.tick(at(t0, 250), Sample { vga_exits: 50, interpreted: 2_000_000, ..Default::default() });
        assert!(p.engaged());
    }

    #[test]
    fn retreats_and_backs_off_when_useless() {
        let t0 = Instant::now();
        let mut p = Policy::new(Mode::Auto, t0);
        let mut s = Sample { vga_exits: 50_000, ..Default::default() };
        p.tick(at(t0, 250), s);
        assert!(p.engaged());
        // Engagé, mais tout repart à l'interpréteur.
        s.jit_instructions += 1000;
        s.jit_fallbacks += 900;
        p.tick(at(t0, 500), s);
        assert!(!p.engaged());
        // Pas de nouvel essai avant une seconde, même sous forte charge.
        s.vga_exits += 50_000;
        p.tick(at(t0, 750), s);
        assert!(!p.engaged());
        s.vga_exits += 50_000;
        p.tick(at(t0, 1600), s);
        assert!(p.engaged());
        // Second échec : deux secondes de retrait.
        s.jit_instructions += 1000;
        s.jit_fallbacks += 900;
        p.tick(at(t0, 1850), s);
        assert!(!p.engaged());
        s.vga_exits += 50_000;
        p.tick(at(t0, 3000), s);
        assert!(!p.engaged());
        s.vga_exits += 50_000;
        p.tick(at(t0, 3900), s);
        assert!(p.engaged());
    }

    #[test]
    fn retreats_when_bursts_do_nothing() {
        let t0 = Instant::now();
        let mut p = Policy::new(Mode::Auto, t0);
        let mut s = Sample { vga_exits: 50_000, ..Default::default() };
        p.tick(at(t0, 250), s);
        assert!(p.engaged());
        // Keen 4 : code 16 bits, chaque rafale s'arrête aussitôt.
        s.jit_bursts += 40;
        p.tick(at(t0, 500), s);
        assert!(!p.engaged());
    }

    #[test]
    fn forced_modes_ignore_statistics() {
        let t0 = Instant::now();
        let mut on = Policy::new(Mode::On, t0);
        let mut off = Policy::new(Mode::Off, t0);
        let s = Sample { vga_exits: 1_000_000, jit_instructions: 10, jit_fallbacks: 10, ..Default::default() };
        on.tick(at(t0, 250), s);
        off.tick(at(t0, 250), s);
        assert!(on.engaged() && !off.engaged());
    }
}
