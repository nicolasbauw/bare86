// SPDX-License-Identifier: GPL-3.0-or-later
//! Sémantique des instructions (mode réel, opérandes 16 et 32 bits).
//!
//! Règle : toute vérification (limites de segment, division, cas non gérés)
//! précède la première écriture. Un `Bail` laisse donc l'état intact et la
//! VM réexécute l'instruction nativement, faute comprise.

use iced_x86::{Code, ConditionCode, Instruction, Mnemonic as Mn, OpKind, Register as R};

use super::alu::*;
use super::{Cpu, Mmio};
use crate::mem::GuestMem;

/// L'interpréteur renonce : l'instruction sera exécutée par la VM.
#[derive(Debug)]
pub struct Bail;

pub type X<T = ()> = Result<T, Bail>;

#[derive(Clone, Copy)]
enum Loc {
    Reg(R),
    Mem { seg: R, off: u32 },
    Imm(u32),
}

pub(super) fn seg_idx(r: R) -> usize {
    r as usize - R::ES as usize
}

impl Cpu {
    pub(super) fn reg(&self, r: R) -> u32 {
        if r.is_gpr8() {
            let n = r as usize - R::AL as usize;
            if n < 4 { self.gpr[n] & 0xFF } else { (self.gpr[n - 4] >> 8) & 0xFF }
        } else if r.is_segment_register() {
            self.seg[seg_idx(r)].selector.into()
        } else if r.is_gpr16() {
            self.gpr[r.number()] & 0xFFFF
        } else {
            self.gpr[r.number()]
        }
    }

    pub(super) fn set_reg(&mut self, r: R, v: u32) {
        if r.is_gpr8() {
            let n = r as usize - R::AL as usize;
            if n < 4 {
                self.gpr[n] = (self.gpr[n] & !0xFF) | (v & 0xFF);
            } else {
                self.gpr[n - 4] = (self.gpr[n - 4] & !0xFF00) | ((v & 0xFF) << 8);
            }
        } else if r.is_segment_register() {
            self.set_sreg(seg_idx(r), v as u16);
        } else if r.is_gpr16() {
            let n = r.number();
            self.gpr[n] = (self.gpr[n] & !0xFFFF) | (v & 0xFFFF);
        } else {
            self.gpr[r.number()] = v;
        }
    }

    /// Mode réel : base = sélecteur × 16, limite conservée (unreal mode).
    fn set_sreg(&mut self, idx: usize, sel: u16) {
        let s = &mut self.seg[idx];
        s.selector = sel;
        s.base = u32::from(sel) << 4;
        self.seg_dirty = true;
    }

    fn sp(&self) -> u32 {
        if self.stack32 { self.gpr[4] } else { self.gpr[4] & 0xFFFF }
    }

    fn set_sp(&mut self, v: u32) {
        self.gpr[4] = if self.stack32 { v } else { (self.gpr[4] & !0xFFFF) | (v & 0xFFFF) };
    }

    fn sp_mask(&self) -> u32 {
        if self.stack32 { u32::MAX } else { 0xFFFF }
    }

    fn flag(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    fn set_flags(&mut self, which: u32, values: u32) {
        self.flags = (self.flags & !which) | (values & which);
    }

    fn condition(&self, cc: ConditionCode) -> bool {
        let (c, z, s, o, p) = (self.flag(CF), self.flag(ZF), self.flag(SF), self.flag(OF), self.flag(PF));
        match cc {
            ConditionCode::o => o,
            ConditionCode::no => !o,
            ConditionCode::b => c,
            ConditionCode::ae => !c,
            ConditionCode::e => z,
            ConditionCode::ne => !z,
            ConditionCode::be => c || z,
            ConditionCode::a => !c && !z,
            ConditionCode::s => s,
            ConditionCode::ns => !s,
            ConditionCode::p => p,
            ConditionCode::np => !p,
            ConditionCode::l => s != o,
            ConditionCode::ge => s == o,
            ConditionCode::le => z || s != o,
            ConditionCode::g => !z && s == o,
            ConditionCode::None => true,
        }
    }
}

fn is_setcc(m: Mn) -> bool {
    matches!(
        m,
        Mn::Seto
            | Mn::Setno
            | Mn::Setb
            | Mn::Setae
            | Mn::Sete
            | Mn::Setne
            | Mn::Setbe
            | Mn::Seta
            | Mn::Sets
            | Mn::Setns
            | Mn::Setp
            | Mn::Setnp
            | Mn::Setl
            | Mn::Setge
            | Mn::Setle
            | Mn::Setg
    )
}

fn is_cmovcc(m: Mn) -> bool {
    matches!(
        m,
        Mn::Cmovo
            | Mn::Cmovno
            | Mn::Cmovb
            | Mn::Cmovae
            | Mn::Cmove
            | Mn::Cmovne
            | Mn::Cmovbe
            | Mn::Cmova
            | Mn::Cmovs
            | Mn::Cmovns
            | Mn::Cmovp
            | Mn::Cmovnp
            | Mn::Cmovl
            | Mn::Cmovge
            | Mn::Cmovle
            | Mn::Cmovg
    )
}

pub(super) struct Exec<'a, B: Mmio> {
    pub cpu: &'a mut Cpu,
    pub mem: &'a GuestMem,
    pub mmio: &'a mut B,
    pub mmio_hits: u64,
    /// Itérations de REP encore permises dans cette rafale.
    pub rep_budget: u64,
    /// Adresse linéaire du mot de drapeaux empilé (tests : bit TF du pas à pas).
    pub flags_push: Option<u32>,
    /// Préfixe 67h : adressage 32 bits.
    pub addr32: bool,
    /// Exécution à blanc (`probe_spin`) : les écritures vont dans cette
    /// mémoire superposée, pas dans celle de l'invité.
    pub dry: Option<&'a mut Overlay>,
}

/// Mémoire superposée d'une exécution à blanc : octets dont la valeur
/// écrite diffère de la mémoire de l'invité.
#[derive(Default)]
pub struct Overlay {
    pub bytes: std::collections::HashMap<u32, u8>,
}

/// Traduction linéaire → physique par la table de pages de l'invité (pages
/// de 4 Ko, de 4 Mo avec CR4.PSE) ; `None` : faute de page, laissée au
/// processeur. Avec `mark`, pose les bits A (et D en écriture) comme lui.
pub(crate) fn translate(cpu: &Cpu, mem: &GuestMem, lin: u32, write: bool, mark: bool) -> Option<u32> {
    const PG: u32 = 1 << 31;
    const WP: u32 = 1 << 16;
    const PSE: u32 = 1 << 4;
    if cpu.cr0 & PG == 0 {
        return Some(lin);
    }
    let user = cpu.cpl() == 3;
    let entry = |a: u32| ((a as usize) + 4 <= mem.size()).then(|| mem.read_u32(a as usize));
    let allowed = |e: u32| e & 1 != 0 && (!user || e & 4 != 0) && (!write || e & 2 != 0 || (!user && cpu.cr0 & WP == 0));
    let pde_at = (cpu.cr3 & !0xFFF).wrapping_add((lin >> 22) << 2);
    let pde = entry(pde_at)?;
    if pde & 0x80 != 0 && cpu.cr4 & PSE != 0 {
        if !allowed(pde) {
            return None;
        }
        if mark {
            mem.write_u32(pde_at as usize, pde | 0x20 | if write { 0x40 } else { 0 });
        }
        return Some((pde & 0xFFC0_0000) | (lin & 0x003F_FFFF));
    }
    let pte_at = (pde & !0xFFF).wrapping_add(((lin >> 12) & 0x3FF) << 2);
    let pte = entry(pte_at)?;
    // Droits effectifs : ceux du répertoire et de la table à la fois.
    if pde & 1 == 0 || !allowed(pte) || !allowed(pde | 1) {
        return None;
    }
    if mark {
        mem.write_u32(pde_at as usize, pde | 0x20);
        mem.write_u32(pte_at as usize, pte | 0x20 | if write { 0x40 } else { 0 });
    }
    Some((pte & !0xFFF) | (lin & 0xFFF))
}

impl<B: Mmio> Exec<'_, B> {
    // --- Mémoire -------------------------------------------------------

    /// Adresse physique d'un octet déjà contrôlé par `lin` (pagination).
    fn phys(&self, addr: u32, write: bool) -> Option<u32> {
        translate(self.cpu, self.mem, addr, write, true)
    }

    fn read8(&mut self, addr: u32) -> u8 {
        let Some(addr) = self.phys(addr, false) else { return 0xFF };
        if self.mmio.claims(addr, false) {
            self.mmio_hits += 1;
            self.mmio.read8(addr)
        } else if let Some(&v) = self.dry.as_ref().and_then(|o| o.bytes.get(&addr)) {
            v
        } else if (addr as usize) < self.mem.size() {
            self.mem.read_u8(addr as usize)
        } else {
            0xFF
        }
    }

    fn write8(&mut self, addr: u32, v: u8) {
        let Some(addr) = self.phys(addr, true) else { return };
        if self.mmio.claims(addr, true) {
            self.mmio_hits += 1;
            self.mmio.write8(addr, v);
        } else if (addr as usize) < self.mem.size() {
            if let Some(o) = self.dry.as_deref_mut() {
                let cur = o.bytes.get(&addr).copied().unwrap_or_else(|| self.mem.read_u8(addr as usize));
                if cur != v {
                    o.bytes.insert(addr, v);
                }
            } else {
                self.mem.write(addr as usize, &[v]);
            }
        }
    }

    /// Adresse physique d'un accès de `size` octets (au plus 4) qui se fait
    /// d'un seul tenant en RAM : dans une page, hors des zones du bus (VGA,
    /// ROM), hors exécution à blanc. `None` : octet par octet.
    fn ram_phys(&self, addr: u32, size: u32, write: bool) -> Option<usize> {
        if self.dry.is_some() || (self.cpu.cr0 & (1 << 31) != 0 && (addr & 0xFFF) + size > 0x1000) {
            return None;
        }
        let p = self.phys(addr, write)?;
        let last = p.checked_add(size - 1)?;
        // Zones du bus d'au moins 4 octets : contrôler les deux bouts suffit.
        ((last as usize) < self.mem.size() && !self.mmio.claims(p, write) && !self.mmio.claims(last, write)).then_some(p as usize)
    }

    fn read_lin(&mut self, addr: u32, size: u32) -> u32 {
        if let Some(p) = self.ram_phys(addr, size, false) {
            let mut b = [0u8; 4];
            self.mem.read(p, &mut b[..size as usize]);
            return u32::from_le_bytes(b);
        }
        (0..size).fold(0, |acc, i| acc | u32::from(self.read8(addr.wrapping_add(i))) << (8 * i))
    }

    fn write_lin(&mut self, addr: u32, size: u32, v: u32) {
        if let Some(p) = self.ram_phys(addr, size, true) {
            self.mem.write(p, &v.to_le_bytes()[..size as usize]);
            return;
        }
        for i in 0..size {
            self.write8(addr.wrapping_add(i), (v >> (8 * i)) as u8);
        }
    }

    /// Adresse linéaire après contrôle du segment : limite, et en mode
    /// protégé, présence et droits d'accès du descripteur en cache.
    fn lin(&self, seg: R, off: u32, size: u32, write: bool) -> X<u32> {
        let s = self.cpu.seg[seg_idx(seg)];
        if self.cpu.pm() {
            let code = s.typ & 8 != 0;
            let ok = s.present
                && !s.unusable
                && s.s
                && if code {
                    !write && s.typ & 2 != 0
                } else {
                    // Segments expand-down : laissés au processeur.
                    s.typ & 4 == 0 && (!write || s.typ & 2 != 0)
                };
            if !ok {
                return Err(Bail);
            }
        }
        if u64::from(off) + u64::from(size) - 1 > u64::from(s.limit) {
            return Err(Bail);
        }
        let a = s.base.wrapping_add(off);
        // Pagination : premier et dernier octet présents et permis, avant
        // tout effet ; sinon la faute de page revient au processeur.
        if self.cpu.cr0 & (1 << 31) != 0 {
            let last = a.wrapping_add(size - 1);
            if translate(self.cpu, self.mem, a, write, false).is_none() || translate(self.cpu, self.mem, last, write, false).is_none() {
                return Err(Bail);
            }
        }
        Ok(a)
    }

    fn rd(&mut self, seg: R, off: u32, size: u32) -> X<u32> {
        let a = self.lin(seg, off, size, false)?;
        Ok(self.read_lin(a, size))
    }

    fn wr(&mut self, seg: R, off: u32, size: u32, v: u32) -> X {
        let a = self.lin(seg, off, size, true)?;
        self.write_lin(a, size, v);
        Ok(())
    }

    fn addr_mask(&self) -> u32 {
        if self.addr32 { u32::MAX } else { 0xFFFF }
    }

    fn ea(&self, ins: &Instruction) -> u32 {
        let mut ea = ins.memory_displacement32();
        if ins.memory_base() != R::None {
            ea = ea.wrapping_add(self.cpu.reg(ins.memory_base()));
        }
        if ins.memory_index() != R::None {
            ea = ea.wrapping_add(self.cpu.reg(ins.memory_index()).wrapping_mul(ins.memory_index_scale()));
        }
        ea & self.addr_mask()
    }

    // --- Opérandes -----------------------------------------------------

    fn loc(&self, ins: &Instruction, n: u32) -> X<Loc> {
        Ok(match ins.op_kind(n) {
            OpKind::Register => {
                // Registres entiers et de segment seulement (pas CRn, DRn, x87, SSE…).
                let r = ins.op_register(n);
                if !(r.is_gpr8() || r.is_gpr16() || r.is_gpr32() || r.is_segment_register()) {
                    return Err(Bail);
                }
                Loc::Reg(r)
            }
            OpKind::Memory => Loc::Mem { seg: ins.memory_segment(), off: self.ea(ins) },
            OpKind::Immediate8
            | OpKind::Immediate16
            | OpKind::Immediate32
            | OpKind::Immediate8to16
            | OpKind::Immediate8to32 => Loc::Imm(ins.immediate(n) as u32),
            _ => return Err(Bail),
        })
    }

    fn get(&mut self, l: Loc, size: u32) -> X<u32> {
        match l {
            Loc::Reg(r) => Ok(self.cpu.reg(r)),
            Loc::Mem { seg, off } => self.rd(seg, off, size),
            Loc::Imm(v) => Ok(v & mask(size)),
        }
    }

    fn check(&self, l: Loc, size: u32) -> X {
        if let Loc::Mem { seg, off } = l {
            self.lin(seg, off, size, true)?;
        }
        Ok(())
    }

    fn set(&mut self, l: Loc, size: u32, v: u32) -> X {
        match l {
            Loc::Reg(R::CS) => Err(Bail),
            Loc::Reg(r) => {
                self.cpu.set_reg(r, v);
                Ok(())
            }
            Loc::Mem { seg, off } => self.wr(seg, off, size, v),
            Loc::Imm(_) => Err(Bail),
        }
    }

    // --- Pile ----------------------------------------------------------

    /// Empile (valeur, taille) dans l'ordre ; tout est vérifié avant d'écrire.
    fn push_all(&mut self, vals: &[(u32, u32)]) -> X {
        let spm = self.cpu.sp_mask();
        let mut sp = self.cpu.sp();
        let mut slots = [(0u32, 0u32, 0u32); 8];
        for (i, &(v, size)) in vals.iter().enumerate() {
            sp = sp.wrapping_sub(size) & spm;
            slots[i] = (self.lin(R::SS, sp, size, true)?, size, v);
        }
        for &(a, size, v) in &slots[..vals.len()] {
            self.write_lin(a, size, v);
        }
        self.cpu.set_sp(sp);
        Ok(())
    }

    /// Lit `size` octets à SP + `offset` sans dépiler.
    fn peek(&mut self, offset: u32, size: u32) -> X<u32> {
        let off = self.cpu.sp().wrapping_add(offset) & self.cpu.sp_mask();
        self.rd(R::SS, off, size)
    }

    fn drop_stack(&mut self, n: u32) {
        let sp = self.cpu.sp().wrapping_add(n) & self.cpu.sp_mask();
        self.cpu.set_sp(sp);
    }

    fn jump(&mut self, ip: u32) -> X {
        if ip > self.cpu.seg[1].limit {
            return Err(Bail);
        }
        self.cpu.ip = ip;
        Ok(())
    }

    fn far_jump(&mut self, cs: u16, ip: u32) -> X {
        if ip > self.cpu.seg[1].limit {
            return Err(Bail);
        }
        self.cpu.set_sreg(1, cs);
        self.cpu.ip = ip;
        Ok(())
    }

    fn load_flags(&mut self, v: u32, size: u32) -> X {
        let m = if size == 2 { POPF16_MASK } else { POPF32_MASK };
        let new = ((self.cpu.flags & !m) | (v & m) | 2) & !RF;
        if new & TF != 0 {
            return Err(Bail);
        }
        self.cpu.flags = new;
        Ok(())
    }

    // --- Exécution -----------------------------------------------------

    pub fn step(&mut self, ins: &Instruction) -> X {
        let next = if self.cpu.code32() { ins.next_ip() as u32 } else { ins.next_ip() as u32 & 0xFFFF };
        let mn = ins.mnemonic();
        if self.cpu.pm() && !self.protected_mode_ok(ins) {
            return Err(Bail);
        }
        // V86 : INT passe par l'IDT du superviseur ; CLI, STI, PUSHF, POPF et
        // IRET sont sensibles à IOPL (le superviseur les intercepte, ou ne
        // laisse pas changer IOPL) : tous laissés au processeur.
        if self.cpu.v86()
            && matches!(
                mn,
                Mn::Int | Mn::Int3 | Mn::Into | Mn::Iret | Mn::Iretd | Mn::Cli | Mn::Sti | Mn::Pushf | Mn::Pushfd | Mn::Popf | Mn::Popfd
            )
        {
            return Err(Bail);
        }
        let size = match ins.op_kind(0) {
            OpKind::Register => ins.op_register(0).size() as u32,
            OpKind::Memory => ins.memory_size().size() as u32,
            _ => 0,
        };
        let size1 = || match ins.op_kind(1) {
            OpKind::Register => ins.op_register(1).size() as u32,
            OpKind::Memory => ins.memory_size().size() as u32,
            _ => size,
        };
        if ins.is_string_instruction() {
            return self.string(ins, next);
        }
        match mn {
            Mn::Nop => {}
            Mn::Mov => {
                let (d, s) = (self.loc(ins, 0)?, self.loc(ins, 1)?);
                let v = self.get(s, size1())?;
                self.set(d, size, v)?;
            }
            Mn::Movzx | Mn::Movsx => {
                let s = self.loc(ins, 1)?;
                let ss = size1();
                let v = self.get(s, ss)?;
                let v = if mn == Mn::Movsx { sext(v, ss) as u32 } else { v };
                self.cpu.set_reg(ins.op_register(0), v & mask(size));
            }
            Mn::Lea => {
                if ins.op_kind(1) != OpKind::Memory {
                    return Err(Bail);
                }
                let v = self.ea(ins);
                self.cpu.set_reg(ins.op_register(0), v & mask(size));
            }
            Mn::Xchg => {
                let (a, b) = (self.loc(ins, 0)?, self.loc(ins, 1)?);
                let va = self.get(a, size)?;
                let vb = self.get(b, size)?;
                self.check(a, size)?;
                self.check(b, size)?;
                self.set(a, size, vb)?;
                self.set(b, size, va)?;
            }
            Mn::Push => {
                let n = (-ins.stack_pointer_increment()) as u32;
                let l = self.loc(ins, 0)?;
                let v = match l {
                    Loc::Imm(v) => v & mask(n),
                    _ => {
                        let s = if ins.op_kind(0) == OpKind::Register { ins.op_register(0).size() as u32 } else { n };
                        self.get(l, s)?
                    }
                };
                self.push_all(&[(v, n)])?;
            }
            Mn::Pop => {
                let n = ins.stack_pointer_increment() as u32;
                if ins.op_kind(0) == OpKind::Register && ins.op_register(0) == R::CS {
                    return Err(Bail);
                }
                let v = self.peek(0, n)?;
                let old = self.cpu.gpr[4];
                self.drop_stack(n);
                // L'adresse d'un opérande mémoire se calcule après l'incrément de SP.
                let d = self.loc(ins, 0)?;
                let ds = if ins.op_kind(0) == OpKind::Register { ins.op_register(0).size() as u32 } else { n };
                if self.check(d, ds).is_err() {
                    self.cpu.gpr[4] = old;
                    return Err(Bail);
                }
                self.set(d, ds, v)?;
            }
            Mn::Pusha | Mn::Pushad => {
                let n = if mn == Mn::Pusha { 2 } else { 4 };
                let m = mask(n);
                let g = self.cpu.gpr;
                let sp = g[4];
                let vals = [g[0], g[1], g[2], g[3], sp, g[5], g[6], g[7]].map(|v| (v & m, n));
                self.push_all(&vals)?;
            }
            Mn::Popa | Mn::Popad => {
                let n = if mn == Mn::Popa { 2 } else { 4 };
                let mut v = [0u32; 8];
                for (i, slot) in v.iter_mut().enumerate() {
                    *slot = self.peek(i as u32 * n, n)?;
                }
                // Ordre de pile : DI, SI, BP, (SP), BX, DX, CX, AX.
                let regs: [R; 8] = if n == 2 {
                    [R::DI, R::SI, R::BP, R::None, R::BX, R::DX, R::CX, R::AX]
                } else {
                    [R::EDI, R::ESI, R::EBP, R::None, R::EBX, R::EDX, R::ECX, R::EAX]
                };
                for (r, val) in regs.into_iter().zip(v) {
                    if r != R::None {
                        self.cpu.set_reg(r, val);
                    }
                }
                self.drop_stack(8 * n);
            }
            Mn::Pushf | Mn::Pushfd => {
                let n = if mn == Mn::Pushf { 2 } else { 4 };
                let v = self.cpu.flags & if n == 2 { 0xFFFF } else { 0x00FC_FFFF };
                self.push_all(&[(v, n)])?;
                let sp = self.cpu.sp();
                self.flags_push = Some(self.cpu.seg[2].base.wrapping_add(sp));
            }
            Mn::Popf | Mn::Popfd => {
                let n = if mn == Mn::Popf { 2 } else { 4 };
                let v = self.peek(0, n)?;
                self.load_flags(v, n)?;
                self.drop_stack(n);
            }
            Mn::Lahf => {
                let v = (self.cpu.flags & 0xD5) | 2;
                self.cpu.set_reg(R::AH, v);
            }
            Mn::Sahf => {
                let ah = self.cpu.reg(R::AH);
                self.cpu.flags = (self.cpu.flags & !0xFF) | (ah & 0xD5) | 2;
            }
            Mn::Cbw => self.cpu.set_reg(R::AX, sext(self.cpu.reg(R::AL), 1) as u32),
            Mn::Cwde => self.cpu.set_reg(R::EAX, sext(self.cpu.reg(R::AX), 2) as u32),
            Mn::Cwd => self.cpu.set_reg(R::DX, if self.cpu.reg(R::AX) & 0x8000 != 0 { 0xFFFF } else { 0 }),
            Mn::Cdq => self.cpu.set_reg(R::EDX, if self.cpu.reg(R::EAX) & 0x8000_0000 != 0 { u32::MAX } else { 0 }),
            Mn::Xlatb => {
                let off = self.ea(ins);
                let v = self.rd(ins.memory_segment(), off, 1)?;
                self.cpu.set_reg(R::AL, v);
            }
            Mn::Bswap => {
                if size != 4 {
                    return Err(Bail);
                }
                let r = ins.op_register(0);
                self.cpu.set_reg(r, self.cpu.reg(r).swap_bytes());
            }
            Mn::Lds | Mn::Les | Mn::Lfs | Mn::Lgs | Mn::Lss => {
                let sreg = match mn {
                    Mn::Lds => R::DS,
                    Mn::Les => R::ES,
                    Mn::Lfs => R::FS,
                    Mn::Lgs => R::GS,
                    _ => R::SS,
                };
                let (seg, off) = (ins.memory_segment(), self.ea(ins));
                let v = self.rd(seg, off, size)?;
                let sel = self.rd(seg, off.wrapping_add(size) & self.addr_mask(), 2)?;
                self.cpu.set_reg(ins.op_register(0), v);
                self.cpu.set_sreg(seg_idx(sreg), sel as u16);
            }
            m if is_setcc(m) => {
                let d = self.loc(ins, 0)?;
                let v = u32::from(self.cpu.condition(ins.condition_code()));
                self.set(d, 1, v)?;
            }
            m if is_cmovcc(m) => {
                let s = self.loc(ins, 1)?;
                let v = self.get(s, size)?;
                if self.cpu.condition(ins.condition_code()) {
                    self.cpu.set_reg(ins.op_register(0), v);
                }
            }
            Mn::Add | Mn::Adc | Mn::Sub | Mn::Sbb | Mn::Cmp | Mn::And | Mn::Or | Mn::Xor | Mn::Test => {
                let (d, s) = (self.loc(ins, 0)?, self.loc(ins, 1)?);
                let a = self.get(d, size)?;
                let b = self.get(s, size)?;
                let carry = self.cpu.flags & CF;
                let (res, flags) = match mn {
                    Mn::Add => add(a, b, 0, size),
                    Mn::Adc => add(a, b, carry, size),
                    Mn::Sub | Mn::Cmp => sub(a, b, 0, size),
                    Mn::Sbb => sub(a, b, carry, size),
                    Mn::And | Mn::Test => logic(a & b, size),
                    Mn::Or => logic(a | b, size),
                    _ => logic(a ^ b, size),
                };
                if !matches!(mn, Mn::Cmp | Mn::Test) {
                    self.set(d, size, res)?;
                }
                self.cpu.set_flags(ARITH, flags);
            }
            Mn::Inc | Mn::Dec => {
                let d = self.loc(ins, 0)?;
                let a = self.get(d, size)?;
                let (res, flags) = if mn == Mn::Inc { add(a, 1, 0, size) } else { sub(a, 1, 0, size) };
                self.set(d, size, res)?;
                self.cpu.set_flags(ARITH & !CF, flags);
            }
            Mn::Neg => {
                let d = self.loc(ins, 0)?;
                let a = self.get(d, size)?;
                let (res, flags) = sub(0, a, 0, size);
                self.set(d, size, res)?;
                self.cpu.set_flags(ARITH, flags);
            }
            Mn::Not => {
                let d = self.loc(ins, 0)?;
                let a = self.get(d, size)?;
                self.set(d, size, !a & mask(size))?;
            }
            Mn::Mul | Mn::Imul if ins.op_count() == 1 => {
                let s = self.loc(ins, 0)?;
                let b = self.get(s, size)?;
                let (acc, hi) = match size {
                    1 => (R::AL, R::AH),
                    2 => (R::AX, R::DX),
                    _ => (R::EAX, R::EDX),
                };
                let a = self.cpu.reg(acc);
                let w = 8 * size;
                let (lo, high, overflow) = if mn == Mn::Mul {
                    let p = u64::from(a) * u64::from(b);
                    (p as u32 & mask(size), (p >> w) as u32 & mask(size), p >> w != 0)
                } else {
                    let p = sext(a, size) * sext(b, size);
                    let lo = p as u32 & mask(size);
                    (lo, (p >> w) as u32 & mask(size), sext(lo, size) != p)
                };
                if size == 1 {
                    self.cpu.set_reg(R::AX, lo | (high << 8));
                } else {
                    self.cpu.set_reg(acc, lo);
                    self.cpu.set_reg(hi, high);
                }
                let of = if overflow { CF | OF } else { 0 };
                self.cpu.set_flags(ARITH, szp(lo, size) | of);
            }
            Mn::Imul => {
                let (a, b) = if ins.op_count() == 3 {
                    let s = self.loc(ins, 1)?;
                    (self.get(s, size)?, ins.immediate(2) as u32)
                } else {
                    let s = self.loc(ins, 1)?;
                    (self.cpu.reg(ins.op_register(0)), self.get(s, size)?)
                };
                let p = sext(a, size) * sext(b & mask(size), size);
                let lo = p as u32 & mask(size);
                self.cpu.set_reg(ins.op_register(0), lo);
                let of = if sext(lo, size) != p { CF | OF } else { 0 };
                self.cpu.set_flags(ARITH, szp(lo, size) | of);
            }
            Mn::Div | Mn::Idiv => {
                let s = self.loc(ins, 0)?;
                let d = self.get(s, size)?;
                if d == 0 {
                    return Err(Bail);
                }
                let w = 8 * size;
                let n: u64 = match size {
                    1 => u64::from(self.cpu.reg(R::AX)),
                    2 => (u64::from(self.cpu.reg(R::DX)) << 16) | u64::from(self.cpu.reg(R::AX)),
                    _ => (u64::from(self.cpu.reg(R::EDX)) << 32) | u64::from(self.cpu.reg(R::EAX)),
                };
                let (q, r) = if mn == Mn::Div {
                    let q = n / u64::from(d);
                    if q > u64::from(mask(size)) {
                        return Err(Bail);
                    }
                    (q as u32, (n % u64::from(d)) as u32)
                } else {
                    let nn: i64 = match size {
                        1 => i64::from(n as u16 as i16),
                        2 => i64::from(n as u32 as i32),
                        _ => n as i64,
                    };
                    let dd = sext(d, size);
                    let Some(q) = nn.checked_div(dd) else { return Err(Bail) };
                    let lim = 1i64 << (w - 1);
                    if q < -lim || q >= lim {
                        return Err(Bail);
                    }
                    (q as u32 & mask(size), (nn % dd) as u32 & mask(size))
                };
                match size {
                    1 => self.cpu.set_reg(R::AX, q | (r << 8)),
                    2 => {
                        self.cpu.set_reg(R::AX, q);
                        self.cpu.set_reg(R::DX, r);
                    }
                    _ => {
                        self.cpu.set_reg(R::EAX, q);
                        self.cpu.set_reg(R::EDX, r);
                    }
                }
            }
            Mn::Daa | Mn::Das => {
                let al = self.cpu.reg(R::AL);
                let cf = self.cpu.flag(CF);
                let mut v = al;
                let mut f = 0;
                if al & 0x0F > 9 || self.cpu.flag(AF) {
                    let (r, borrow) = if mn == Mn::Daa {
                        (al + 6, al + 6 > 0xFF)
                    } else {
                        (al.wrapping_sub(6), al < 6)
                    };
                    v = r & 0xFF;
                    f |= AF;
                    if cf || borrow {
                        f |= CF;
                    }
                }
                if al > 0x99 || cf {
                    v = if mn == Mn::Daa { v.wrapping_add(0x60) } else { v.wrapping_sub(0x60) } & 0xFF;
                    f |= CF;
                }
                self.cpu.set_reg(R::AL, v);
                self.cpu.set_flags(ARITH & !OF, f | szp(v, 1));
            }
            Mn::Aaa | Mn::Aas => {
                let ax = self.cpu.reg(R::AX);
                let f = if ax & 0x0F > 9 || self.cpu.flag(AF) {
                    let v = if mn == Mn::Aaa { ax.wrapping_add(0x106) } else { ax.wrapping_sub(6).wrapping_sub(0x100) };
                    self.cpu.set_reg(R::AX, v & 0xFF0F);
                    AF | CF
                } else {
                    self.cpu.set_reg(R::AX, ax & 0xFF0F);
                    0
                };
                self.cpu.set_flags(AF | CF, f);
            }
            Mn::Aam => {
                let base = ins.immediate8() as u32;
                if base == 0 {
                    return Err(Bail);
                }
                let al = self.cpu.reg(R::AL);
                let v = al % base;
                self.cpu.set_reg(R::AX, ((al / base) << 8) | v);
                self.cpu.set_flags(ARITH, szp(v, 1));
            }
            Mn::Aad => {
                let base = ins.immediate8() as u32;
                let v = (self.cpu.reg(R::AL) + self.cpu.reg(R::AH) * base) & 0xFF;
                self.cpu.set_reg(R::AX, v);
                self.cpu.set_flags(ARITH, szp(v, 1));
            }
            Mn::Salc => self.cpu.set_reg(R::AL, if self.cpu.flag(CF) { 0xFF } else { 0 }),
            Mn::Rol | Mn::Ror | Mn::Rcl | Mn::Rcr | Mn::Shl | Mn::Sal | Mn::Shr | Mn::Sar => {
                let op = match mn {
                    Mn::Rol => Shift::Rol,
                    Mn::Ror => Shift::Ror,
                    Mn::Rcl => Shift::Rcl,
                    Mn::Rcr => Shift::Rcr,
                    Mn::Shr => Shift::Shr,
                    Mn::Sar => Shift::Sar,
                    _ => Shift::Shl,
                };
                let d = self.loc(ins, 0)?;
                let c = self.loc(ins, 1)?;
                let count = self.get(c, 1)? & 0x1F;
                let a = self.get(d, size)?;
                // Lecture-modification-écriture : contrôle d'écriture même si compte nul.
                self.check(d, size)?;
                if count != 0 {
                    let (res, flags, touched) = shift(op, a, count, self.cpu.flag(CF), size);
                    self.set(d, size, res)?;
                    self.cpu.set_flags(touched, flags);
                }
            }
            Mn::Shld | Mn::Shrd => {
                let d = self.loc(ins, 0)?;
                let src = self.cpu.reg(ins.op_register(1));
                let c = self.loc(ins, 2)?;
                let count = self.get(c, 1)? & 0x1F;
                let a = self.get(d, size)?;
                self.check(d, size)?;
                let w = 8 * size;
                if count > w {
                    return Err(Bail);
                }
                if count != 0 {
                    let (res, cf) = if mn == Mn::Shld {
                        let v = (u64::from(a) << w) | u64::from(src);
                        ((v << count >> w) as u32 & mask(size), (a >> (w - count)) & 1)
                    } else {
                        let v = (u64::from(src) << w) | u64::from(a);
                        ((v >> count) as u32 & mask(size), (a >> (count - 1)) & 1)
                    };
                    self.set(d, size, res)?;
                    let of = (res ^ a) & sign(size) != 0;
                    let f = szp(res, size) | if cf != 0 { CF } else { 0 } | if of { OF } else { 0 };
                    self.cpu.set_flags(ARITH, f);
                }
            }
            Mn::Bt | Mn::Bts | Mn::Btr | Mn::Btc => {
                let bits = 8 * size;
                let c = self.loc(ins, 1)?;
                let raw = self.get(c, size)?;
                let (d, bit) = match (self.loc(ins, 0)?, c) {
                    (Loc::Mem { seg, off }, Loc::Reg(_)) => {
                        let s = sext(raw, size);
                        let delta = (s >> bits.trailing_zeros()) * i64::from(size);
                        let off = (i64::from(off) + delta) as u32 & self.addr_mask();
                        (Loc::Mem { seg, off }, (s & i64::from(bits - 1)) as u32)
                    }
                    (d, _) => (d, raw & (bits - 1)),
                };
                let v = self.get(d, size)?;
                let old = (v >> bit) & 1;
                let new = match mn {
                    Mn::Bts => v | (1 << bit),
                    Mn::Btr => v & !(1 << bit),
                    Mn::Btc => v ^ (1 << bit),
                    _ => v,
                };
                if mn != Mn::Bt {
                    self.set(d, size, new)?;
                }
                self.cpu.set_flags(CF, old);
            }
            Mn::Bsf | Mn::Bsr => {
                let s = self.loc(ins, 1)?;
                let v = self.get(s, size)?;
                if v == 0 {
                    self.cpu.set_flags(ZF, ZF);
                } else {
                    let idx = if mn == Mn::Bsf { v.trailing_zeros() } else { 31 - v.leading_zeros() };
                    self.cpu.set_reg(ins.op_register(0), idx);
                    self.cpu.set_flags(ZF, 0);
                }
            }
            Mn::Cmpxchg => {
                let d = self.loc(ins, 0)?;
                let acc = match size {
                    1 => R::AL,
                    2 => R::AX,
                    _ => R::EAX,
                };
                let v = self.get(d, size)?;
                let a = self.cpu.reg(acc);
                let (_, flags) = sub(a, v, 0, size);
                if a == v {
                    let src = self.cpu.reg(ins.op_register(1));
                    self.set(d, size, src)?;
                } else {
                    self.check(d, size)?;
                    self.cpu.set_reg(acc, v);
                }
                self.cpu.set_flags(ARITH, flags);
            }
            Mn::Xadd => {
                let d = self.loc(ins, 0)?;
                let r = ins.op_register(1);
                let a = self.get(d, size)?;
                let b = self.cpu.reg(r);
                let (res, flags) = add(a, b, 0, size);
                self.check(d, size)?;
                self.cpu.set_reg(r, a);
                self.set(d, size, res)?;
                self.cpu.set_flags(ARITH, flags);
            }
            Mn::Clc => self.cpu.set_flags(CF, 0),
            Mn::Stc => self.cpu.set_flags(CF, CF),
            Mn::Cmc => self.cpu.flags ^= CF,
            Mn::Cld => self.cpu.set_flags(DF, 0),
            Mn::Std => self.cpu.set_flags(DF, DF),
            Mn::Cli => self.cpu.set_flags(IF, 0),
            Mn::Sti => self.cpu.set_flags(IF, IF),
            Mn::Jmp | Mn::Call => return self.branch(ins, next),
            Mn::Ret | Mn::Retf => {
                let (n, far) = match ins.code() {
                    Code::Retnw | Code::Retnw_imm16 => (2, false),
                    Code::Retnd | Code::Retnd_imm16 => (4, false),
                    Code::Retfw | Code::Retfw_imm16 => (2, true),
                    Code::Retfd | Code::Retfd_imm16 => (4, true),
                    _ => return Err(Bail),
                };
                let extra = if ins.op_count() == 1 { ins.immediate16() as u32 } else { 0 };
                let ip = self.peek(0, n)?;
                if far {
                    let cs = self.peek(n, 2)?;
                    self.far_jump(cs as u16, ip)?;
                    self.drop_stack(2 * n + extra);
                } else {
                    self.jump(ip)?;
                    self.drop_stack(n + extra);
                }
                return Ok(());
            }
            Mn::Iret | Mn::Iretd => {
                let n = if mn == Mn::Iret { 2 } else { 4 };
                let ip = self.peek(0, n)?;
                let cs = self.peek(n, 2)?;
                let fl = self.peek(2 * n, n)?;
                if ip > self.cpu.seg[1].limit {
                    return Err(Bail);
                }
                self.load_flags(fl, n)?;
                self.far_jump(cs as u16, ip)?;
                self.drop_stack(3 * n);
                return Ok(());
            }
            Mn::Int | Mn::Int3 | Mn::Into => {
                let vector = match mn {
                    Mn::Int => u32::from(ins.immediate8()),
                    Mn::Int3 => 3,
                    _ if self.cpu.flag(OF) => 4,
                    _ => {
                        self.cpu.ip = next;
                        return Ok(());
                    }
                };
                let entry = self.cpu.idt_base.wrapping_add(vector * 4);
                let ip = self.read_lin(entry, 2);
                let cs = self.read_lin(entry + 2, 2);
                let flags = self.cpu.flags & 0xFFFF;
                let old_cs = u32::from(self.cpu.seg[1].selector);
                self.push_all(&[(flags, 2), (old_cs, 2), (next, 2)])?;
                self.cpu.flags &= !(IF | TF | AC);
                self.far_jump(cs as u16, ip)?;
                return Ok(());
            }
            Mn::Enter => {
                if ins.immediate8_2nd() & 0x1F != 0 {
                    return Err(Bail);
                }
                let n = if ins.code() == Code::Enterw_imm16_imm8 { 2 } else { 4 };
                let bp = if n == 2 { R::BP } else { R::EBP };
                // Le nouveau sommet de pile doit tenir dans la limite (sinon #SS).
                let spm = self.cpu.sp_mask();
                let top = self.cpu.sp().wrapping_sub(n) & spm;
                let new_sp = top.wrapping_sub(u32::from(ins.immediate16())) & spm;
                self.lin(R::SS, new_sp, 1, true)?;
                self.push_all(&[(self.cpu.reg(bp), n)])?;
                // Constaté sur le processeur : en 32 bits, EBP reçoit l'ESP entier,
                // même avec une pile 16 bits.
                let frame = if n == 4 { self.cpu.gpr[4] } else { self.cpu.sp() };
                self.cpu.set_reg(bp, frame);
                self.cpu.set_sp(frame.wrapping_sub(u32::from(ins.immediate16())) & self.cpu.sp_mask());
            }
            Mn::Leave => {
                let n = if ins.code() == Code::Leavew { 2 } else { 4 };
                let saved = self.cpu.gpr[4];
                let ebp = self.cpu.gpr[5];
                self.cpu.set_sp(ebp);
                match self.peek(0, n) {
                    Ok(v) => {
                        self.cpu.set_reg(if n == 2 { R::BP } else { R::EBP }, v);
                        self.drop_stack(n);
                    }
                    Err(e) => {
                        self.cpu.gpr[4] = saved;
                        return Err(e);
                    }
                }
            }
            _ if ins.is_jcc_short_or_near() => {
                if self.cpu.condition(ins.condition_code()) {
                    return self.jump(ins.near_branch_target() as u32);
                }
            }
            // LOOPE/LOOPNE préfixés F2/F3 : le processeur ignore alors la condition
            // documentée (constaté) ; on le laisse faire.
            _ if ins.is_loopcc() && (ins.has_rep_prefix() || ins.has_repne_prefix()) => return Err(Bail),
            _ if ins.is_loop() || ins.is_loopcc() || ins.is_jcx_short() => {
                let cx = if self.addr32 { R::ECX } else { R::CX };
                let taken = if ins.is_jcx_short() {
                    let r = if matches!(mn, Mn::Jecxz) { R::ECX } else { R::CX };
                    self.cpu.reg(r) == 0
                } else {
                    let count = self.cpu.reg(cx).wrapping_sub(1) & mask(cx.size() as u32);
                    let z = self.cpu.flag(ZF);
                    let cond = match mn {
                        Mn::Loope => z,
                        Mn::Loopne => !z,
                        _ => true,
                    };
                    let target = ins.near_branch_target() as u32;
                    if count != 0 && cond && target > self.cpu.seg[1].limit {
                        return Err(Bail);
                    }
                    self.cpu.set_reg(cx, count);
                    count != 0 && cond
                };
                if taken {
                    return self.jump(ins.near_branch_target() as u32);
                }
            }
            Mn::In | Mn::Out => self.port_io(ins)?,
            _ => return Err(Bail),
        }
        self.cpu.ip = next;
        Ok(())
    }

    /// IN/OUT sur un port servi par l'hôte pendant la rafale, si le
    /// programme y a droit (sinon le processeur lèvera la faute).
    fn port_io(&mut self, ins: &Instruction) -> X {
        let (acc_op, port_op) = if ins.mnemonic() == Mn::In { (0, 1) } else { (1, 0) };
        let port = match ins.op_kind(port_op) {
            OpKind::Immediate8 => u16::from(ins.immediate8()),
            _ => self.cpu.reg(R::DX) as u16,
        };
        let size = ins.op_register(acc_op).size() as u8;
        if !self.mmio.port_claims(port) || !self.io_allowed(port, size) {
            return Err(Bail);
        }
        let acc = self.loc(ins, acc_op)?;
        if ins.mnemonic() == Mn::In {
            let v = self.mmio.port_in(port, size);
            self.set(acc, u32::from(size), v)?;
        } else {
            let v = self.get(acc, u32::from(size))?;
            self.mmio.port_out(port, size, v);
        }
        self.mmio_hits += 1;
        Ok(())
    }

    /// Droit d'accès aux ports : toujours en mode réel, si CPL ≤ IOPL en mode
    /// protégé ; sinon (et toujours en V86) d'après la bitmap du TSS.
    fn io_allowed(&self, port: u16, size: u8) -> bool {
        if !self.cpu.protected() {
            return true;
        }
        let iopl = ((self.cpu.flags >> 12) & 3) as u8;
        if !self.cpu.v86() && self.cpu.cpl() <= iopl {
            return true;
        }
        let byte = |off: u32| -> Option<u8> {
            if off > self.cpu.tr_limit {
                return None;
            }
            let p = translate(self.cpu, self.mem, self.cpu.tr_base.wrapping_add(off), false, false)? as usize;
            (p < self.mem.size()).then(|| self.mem.read_u8(p))
        };
        let Some(map) = byte(0x66).zip(byte(0x67)).map(|(lo, hi)| u32::from(u16::from_le_bytes([lo, hi]))) else { return false };
        (0..u32::from(size)).all(|i| {
            let p = u32::from(port) + i;
            byte(map + (p >> 3)).is_some_and(|b| b & (1 << (p & 7)) == 0)
        })
    }

    /// Mode protégé : chargements de segment, transferts lointains,
    /// interruptions et instructions sensibles au privilège sont laissés au
    /// processeur (descripteurs, portes, contrôles de privilège).
    fn protected_mode_ok(&self, ins: &Instruction) -> bool {
        let writes_sreg = ins.op_count() > 0
            && ins.op_kind(0) == OpKind::Register
            && ins.op_register(0).is_segment_register()
            && matches!(ins.mnemonic(), Mn::Mov | Mn::Pop);
        let far = matches!(
            ins.op_kind(0),
            OpKind::FarBranch16 | OpKind::FarBranch32
        ) || matches!(
            ins.code(),
            Code::Jmp_m1616 | Code::Jmp_m1632 | Code::Call_m1616 | Code::Call_m1632
        );
        let cpl = self.cpu.cpl();
        let iopl = ((self.cpu.flags >> 12) & 3) as u8;
        let privileged = match ins.mnemonic() {
            Mn::Cli | Mn::Sti => cpl > iopl,
            Mn::Popf | Mn::Popfd => cpl != 0,
            _ => false,
        };
        !(writes_sreg
            || far
            || privileged
            || matches!(
                ins.mnemonic(),
                Mn::Int | Mn::Int3 | Mn::Into | Mn::Iret | Mn::Iretd | Mn::Retf | Mn::Lds | Mn::Les | Mn::Lfs | Mn::Lgs | Mn::Lss
            ))
    }

    fn branch(&mut self, ins: &Instruction, next: u32) -> X {
        let call = ins.mnemonic() == Mn::Call;
        let (far, target_cs, target_ip) = match ins.op_kind(0) {
            OpKind::NearBranch16 | OpKind::NearBranch32 => (false, 0, ins.near_branch_target() as u32),
            OpKind::FarBranch16 => (true, ins.far_branch_selector(), u32::from(ins.far_branch16())),
            OpKind::FarBranch32 => (true, ins.far_branch_selector(), ins.far_branch32()),
            OpKind::Register => (false, 0, self.cpu.reg(ins.op_register(0))),
            OpKind::Memory => {
                let (seg, off) = (ins.memory_segment(), self.ea(ins));
                match ins.code() {
                    Code::Jmp_m1616 | Code::Call_m1616 => {
                        let ip = self.rd(seg, off, 2)?;
                        (true, self.rd(seg, off.wrapping_add(2) & self.addr_mask(), 2)? as u16, ip)
                    }
                    Code::Jmp_m1632 | Code::Call_m1632 => {
                        let ip = self.rd(seg, off, 4)?;
                        (true, self.rd(seg, off.wrapping_add(4) & self.addr_mask(), 2)? as u16, ip)
                    }
                    Code::Jmp_rm16 | Code::Call_rm16 => (false, 0, self.rd(seg, off, 2)?),
                    Code::Jmp_rm32 | Code::Call_rm32 => (false, 0, self.rd(seg, off, 4)?),
                    _ => return Err(Bail),
                }
            }
            _ => return Err(Bail),
        };
        if target_ip > self.cpu.seg[1].limit {
            return Err(Bail);
        }
        if call {
            let spi = (-ins.stack_pointer_increment()) as u32;
            if far {
                let n = spi / 2;
                let cs = u32::from(self.cpu.seg[1].selector);
                self.push_all(&[(cs, n), (next, n)])?;
            } else {
                self.push_all(&[(next, spi)])?;
            }
        }
        if far { self.far_jump(target_cs, target_ip) } else { self.jump(target_ip) }
    }

    fn string(&mut self, ins: &Instruction, next: u32) -> X {
        let mn = ins.mnemonic();
        let size = ins.memory_size().size() as u32;
        let port = self.cpu.reg(R::DX) as u16;
        // INS/OUTS : seulement sur un port servi en rafale, avec le droit d'accès.
        if matches!(mn, Mn::Insb | Mn::Insw | Mn::Insd | Mn::Outsb | Mn::Outsw | Mn::Outsd)
            && (!self.mmio.port_claims(port) || !self.io_allowed(port, size as u8))
        {
            return Err(Bail);
        }
        let (si, di, cx) = if self.addr32 { (R::ESI, R::EDI, R::ECX) } else { (R::SI, R::DI, R::CX) };
        let src = if ins.segment_prefix() != R::None { ins.segment_prefix() } else { R::DS };
        let acc = match size {
            1 => R::AL,
            2 => R::AX,
            _ => R::EAX,
        };
        let rep = ins.has_rep_prefix() || ins.has_repne_prefix();
        let cmp = matches!(mn, Mn::Cmpsb | Mn::Cmpsw | Mn::Cmpsd | Mn::Scasb | Mn::Scasw | Mn::Scasd);
        let delta = if self.cpu.flag(DF) { size.wrapping_neg() } else { size };
        loop {
            if rep {
                if self.cpu.reg(cx) == 0 {
                    break;
                }
                if self.rep_budget == 0 {
                    return Ok(());
                }
            }
            let (s, d) = (self.cpu.reg(si), self.cpu.reg(di));
            match mn {
                Mn::Movsb | Mn::Movsw | Mn::Movsd => {
                    self.lin(R::ES, d, size, true)?;
                    let v = self.rd(src, s, size)?;
                    self.wr(R::ES, d, size, v)?;
                    self.cpu.set_reg(si, s.wrapping_add(delta));
                    self.cpu.set_reg(di, d.wrapping_add(delta));
                }
                Mn::Stosb | Mn::Stosw | Mn::Stosd => {
                    let v = self.cpu.reg(acc);
                    self.wr(R::ES, d, size, v)?;
                    self.cpu.set_reg(di, d.wrapping_add(delta));
                }
                Mn::Lodsb | Mn::Lodsw | Mn::Lodsd => {
                    let v = self.rd(src, s, size)?;
                    self.cpu.set_reg(acc, v);
                    self.cpu.set_reg(si, s.wrapping_add(delta));
                }
                Mn::Cmpsb | Mn::Cmpsw | Mn::Cmpsd => {
                    self.lin(R::ES, d, size, false)?;
                    let a = self.rd(src, s, size)?;
                    let b = self.rd(R::ES, d, size)?;
                    self.cpu.set_flags(ARITH, sub(a, b, 0, size).1);
                    self.cpu.set_reg(si, s.wrapping_add(delta));
                    self.cpu.set_reg(di, d.wrapping_add(delta));
                }
                Mn::Scasb | Mn::Scasw | Mn::Scasd => {
                    let b = self.rd(R::ES, d, size)?;
                    let a = self.cpu.reg(acc);
                    self.cpu.set_flags(ARITH, sub(a, b, 0, size).1);
                    self.cpu.set_reg(di, d.wrapping_add(delta));
                }
                Mn::Insb | Mn::Insw | Mn::Insd => {
                    // Destination vérifiée avant la lecture du port (effet de bord).
                    self.lin(R::ES, d, size, true)?;
                    let v = self.mmio.port_in(port, size as u8);
                    self.wr(R::ES, d, size, v)?;
                    self.cpu.set_reg(di, d.wrapping_add(delta));
                    self.mmio_hits += 1;
                }
                Mn::Outsb | Mn::Outsw | Mn::Outsd => {
                    let v = self.rd(src, s, size)?;
                    self.mmio.port_out(port, size as u8, v);
                    self.cpu.set_reg(si, s.wrapping_add(delta));
                    self.mmio_hits += 1;
                }
                _ => return Err(Bail),
            }
            if !rep {
                break;
            }
            let count = self.cpu.reg(cx).wrapping_sub(1);
            self.cpu.set_reg(cx, count);
            self.rep_budget -= 1;
            let z = self.cpu.flag(ZF);
            if count & mask(cx.size() as u32) == 0
                || (cmp && ((ins.has_rep_prefix() && !z) || (ins.has_repne_prefix() && z)))
            {
                break;
            }
        }
        self.cpu.ip = next;
        Ok(())
    }
}
