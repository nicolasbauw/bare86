// SPDX-License-Identifier: GPL-3.0-or-later
//! Traducteur dynamique x86 32 bits → x86-64 (branche `dyntrans`).
//!
//! Pour le code 32 bits du mode protégé « plat » (segments de base 0 et de
//! limite 4 Go, sans pagination : Doom sous DOS/4GW), les blocs de base de
//! l'invité sont réencodés presque à l'identique pour l'hôte et exécutés
//! côté hôte, sans sortie de VM (docs/JOURNAL.md, point 7) :
//!
//! - registres de l'invité dans ceux de l'hôte : EAX…EDI tels quels, sauf
//!   ESP → R12D ; R15 = base de la mémoire de l'invité, R10 = `State` ;
//! - opérande mémoire : `lea r13d,[…]` (arithmétique 32 bits de l'invité),
//!   contrôles (fin de la mémoire, zone A0000–FFFFF, pages de code
//!   traduites), puis l'instruction d'origine sur `[r15+r13]` ;
//! - drapeaux calculés par le processeur hôte (même instruction), sauvés
//!   autour des contrôles seulement s'ils sont encore lus ensuite ;
//! - le reste (pile, chaînes, E/S, fautes possibles…) est rendu à
//!   l'interpréteur, une instruction à la fois.

pub mod policy;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use iced_x86::code_asm::*;
use iced_x86::{
    BlockEncoderOptions, Code, ConditionCode, Decoder, DecoderOptions, Encoder, FlowControl, Instruction, MemoryOperand, Mnemonic as Mn, OpKind,
    Register,
};

use crate::interp::{self, Cpu, Mmio, StopReason};
use crate::mem::GuestMem;

/// État échangé avec le code traduit (décalages figés dans le code émis).
#[repr(C)]
#[derive(Default)]
pub struct State {
    /// EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI.
    pub gpr: [u32; 8],
    pub flags: u32,
    pub eip: u32,
    /// Base de la mémoire de l'invité côté hôte.
    membase: u64,
    /// Adresse de l'invité de l'accès qui a fait sortir le bloc.
    pub addr: u32,
    /// Motif de sortie du bloc (`EXIT_*`).
    pub exit: u32,
    /// Instructions de l'invité exécutées par le bloc.
    pub count: u32,
    _pad2: u32,
    /// Un bit par octet de l'invité : octet de code traduit (hors immédiats
    /// volatils).
    code_bits: u64,
    /// Plan d'écriture direct de la fenêtre A0000 (0 : chemin lent).
    vram: u64,
    /// RFLAGS de l'invité sauvés avant un contrôle d'accès.
    saved_flags: u64,
    /// Écritures directes dans un plan VGA (activité de la carte).
    vga_writes: u64,
    /// Instructions de l'invité exécutées depuis l'appel (blocs enchaînés
    /// compris).
    total: u64,
    /// Enchaînements de blocs encore permis avant de rendre la main.
    chain: u64,
    /// Case de saut de la sortie prise (0 : sortie sans enchaînement
    /// possible) : Rust y écrit le corps du bloc suivant.
    link: u64,
}

const OFF_FLAGS: i32 = 32;
const OFF_EIP: i32 = 36;
const OFF_MEMBASE: i32 = 40;
const OFF_ADDR: i32 = 48;
const OFF_EXIT: i32 = 52;
const OFF_CODE_BITS: i32 = 64;
const OFF_VRAM: i32 = 72;
const OFF_SAVED: i32 = 80;
const OFF_VGA_WRITES: i32 = 88;
const OFF_TOTAL: i32 = 96;
const OFF_CHAIN: i32 = 104;
const OFF_LINK: i32 = 112;
/// Blocs enchaînés au plus par appel (la boucle Rust contrôle ensuite
/// budget, inactivité et arrêt).
const CHAIN_MAX: u64 = 256;

/// Fin de bloc normale (saut, ou instruction laissée à l'interpréteur).
pub const EXIT_NEXT: u32 = 0;
/// Accès mémoire hors du cas simple (VRAM, ROM, fin de la mémoire) :
/// l'interpréteur exécute l'instruction.
pub const EXIT_SLOW: u32 = 1;
/// Écriture dans une page de code traduit : cache vidé, puis interpréteur.
pub const EXIT_SMC: u32 = 2;

/// Drapeaux arithmétiques et DF : ceux que le code traduit porte dans
/// RFLAGS de l'hôte.
const HOST_FLAGS: u32 = 0x0CD5;
/// Drapeaux arithmétiques seuls (vivacité).
const ARITH: u32 = 0x08D5;

/// Instructions au plus par bloc.
const MAX_BLOCK: usize = 64;
/// Taille du tampon de code exécutable.
const CODE_SIZE: usize = 32 << 20;
/// Accès mémoire au-delà de `mem.size() - MEM_GUARD` : chemin lent.
const MEM_GUARD: u32 = 64;

struct CodeBuf {
    ptr: *mut u8,
    used: usize,
}

impl CodeBuf {
    fn new() -> std::io::Result<Self> {
        // SAFETY: mappage anonyme privé, vérifié.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                CODE_SIZE,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { ptr: ptr.cast(), used: 0 })
    }
}

impl Drop for CodeBuf {
    fn drop(&mut self) {
        // SAFETY: mappage de `new`.
        unsafe { libc::munmap(self.ptr.cast(), CODE_SIZE) };
    }
}

/// Bloc traduit : point d'entrée et octets de l'invité dont il provient.
struct Block {
    entry: *const u8,
    /// Corps du bloc, après le prologue : cible des enchaînements (registres
    /// et drapeaux de l'invité déjà dans ceux de l'hôte).
    body: *const u8,
    bytes: Vec<u8>,
    /// Octets comparés à la vérification (0 : immédiat volatil).
    mask: Vec<u8>,
    /// Époque à laquelle `bytes` a été vérifié contre la mémoire.
    verified: u64,
}

/// Hachage d'une adresse 32 bits (le SipHash par défaut coûte trop cher
/// par bloc).
#[derive(Default, Clone, Copy)]
struct AddrHash(u64);

impl std::hash::Hasher for AddrHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, _: &[u8]) {
        unreachable!()
    }
    fn write_u32(&mut self, v: u32) {
        self.0 = u64::from(v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Map<V> = HashMap<u32, V, std::hash::BuildHasherDefault<AddrHash>>;

#[derive(Default, Debug, Clone)]
pub struct JitStats {
    pub blocks_run: u64,
    pub translated: u64,
    pub jit_instructions: u64,
    pub interp_steps: u64,
    pub slow_exits: u64,
    pub smc_flushes: u64,
    pub stale_flushes: u64,
    /// Instructions laissées à l'interpréteur, par mnémonique.
    pub fallbacks: HashMap<String, u64>,
}

pub struct Jit {
    code: CodeBuf,
    /// Blocs par EIP ; `None` : la première instruction n'est pas traduite.
    blocks: Map<Option<Block>>,
    /// Cases de saut redirigées vers un bloc, et leur valeur d'origine.
    links: Vec<(*mut u64, u64)>,
    code_bits: Vec<u64>,
    /// Instructions dont le programme réécrit l'immédiat (Doom : pas de
    /// texture des colonnes et des sols) : traduites avec l'immédiat lu en
    /// mémoire. Gardé d'un vidage à l'autre.
    volatile: std::collections::HashSet<u32>,
    mem_size: usize,
    /// Incrémentée quand la mémoire a pu changer hors du code traduit
    /// (rafale nouvelle, pas de l'interpréteur) : les blocs sont revérifiés.
    epoch: u64,
    pub stats: JitStats,
    /// Comptage des replis par mnémonique (diagnostic, coûteux).
    pub count_fallbacks: bool,
    /// Instructions au plus par bloc (1 : pas à pas, tests).
    pub max_block: usize,
}

/// Mode du processeur que le traducteur sait servir.
pub fn eligible(cpu: &Cpu) -> bool {
    let flat = |i: usize| {
        let s = &cpu.seg[i];
        s.base == 0 && s.limit == 0xFFFF_FFFF && !s.unusable
    };
    cpu.cr0 & 0x8000_0001 == 1 // protégé, sans pagination
        && cpu.flags & 0x0002_0100 == 0 // ni V86 ni pas à pas
        && cpu.code32()
        && cpu.stack32
        && (0..4).all(flat)
}

impl Jit {
    pub fn new(mem: &GuestMem) -> std::io::Result<Self> {
        Ok(Self {
            code: CodeBuf::new()?,
            blocks: Map::default(),
            links: Vec::new(),
            code_bits: vec![0; mem.size().div_ceil(64) + 1],
            volatile: Default::default(),
            mem_size: mem.size(),
            epoch: 1,
            stats: JitStats::default(),
            count_fallbacks: false,
            max_block: MAX_BLOCK,
        })
    }

    /// Vide le cache (code automodifiant, tampon plein).
    pub fn flush(&mut self) {
        self.blocks.clear();
        self.links.clear();
        self.code.used = 0;
        self.code_bits.fill(0);
    }

    /// La mémoire a pu changer hors du code traduit.
    pub fn touch(&mut self) {
        self.epoch += 1;
        // Les enchaînements sautent la vérification : défaits, ils se
        // refont après revérification de leurs cibles.
        for (slot, orig) in self.links.drain(..) {
            // SAFETY: case du tampon de code, écrite à l'émission.
            unsafe { slot.write_unaligned(orig) };
        }
    }

    /// Exécute à partir de l'état courant, au plus `budget` instructions,
    /// jusqu'à ce que `mmio.stop()` le demande, qu'une instruction ne puisse
    /// être servie ni par le traducteur ni par l'interpréteur, ou que plus
    /// de `idle` instructions passent sans activité de la carte
    /// (`Mmio::activity`) : le code natif fait alors mieux.
    pub fn run<M: Mmio>(&mut self, cpu: &mut Cpu, mem: &GuestMem, mmio: &mut M, budget: u64, idle: u64) -> (u64, StopReason) {
        self.touch();
        let mut st = State { membase: mem.ptr as u64, vram: mmio.direct_write_window(), ..Default::default() };
        let mut done = 0u64;
        let (mut activity, mut active_at) = (mmio.activity(), 0u64);
        loop {
            if done >= budget {
                return (done, StopReason::Budget);
            }
            let a = mmio.activity() + st.vga_writes;
            if a != activity {
                (activity, active_at) = (a, done);
            } else if done - active_at > idle {
                return (done, StopReason::Idle);
            }
            if mmio.stop() {
                return (done, StopReason::Stopped);
            }
            if !eligible(cpu) {
                return (done, StopReason::Unsupported);
            }
            let mut interp_next = true;
            if let Some(entry) = self.block(cpu.ip, mem) {
                st.gpr = cpu.gpr;
                st.flags = cpu.flags;
                st.code_bits = self.code_bits.as_ptr() as u64;
                st.total = 0;
                st.chain = CHAIN_MAX;
                st.link = 0;
                // SAFETY: code émis par `translate` pour cet état ; R15 et
                // les contrôles bornent les accès à la mémoire de l'invité.
                unsafe {
                    let f: extern "sysv64" fn(*mut State) = std::mem::transmute(entry);
                    f(&mut st);
                }
                cpu.gpr = st.gpr;
                cpu.flags = st.flags;
                cpu.ip = st.eip;
                done += st.total;
                self.stats.blocks_run += 1 + CHAIN_MAX - st.chain.min(CHAIN_MAX);
                self.stats.jit_instructions += st.total;
                match st.exit {
                    EXIT_SLOW => self.stats.slow_exits += 1,
                    EXIT_SMC => {
                        self.stats.smc_flushes += 1;
                        if self.count_fallbacks {
                            let what = format!("SMC by {}", interp::describe_at(cpu, mem));
                            if !self.stats.fallbacks.contains_key(&what) {
                                let mut c = cpu.clone();
                                eprintln!("[jit] {what} -> {:08X} ({:08X?})", st.addr, cpu.gpr);
                                c.ip = st.addr.saturating_sub(24);
                                let mut bytes = vec![0u8; 64];
                                mem.read(c.ip as usize, &mut bytes);
                                for i in Decoder::with_ip(32, &bytes, u64::from(c.ip), DecoderOptions::NONE).into_iter().take(14) {
                                    eprintln!("[jit]     {:08X} {i}", i.ip());
                                }
                            }
                            *self.stats.fallbacks.entry(what).or_default() += 1;
                        }
                        self.patched(cpu, mem, st.addr);
                        self.flush();
                    }
                    // Fin par un saut : on enchaîne sur le bloc suivant, et
                    // la sortie prise saute désormais directement vers lui.
                    _ if self.blocks.get(&cpu.ip).is_none_or(|b| b.is_some()) => {
                        interp_next = false;
                        if st.link != 0
                            && let Some(body) = self.block(cpu.ip, mem).and_then(|_| self.blocks.get(&cpu.ip)?.as_ref().map(|b| b.body))
                        {
                            let slot = st.link as *mut u64;
                            // SAFETY: case de la sortie, dans le tampon de code.
                            unsafe {
                                self.links.push((slot, slot.read_unaligned()));
                                slot.write_unaligned(body as u64);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if interp_next {
                if self.count_fallbacks {
                    let what = interp::describe_at(cpu, mem);
                    let mut mnemonic = what.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
                    // Immédiats variables : regroupés par adresse.
                    if let Some(i) = mnemonic.rfind(',')
                        && mnemonic[i + 1..].ends_with('h')
                    {
                        mnemonic = format!("{} {},imm (vol={})", &what[..8], &mnemonic[..i], self.volatile.contains(&cpu.ip));
                    }
                    *self.stats.fallbacks.entry(mnemonic).or_default() += 1;
                }
                let writes = interp_may_write(cpu, mem);
                let b = interp::run_burst(cpu, mem, mmio, u64::MAX, 1);
                self.stats.interp_steps += 1;
                if b.reason == StopReason::Unsupported {
                    // L'instruction refusée est comptée par `run_burst`.
                    return (done + b.instructions.saturating_sub(1), StopReason::Unsupported);
                }
                done += b.instructions;
                // Le pas a pu écrire dans du code traduit (sinon, comme pour
                // un `out`, les blocs restent vérifiés), ou changer la
                // configuration de la carte (registres de plans).
                if writes {
                    self.touch();
                }
                st.vram = mmio.direct_write_window();
            }
        }
    }

    /// Bloc à `eip`, traduit au besoin ; `None` si la première instruction
    /// revient à l'interpréteur.
    fn block(&mut self, eip: u32, mem: &GuestMem) -> Option<*const u8> {
        if let Some(slot) = self.blocks.get_mut(&eip) {
            match slot {
                None => return None,
                Some(b) if b.verified == self.epoch => return Some(b.entry),
                Some(b) => {
                    // SAFETY: le bloc a été lu dans la mémoire de l'invité à
                    // cette adresse (dans ses bornes) ; le vCPU est arrêté.
                    let now = unsafe { std::slice::from_raw_parts(mem.ptr.add(eip as usize), b.bytes.len()) };
                    if now.iter().zip(&b.mask).map(|(x, m)| x & m).eq(b.bytes.iter().copied()) {
                        b.verified = self.epoch;
                        return Some(b.entry);
                    }
                    // Code modifié hors du traducteur.
                    self.stats.stale_flushes += 1;
                    let changed: Vec<u32> = (0..now.len()).filter(|&i| now[i] & b.mask[i] != b.bytes[i]).map(|i| eip + i as u32).collect();
                    if self.count_fallbacks && self.stats.stale_flushes <= 5 {
                        eprintln!("[jit] stale block {eip:08X}: bytes changed at {changed:08X?}");
                    }
                    for a in changed {
                        self.mark_volatile(mem, a, 1);
                    }
                    self.flush();
                }
            }
        }
        // Assemblé à son adresse définitive (sauts relatifs) ; recommencé
        // au début du tampon s'il n'y tient plus.
        let mut emitted = None;
        for _ in 0..2 {
            // SAFETY: `used` reste dans le tampon.
            let at = unsafe { self.code.ptr.add(self.code.used) };
            match emit_block(eip, mem, self.mem_size, at as u64, &self.volatile, self.max_block) {
                Ok(Some((code, len, holes, body))) if self.code.used + code.len() <= CODE_SIZE => {
                    emitted = Some((at, code, len, holes, body));
                    break;
                }
                Ok(Some(_)) => self.flush(),
                Err(e) => {
                    if self.count_fallbacks {
                        eprintln!("[jit] assembly of {eip:08X} failed: {e}");
                    }
                    break;
                }
                _ => break,
            }
        }
        let entry = match emitted {
            None => {
                if self.count_fallbacks && self.volatile.contains(&eip) {
                    let all = decode_block(eip, mem, self.mem_size, 1);
                    let (ins, imm) = all[0];
                    eprintln!("[jit] volatile {eip:08X} {ins} {:?} imm {imm:?} native_ok {} form {:?}", ins.code(), native_ok(&ins), imm.and_then(|(o, _)| volatile_form(&ins, o)).is_some());
                }
                self.blocks.insert(eip, None);
                return None;
            }
            Some((at, code, len, holes, body)) => {
                // SAFETY: place vérifiée ; le tampon est RWX.
                unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), at, code.len()) };
                self.code.used += code.len().next_multiple_of(16);
                let mut bytes = vec![0u8; len];
                mem.read(eip as usize, &mut bytes);
                let mut mask = vec![0xFFu8; len];
                for &(off, n) in &holes {
                    mask[off..off + n].fill(0);
                }
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b &= mask[i];
                    if mask[i] != 0 {
                        let a = eip as usize + i;
                        self.code_bits[a / 64] |= 1 << (a % 64);
                    }
                }
                // SAFETY: décalage dans le code émis.
                let body = unsafe { at.add(body) };
                self.blocks.insert(eip, Some(Block { entry: at, body, bytes, mask, verified: self.epoch }));
                self.stats.translated += 1;
                at as *const u8
            }
        };
        Some(entry)
    }
}

impl Jit {
    /// Écriture de l'invité à `addr` dans du code traduit : si elle tombe
    /// dans l'immédiat d'une instruction, celle-ci devient volatile.
    fn patched(&mut self, cpu: &Cpu, mem: &GuestMem, addr: u32) {
        let mut b = [0u8; 15];
        mem.read(cpu.ip as usize, &mut b);
        let store = Decoder::with_ip(32, &b, u64::from(cpu.ip), DecoderOptions::NONE).decode();
        let size = store.memory_size().size().max(1) as u32;
        self.mark_volatile(mem, addr, size);
    }

    /// Octets `addr..addr+size` modifiés : les instructions traduites dont
    /// l'immédiat les contient deviennent volatiles.
    fn mark_volatile(&mut self, mem: &GuestMem, addr: u32, size: u32) {
        let starts: Vec<u32> = self.blocks.iter().filter(|(_, b)| b.is_some()).map(|(&k, _)| k).collect();
        for start in starts {
            let len = self.blocks[&start].as_ref().unwrap().bytes.len() as u32;
            if addr + size <= start || addr >= start + len {
                continue;
            }
            for (ins, imm) in decode_block(start, mem, self.mem_size, MAX_BLOCK) {
                let ip = ins.ip32();
                if let Some((off, n)) = imm
                    && addr >= ip + off as u32
                    && addr + size <= ip + (off + n) as u32
                {
                    self.volatile.insert(ip);
                }
            }
        }
    }
}

/// L'instruction à CS:EIP peut-elle écrire en mémoire ? (Opérande mémoire,
/// pile, chaînes, interruptions ; prudent : vrai si elle ne se décode pas.)
fn interp_may_write(cpu: &Cpu, mem: &GuestMem) -> bool {
    let at = cpu.ip as usize;
    if at + 15 > mem.size() {
        return true;
    }
    // SAFETY: dans les bornes ; le vCPU est arrêté.
    let bytes = unsafe { std::slice::from_raw_parts(mem.ptr.add(at), 15) };
    let ins = Decoder::with_ip(32, bytes, u64::from(cpu.ip), DecoderOptions::NONE).decode();
    if ins.is_invalid() {
        return true;
    }
    let reads_only = matches!(ins.mnemonic(), Mn::Cmp | Mn::Test | Mn::Bt | Mn::Lea | Mn::Nop);
    let memory = (0..ins.op_count()).any(|i| ins.op_kind(i) == OpKind::Memory) && !reads_only;
    let implicit = ins.is_string_instruction() || ins.is_stack_instruction() || matches!(ins.flow_control(), FlowControl::Interrupt | FlowControl::Call | FlowControl::IndirectCall)
        || matches!(ins.mnemonic(), Mn::Pushf | Mn::Pushfd | Mn::Enter | Mn::Leave);
    memory || implicit
}

/// Immédiat d'une instruction : décalage et taille dans ses octets.
type Imm = Option<(usize, usize)>;

/// Instructions de l'invité à partir de `eip`, jusqu'à la première fin de
/// bloc incluse, avec la place de leur immédiat.
fn decode_block(eip: u32, mem: &GuestMem, mem_size: usize, max: usize) -> Vec<(Instruction, Imm)> {
    let start = eip as usize;
    if start >= mem_size {
        return Vec::new();
    }
    let n = (MAX_BLOCK * 15).min(mem_size - start);
    let mut bytes = vec![0u8; n];
    mem.read(start, &mut bytes);
    let mut dec = Decoder::with_ip(32, &bytes, u64::from(eip), DecoderOptions::NONE);
    let mut out = Vec::new();
    while dec.can_decode() && out.len() < max {
        let ins = dec.decode();
        if ins.is_invalid() {
            break;
        }
        let c = dec.get_constant_offsets(&ins);
        let imm = c.has_immediate().then(|| (c.immediate_offset(), c.immediate_size()));
        let end = ins.flow_control() != FlowControl::Next;
        out.push((ins, imm));
        if end {
            break;
        }
    }
    out
}

/// Registre de l'invité vu dans le code de l'hôte (ESP → R12D) ; `None` si
/// le registre n'est pas traduisible.
fn host_reg(r: Register) -> Option<Register> {
    Some(match r {
        Register::ESP => Register::R12D,
        Register::SP => Register::R12W,
        r if r.is_gpr32() || r.is_gpr16() || r.is_gpr8() => r,
        _ => return None,
    })
}

/// Instructions exécutées telles quelles (registres et mémoire réécrits) :
/// sans faute possible une fois l'adresse contrôlée, sans pile, sans accès
/// implicite à la mémoire.
fn native_ok(ins: &Instruction) -> bool {
    use Mn::*;
    let m = ins.mnemonic();
    let ok = matches!(
        m,
        Mov | Movzx | Movsx | Lea | Add | Adc | Sub | Sbb | And | Or | Xor | Cmp | Test | Inc | Dec | Neg | Not | Shl | Sal | Shr | Sar
            | Rol | Ror | Rcl | Rcr | Shld | Shrd | Imul | Mul | Xchg | Cdq | Cwde | Cbw | Cwd | Bswap | Bsf | Bsr | Nop | Clc | Stc
            | Cmc | Lahf | Sahf | Sete | Setne | Setb | Setae | Seta | Setbe | Setl | Setge | Setg | Setle | Sets | Setns | Seto
            | Setno | Setp | Setnp | Cmove | Cmovne | Cmovb | Cmovae | Cmova | Cmovbe | Cmovl | Cmovge | Cmovg | Cmovle | Cmovs
            | Cmovns | Cmovo | Cmovno | Cmovp | Cmovnp | Bt | Bts | Btr | Btc
    );
    if !ok || ins.has_lock_prefix() || ins.has_rep_prefix() || ins.has_repne_prefix() {
        return false;
    }
    // Écriture par CS : #GP en mode protégé, laissée à l'interpréteur.
    if writes_memory(ins) && ins.memory_segment() == Register::CS {
        return false;
    }
    // shld/shrd sur 16 bits : compte > 16 non défini.
    if matches!(m, Shld | Shrd) && ins.memory_size().size() != 4 && !(ins.op1_kind() == OpKind::Register && ins.op1_register().is_gpr32()) {
        return false;
    }
    // bswap sur 16 bits : résultat non défini.
    if m == Bswap && ins.op0_register().is_gpr16() {
        return false;
    }
    // bt* [mem],reg adresse au-delà de l'opérande : laissé à l'interpréteur.
    if matches!(m, Bt | Bts | Btr | Btc) && ins.op0_kind() == OpKind::Memory && ins.op1_kind() == OpKind::Register {
        return false;
    }
    (0..ins.op_count()).all(|i| match ins.op_kind(i) {
        OpKind::Register => host_reg(ins.op_register(i)).is_some(),
        OpKind::Memory => {
            let ok_reg = |r: Register| r == Register::None || r.is_gpr32();
            ok_reg(ins.memory_base())
                && ok_reg(ins.memory_index())
                && matches!(ins.memory_segment(), Register::DS | Register::ES | Register::SS | Register::CS)
        }
        _ => true,
    })
}

/// L'instruction écrit-elle son opérande mémoire ?
fn writes_memory(ins: &Instruction) -> bool {
    use Mn::*;
    ins.op0_kind() == OpKind::Memory && !matches!(ins.mnemonic(), Cmp | Test | Bt)
        || (ins.mnemonic() == Xchg && ins.op1_kind() == OpKind::Memory)
}

/// Drapeaux arithmétiques écrits à coup sûr (les décalages et rotations,
/// dont le compte peut être nul, n'en écrivent aucun ici).
fn flags_written(ins: &Instruction) -> u32 {
    use Mn::*;
    if matches!(ins.mnemonic(), Shl | Sal | Shr | Sar | Rol | Ror | Rcl | Rcr | Shld | Shrd) {
        return 0;
    }
    rflags_to_x86(ins.rflags_modified())
}

fn flags_read(ins: &Instruction) -> u32 {
    rflags_to_x86(ins.rflags_read())
}

fn rflags_to_x86(f: u32) -> u32 {
    use iced_x86::RflagsBits as B;
    let mut x = 0;
    for (b, v) in [(B::CF, 0x1), (B::PF, 0x4), (B::AF, 0x10), (B::ZF, 0x40), (B::SF, 0x80), (B::OF, 0x800)] {
        if f & b != 0 {
            x |= v;
        }
    }
    x
}

/// Copie de `ins` pour l'hôte : registres réécrits, opérande mémoire sur
/// `[r15+r13]`, formes absentes du mode 64 bits remplacées.
fn rewrite(ins: &Instruction) -> Option<Instruction> {
    let mut h = *ins;
    let code = match ins.code() {
        Code::Inc_r16 => Some(Code::Inc_rm16),
        Code::Inc_r32 => Some(Code::Inc_rm32),
        Code::Dec_r16 => Some(Code::Dec_rm16),
        Code::Dec_r32 => Some(Code::Dec_rm32),
        Code::Mov_AL_moffs8 => Some(Code::Mov_r8_rm8),
        Code::Mov_AX_moffs16 => Some(Code::Mov_r16_rm16),
        Code::Mov_EAX_moffs32 => Some(Code::Mov_r32_rm32),
        Code::Mov_moffs8_AL => Some(Code::Mov_rm8_r8),
        Code::Mov_moffs16_AX => Some(Code::Mov_rm16_r16),
        Code::Mov_moffs32_EAX => Some(Code::Mov_rm32_r32),
        _ => None,
    };
    if let Some(c) = code {
        h.set_code(c);
    }
    for i in 0..ins.op_count() {
        match ins.op_kind(i) {
            OpKind::Register => h.set_op_register(i, host_reg(ins.op_register(i))?),
            OpKind::Memory => {
                h.set_op_kind(i, OpKind::Memory);
                h.set_memory_base(Register::R15);
                h.set_memory_index(Register::R13);
                h.set_memory_index_scale(1);
                h.set_memory_displacement64(0);
                h.set_memory_displ_size(0);
                h.set_segment_prefix(Register::None);
            }
            _ => {}
        }
    }
    if ins.mnemonic() == Mn::Lea {
        return None; // traité à part
    }
    // Adresse neutre : l'assembleur refuse deux instructions de même IP.
    h.set_ip(0);
    let mut enc = Encoder::new(64);
    enc.encode(&h, 0).ok()?;
    Some(h)
}

/// Opérande mémoire de l'invité pour `lea` dans le code de l'hôte (adressage
/// 32 bits, ESP → R12D).
fn guest_address(ins: &Instruction) -> Option<MemoryOperand> {
    let map = |r: Register| if r == Register::None { Some(r) } else { host_reg(r) };
    Some(MemoryOperand::new(
        map(ins.memory_base())?,
        map(ins.memory_index())?,
        ins.memory_index_scale(),
        ins.memory_displacement32() as i32 as i64,
        4,
        false,
        Register::None,
    ))
}

/// Sortie de bloc en attente d'émission.
struct Exit {
    label: CodeLabel,
    /// EIP de reprise ; `None` : calculé dans R8D (ret, sauts indirects).
    eip: Option<u32>,
    count: u32,
    kind: u32,
    /// Drapeaux de l'invité sauvés par un contrôle (`OFF_SAVED`), à rétablir
    /// d'abord.
    pushed: bool,
}

/// Instruction du corps d'un bloc, par genre de traduction.
enum Op {
    /// Recopiée (registres et opérande mémoire réécrits).
    Native(Instruction),
    /// Immédiat volatil lu dans la mémoire de l'invité.
    Volatile(Instruction),
    /// Adresse de l'invité calculée directement dans la destination.
    Lea,
    Push(Option<Register>, u32),
    Pop(Register),
    Pushad,
    Popad,
    Leave,
    /// `mov r8h,[m]` / `mov [m],r8h` : registre 32 bits complet ; les
    /// registres hauts ne s'encodent pas avec R13/R15.
    HighLoad(Register),
    HighStore(Register),
}

/// Fin de bloc.
enum Tail {
    /// Instruction suivante laissée à l'interpréteur.
    Fallthrough,
    Jump(Instruction),
    Call(Instruction),
    /// `ret` (octets de pile à libérer en plus).
    Ret(u32),
    /// `jmp`/`call` vers une cible en registre ou en mémoire.
    Indirect(Instruction),
}

/// Registre 32 bits d'un registre haut (AH → EAX…).
fn high_full(r: Register) -> Option<Register> {
    Some(match r {
        Register::AH => Register::EAX,
        Register::CH => Register::ECX,
        Register::DH => Register::EDX,
        Register::BH => Register::EBX,
        _ => return None,
    })
}

/// Opérande mémoire plat, adressage 32 bits (pas FS/GS).
fn flat_memory(ins: &Instruction) -> bool {
    let ok_reg = |r: Register| r == Register::None || r.is_gpr32();
    ok_reg(ins.memory_base())
        && ok_reg(ins.memory_index())
        && matches!(ins.memory_segment(), Register::DS | Register::ES | Register::SS | Register::CS)
}

/// Genre de traduction d'une instruction non terminale ; `None` : rendue à
/// l'interpréteur.
fn classify(ins: &Instruction, imm: Imm, volatile: &std::collections::HashSet<u32>) -> Option<Op> {
    if ins.has_lock_prefix() || ins.has_rep_prefix() || ins.has_repne_prefix() {
        return None;
    }
    let reg32 = |i: u32| ins.op_kind(i) == OpKind::Register && ins.op_register(i).is_gpr32();
    match ins.code() {
        Code::Push_r32 | Code::Push_rm32 if reg32(0) => return Some(Op::Push(host_reg(ins.op0_register()), 0)),
        Code::Pushd_imm32 | Code::Pushd_imm8 => return Some(Op::Push(None, ins.immediate(0) as u32)),
        Code::Pop_r32 | Code::Pop_rm32 if reg32(0) => return Some(Op::Pop(host_reg(ins.op0_register())?)),
        Code::Pushad => return Some(Op::Pushad),
        Code::Popad => return Some(Op::Popad),
        Code::Leaved => return Some(Op::Leave),
        Code::Mov_r8_rm8 if ins.op1_kind() == OpKind::Memory && flat_memory(ins) => {
            if let Some(full) = high_full(ins.op0_register()) {
                return Some(Op::HighLoad(full));
            }
        }
        Code::Mov_rm8_r8 if ins.op0_kind() == OpKind::Memory && flat_memory(ins) && ins.memory_segment() != Register::CS => {
            if let Some(full) = high_full(ins.op1_register()) {
                return Some(Op::HighStore(full));
            }
        }
        _ => {}
    }
    if !native_ok(ins) {
        return None;
    }
    if volatile.contains(&ins.ip32()) {
        let (off, _) = imm?;
        return volatile_form(ins, off).map(Op::Volatile);
    }
    if ins.mnemonic() == Mn::Lea {
        let ok = ins.op0_kind() == OpKind::Register && ins.op0_register().is_gpr32() && guest_address(ins).is_some();
        return ok.then_some(Op::Lea);
    }
    rewrite(ins).map(Op::Native)
}

/// Fin de bloc traduite ; `None` : rendue à l'interpréteur.
fn classify_tail(ins: &Instruction) -> Option<Tail> {
    let target_ok = |ins: &Instruction| match ins.op0_kind() {
        OpKind::Register => ins.op0_register().is_gpr32(),
        OpKind::Memory => flat_memory(ins) && ins.memory_size().size() == 4,
        _ => false,
    };
    match ins.flow_control() {
        // jmp et jcc seulement : jecxz et loop* (iced donne e/ne à loope et
        // loopne) décrémentent ou testent ECX, laissés à l'interpréteur.
        FlowControl::UnconditionalBranch | FlowControl::ConditionalBranch
            if (ins.is_jcc_short_or_near() || ins.is_jmp_short_or_near()) && ins.op0_kind() == OpKind::NearBranch32 =>
        {
            Some(Tail::Jump(*ins))
        }
        FlowControl::Call if ins.code() == Code::Call_rel32_32 => Some(Tail::Call(*ins)),
        FlowControl::Return if ins.code() == Code::Retnd => Some(Tail::Ret(0)),
        FlowControl::Return if ins.code() == Code::Retnd_imm16 => Some(Tail::Ret(u32::from(ins.immediate16()))),
        FlowControl::IndirectBranch if ins.code() == Code::Jmp_rm32 && target_ok(ins) => Some(Tail::Indirect(*ins)),
        FlowControl::IndirectCall if ins.code() == Code::Call_rm32 && target_ok(ins) => Some(Tail::Indirect(*ins)),
        _ => None,
    }
}

/// Émetteur d'un bloc.
struct Emit {
    a: CodeAssembler,
    exits: Vec<Exit>,
    mem_limit: i32,
}

impl Emit {
    /// Sauve les drapeaux de l'invité avant un contrôle. Toujours avant une
    /// écriture : elle peut modifier le code qui suit, et donc la vivacité
    /// calculée ; les sorties qui en partent les rétablissent exactement.
    fn save(&mut self) -> Result<(), IcedError> {
        self.a.pushfq()?;
        self.a.pop(qword_ptr(r10 + OFF_SAVED))
    }

    fn restore(&mut self) -> Result<(), IcedError> {
        self.a.push(qword_ptr(r10 + OFF_SAVED))?;
        self.a.popfq()
    }

    fn exit(&mut self, eip: Option<u32>, count: u32, kind: u32, pushed: bool) -> CodeLabel {
        let label = self.a.create_label();
        self.exits.push(Exit { label, eip, count, kind, pushed });
        label
    }

    /// Contrôles d'un accès de `size` octets à l'adresse de l'invité dans
    /// R13D : fin de la mémoire et zone A0000–FFFFF (chemin lent), octets de
    /// code traduit pour une écriture. Écrase les drapeaux, R9, R11, R14.
    /// Sans `vga`, la fenêtre mène au chemin lent ; sinon à `vga`.
    fn guard(&mut self, ip: u32, count: u32, size: i32, write: bool, pushed: bool, vga: Option<CodeLabel>) -> Result<(), IcedError> {
        let slow = self.exit(Some(ip), count, EXIT_SLOW, pushed);
        let a = &mut self.a;
        a.cmp(r13d, self.mem_limit)?;
        a.jae(slow)?;
        // [r13, r13+size) touche-t-il A0000–FFFFF ?
        a.lea(r14d, ptr(r13 - (0xA0000 - size + 1)))?;
        a.cmp(r14d, 0x60000 + size - 1)?;
        a.jb(vga.unwrap_or(slow))?;
        if write {
            let smc = self.exit(Some(ip), count, EXIT_SMC, pushed);
            let a = &mut self.a;
            // Bits du premier et du dernier octet écrits.
            a.mov(r9, qword_ptr(r10 + OFF_CODE_BITS))?;
            a.mov(r14d, r13d)?;
            a.shr(r14d, 3)?;
            a.movzx(r14d, word_ptr(r9 + r14))?;
            a.mov(r11d, r13d)?;
            a.and(r11d, 7)?;
            a.bt(r14d, r11d)?;
            a.jb(smc)?;
            if size > 1 {
                a.add(r11d, size - 1)?;
                a.bt(r14d, r11d)?;
                a.jb(smc)?;
            }
        }
        Ok(())
    }

    /// Adresse de l'opérande mémoire de l'invité dans R13D.
    fn address(&mut self, ins: &Instruction) -> Result<(), IcedError> {
        let m = guest_address(ins).unwrap();
        if m.base == Register::None && m.index == Register::None {
            self.a.mov(r13d, m.displacement as u32)?;
        } else {
            self.a.add_instruction(Instruction::with2(Code::Lea_r32_m, Register::R13D, m)?)?;
        }
        Ok(())
    }
}

fn r32(r: Register) -> AsmRegister32 {
    match r {
        Register::EAX => eax,
        Register::ECX => ecx,
        Register::EDX => edx,
        Register::EBX => ebx,
        Register::R12D => r12d,
        Register::EBP => ebp,
        Register::ESI => esi,
        Register::EDI => edi,
        _ => unreachable!("{r:?}"),
    }
}

/// Pile et registres hauts : contrôle de l'accès (drapeaux sauvés autour
/// s'ils sont vivants), puis l'opération.
fn emit_stackish(e: &mut Emit, ins: &Instruction, op: &Op, ip: u32, n: u32, live: bool) -> Result<(), IcedError> {
    let pushed = live || matches!(op, Op::Push(..) | Op::Pushad | Op::HighStore(_));
    if pushed {
        e.save()?;
    }
    match *op {
        Op::Push(src, imm) => {
            e.a.lea(r13d, ptr(r12 - 4))?;
            e.guard(ip, n, 4, true, pushed, None)?;
            match src {
                Some(r) => e.a.mov(dword_ptr(r15 + r13), r32(r))?,
                None => e.a.mov(dword_ptr(r15 + r13), imm)?,
            }
            e.a.mov(r12d, r13d)?;
        }
        Op::Pop(dst) => {
            e.a.mov(r13d, r12d)?;
            e.guard(ip, n, 4, false, pushed, None)?;
            e.a.mov(r11d, dword_ptr(r15 + r13))?;
            e.a.lea(r12d, ptr(r12 + 4))?;
            e.a.mov(r32(dst), r11d)?;
        }
        Op::Pushad => {
            e.a.lea(r13d, ptr(r12 - 32))?;
            e.guard(ip, n, 32, true, pushed, None)?;
            for (k, r) in [edi, esi, ebp, r12d, ebx, edx, ecx, eax].into_iter().enumerate() {
                e.a.mov(dword_ptr(r15 + r13 + 4 * k as i32), r)?;
            }
            e.a.mov(r12d, r13d)?;
        }
        Op::Popad => {
            e.a.mov(r13d, r12d)?;
            e.guard(ip, n, 32, false, pushed, None)?;
            for (k, r) in [Some(edi), Some(esi), Some(ebp), None, Some(ebx), Some(edx), Some(ecx), Some(eax)].into_iter().enumerate() {
                if let Some(r) = r {
                    e.a.mov(r, dword_ptr(r15 + r13 + 4 * k as i32))?;
                }
            }
            e.a.lea(r12d, ptr(r13 + 32))?;
        }
        Op::Leave => {
            e.a.mov(r13d, ebp)?;
            e.guard(ip, n, 4, false, pushed, None)?;
            e.a.mov(r11d, dword_ptr(r15 + r13))?;
            e.a.lea(r12d, ptr(r13 + 4))?;
            e.a.mov(ebp, r11d)?;
        }
        Op::HighLoad(full) => {
            e.address(ins)?;
            e.guard(ip, n, 1, false, pushed, None)?;
            let f = r32(full);
            e.a.movzx(r11d, byte_ptr(r15 + r13))?;
            e.a.shl(r11d, 8)?;
            e.a.and(f, 0xFFFF_00FFu32 as i32)?;
            e.a.or(f, r11d)?;
        }
        Op::HighStore(full) => {
            e.address(ins)?;
            e.guard(ip, n, 1, true, pushed, None)?;
            e.a.mov(r11d, r32(full))?;
            e.a.shr(r11d, 8)?;
            e.a.mov(byte_ptr(r15 + r13), r11b)?;
        }
        _ => unreachable!(),
    }
    if live {
        e.restore()?;
    }
    Ok(())
}

/// Émet le bloc à `eip`, assemblé pour l'adresse `rip`. `Ok(None)` : la
/// première instruction n'est pas traduite.
/// Résultat : code, longueur en octets de l'invité, trous (immédiats
/// volatils : décalage dans le bloc, taille).
/// Le dernier élément est le décalage du corps (cible des enchaînements).
type Emitted = (Vec<u8>, usize, Vec<(usize, usize)>, usize);

fn emit_block(
    eip: u32,
    mem: &GuestMem,
    mem_size: usize,
    rip: u64,
    volatile: &std::collections::HashSet<u32>,
    max: usize,
) -> Result<Option<Emitted>, IcedError> {
    let all = decode_block(eip, mem, mem_size, max);
    let mut holes = Vec::new();
    let mut body: Vec<(Instruction, Op)> = Vec::new();
    let mut tail = Tail::Fallthrough;
    let mut tail_ins = None;
    for (ins, imm) in &all {
        if ins.flow_control() == FlowControl::Next {
            match classify(ins, *imm, volatile) {
                Some(op) => {
                    if let (Op::Volatile(_), Some((off, n))) = (&op, imm) {
                        holes.push(((ins.ip32() - eip) as usize + off, *n));
                    }
                    body.push((*ins, op));
                }
                None => break,
            }
        } else {
            if let Some(t) = classify_tail(ins) {
                tail = t;
                tail_ins = Some(*ins);
            }
            break;
        }
    }
    if body.is_empty() && tail_ins.is_none() {
        return Ok(None);
    }
    let end = tail_ins.as_ref().or(body.last().map(|(i, _)| i)).unwrap().next_ip32();
    let guest_len = (end - eip) as usize;

    // Vivacité des drapeaux arithmétiques avant chaque instruction (tous
    // vivants à la sortie du bloc).
    let mut live = vec![0u32; body.len() + 1];
    live[body.len()] = ARITH;
    for (i, (ins, _)) in body.iter().enumerate().rev() {
        live[i] = (live[i + 1] & !flags_written(ins)) | flags_read(ins);
    }

    let mut e = Emit { a: CodeAssembler::new(64)?, exits: Vec::new(), mem_limit: (mem_size as u32).saturating_sub(MEM_GUARD) as i32 };
    // Prologue.
    for r in [rbx, rbp, r12, r13, r14, r15] {
        e.a.push(r)?;
    }
    e.a.mov(r10, rdi)?;
    e.a.mov(r15, qword_ptr(r10 + OFF_MEMBASE))?;
    e.a.mov(r11d, dword_ptr(r10 + OFF_FLAGS))?;
    e.a.and(r11d, HOST_FLAGS as i32)?;
    e.a.push(r11)?;
    e.a.popfq()?;
    for (i, r) in [eax, ecx, edx, ebx, r12d, ebp, esi].into_iter().enumerate() {
        e.a.mov(r, dword_ptr(r10 + 4 * i as i32))?;
    }
    e.a.mov(edi, dword_ptr(r10 + 28))?;
    let mut body_label = e.a.create_label();
    e.a.set_label(&mut body_label)?;

    for (i, (ins, op)) in body.iter().enumerate() {
        let (ip, n) = (ins.ip32(), i as u32);
        let live_here = live[i] != 0;
        match op {
            Op::Lea => {
                let dst = host_reg(ins.op0_register()).unwrap();
                e.a.add_instruction(Instruction::with2(Code::Lea_r32_m, dst, guest_address(ins).unwrap())?)?;
            }
            Op::Volatile(h) => e.a.add_instruction(*h)?,
            Op::Native(h) if !(0..ins.op_count()).any(|k| ins.op_kind(k) == OpKind::Memory) => e.a.add_instruction(*h)?,
            Op::Native(h) => {
                // Écriture simple (mov) dans la fenêtre : directement dans le plan.
                let direct = ins.mnemonic() == Mn::Mov && ins.op0_kind() == OpKind::Memory;
                let mut vga = e.a.create_label();
                let pushed = live_here || writes_memory(ins);
                if pushed {
                    e.save()?;
                }
                e.address(ins)?;
                let size = ins.memory_size().size().max(1) as i32;
                e.guard(ip, n, size, writes_memory(ins), pushed, direct.then_some(vga))?;
                if live_here {
                    e.restore()?;
                }
                e.a.add_instruction(*h)?;
                if direct {
                    let slow = e.exit(Some(ip), n, EXIT_SLOW, pushed);
                    let a = &mut e.a;
                    let mut done = a.create_label();
                    a.jmp(done)?;
                    a.set_label(&mut vga)?;
                    a.lea(r14d, ptr(r13 - 0xA0000))?;
                    a.cmp(r14d, 0x10000 - size + 1)?;
                    a.jae(slow)?;
                    a.mov(r9, qword_ptr(r10 + OFF_VRAM))?;
                    a.test(r9, r9)?;
                    a.jz(slow)?;
                    a.add(qword_ptr(r10 + OFF_VGA_WRITES), 1)?;
                    if live_here {
                        e.restore()?;
                    }
                    let a = &mut e.a;
                    let mut w = *h;
                    w.set_memory_base(Register::R9);
                    w.set_memory_index(Register::R14);
                    a.add_instruction(w)?;
                    a.set_label(&mut done)?;
                    a.nop()?;
                }
            }
            _ => emit_stackish(&mut e, ins, op, ip, n, live_here)?,
        }
    }

    let n = body.len() as u32;
    match &tail {
        Tail::Fallthrough => {
            let l = e.exit(Some(end), n, EXIT_NEXT, false);
            e.a.jmp(l)?;
        }
        Tail::Jump(t) => {
            let taken = e.exit(Some(t.near_branch32()), n + 1, EXIT_NEXT, false);
            if t.flow_control() == FlowControl::ConditionalBranch {
                jcc(&mut e.a, t.condition_code(), taken)?;
                let not = e.exit(Some(end), n + 1, EXIT_NEXT, false);
                e.a.jmp(not)?;
            } else {
                e.a.jmp(taken)?;
            }
        }
        Tail::Call(t) => {
            let ip = t.ip32();
            e.save()?;
            e.a.lea(r13d, ptr(r12 - 4))?;
            e.guard(ip, n, 4, true, true, None)?;
            e.restore()?;
            e.a.mov(dword_ptr(r15 + r13), end)?;
            e.a.mov(r12d, r13d)?;
            let l = e.exit(Some(t.near_branch32()), n + 1, EXIT_NEXT, false);
            e.a.jmp(l)?;
        }
        Tail::Ret(extra) => {
            let ip = tail_ins.unwrap().ip32();
            e.save()?;
            e.a.mov(r13d, r12d)?;
            e.guard(ip, n, 4, false, true, None)?;
            e.restore()?;
            e.a.mov(r8d, dword_ptr(r15 + r13))?;
            e.a.lea(r12d, ptr(r12 + 4 + *extra as i32))?;
            let l = e.exit(None, n + 1, EXIT_NEXT, false);
            e.a.jmp(l)?;
        }
        Tail::Indirect(t) => {
            let ip = t.ip32();
            e.save()?;
            if t.op0_kind() == OpKind::Register {
                e.a.mov(r8d, r32(host_reg(t.op0_register()).unwrap()))?;
            } else {
                e.address(t)?;
                e.guard(ip, n, 4, false, true, None)?;
                e.a.mov(r8d, dword_ptr(r15 + r13))?;
            }
            if t.flow_control() == FlowControl::IndirectCall {
                e.a.lea(r13d, ptr(r12 - 4))?;
                e.guard(ip, n, 4, true, true, None)?;
                e.a.mov(dword_ptr(r15 + r13), end)?;
                e.a.mov(r12d, r13d)?;
            }
            e.restore()?;
            let l = e.exit(None, n + 1, EXIT_NEXT, false);
            e.a.jmp(l)?;
        }
    }

    // Sorties, puis épilogue commun.
    let Emit { mut a, mut exits, .. } = e;
    let mut common = a.create_label();
    // Cases de saut des sorties enchaînables : (case, sortie normale).
    let mut slots = Vec::new();
    for x in &mut exits {
        a.set_label(&mut x.label)?;
        if x.pushed {
            a.push(qword_ptr(r10 + OFF_SAVED))?;
            a.popfq()?;
        }
        // Instructions exécutées, sans toucher aux drapeaux.
        a.mov(r9, qword_ptr(r10 + OFF_TOTAL))?;
        a.lea(r9, ptr(r9 + x.count as i32))?;
        a.mov(qword_ptr(r10 + OFF_TOTAL), r9)?;
        if x.kind == EXIT_NEXT && x.eip.is_some() {
            // Enchaînement : compteur décrémenté par RCX et `jrcxz` (les
            // drapeaux de l'invité sont vivants), puis saut par la case.
            let mut slot = a.create_label();
            let mut out = a.create_label();
            let mut unlinked = a.create_label();
            a.mov(r11, rcx)?;
            a.mov(rcx, qword_ptr(r10 + OFF_CHAIN))?;
            a.lea(rcx, ptr(rcx - 1))?;
            a.mov(qword_ptr(r10 + OFF_CHAIN), rcx)?;
            a.jrcxz(out)?;
            a.mov(rcx, r11)?;
            a.jmp(qword_ptr(slot))?;
            a.set_label(&mut slot)?;
            a.dq(&[0])?;
            a.set_label(&mut out)?;
            a.mov(rcx, r11)?;
            a.set_label(&mut unlinked)?;
            a.lea(r9, ptr(slot))?;
            a.mov(qword_ptr(r10 + OFF_LINK), r9)?;
            slots.push((slot, unlinked));
        } else {
            a.mov(qword_ptr(r10 + OFF_LINK), 0)?;
        }
        match x.eip {
            Some(v) => a.mov(dword_ptr(r10 + OFF_EIP), v)?,
            None => a.mov(dword_ptr(r10 + OFF_EIP), r8d)?,
        }
        a.mov(dword_ptr(r10 + OFF_EXIT), x.kind)?;
        if x.kind != EXIT_NEXT {
            a.mov(dword_ptr(r10 + OFF_ADDR), r13d)?;
        }
        a.jmp(common)?;
    }
    a.set_label(&mut common)?;
    a.pushfq()?;
    a.pop(r11)?;
    a.mov(r13d, dword_ptr(r10 + OFF_FLAGS))?;
    a.and(r13d, !HOST_FLAGS as i32)?;
    a.and(r11d, HOST_FLAGS as i32)?;
    a.or(r11d, r13d)?;
    a.mov(dword_ptr(r10 + OFF_FLAGS), r11d)?;
    a.cld()?;
    for (i, r) in [eax, ecx, edx, ebx, r12d, ebp, esi, edi].into_iter().enumerate() {
        a.mov(dword_ptr(r10 + 4 * i as i32), r)?;
    }
    for r in [r15, r14, r13, r12, rbp, rbx] {
        a.pop(r)?;
    }
    a.ret()?;
    let r = a.assemble_options(rip, BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS)?;
    let body_off = (r.label_ip(&body_label)? - rip) as usize;
    // Chaque case vise d'abord sa sortie normale.
    let patches: Vec<(usize, u64)> =
        slots.iter().map(|(slot, unlinked)| Ok(((r.label_ip(slot)? - rip) as usize, r.label_ip(unlinked)?))).collect::<Result<_, IcedError>>()?;
    let mut code = r.inner.code_buffer;
    for (at, target) in patches {
        code[at..at + 8].copy_from_slice(&target.to_le_bytes());
    }
    Ok(Some((code, guest_len, holes, body_off)))
}

fn jcc(a: &mut CodeAssembler, cc: ConditionCode, l: CodeLabel) -> Result<(), IcedError> {
    match cc {
        ConditionCode::o => a.jo(l),
        ConditionCode::no => a.jno(l),
        ConditionCode::b => a.jb(l),
        ConditionCode::ae => a.jae(l),
        ConditionCode::e => a.je(l),
        ConditionCode::ne => a.jne(l),
        ConditionCode::be => a.jbe(l),
        ConditionCode::a => a.ja(l),
        ConditionCode::s => a.js(l),
        ConditionCode::ns => a.jns(l),
        ConditionCode::p => a.jp(l),
        ConditionCode::np => a.jnp(l),
        ConditionCode::l => a.jl(l),
        ConditionCode::ge => a.jge(l),
        ConditionCode::le => a.jle(l),
        ConditionCode::g => a.jg(l),
        ConditionCode::None => unreachable!("jecxz/loop"),
    }
}

/// Après une sortie de VM déjà servie : rafale par le traducteur, puis
/// recopie de l'état (comme `interp::continue_in_host`).
pub fn continue_in_host<B: crate::backend::Backend, M: Mmio>(
    jit: &mut Jit,
    vm: &mut B,
    mmio: &mut M,
    budget: u64,
    idle: u64,
) -> crate::backend::Result<interp::Burst> {
    vm.complete_pending(&mut |addr, data, write| {
        for (i, b) in data.iter_mut().enumerate() {
            let a = addr as u32 + i as u32;
            if write {
                mmio.write8(a, *b);
            } else {
                *b = if mmio.claims(a, false) { mmio.read8(a) } else { 0xFF };
            }
        }
    })?;
    let mut cpu = vm.state()?;
    let (instructions, reason) = jit.run(&mut cpu, vm.mem(), mmio, budget, idle);
    vm.set_state(&cpu)?;
    Ok(interp::Burst { instructions, mmio_accesses: 0, reason })
}

/// Forme de l'hôte d'une instruction à immédiat volatil : l'immédiat est lu
/// dans la mémoire de l'invité (`[r15 + adresse]`). Formes registre seules.
fn volatile_form(ins: &Instruction, imm_off: usize) -> Option<Instruction> {
    if ins.op0_kind() != OpKind::Register || !ins.op0_register().is_gpr32() {
        return None;
    }
    let code = match ins.code() {
        Code::Add_rm32_imm32 => Code::Add_r32_rm32,
        Code::Sub_rm32_imm32 => Code::Sub_r32_rm32,
        Code::Adc_rm32_imm32 => Code::Adc_r32_rm32,
        Code::Sbb_rm32_imm32 => Code::Sbb_r32_rm32,
        Code::And_rm32_imm32 => Code::And_r32_rm32,
        Code::Or_rm32_imm32 => Code::Or_r32_rm32,
        Code::Xor_rm32_imm32 => Code::Xor_r32_rm32,
        Code::Cmp_rm32_imm32 => Code::Cmp_r32_rm32,
        Code::Mov_r32_imm32 | Code::Mov_rm32_imm32 => Code::Mov_r32_rm32,
        _ => return None,
    };
    let at = ins.ip32() + imm_off as u32;
    // Adressage 64 bits : iced attend une taille de déplacement de 8.
    let m = MemoryOperand::new(Register::R15, Register::None, 1, i64::from(at), 8, false, Register::None);
    let h = Instruction::with2(code, host_reg(ins.op0_register())?, m).ok()?;
    Encoder::new(64).encode(&h, 0).ok()?;
    Some(h)
}
