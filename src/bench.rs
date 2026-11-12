// SPDX-License-Identifier: GPL-3.0-or-later
//! Étape 0 : coût d'une sortie de VM (port d'E/S, MMIO en A0000).

use std::time::{Duration, Instant};

use kvm_ioctls::{IoEventAddress, VcpuExit};

use crate::throttle::InstrCounter;
use crate::vm::Machine;
use crate::{HOST_CALL_PORT, MEM_SIZE, ROM_SEG, install_ivt, load_com};

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
}

fn run_case(op: &[u8], coalesced: bool) -> Result<(Duration, Stats), Box<dyn std::error::Error>> {
    let mut m = Machine::new(MEM_SIZE)?;
    if coalesced {
        m.vm.register_coalesced_mmio(IoEventAddress::Mmio(0xA0000), 0x10000)?;
        m.vcpu.map_coalesced_mmio_ring()?;
    }
    install_ivt(&m);
    load_com(&m, &loop_program(op))?;
    let mut st = Stats::default();
    let start = Instant::now();
    loop {
        let host_call = match m.vcpu.run()? {
            VcpuExit::IoOut(port, _) => {
                st.io += 1;
                port == HOST_CALL_PORT
            }
            VcpuExit::MmioRead(_, data) => {
                data.fill(0);
                st.mmio_read += 1;
                false
            }
            VcpuExit::MmioWrite(..) => {
                st.mmio_write += 1;
                false
            }
            other => return Err(format!("sortie inattendue : {other:?}").into()),
        };
        if coalesced {
            while m.vcpu.coalesced_mmio_read()?.is_some() {
                st.coalesced += 1;
            }
        }
        if host_call && m.vcpu.get_sregs()?.cs.selector == ROM_SEG {
            return Ok((start.elapsed(), st));
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cases: [(&str, &[u8], bool); 6] = [
        ("boucle vide (nop)", &[0x90], false),
        // push cx ; push bx ; xor ax, ax ; cpuid ; pop bx ; pop cx
        ("cpuid (sortie noyau seule)", &[0x51, 0x53, 0x31, 0xC0, 0x0F, 0xA2, 0x5B, 0x59], false),
        ("out 80h, al", &[0xE6, 0x80], false),
        ("mov es:[di], al (MMIO)", &[0x26, 0x88, 0x05], false),
        ("mov al, es:[di] (MMIO)", &[0x26, 0x8A, 0x05], false),
        ("mov es:[di], al (regroupé)", &[0x26, 0x88, 0x05], true),
    ];
    // Accès VRAM par image du critère (20 000 à 70 Hz).
    let per_second = 20_000.0 * 70.0;
    let mut base_ns = 0.0;
    println!("{ITERATIONS} itérations par cas\n");
    println!("{:<28} {:>9} {:>11} {:>9}  sorties (io/lect/écr/regroupées)", "cas", "total ms", "ns/accès", "% CPU");
    for (i, (name, op, coalesced)) in cases.iter().enumerate() {
        let (t, st) = run_case(op, *coalesced)?;
        let ns = t.as_nanos() as f64 / ITERATIONS as f64;
        if i == 0 {
            base_ns = ns;
        }
        let cost = (ns - base_ns).max(0.0);
        let cpu = cost * per_second / 1e7;
        println!(
            "{name:<28} {:>9.1} {cost:>11.0} {cpu:>8.0}%  {}/{}/{}/{}",
            t.as_secs_f64() * 1e3,
            st.io,
            st.mmio_read,
            st.mmio_write,
            st.coalesced
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
    load_com(&m, SPIN_COM)?;
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
