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
  d <adr> [n]    memory (adr: seg:off or linear below 1 MB, in hex)
  u [adr] [n]    disassemble 16-bit code (default CS:IP)
  dl <lin> [n]   memory at a linear address (through the page tables if CR0.PG)
  dp <phys> [n]  physical memory (any address)
  ul [lin] [n]   disassemble at a linear address, 16 or 32 bits as CS (default CS:EIP);
                 ul32 / ul16 force the code size (after the program ended, CS is real mode)
  dv <lin> [n]   like dl with the last CR3 even if paging is off (post-mortem);
                 uv32 / uv16: like ul32 / ul16 through that CR3
  segs           segment registers: selector, base, limit, size
  g              run to the next hardware breakpoint (BARE86_HWBP=lin[,lin…] in the
                 environment, at most four, execution breakpoints), then stay: the
                 state is that of the faulting instruction (before any #PF is taken)
  x              run to the end of the program, then stay (post-mortem: memory
                 and registers as left by the program's last instruction)
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

/// Adresse physique d'une adresse linéaire : parcours des tables de pages (32 bits,
/// pages de 4 Ko ou de 4 Mo avec PSE) sans les contrôles de droits, comme le
/// ferait un débogueur ; `None` si la page est absente. Sans pagination : identité.
pub fn phys(cpu: &Cpu, mem: &GuestMem, lin: u32) -> Option<u32> {
    if cpu.cr0 & (1 << 31) == 0 {
        return Some(lin);
    }
    let entry = |a: u32| ((a as usize) + 4 <= mem.size()).then(|| mem.read_u32(a as usize));
    let pde = entry((cpu.cr3 & !0xFFF).wrapping_add((lin >> 22) << 2))?;
    if pde & 1 == 0 {
        return None;
    }
    if pde & 0x80 != 0 && cpu.cr4 & (1 << 4) != 0 {
        return Some((pde & 0xFFC0_0000) | (lin & 0x003F_FFFF));
    }
    let pte = entry((pde & !0xFFF).wrapping_add(((lin >> 12) & 0x3FF) << 2))?;
    (pte & 1 != 0).then(|| (pte & !0xFFF) | (lin & 0xFFF))
}

/// Lit `n` octets à une adresse linéaire ; `None` pour un octet dont la page est absente.
pub fn read_linear(cpu: &Cpu, mem: &GuestMem, lin: u32, n: usize) -> Vec<Option<u8>> {
    (0..n)
        .map(|i| {
            let p = phys(cpu, mem, lin.wrapping_add(i as u32))? as usize;
            (p < mem.size()).then(|| mem.read_u8(p))
        })
        .collect()
}

/// Désassemble `n` instructions à une adresse linéaire, en code de `bitness` bits.
pub fn disassemble_linear(cpu: &Cpu, mem: &GuestMem, lin: u32, bitness: u32, n: usize) -> Vec<String> {
    let raw = read_linear(cpu, mem, lin, 16 * n);
    let bytes: Vec<u8> = raw.iter().map_while(|b| *b).collect();
    if bytes.is_empty() {
        return vec![format!("{lin:08X}  (page not present)")];
    }
    let mut dec = Decoder::with_ip(bitness, &bytes, u64::from(lin), DecoderOptions::NONE);
    let mut fmt = IntelFormatter::new();
    let mut out = Vec::new();
    for _ in 0..n {
        if !dec.can_decode() {
            break;
        }
        let ins = dec.decode();
        let mut text = String::new();
        fmt.format(&ins, &mut text);
        let at = (ins.ip() - u64::from(lin)) as usize;
        let hex: String = bytes[at..at + ins.len()].iter().map(|b| format!("{b:02X}")).collect();
        out.push(format!("{:08X}  {hex:<20} {text}", ins.ip()));
    }
    out
}

/// Ligne par registre de segment : sélecteur, base, limite, taille par défaut.
pub fn format_segs(c: &Cpu) -> String {
    ["ES", "CS", "SS", "DS", "FS", "GS"]
        .iter()
        .zip(c.seg)
        .map(|(n, s)| format!("{n}={:04X} base={:08X} limit={:08X} {}", s.selector, s.base, s.limit, if s.db { "32-bit" } else { "16-bit" }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Vidage hexadécimal de `bytes` (`None` : page absente) à partir de `at`.
fn hexdump(at: u32, bytes: &[Option<u8>]) {
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| b.map_or("??".to_string(), |x| format!("{x:02X}"))).collect();
        let asc: String = chunk.iter().map(|b| b.filter(|x| (0x20..0x7F).contains(x)).map_or('.', |x| x as char)).collect();
        println!("{:08X}  {:<48} {asc}", at.wrapping_add(row as u32 * 16), hex.join(" "));
    }
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
        if cpu.cr0 & 1 != 0 && cpu.flags & (1 << 17) == 0 {
            // Mode protégé : désassemblage à l'adresse linéaire de CS:EIP.
            println!("CR0={:08X} CR3={:08X} CR4={:08X}", cpu.cr0, cpu.cr3, cpu.cr4);
            let bitness = if cpu.seg[1].db { 32 } else { 16 };
            for l in disassemble_linear(&cpu, pc.vm.mem(), cpu.linear(1, cpu.ip), bitness, 1) {
                println!("{l}");
            }
            return Ok(());
        }
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
            Some("dl") => {
                let Some(lin) = words.get(1).and_then(|w| u32::from_str_radix(w.trim_start_matches("0x"), 16).ok()) else {
                    println!("address?");
                    continue;
                };
                let cpu = pc.vm.state()?;
                hexdump(lin, &read_linear(&cpu, pc.vm.mem(), lin, num(2, 128)));
            }
            Some("dp") => {
                let Some(at) = words.get(1).and_then(|w| u32::from_str_radix(w.trim_start_matches("0x"), 16).ok()) else {
                    println!("address?");
                    continue;
                };
                let mem = pc.vm.mem();
                let bytes: Vec<Option<u8>> = (0..num(2, 128)).map(|i| (at as usize + i < mem.size()).then(|| mem.read_u8(at as usize + i))).collect();
                hexdump(at, &bytes);
            }
            Some(c @ ("ul" | "ul32" | "ul16")) => {
                let cpu = pc.vm.state()?;
                let lin = match words.get(1) {
                    Some(w) => match u32::from_str_radix(w.trim_start_matches("0x"), 16) {
                        Ok(v) => v,
                        Err(_) => {
                            println!("address?");
                            continue;
                        }
                    },
                    None => cpu.linear(1, cpu.ip),
                };
                let bitness = match c {
                    "ul32" => 32,
                    "ul16" => 16,
                    _ if cpu.cr0 & 1 != 0 && cpu.seg[1].db => 32,
                    _ => 16,
                };
                for l in disassemble_linear(&cpu, pc.vm.mem(), lin, bitness, num(2, 10)) {
                    println!("{l}");
                }
            }
            // `dv`, `uv32` : comme `dl`, `ul32`, mais avec le CR3 conservé même si la
            // pagination est coupée (après la fin du programme, le CPU est en mode réel).
            Some(c @ ("dv" | "uv32" | "uv16")) => {
                let Some(lin) = words.get(1).and_then(|w| u32::from_str_radix(w.trim_start_matches("0x"), 16).ok()) else {
                    println!("address?");
                    continue;
                };
                let mut cpu = pc.vm.state()?;
                cpu.cr0 |= 1 << 31;
                println!("CR3={:08X} CR4={:08X}", cpu.cr3, cpu.cr4);
                match c {
                    "dv" => hexdump(lin, &read_linear(&cpu, pc.vm.mem(), lin, num(2, 128))),
                    _ => {
                        for l in disassemble_linear(&cpu, pc.vm.mem(), lin, if c == "uv32" { 32 } else { 16 }, num(2, 10)) {
                            println!("{l}");
                        }
                    }
                }
            }
            Some("segs") => println!("{}", format_segs(&pc.vm.state()?)),
            Some("g") => {
                pc.stop_at_breakpoint = true;
                let event = pc.resume();
                pc.stop_at_breakpoint = false;
                match event? {
                    Event::Breakpoint => show(pc)?,
                    Event::Exit(code) => println!("Program finished, code {code} (breakpoint not reached)."),
                    Event::Step => show(pc)?,
                }
            }
            Some("x") => {
                let code = pc.run()?;
                println!("Program finished, code {code}. Post-mortem: d, dl, dp, u, ul, r, segs still work.");
            }
            Some("q") => return Ok(None),
            Some(_) | None => println!("{HELP}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::Seg;

    fn cpu(cr0: u32, cr3: u32, cr4: u32) -> Cpu {
        Cpu {
            gpr: [0; 8],
            ip: 0,
            flags: 0x202,
            seg: [Seg::real(0); 6],
            stack32: false,
            idt_base: 0,
            cr0,
            cr3,
            cr4,
            tr_base: 0,
            tr_limit: 0,
            seg_dirty: false,
        }
    }

    #[test]
    fn page_walk_for_the_debugger() {
        let mem = GuestMem::new(0x20_0000).unwrap();
        // Répertoire en 1000h, table en 2000h : page 0 absente, page 1 → 5000h,
        // et l'entrée 1 du répertoire en pages de 4 Mo (PSE) → 40_0000h (hors mémoire).
        mem.write_u32(0x1000, 0x2003);
        mem.write_u32(0x2000 + 4, 0x5003);
        mem.write_u32(0x5000 + 0x10, 0xDEAD_BEEF);
        let c = cpu(1 << 31 | 1, 0x1000, 0);
        assert_eq!(phys(&c, &mem, 0x0000_0FFF), None, "page absente");
        assert_eq!(phys(&c, &mem, 0x1010), Some(0x5010));
        assert_eq!(read_linear(&c, &mem, 0x100E, 4), vec![Some(0), Some(0), Some(0xEF), Some(0xBE)]);
        // Sans pagination : identité.
        assert_eq!(phys(&cpu(1, 0x1000, 0), &mem, 0x1234), Some(0x1234));
        // PSE : 4 Mo, base en 0080_0000h (bits 31:22).
        mem.write_u32(0x1004, 0x0080_0083);
        let c = cpu(1 << 31 | 1, 0x1000, 1 << 4);
        assert_eq!(phys(&c, &mem, 0x0040_1234), Some(0x0080_1234));
        // Désassemblage 32 bits d'un octet lu par la pagination : `mov eax,1`.
        mem.write(0x5000 + 0x20, &[0xB8, 1, 0, 0, 0]);
        let l = disassemble_linear(&cpu(1 << 31 | 1, 0x1000, 0), &mem, 0x1020, 32, 1);
        assert!(l[0].contains("mov eax,1"), "{l:?}");
    }
}
