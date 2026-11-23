// SPDX-License-Identifier: GPL-3.0-or-later
//! Backend KVM : une VM à un vCPU, mémoire découpée en slots.

use kvm_bindings::{KVM_GUESTDBG_ENABLE, KVM_GUESTDBG_SINGLESTEP, KVM_GUESTDBG_USE_HW_BP, KVM_MEM_READONLY, kvm_guest_debug, kvm_userspace_memory_region};
use std::os::fd::AsRawFd;

use kvm_ioctls::{Kvm, VcpuExit, VcpuFd, VmFd};

use crate::backend::{Backend, Exit, POSTED_PORTS, Result};
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
/// Slot de la HMA (100000h–10FFFFh) : la mémoire de 1 Mo, ou les 64
/// premiers Ko quand la porte A20 est fermée (repliement).
const HMA_SLOT: u32 = 30;
/// Copie de la ROM du BIOS (F0000h–FFFFFh) au sommet de l'espace de 4 Go, comme sur un PC : le
/// BIOS y est aussi visible en FFFF0000h–FFFFFFFFh. Des programmes l'utilisent sans le savoir :
/// SHINE#2 convertit le segment réel F000h rendu par INT 10h AX=4F0Ah par `sar edx,10h` (F000h
/// devient FFFFF000h) puis `shl edx,4` : son pointeur vers l'interface protégée de VBE tombe en
/// FFFFEC00h, et il lit et exécute la table là.
const ROM_ALIAS_SLOT: u32 = 28;
const ROM_ALIAS_ADDR: u64 = 0xFFFF_0000;
/// Adresses sans mémoire ni périphérique (au-dessus de la RAM, de part et d'autre du framebuffer) : mappées
/// en lecture seule sur des pages d'octets FFh, comme le bus ouvert d'un PC. Sans cela, une lecture par une
/// instruction que l'émulateur de KVM ne sait pas faire (x87…) échouait en « InternalError » au lieu de
/// rendre FFh (AI_OVERD lit par un pointeur fou).
const OPEN_BUS_SLOTS: [u32; 2] = [26, 27];
/// Framebuffer linéaire SVGA (VBE 2.0), à une adresse physique haute.
const LFB_SLOT: u32 = 29;
pub const LFB_ADDR: u64 = 0xE000_0000;
pub const HMA: std::ops::Range<usize> = 0x10_0000..0x11_0000;

pub struct KvmVm {
    _kvm: Kvm,
    pub vm: VmFd,
    pub vcpu: VcpuFd,
    pub mem: GuestMem,
    vga_mapped: std::cell::Cell<bool>,
    single_step: bool,
    /// Points d'arrêt matériels d'exécution (adresses linéaires, au plus
    /// quatre), d'après `BARE86_HWBP=adr[,adr…]` en hexadécimal : outil de
    /// diagnostic (registres et pile affichés à chaque passage).
    hw_bp: Vec<u64>,
    /// Arrêt sur un point d'arrêt : RF à poser avant de reprendre, pour ne
    /// pas s'y arrêter de nouveau.
    resume_rf: bool,
    /// Interruption confiée à KVM (KVM_INTERRUPT) que l'invité n'a pas encore
    /// prise : KVM_RUN doit être exécuté avant toute autre injection.
    injected: bool,
    /// Registres système à la création du vCPU (réinitialisation).
    reset_sregs: kvm_bindings::kvm_sregs,
    /// Slots de RAM (numéro, début, fin) et leur adresse hôte : journal des
    /// pages modifiées pour la surveillance du code traduit.
    ram_slots: Vec<(u32, usize, usize)>,
    /// Mappages hôte (adresse, longueur) du bus ouvert, dont `trim_open_bus` retire les pages touchées.
    open_bus: Vec<(u64, usize)>,
    /// Journal en place (`None` : pas encore essayé).
    dirty_log: Option<bool>,
    /// File des écritures de ports mises en attente par KVM (`POSTED_PORTS`).
    posted: Option<PostedRing>,
}

/// Page de la file des écritures mises en attente (« coalesced MMIO/PIO »)
/// du vCPU, mappée ici plutôt que par `kvm-ioctls` : elle se lit pendant
/// que la sortie de `KVM_RUN` emprunte le vCPU.
struct PostedRing {
    page: *mut kvm_bindings::kvm_coalesced_mmio_ring,
    size: usize,
    max: u32,
}

impl PostedRing {
    fn map(vcpu: &VcpuFd) -> std::io::Result<Self> {
        // SAFETY: sysconf sans effet de bord.
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        // SAFETY: page de la file au décalage documenté du descripteur du
        // vCPU ; l'appel échoue simplement si elle n'existe pas.
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                vcpu.as_raw_fd(),
                (kvm_bindings::KVM_COALESCED_MMIO_PAGE_OFFSET as usize * size) as libc::off_t,
            )
        };
        if page == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        let max = ((size - size_of::<kvm_bindings::kvm_coalesced_mmio_ring>()) / size_of::<kvm_bindings::kvm_coalesced_mmio>()) as u32;
        Ok(Self { page: page.cast(), size, max })
    }

    /// Rend les écritures en attente, dans l'ordre, à `f(port, données)`.
    fn drain(&self, f: &mut dyn FnMut(u16, &[u8])) {
        // SAFETY: page mappée tant que `self` vit ; KVM n'avance que `last`
        // (après avoir écrit l'entrée), nous seulement `first`, et KVM_RUN
        // n'est pas en cours.
        unsafe {
            let first = &raw mut (*self.page).first;
            let last = &raw const (*self.page).last;
            let entries = (&raw mut (*self.page).coalesced_mmio).cast::<kvm_bindings::kvm_coalesced_mmio>();
            loop {
                let i = first.read_volatile();
                if i == last.read_volatile() {
                    break;
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                let w = entries.add(i as usize).read_volatile();
                let len = (w.len as usize).min(w.data.len());
                f(w.phys_addr as u16, &w.data[..len]);
                first.write_volatile((i + 1) % self.max);
            }
        }
    }
}

impl Drop for PostedRing {
    fn drop(&mut self) {
        // SAFETY: mappage fait par `map`.
        unsafe { libc::munmap(self.page.cast(), self.size) };
    }
}

/// Mappe `start..end` (adresses invitées) en lecture seule sur des pages FFh : un mappage de réservation
/// recouvert, par blocs de 2 Mo, d'un même memfd rempli de FFh.
fn map_open_bus(vm: &VmFd, slot: u32, start: u64, end: u64) -> std::result::Result<(u64, usize), String> {
    const CHUNK: usize = 2 << 20;
    static FF: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let fd = *FF.get_or_init(|| {
        // SAFETY: memfd créé et rempli ici, jamais fermé (vit autant que le processus).
        unsafe {
            let fd = libc::memfd_create(c"bare86-openbus".as_ptr(), libc::MFD_CLOEXEC);
            if fd >= 0 {
                let data = vec![0xFFu8; CHUNK];
                if libc::write(fd, data.as_ptr().cast(), CHUNK) != CHUNK as isize {
                    libc::close(fd);
                    return -1;
                }
            }
            fd
        }
    });
    if fd < 0 || end <= start {
        return Err("open bus: no memfd".into());
    }
    let len = (end - start) as usize;
    // SAFETY: réservation anonyme puis recouvrement par blocs du même fichier, dans la plage réservée.
    unsafe {
        let base = libc::mmap(std::ptr::null_mut(), len, libc::PROT_NONE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0);
        if base == libc::MAP_FAILED {
            return Err("open bus: mmap".into());
        }
        let mut off = 0;
        while off < len {
            let n = CHUNK.min(len - off);
            let r = libc::mmap(base.cast::<u8>().add(off).cast(), n, libc::PROT_READ, libc::MAP_SHARED | libc::MAP_FIXED, fd, 0);
            if r == libc::MAP_FAILED {
                libc::munmap(base, len);
                return Err("open bus: remap".into());
            }
            off += n;
        }
        let region = kvm_userspace_memory_region { slot, flags: KVM_MEM_READONLY, guest_phys_addr: start, memory_size: len as u64, userspace_addr: base as u64 };
        if let Err(e) = vm.set_user_memory_region(region) {
            libc::munmap(base, len);
            return Err(format!("open bus: {e}"));
        }
        Ok((base as u64, len))
    }
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
        let kvm = Kvm::new().map_err(|e| format!("cannot open /dev/kvm: {e}"))?;
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
        if mem_size >= 0x10_0000 {
            let region = kvm_userspace_memory_region {
                slot: ROM_ALIAS_SLOT,
                flags: KVM_MEM_READONLY,
                guest_phys_addr: ROM_ALIAS_ADDR,
                memory_size: 0x1_0000,
                // SAFETY: F0000h–FFFFFh, dans notre mappage (mem_size ≥ 1 Mo).
                userspace_addr: unsafe { mem.ptr.add(0xF_0000) } as u64,
            };
            // SAFETY: la région reste mappée tant que la VM vit.
            unsafe { vm.set_user_memory_region(region)? };
        }
        let mut open_bus: Vec<(u64, usize)> = Vec::new();
        if mem_size >= 0x10_0000 {
            let top = (mem_size as u64).next_multiple_of(0x1000);
            // Un échec (adresses virtuelles refusées, par exemple) est sans gravité : le bus reste « vide ».
            open_bus.extend(map_open_bus(&vm, OPEN_BUS_SLOTS[0], top, LFB_ADDR));
            open_bus.extend(map_open_bus(&vm, OPEN_BUS_SLOTS[1], LFB_ADDR + crate::vga::FB_SIZE as u64, ROM_ALIAS_ADDR));
        }
        let vcpu = vm.create_vcpu(0)?;
        vcpu.set_cpuid2(&dos_era_cpuid(&kvm)?)?;
        // PAT de l'invité : toutes les entrées en write-back. Sous AMD (NPT),
        // elle s'applique aux tables de pages de l'invité : un extendeur qui
        // mappe le framebuffer linéaire en non caché (PCD, comme pour un
        // périphérique) le rendrait très lent (Quake en VESA : 17 i/s).
        // Le Pentium MMX annoncé n'a pas de PAT ; Intel (EPT) l'ignore.
        let pat = kvm_bindings::Msrs::from_entries(&[kvm_bindings::kvm_msr_entry {
            index: 0x277,
            data: 0x0606_0606_0606_0606,
            ..Default::default()
        }])
        .map_err(|e| format!("PAT MSR: {e:?}"))?;
        vcpu.set_msrs(&pat)?;
        // Écritures de l'index (et des données) du CMOS mises en file par KVM
        // au lieu d'une sortie chacune : DOS/4GW y écrit l'état d'arrêt à
        // chaque passage en mode réel (≈ 6 000 fois par seconde sous Doom,
        // pour la souris). Rendues avant la sortie suivante (`run`).
        // Sans cette possibilité, ces écritures sortent comme les autres.
        let posted = match kvm.check_extension(kvm_ioctls::Cap::CoalescedPio).then(|| PostedRing::map(&vcpu)) {
            Some(Ok(ring)) => {
                vm.register_coalesced_mmio(kvm_ioctls::IoEventAddress::Pio(u64::from(POSTED_PORTS.start)), POSTED_PORTS.len() as u32)?;
                Some(ring)
            }
            _ => None,
        };
        let reset_sregs = vcpu.get_sregs()?;
        let hw_bp = hw_breakpoints()?;
        let ram_slots = regions.iter().enumerate().filter(|(_, r)| !r.readonly).map(|(i, r)| (i as u32, r.start, r.end)).collect();
        let vm = Self {
            _kvm: kvm,
            vm,
            vcpu,
            mem,
            vga_mapped: false.into(),
            single_step: false,
            hw_bp,
            resume_rf: false,
            injected: false,
            reset_sregs,
            ram_slots,
            open_bus,
            dirty_log: None,
            posted,
        };
        if !vm.hw_bp.is_empty() {
            vm.arm_single_step()?;
        }
        Ok(vm)
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

    /// Mappe 64 Ko de mémoire hôte à `host_addr` sur la fenêtre VGA (en
    /// lecture seule si demandé).
    pub fn map_vga_window_raw(&self, host_addr: u64, readonly: bool) -> Result<()> {
        let region = kvm_userspace_memory_region {
            slot: VGA_SLOT,
            flags: if readonly { KVM_MEM_READONLY } else { 0 },
            guest_phys_addr: VGA_WINDOW.start as u64,
            memory_size: VGA_WINDOW.len() as u64,
            userspace_addr: host_addr,
        };
        // SAFETY: la mémoire doit rester mappée tant que la VM vit (à la charge de l'appelant).
        unsafe { self.vm.set_user_memory_region(region)? };
        self.vga_mapped.set(true);
        Ok(())
    }

    /// Journal des pages modifiées sur les slots de RAM, protection manuelle
    /// et bits initialement levés : aucune page n'est protégée (ni découpée)
    /// avant `protect_pages`. Activé au premier besoin ; faux si KVM ne sait
    /// pas faire.
    fn enable_dirty_log(&mut self) -> bool {
        if let Some(ok) = self.dirty_log {
            return ok;
        }
        use kvm_bindings::{KVM_CAP_MANUAL_DIRTY_LOG_PROTECT2, KVM_DIRTY_LOG_INITIALLY_SET, KVM_DIRTY_LOG_MANUAL_PROTECT_ENABLE, KVM_MEM_LOG_DIRTY_PAGES};
        let want = KVM_DIRTY_LOG_MANUAL_PROTECT_ENABLE | KVM_DIRTY_LOG_INITIALLY_SET;
        let caps = self.vm.check_extension_raw(libc::c_ulong::from(KVM_CAP_MANUAL_DIRTY_LOG_PROTECT2));
        let mut ok = caps > 0 && caps as u32 & want == want;
        if ok {
            let cap = kvm_bindings::kvm_enable_cap { cap: KVM_CAP_MANUAL_DIRTY_LOG_PROTECT2, args: [u64::from(want), 0, 0, 0], ..Default::default() };
            ok = self.vm.enable_cap(&cap).is_ok();
        }
        for &(slot, start, end) in &self.ram_slots {
            if !ok {
                break;
            }
            let region = kvm_userspace_memory_region {
                slot,
                flags: KVM_MEM_LOG_DIRTY_PAGES,
                guest_phys_addr: start as u64,
                memory_size: (end - start) as u64,
                // SAFETY: dans notre mappage, comme à la création du slot.
                userspace_addr: unsafe { self.mem.ptr.add(start) } as u64,
            };
            // SAFETY: même slot, mêmes bornes : seuls les drapeaux changent.
            ok = unsafe { self.vm.set_user_memory_region(region) }.is_ok();
        }
        self.dirty_log = Some(ok);
        ok
    }

    /// Slot de RAM d'une page : (numéro, première page du slot, taille).
    fn page_slot(&self, page: u32) -> Option<(u32, u32, usize)> {
        let a = (page as usize) << 12;
        self.ram_slots.iter().find(|&&(_, s, e)| a >= s && a + 0x1000 <= e).map(|&(slot, s, e)| (slot, (s >> 12) as u32, e - s))
    }

    /// Protège `pages` (bits du journal effacés : la prochaine écriture de
    /// l'invité y sera notée). Pages hors des slots de RAM ignorées.
    fn protect_pages(&self, pages: &[u32]) {
        for &p in pages {
            let Some((slot, first, size)) = self.page_slot(p) else { continue };
            let rel = p - first;
            // Par blocs de 64 pages alignés sur le slot (exigence de KVM).
            let chunk = rel & !63;
            let num = 64.min(((size >> 12) as u32) - chunk);
            let mut bitmap = [1u64 << (rel - chunk)];
            let clear = kvm_bindings::kvm_clear_dirty_log {
                slot,
                num_pages: num,
                first_page: u64::from(chunk),
                __bindgen_anon_1: kvm_bindings::kvm_clear_dirty_log__bindgen_ty_1 { dirty_bitmap: bitmap.as_mut_ptr().cast() },
            };
            // KVM_CLEAR_DIRTY_LOG = _IOWR(KVMIO, 0xC0, struct kvm_clear_dirty_log).
            const KVM_CLEAR_DIRTY_LOG: libc::c_ulong = 0xC018_AEC0;
            // SAFETY: structure et bitmap valides pendant l'appel.
            unsafe { libc::ioctl(std::os::fd::AsRawFd::as_raw_fd(&self.vm), KVM_CLEAR_DIRTY_LOG, &clear) };
        }
    }

    /// Porte A20 : HMA sur la mémoire de 1 Mo (ouverte) ou repliée sur 0.
    pub fn set_a20_slot(&self, open: bool) -> Result<()> {
        assert!(self.mem.size() >= HMA.end);
        let mut region = kvm_userspace_memory_region {
            slot: HMA_SLOT,
            flags: 0,
            guest_phys_addr: HMA.start as u64,
            memory_size: 0,
            userspace_addr: 0,
        };
        // SAFETY: taille 0 = suppression (sans effet si le slot n'existe pas encore).
        let _ = unsafe { self.vm.set_user_memory_region(region) };
        region.memory_size = HMA.len() as u64;
        // SAFETY: dans notre mappage, vivant autant que la VM.
        region.userspace_addr = unsafe { self.mem.ptr.add(if open { HMA.start } else { 0 }) } as u64;
        unsafe { self.vm.set_user_memory_region(region)? };
        Ok(())
    }

    /// Arme le pas à pas et les points d'arrêt matériels.
    fn arm_single_step(&self) -> Result<()> {
        let mut dbg = kvm_guest_debug::default();
        if self.single_step {
            dbg.control |= KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP;
        }
        if !self.hw_bp.is_empty() {
            dbg.control |= KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_USE_HW_BP;
            for (i, &a) in self.hw_bp.iter().enumerate() {
                dbg.arch.debugreg[i] = a;
                // DR7 : Li posé, R/W = 00 (exécution), LEN = 00.
                dbg.arch.debugreg[7] |= 1 << (2 * i);
                // BARE86_HWBP_W : surveillance des écritures de 4 octets
                // (R/W = 01, LEN = 11) ; arrêt après l'instruction qui écrit.
                if std::env::var_os("BARE86_HWBP_W").is_some() {
                    dbg.arch.debugreg[7] |= 0b1101 << (16 + 4 * i);
                }
            }
        }
        self.vcpu.set_guest_debug(&dbg)?;
        Ok(())
    }
}

/// Points d'arrêt de `BARE86_HWBP` (adresses linéaires en hexadécimal,
/// séparées par des virgules ; quatre au plus). Avec `BARE86_HWBP_W`, ce
/// sont des surveillances d'écriture (dword) au lieu de points d'exécution.
fn hw_breakpoints() -> Result<Vec<u64>> {
    let Some(v) = std::env::var_os("BARE86_HWBP") else { return Ok(Vec::new()) };
    let v = v.to_string_lossy();
    let bps = v
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).map_err(|e| format!("BARE86_HWBP: {s}: {e}")))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if bps.len() > 4 {
        return Err("BARE86_HWBP: four breakpoints at most".into());
    }
    Ok(bps)
}

/// Taille d'un accès de la sortie d'E/S en cours.
///
/// SAFETY: `run` désigne le kvm_run mappé du vCPU, dont la sortie en cours
/// est KVM_EXIT_IO (champ `io` de l'union).
unsafe fn io_size(run: *const kvm_bindings::kvm_run) -> u8 {
    unsafe { (*run).__bindgen_anon_1.io.size }
}

impl Backend for KvmVm {
    fn mem(&self) -> &GuestMem {
        &self.mem
    }

    fn trim_open_bus(&mut self) {
        for &(base, len) in &self.open_bus {
            // SAFETY: mappages anonymes/fichier créés par `map_open_bus`, jamais libérés ; MADV_DONTNEED sur du
            // partagé en lecture seule ne perd rien (le memfd garde les octets).
            unsafe { libc::madvise(base as *mut libc::c_void, len, libc::MADV_DONTNEED) };
        }
    }

    fn run(&mut self, posted: &mut dyn FnMut(u16, &[u8])) -> Result<Exit<'_>> {
        if self.single_step {
            // KVM ne pose TF que si RIP est celui mémorisé par cet appel.
            self.arm_single_step()?;
        }
        // Signal arrivé pendant le service de la sortie précédente : rendu
        // comme s'il avait interrompu KVM_RUN. Pas avec une interruption
        // injectée en attente : les champs de kvm_run (prêt à injecter) ne
        // seraient pas remis à jour, et une seconde injection écraserait la
        // première (IRQ 0 restée « en service » au PIC, sans gestionnaire :
        // Doom privé de tics jusqu'à l'EOI d'une autre IRQ). Le signal reste
        // alors noté, pour la sortie suivante.
        if !self.holds_injection() && crate::timer::take_kick() {
            return Ok(Exit::Interrupted);
        }
        if std::mem::take(&mut self.resume_rf) {
            let mut regs = self.vcpu.get_regs()?;
            regs.rflags |= 1 << 16;
            self.vcpu.set_regs(&regs)?;
        }
        // Le mappage de kvm_run est fixe pour toute la vie du vCPU.
        let kvm_run: *const kvm_bindings::kvm_run = self.vcpu.get_kvm_run();
        let r = self.vcpu.run();
        self.injected = false;
        if let Some(ring) = &self.posted {
            ring.drain(posted);
        }
        Ok(match r {
            // kvm-ioctls ne rend pas la taille d'un accès : elle est relue
            // dans kvm_run.
            Ok(VcpuExit::IoOut(port, data)) => {
                // SAFETY: sortie d'E/S en cours.
                Exit::IoOut { port, size: unsafe { io_size(kvm_run) }, data }
            }
            Ok(VcpuExit::IoIn(port, data)) => {
                // SAFETY: sortie d'E/S en cours.
                Exit::IoIn { port, size: unsafe { io_size(kvm_run) }, data }
            }
            Ok(VcpuExit::MmioRead(addr, data)) => Exit::MmioRead { addr, data },
            Ok(VcpuExit::MmioWrite(addr, data)) => Exit::MmioWrite { addr, data },
            Ok(VcpuExit::Hlt) => Exit::Hlt,
            Ok(VcpuExit::IrqWindowOpen) => Exit::IrqWindow,
            Ok(VcpuExit::Debug(_)) if !self.single_step && !self.hw_bp.is_empty() => {
                self.resume_rf = true;
                Exit::Breakpoint
            }
            Ok(VcpuExit::Debug(_)) => Exit::Step,
            Ok(VcpuExit::Shutdown) => Exit::Shutdown,
            Ok(other) => Exit::Other(format!("{other:?}")),
            Err(e) if e.errno() == libc::EINTR => {
                crate::timer::take_kick();
                Exit::Interrupted
            }
            Err(e) => return Err(e.into()),
        })
    }

    fn complete_pending(&mut self, mmio: &mut dyn FnMut(u64, &mut [u8], bool)) -> Result<()> {
        self.vcpu.set_kvm_immediate_exit(1);
        let r = loop {
            match self.vcpu.run() {
                Ok(VcpuExit::MmioRead(addr, data)) => mmio(addr, data, false),
                Ok(VcpuExit::MmioWrite(addr, data)) => {
                    let mut d = data.to_vec();
                    mmio(addr, &mut d, true);
                }
                // Instruction terminée : KVM rend la main sans relancer l'invité.
                Err(e) if e.errno() == libc::EINTR => break Ok(()),
                Ok(other) => break Err(format!("instruction completion: unexpected exit {other:?}").into()),
                Err(e) => break Err(e.into()),
            }
        };
        self.vcpu.set_kvm_immediate_exit(0);
        r
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

    fn reset_cpu(&mut self) -> Result<()> {
        self.vcpu.set_sregs(&self.reset_sregs)?;
        Ok(())
    }

    fn set_single_step(&mut self, on: bool) -> Result<()> {
        self.single_step = on;
        self.arm_single_step()
    }

    /// Sans irqchip dans le noyau, KVM tient `ready_for_interrupt_injection`
    /// à zéro tant qu'une interruption injectée n'est pas livrée
    /// (`kvm_cpu_has_extint` = `interrupt.injected`) : avec IF à 1, c'est
    /// le signe qu'elle attend (ou d'une ombre d'interruption, aussi brève).
    /// Sans ce test, un `KVM_RUN` interrompu par un signal juste avant
    /// l'entrée laissait l'IRQ 0 en file ; la sonde d'attente active, ne
    /// voyant rien d'attendu au PIC, endormait le vCPU, et chaque signal
    /// reçu pendant le sommeil faisait rendre `Interrupted` sans rentrer
    /// dans l'invité : Doom figé après `ST_Init`, IRQ 0 « en service ».
    fn holds_injection(&mut self) -> bool {
        let run = self.vcpu.get_kvm_run();
        self.injected || (run.ready_for_interrupt_injection == 0 && run.if_flag != 0)
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
        self.injected = true;
        Ok(())
    }

    fn set_a20(&mut self, open: bool) -> Result<()> {
        self.set_a20_slot(open)
    }

    fn watch_pages(&mut self, pages: &[u32]) -> Result<bool> {
        if !self.enable_dirty_log() {
            return Ok(false);
        }
        self.protect_pages(pages);
        Ok(true)
    }

    fn watched_written(&mut self, pages: &[u32], out: &mut Vec<u32>) -> Result<bool> {
        if self.dirty_log != Some(true) {
            return Ok(false);
        }
        let start = out.len();
        let mut logs: Vec<(u32, Vec<u64>)> = Vec::new();
        for &p in pages {
            let Some((slot, first, size)) = self.page_slot(p) else {
                // Hors des slots journalisés (HMA…) : tenue pour écrite.
                out.push(p);
                continue;
            };
            if !logs.iter().any(|(s, _)| *s == slot) {
                logs.push((slot, self.vm.get_dirty_log(slot, size)?));
            }
            let log = &logs.iter().find(|(s, _)| *s == slot).unwrap().1;
            let rel = (p - first) as usize;
            if log.get(rel / 64).is_some_and(|w| w >> (rel % 64) & 1 != 0) {
                out.push(p);
            }
        }
        // Surveillées de nouveau.
        self.protect_pages(&out[start..]);
        Ok(true)
    }

    fn map_lfb(&mut self, host_addr: u64, size: usize) -> Result<()> {
        let region = kvm_userspace_memory_region {
            slot: LFB_SLOT,
            flags: 0,
            guest_phys_addr: LFB_ADDR,
            memory_size: size as u64,
            userspace_addr: host_addr,
        };
        // SAFETY: la mémoire doit rester mappée tant que la VM vit (à la charge de l'appelant).
        unsafe { self.vm.set_user_memory_region(region)? };
        Ok(())
    }

    fn map_vga(&mut self, host_addr: Option<u64>, readonly: bool) -> Result<()> {
        match host_addr {
            Some(a) => self.map_vga_window_raw(a, readonly),
            None => self.map_vga_window(None),
        }
    }

    fn request_irq_window(&mut self, on: bool) {
        self.vcpu.get_kvm_run().request_interrupt_window = u8::from(on);
    }
}

/// Table CPUID vue par l'invité : un Pentium MMX (famille 5, modèle 4),
/// fabricant de l'hôte, fonctions de l'époque seulement (FPU, VME, DE, PSE,
/// TSC, MSR, CX8, MMX), sans fonctions étendues. Sans table, KVM rendrait
/// des zéros : famille 0, que DOS/32A ou PMODE/W prennent pour un 286.
fn dos_era_cpuid(kvm: &Kvm) -> Result<kvm_bindings::CpuId> {
    let host = kvm.get_supported_cpuid(kvm_bindings::KVM_MAX_CPUID_ENTRIES)?;
    let leaf = |n: u32| host.as_slice().iter().find(|e| e.function == n && e.index == 0).copied();
    let mut entries = Vec::new();
    if let Some(mut e) = leaf(0) {
        e.eax = 1;
        entries.push(e);
    }
    if let Some(mut e) = leaf(1) {
        const FEATURES: u32 = 1 << 0 | 1 << 1 | 1 << 2 | 1 << 3 | 1 << 4 | 1 << 5 | 1 << 8 | 1 << 23;
        e.eax = 0x0543;
        e.ebx = 0;
        e.ecx = 0;
        e.edx &= FEATURES;
        entries.push(e);
    }
    Ok(kvm_bindings::CpuId::from_entries(&entries).map_err(|e| format!("CPUID: {e:?}"))?)
}
