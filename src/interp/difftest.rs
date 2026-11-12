// SPDX-License-Identifier: GPL-3.0-or-later
//! Test différentiel : chaque instruction est exécutée par le processeur
//! (pas à pas KVM) et par l'interpréteur, à partir du même état aléatoire,
//! puis registres, drapeaux définis et mémoire sont comparés.
//!
//! `cargo test --release interpreter_matches_cpu -- --nocapture`
//! Variables : DIFFTEST_CASES (défaut 20000), DIFFTEST_SEED.

use std::collections::BTreeMap;

use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic as Mn, OpKind, Register as R};
use kvm_bindings::{KVM_GUESTDBG_ENABLE, KVM_GUESTDBG_SINGLESTEP, kvm_guest_debug, kvm_regs};
use kvm_ioctls::VcpuExit;

use super::alu::*;
use super::{Cpu, Mmio, Seg, step_once};
use crate::kvm::KvmVm;
use crate::machine::{HOST_CALL_PORT, ROM_SEG};
use crate::mem::GuestMem;

/// 1 Mo + HMA.
const MEM_SIZE: usize = 0x11_0000;

struct NoMmio;

impl Mmio for NoMmio {
    fn claims(&self, _: u32, _: bool) -> bool {
        false
    }
    fn read8(&mut self, _: u32) -> u8 {
        unreachable!()
    }
    fn write8(&mut self, _: u32, _: u8) {
        unreachable!()
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

/// Valeurs de registre favorisant les cas limites.
fn interesting(rng: &mut Rng) -> u32 {
    const EDGES: [u32; 9] = [0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF];
    match rng.below(10) {
        0 => 0,
        1 => 1,
        2 => EDGES[rng.below(EDGES.len() as u64) as usize],
        3 | 4 => rng.below(0x20) as u32,
        5..=7 => rng.next() as u32 & 0xFFFF,
        _ => rng.next() as u32,
    }
}

fn random_instruction(rng: &mut Rng) -> Vec<u8> {
    const PREFIXES: [u8; 12] = [0x66, 0x66, 0x67, 0x67, 0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65, 0xF2, 0xF3];
    let mut b = Vec::with_capacity(15);
    for _ in 0..rng.below(3) {
        b.push(PREFIXES[rng.below(PREFIXES.len() as u64) as usize]);
    }
    if rng.chance(25) {
        b.push(0x0F);
    }
    while b.len() < 15 {
        b.push(rng.next() as u8);
    }
    b
}

/// Instructions à ne pas tester en pas à pas (le pas à pas natif ne
/// s'arrête pas juste après : interruptions, ombre de MOV SS…).
fn excluded(ins: &Instruction) -> bool {
    let writes_ss = ins.op_count() > 0 && ins.op_kind(0) == OpKind::Register && ins.op_register(0) == R::SS;
    writes_ss
        || matches!(
            ins.mnemonic(),
            Mn::Int | Mn::Int1 | Mn::Int3 | Mn::Into | Mn::Hlt | Mn::Lss | Mn::In | Mn::Out
        )
}

fn op_size(ins: &Instruction) -> u32 {
    match ins.op_kind(0) {
        OpKind::Register => ins.op_register(0).size() as u32,
        OpKind::Memory => ins.memory_size().size() as u32,
        _ => 0,
    }
}

/// Drapeaux non définis par l'architecture pour cette instruction.
fn undefined_flags(ins: &Instruction, before: &Cpu) -> u32 {
    let shift_count = |n: u32| -> u32 {
        let c = match ins.op_kind(n) {
            OpKind::Register => before.gpr[1] & 0xFF,
            _ => ins.immediate(n) as u32,
        };
        c & 0x1F
    };
    match ins.mnemonic() {
        Mn::And | Mn::Or | Mn::Xor | Mn::Test => AF,
        Mn::Shl | Mn::Sal | Mn::Shr | Mn::Sar => match shift_count(1) {
            0 => 0,
            c => {
                let mut u = AF;
                if c != 1 {
                    u |= OF;
                }
                if c >= 8 * op_size(ins) {
                    u |= CF;
                }
                u
            }
        },
        Mn::Rol | Mn::Ror | Mn::Rcl | Mn::Rcr => match shift_count(1) {
            0 | 1 => 0,
            _ => OF,
        },
        Mn::Shld | Mn::Shrd => match shift_count(2) {
            0 => 0,
            1 => AF,
            _ => AF | OF,
        },
        Mn::Mul | Mn::Imul => SF | ZF | AF | PF,
        Mn::Div | Mn::Idiv => ARITH,
        Mn::Bsf | Mn::Bsr => CF | OF | SF | AF | PF,
        Mn::Bt | Mn::Bts | Mn::Btr | Mn::Btc => OF | SF | AF | PF,
        Mn::Aaa | Mn::Aas => OF | SF | ZF | PF,
        Mn::Aam | Mn::Aad => OF | AF | CF,
        Mn::Daa | Mn::Das => OF,
        _ => 0,
    }
}

struct Harness {
    m: KvmVm,
    shadow: GuestMem,
    image: Vec<u8>,
    base_sregs: kvm_bindings::kvm_sregs,
}

impl Harness {
    fn new(rng: &mut Rng) -> Self {
        let m = KvmVm::new_flat(MEM_SIZE).expect("VM");
        let shadow = GuestMem::new(MEM_SIZE).expect("mémoire");
        let mut image: Vec<u8> = (0..MEM_SIZE).map(|_| rng.next() as u8).collect();
        // IVT vers les stubs `out 0xE0, al ; iret` en F000:n*16.
        for n in 0..256usize {
            image[n * 4..n * 4 + 2].copy_from_slice(&((n * 16) as u16).to_le_bytes());
            image[n * 4 + 2..n * 4 + 4].copy_from_slice(&ROM_SEG.to_le_bytes());
            let stub = ((ROM_SEG as usize) << 4) + n * 16;
            image[stub..stub + 3].copy_from_slice(&[0xE6, HOST_CALL_PORT as u8, 0xCF]);
        }
        m.mem.write(0, &image);
        shadow.write(0, &image);
        let base_sregs = m.vcpu.get_sregs().expect("sregs");
        Self { m, shadow, image, base_sregs }
    }

    /// Exécute l'instruction nativement ; Err si le processeur n'a pas
    /// simplement terminé l'instruction.
    fn native(&mut self, cpu: &Cpu) -> Result<Cpu, String> {
        let mut sregs = self.base_sregs;
        for (dst, src) in [&mut sregs.es, &mut sregs.cs, &mut sregs.ss, &mut sregs.ds, &mut sregs.fs, &mut sregs.gs]
            .into_iter()
            .zip(cpu.seg)
        {
            dst.selector = src.selector;
            dst.base = src.base.into();
            dst.limit = src.limit;
        }
        let g = cpu.gpr.map(u64::from);
        let regs = kvm_regs {
            rax: g[0],
            rcx: g[1],
            rdx: g[2],
            rbx: g[3],
            rsp: g[4],
            rbp: g[5],
            rsi: g[6],
            rdi: g[7],
            rip: cpu.ip.into(),
            rflags: cpu.flags.into(),
            ..Default::default()
        };
        self.m.vcpu.set_sregs(&sregs).map_err(|e| e.to_string())?;
        self.m.vcpu.set_regs(&regs).map_err(|e| e.to_string())?;
        // À réarmer à chaque état : KVM ne pose TF que si l'IP courante est
        // celle mémorisée au moment de cet appel.
        let dbg = kvm_guest_debug { control: KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP, ..Default::default() };
        self.m.vcpu.set_guest_debug(&dbg).map_err(|e| e.to_string())?;
        let exit = match self.m.vcpu.run() {
            Ok(VcpuExit::Debug(_)) => None,
            Ok(VcpuExit::IoOut(port, _)) => Some(format!("OUT {port:X}h")),
            Ok(other) => Some(format!("{other:?}")),
            Err(e) => Some(e.to_string()),
        };
        let r = self.m.vcpu.get_regs().map_err(|e| e.to_string())?;
        let s = self.m.vcpu.get_sregs().map_err(|e| e.to_string())?;
        if let Some(e) = exit {
            let v = if s.cs.selector == ROM_SEG { format!(" (exception {:02X}h)", (r.rip & 0xFFF) >> 4) } else { String::new() };
            return Err(format!("sortie native : {e}{v}"));
        }
        Ok(Cpu::from_kvm(&r, &s))
    }

    fn restore(&mut self) {
        self.m.mem.write(0, &self.image);
        self.shadow.write(0, &self.image);
    }
}

fn random_cpu(rng: &mut Rng) -> Cpu {
    let mut seg = [Seg { selector: 0, base: 0, limit: 0xFFFF }; 6];
    for (i, s) in seg.iter_mut().enumerate() {
        // CS : code entre 01000h et EFFFFh, hors IVT et stubs.
        s.selector = if i == 1 { 0x0100 + rng.below(0xD000) as u16 } else { rng.next() as u16 };
        s.base = u32::from(s.selector) << 4;
    }
    let mut gpr = [0u32; 8];
    for g in &mut gpr {
        *g = interesting(rng);
    }
    Cpu {
        gpr,
        ip: rng.below(0xFFF0) as u32,
        flags: (rng.next() as u32 & (ARITH | DF | IF)) | 2,
        seg,
        stack32: false,
        idt_base: 0,
        cr0: 0,
        seg_dirty: false,
    }
}

fn describe(bytes: &[u8], ins: &Instruction, before: &Cpu) -> String {
    let hex: Vec<String> = bytes[..ins.len()].iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "{} {:?} | AX={:08X} CX={:08X} DX={:08X} BX={:08X} SP={:08X} BP={:08X} SI={:08X} DI={:08X} FL={:04X}",
        hex.join(" "),
        ins.code(),
        before.gpr[0],
        before.gpr[1],
        before.gpr[2],
        before.gpr[3],
        before.gpr[4],
        before.gpr[5],
        before.gpr[6],
        before.gpr[7],
        before.flags
    )
}

fn compare(h: &Harness, ins: &Instruction, before: &Cpu, ours: &Cpu, cpu: &Cpu, flags_push: Option<u32>) -> Vec<String> {
    let mut diffs = Vec::new();
    const NAMES: [&str; 8] = ["EAX", "ECX", "EDX", "EBX", "ESP", "EBP", "ESI", "EDI"];
    for ((name, a), b) in NAMES.iter().zip(ours.gpr).zip(cpu.gpr) {
        if a != b {
            diffs.push(format!("{name} : interp {a:08X}, CPU {b:08X}"));
        }
    }
    if ours.ip != cpu.ip {
        diffs.push(format!("IP : interp {:04X}, CPU {:04X}", ours.ip, cpu.ip));
    }
    let m = !(undefined_flags(ins, before) | TF | RF);
    if ours.flags & m != cpu.flags & m {
        diffs.push(format!("FLAGS : interp {:05X}, CPU {:05X} (masque {:05X})", ours.flags & m, cpu.flags & m, m & 0xFFFFF));
    }
    const SEGS: [&str; 6] = ["ES", "CS", "SS", "DS", "FS", "GS"];
    for ((name, a), b) in SEGS.iter().zip(ours.seg).zip(cpu.seg) {
        if a.selector != b.selector || a.base != b.base {
            diffs.push(format!("{name} : interp {:04X}/{:X}, CPU {:04X}/{:X}", a.selector, a.base, b.selector, b.base));
        }
    }
    let mut a = vec![0u8; MEM_SIZE];
    let mut b = vec![0u8; MEM_SIZE];
    h.shadow.read(0, &mut a);
    h.m.mem.read(0, &mut b);
    if a != b {
        // Le pas à pas natif laisse TF dans les drapeaux empilés.
        let tf_byte = flags_push.map(|p| p as usize + 1);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if x != y && !(Some(i) == tf_byte && *y == *x | 1) {
                diffs.push(format!("mémoire {i:05X} : interp {x:02X}, CPU {y:02X}"));
                if diffs.len() > 8 {
                    break;
                }
            }
        }
    }
    diffs
}

#[derive(Default)]
struct Count {
    compared: u64,
    bailed: u64,
}

#[test]
fn interpreter_matches_cpu() {
    let cases: u64 = std::env::var("DIFFTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let seed: u64 = std::env::var("DIFFTEST_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x5EED_D05B_0C5E_ED01);
    let mut rng = Rng(seed);
    let mut h = Harness::new(&mut rng);
    let mut stats: BTreeMap<String, Count> = BTreeMap::new();
    let mut failures: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut done = 0;
    while done < cases {
        let bytes = random_instruction(&mut rng);
        let ins = Decoder::with_ip(16, &bytes, 0, DecoderOptions::NONE).decode();
        if ins.is_invalid() || excluded(&ins) {
            continue;
        }
        done += 1;
        let before = random_cpu(&mut rng);
        let at = (before.seg[1].base + before.ip) as usize;
        h.m.mem.write(at, &bytes);
        h.shadow.write(at, &bytes);
        let key = format!("{:?}", ins.mnemonic());
        let mut ours = before.clone();
        let Some(stepped) = step_once(&mut ours, &h.shadow, &mut NoMmio) else {
            stats.entry(key).or_default().bailed += 1;
            h.m.mem.write(at, &h.image[at..at + bytes.len()]);
            h.shadow.write(at, &h.image[at..at + bytes.len()]);
            continue;
        };
        stats.entry(key.clone()).or_default().compared += 1;
        if std::env::var_os("DIFFTEST_TRACE").is_some() {
            eprintln!("{done} {}", describe(&bytes, &stepped.ins, &before));
        }
        let diffs = match h.native(&before) {
            Ok(cpu) => compare(&h, &stepped.ins, &before, &ours, &cpu, stepped.flags_push),
            Err(e) => vec![e],
        };
        if !diffs.is_empty() {
            let f = failures.entry(key).or_insert_with(|| (0, String::new()));
            if f.0 == 0 {
                f.1 = format!("{}\n    {}", describe(&bytes, &stepped.ins, &before), diffs.join("\n    "));
            }
            f.0 += 1;
        }
        h.restore();
    }

    println!("\n{cases} cas, graine {seed:#X}");
    let (mut cmp, mut bail) = (0, 0);
    let mut line = String::new();
    for (k, c) in &stats {
        cmp += c.compared;
        bail += c.bailed;
        if c.compared > 0 {
            line.push_str(&format!("{k}:{} ", c.compared));
        }
    }
    println!("comparés {cmp}, non gérés {bail}\ncomparés par mnémonique : {line}");
    let mut unsupported: Vec<_> = stats.iter().filter(|(_, c)| c.compared == 0).map(|(k, c)| (c.bailed, k)).collect();
    unsupported.sort_by(|a, b| b.cmp(a));
    let top: Vec<String> = unsupported.iter().take(40).map(|(n, k)| format!("{k}:{n}")).collect();
    println!("jamais exécutés (premiers) : {}", top.join(" "));
    for (k, (n, first)) in &failures {
        println!("\nÉCHEC {k} ({n} cas) :\n  {first}");
    }
    assert!(failures.is_empty(), "{} mnémoniques divergent", failures.len());
}

#[test]
fn int_pushes_and_vectors() {
    let mem = GuestMem::new(MEM_SIZE).unwrap();
    mem.write(0x21 * 4, &[0x34, 0x12, 0x00, 0xF0]); // INT 21h → F000:1234
    let mut cpu = Cpu {
        gpr: [0, 0, 0, 0, 0x100, 0, 0, 0],
        ip: 0x10,
        flags: 0x0202 | CF,
        seg: [Seg { selector: 0x1000, base: 0x10000, limit: 0xFFFF }; 6],
        stack32: false,
        idt_base: 0,
        cr0: 0,
        seg_dirty: false,
    };
    mem.write(0x10010, &[0xCD, 0x21]);
    step_once(&mut cpu, &mem, &mut NoMmio).expect("INT exécuté");
    assert_eq!((cpu.seg[1].selector, cpu.ip), (0xF000, 0x1234));
    assert_eq!(cpu.flags & IF, 0);
    assert_eq!(cpu.gpr[4], 0xFA);
    let mut st = [0u8; 6];
    mem.read(0x100FA, &mut st);
    assert_eq!(st, [0x12, 0x00, 0x00, 0x10, 0x03, 0x02]); // IP, CS, FLAGS
}
