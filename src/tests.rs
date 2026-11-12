// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests de la machine sur de petits programmes assemblés par build.rs
//! (asm/tests/*.S), exécutés sans fenêtre.

use crate::debug;
use crate::host::{Capture, Host};
use crate::machine::{Config, Pc};

macro_rules! program {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".bin"))
    };
}

fn machine(cfg: &Config, image: &[u8]) -> (Pc, Capture) {
    let out = Capture::default();
    let mut pc = Pc::new(cfg, Host::headless(Box::new(out.clone()))).expect("machine");
    pc.load_com(image).expect("chargement");
    (pc, out)
}

fn run(image: &[u8]) -> (u8, String, Pc) {
    let (mut pc, out) = machine(&Config::default(), image);
    let code = pc.run().expect("exécution");
    let text = String::from_utf8_lossy(&out.0.borrow()).into_owned();
    (code, text, pc)
}

#[test]
fn hello_prints_and_exits() {
    let (code, text, _) = run(program!("hello"));
    assert_eq!((code, text.as_str()), (0, "Hello from KVM!\r\n"));
}

#[test]
fn exit_code() {
    assert_eq!(run(program!("exit42")).0, 42);
}

#[test]
fn ret_reaches_int20() {
    let (code, text, _) = run(program!("ret"));
    assert_eq!((code, text.as_str()), (0, "R"));
}

#[test]
fn memory_map() {
    assert_eq!(run(program!("memmap")).0, 0, "n° du contrôle en échec (voir asm/tests/memmap.S)");
}

#[test]
fn vga_window_goes_through_interpreter() {
    let (code, _, pc) = run(program!("vga"));
    assert_eq!(code, 0xE7);
    assert_eq!(pc.vga_byte(0), 0xE7);
    // REP STOSB de 1000 octets : une seule sortie MMIO, le reste interprété.
    assert!(pc.stats.mmio_exits <= 2, "{:?}", pc.stats);
    assert!(pc.stats.interpreted >= 1000, "{:?}", pc.stats);
}

#[test]
fn protected_mode_burst() {
    let (code, _, pc) = run(program!("pmode"));
    assert_eq!(code, 0x5A);
    assert_eq!(pc.vga_byte(999), 0x5A);
    // Rafale en mode protégé 32 bits : quelques sorties MMIO seulement.
    assert!(pc.stats.mmio_exits <= 2, "{:?}", pc.stats);
    assert!(pc.stats.interpreted >= 1000, "{:?}", pc.stats);
}

#[test]
fn unhandled_interrupt_is_reported() {
    let (code, _, pc) = run(program!("unhandled"));
    assert_eq!(code, 7);
    assert!(pc.unhandled().iter().any(|&(v, _)| v == 0x99));
}

#[test]
fn single_step() {
    let (mut pc, _) = machine(&Config::default(), program!("hello"));
    // mov dx, msg ; mov ah, 9 : deux pas.
    assert_eq!(debug::step(&mut pc, 2).unwrap(), None);
    let cpu = pc.vm.state().unwrap();
    assert_eq!(cpu.ip, 0x105);
    assert_eq!(cpu.gpr[2] & 0xFFFF, 0x10C);
    assert_eq!((cpu.gpr[0] >> 8) & 0xFF, 9);
    // Le reste s'exécute normalement.
    assert_eq!(pc.run().unwrap(), 0);
}

#[test]
fn disassembler() {
    let (pc, _) = machine(&Config::default(), program!("hello"));
    let lines = debug::disassemble(pc.vm.mem(), 0x1000, 0x100, 2);
    assert!(lines[0].ends_with("mov dx,10Ch"), "{lines:?}");
    assert!(lines[1].ends_with("mov ah,9"), "{lines:?}");
}

#[test]
fn throttled_run_completes() {
    let cfg = Config { mips: Some(50.0), ..Config::default() };
    let (mut pc, _) = machine(&cfg, program!("exit42"));
    assert_eq!(pc.run().unwrap(), 42);
}

use crate::backend::Backend;
