// SPDX-License-Identifier: GPL-3.0-or-later
//! Test différentiel du traducteur : des séquences d'instructions tirées au
//! hasard sont exécutées par le traducteur et par l'interpréteur à partir
//! du même état, puis registres, drapeaux définis et mémoire sont comparés.
//!
//! `cargo test --release jit_matches_interpreter -- --nocapture`
//! Variables : JIT_CASES (défaut 3000), JIT_SEED.

use iced_x86::{Decoder, DecoderOptions, Instruction};

use super::*;
use crate::interp::difftest::{Rng, interesting, undefined_flags};
use crate::interp::{Seg, run_burst};

const MEM_SIZE: usize = 0x11_0000;
const CODE_AT: u32 = 0x2000;

struct NoMmio;

impl Mmio for NoMmio {
    fn claims(&self, addr: u32, write: bool) -> bool {
        // Comme la machine : fenêtre VGA et ROM (en écriture) hors de la RAM.
        (0xA0000..0xC0000).contains(&addr) || (write && (0xC0000..0x100000).contains(&addr))
    }
    fn read8(&mut self, addr: u32) -> u8 {
        addr as u8 ^ 0x5A
    }
    fn write8(&mut self, _: u32, _: u8) {}
}

fn flat(selector: u16, typ: u8) -> Seg {
    Seg { selector, base: 0, limit: 0xFFFF_FFFF, typ, s: true, dpl: 0, present: true, db: true, g: true, unusable: false }
}

fn random_cpu(rng: &mut Rng) -> Cpu {
    let mut gpr = [0u32; 8];
    for g in &mut gpr {
        // Une moitié d'adresses de la RAM (accès mémoire), une moitié de
        // valeurs limites (arithmétique).
        *g = if rng.chance(50) { 0x10000 + rng.below(0x80000) as u32 } else { interesting(rng) };
    }
    let data = flat(0x10, 3);
    Cpu {
        gpr,
        ip: CODE_AT,
        flags: (rng.next() as u32 & 0x8D5) | 2 | 0x200,
        seg: [data, flat(0x08, 0xB), data, data, Seg::real(0), Seg::real(0)],
        stack32: true,
        idt_base: 0,
        cr0: 0x11,
        cr3: 0,
        cr4: 0,
        tr_base: 0,
        tr_limit: 0,
        seg_dirty: false,
    }
}

/// Instruction aléatoire que le traducteur exécute nativement, ou un saut
/// conditionnel court vers l'avant.
fn random_native(rng: &mut Rng) -> Vec<u8> {
    loop {
        let mut b = Vec::with_capacity(15);
        if rng.chance(15) {
            b.push(0x66);
        }
        if rng.chance(30) {
            b.push(0x0F);
        }
        while b.len() < 15 {
            b.push(rng.next() as u8);
        }
        let ins = Decoder::with_ip(32, &b, 0, DecoderOptions::NONE).decode();
        if ins.is_invalid() || !native_ok(&ins) {
            continue;
        }
        if ins.mnemonic() != Mn::Lea && rewrite(&ins).is_none() {
            continue;
        }
        return b[..ins.len()].to_vec();
    }
}

fn fill_memory(mem: &GuestMem, seed: u64) {
    let mut r = Rng(seed | 1);
    for a in (0..MEM_SIZE).step_by(8) {
        let v = r.next();
        mem.write(a, &v.to_le_bytes()[..(MEM_SIZE - a).min(8)]);
    }
}

#[test]
fn jit_matches_interpreter() {
    let cases: u64 = std::env::var("JIT_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let seed: u64 = std::env::var("JIT_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x1234_5678);
    let mut rng = Rng(seed | 1);
    let mem_jit = GuestMem::new(MEM_SIZE).unwrap();
    let mem_ref = GuestMem::new(MEM_SIZE).unwrap();
    let mut jit = Jit::new(&mem_jit).unwrap();
    let mut failures = Vec::new();
    let mut translated_total = 0;
    let mut skipped = 0;
    'cases: for case in 0..cases {
        let fill = rng.next();
        fill_memory(&mem_jit, fill);
        fill_memory(&mem_ref, fill);
        // Séquence : instructions natives, quelques sauts conditionnels
        // courts vers l'avant, puis `hlt` (rendu au processeur).
        let n = 1 + rng.below(12) as usize;
        let mut code = Vec::new();
        for _ in 0..n {
            code.extend(random_native(&mut rng));
            if rng.chance(10) {
                code.extend([0x70 + rng.below(16) as u8, 0x00]); // jcc +0
            }
        }
        code.push(0xF4);
        mem_jit.write(CODE_AT as usize, &code);
        mem_ref.write(CODE_AT as usize, &code);
        jit.flush();

        let start = random_cpu(&mut rng);
        // Référence : l'interpréteur, pas à pas, avec les drapeaux non
        // définis encore visibles à la fin.
        let mut reference = start.clone();
        let mut undefined = 0u32;
        let mut steps = 0;
        loop {
            let at = reference.ip as usize;
            let mut bytes = [0u8; 15];
            mem_ref.read(at, &mut bytes);
            let ins: Instruction = Decoder::with_ip(32, &bytes, at as u64, DecoderOptions::NONE).decode();
            if ins.mnemonic() == Mn::Hlt {
                break;
            }
            // Lecture d'un drapeau non défini : cas sans référence fiable.
            if flags_read(&ins) & undefined != 0 {
                skipped += 1;
                continue 'cases;
            }
            let before = reference.clone();
            let b = run_burst(&mut reference, &mem_ref, &mut NoMmio, u64::MAX, 1);
            if b.reason == StopReason::Unsupported {
                break;
            }
            undefined = (undefined & !flags_written(&ins)) | undefined_flags(&ins, &before);
            steps += 1;
        }

        let mut ours = start.clone();
        let (done, _) = jit.run(&mut ours, &mem_jit, &mut NoMmio, u64::MAX);
        translated_total += jit.stats.jit_instructions;
        jit.stats.jit_instructions = 0;

        // Arrêt avant `hlt` (instruction refusée) : les drapeaux que la suite
        // écrit avant de les lire sont morts, le contrôle d'adresse du
        // traducteur a pu les écraser.
        let dead = if reference.ip as usize != CODE_AT as usize + code.len() - 1 { dead_flags(&mem_ref, reference.ip) } else { 0 };
        let mask = !(undefined | dead | 0x20 | 0x8);
        let mut diffs = Vec::new();
        if ours.gpr != reference.gpr {
            diffs.push(format!("gpr ours {:08X?}\n     ref  {:08X?}", ours.gpr, reference.gpr));
        }
        if ours.flags & mask != reference.flags & mask {
            diffs.push(format!("flags ours {:08X} ref {:08X} (undefined {undefined:03X})", ours.flags, reference.flags));
        }
        if ours.ip != reference.ip {
            diffs.push(format!("eip ours {:08X} ref {:08X}", ours.ip, reference.ip));
        }
        if done != steps {
            diffs.push(format!("instructions ours {done} ref {steps}"));
        }
        let (mut a, mut b) = (vec![0u8; MEM_SIZE], vec![0u8; MEM_SIZE]);
        mem_jit.read(0, &mut a);
        mem_ref.read(0, &mut b);
        if let Some(i) = (0..MEM_SIZE).find(|&i| a[i] != b[i]) {
            diffs.push(format!("memory at {i:06X}: ours {:02X} ref {:02X}", a[i], b[i]));
        }
        if !diffs.is_empty() && std::env::var_os("JIT_TRACE").is_some() {
            bisect(&code, &start, fill);
        }
        if !diffs.is_empty() {
            let listing: Vec<String> = Decoder::with_ip(32, &code, u64::from(CODE_AT), DecoderOptions::NONE)
                .into_iter()
                .map(|i| format!("  {:08X} {i}", i.ip()))
                .collect();
            failures.push(format!(
                "case {case}:\n{}\nstart {:08X?} flags {:08X}\n{}",
                listing.join("\n"),
                start.gpr,
                start.flags,
                diffs.join("\n")
            ));
            if failures.len() >= 10 {
                break;
            }
        }
    }
    println!("{cases} cases ({skipped} skipped: undefined flag read), {translated_total} instructions run by translated code, stats {:?}", jit.stats);
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n\n"));
}

/// Rejoue `code` préfixe par préfixe et affiche le premier qui diverge.
fn bisect(code: &[u8], start: &Cpu, fill: u64) {
    let instrs: Vec<Instruction> = Decoder::with_ip(32, code, u64::from(CODE_AT), DecoderOptions::NONE).into_iter().collect();
    let mem_jit = GuestMem::new(MEM_SIZE).unwrap();
    let mem_ref = GuestMem::new(MEM_SIZE).unwrap();
    let mut jit = Jit::new(&mem_jit).unwrap();
    for k in 1..instrs.len() {
        let end = (instrs[k - 1].next_ip() as u32 - CODE_AT) as usize;
        let mut c = code[..end].to_vec();
        c.push(0xF4);
        fill_memory(&mem_jit, fill);
        fill_memory(&mem_ref, fill);
        mem_jit.write(CODE_AT as usize, &c);
        mem_ref.write(CODE_AT as usize, &c);
        jit.flush();
        let mut r = start.clone();
        while run_burst(&mut r, &mem_ref, &mut NoMmio, u64::MAX, 1).reason != StopReason::Unsupported {}
        let mut o = start.clone();
        jit.run(&mut o, &mem_jit, &mut NoMmio, u64::MAX);
        let mut before = start.clone();
        let mut stepped = 0;
        // État avant la k-ième instruction (référence), pour ses drapeaux non définis.
        {
            let mut t = start.clone();
            let m2 = GuestMem::new(MEM_SIZE).unwrap();
            fill_memory(&m2, fill);
            m2.write(CODE_AT as usize, &c);
            while stepped + 1 < k && run_burst(&mut t, &m2, &mut NoMmio, u64::MAX, 1).reason != StopReason::Unsupported {
                stepped += 1;
            }
            before = if stepped + 1 == k { t } else { before };
        }
        let mask = !(undefined_flags(&instrs[k - 1], &before) | 0x28);
        let (mut a, mut b) = (vec![0u8; MEM_SIZE], vec![0u8; MEM_SIZE]);
        mem_jit.read(0, &mut a);
        mem_ref.read(0, &mut b);
        if o.gpr != r.gpr || o.flags & mask != r.flags & mask || a != b {
            println!("first divergence after {k} instructions (last: {}):\n ours {:08X?} {:08X}\n ref  {:08X?} {:08X}\n stats {:?}", instrs[k - 1], o.gpr, o.flags, r.gpr, r.flags, jit.stats);
            return;
        }
    }
}

/// Drapeaux écrits avant d'être lus par le code qui suit `eip` (jusqu'à `hlt`).
fn dead_flags(mem: &GuestMem, eip: u32) -> u32 {
    let mut bytes = vec![0u8; 256];
    mem.read(eip as usize, &mut bytes);
    let (mut dead, mut read) = (0, 0);
    for ins in Decoder::with_ip(32, &bytes, u64::from(eip), DecoderOptions::NONE) {
        if ins.mnemonic() == Mn::Hlt || ins.is_invalid() {
            break;
        }
        read |= flags_read(&ins) & !dead;
        dead |= flags_written(&ins) & !read;
    }
    dead
}
