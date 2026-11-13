// SPDX-License-Identifier: GPL-3.0-or-later
//! Backend KVM : une VM à un vCPU, mémoire découpée en slots.

use kvm_bindings::{KVM_GUESTDBG_ENABLE, KVM_GUESTDBG_SINGLESTEP, KVM_MEM_READONLY, kvm_guest_debug, kvm_userspace_memory_region};
use std::os::fd::AsRawFd;

use kvm_ioctls::{Kvm, VcpuExit, VcpuFd, VmFd};

use crate::backend::{Backend, Exit, Result};
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Fenêtre graphique VGA (A0000–AFFFF), émulée par sorties MMIO.
pub const VGA_WINDOW: std::ops::Range<usize> = 0xA_0000..0xB_0000;

/// Plage de mémoire invitée exposée par un slot KVM.
#[derive(Clone, Copy)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    /// Écritures de l'invité ignorées par la mémoire (sorties MMIO).
    pub readonly: bool,
}

/// Slot KVM réservé à la fenêtre VGA quand elle est mappée (bancs).
const VGA_SLOT: u32 = 31;

pub struct KvmVm {
    _kvm: Kvm,
    pub vm: VmFd,
    pub vcpu: VcpuFd,
    pub mem: GuestMem,
    vga_mapped: std::cell::Cell<bool>,
    single_step: bool,
}

impl KvmVm {
    /// `mem_size` octets de RAM, fenêtre graphique VGA non mappée.
    pub fn new(mem_size: usize) -> Result<Self> {
        let r = |start, end| Region { start, end, readonly: false };
        Self::with_layout(mem_size, &[r(0, VGA_WINDOW.start), r(VGA_WINDOW.end, mem_size)])
    }

    /// Toute la mémoire en RAM (tests différentiels).
    #[cfg(test)]
    pub fn new_flat(mem_size: usize) -> Result<Self> {
        Self::with_layout(mem_size, &[Region { start: 0, end: mem_size, readonly: false }])
    }

    pub fn with_layout(mem_size: usize, regions: &[Region]) -> Result<Self> {
        let kvm = Kvm::new().map_err(|e| format!("ouverture de /dev/kvm impossible : {e}"))?;
        let vm = kvm.create_vm()?;
        let mem = GuestMem::new(mem_size)?;
        for (slot, r) in regions.iter().enumerate() {
            assert!(r.start < r.end && r.end <= mem_size);
            let region = kvm_userspace_memory_region {
                slot: slot as u32,
                flags: if r.readonly { KVM_MEM_READONLY } else { 0 },
                guest_phys_addr: r.start as u64,
                memory_size: (r.end - r.start) as u64,
                // SAFETY: start < mem_size, dans notre mappage.
                userspace_addr: unsafe { mem.ptr.add(r.start) } as u64,
            };
            // SAFETY: la région reste mappée tant que la VM vit.
            unsafe { vm.set_user_memory_region(region)? };
        }
        let vcpu = vm.create_vcpu(0)?;
        Ok(Self { _kvm: kvm, vm, vcpu, mem, vga_mapped: false.into(), single_step: false })
    }

    /// Mappe `plane` (64 Ko) sur la fenêtre VGA, ou la démappe (`None`).
    /// KVM interdit de changer l'adresse hôte d'un slot : suppression puis création.
    pub fn map_vga_window(&self, plane: Option<&GuestMem>) -> Result<()> {
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
    pub fn map_vga_window_raw(&self, host_addr: u64) -> Result<()> {
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

    fn arm_single_step(&self) -> Result<()> {
        let control = if self.single_step { KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP } else { 0 };
        self.vcpu.set_guest_debug(&kvm_guest_debug { control, ..Default::default() })?;
        Ok(())
    }
}

impl Backend for KvmVm {
    fn mem(&self) -> &GuestMem {
        &self.mem
    }

    fn run(&mut self) -> Result<Exit<'_>> {
        if self.single_step {
            // KVM ne pose TF que si RIP est celui mémorisé par cet appel.
            self.arm_single_step()?;
        }
        Ok(match self.vcpu.run() {
            Ok(VcpuExit::IoOut(port, data)) => Exit::IoOut { port, data },
            Ok(VcpuExit::IoIn(port, data)) => Exit::IoIn { port, data },
            Ok(VcpuExit::MmioRead(addr, data)) => Exit::MmioRead { addr, data },
            Ok(VcpuExit::MmioWrite(addr, data)) => Exit::MmioWrite { addr, data },
            Ok(VcpuExit::Hlt) => Exit::Hlt,
            Ok(VcpuExit::IrqWindowOpen) => Exit::IrqWindow,
            Ok(VcpuExit::Debug(_)) => Exit::Step,
            Ok(VcpuExit::Shutdown) => Exit::Shutdown,
            Ok(other) => Exit::Other(format!("{other:?}")),
            Err(e) if e.errno() == libc::EINTR => Exit::Interrupted,
            Err(e) => return Err(e.into()),
        })
    }

    fn complete_pending(&mut self) -> Result<()> {
        self.vcpu.set_kvm_immediate_exit(1);
        let r = self.vcpu.run().map(|_| ());
        self.vcpu.set_kvm_immediate_exit(0);
        match r {
            Err(e) if e.errno() != libc::EINTR => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn state(&self) -> Result<Cpu> {
        Ok(Cpu::from_kvm(&self.vcpu.get_regs()?, &self.vcpu.get_sregs()?))
    }

    fn set_state(&mut self, cpu: &Cpu) -> Result<()> {
        let mut regs = self.vcpu.get_regs()?;
        let mut sregs = self.vcpu.get_sregs()?;
        if cpu.to_kvm(&mut regs, &mut sregs) {
            self.vcpu.set_sregs(&sregs)?;
        }
        self.vcpu.set_regs(&regs)?;
        Ok(())
    }

    fn set_single_step(&mut self, on: bool) -> Result<()> {
        self.single_step = on;
        self.arm_single_step()
    }

    fn can_inject(&mut self) -> bool {
        let run = self.vcpu.get_kvm_run();
        run.ready_for_interrupt_injection != 0 && run.if_flag != 0
    }

    fn inject(&mut self, vector: u8) -> Result<()> {
        /// _IOW(KVMIO, 0x86, struct kvm_interrupt)
        const KVM_INTERRUPT: libc::c_ulong = 0x4004_AE86;
        let irq = kvm_bindings::kvm_interrupt { irq: u32::from(vector) };
        // SAFETY: ioctl du vCPU avec une structure kvm_interrupt valide.
        let r = unsafe { libc::ioctl(self.vcpu.as_raw_fd(), KVM_INTERRUPT, &irq) };
        if r != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn request_irq_window(&mut self, on: bool) {
        self.vcpu.get_kvm_run().request_interrupt_window = u8::from(on);
    }
}
