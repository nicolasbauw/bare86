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

const MEM_SIZE: usize = 0x30_0000;
const CODE_AT: u32 = 0x2000;
/// Code au-dessus du premier Mo (chemin rapide des contrôles, préfiltre des
/// pages de code), un cas sur deux.
const HIGH_CODE_AT: u32 = 0x10_1000;

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
        *g = match rng.below(4) {
            0 => 0x10000 + rng.below(0x80000) as u32,
            1 => 0x10_0000 + rng.below(0x1C_0000) as u32,
            _ => interesting(rng),
        };
    }
    // Pile valide (push, pop, call, ret).
    gpr[4] = 0x9_0000 + (rng.below(0x1000) as u32 & !3);
    if rng.chance(50) {
        gpr[5] = gpr[4] + 4 * rng.below(16) as u32;
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
        if ins.is_invalid() || ins.flow_control() != FlowControl::Next || classify(&ins, None, &Default::default()).is_none() {
            continue;
        }
        // La pile reste valide : pas d'écriture de ESP autrement que par
        // push/pop (sauf de temps en temps).
        let writes_esp = ins.op_count() > 0 && ins.op0_kind() == OpKind::Register && matches!(ins.op0_register(), Register::ESP | Register::SP);
        if writes_esp && !matches!(ins.mnemonic(), Mn::Push | Mn::Pop) && rng.chance(90) {
            continue;
        }
        if matches!(ins.mnemonic(), Mn::Popad | Mn::Leave) && rng.chance(50) {
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
        let code_at = if case % 2 == 0 { CODE_AT } else { HIGH_CODE_AT };
        let n = 1 + rng.below(12) as usize;
        let mut code = Vec::new();
        for _ in 0..n {
            code.extend(random_native(&mut rng));
            if rng.chance(10) {
                code.extend([0x70 + rng.below(16) as u8, 0x00]); // jcc +0
            }
            let here = code_at + code.len() as u32;
            match rng.below(40) {
                // call +0 ; add esp,4
                0 => code.extend([0xE8, 0, 0, 0, 0, 0x83, 0xC4, 0x04]),
                // push suite ; ret
                1 => {
                    code.push(0x68);
                    code.extend((here + 6).to_le_bytes());
                    code.push(0xC3);
                }
                // push suite ; ret 0 (forme imm16, sans rien libérer)
                2 => {
                    code.push(0x68);
                    code.extend((here + 8).to_le_bytes());
                    code.extend([0xC2, 0x00, 0x00]);
                }
                // mov ebx,suite ; jmp ebx  (ebx écrasé)
                3 => {
                    code.push(0xBB);
                    code.extend((here + 7).to_le_bytes());
                    code.extend([0xFF, 0xE3]);
                }
                // mov ebx,suite ; call ebx ; add esp,4
                4 => {
                    code.push(0xBB);
                    code.extend((here + 7).to_le_bytes());
                    code.extend([0xFF, 0xD3, 0x83, 0xC4, 0x04]);
                }
                _ => {}
            }
        }
        code.push(0xF4);
        mem_jit.write(code_at as usize, &code);
        mem_ref.write(code_at as usize, &code);
        jit.flush();

        let mut start = random_cpu(&mut rng);
        start.ip = code_at;
        // Référence : l'interpréteur, pas à pas, avec les drapeaux non
        // définis encore visibles à la fin.
        let mut reference = start.clone();
        let mut undefined = 0u32;
        let mut steps = 0;
        loop {
            let at = reference.ip as usize;
            if at + 15 > MEM_SIZE {
                break;
            }
            let mut bytes = [0u8; 15];
            mem_ref.read(at, &mut bytes);
            let ins: Instruction = Decoder::with_ip(32, &bytes, at as u64, DecoderOptions::NONE).decode();
            if ins.mnemonic() == Mn::Hlt {
                break;
            }
            // Boucle sans fin (code automodifiant) : cas écarté.
            if steps > 10_000 {
                skipped += 1;
                continue 'cases;
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
        let (done, _) = jit.run(&mut ours, &mem_jit, &mut NoMmio, 20_000, u64::MAX);
        translated_total += jit.stats.jit_instructions;
        jit.stats.jit_instructions = 0;

        // Arrêt avant `hlt` (instruction refusée) : les drapeaux que la suite
        // écrit avant de les lire sont morts, le contrôle d'adresse du
        // traducteur a pu les écraser.
        let dead = if reference.ip as usize != code_at as usize + code.len() - 1 { dead_flags(&mem_ref, reference.ip) } else { 0 };
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
            trace(&code, &start, fill, code_at);
        }
        if !diffs.is_empty() {
            let listing: Vec<String> = Decoder::with_ip(32, &code, u64::from(code_at), DecoderOptions::NONE)
                .into_iter()
                .map(|i| format!("  {:08X} {i}", i.ip()))
                .collect();
            failures.push(format!(
                "case {case}: bytes {}\n{}\nstart {:08X?} flags {:08X}\n{}",
                code.iter().map(|b| format!("{b:02X}")).collect::<String>(),
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


/// Drapeaux écrits avant d'être lus par le code qui suit `eip` (jusqu'à `hlt`).
fn dead_flags(mem: &GuestMem, eip: u32) -> u32 {
    if eip as usize + 256 > MEM_SIZE {
        return 0;
    }
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

/// Rejoue `code` pas à pas (blocs d'une instruction) contre l'interpréteur
/// et affiche le premier pas qui diverge.
fn trace(code: &[u8], start: &Cpu, fill: u64, code_at: u32) {
    let mem_jit = GuestMem::new(MEM_SIZE).unwrap();
    let mem_ref = GuestMem::new(MEM_SIZE).unwrap();
    for m in [&mem_jit, &mem_ref] {
        fill_memory(m, fill);
        m.write(code_at as usize, code);
    }
    let mut jit = Jit::new(&mem_jit).unwrap();
    jit.max_block = 1;
    let (mut o, mut r) = (start.clone(), start.clone());
    for step in 0..200 {
        let mut b = [0u8; 15];
        if r.ip as usize + 15 > MEM_SIZE {
            return;
        }
        mem_ref.read(r.ip as usize, &mut b);
        let ins = Decoder::with_ip(32, &b, u64::from(r.ip), DecoderOptions::NONE).decode();
        if ins.mnemonic() == Mn::Hlt {
            println!("trace: no divergence before hlt");
            return;
        }
        let before = r.clone();
        if run_burst(&mut r, &mem_ref, &mut NoMmio, u64::MAX, 1).reason == StopReason::Unsupported {
            println!("trace: reference stops at {:08X} {ins}", r.ip);
            return;
        }
        jit.run(&mut o, &mem_jit, &mut NoMmio, 1, u64::MAX);
        let mask = !(undefined_flags(&ins, &before) | 0x28);
        let (mut ma, mut mb) = (vec![0u8; MEM_SIZE], vec![0u8; MEM_SIZE]);
        mem_jit.read(0, &mut ma);
        mem_ref.read(0, &mut mb);
        if let Some(i) = (0..MEM_SIZE).find(|&i| ma[i] != mb[i]) {
            println!("trace: step {step} {:08X} {ins}: memory differs at {i:06X} (ours {:02X} ref {:02X})", before.ip, ma[i], mb[i]);
            return;
        }
        if o.gpr != r.gpr || o.ip != r.ip || o.flags & mask != r.flags & mask {
            println!("trace: step {step} {:08X} {ins}\n ours {:08X?} {:08X} eip {:08X}\n ref  {:08X?} {:08X} eip {:08X}\n before {:08X?} {:08X}", before.ip, o.gpr, o.flags, o.ip, r.gpr, r.flags, r.ip, before.gpr, before.flags);
            return;
        }
    }
}

#[test]
fn volatile_add_ebp() {
    let bytes = [0x81, 0xC5, 0x78, 0x56, 0x34, 0x12];
    let mut d = Decoder::with_ip(32, &bytes, 0x173E07, DecoderOptions::NONE);
    let ins = d.decode();
    let c = d.get_constant_offsets(&ins);
    println!("{ins} code {:?} imm {} {}", ins.code(), c.immediate_offset(), c.immediate_size());
    let h = volatile_form(&ins, c.immediate_offset());
    println!("{h:?}");
    let mut v = std::collections::HashSet::new();
    v.insert(0x173E07);
    assert!(classify(&ins, Some((c.immediate_offset(), c.immediate_size())), &v).is_some());
}

/// Boucle (saut arrière) : enchaînements répétés au-delà de `CHAIN_MAX`,
/// avec une écriture en mémoire et la pile à chaque tour.
#[test]
fn chained_loop() {
    let mem = GuestMem::new(MEM_SIZE).unwrap();
    // mov ecx,3000 ; l: add eax,ecx ; mov [ebx],eax ; push ecx ; pop edx ;
    // dec ecx ; jnz l ; hlt
    let code = [0xB9, 0xB8, 0x0B, 0, 0, 0x01, 0xC8, 0x89, 0x03, 0x51, 0x5A, 0x49, 0x75, 0xF7, 0xF4];
    mem.write(CODE_AT as usize, &code);
    let mut rng = Rng(5);
    let mut cpu = random_cpu(&mut rng);
    cpu.gpr[0] = 0;
    cpu.gpr[3] = 0x50000;
    let mut jit = Jit::new(&mem).unwrap();
    let (done, reason) = jit.run(&mut cpu, &mem, &mut NoMmio, u64::MAX, u64::MAX);
    assert_eq!(reason, StopReason::Unsupported);
    assert_eq!(done, 1 + 3000 * 6);
    assert_eq!(cpu.gpr[0], 3000 * 3001 / 2);
    assert_eq!(mem.read_u32(0x50000), 3000 * 3001 / 2);
    assert_eq!((cpu.gpr[1], cpu.gpr[2]), (0, 1));
    assert_eq!(cpu.ip, CODE_AT + code.len() as u32 - 1);
    assert!(jit.stats.blocks_run > 3000, "{:?}", jit.stats);
}

/// Appels répétés d'un sous-programme : les `ret` passent par la table de
/// répartition du code traduit, un `jmp ebx` aussi.
#[test]
fn call_ret_loop_through_dispatch() {
    let mem = GuestMem::new(MEM_SIZE).unwrap();
    // 2000: mov ecx,3000 ; mov ebx,2014h
    // 200A: l: call 2018 ; dec ecx ; jnz l ; jmp ebx      (→ 2014)
    // 2014: hlt ; nop ; nop ; nop
    // 2018: s: add eax,ecx ; adc edx,0 ; ret
    let code = [
        0xB9, 0xB8, 0x0B, 0, 0, 0xBB, 0x14, 0x20, 0, 0, // mov ecx ; mov ebx
        0xE8, 0x09, 0, 0, 0, 0x49, 0x75, 0xF8, 0xFF, 0xE3, // call ; dec ; jnz ; jmp ebx
        0xF4, 0x90, 0x90, 0x90, // hlt
        0x01, 0xC8, 0x83, 0xD2, 0x00, 0xC3, // add ; adc ; ret
    ];
    mem.write(CODE_AT as usize, &code);
    let mut rng = Rng(9);
    let mut cpu = random_cpu(&mut rng);
    cpu.gpr[0] = 0;
    cpu.gpr[2] = 0;
    cpu.flags &= !1;
    let stack = cpu.gpr[4];
    let mut jit = Jit::new(&mem).unwrap();
    let (done, reason) = jit.run(&mut cpu, &mem, &mut NoMmio, u64::MAX, u64::MAX);
    assert_eq!(reason, StopReason::Unsupported);
    assert_eq!(done, 2 + 3000 * 6 + 1);
    assert_eq!(cpu.gpr[0], 3000 * 3001 / 2);
    assert_eq!(cpu.gpr[4], stack);
    assert_eq!(cpu.ip, 0x2014);
    // Peu de retours à Rust : la plupart des `ret` sont servis par la table.
    assert!(jit.stats.blocks_run > 9000, "{:?}", jit.stats);
}

/// Divisions : opérandes tirés pour réussir le plus souvent (dividendes à
/// la `FixedDiv` de Doom, cas limites), comparées à l'interpréteur.
#[test]
fn divisions_match_interpreter() {
    let mut rng = Rng(77);
    let mem_jit = GuestMem::new(MEM_SIZE).unwrap();
    let mem_ref = GuestMem::new(MEM_SIZE).unwrap();
    let mut jit = Jit::new(&mem_jit).unwrap();
    // div ebx ; hlt | idiv ebx ; hlt | div dword [esi] ; hlt | idiv dword [esi] ; hlt
    let codes: [&[u8]; 4] = [&[0xF7, 0xF3, 0xF4], &[0xF7, 0xFB, 0xF4], &[0xF7, 0x36, 0xF4], &[0xF7, 0x3E, 0xF4]];
    let mut native = 0;
    for case in 0..20_000 {
        let code = codes[case % 4];
        let a = interesting(&mut rng) as i32;
        let b = match rng.below(4) {
            0 => interesting(&mut rng),
            1 => 1 + rng.below(0x10000) as u32,
            2 => (rng.next() as u32) | 1,
            _ => [0, 1, u32::MAX, 0x8000_0000][rng.below(4) as usize],
        };
        let (lo, hi) = match rng.below(3) {
            // FixedDiv2 : (a << 16) / b
            0 => ((a as u32) << 16, (a >> 16) as u32),
            // cdq
            1 => (a as u32, (a >> 31) as u32),
            _ => (rng.next() as u32, interesting(&mut rng)),
        };
        for m in [&mem_jit, &mem_ref] {
            m.write(CODE_AT as usize, code);
            m.write_u32(0x40000, b);
        }
        let mut start = random_cpu(&mut rng);
        (start.gpr[0], start.gpr[2], start.gpr[3], start.gpr[6]) = (lo, hi, b, 0x40000);
        let mut r = start.clone();
        while run_burst(&mut r, &mem_ref, &mut NoMmio, u64::MAX, 1).reason != StopReason::Unsupported {}
        let mut o = start.clone();
        jit.flush();
        let before = jit.stats.jit_instructions;
        jit.run(&mut o, &mem_jit, &mut NoMmio, u64::MAX, u64::MAX);
        native += jit.stats.jit_instructions - before;
        assert_eq!((o.gpr, o.ip), (r.gpr, r.ip), "case {case}: {:02X?} eax {lo:08X} edx {hi:08X} divisor {b:08X}", code);
    }
    // La plupart des divisions ont été faites par le code traduit.
    assert!(native > 10_000, "{native}");
}
