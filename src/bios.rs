// SPDX-License-Identifier: GPL-3.0-or-later
//! BIOS en HLE (étape 2) : zone de données (BDA), vidéo texte (INT 10h),
//! configuration (11h, 12h, 15h), disque minimal (13h), clavier (16h),
//! horloge (1Ah).
//!
//! Comme sur un vrai PC, l'état vit dans la BDA (0040:0000) : un programme
//! qui la lit ou l'écrit directement reste cohérent avec les services.

use std::collections::BTreeSet;
use std::ops::Range;
use std::time::Instant;

use crate::backend::Result;
use crate::hle::{AX, BP, BX, CF, CX, DI, DX, ES, Flow, Regs, SI, ZF, lin, set_ret_flag};
use crate::host::{Host, InputEvent, TextSink};
use crate::interp::Cpu;
use crate::keyboard;
use crate::mem::GuestMem;

/// Segment de la ROM BIOS ; les stubs d'interruption sont en F000:n*16.
pub const ROM_SEG: u16 = 0xF000;
pub const ROM: Range<usize> = 0xF_0000..0x10_0000;
/// ROM BIOS assemblée par build.rs (asm/bios.S).
const BIOS_IMAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.bin"));
/// Table de configuration (INT 15h AH=C0h), dans asm/bios.S.
const CONFIG_TABLE: u16 = 0xE6F5;
/// Table DBCS vide (INT 21h AX=6300h), dans asm/bios.S.
pub const DBCS_TABLE: u16 = 0xE700;
/// Routine de conversion de casse des informations nationales (RETF).
pub const CASE_MAP: u16 = 0xE710;

/// Adresses dans la BDA.
mod bda {
    pub const EQUIPMENT: usize = 0x410;
    pub const MEM_KB: usize = 0x413;
    pub const SHIFT: usize = 0x417;
    pub const KB_HEAD: usize = 0x41A;
    pub const KB_TAIL: usize = 0x41C;
    pub const VIDEO_MODE: usize = 0x449;
    pub const COLS: usize = 0x44A;
    pub const PAGE_SIZE: usize = 0x44C;
    pub const PAGE_START: usize = 0x44E;
    /// Position du curseur des pages 0 à 7 (ligne << 8 | colonne).
    pub const CURSOR: usize = 0x450;
    pub const CURSOR_SHAPE: usize = 0x460;
    pub const PAGE: usize = 0x462;
    pub const CRTC: usize = 0x463;
    pub const MODE_CTRL: usize = 0x465;
    pub const PALETTE: usize = 0x466;
    pub const TICKS: usize = 0x46C;
    pub const MIDNIGHT: usize = 0x470;
    pub const KB_START: usize = 0x480;
    pub const KB_END: usize = 0x482;
    pub const ROWS: usize = 0x484;
    pub const CHAR_HEIGHT: usize = 0x485;
    pub const EGA_MISC: usize = 0x487;
    pub const EGA_SWITCHES: usize = 0x488;
    pub const VGA_FLAGS: usize = 0x489;
    pub const DCC: usize = 0x48A;
}

/// Équipement (INT 11h) : coprocesseur, 80×25 couleur, pas de lecteur.
const EQUIPMENT: u16 = 0x0022;
/// Tics par jour à 1 193 182 / 65 536 Hz.
const TICKS_PER_DAY: u64 = 0x1800B0;
const PIT_HZ: u64 = 1_193_182;

/// ROM BIOS en F0000 et IVT : le vecteur n pointe sur F000:n*16.
pub fn install_rom(mem: &GuestMem) {
    mem.write(ROM.start, BIOS_IMAGE);
    for n in 0..256u16 {
        mem.write_u16(usize::from(n) * 4, n * 16);
        mem.write_u16(usize::from(n) * 4 + 2, ROM_SEG);
    }
}

pub struct Bios {
    extended_kb: usize,
    /// Origine de l'horloge hôte pour le compteur de tics.
    epoch: Instant,
    /// Tics depuis le minuit du démarrage, à `epoch` (ajusté par 1Ah/01h).
    base: i64,
    /// Jours écoulés déjà signalés par le drapeau de minuit.
    day: i64,
    /// Modes vidéo graphiques déjà signalés.
    warned_modes: BTreeSet<u8>,
}

impl Bios {
    pub fn new(extended_kb: usize) -> Self {
        let t = local_time();
        let secs = (t.tm_hour * 3600 + t.tm_min * 60 + t.tm_sec) as u64;
        let base = (secs * PIT_HZ / 65536) as i64;
        Self { extended_kb, epoch: Instant::now(), base, day: 0, warned_modes: BTreeSet::new() }
    }

    /// ROM, IVT, BDA, écran texte effacé.
    pub fn install(&mut self, mem: &GuestMem) {
        install_rom(mem);
        mem.fill(0x400, 0x100, 0);
        mem.write_u16(bda::EQUIPMENT, EQUIPMENT);
        mem.write_u16(bda::MEM_KB, 640);
        mem.write_u16(bda::KB_START, 0x1E);
        mem.write_u16(bda::KB_END, 0x3E);
        mem.write_u16(bda::KB_HEAD, 0x1E);
        mem.write_u16(bda::KB_TAIL, 0x1E);
        mem.write_u8(bda::EGA_MISC, 0x60);
        mem.write_u8(bda::EGA_SWITCHES, 0x09);
        mem.write_u8(bda::VGA_FLAGS, 0x11);
        mem.write_u8(bda::DCC, 0x0B);
        self.set_mode(mem, 3);
        self.sync_ticks(mem);
    }

    /// Met à jour le compteur de tics de la BDA d'après l'horloge hôte.
    /// Provisoire : à l'étape 4, l'IRQ 0 injectée (INT 08h) le fera avancer.
    pub fn sync_ticks(&mut self, mem: &GuestMem) {
        let t = self.now_ticks();
        let day = t.div_euclid(TICKS_PER_DAY as i64);
        if day > self.day {
            self.day = day;
            mem.write_u8(bda::MIDNIGHT, 1);
        }
        mem.write_u32(bda::TICKS, t.rem_euclid(TICKS_PER_DAY as i64) as u32);
    }

    fn now_ticks(&self) -> i64 {
        let ns = self.epoch.elapsed().as_nanos();
        self.base + (ns * u128::from(PIT_HZ) / (65536 * 1_000_000_000)) as i64
    }

    /// Sert l'interruption `vector` ; `Unhandled` si elle n'est pas du BIOS.
    pub fn call(&mut self, vector: u8, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Flow> {
        let ah = cpu.hi(AX);
        Ok(match vector {
            0x10 => self.int10(cpu, mem, host.text.as_mut()),
            0x11 => {
                cpu.set_w(AX, mem.read_u16(bda::EQUIPMENT));
                Flow::Continue
            }
            0x12 => {
                cpu.set_w(AX, mem.read_u16(bda::MEM_KB));
                Flow::Continue
            }
            0x13 => int13(cpu, mem),
            0x15 => self.int15(cpu, mem),
            0x16 => int16(cpu, mem, host)?,
            0x1A => self.int1a(cpu, mem),
            // Impression d'écran, Ctrl-Break, tic utilisateur : rien à faire.
            0x05 | 0x1B | 0x1C => Flow::Continue,
            _ => Flow::Unhandled(ah),
        })
    }

    // ---- Vidéo ----

    fn set_mode(&mut self, mem: &GuestMem, al: u8) -> Flow {
        let mode = al & 0x7F;
        let (cols, rows): (u16, u8) = match mode {
            0 | 1 | 4 | 5 | 0x0D | 0x13 => (40, 25),
            2 | 3 | 6 | 7 | 0x0E | 0x0F | 0x10 => (80, 25),
            0x11 | 0x12 => (80, 30),
            _ => return Flow::Unhandled(0x00),
        };
        let text = is_text(mode);
        if !text && self.warned_modes.insert(mode) {
            eprintln!("[bios] mode vidéo {mode:02X}h : affichage à l'étape 5");
        }
        mem.write_u8(bda::VIDEO_MODE, mode);
        mem.write_u16(bda::COLS, cols);
        mem.write_u8(bda::ROWS, rows - 1);
        let page_size = if text { cols * u16::from(rows) * 2 } else { 0x4000 };
        mem.write_u16(bda::PAGE_SIZE, page_size.next_power_of_two());
        mem.write_u16(bda::PAGE_START, 0);
        mem.fill(bda::CURSOR, 16, 0);
        mem.write_u8(bda::PAGE, 0);
        mem.write_u16(bda::CURSOR_SHAPE, 0x0607);
        mem.write_u16(bda::CRTC, if mode == 7 { 0x3B4 } else { 0x3D4 });
        mem.write_u8(bda::MODE_CTRL, if text { 0x29 } else { 0x1E });
        mem.write_u8(bda::PALETTE, 0x30);
        mem.write_u8(bda::CHAR_HEIGHT, if text || mode >= 0x0F { 16 } else { 8 });
        let equip = mem.read_u16(bda::EQUIPMENT) & !0x30;
        mem.write_u16(bda::EQUIPMENT, equip | if mode == 7 { 0x30 } else { 0x20 });
        if text && al & 0x80 == 0 {
            let base = Screen::of(mem).base;
            for i in 0..0x4000 {
                mem.write_u16(base + i * 2, 0x0720);
            }
        }
        Flow::Continue
    }

    fn int10(&mut self, cpu: &mut Cpu, mem: &GuestMem, out: &mut dyn TextSink) -> Flow {
        let ah = cpu.hi(AX);
        let al = cpu.lo(AX);
        let page_bh = cpu.hi(BX) & 7;
        match ah {
            0x00 => return self.set_mode(mem, al),
            0x01 => mem.write_u16(bda::CURSOR_SHAPE, cpu.w(CX)),
            0x02 => set_cursor(mem, page_bh, cpu.hi(DX), cpu.lo(DX)),
            0x03 => {
                cpu.set_w(CX, mem.read_u16(bda::CURSOR_SHAPE));
                cpu.set_w(DX, mem.read_u16(bda::CURSOR + usize::from(page_bh) * 2));
            }
            0x05 => {
                let page = al & 7;
                let s = Screen::of(mem);
                mem.write_u8(bda::PAGE, page);
                mem.write_u16(bda::PAGE_START, (usize::from(page) * s.page_size) as u16);
            }
            0x06 | 0x07 => {
                let page = mem.read_u8(bda::PAGE);
                let (top, left) = (cpu.hi(CX), cpu.lo(CX));
                let (bottom, right) = (cpu.hi(DX), cpu.lo(DX));
                scroll(mem, page, al, cpu.hi(BX), (top, left, bottom, right), ah == 0x06);
            }
            0x08 => {
                let s = Screen::of(mem);
                let (row, col) = cursor(mem, page_bh);
                let v = if s.text { mem.read_u16(s.cell(page_bh, row, col)) } else { 0 };
                cpu.set_w(AX, v);
            }
            0x09 | 0x0A => {
                let s = Screen::of(mem);
                if s.text {
                    let (row, col) = cursor(mem, page_bh);
                    let start = usize::from(row) * s.cols + usize::from(col);
                    let end = (start + usize::from(cpu.w(CX))).min(s.cols * s.rows);
                    let at = s.cell(page_bh, 0, 0);
                    for pos in start..end {
                        mem.write_u8(at + pos * 2, al);
                        if ah == 0x09 {
                            mem.write_u8(at + pos * 2 + 1, cpu.lo(BX));
                        }
                    }
                }
            }
            0x0E => {
                let page = mem.read_u8(bda::PAGE);
                tty(mem, out, page, al, None);
            }
            0x0F => {
                cpu.set_lo(AX, mem.read_u8(bda::VIDEO_MODE));
                cpu.set_hi(AX, mem.read_u16(bda::COLS) as u8);
                cpu.set_hi(BX, mem.read_u8(bda::PAGE));
            }
            // Palette, DAC : écritures acceptées, gérées à l'étape 5.
            0x10 if matches!(al, 0x00..=0x03 | 0x10 | 0x12) => {}
            // Informations sur la police (pas encore de police en ROM).
            0x11 if al == 0x30 => {
                cpu.set_w(CX, u16::from(mem.read_u8(bda::CHAR_HEIGHT)));
                cpu.set_lo(DX, mem.read_u8(bda::ROWS));
                cpu.set_sreg(ES, 0);
                cpu.set_w(BP, 0);
            }
            // Informations EGA : couleur, 256 Ko.
            0x12 if cpu.lo(BX) == 0x10 => {
                cpu.set_w(BX, 0x0003);
                cpu.set_w(CX, 0x0009);
            }
            0x13 => write_string(cpu, mem, out),
            // Combinaison d'affichage : VGA couleur.
            0x1A if al == 0x00 => {
                cpu.set_lo(AX, 0x1A);
                cpu.set_w(BX, 0x0008);
            }
            // Pas de VBE avant l'étape 8 : AL reste différent de 4Fh.
            0x4F => {}
            _ => return Flow::Unhandled(ah),
        }
        Flow::Continue
    }

    // ---- Configuration système ----

    fn int15(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Flow {
        let ah = cpu.hi(AX);
        let ok = |cpu: &mut Cpu, ah: u8| {
            cpu.set_hi(AX, ah);
            set_ret_flag(cpu, mem, CF, false);
        };
        match (ah, cpu.lo(AX)) {
            // Porte A20 : toujours ouverte (fermeture gérée à l'étape 6).
            (0x24, 0x01) => ok(cpu, 0),
            (0x24, 0x02) => {
                cpu.set_lo(AX, 1);
                ok(cpu, 0);
            }
            (0x24, 0x03) => {
                cpu.set_w(BX, 0x0003);
                ok(cpu, 0);
            }
            // Attente de CX:DX µs.
            (0x86, _) => {
                let us = (u64::from(cpu.w(CX)) << 16) | u64::from(cpu.w(DX));
                std::thread::sleep(std::time::Duration::from_micros(us));
                ok(cpu, 0);
            }
            (0x87, _) => {
                let status = move_block(cpu, mem);
                cpu.set_hi(AX, status);
                set_ret_flag(cpu, mem, CF, status != 0);
                set_ret_flag(cpu, mem, ZF, status == 0);
            }
            // Mémoire étendue en Ko (plafonnée à 63 Mo par le registre 16 bits).
            (0x88, _) => {
                cpu.set_w(AX, self.extended_kb.min(0xFC00) as u16);
                set_ret_flag(cpu, mem, CF, false);
            }
            // Attente / fin d'attente de périphérique : rien à attendre.
            (0x90 | 0x91, _) => ok(cpu, 0),
            (0xC0, _) => {
                cpu.set_sreg(ES, ROM_SEG);
                cpu.set_w(BX, CONFIG_TABLE);
                ok(cpu, 0);
            }
            // AX=E801h : Ko entre 1 et 16 Mo, blocs de 64 Ko au-delà.
            (0xE8, 0x01) => {
                let low = self.extended_kb.min(15 * 1024) as u16;
                let high = (self.extended_kb.saturating_sub(15 * 1024) / 64).min(0xFFFF) as u16;
                cpu.set_w(AX, low);
                cpu.set_w(CX, low);
                cpu.set_w(BX, high);
                cpu.set_w(DX, high);
                set_ret_flag(cpu, mem, CF, false);
            }
            (0xE8, 0x20) if cpu.gpr[DX] == SMAP => self.e820(cpu, mem),
            _ => {
                // Fonction non prise en charge : AH=86h, CF=1.
                cpu.set_hi(AX, 0x86);
                set_ret_flag(cpu, mem, CF, true);
                return Flow::Unhandled(ah);
            }
        }
        Flow::Continue
    }

    /// INT 15h AX=E820h : carte mémoire, une entrée par appel (EBX = n°).
    fn e820(&self, cpu: &mut Cpu, mem: &GuestMem) {
        let top = 0x10_0000 + self.extended_kb as u64 * 1024;
        let map: [(u64, u64, u32); 3] =
            [(0, 0xA_0000, 1), (ROM.start as u64, ROM.len() as u64, 2), (0x10_0000, top - 0x10_0000, 1)];
        let Some(&(base, len, typ)) = map.get(cpu.gpr[BX] as usize) else {
            set_ret_flag(cpu, mem, CF, true);
            return;
        };
        let at = lin(cpu.sreg(ES), cpu.w(DI));
        let mut entry = [0u8; 20];
        entry[0..8].copy_from_slice(&base.to_le_bytes());
        entry[8..16].copy_from_slice(&len.to_le_bytes());
        entry[16..20].copy_from_slice(&typ.to_le_bytes());
        mem.write(at, &entry);
        let next = cpu.gpr[BX] + 1;
        cpu.gpr[BX] = if (next as usize) < map.len() { next } else { 0 };
        cpu.gpr[AX] = SMAP;
        cpu.gpr[CX] = 20;
        set_ret_flag(cpu, mem, CF, false);
    }

    // ---- Horloge ----

    fn int1a(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Flow {
        let ah = cpu.hi(AX);
        let bcd = |v: i32| (((v / 10) << 4) | (v % 10)) as u8;
        match ah {
            0x00 => {
                let t = mem.read_u32(bda::TICKS);
                cpu.set_w(CX, (t >> 16) as u16);
                cpu.set_w(DX, t as u16);
                cpu.set_lo(AX, mem.read_u8(bda::MIDNIGHT));
                mem.write_u8(bda::MIDNIGHT, 0);
            }
            0x01 => {
                let want = (i64::from(cpu.w(CX)) << 16) | i64::from(cpu.w(DX));
                self.base += want - self.now_ticks().rem_euclid(TICKS_PER_DAY as i64);
                mem.write_u8(bda::MIDNIGHT, 0);
                self.sync_ticks(mem);
            }
            // Heure de la RTC (BCD), heure d'été non signalée.
            0x02 => {
                let t = local_time();
                cpu.set_hi(CX, bcd(t.tm_hour));
                cpu.set_lo(CX, bcd(t.tm_min));
                cpu.set_hi(DX, bcd(t.tm_sec));
                cpu.set_lo(DX, 0);
                set_ret_flag(cpu, mem, CF, false);
            }
            0x04 => {
                let t = local_time();
                let year = t.tm_year + 1900;
                cpu.set_hi(CX, bcd(year / 100));
                cpu.set_lo(CX, bcd(year % 100));
                cpu.set_hi(DX, bcd(t.tm_mon + 1));
                cpu.set_lo(DX, bcd(t.tm_mday));
                set_ret_flag(cpu, mem, CF, false);
            }
            // Réglage de la RTC : l'horloge hôte fait foi.
            0x03 | 0x05 => set_ret_flag(cpu, mem, CF, false),
            _ => return Flow::Unhandled(ah),
        }
        Flow::Continue
    }
}

const SMAP: u32 = 0x534D_4150;

pub fn local_time() -> libc::tm {
    // SAFETY: localtime_r remplit `tm`, entièrement initialisé à zéro avant.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

// ---- Mode texte ----

fn is_text(mode: u8) -> bool {
    mode <= 3 || mode == 7
}

/// Géométrie de l'écran d'après la BDA.
struct Screen {
    text: bool,
    base: usize,
    cols: usize,
    rows: usize,
    page_size: usize,
}

impl Screen {
    fn of(mem: &GuestMem) -> Self {
        let mode = mem.read_u8(bda::VIDEO_MODE);
        Self {
            text: is_text(mode),
            base: if mode == 7 { 0xB_0000 } else { 0xB_8000 },
            cols: usize::from(mem.read_u16(bda::COLS)).max(1),
            rows: usize::from(mem.read_u8(bda::ROWS)) + 1,
            page_size: usize::from(mem.read_u16(bda::PAGE_SIZE)),
        }
    }

    fn cell(&self, page: u8, row: u8, col: u8) -> usize {
        self.base + usize::from(page) * self.page_size + (usize::from(row) * self.cols + usize::from(col)) * 2
    }
}

fn cursor(mem: &GuestMem, page: u8) -> (u8, u8) {
    let w = mem.read_u16(bda::CURSOR + usize::from(page & 7) * 2);
    ((w >> 8) as u8, w as u8)
}

fn set_cursor(mem: &GuestMem, page: u8, row: u8, col: u8) {
    mem.write_u16(bda::CURSOR + usize::from(page & 7) * 2, (u16::from(row) << 8) | u16::from(col));
}

/// Défile la fenêtre (haut, gauche, bas, droite) de `n` lignes ; 0 : efface.
fn scroll(mem: &GuestMem, page: u8, n: u8, attr: u8, win: (u8, u8, u8, u8), up: bool) {
    let s = Screen::of(mem);
    if !s.text {
        return;
    }
    let (top, left) = (usize::from(win.0), usize::from(win.1));
    let bottom = usize::from(win.2).min(s.rows - 1);
    let right = usize::from(win.3).min(s.cols - 1);
    if top > bottom || left > right {
        return;
    }
    let height = bottom - top + 1;
    let n = if n == 0 { height } else { usize::from(n).min(height) };
    let row_at = |r: usize| s.cell(page, r as u8, left as u8);
    let mut line = vec![0u8; (right - left + 1) * 2];
    let moved: Vec<usize> = if up { (top..bottom + 1 - n).collect() } else { (top + n..bottom + 1).rev().collect() };
    for r in moved {
        let src = if up { r + n } else { r - n };
        mem.read(row_at(src), &mut line);
        mem.write(row_at(r), &line);
    }
    let blank: Vec<u8> = line.chunks(2).flat_map(|_| [b' ', attr]).collect();
    let cleared = if up { bottom + 1 - n..bottom + 1 } else { top..top + n };
    for r in cleared {
        mem.write(row_at(r), &blank);
    }
}

/// Efface la page active (attribut 07h) et ramène le curseur en haut.
pub fn clear_screen(mem: &GuestMem) {
    let s = Screen::of(mem);
    let page = active_page(mem);
    if s.text {
        let last = (s.rows - 1) as u8;
        scroll(mem, page, 0, 0x07, (0, 0, last, (s.cols - 1) as u8), true);
    }
    set_cursor(mem, page, 0, 0);
}

pub fn active_page(mem: &GuestMem) -> u8 {
    mem.read_u8(bda::PAGE)
}

/// Sortie télétype : caractère, BEL, BS, LF, CR, défilement en bas d'écran.
/// Recopiée sur `out` (terminal, tests) en attendant l'affichage.
pub fn tty(mem: &GuestMem, out: &mut dyn TextSink, page: u8, ch: u8, attr: Option<u8>) {
    out.write(&[ch]);
    let s = Screen::of(mem);
    let (mut row, mut col) = cursor(mem, page);
    match ch {
        0x07 => return,
        0x08 => col = col.saturating_sub(1),
        b'\n' => row = row.saturating_add(1),
        b'\r' => col = 0,
        _ => {
            if s.text {
                let at = s.cell(page, row, col);
                mem.write_u8(at, ch);
                if let Some(a) = attr {
                    mem.write_u8(at + 1, a);
                }
            }
            col += 1;
            if usize::from(col) >= s.cols {
                col = 0;
                row = row.saturating_add(1);
            }
        }
    }
    if usize::from(row) >= s.rows {
        row = (s.rows - 1) as u8;
        let a = if s.text { mem.read_u8(s.cell(page, row, col) + 1) } else { 0 };
        scroll(mem, page, 1, a, (0, 0, row, (s.cols - 1) as u8), true);
    }
    set_cursor(mem, page, row, col);
}

/// INT 10h AH=13h : chaîne ES:BP de CX caractères en DH:DL, page BH.
fn write_string(cpu: &Cpu, mem: &GuestMem, out: &mut dyn TextSink) {
    let mode = cpu.lo(AX);
    let page = cpu.hi(BX) & 7;
    let saved = cursor(mem, page);
    set_cursor(mem, page, cpu.hi(DX), cpu.lo(DX));
    let mut at = lin(cpu.sreg(ES), cpu.w(BP));
    for _ in 0..cpu.w(CX) {
        let ch = mem.read_u8(at);
        at += 1;
        let mut attr = cpu.lo(BX);
        if mode & 2 != 0 {
            attr = mem.read_u8(at);
            at += 1;
        }
        let ctrl = matches!(ch, 0x07 | 0x08 | b'\n' | b'\r');
        tty(mem, out, page, ch, (!ctrl).then_some(attr));
    }
    if mode & 1 == 0 {
        set_cursor(mem, page, saved.0, saved.1);
    }
}

/// Texte de la page active, une chaîne par ligne (tests, `--screen`).
pub fn screen_text(mem: &GuestMem) -> Vec<String> {
    let s = Screen::of(mem);
    if !s.text {
        return Vec::new();
    }
    let page = mem.read_u8(bda::PAGE);
    (0..s.rows)
        .map(|r| {
            let line: String = (0..s.cols).map(|c| cp437(mem.read_u8(s.cell(page, r as u8, c as u8)))).collect();
            line.trim_end().to_owned()
        })
        .collect()
}

/// Caractère Unicode d'un octet de la page de codes 437.
fn cp437(b: u8) -> char {
    const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
    match b {
        0 => ' ',
        0x20..=0x7E => char::from(b),
        0x80.. => HIGH.chars().nth(usize::from(b - 0x80)).unwrap_or('?'),
        _ => '·',
    }
}

/// Octet d'attribut d'une cellule de la page active (tests).
#[cfg(test)]
pub fn screen_attr(mem: &GuestMem, row: u8, col: u8) -> u8 {
    let s = Screen::of(mem);
    mem.read_u8(s.cell(mem.read_u8(bda::PAGE), row, col) + 1)
}

// ---- Disque ----

/// INT 13h minimal : aucun lecteur avant le DOS sur images disque (étape 9).
fn int13(cpu: &mut Cpu, mem: &GuestMem) -> Flow {
    let ok = matches!(cpu.hi(AX), 0x00 | 0x15);
    cpu.set_hi(AX, if ok { 0x00 } else { 0x01 });
    set_ret_flag(cpu, mem, CF, !ok);
    Flow::Continue
}

// ---- Mémoire étendue ----

/// INT 15h AH=87h : copie de CX mots selon la GDT en ES:SI (source en +10h,
/// destination en +18h). Renvoie le statut (AH).
fn move_block(cpu: &Cpu, mem: &GuestMem) -> u8 {
    let gdt = lin(cpu.sreg(ES), cpu.w(SI));
    let base = |d: usize| {
        let b = mem.read_u32(gdt + d + 2) & 0x00FF_FFFF;
        (b | (u32::from(mem.read_u8(gdt + d + 7)) << 24)) as usize
    };
    let (src, dst) = (base(0x10), base(0x18));
    let len = usize::from(cpu.w(CX)) * 2;
    if len > 0x1_0000 || src + len > mem.size() || dst + len > mem.size() {
        return 0x03;
    }
    let mut buf = vec![0u8; len];
    mem.read(src, &mut buf);
    mem.write(dst, &buf);
    0x00
}

// ---- Clavier ----

fn kb_push(mem: &GuestMem, word: u16) -> bool {
    let tail = mem.read_u16(bda::KB_TAIL);
    let mut next = tail + 2;
    if next >= mem.read_u16(bda::KB_END) {
        next = mem.read_u16(bda::KB_START);
    }
    if next == mem.read_u16(bda::KB_HEAD) {
        return false;
    }
    mem.write_u16(0x400 + usize::from(tail), word);
    mem.write_u16(bda::KB_TAIL, next);
    true
}

fn kb_peek(mem: &GuestMem) -> Option<u16> {
    let head = mem.read_u16(bda::KB_HEAD);
    (head != mem.read_u16(bda::KB_TAIL)).then(|| mem.read_u16(0x400 + usize::from(head)))
}

fn kb_pop(mem: &GuestMem) -> Option<u16> {
    let word = kb_peek(mem)?;
    let mut head = mem.read_u16(bda::KB_HEAD) + 2;
    if head >= mem.read_u16(bda::KB_END) {
        head = mem.read_u16(bda::KB_START);
    }
    mem.write_u16(bda::KB_HEAD, head);
    Some(word)
}

/// Range un événement d'entrée dans le tampon clavier de la BDA.
/// Provisoire : à l'étape 4, l'IRQ 1 et INT 09h s'en chargeront.
/// Applique un événement : état des modificateurs, touche rangée.
fn kb_event(mem: &GuestMem, ev: InputEvent) {
    match ev {
        InputEvent::Key(code) => {
            let scan = code & 0x7F;
            let make = code & 0x80 == 0;
            let mut shift = mem.read_u8(bda::SHIFT);
            if let Some(bit) = keyboard::modifier(scan) {
                shift = if make { shift | bit } else { shift & !bit };
            } else if scan == 0x3A && make {
                shift ^= keyboard::CAPS;
            } else if make && let Some(w) = keyboard::key_word(scan, shift) {
                kb_push(mem, w);
            }
            mem.write_u8(bda::SHIFT, shift);
        }
        InputEvent::Text(c) => {
            if let Some(w) = keyboard::char_word(c) {
                kb_push(mem, w);
            }
        }
        InputEvent::Mouse { .. } | InputEvent::Quit => {}
    }
}

/// Range l'entrée en attente dans le tampon de la BDA. Le relâchement d'un
/// modificateur attend aussi que la touche frappée avec lui ait été lue :
/// AH=02h juste après la lecture voit l'état de la frappe (DFlat, Alt+F).
fn kb_pump(mem: &GuestMem, host: &mut Host) {
    // Une touche à la fois, comme sous des doigts : la suivante attend que
    // la précédente ait été lue (les boucles d'événements, DFlat par
    // exemple, ne mélangent ainsi pas l'ordre des raccourcis et du texte).
    if kb_peek(mem).is_some() {
        return;
    }
    while !host.quit {
        match host.input.poll() {
            Some(InputEvent::Quit) => host.quit = true,
            Some(ev) => {
                kb_event(mem, ev);
                if kb_peek(mem).is_some() {
                    break;
                }
            }
            None => break,
        }
    }
}

/// Mot clavier vu par les fonctions « non étendues » (00h, 01h).
fn classic(word: u16) -> u16 {
    if word as u8 == 0xE0 { word & 0xFF00 } else { word }
}

/// Retire la prochaine touche du tampon, en l'attendant au besoin.
pub fn read_key(mem: &GuestMem, host: &mut Host) -> Result<u16> {
    kb_pump(mem, host);
    loop {
        if let Some(w) = kb_pop(mem) {
            return Ok(w);
        }
        if host.quit {
            host.stop = true;
            return Err("attente d'une touche, fin de l'entrée".into());
        }
        match host.input.wait() {
            Some(InputEvent::Quit) => host.quit = true,
            Some(ev) => kb_event(mem, ev),
            None => return Err("attente d'une touche, plus aucune entrée".into()),
        }
    }
}

/// Délai laissé à un programme qui scrute le clavier après la fin de l'entrée.
const QUIT_GRACE: std::time::Duration = std::time::Duration::from_millis(300);

/// Prochaine touche du tampon, sans la retirer ni attendre.
pub fn peek_key(mem: &GuestMem, host: &mut Host) -> Option<u16> {
    kb_pump(mem, host);
    let key = kb_peek(mem);
    if key.is_none() && host.quit {
        let since = *host.drained_at.get_or_insert_with(std::time::Instant::now);
        host.stop = since.elapsed() >= QUIT_GRACE;
    }
    key
}

fn int16(cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Flow> {
    let ah = cpu.hi(AX);
    match ah {
        0x00 | 0x10 => {
            let word = read_key(mem, host).map_err(|e| format!("INT 16h : {e}"))?;
            cpu.set_w(AX, if ah == 0 { classic(word) } else { word });
        }
        0x01 | 0x11 => {
            let word = peek_key(mem, host);
            if let Some(w) = word {
                cpu.set_w(AX, if ah == 1 { classic(w) } else { w });
            }
            set_ret_flag(cpu, mem, ZF, word.is_none());
        }
        // État des modificateurs, sans lire l'entrée : il reste celui de la
        // dernière touche rangée.
        0x02 | 0x12 => {
            cpu.set_lo(AX, mem.read_u8(bda::SHIFT));
            if ah == 0x12 {
                cpu.set_hi(AX, 0);
            }
        }
        // Répétition automatique : sans effet.
        0x03 => {}
        0x05 => {
            let full = !kb_push(mem, cpu.w(CX));
            cpu.set_lo(AX, u8::from(full));
        }
        _ => return Ok(Flow::Unhandled(ah)),
    }
    Ok(Flow::Continue)
}
