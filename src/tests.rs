// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests de la machine sur de petits programmes assemblés par build.rs
//! (asm/tests/*.S), exécutés sans fenêtre.

use crate::debug;
use crate::bios;
use crate::host::{Capture, Host, InputEvent, ScriptedInput};
use crate::machine::{Config, Pc};

macro_rules! program {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".bin"))
    };
}

fn machine(cfg: &Config, image: &[u8]) -> (Pc, Capture) {
    machine_with_input(cfg, image, &[])
}

fn machine_with_input(cfg: &Config, image: &[u8], input: &[InputEvent]) -> (Pc, Capture) {
    let out = Capture::default();
    let mut host = Host::headless(Box::new(out.clone()));
    host.input = Box::new(ScriptedInput(input.iter().copied().collect()));
    let mut pc = Pc::new(cfg, host).expect("machine");
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
    let cs = pc.vm.state().unwrap().seg[1].selector;
    let lines = debug::disassemble(pc.vm.mem(), cs, 0x100, 2);
    assert!(lines[0].ends_with("mov dx,10Ch"), "{lines:?}");
    assert!(lines[1].ends_with("mov ah,9"), "{lines:?}");
}

#[test]
fn throttled_run_completes() {
    let cfg = Config { mips: Some(50.0), ..Config::default() };
    let (mut pc, _) = machine(&cfg, program!("exit42"));
    assert_eq!(pc.run().unwrap(), 42);
}

#[test]
fn hello_reaches_text_screen() {
    let (_, _, pc) = run(program!("hello"));
    assert_eq!(pc.screen_text()[0], "Hello from KVM!");
}

#[test]
fn bios_text_mode() {
    let (code, text, pc) = run(program!("bios_text"));
    assert_eq!(code, 0, "n° du contrôle en échec (voir asm/tests/bios_text.S)");
    assert_eq!(text, "L1\r\nL2Hi");
    let screen = pc.screen_text();
    // Après défilement d'une ligne : L1 disparaît, tout remonte.
    assert_eq!(screen[0], "L2");
    assert_eq!(screen[9], "     XXX");
    assert_eq!(screen[11], "Hi");
    assert_eq!(screen[24], "");
    let mem = pc.vm.mem();
    assert_eq!(bios::screen_attr(mem, 9, 5), 0x1E);
    assert_eq!(bios::screen_attr(mem, 11, 1), 0x70);
    assert_eq!(bios::screen_attr(mem, 24, 0), 0x07);
    // 13h avec AL=1 : curseur après la chaîne (avant défilement).
    assert_eq!(mem.read_u16(0x450), 0x0C02);
}

#[test]
fn bios_info() {
    assert_eq!(run(program!("bios_info")).0, 0, "n° du contrôle en échec (voir asm/tests/bios_info.S)");
}

#[test]
fn bios_keyboard() {
    // Maj + h, puis « i » tapé au terminal.
    let keys = [InputEvent::Key(0x2A), InputEvent::Key(0x23), InputEvent::Key(0xA3), InputEvent::Key(0xAA), InputEvent::Text(b'i')];
    let (mut pc, out) = machine_with_input(&Config::default(), program!("bios_kbd"), &keys);
    assert_eq!(pc.run().unwrap(), 0, "n° du contrôle en échec (voir asm/tests/bios_kbd.S)");
    assert_eq!(&*out.0.borrow(), b"Hi");
}

#[test]
fn bios_keyboard_without_input_fails_cleanly() {
    let (mut pc, _) = machine(&Config::default(), program!("bios_kbd"));
    let err = pc.run().unwrap_err();
    assert!(err.0.contains("INT 16h"), "{err}");
}

#[test]
fn bios_ticks_follow_host_clock() {
    let ticks = run(program!("bios_tick")).0;
    assert!((1..=3).contains(&ticks), "{ticks} tics pour 110 ms");
}

/// Critère de l'étape 2 : programme C Open Watcom (c/tests/textdemo.c,
/// compilé par c/build.sh) ; écran texte par INT 10h, printf, getch.
#[test]
fn watcom_text_program() {
    let image = include_bytes!("../c/tests/textdemo.com");
    let (mut pc, out) = machine_with_input(&Config::default(), image, &[InputEvent::Text(b'k')]);
    assert_eq!(pc.run().unwrap(), 0);
    assert!(pc.unhandled().is_empty(), "{:?}", pc.unhandled());
    assert_eq!(&*out.0.borrow(), b"Touche ? k (107)\r\n");
    let screen = pc.screen_text();
    assert_eq!(screen[4], "         Open Watcom en mode texte");
    assert_eq!(screen[6], "         Touche ? k (107)");
    assert_eq!(bios::screen_attr(pc.vm.mem(), 4, 9), 0x1E);
}

/// Étape 3 : exécutables MZ Open Watcom sur un répertoire hôte monté
/// (fichiers, recherche, répertoires, mémoire, processus enfant).
#[test]
fn watcom_files_and_exec() {
    let dir = std::env::temp_dir().join(format!("dosbox-c3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("files.exe"), include_bytes!("../c/tests/files.exe")).unwrap();
    std::fs::write(dir.join("child.exe"), include_bytes!("../c/tests/child.exe")).unwrap();
    let out = Capture::default();
    let mut pc = Pc::new(&Config::default(), Host::headless(Box::new(out.clone()))).unwrap();
    pc.mount(2, dir.clone());
    pc.load_program(b"C:\\FILES.EXE", b" un deux").unwrap();
    let code = pc.run().unwrap();
    let text = String::from_utf8_lossy(&out.0.borrow()).into_owned();
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("enfant : salut") && text.ends_with("fin\r\n"), "{text}");
    assert!(pc.unhandled().is_empty(), "{:?}", pc.unhandled());
    let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    left.sort();
    assert_eq!(left, ["child.exe", "files.exe"]);
    // Tout est rendu : PSP et environnement du shell racine, puis un bloc libre.
    let chain = crate::dos::mcb_chain(pc.vm.mem());
    assert_eq!(chain.len(), 3, "{chain:x?}");
    assert_eq!(chain[0].0, chain[1].0);
    assert_eq!(chain[2].0, 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Shell interne : commandes internes et lancement d'un programme.
#[test]
fn shell_session() {
    let dir = std::env::temp_dir().join(format!("dosbox-shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("child.exe"), include_bytes!("../c/tests/child.exe")).unwrap();
    std::fs::write(dir.join("Notes de cours.txt"), b"bonjour\r\n\x1Acache").unwrap();
    let typed = b"md jeux\ncd jeux\ncd\ncd..\ntype notesd~1.txt\nchild salut\nfoo\nexit\nver\n";
    let out = Capture::default();
    let mut host = Host::headless(Box::new(out.clone()));
    host.input = Box::new(ScriptedInput(typed.iter().map(|&c| InputEvent::Text(c)).collect()));
    let mut pc = Pc::new(&Config::default(), host).unwrap();
    pc.mount(2, dir.clone());
    assert_eq!(pc.shell().unwrap(), 7);
    let text = String::from_utf8_lossy(&out.0.borrow()).into_owned();
    for want in ["C:\\JEUX>cd\r\nC:\\JEUX\r\n", "bonjour\r\nC:\\>", "enfant : salut", "incorrect"] {
        assert!(text.contains(want), "{want:?} absent de :\n{text}");
    }
    assert!(!text.contains("cache") && !text.contains("MS-DOS version"), "{text}");
    assert!(dir.join("JEUX").is_dir());
    std::fs::remove_dir_all(&dir).unwrap();
}

use crate::backend::Backend;
