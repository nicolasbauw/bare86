// SPDX-License-Identifier: GPL-3.0-or-later
//! Débogueur minimal en ligne de commande : registres, mémoire,
//! désassemblage, pas à pas.

use std::io::{BufRead, Write};

use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};

use crate::backend::{Backend, Result};
use crate::interp::Cpu;
use crate::machine::{Event, Pc};
use crate::mem::GuestMem;

const HELP: &str = "\
  r              registers
  s [n]          n steps (default 1)
  c              continue to the end
  d <adr> [n]    memory (adr: seg:off or linear, in hex)
  u [adr] [n]    disassemble (default CS:IP)
  q              quit";

pub fn format_regs(c: &Cpu) -> String {
    let g = &c.gpr;
    let s: Vec<String> = ["ES", "CS", "SS", "DS", "FS", "GS"]
        .iter()
        .zip(c.seg)
        .map(|(n, s)| format!("{n}={:04X}", s.selector))
        .collect();
    let f = c.flags;
    let flag = |bit: u32, ch: char| if f & (1 << bit) != 0 { ch } else { '-' };
    format!(
        "EAX={:08X} EBX={:08X} ECX={:08X} EDX={:08X} ESI={:08X} EDI={:08X} EBP={:08X} ESP={:08X}\n{} IP={:04X} [{}{}{}{}{}{}{}{}{}]",
        g[0],
        g[3],
        g[1],
        g[2],
        g[6],
        g[7],
        g[5],
        g[4],
        s.join(" "),
        c.ip,
        flag(11, 'O'),
        flag(10, 'D'),
        flag(9, 'I'),
        flag(8, 'T'),
        flag(7, 'S'),
        flag(6, 'Z'),
        flag(4, 'A'),
        flag(2, 'P'),
        flag(0, 'C'),
    )
}

/// Désassemble `n` instructions à seg:off (code 16 bits).
pub fn disassemble(mem: &GuestMem, seg: u16, off: u16, n: usize) -> Vec<String> {
    let base = usize::from(seg) << 4;
    let mut bytes = vec![0u8; 15 * n];
    let start = base + usize::from(off);
    let len = bytes.len().min(mem.size().saturating_sub(start));
    mem.read(start, &mut bytes[..len]);
    let mut dec = Decoder::with_ip(16, &bytes[..len], u64::from(off), DecoderOptions::NONE);
    let mut fmt = IntelFormatter::new();
    let mut out = Vec::new();
    for _ in 0..n {
        if !dec.can_decode() {
            break;
        }
        let ins = dec.decode();
        let mut text = String::new();
        fmt.format(&ins, &mut text);
        let at = ins.ip() as usize - usize::from(off);
        let hex: String = bytes[at..at + ins.len()].iter().map(|b| format!("{b:02X}")).collect();
        out.push(format!("{seg:04X}:{:04X}  {hex:<16} {text}", ins.ip()));
    }
    out
}

/// `seg:off` ou adresse linéaire, en hexadécimal.
fn parse_addr(s: &str) -> Option<(u16, u16)> {
    if let Some((seg, off)) = s.split_once(':') {
        Some((u16::from_str_radix(seg, 16).ok()?, u16::from_str_radix(off, 16).ok()?))
    } else {
        let lin = u32::from_str_radix(s, 16).ok()?;
        Some(((lin >> 4) as u16, (lin & 0xF) as u16))
    }
}

/// Avance de `n` instructions ; renvoie le code de sortie si le programme se termine.
pub fn step<B: Backend>(pc: &mut Pc<B>, n: usize) -> Result<Option<u8>> {
    pc.set_single_step(true)?;
    let mut end = None;
    for _ in 0..n {
        if let Event::Exit(code) = pc.resume()? {
            end = Some(code);
            break;
        }
    }
    pc.set_single_step(false)?;
    Ok(end)
}

/// Boucle interactive ; renvoie le code de sortie du programme, s'il a fini.
pub fn repl<B: Backend>(pc: &mut Pc<B>) -> Result<Option<u8>> {
    println!("Debugger; \"h\" for help.");
    let stdin = std::io::stdin();
    let show = |pc: &Pc<B>| -> Result<()> {
        let cpu = pc.vm.state()?;
        println!("{}", format_regs(&cpu));
        for l in disassemble(pc.vm.mem(), cpu.seg[1].selector, cpu.ip as u16, 1) {
            println!("{l}");
        }
        Ok(())
    };
    show(pc)?;
    loop {
        print!("dbg> ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize, def: usize| words.get(i).and_then(|w| w.parse().ok()).unwrap_or(def);
        match words.first().copied() {
            Some("r") => show(pc)?,
            Some("s") => {
                if let Some(code) = step(pc, num(1, 1))? {
                    println!("Program finished, code {code}.");
                    return Ok(Some(code));
                }
                show(pc)?;
            }
            Some("c") => return pc.run().map(Some),
            Some("d") => {
                let Some((seg, off)) = words.get(1).and_then(|w| parse_addr(w)) else {
                    println!("address?");
                    continue;
                };
                let n = num(2, 128);
                let base = (usize::from(seg) << 4) + usize::from(off);
                for row in (0..n).step_by(16) {
                    let mut b = [0u8; 16];
                    let len = 16.min(n - row).min(pc.vm.mem().size().saturating_sub(base + row));
                    pc.vm.mem().read(base + row, &mut b[..len]);
                    let hex: Vec<String> = b[..len].iter().map(|x| format!("{x:02X}")).collect();
                    let asc: String = b[..len].iter().map(|&x| if (0x20..0x7F).contains(&x) { x as char } else { '.' }).collect();
                    println!("{:05X}  {:<48} {asc}", base + row, hex.join(" "));
                }
            }
            Some("u") => {
                let cpu = pc.vm.state()?;
                let (seg, off) = words.get(1).and_then(|w| parse_addr(w)).unwrap_or((cpu.seg[1].selector, cpu.ip as u16));
                for l in disassemble(pc.vm.mem(), seg, off, num(2, 10)) {
                    println!("{l}");
                }
            }
            Some("q") => return Ok(None),
            Some(_) | None => println!("{HELP}"),
        }
    }
}
