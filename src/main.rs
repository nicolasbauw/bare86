// SPDX-License-Identifier: GPL-3.0-or-later
//! Étape 0 : exécuter un .COM nativement sous KVM, INT 21h minimal côté hôte.

mod bench;
mod interp;
mod throttle;
mod vm;

use std::io::Write;
use std::process::ExitCode;

use kvm_ioctls::VcpuExit;
use vm::Machine;

/// 1 Mo + HMA.
pub(crate) const MEM_SIZE: usize = 0x11_0000;
/// Segment de la ROM des stubs d'interruption.
pub(crate) const ROM_SEG: u16 = 0xF000;
/// Port d'appel hôte utilisé par les stubs.
pub(crate) const HOST_CALL_PORT: u16 = 0xE0;
/// Segment du PSP du programme chargé.
const PSP_SEG: u16 = 0x1000;

/// Programme de test intégré :
/// ```text
/// mov dx, msg ; mov ah, 9 ; int 21h ; mov ax, 4C00h ; int 21h
/// msg db "Hello from KVM!", 13, 10, "$"
/// ```
const HELLO_COM: &[u8] = b"\xBA\x0C\x01\xB4\x09\xCD\x21\xB8\x00\x4C\xCD\x21Hello from KVM!\r\n$";

fn lin(seg: u64, off: u64) -> usize {
    ((seg << 4) + (off & 0xFFFF)) as usize
}

/// IVT : le vecteur n pointe sur F000:n*16, où se trouve `out 0xE0, al ; iret`.
/// L'hôte retrouve le vecteur à partir de l'IP du stub.
pub(crate) fn install_ivt(m: &Machine) {
    for n in 0..256u16 {
        m.mem.write_u16(n as usize * 4, n * 16);
        m.mem.write_u16(n as usize * 4 + 2, ROM_SEG);
        m.mem.write(lin(ROM_SEG.into(), u64::from(n) * 16), &[0xE6, HOST_CALL_PORT as u8, 0xCF]);
    }
}

/// PSP minimal : INT 20h en PSP:0, ligne de commande vide.
pub(crate) fn load_com(m: &Machine, image: &[u8]) -> Result<(), String> {
    if image.len() > 0xFF00 - 2 {
        return Err("fichier .COM trop gros".into());
    }
    let psp = lin(PSP_SEG.into(), 0);
    m.mem.write(psp, &[0xCD, 0x20]);
    m.mem.write(psp + 0x80, &[0, 0x0D]);
    m.mem.write(psp + 0x100, image);
    // Mot nul en sommet de pile : un RET final saute en PSP:0 (INT 20h).
    m.mem.write_u16(psp + 0xFFFE, 0);
    m.set_real_mode(PSP_SEG, 0x100, 0xFFFE).map_err(|e| e.to_string())
}

enum Flow {
    Continue,
    Exit(u8),
}

fn int21(m: &Machine, out: &mut impl Write) -> Result<Flow, Box<dyn std::error::Error>> {
    let regs = m.vcpu.get_regs()?;
    let sregs = m.vcpu.get_sregs()?;
    let ah = (regs.rax >> 8) as u8;
    match ah {
        0x09 => {
            let mut addr = lin(sregs.ds.selector.into(), regs.rdx);
            let mut s = Vec::new();
            loop {
                let c = m.mem.read_u8(addr);
                if c == b'$' {
                    break;
                }
                s.push(c);
                addr += 1;
            }
            out.write_all(&s)?;
            out.flush()?;
            Ok(Flow::Continue)
        }
        0x4C => Ok(Flow::Exit(regs.rax as u8)),
        _ => {
            eprintln!("INT 21h AH={ah:02X}h non géré");
            Ok(Flow::Continue)
        }
    }
}

fn run(m: &mut Machine) -> Result<u8, Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout();
    loop {
        let port = match m.vcpu.run()? {
            VcpuExit::IoOut(port, _) => port,
            VcpuExit::Hlt => return Err("HLT".into()),
            other => return Err(format!("sortie de VM inattendue : {other:?}").into()),
        };
        let regs = m.vcpu.get_regs()?;
        let cs = m.vcpu.get_sregs()?.cs.selector;
        if port != HOST_CALL_PORT || cs != ROM_SEG {
            eprintln!("OUT port {port:04X}h ignoré ({cs:04X}:{:04X})", regs.rip);
            continue;
        }
        let vector = (regs.rip & 0xFFF) >> 4;
        let flow = match vector {
            0x20 => Flow::Exit(0),
            0x21 => int21(m, &mut stdout)?,
            v => {
                eprintln!("INT {v:02X}h non géré");
                Flow::Continue
            }
        };
        if let Flow::Exit(code) = flow {
            return Ok(code);
        }
    }
}

fn main() -> ExitCode {
    let bench = match std::env::args().nth(1).as_deref() {
        Some("--bench") => Some(bench::run as fn() -> _),
        Some("--throttle") => Some(bench::run_throttle as fn() -> _),
        Some("--remap") => Some(bench::run_remap as fn() -> _),
        _ => None,
    };
    if let Some(bench) = bench {
        return match bench() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("erreur : {e}");
                ExitCode::from(1)
            }
        };
    }
    let image = match std::env::args_os().nth(1) {
        Some(path) => match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {e}", path.to_string_lossy());
                return ExitCode::from(1);
            }
        },
        None => HELLO_COM.to_vec(),
    };
    let result = Machine::new(MEM_SIZE).and_then(|mut m| {
        install_ivt(&m);
        load_com(&m, &image)?;
        run(&mut m)
    });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(1)
        }
    }
}
