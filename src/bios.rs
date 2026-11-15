// SPDX-License-Identifier: GPL-3.0-or-later
//! BIOS en HLE (étape 2) : zone de données (BDA), vidéo texte (INT 10h),
//! configuration (11h, 12h, 15h), disque minimal (13h), clavier (16h),
//! horloge (1Ah).
//!
//! Comme sur un vrai PC, l'état vit dans la BDA (0040:0000) : un programme
//! qui la lit ou l'écrit directement reste cohérent avec les services.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::backend::Result;
use crate::hle::{AX, BP, BX, CF, CX, DI, DX, ES, Flow, Regs, SI, ZF, lin, set_ret_flag};
use crate::host::{Host, TextSink};
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
/// Polices en ROM (`INT 10h AX=1130h`, INT 43h) : 8×16 et 8×14 tirées de
/// Terminus, et une table de remplacement 9 points vide (un seul 00h).
const FONT_8X16: u16 = 0x1000;
const FONT_8X14: u16 = 0x2000;
const FONT_ALT_EMPTY: u16 = 0x2E00;
/// Routine de conversion de casse des informations nationales (RETF).
pub const CASE_MAP: u16 = 0xE710;

/// Adresses dans la BDA.
pub mod bda {
    pub const EQUIPMENT: usize = 0x410;
    pub const HARD_DISKS: usize = 0x475;
    pub const MEM_KB: usize = 0x413;
    pub const SHIFT: usize = 0x417;
    /// État étendu du clavier (KB_FLAG_3) : bit 4 clavier MF2 présent, bit 1
    /// dernier code reçu E0h.
    pub const KB_FLAG_3: usize = 0x496;
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

/// Équipement (INT 11h) : coprocesseur, 80×25 couleur ; les disquettes
/// s'y ajoutent (bit 0, nombre − 1 en bits 6–7).
const EQUIPMENT: u16 = 0x0022;
/// Tics par jour à 1 193 182 / 65 536 Hz.
const TICKS_PER_DAY: u64 = 0x1800B0;
use crate::pit::PIT_HZ;

/// Gestionnaires en ROM (asm/bios.S).
const INT08: u16 = 0xFEA5;
const IRQ_MASTER_DEFAULT: u16 = 0xE820;
const IRQ_SLAVE_DEFAULT: u16 = 0xE830;
const DUMMY_IRET: u16 = 0xE840;
/// INT 09h (IRQ 1), à l'adresse de l'IBM AT.
pub const INT09: u16 = 0xE987;
/// Reprises après HLT d'un appel qui attend une touche (asm/bios.S).
/// Point d'entrée XMS (saut court, NOP, `out E0h, al`, `retf`).
/// Table des paramètres de disquette (asm/bios.S, vecteur 1Eh).
pub const DISKETTE_PARAMS: u16 = 0xEFC7;
pub const XMS_ENTRY: u16 = 0xE8A0;
/// Souris PS/2 (asm/bios.S) : routine du pilote, INT 74h, appel hôte.
pub const PS2_HANDLER: usize = 0xF_E938;
const INT74: u16 = 0xE940;
pub const PS2_FETCH: u16 = 0xEAA8;
/// Demande d'événement du pilote INT 33h intégré, et adresse de son
/// gestionnaire utilisateur (asm/bios.S).
pub const MOUSE33_FETCH: u16 = 0xEAC0;
pub const MOUSE33_HANDLER: u16 = 0xEAE0;
/// VBE (asm/bios.S) : fonction de fenêtre, chaînes, liste des modes.
const VBE_WIN_FUNC: u16 = 0xE8D0;
const VBE_OEM: u16 = 0xE8E0;
const VBE_VENDOR: u16 = 0xE8F0;
const VBE_PRODUCT: u16 = 0xE900;
const VBE_REVISION: u16 = 0xE910;
/// Liste des modes VBE (écrite par `install_rom`, terminée par FFFFh).
const VBE_MODES: u16 = 0xEA00;

/// Modes VBE : numéro, largeur, hauteur, bits par point. Numéros de VBE 1.2
/// jusqu'à 11Bh ; au-delà, numéros propres (VBE 2.0 : les programmes
/// parcourent la liste et lisent les caractéristiques de chaque mode) : 32 bits
/// (OpenGL logiciel et SDL sous HX), 512×384 (`sw_mode 2` de Quake 2).
const VBE_MODE_LIST: [(u16, usize, usize, usize); 26] = [
    (0x100, 640, 400, 8),
    (0x101, 640, 480, 8),
    (0x103, 800, 600, 8),
    (0x105, 1024, 768, 8),
    (0x10D, 320, 200, 15),
    (0x10E, 320, 200, 16),
    (0x10F, 320, 200, 24),
    (0x110, 640, 480, 15),
    (0x111, 640, 480, 16),
    (0x112, 640, 480, 24),
    (0x113, 800, 600, 15),
    (0x114, 800, 600, 16),
    (0x115, 800, 600, 24),
    (0x116, 1024, 768, 15),
    (0x117, 1024, 768, 16),
    (0x118, 1024, 768, 24),
    (0x120, 320, 200, 32),
    (0x121, 640, 480, 32),
    (0x122, 800, 600, 32),
    (0x123, 1024, 768, 32),
    (0x124, 512, 384, 8),
    (0x125, 512, 384, 15),
    (0x126, 512, 384, 16),
    (0x127, 512, 384, 32),
    (0x128, 320, 240, 8),
    (0x129, 320, 240, 32),
];

/// Mode VBE de la liste.
struct VbeMode {
    mode: u16,
    width: usize,
    height: usize,
    bpp: usize,
}

impl VbeMode {
    fn bytes(&self) -> usize {
        self.bpp.div_ceil(8)
    }
}

fn vbe_mode(mode: u16) -> Option<VbeMode> {
    VBE_MODE_LIST.iter().find(|m| m.0 == mode).map(|&(mode, width, height, bpp)| VbeMode { mode, width, height, bpp })
}

/// Bloc d'information d'un mode (INT 10h AX=4F01h), 256 octets.
fn write_mode_info(mem: &GuestMem, at: usize, m: &VbeMode, lfb: bool) {
    let pitch = m.width * m.bytes();
    mem.fill(at, 256, 0);
    // Supporté, informations étendues, couleur, graphique, sans télétype, LFB.
    mem.write_u16(at, if lfb { 0x009B } else { 0x001B });
    mem.write_u8(at + 2, 0x07); // fenêtre A : présente, lecture, écriture
    mem.write_u16(at + 4, 64); // granularité (Ko)
    mem.write_u16(at + 6, 64); // taille (Ko)
    mem.write_u16(at + 8, 0xA000);
    mem.write_u32(at + 0x0C, (u32::from(ROM_SEG) << 16) | u32::from(VBE_WIN_FUNC));
    mem.write_u16(at + 0x10, pitch as u16);
    mem.write_u16(at + 0x12, m.width as u16);
    mem.write_u16(at + 0x14, m.height as u16);
    mem.write(at + 0x16, &[8, 16, 1, m.bpp as u8, 1]);
    mem.write_u8(at + 0x1B, if m.bpp == 8 { 4 } else { 6 }); // compact / couleurs directes
    let pages = (crate::vga::FB_SIZE / (pitch * m.height)).saturating_sub(1).min(255);
    mem.write_u8(at + 0x1D, pages as u8);
    mem.write_u8(at + 0x1E, 1);
    // Masques (taille, position) rouge, vert, bleu, réservé.
    let masks: [u8; 8] = match m.bpp {
        15 => [5, 10, 5, 5, 5, 0, 1, 15],
        16 => [5, 11, 6, 5, 5, 0, 0, 0],
        24 => [8, 16, 8, 8, 8, 0, 0, 0],
        32 => [8, 16, 8, 8, 8, 0, 8, 24],
        _ => [0; 8],
    };
    mem.write(at + 0x1F, &masks);
    if lfb {
        mem.write_u32(at + 0x28, crate::kvm::LFB_ADDR as u32);
    }
}
pub const WAIT16: u16 = 0xE850;
pub const WAIT21: u16 = 0xE860;
/// INT 33h en ROM (AX=0003h sans sortie) et son bloc d'état (asm/bios.S).
const INT33: u16 = 0xE870;
pub const MOUSE_STATE: usize = 0xF_E890;

/// ROM BIOS en F0000 et IVT : le vecteur n pointe sur le stub F000:n*16,
/// sauf les IRQ et INT 1Ch, servies en ROM sans sortie de VM.
pub fn install_rom(mem: &GuestMem) {
    mem.write(ROM.start, BIOS_IMAGE);
    mem.write(ROM.start + usize::from(FONT_8X16), crate::font::BUILTIN);
    mem.write(ROM.start + usize::from(FONT_8X14), &crate::font::builtin_8x14());
    mem.write_u8(ROM.start + usize::from(FONT_ALT_EMPTY), 0);
    let modes = ROM.start + usize::from(VBE_MODES);
    for (i, m) in VBE_MODE_LIST.iter().enumerate() {
        mem.write_u16(modes + 2 * i, m.0);
    }
    mem.write_u16(modes + 2 * VBE_MODE_LIST.len(), 0xFFFF);
    let set = |n: u16, off: u16| {
        mem.write_u16(usize::from(n) * 4, off);
        mem.write_u16(usize::from(n) * 4 + 2, ROM_SEG);
    };
    for n in 0..256u16 {
        set(n, n * 16);
    }
    set(0x08, INT08);
    set(0x09, INT09);
    set(0x33, INT33);
    // EMS : segment de l'en-tête « EMMXXXX0 » (F000:E8B0), entrée en 12h.
    mem.write_u16(0x67 * 4, 0x0012);
    mem.write_u16(0x67 * 4 + 2, ROM_SEG + (0xE8B0 >> 4));
    for n in 0x0A..=0x0F {
        set(n, IRQ_MASTER_DEFAULT);
    }
    for n in 0x70..=0x77 {
        set(n, IRQ_SLAVE_DEFAULT);
    }
    set(0x1C, DUMMY_IRET);
    set(0x74, INT74);
    // Vecteur 1Eh : données (paramètres de disquette), pas du code.
    set(0x1E, DISKETTE_PARAMS);
}

pub struct Bios {
    extended_kb: usize,
    /// Préfixe E0h reçu : le scancode suivant est une touche étendue.
    e0: bool,
    /// Modes vidéo graphiques déjà signalés.
    warned_modes: BTreeSet<u8>,
    pub mouse: crate::mouse::Mouse,
    /// VBE 2.0 avec framebuffer linéaire ; sinon VBE 1.2, banques seules (`--vbe1`).
    pub vbe2: bool,
    vga: std::rc::Rc<std::cell::RefCell<crate::vga::Vga>>,
    /// Images de disquette et de disque dur (INT 13h).
    pub disks: crate::disk::Disks,
    /// XMS et EMS fournies par l'hôte (sinon, démarrage d'un vrai DOS : ses
    /// propres gestionnaires prennent la mémoire étendue).
    pub host_memory: bool,
}

impl Bios {
    pub fn new(extended_kb: usize, vga: std::rc::Rc<std::cell::RefCell<crate::vga::Vga>>) -> Self {
        Self { extended_kb, e0: false, warned_modes: BTreeSet::new(), mouse: crate::mouse::Mouse::new(), vbe2: true, vga, disks: Default::default(), host_memory: true }
    }

    /// ROM, IVT, BDA, écran texte effacé.
    pub fn install(&mut self, mem: &GuestMem) {
        install_rom(mem);
        mem.fill(0x400, 0x100, 0);
        let floppies = u16::from(self.disks.floppy_count());
        let drives = if floppies > 0 { 1 | ((floppies - 1) << 6) } else { 0 };
        mem.write_u16(bda::EQUIPMENT, EQUIPMENT | drives);
        mem.write_u8(bda::HARD_DISKS, u8::from(self.disks.hard.is_some()));
        mem.write_u16(bda::MEM_KB, 640);
        mem.write_u16(bda::KB_START, 0x1E);
        mem.write_u16(bda::KB_END, 0x3E);
        mem.write_u16(bda::KB_HEAD, 0x1E);
        mem.write_u16(bda::KB_TAIL, 0x1E);
        mem.write_u8(bda::KB_FLAG_3, KBX);
        mem.write_u8(bda::EGA_MISC, 0x60);
        mem.write_u8(bda::EGA_SWITCHES, 0x09);
        mem.write_u8(bda::VGA_FLAGS, 0x11);
        mem.write_u8(bda::DCC, 0x0B);
        self.set_mode(mem, 3);
        // Tics depuis minuit ; ensuite, c'est l'IRQ 0 (INT 08h) qui les fait avancer.
        let t = local_time();
        let secs = (t.tm_hour * 3600 + t.tm_min * 60 + t.tm_sec) as u64;
        mem.write_u32(bda::TICKS, (secs * PIT_HZ / 65536 % TICKS_PER_DAY) as u32);
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
            0x13 => {
                self.disks.int13(cpu, mem);
                Flow::Continue
            }
            0x15 => self.int15(cpu, mem),
            0x16 => int16(cpu, mem),
            0x1A => self.int1a(cpu, mem),
            0x33 => self.mouse.int33(cpu, mem),
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
        if !text && !matches!(mode, 0x0D | 0x0E | 0x10 | 0x12 | 0x13) && self.warned_modes.insert(mode) {
            eprintln!("[bios] video mode {mode:02X}h: VGA registers not programmed");
        }
        self.vga.borrow_mut().set_mode(mode, al & 0x80 == 0);
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
        // Hauteur des caractères, et police graphique (INT 43h) des modes
        // 350 et 480 lignes ; pas encore de police 8×8 pour ceux de 200.
        let height = match mode {
            0x0F | 0x10 => 14,
            0x11 | 0x12 => 16,
            _ if text => 16,
            _ => 8,
        };
        mem.write_u8(bda::CHAR_HEIGHT, height);
        if !text && height > 8 {
            let font = if height == 14 { FONT_8X14 } else { FONT_8X16 };
            mem.write_u16(0x43 * 4, font);
            mem.write_u16(0x43 * 4 + 2, ROM_SEG);
        }
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
            // Police ROM 8×16 dans un bloc (AL=04h, 14h) : c'est déjà celle
            // du rendu texte. Les polices chargées par le programme (plan 2)
            // ne sont pas encore affichées.
            0x11 if matches!(al, 0x04 | 0x14) => {}
            // Informations sur la police : ES:BP selon BH (00h INT 1Fh,
            // 01h INT 43h, 02h 8×14, 06h 8×16, 05h/07h remplacements 9
            // points) ; 8×8 (03h, 04h) pas encore en ROM : 0000:0000.
            0x11 if al == 0x30 => {
                cpu.set_w(CX, u16::from(mem.read_u8(bda::CHAR_HEIGHT)));
                cpu.set_lo(DX, mem.read_u8(bda::ROWS));
                let vector = |n: usize| (mem.read_u16(n * 4 + 2), mem.read_u16(n * 4));
                let (seg, off) = match cpu.hi(BX) {
                    0x00 => vector(0x1F),
                    0x01 => vector(0x43),
                    0x02 => (ROM_SEG, FONT_8X14),
                    0x06 => (ROM_SEG, FONT_8X16),
                    0x05 | 0x07 => (ROM_SEG, FONT_ALT_EMPTY),
                    _ => (0, 0),
                };
                cpu.set_sreg(ES, seg);
                cpu.set_w(BP, off);
            }
            // Informations EGA : couleur, 256 Ko.
            0x12 if cpu.lo(BX) == 0x10 => {
                cpu.set_w(BX, 0x0003);
                cpu.set_w(CX, 0x0009);
            }
            // Lignes de balayage, chargement de palette, adressage, vidéo,
            // sommation des gris, curseur : acceptés (réglages de l'étape 5).
            0x12 if (0x30..=0x36).contains(&cpu.lo(BX)) => cpu.set_lo(AX, 0x12),
            0x13 => write_string(cpu, mem, out),
            // Combinaison d'affichage : VGA couleur.
            0x1A if al == 0x00 => {
                cpu.set_lo(AX, 0x1A);
                cpu.set_w(BX, 0x0008);
            }
            0x4F => return self.vbe(cpu, mem),
            _ => return Flow::Unhandled(ah),
        }
        Flow::Continue
    }

    // ---- VBE 2.0 (SVGA) ----

    /// INT 10h AH=4Fh. AX=004Fh en cas de succès, 014Fh en cas d'échec.
    fn vbe(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Flow {
        let al = cpu.lo(AX);
        let es_di = lin(cpu.sreg(ES), cpu.w(DI));
        let rom = |off: u16| (u32::from(ROM_SEG) << 16) | u32::from(off);
        let ok = match al {
            // Information sur le contrôleur ; bloc VBE 2.0 de 512 octets si
            // l'appelant a écrit « VBE2 ».
            0x00 => {
                let vbe2 = mem.read_u32(es_di) == u32::from_le_bytes(*b"VBE2") && self.vbe2;
                mem.fill(es_di, if vbe2 { 512 } else { 256 }, 0);
                mem.write(es_di, b"VESA");
                mem.write_u16(es_di + 4, if self.vbe2 { 0x0200 } else { 0x0102 });
                mem.write_u32(es_di + 6, rom(VBE_OEM));
                mem.write_u32(es_di + 14, rom(VBE_MODES));
                mem.write_u16(es_di + 18, (crate::vga::FB_SIZE >> 16) as u16);
                if vbe2 {
                    mem.write_u16(es_di + 20, 0x0001);
                    mem.write_u32(es_di + 22, rom(VBE_VENDOR));
                    mem.write_u32(es_di + 26, rom(VBE_PRODUCT));
                    mem.write_u32(es_di + 30, rom(VBE_REVISION));
                }
                true
            }
            0x01 => match vbe_mode(cpu.w(CX) & 0x1FF) {
                Some(m) => {
                    write_mode_info(mem, es_di, &m, self.vbe2);
                    true
                }
                None => false,
            },
            0x02 => {
                let bx = cpu.w(BX);
                let keep = bx & 0x8000 != 0;
                if bx & 0x1FF < 0x100 {
                    let _ = self.set_mode(mem, (bx as u8 & 0x7F) | if keep { 0x80 } else { 0 });
                    true
                } else if bx & 0x4000 != 0 && !self.vbe2 {
                    false
                } else if let Some(m) = vbe_mode(bx & 0x1FF) {
                    let _ = self.set_mode(mem, 0x13 | 0x80);
                    let v = crate::vga::Vbe {
                        mode: m.mode,
                        width: m.width,
                        height: m.height,
                        bpp: m.bpp,
                        pitch: m.width * m.bytes(),
                        start: 0,
                        bank: 0,
                        lfb: bx & 0x4000 != 0,
                    };
                    self.vga.borrow_mut().set_vbe(v, !keep);
                    true
                } else {
                    false
                }
            }
            0x03 => {
                let mode = match &self.vga.borrow().vbe {
                    Some(v) => v.mode | if v.lfb { 0x4000 } else { 0 },
                    None => u16::from(mem.read_u8(bda::VIDEO_MODE)),
                };
                cpu.set_w(BX, mode);
                true
            }
            // Fenêtre A seulement, granularité de 64 Ko.
            0x05 => {
                let mut vga = self.vga.borrow_mut();
                match (&mut vga.vbe, cpu.hi(BX), cpu.lo(BX)) {
                    (Some(v), 0, 0) => {
                        v.bank = usize::from(cpu.w(DX));
                        true
                    }
                    (Some(v), 1, 0) => {
                        cpu.set_w(DX, v.bank as u16);
                        true
                    }
                    _ => false,
                }
            }
            // Longueur de ligne logique : en points (0), lue (1), en octets (2), maximale (3).
            0x06 => {
                let mut vga = self.vga.borrow_mut();
                match &mut vga.vbe {
                    Some(v) => {
                        let bytes = v.bpp.div_ceil(8);
                        match cpu.lo(BX) {
                            0 => v.pitch = usize::from(cpu.w(CX)) * bytes,
                            2 => v.pitch = usize::from(cpu.w(CX)),
                            _ => {}
                        }
                        let pitch = if cpu.lo(BX) == 3 { (crate::vga::FB_SIZE / v.height).min(0x7FFF) } else { v.pitch };
                        cpu.set_w(BX, pitch as u16);
                        cpu.set_w(CX, (pitch / bytes) as u16);
                        cpu.set_w(DX, (crate::vga::FB_SIZE / pitch.max(1)).min(0xFFFF) as u16);
                        true
                    }
                    None => false,
                }
            }
            // Début de l'affichage : réglage (00h, 80h au retour vertical), lecture (01h).
            0x07 => {
                let mut vga = self.vga.borrow_mut();
                match &mut vga.vbe {
                    Some(v) => {
                        let bytes = v.bpp.div_ceil(8);
                        match cpu.lo(BX) {
                            0x00 | 0x80 => v.start = usize::from(cpu.w(DX)) * v.pitch + usize::from(cpu.w(CX)) * bytes,
                            _ => {
                                cpu.set_hi(BX, 0);
                                cpu.set_w(CX, ((v.start % v.pitch.max(1)) / bytes) as u16);
                                cpu.set_w(DX, (v.start / v.pitch.max(1)) as u16);
                            }
                        }
                        true
                    }
                    None => false,
                }
            }
            // DAC fixe à 6 bits.
            0x08 => {
                cpu.set_hi(BX, 6);
                true
            }
            // Palette : entrées de 4 octets (bleu, vert, rouge, alignement).
            0x09 => {
                let (count, first) = (usize::from(cpu.w(CX)), usize::from(cpu.w(DX)));
                let mut vga = self.vga.borrow_mut();
                for i in 0..count.min(256usize.saturating_sub(first)) {
                    let at = es_di + i * 4;
                    let e = &mut vga.dac[first + i];
                    if cpu.lo(BX) & 0x7F == 0 {
                        *e = [mem.read_u8(at + 2) & 0x3F, mem.read_u8(at + 1) & 0x3F, mem.read_u8(at) & 0x3F];
                    } else {
                        mem.write(at, &[e[2], e[1], e[0], 0]);
                    }
                }
                true
            }
            // Sauvegarde d'état (04h), interface protégée (0Ah) : absentes.
            _ => false,
        };
        cpu.set_w(AX, if ok { 0x004F } else { 0x014F });
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
            // Porte A20 : AX=2400h–2402h servis par la machine (8042).
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
            // Mémoire étendue libre en Ko : aucune sous la XMS de l'hôte, comme
            // sous HIMEM.SYS (E801h et E820h restent la carte physique) ; toute
            // sous un vrai DOS.
            (0x88, _) => {
                cpu.set_w(AX, if self.host_memory { 0 } else { self.extended_kb.min(0xFFFF) as u16 });
                set_ret_flag(cpu, mem, CF, false);
            }
            // Attente / fin d'attente de périphérique : rien à attendre.
            (0x90 | 0x91, _) => ok(cpu, 0),
            // Interception clavier (appelée par INT 09h) : touche à traiter.
            (0x4F, _) => set_ret_flag(cpu, mem, CF, true),
            // APM absente : « fonction non prise en charge ».
            (0x53, _) => {
                cpu.set_hi(AX, 0x86);
                set_ret_flag(cpu, mem, CF, true);
            }
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
            // Détection de DOS/16M (faite par DOS/4GW) : absent, sans message.
            (0xBF, _) => {
                cpu.set_hi(AX, 0x86);
                set_ret_flag(cpu, mem, CF, true);
            }
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
                mem.write_u32(bda::TICKS, (u32::from(cpu.w(CX)) << 16) | u32::from(cpu.w(DX)));
                mem.write_u8(bda::MIDNIGHT, 0);
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
            // BIOS PCI absent (AH=B1h) : CF levé, AH inchangé (convention des
            // BIOS sans PCI, que les pilotes testent).
            0xB1 => set_ret_flag(cpu, mem, CF, true),
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
pub fn cp437(b: u8) -> char {
    const LOW: &str = "☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼";
    const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
    match b {
        0 => ' ',
        0x01..=0x1F => LOW.chars().nth(usize::from(b - 1)).unwrap_or('?'),
        0x20..=0x7E => char::from(b),
        0x7F => '⌂',
        0x80.. => HIGH.chars().nth(usize::from(b - 0x80)).unwrap_or('?'),
    }
}

/// Page active du mode texte : cellules (caractère, attribut) et curseur.
#[derive(Clone, PartialEq, Eq)]
pub struct TextSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<(u8, u8)>,
    /// Ligne, colonne ; `None` si le curseur est masqué (bit 5 de sa forme).
    pub cursor: Option<(usize, usize)>,
}

/// Instantané de l'écran ; `None` en mode graphique.
pub fn text_snapshot(mem: &GuestMem) -> Option<TextSnapshot> {
    let s = Screen::of(mem);
    if !s.text {
        return None;
    }
    let page = active_page(mem);
    let rows = s.rows.min(60);
    let mut raw = vec![0u8; s.cols * rows * 2];
    mem.read(s.cell(page, 0, 0), &mut raw);
    let cells = raw.chunks(2).map(|c| (c[0], c[1])).collect();
    let (r, c) = cursor(mem, page);
    let hidden = mem.read_u8(bda::CURSOR_SHAPE + 1) & 0x20 != 0;
    let cursor = (!hidden && usize::from(r) < rows && usize::from(c) < s.cols).then_some((usize::from(r), usize::from(c)));
    Some(TextSnapshot { cols: s.cols, rows, cells, cursor })
}

/// Octet d'attribut d'une cellule de la page active (tests).
#[cfg(test)]
pub fn screen_attr(mem: &GuestMem, row: u8, col: u8) -> u8 {
    let s = Screen::of(mem);
    mem.read_u8(s.cell(mem.read_u8(bda::PAGE), row, col) + 1)
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

/// Prochaine touche du tampon de la BDA, sans la retirer.
pub fn peek_key(mem: &GuestMem) -> Option<u16> {
    let head = mem.read_u16(bda::KB_HEAD);
    (head != mem.read_u16(bda::KB_TAIL)).then(|| mem.read_u16(0x400 + usize::from(head)))
}

/// Retire la prochaine touche du tampon de la BDA.
pub fn read_key(mem: &GuestMem) -> Option<u16> {
    let word = peek_key(mem)?;
    let mut head = mem.read_u16(bda::KB_HEAD) + 2;
    if head >= mem.read_u16(bda::KB_END) {
        head = mem.read_u16(bda::KB_START);
    }
    mem.write_u16(bda::KB_HEAD, head);
    Some(word)
}

/// KB_FLAG_3 : clavier MF2 (101/102 touches) installé.
const KBX: u8 = 0x10;
/// KB_FLAG_3 : le dernier code reçu était le préfixe E0h.
const LC_E0: u8 = 0x02;

/// Mot clavier vu par les fonctions « non étendues » (00h, 01h).
fn classic(word: u16) -> u16 {
    if word as u8 == 0xE0 { word & 0xFF00 } else { word }
}

impl Bios {
    /// Scancode reçu par l'INT 09h de la ROM (port E1h) : état des
    /// modificateurs et mot clavier rangé dans le tampon de la BDA.
    pub fn scancode(&mut self, mem: &GuestMem, code: u8) {
        if code == 0xE0 {
            self.e0 = true;
            mem.write_u8(bda::KB_FLAG_3, mem.read_u8(bda::KB_FLAG_3) | LC_E0);
            return;
        }
        let extended = std::mem::take(&mut self.e0);
        mem.write_u8(bda::KB_FLAG_3, mem.read_u8(bda::KB_FLAG_3) & !LC_E0);
        let scan = code & 0x7F;
        let make = code & 0x80 == 0;
        // Maj « fictives » ajoutées par le clavier autour des touches étendues.
        if extended && matches!(scan, 0x2A | 0x36) {
            return;
        }
        let mut shift = mem.read_u8(bda::SHIFT);
        if let Some(bit) = keyboard::modifier(scan) {
            shift = if make { shift | bit } else { shift & !bit };
        } else if make {
            match scan {
                0x3A => shift ^= keyboard::CAPS,
                0x45 => shift ^= 0x20,
                0x46 => shift ^= 0x10,
                _ => {
                    if scan == 0x52 {
                        shift ^= 0x80;
                    }
                    if let Some(mut w) = keyboard::key_word(scan, shift) {
                        // Pavé gris (E0) : ASCII E0h, comme un BIOS de clavier MF2.
                        if extended && (0x47..=0x53).contains(&scan) {
                            w = (w & 0xFF00) | 0xE0;
                        }
                        kb_push(mem, w);
                    }
                }
            }
        }
        mem.write_u8(bda::SHIFT, shift);
    }
}

fn int16(cpu: &mut Cpu, mem: &GuestMem) -> Flow {
    let ah = cpu.hi(AX);
    match ah {
        // Lecture bloquante : sans touche, l'appel est repris après HLT.
        0x00 | 0x10 => match read_key(mem) {
            Some(word) => cpu.set_w(AX, if ah == 0 { classic(word) } else { word }),
            None => return Flow::Wait,
        },
        0x01 | 0x11 => {
            let word = peek_key(mem);
            if let Some(w) = word {
                cpu.set_w(AX, if ah == 1 { classic(w) } else { w });
            }
            set_ret_flag(cpu, mem, ZF, word.is_none());
        }
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
        _ => return Flow::Unhandled(ah),
    }
    Flow::Continue
}
