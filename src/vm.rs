// SPDX-License-Identifier: GPL-3.0-or-later
//! Machine virtuelle KVM minimale : mémoire invitée et vCPU en mode réel.

use std::ptr;

use kvm_bindings::kvm_userspace_memory_region;
use kvm_ioctls::{Kvm, VcpuFd, VmFd};

/// Mémoire invitée allouée par `mmap`, partagée avec le vCPU.
/// Accès uniquement par copie : jamais de référence `&[u8]` dessus.
pub struct GuestMem {
    ptr: *mut u8,
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
        assert!(addr + data.len() <= self.size, "écriture hors mémoire invitée");
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
        assert!(addr + buf.len() <= self.size, "lecture hors mémoire invitée");
        // SAFETY: bornes vérifiées ; le vCPU est arrêté pendant les accès hôte.
        unsafe { ptr::copy_nonoverlapping(self.ptr.add(addr), buf.as_mut_ptr(), buf.len()) }
    }

    pub fn read_u8(&self, addr: usize) -> u8 {
        assert!(addr < self.size, "lecture hors mémoire invitée");
        // SAFETY: bornes vérifiées.
        unsafe { ptr::read_volatile(self.ptr.add(addr)) }
    }

    pub fn write_u16(&self, addr: usize, v: u16) {
        self.write(addr, &v.to_le_bytes());
    }
}

impl Drop for GuestMem {
    fn drop(&mut self) {
        // SAFETY: ptr/size proviennent de notre mmap.
        unsafe { libc::munmap(self.ptr.cast(), self.size) };
    }
}

/// Fenêtre graphique VGA (A0000–AFFFF), émulée par sorties MMIO.
pub const VGA_WINDOW: std::ops::Range<usize> = 0xA_0000..0xB_0000;

/// Slot KVM réservé à la fenêtre VGA quand elle est mappée.
const VGA_SLOT: u32 = 2;

pub struct Machine {
    _kvm: Kvm,
    pub vm: VmFd,
    pub vcpu: VcpuFd,
    pub mem: GuestMem,
    vga_mapped: std::cell::Cell<bool>,
}

impl Machine {
    /// Crée une VM avec `mem_size` octets de RAM à l'adresse physique 0.
    pub fn new(mem_size: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let kvm = Kvm::new().map_err(|e| format!("ouverture de /dev/kvm impossible : {e}"))?;
        let vm = kvm.create_vm()?;
        let mem = GuestMem::new(mem_size)?;
        // Fenêtre graphique VGA laissée non mappée : chaque accès sort de la VM.
        let ranges = [(0, VGA_WINDOW.start), (VGA_WINDOW.end, mem_size)];
        for (slot, (start, end)) in ranges.into_iter().enumerate() {
            let region = kvm_userspace_memory_region {
                slot: slot as u32,
                flags: 0,
                guest_phys_addr: start as u64,
                memory_size: (end - start) as u64,
                // SAFETY: start < mem_size, dans notre mappage.
                userspace_addr: unsafe { mem.ptr.add(start) } as u64,
            };
            // SAFETY: la région reste mappée tant que `Machine` vit.
            unsafe { vm.set_user_memory_region(region)? };
        }
        let vcpu = vm.create_vcpu(0)?;
        Ok(Self { _kvm: kvm, vm, vcpu, mem, vga_mapped: false.into() })
    }

    /// Mappe `plane` (64 Ko) sur la fenêtre VGA, ou la démappe (`None`).
    /// KVM interdit de changer l'adresse hôte d'un slot : suppression puis création.
    pub fn map_vga_window(&self, plane: Option<&GuestMem>) -> Result<(), kvm_ioctls::Error> {
        let mut region = kvm_userspace_memory_region {
            slot: VGA_SLOT,
            flags: 0,
            guest_phys_addr: VGA_WINDOW.start as u64,
            memory_size: 0,
            userspace_addr: 0,
        };
        if self.vga_mapped.get() {
            // SAFETY: taille 0 = suppression du slot.
            unsafe { self.vm.set_user_memory_region(region)? };
            self.vga_mapped.set(false);
        }
        if let Some(p) = plane {
            assert!(p.size() >= VGA_WINDOW.len());
            region.memory_size = VGA_WINDOW.len() as u64;
            region.userspace_addr = p.host_addr();
            // SAFETY: le plan doit rester vivant tant qu'il est mappé (à la charge de l'appelant).
            unsafe { self.vm.set_user_memory_region(region)? };
            self.vga_mapped.set(true);
        }
        Ok(())
    }

    /// Mappe 64 Ko de mémoire hôte à `host_addr` sur la fenêtre VGA.
    pub fn map_vga_window_raw(&self, host_addr: u64) -> Result<(), kvm_ioctls::Error> {
        let region = kvm_userspace_memory_region {
            slot: VGA_SLOT,
            flags: 0,
            guest_phys_addr: VGA_WINDOW.start as u64,
            memory_size: VGA_WINDOW.len() as u64,
            userspace_addr: host_addr,
        };
        // SAFETY: la mémoire doit rester mappée tant que la VM vit (à la charge de l'appelant).
        unsafe { self.vm.set_user_memory_region(region)? };
        self.vga_mapped.set(true);
        Ok(())
    }

    /// Place le vCPU en mode réel : tous les segments à `seg`, IP et SP donnés.
    pub fn set_real_mode(&self, seg: u16, ip: u16, sp: u16) -> Result<(), kvm_ioctls::Error> {
        let mut sregs = self.vcpu.get_sregs()?;
        for s in [
            &mut sregs.cs,
            &mut sregs.ds,
            &mut sregs.es,
            &mut sregs.fs,
            &mut sregs.gs,
            &mut sregs.ss,
        ] {
            s.selector = seg;
            s.base = u64::from(seg) << 4;
            s.limit = 0xFFFF;
        }
        self.vcpu.set_sregs(&sregs)?;
        let mut regs = self.vcpu.get_regs()?;
        regs.rip = u64::from(ip);
        regs.rsp = u64::from(sp);
        regs.rflags = 0x202; // bit 1 réservé + IF
        self.vcpu.set_regs(&regs)
    }
}
