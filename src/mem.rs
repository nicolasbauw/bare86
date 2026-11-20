// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire invitée : un mappage anonyme partagé avec le vCPU.

use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

/// Mémoire invitée allouée par `mmap`, partagée avec le vCPU.
/// Accès uniquement par copie : jamais de référence `&[u8]` dessus.
pub struct GuestMem {
    pub(crate) ptr: *mut u8,
    size: usize,
    /// memfd qui porte la mémoire : le traducteur dynamique la mappe une
    /// seconde fois, avec ses propres protections (`jit::fastmem`).
    fd: libc::c_int,
    /// Un bit par page de 4 Ko écrite par l'hôte (`write`…) : KVM ne voit
    /// pas ces écritures ; le traducteur dynamique en tient compte pour ses
    /// pages de code (`take_host_written`).
    host_written: Box<[AtomicU64]>,
}

impl GuestMem {
    pub fn new(size: usize) -> std::io::Result<Self> {
        // SAFETY: memfd créé et dimensionné ici, mappage partagé vérifié ;
        // pages énormes demandées (THP de shmem en mode « advise ») : le
        // code natif sous KVM y gagne comme avec la mémoire anonyme.
        let (fd, ptr) = unsafe {
            let fd = libc::memfd_create(c"bare86-ram".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ftruncate(fd, size as libc::off_t) != 0 {
                let e = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }
            let ptr = libc::mmap(ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED | libc::MAP_NORESERVE, fd, 0);
            if ptr == libc::MAP_FAILED {
                let e = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }
            libc::madvise(ptr, size, libc::MADV_HUGEPAGE);
            (fd, ptr)
        };
        let host_written = (0..size.div_ceil(0x1000 * 64)).map(|_| AtomicU64::new(0)).collect();
        Ok(Self { ptr: ptr.cast(), size, fd, host_written })
    }

    pub fn write(&self, addr: usize, data: &[u8]) {
        assert!(addr + data.len() <= self.size, "write outside guest memory");
        if !data.is_empty() {
            for p in addr >> 12..=(addr + data.len() - 1) >> 12 {
                // Lecture d'abord : l'interpréteur écrit sans cesse dans les
                // mêmes pages, et l'opération atomique coûte bien plus.
                let (w, bit) = (&self.host_written[p / 64], 1u64 << (p % 64));
                if w.load(Ordering::Relaxed) & bit == 0 {
                    w.fetch_or(bit, Ordering::Relaxed);
                }
            }
        }
        // SAFETY: bornes vérifiées ; le vCPU est arrêté pendant les accès hôte.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(addr), data.len()) }
    }

    pub fn host_addr(&self) -> u64 {
        self.ptr as u64
    }

    pub fn fd(&self) -> libc::c_int {
        self.fd
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn read(&self, addr: usize, buf: &mut [u8]) {
        assert!(addr + buf.len() <= self.size, "read outside guest memory");
        // SAFETY: bornes vérifiées ; le vCPU est arrêté pendant les accès hôte.
        unsafe { ptr::copy_nonoverlapping(self.ptr.add(addr), buf.as_mut_ptr(), buf.len()) }
    }

    /// 16 octets à `addr` (petit-boutiste) : lecture des instructions par
    /// l'interpréteur.
    pub fn read_u128(&self, addr: usize) -> u128 {
        assert!(addr + 16 <= self.size, "read outside guest memory");
        // SAFETY: bornes vérifiées ; lecture non alignée explicite.
        u128::from_le(unsafe { ptr::read_unaligned(self.ptr.add(addr).cast::<u128>()) })
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
        // SAFETY: ptr/size proviennent de notre mmap, fd de memfd_create.
        unsafe {
            libc::munmap(self.ptr.cast(), self.size);
            libc::close(self.fd);
        }
    }
}
