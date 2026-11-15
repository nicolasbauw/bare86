// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire invitée : un mappage anonyme partagé avec le vCPU.

use std::ptr;

/// Mémoire invitée allouée par `mmap`, partagée avec le vCPU.
/// Accès uniquement par copie : jamais de référence `&[u8]` dessus.
pub struct GuestMem {
    pub(crate) ptr: *mut u8,
    size: usize,
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
        Ok(Self { ptr: ptr.cast(), size })
    }

    pub fn write(&self, addr: usize, data: &[u8]) {
        assert!(addr + data.len() <= self.size, "write outside guest memory");
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
}

impl Drop for GuestMem {
    fn drop(&mut self) {
        // SAFETY: ptr/size proviennent de notre mmap.
        unsafe { libc::munmap(self.ptr.cast(), self.size) };
    }
}
