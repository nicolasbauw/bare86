// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire invitée : un mappage anonyme partagé avec le vCPU.

use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

/// Mémoire invitée allouée par `mmap`, partagée avec le vCPU.
/// Accès uniquement par copie : jamais de référence `&[u8]` dessus.
pub struct GuestMem {
    pub(crate) ptr: *mut u8,
    size: usize,
    /// Un bit par page de 4 Ko écrite par l'hôte (`write`…) : KVM ne voit
    /// pas ces écritures ; le traducteur dynamique en tient compte pour ses
    /// pages de code (`take_host_written`).
    host_written: Box<[AtomicU64]>,
}

impl GuestMem {
    pub fn new(size: usize) -> std::io::Result<Self> {
        // SAFETY: mappage anonyme privé, vérifié ci-dessous.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        let host_written = (0..size.div_ceil(0x1000 * 64)).map(|_| AtomicU64::new(0)).collect();
        Ok(Self { ptr: ptr.cast(), size, host_written })
    }

    pub fn write(&self, addr: usize, data: &[u8]) {
        assert!(addr + data.len() <= self.size, "write outside guest memory");
        if !data.is_empty() {
            for p in addr >> 12..=(addr + data.len() - 1) >> 12 {
                self.host_written[p / 64].fetch_or(1 << (p % 64), Ordering::Relaxed);
            }
        }
        // SAFETY: bornes vérifiées ; le vCPU est arrêté pendant les accès hôte.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(addr), data.len()) }
    }

    pub fn host_addr(&self) -> u64 {
        self.ptr as u64
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn read(&self, addr: usize, buf: &mut [u8]) {
        assert!(addr + buf.len() <= self.size, "read outside guest memory");
        // SAFETY: bornes vérifiées ; le vCPU est arrêté pendant les accès hôte.
        unsafe { ptr::copy_nonoverlapping(self.ptr.add(addr), buf.as_mut_ptr(), buf.len()) }
    }

    pub fn read_u8(&self, addr: usize) -> u8 {
        assert!(addr < self.size, "read outside guest memory");
        // SAFETY: bornes vérifiées.
        unsafe { ptr::read_volatile(self.ptr.add(addr)) }
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        u16::from(self.read_u8(addr)) | (u16::from(self.read_u8(addr + 1)) << 8)
    }

    pub fn write_u16(&self, addr: usize, v: u16) {
        self.write(addr, &v.to_le_bytes());
    }

    pub fn write_u8(&self, addr: usize, v: u8) {
        self.write(addr, &[v]);
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }

    pub fn write_u32(&self, addr: usize, v: u32) {
        self.write(addr, &v.to_le_bytes());
    }

    pub fn fill(&self, addr: usize, len: usize, v: u8) {
        self.write(addr, &vec![v; len]);
    }

    /// Pages de `pages` écrites par l'hôte depuis la dernière fois, ajoutées
    /// à `out` (et oubliées).
    pub fn take_host_written(&self, pages: &[u32], out: &mut Vec<u32>) {
        for &p in pages {
            let (w, bit) = (p as usize / 64, 1u64 << (p % 64));
            if self.host_written.get(w).is_some_and(|x| x.fetch_and(!bit, Ordering::Relaxed) & bit != 0) {
                out.push(p);
            }
        }
    }
}

impl Drop for GuestMem {
    fn drop(&mut self) {
        // SAFETY: ptr/size proviennent de notre mmap.
        unsafe { libc::munmap(self.ptr.cast(), self.size) };
    }
}
