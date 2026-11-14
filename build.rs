// SPDX-License-Identifier: GPL-3.0-or-later
//! Assemble le code invité (ROM BIOS, programmes de test) avec GNU as.
//! Chaque `asm/**/*.S` donne `$OUT_DIR/<nom>.bin` (binaire plat, section .text).
//! Compile aussi Nuked-OPL3 (third_party/nuked-opl3).

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap_or_else(|e| panic!("{cmd:?} : {e}"));
    assert!(status.success(), "{cmd:?} a échoué");
}

/// Adresse de chargement : 0 pour la ROM, 100h pour les .COM.
fn assemble(src: &Path, out: &Path, origin: u32) {
    let stem = src.file_stem().unwrap().to_str().unwrap();
    let obj = out.join(format!("{stem}.o"));
    let elf = out.join(format!("{stem}.elf"));
    run(Command::new("as").args(["--32", "-o"]).arg(&obj).arg(src));
    run(Command::new("ld")
        .args(["-m", "elf_i386", "-e", "0", &format!("-Ttext={origin:#x}"), "-o"])
        .arg(&elf)
        .arg(&obj));
    run(Command::new("objcopy").args(["-O", "binary", "-j", ".text"]).arg(&elf).arg(out.join(format!("{stem}.bin"))));
}

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=asm");
    assemble(Path::new("asm/bios.S"), &out, 0);
    println!("cargo:rerun-if-changed=third_party/nuked-opl3");
    cc::Build::new()
        .file("third_party/nuked-opl3/opl3.c")
        .file("third_party/nuked-opl3/shim.c")
        .opt_level(2)
        .warnings(false)
        .compile("nuked_opl3");
    // Musique MIDI jouée sur OPL3 : libADLMIDI (sous-ensemble vendu,
    // third_party/libadlmidi/README.bare86), émulateur Nuked seul, sans
    // séquenceur de fichiers. C++ puis C : ordre de l'édition de liens.
    println!("cargo:rerun-if-changed=third_party/libadlmidi");
    let adl = Path::new("third_party/libadlmidi/src");
    let defines = [
        "ADLMIDI_DISABLE_MIDI_SEQUENCER",
        "ADLMIDI_DISABLE_DOSBOX_EMULATOR",
        "ADLMIDI_DISABLE_OPAL_EMULATOR",
        "ADLMIDI_DISABLE_JAVA_EMULATOR",
        "ADLMIDI_DISABLE_ESFMU_EMULATOR",
        "ADLMIDI_DISABLE_MAME_OPL2_EMULATOR",
        "ADLMIDI_DISABLE_YMFM_EMULATOR",
    ];
    let mut cpp = cc::Build::new();
    cpp.cpp(true).std("c++11").opt_level(2).warnings(false).include("third_party/libadlmidi/include").include(adl);
    for f in ["adlmidi.cpp", "adlmidi_midiplay.cpp", "adlmidi_opl3.cpp", "adlmidi_private.cpp", "adlmidi_load.cpp", "inst_db.cpp", "chips/nuked_opl3.cpp", "chips/nuked_opl3_fast.cpp", "chips/nuked_opl2.cpp", "chips/nuked_cqm.cpp"] {
        cpp.file(adl.join(f));
    }
    let mut c = cc::Build::new();
    c.opt_level(2).warnings(false).include("third_party/libadlmidi/include").include(adl);
    // Puce OPL2 de la carte son (--opl opl2|dualopl2) : notre shim d'allocation.
    c.file("src/opl2_shim.c");
    println!("cargo:rerun-if-changed=src/opl2_shim.c");
    for f in ["chips/nuked/nukedopl3.c", "chips/nuked/nukedopl2.c", "chips/nuked_fast/nukedopl3_fast.c", "chips/nuked_cqm/cqm.c", "wopl/wopl_file.c"] {
        c.file(adl.join(f));
    }
    for entry in std::fs::read_dir(adl.join("models")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "c") {
            c.file(path);
        }
    }
    for d in defines {
        cpp.define(d, None);
        c.define(d, None);
    }
    for b in [&mut cpp, &mut c] {
        b.flag("-include").flag("third_party/libadlmidi/bare86_rename.h");
    }
    cpp.compile("adlmidi_cpp");
    c.compile("adlmidi_c");
    // Réseau : libslirp (NAT en espace utilisateur), par un petit shim C.
    println!("cargo:rerun-if-changed=src/net/slirp_shim.c");
    let flags = Command::new("pkg-config").args(["--cflags", "slirp"]).output().expect("pkg-config");
    assert!(flags.status.success(), "libslirp introuvable (pkg-config slirp)");
    let mut shim = cc::Build::new();
    shim.file("src/net/slirp_shim.c").opt_level(2);
    for f in String::from_utf8_lossy(&flags.stdout).split_whitespace() {
        shim.flag(f);
    }
    shim.compile("bare86_slirp");
    println!("cargo:rustc-link-lib=slirp");
    for dir in ["asm/tests", "asm/bench"] {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "S") {
                println!("cargo:rerun-if-changed={}", path.display());
                assemble(&path, &out, 0x100);
            }
        }
    }
}
