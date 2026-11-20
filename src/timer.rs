// SPDX-License-Identifier: GPL-3.0-or-later
//! Battement : minuterie POSIX périodique qui interrompt `KVM_RUN` (signal
//! vers le thread du vCPU → EINTR), pour que l'hôte serve l'IRQ 0 et les
//! entrées même quand l'invité tourne sans sortir de la VM. Périodique :
//! un signal arrivé juste avant l'entrée dans `KVM_RUN` ne retarde l'IRQ
//! que d'une période.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// Signal arrivé et pas encore vu par la boucle du vCPU. Hors de `KVM_RUN`
// (pendant le service d'une sortie de VM), le signal n'interrompt rien : sans
// ce drapeau, la sonde d'attente active qui suit un réveil se perdait une
// fois sur quelques-unes, et le vCPU tournait à vide jusqu'au battement.
// Propre au thread : le signal vise le thread du vCPU et son gestionnaire
// s'y exécute (les tests font tourner plusieurs machines à la fois).
thread_local! {
    static KICKED: AtomicBool = const { AtomicBool::new(false) };
}

extern "C" fn on_signal(_: libc::c_int) {
    KICKED.with(|k| k.store(true, Ordering::Relaxed));
}

/// Rend et efface le drapeau d'un signal reçu.
pub fn take_kick() -> bool {
    KICKED.with(|k| k.swap(false, Ordering::Relaxed))
}

/// Un signal est-il arrivé (sans l'effacer) ? Le traducteur dynamique,
/// qui exécute l'invité hors de `KVM_RUN`, rend alors la main comme le
/// ferait le vCPU, pour que l'hôte serve l'échéance.
pub fn kick_pending() -> bool {
    KICKED.with(|k| k.load(Ordering::Relaxed))
}

/// Signal du battement ; envoyé aussi par d'autres threads (réseau) pour
/// faire sortir le vCPU de `KVM_RUN` sans attendre la période.
pub fn signal() -> libc::c_int {
    libc::SIGRTMIN() + 1
}

/// Gestionnaire vide, sans SA_RESTART pour que KVM_RUN rende EINTR.
pub fn install_handler() -> io::Result<()> {
    // SAFETY: sigaction entièrement initialisée, gestionnaire sans effet.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(signal(), &sa, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub struct Heartbeat {
    id: libc::timer_t,
    period: Duration,
    /// Minuterie ponctuelle, même signal (`probe_in`).
    probe: libc::timer_t,
    /// Réveil à l'échéance de la prochaine IRQ de minuterie (`wake_in`).
    deadline: libc::timer_t,
}

impl Heartbeat {
    /// À créer dans le thread qui exécute la VM (le signal lui est destiné).
    pub fn new(period: Duration) -> io::Result<Self> {
        let sig = signal();
        install_handler()?;
        // Sommeils du vCPU (attentes actives, scrutations) à l'heure : la
        // tolérance par défaut des minuteries (50 µs, « timer slack ») plus
        // le réveil retardaient les IRQ 0 de ≈ 150 µs, et les démos qui
        // calent leur minuterie sur le balayage n'ont que ≈ 300 µs de marge
        // (Verses : impulsion de synchronisation manquée, une trame perdue).
        // SAFETY: réglage du thread appelant, sans pointeur.
        unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, 1 as libc::c_ulong) };
        // SAFETY: sigevent entièrement initialisé.
        unsafe {
            let mut ev: libc::sigevent = std::mem::zeroed();
            ev.sigev_notify = libc::SIGEV_THREAD_ID;
            ev.sigev_signo = sig;
            ev.sigev_notify_thread_id = libc::gettid();
            let mut id: libc::timer_t = std::mem::zeroed();
            if libc::timer_create(libc::CLOCK_MONOTONIC, &mut ev, &mut id) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut probe: libc::timer_t = std::mem::zeroed();
            if libc::timer_create(libc::CLOCK_MONOTONIC, &mut ev, &mut probe) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut deadline: libc::timer_t = std::mem::zeroed();
            if libc::timer_create(libc::CLOCK_MONOTONIC, &mut ev, &mut deadline) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut hb = Self { id, period: Duration::ZERO, probe, deadline };
            hb.set_period(period)?;
            Ok(hb)
        }
    }

    pub fn set_period(&mut self, period: Duration) -> io::Result<()> {
        if period == self.period {
            return Ok(());
        }
        self.period = period;
        let ts = libc::timespec { tv_sec: period.as_secs() as _, tv_nsec: period.subsec_nanos() as _ };
        let spec = libc::itimerspec { it_interval: ts, it_value: ts };
        // SAFETY: minuterie créée par timer_create.
        if unsafe { libc::timer_settime(self.id, 0, &spec, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Heartbeat {
    /// Un signal supplémentaire dans `d`, hors de la cadence périodique
    /// (remplace le précédent s'il n'est pas encore parti).
    pub fn probe_in(&self, d: Duration) -> io::Result<()> {
        let zero = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let ts = libc::timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos().max(1) as _ };
        let spec = libc::itimerspec { it_interval: zero, it_value: ts };
        // SAFETY: minuterie créée par timer_create.
        if unsafe { libc::timer_settime(self.probe, 0, &spec, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Heartbeat {
    /// Un signal dans `d`, à l'échéance de la prochaine IRQ de minuterie :
    /// sans lui, une IRQ due entre deux battements attendait le suivant
    /// (jusqu'à une période du PIT, 10 ms au plus), ce que les démos qui
    /// règlent le PIT juste avant le retour de trame ne tolèrent pas
    /// (Second Reality : trames perdues, défilement 20 % trop lent).
    pub fn wake_in(&self, d: Duration) -> io::Result<()> {
        let zero = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let ts = libc::timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos().max(1) as _ };
        let spec = libc::itimerspec { it_interval: zero, it_value: ts };
        // SAFETY: minuterie créée par timer_create.
        if unsafe { libc::timer_settime(self.deadline, 0, &spec, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        // SAFETY: minuteries possédées.
        unsafe {
            libc::timer_delete(self.id);
            libc::timer_delete(self.probe);
            libc::timer_delete(self.deadline);
        }
    }
}
