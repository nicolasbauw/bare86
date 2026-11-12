// SPDX-License-Identifier: GPL-3.0-or-later
//! Bridage : compteur d'instructions de l'invité qui interrompt `KVM_RUN`
//! toutes les `period` instructions (signal → EINTR).

use std::io;
use std::os::raw::c_int;

use perf_event_open_sys as sys;

// Absents de la crate libc pour x86_64 glibc.
const F_SETSIG: c_int = 10;
const F_SETOWN_EX: c_int = 15;
const F_OWNER_TID: c_int = 0;

#[repr(C)]
struct FOwnerEx {
    type_: c_int,
    pid: libc::pid_t,
}

extern "C" fn on_signal(_: c_int) {}

pub struct InstrCounter {
    fd: c_int,
}

impl InstrCounter {
    /// Compteur des instructions exécutées en mode invité par le thread courant.
    pub fn new(period: u64) -> io::Result<Self> {
        let sig = libc::SIGRTMIN();
        // SAFETY: gestionnaire vide, sans SA_RESTART pour que KVM_RUN rende EINTR.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_signal as *const () as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        let mut attr = sys::bindings::perf_event_attr {
            type_: sys::bindings::PERF_TYPE_HARDWARE,
            size: std::mem::size_of::<sys::bindings::perf_event_attr>() as u32,
            config: sys::bindings::PERF_COUNT_HW_INSTRUCTIONS as u64,
            ..Default::default()
        };
        attr.__bindgen_anon_1.sample_period = period;
        attr.set_disabled(1);
        // Invité seulement ; le code DOS en mode réel est en CPL 0 : ne pas exclure le noyau.
        attr.set_exclude_host(1);
        attr.__bindgen_anon_2.wakeup_events = 1;

        // SAFETY: attr initialisé ; pid 0 = thread courant, tous CPU.
        let fd = unsafe {
            sys::perf_event_open(&mut attr, 0, -1, -1, sys::bindings::PERF_FLAG_FD_CLOEXEC as _)
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let counter = Self { fd };
        // SAFETY: fd valide ; signal envoyé à ce thread à chaque débordement.
        unsafe {
            let owner = FOwnerEx { type_: F_OWNER_TID, pid: libc::gettid() };
            if libc::fcntl(fd, libc::F_SETFL, libc::O_ASYNC | libc::O_NONBLOCK) != 0
                || libc::fcntl(fd, F_SETSIG, sig) != 0
                || libc::fcntl(fd, F_SETOWN_EX, &owner) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        counter.arm()?;
        Ok(counter)
    }

    /// (Ré)arme le compteur pour un débordement.
    pub fn arm(&self) -> io::Result<()> {
        // SAFETY: fd valide.
        if unsafe { sys::ioctls::REFRESH(self.fd, 1) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Instructions invitées comptées depuis la création.
    pub fn read(&self) -> io::Result<u64> {
        let mut v = 0u64;
        // SAFETY: lecture de 8 octets dans v.
        let n = unsafe { libc::read(self.fd, (&raw mut v).cast(), 8) };
        if n != 8 {
            return Err(io::Error::last_os_error());
        }
        Ok(v)
    }
}

impl Drop for InstrCounter {
    fn drop(&mut self) {
        // SAFETY: fd possédé.
        unsafe { libc::close(self.fd) };
    }
}
