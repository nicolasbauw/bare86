// SPDX-License-Identifier: GPL-3.0-or-later
//! Étape 0 : coût d'une sortie de VM (port d'E/S, MMIO en A0000).

use std::time::{Duration, Instant};

use kvm_ioctls::{IoEventAddress, VcpuExit};

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
