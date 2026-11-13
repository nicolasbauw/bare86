// SPDX-License-Identifier: GPL-3.0-or-later
//! Battement : minuterie POSIX périodique qui interrompt `KVM_RUN` (signal
//! vers le thread du vCPU → EINTR), pour que l'hôte serve l'IRQ 0 et les
//! entrées même quand l'invité tourne sans sortir de la VM. Périodique :
//! un signal arrivé juste avant l'entrée dans `KVM_RUN` ne retarde l'IRQ
//! que d'une période.

use std::io;
use std::time::Duration;

extern "C" fn on_signal(_: libc::c_int) {}

pub struct Heartbeat {
    id: libc::timer_t,
    period: Duration,
}

impl Heartbeat {
    /// À créer dans le thread qui exécute la VM (le signal lui est destiné).
    pub fn new(period: Duration) -> io::Result<Self> {
        let sig = libc::SIGRTMIN() + 1;
        // SAFETY: gestionnaire vide, sans SA_RESTART pour que KVM_RUN rende
        // EINTR ; sigevent entièrement initialisé.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_signal as *const () as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut ev: libc::sigevent = std::mem::zeroed();
            ev.sigev_notify = libc::SIGEV_THREAD_ID;
            ev.sigev_signo = sig;
            ev.sigev_notify_thread_id = libc::gettid();
            let mut id: libc::timer_t = std::mem::zeroed();
            if libc::timer_create(libc::CLOCK_MONOTONIC, &mut ev, &mut id) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut hb = Self { id, period: Duration::ZERO };
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

impl Drop for Heartbeat {
    fn drop(&mut self) {
        // SAFETY: minuterie possédée.
        unsafe { libc::timer_delete(self.id) };
    }
}
