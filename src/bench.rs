// SPDX-License-Identifier: GPL-3.0-or-later
//! Étape 0 : coût d'une sortie de VM (port d'E/S, MMIO en A0000).

use std::time::{Duration, Instant};

use kvm_ioctls::{IoEventAddress, VcpuExit};

use crate::throttle::InstrCounter;
use crate::interp::{Mmio, continue_in_host};
use crate::backend::Backend;
use crate::kvm::{KvmVm as Machine, VGA_WINDOW};
use crate::bios::ROM_SEG;
use crate::machine::HOST_CALL_PORT;
use crate::mem::GuestMem;

/// 1 Mo + HMA.
const MEM_SIZE: usize = 0x11_0000;

fn install_ivt(m: &Machine) {
    crate::bios::install_rom(&m.mem);
}

fn load_com(m: &mut Machine, image: &[u8]) -> crate::backend::Result<()> {
    let cpu = crate::dos::load_com(&m.mem, image)?;
    m.set_state(&cpu)
}

const OUTER: u16 = 100;
const INNER: u16 = 10_000;
const ITERATIONS: u64 = OUTER as u64 * INNER as u64;

/// Boucle `OUTER` × `INNER` autour de `op`, ES:DI = A000:0000.
/// ```text
///     mov ax, 0A000h ; mov es, ax ; xor di, di ; mov bx, OUTER
/// l1: mov cx, INNER
/// l2: <op>
///     loop l2 ; dec bx ; jnz l1 ; mov ax, 4C00h ; int 21h
/// ```
fn loop_program(op: &[u8]) -> Vec<u8> {
    let k = op.len() as i8;
    let mut p = vec![0xB8, 0x00, 0xA0, 0x8E, 0xC0, 0x31, 0xFF, 0xBB];
    p.extend(OUTER.to_le_bytes());
    p.push(0xB9);
    p.extend(INNER.to_le_bytes());
    p.extend(op);
    p.extend([0xE2, (-(k + 2)) as u8, 0x4B, 0x75, (-(k + 8)) as u8]);
    p.extend([0xB8, 0x00, 0x4C, 0xCD, 0x21]);
    p
}

#[derive(Default)]
struct Stats {
    io: u64,
    mmio_read: u64,
    mmio_write: u64,
    coalesced: u64,
    /// Instructions exécutées par l'interpréteur hôte.
    interpreted: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Chaque accès MMIO sort vers l'espace utilisateur.
    Exit,
    /// Écritures MMIO regroupées dans l'anneau du noyau.
    Coalesced,
    /// Rafales exécutées par l'interpréteur hôte.
    Interp,
}

/// Fenêtre VGA factice : 64 Ko de mémoire ordinaire.
struct FakeVga(Vec<u8>);

impl Mmio for FakeVga {
    fn claims(&self, addr: u32, _write: bool) -> bool {
        VGA_WINDOW.contains(&(addr as usize))
    }
    fn read8(&mut self, addr: u32) -> u8 {
        self.0[addr as usize - VGA_WINDOW.start]
    }
    fn write8(&mut self, addr: u32, v: u8) {
        self.0[addr as usize - VGA_WINDOW.start] = v;
    }
}

/// Instructions sans accès VRAM avant de rendre la main à la VM.
const IDLE_LIMIT: u64 = 256;
/// Plafond d'instructions par rafale.
const BURST_BUDGET: u64 = 100_000;

fn run_case(op: &[u8], mode: Mode) -> Result<(Duration, Stats), Box<dyn std::error::Error>> {
    let mut m = Machine::new(MEM_SIZE)?;
    if mode == Mode::Coalesced {
        m.vm.register_coalesced_mmio(IoEventAddress::Mmio(0xA0000), 0x10000)?;
        m.vcpu.map_coalesced_mmio_ring()?;
    }
    install_ivt(&m);
    load_com(&mut m, &loop_program(op))?;
    let mut vga = FakeVga(vec![0; VGA_WINDOW.len()]);
    let mut st = Stats::default();
    let start = Instant::now();
    loop {
        let (host_call, mmio) = match m.vcpu.run()? {
            VcpuExit::IoOut(port, _) => {
                st.io += 1;
                (port == HOST_CALL_PORT, false)
            }
            VcpuExit::MmioRead(addr, data) => {
                for (i, b) in data.iter_mut().enumerate() {
                    *b = vga.read8(addr as u32 + i as u32);
                }
                st.mmio_read += 1;
                (false, true)
            }
            VcpuExit::MmioWrite(addr, data) => {
                for (i, b) in data.iter().enumerate() {
                    vga.write8(addr as u32 + i as u32, *b);
                }
                st.mmio_write += 1;
                (false, true)
            }
            other => return Err(format!("sortie inattendue : {other:?}").into()),
        };
        if mode == Mode::Coalesced {
            while m.vcpu.coalesced_mmio_read()?.is_some() {
                st.coalesced += 1;
            }
        }
        if mmio && mode == Mode::Interp {
            st.interpreted += continue_in_host(&mut m, &mut vga, IDLE_LIMIT, BURST_BUDGET)?.instructions;
        }
        if host_call && m.vcpu.get_sregs()?.cs.selector == ROM_SEG {
            // Contrôle : BX = CX = 0 en fin de boucle.
            let r = m.vcpu.get_regs()?;
            if r.rbx & 0xFFFF != 0 || r.rcx & 0xFFFF != 0 {
                return Err(format!("état final faux : BX={:04X} CX={:04X}", r.rbx, r.rcx).into());
            }
            return Ok((start.elapsed(), st));
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    const WRITE: &[u8] = &[0x26, 0x88, 0x05]; // mov es:[di], al
    const READ: &[u8] = &[0x26, 0x8A, 0x05]; // mov al, es:[di]
    let cases: [(&str, &[u8], Mode); 8] = [
        ("boucle vide (nop)", &[0x90], Mode::Exit),
        // push cx ; push bx ; xor ax, ax ; cpuid ; pop bx ; pop cx
        ("cpuid (sortie noyau seule)", &[0x51, 0x53, 0x31, 0xC0, 0x0F, 0xA2, 0x5B, 0x59], Mode::Exit),
        ("out 80h, al", &[0xE6, 0x80], Mode::Exit),
        ("écriture VRAM (MMIO)", WRITE, Mode::Exit),
        ("lecture VRAM (MMIO)", READ, Mode::Exit),
        ("écriture VRAM (regroupé)", WRITE, Mode::Coalesced),
        ("écriture VRAM (interprété)", WRITE, Mode::Interp),
        ("lecture VRAM (interprété)", READ, Mode::Interp),
    ];
    // Accès VRAM par image du critère (20 000 à 70 Hz).
    let per_second = 20_000.0 * 70.0;
    let mut base_ns = 0.0;
    println!("{ITERATIONS} itérations par cas\n");
    println!("{:<28} {:>9} {:>11} {:>9}  sorties io/lect/écr, regroupées, interprétées", "cas", "total ms", "ns/accès", "% CPU");
    for (i, (name, op, mode)) in cases.iter().enumerate() {
        let (t, st) = run_case(op, *mode)?;
        let ns = t.as_nanos() as f64 / ITERATIONS as f64;
        if i == 0 {
            base_ns = ns;
        }
        let cost = (ns - base_ns).max(0.0);
        let cpu = cost * per_second / 1e7;
        println!(
            "{name:<28} {:>9.1} {cost:>11.0} {cpu:>8.0}%  {}/{}/{}, {}, {}",
            t.as_secs_f64() * 1e3,
            st.io,
            st.mmio_read,
            st.mmio_write,
            st.coalesced,
            st.interpreted
        );
    }
    println!("\n% CPU : part d'un cœur pour 20 000 accès × 70 images/s (critère étape 0).");
    Ok(())
}

/// Débits visés en MIPS ; `None` = sans bridage.
const TARGETS: [Option<f64>; 6] = [Some(1.0), Some(5.0), Some(25.0), Some(100.0), Some(500.0), None];
/// Durée de chaque cas.
const THROTTLE_RUN: Duration = Duration::from_millis(500);
/// Tranche de bridage.
const SLICE: Duration = Duration::from_millis(1);

/// Boucle infinie : `xor eax, eax ; l: inc eax ; jmp l` (2 instructions par tour).
const SPIN_COM: &[u8] = &[0x66, 0x31, 0xC0, 0x66, 0x40, 0xEB, 0xFC];

fn throttle_case(target: Option<f64>) -> Result<(f64, f64, u64), Box<dyn std::error::Error>> {
    let mut m = Machine::new(MEM_SIZE)?;
    install_ivt(&m);
    load_com(&mut m, SPIN_COM)?;
    let period = match target {
        Some(mips) => (mips * 1e6 * SLICE.as_secs_f64()) as u64,
        None => 10_000_000,
    };
    let counter = InstrCounter::new(period)?;
    let start = Instant::now();
    let mut interrupts = 0u64;
    loop {
        match m.vcpu.run() {
            Err(e) if e.errno() == libc::EINTR => {}
            Ok(exit) => return Err(format!("sortie inattendue : {exit:?}").into()),
            Err(e) => return Err(e.into()),
        }
        interrupts += 1;
        let now = start.elapsed();
        if now >= THROTTLE_RUN {
            break;
        }
        if let Some(mips) = target {
            let executed = counter.read()? as f64;
            let due = Duration::from_secs_f64(executed / (mips * 1e6));
            if due > now {
                std::thread::sleep(due - now);
            }
        }
        counter.arm()?;
    }
    let secs = start.elapsed().as_secs_f64();
    let guest = 2.0 * (m.vcpu.get_regs()?.rax as u32) as f64;
    let perf = counter.read()? as f64;
    Ok((guest / secs / 1e6, perf / secs / 1e6, interrupts))
}

pub fn run_throttle() -> Result<(), Box<dyn std::error::Error>> {
    println!("Tranches de {} ms, {} ms par cas\n", SLICE.as_millis(), THROTTLE_RUN.as_millis());
    println!("{:>12} {:>14} {:>12} {:>8} {:>12}", "visé MIPS", "invité MIPS", "perf MIPS", "écart", "interruptions");
    for target in TARGETS {
        let (guest, perf, n) = throttle_case(target)?;
        let (label, err) = match target {
            Some(t) => (format!("{t}"), format!("{:+.1}%", (guest / t - 1.0) * 100.0)),
            None => ("libre".into(), "-".into()),
        };
        println!("{label:>12} {guest:>14.1} {perf:>12.1} {err:>8} {n:>12}");
    }
    println!("\ninvité : compté par le programme lui-même (inc eax) ; perf : compteur matériel.");
    Ok(())
}

const REMAP_COUNT: u16 = 10_000;

/// Changement de plan par `out 3C5h` puis `op`, `REMAP_COUNT` fois.
/// ```text
///     mov ax, 0A000h ; mov es, ax ; xor di, di ; mov dx, 3C5h ; mov bx, N
/// l1: inc al ; out dx, al ; <op> ; dec bx ; jnz l1 ; mov ax, 4C00h ; int 21h
/// ```
fn remap_program(op: &[u8]) -> Vec<u8> {
    let k = op.len() as i8;
    let mut p = vec![0xB8, 0x00, 0xA0, 0x8E, 0xC0, 0x31, 0xFF, 0xBA, 0xC5, 0x03, 0xBB];
    p.extend(REMAP_COUNT.to_le_bytes());
    p.extend([0xFE, 0xC0, 0xEE]);
    p.extend(op);
    p.extend([0x4B, 0x75, (-(k + 6)) as u8]);
    p.extend([0xB8, 0x00, 0x4C, 0xCD, 0x21]);
    p
}

/// Les 4 plans dans un memfd ; la fenêtre VGA est une vue de 64 Ko dessus,
/// dont on change le plan visé par `mmap(MAP_FIXED)` sans toucher au slot KVM.
struct PlaneFile {
    fd: i32,
    view: *mut u8,
}

impl PlaneFile {
    fn new() -> std::io::Result<Self> {
        let len = VGA_WINDOW.len();
        // SAFETY: appels système vérifiés.
        unsafe {
            let fd = libc::memfd_create(c"vga-planes".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 || libc::ftruncate(fd, 4 * len as i64) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let view = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if view == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self { fd, view: view.cast() })
        }
    }

    fn select(&self, plane: usize) -> std::io::Result<()> {
        let len = VGA_WINDOW.len();
        // SAFETY: remplace la vue existante, même adresse et même taille.
        let p = unsafe {
            libc::mmap(
                self.view.cast(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED,
                self.fd,
                (plane * len) as i64,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn first_byte(&self, plane: usize) -> u8 {
        let mut b = 0u8;
        // SAFETY: lecture d'un octet dans le fichier.
        unsafe { libc::pread(self.fd, (&raw mut b).cast(), 1, (plane * VGA_WINDOW.len()) as i64) };
        b
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Remap {
    None,
    /// Suppression puis recréation du slot KVM.
    Slot,
    /// Slot fixe, vue hôte remappée (memfd).
    HostView,
}

fn remap_case(op: &[u8], remap: Remap) -> Result<f64, Box<dyn std::error::Error>> {
    let mut m = Machine::new(MEM_SIZE)?;
    let planes: Vec<GuestMem> = (0..4).map(|_| GuestMem::new(VGA_WINDOW.len())).collect::<Result<_, _>>()?;
    let file = PlaneFile::new()?;
    if remap == Remap::HostView {
        m.map_vga_window_raw(file.view as u64, false)?;
    } else {
        m.map_vga_window(Some(&planes[0]))?;
    }
    install_ivt(&m);
    load_com(&mut m, &remap_program(op))?;
    let start = Instant::now();
    loop {
        let (port, value) = match m.vcpu.run()? {
            VcpuExit::IoOut(port, data) => (port, data[0]),
            other => return Err(format!("sortie inattendue : {other:?}").into()),
        };
        if port == 0x3C5 && remap == Remap::Slot {
            m.map_vga_window(Some(&planes[usize::from(value & 3)]))?;
        } else if port == 0x3C5 && remap == Remap::HostView {
            file.select(usize::from(value & 3))?;
        } else if port == HOST_CALL_PORT && m.vcpu.get_sregs()?.cs.selector == ROM_SEG {
            break;
        }
    }
    let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(REMAP_COUNT);
    // Contrôle : chaque plan a bien reçu des écritures (sauf cas sans écriture).
    let written = match remap {
        Remap::None => true,
        Remap::Slot => planes.iter().all(|p| p.read_u8(0) != 0),
        Remap::HostView => (0..4).all(|i| file.first_byte(i) != 0),
    };
    if !op.is_empty() && !written {
        return Err("un plan n'a reçu aucune écriture".into());
    }
    Ok(us)
}

pub fn run_remap() -> Result<(), Box<dyn std::error::Error>> {
    // Remappages seuls, sans exécuter l'invité.
    let m = Machine::new(MEM_SIZE)?;
    let planes: Vec<GuestMem> = (0..4).map(|_| GuestMem::new(VGA_WINDOW.len())).collect::<Result<_, _>>()?;
    let start = Instant::now();
    for i in 0..usize::from(REMAP_COUNT) {
        m.map_vga_window(Some(&planes[i & 3]))?;
    }
    let ioctl_us = start.elapsed().as_secs_f64() * 1e6 / f64::from(REMAP_COUNT);

    const WRITE: &[u8] = &[0x26, 0x88, 0x05]; // mov es:[di], al
    // mov cx, 16000 ; xor di, di ; rep stosb  (un quart d'écran mode X)
    const FILL: &[u8] = &[0xB9, 0x80, 0x3E, 0x31, 0xFF, 0xF3, 0xAA];
    println!("{REMAP_COUNT} changements de plan par cas\n");
    println!("{:<46} {:>10}", "cas", "µs/changement");
    println!("{:<46} {ioctl_us:>10.1}", "ioctl seules (supprimer + recréer le slot)");
    let cases: [(&str, &[u8], Remap); 7] = [
        ("out 3C5h seul, sans remappage", &[], Remap::None),
        ("out 3C5h + 16 000 écritures, sans remappage", FILL, Remap::None),
        ("slot : out 3C5h + remappage", &[], Remap::Slot),
        ("slot : idem + 1 écriture", WRITE, Remap::Slot),
        ("slot : idem + 16 000 écritures", FILL, Remap::Slot),
        ("vue hôte : out 3C5h + remappage + 1 écriture", WRITE, Remap::HostView),
        ("vue hôte : idem + 16 000 écritures", FILL, Remap::HostView),
    ];
    for (name, op, remap) in cases {
        println!("{name:<46} {:>10.1}", remap_case(op, remap)?);
    }
    Ok(())
}
