// SPDX-License-Identifier: GPL-3.0-or-later
//! Carte VGA : registres (séquenceur, contrôleur graphique, attributs,
//! DAC, divers, CRTC, état), mémoire vidéo et logique des plans (latches,
//! modes de lecture et d'écriture, opérations logiques, masque de bits).
//!
//! Mémoire : un memfd de 5 × 64 Ko, les 4 plans puis un tampon linéaire qui
//! fait foi en mode chain4 (13h) ; au passage chain4 ↔ non chaîné, le
//! contenu est réparti ou rassemblé (plan a & 3, décalage a & !3, comme le
//! câblage de la VGA). La fenêtre A0000 de l'invité est une vue de 64 Ko
//! sur ce fichier, que la machine fait pointer sur le tampon linéaire ou
//! sur un plan quand la configuration le permet (accès natifs, sans sortie
//! de VM) ; sinon les accès passent par `read` / `write` (MMIO,
//! interpréteur). Voir FAISABILITE.md, « décision sur la mémoire vidéo ».
//!
//! Texte et CGA restent en RAM ordinaire (B0000–BFFFF).

use std::time::Instant;

use crate::io::IoDevice;

pub const PLANE: usize = 0x1_0000;
/// Région du tampon linéaire chain4 dans le fichier.
pub const LINEAR: usize = 4;
const REGIONS: usize = 5;

/// Retours de trame (70 Hz) et de ligne (31,5 kHz) pour le registre d'état.
const FRAME_NS: u128 = 14_285_714;
const VRETRACE_NS: u128 = 2 * 31_778;
const LINE_NS: u128 = 31_778;
const HBLANK_NS: u128 = 6_356;

/// Mémoire vidéo dans un memfd, avec une vue complète pour le modèle et une
/// vue de 64 Ko pour la fenêtre de l'invité.
struct Vram {
    fd: i32,
    all: *mut u8,
    window: *mut u8,
    /// Région visée par la vue de la fenêtre.
    window_region: usize,
}

impl Vram {
    fn new() -> std::io::Result<Self> {
        // SAFETY: appels système vérifiés ; les vues vivent autant que Vram.
        unsafe {
            let fd = libc::memfd_create(c"vga-vram".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 || libc::ftruncate(fd, (REGIONS * PLANE) as i64) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let map = |len: usize, off: usize| {
                let p = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, off as i64);
                if p == libc::MAP_FAILED { Err(std::io::Error::last_os_error()) } else { Ok(p.cast::<u8>()) }
            };
            let all = map(REGIONS * PLANE, 0)?;
            let window = map(PLANE, 0)?;
            Ok(Self { fd, all, window, window_region: 0 })
        }
    }

    fn get(&self, region: usize, off: usize) -> u8 {
        debug_assert!(region < REGIONS && off < PLANE);
        // SAFETY: dans le mappage ; le vCPU est arrêté pendant les accès hôte.
        unsafe { self.all.add(region * PLANE + off).read_volatile() }
    }

    fn set(&self, region: usize, off: usize, v: u8) {
        debug_assert!(region < REGIONS && off < PLANE);
        // SAFETY: idem.
        unsafe { self.all.add(region * PLANE + off).write_volatile(v) }
    }

    /// Fait pointer la vue de la fenêtre sur `region` (même adresse hôte).
    fn point_window(&mut self, region: usize) -> std::io::Result<()> {
        if region == self.window_region {
            return Ok(());
        }
        // SAFETY: remplace la vue existante, même adresse et même taille.
        let p = unsafe {
            libc::mmap(
                self.window.cast(),
                PLANE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED,
                self.fd,
                (region * PLANE) as i64,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        self.window_region = region;
        Ok(())
    }
}

impl Drop for Vram {
    fn drop(&mut self) {
        // SAFETY: mappages et descripteur possédés.
        unsafe {
            libc::munmap(self.all.cast(), REGIONS * PLANE);
            libc::munmap(self.window.cast(), PLANE);
            libc::close(self.fd);
        }
    }
}

/// Façon dont la fenêtre A0000 peut être servie dans la configuration courante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Chain4 (13h) : le tampon linéaire, en lecture et écriture natives.
    Linear,
    /// Non chaîné, écriture simple vers ce seul plan. Une lecture native
    /// rendrait ce plan même si le plan de lecture diffère : la machine ne
    /// mappe alors que si aucune lecture n'a été vue dans cette configuration.
    Plane(usize),
    /// Logique des plans nécessaire : MMIO et interpréteur.
    Emulated,
}

pub struct Vga {
    vram: Vram,
    latch: [u8; 4],
    pub misc: u8,
    seq_index: u8,
    pub seq: [u8; 8],
    gc_index: u8,
    pub gc: [u8; 16],
    attr_index: u8,
    /// Bascule index/donnée du port 3C0h (remise à « index » par 3DAh).
    attr_data: bool,
    pub attr: [u8; 32],
    crtc_index: u8,
    pub crtc: [u8; 32],
    pub dac: [[u8; 3]; 256],
    pub dac_mask: u8,
    dac_write: (u8, usize),
    dac_read: (u8, usize),
    /// Dernière opération sur le DAC : 3 = écriture, 0 = lecture (3C7h).
    dac_state: u8,
    cga_mode: u8,
    cga_color: u8,
    start: Instant,
    /// Incrémenté à chaque changement de registre qui touche l'accès mémoire.
    pub epoch: u64,
    /// Chain4 au dernier changement de configuration (bascule du contenu).
    chain4_was: bool,
}

impl Vga {
    pub fn new() -> std::io::Result<Self> {
        let mut v = Self {
            vram: Vram::new()?,
            latch: [0; 4],
            misc: 0x67,
            seq_index: 0,
            seq: [0x03, 0x00, 0x03, 0x00, 0x02, 0, 0, 0],
            gc_index: 0,
            gc: [0, 0, 0, 0, 0, 0x10, 0x0E, 0x00, 0xFF, 0, 0, 0, 0, 0, 0, 0],
            attr_index: 0,
            attr_data: false,
            attr: [0; 32],
            crtc_index: 0,
            crtc: [0; 32],
            dac: default_dac(),
            dac_mask: 0xFF,
            dac_write: (0, 0),
            dac_read: (0, 0),
            dac_state: 0,
            cga_mode: 0x29,
            cga_color: 0x30,
            start: Instant::now(),
            epoch: 0,
            chain4_was: false,
        };
        for i in 0..16 {
            v.attr[i] = i as u8;
        }
        v.attr[0x10] = 0x0C;
        v.attr[0x12] = 0x0F;
        Ok(v)
    }

    /// Registre de couleur CGA (3D9h) : fond, palette, intensité.
    pub fn cga_color(&self) -> u8 {
        self.cga_color
    }

    /// Plan lu en mode de lecture 0.
    pub fn read_plane(&self) -> usize {
        usize::from(self.gc[4] & 3)
    }

    pub fn chain4(&self) -> bool {
        self.seq[4] & 0x08 != 0
    }

    /// Adresse hôte de la vue de la fenêtre (slot KVM de A0000).
    pub fn window_host_addr(&self) -> u64 {
        self.vram.window as u64
    }

    /// Accès possible à la fenêtre dans la configuration courante.
    pub fn access(&self) -> Access {
        if self.chain4() {
            return Access::Linear;
        }
        let mask = self.seq[2] & 0x0F;
        let simple = self.gc[5] & 0x0B == 0 // mode d'écriture 0, lecture 0
            && self.gc[3] == 0              // sans rotation, remplacement
            && self.gc[1] & 0x0F == 0       // set/reset inactif
            && self.gc[8] == 0xFF           // tous les bits
            && self.gc[5] & 0x10 == 0       // pas de pair/impair
            && self.seq[4] & 0x04 != 0;
        if simple && mask.count_ones() == 1 {
            Access::Plane(mask.trailing_zeros() as usize)
        } else {
            Access::Emulated
        }
    }

    /// Oriente la vue de la fenêtre pour `access` (sans effet si Emulated).
    pub fn point_window(&mut self, access: Access) -> std::io::Result<()> {
        match access {
            Access::Linear => self.vram.point_window(LINEAR),
            Access::Plane(p) => self.vram.point_window(p),
            Access::Emulated => Ok(()),
        }
    }

    /// Après un changement de registre : bascule chain4 ↔ plans si besoin.
    fn config_changed(&mut self) {
        self.epoch += 1;
        let c4 = self.chain4();
        if c4 != self.chain4_was {
            for a in 0..PLANE {
                let (p, o) = (a & 3, a & !3);
                if c4 {
                    self.vram.set(LINEAR, a, self.vram.get(p, o));
                } else {
                    self.vram.set(p, o, self.vram.get(LINEAR, a));
                }
            }
            self.chain4_was = c4;
        }
    }

    // ---- Mémoire (accès émulés) ----

    pub fn read(&mut self, off: usize) -> u8 {
        if self.chain4() {
            return self.vram.get(LINEAR, off);
        }
        for p in 0..4 {
            self.latch[p] = self.vram.get(p, off);
        }
        if self.gc[5] & 0x08 == 0 {
            return self.latch[usize::from(self.gc[4] & 3)];
        }
        // Mode de lecture 1 : comparaison de couleur.
        let mut diff = 0u8;
        for p in 0..4 {
            if self.gc[7] >> p & 1 != 0 {
                let want = if self.gc[2] >> p & 1 != 0 { 0xFF } else { 0x00 };
                diff |= self.latch[p] ^ want;
            }
        }
        !diff
    }

    pub fn write(&mut self, off: usize, v: u8) {
        if self.chain4() {
            if self.seq[2] >> (off & 3) & 1 != 0 {
                self.vram.set(LINEAR, off, v);
            }
            return;
        }
        let rot = self.gc[3] & 7;
        let func = self.gc[3] >> 3 & 3;
        let sr = self.gc[0];
        let esr = self.gc[1];
        let mut bitmask = self.gc[8];
        let rotated = v.rotate_right(u32::from(rot));
        let mode = self.gc[5] & 3;
        if mode == 3 {
            bitmask &= rotated;
        }
        for p in 0..4 {
            if self.seq[2] >> p & 1 == 0 {
                continue;
            }
            let latch = self.latch[p];
            let fill = |bit: u8| if bit & 1 != 0 { 0xFFu8 } else { 0x00 };
            let x = match mode {
                0 => {
                    if esr >> p & 1 != 0 {
                        fill(sr >> p)
                    } else {
                        rotated
                    }
                }
                1 => {
                    self.vram.set(p, off, latch);
                    continue;
                }
                2 => fill(v >> p),
                _ => fill(sr >> p),
            };
            let x = match func {
                0 => x,
                1 => x & latch,
                2 => x | latch,
                _ => x ^ latch,
            };
            self.vram.set(p, off, (x & bitmask) | (latch & !bitmask));
        }
    }

    /// Octet d'un plan (rendu, tests).
    pub fn plane_byte(&self, plane: usize, off: usize) -> u8 {
        self.vram.get(plane, off)
    }

    /// Octet du tampon linéaire chain4 (rendu du mode 13h, tests).
    pub fn linear_byte(&self, off: usize) -> u8 {
        self.vram.get(LINEAR, off)
    }

    // ---- Ports ----

    fn status(&mut self) -> u8 {
        self.attr_data = false;
        let t = self.start.elapsed().as_nanos();
        let vretrace = t % FRAME_NS >= FRAME_NS - VRETRACE_NS;
        let hblank = t % LINE_NS >= LINE_NS - HBLANK_NS;
        u8::from(vretrace || hblank) | if vretrace { 0x08 } else { 0 }
    }

    /// Ports CRTC et état : 3Dxh en couleur (misc bit 0), 3Bxh en mono.
    fn crtc_port(&self, port: u16) -> bool {
        let color = self.misc & 1 != 0;
        (port & 0xFFF0 == 0x3D0) == color
    }

    pub fn port_read(&mut self, port: u16) -> u8 {
        match port {
            0x3C0 => self.attr_index,
            0x3C1 => self.attr[usize::from(self.attr_index & 0x1F)],
            0x3C2 => 0x10,
            0x3C4 => self.seq_index,
            0x3C5 => self.seq[usize::from(self.seq_index & 7)],
            0x3C6 => self.dac_mask,
            0x3C7 => self.dac_state,
            0x3C8 => self.dac_write.0,
            0x3C9 => {
                let (i, c) = self.dac_read;
                let v = self.dac[usize::from(i)][c];
                self.dac_read = if c == 2 { (i.wrapping_add(1), 0) } else { (i, c + 1) };
                v
            }
            0x3CC => self.misc,
            0x3CE => self.gc_index,
            0x3CF => self.gc[usize::from(self.gc_index & 15)],
            0x3B4 | 0x3D4 => self.crtc_index,
            0x3B5 | 0x3D5 => self.crtc[usize::from(self.crtc_index & 31)],
            0x3BA | 0x3DA if self.crtc_port(port) => self.status(),
            0x3D8 => self.cga_mode,
            0x3D9 => self.cga_color,
            _ => 0xFF,
        }
    }

    pub fn port_write(&mut self, port: u16, v: u8) {
        match port {
            0x3C0 => {
                if self.attr_data {
                    let i = usize::from(self.attr_index & 0x1F);
                    if i < 0x15 {
                        self.attr[i] = v;
                    }
                } else {
                    self.attr_index = v & 0x3F;
                }
                self.attr_data = !self.attr_data;
            }
            0x3C2 => self.misc = v,
            0x3C4 => self.seq_index = v,
            0x3C5 => {
                self.seq[usize::from(self.seq_index & 7)] = v;
                self.config_changed();
            }
            0x3C6 => self.dac_mask = v,
            0x3C7 => {
                self.dac_read = (v, 0);
                self.dac_state = 0;
            }
            0x3C8 => {
                self.dac_write = (v, 0);
                self.dac_state = 3;
            }
            0x3C9 => {
                let (i, c) = self.dac_write;
                self.dac[usize::from(i)][c] = v & 0x3F;
                self.dac_write = if c == 2 { (i.wrapping_add(1), 0) } else { (i, c + 1) };
            }
            0x3CE => self.gc_index = v,
            0x3CF => {
                self.gc[usize::from(self.gc_index & 15)] = v;
                self.config_changed();
            }
            0x3B4 | 0x3D4 => self.crtc_index = v,
            0x3B5 | 0x3D5 => {
                let i = usize::from(self.crtc_index & 31);
                // Registres 0–7 protégés en écriture (bit 7 du registre 11h), sauf le bit 4 de 07h.
                if i < 8 && self.crtc[0x11] & 0x80 != 0 {
                    if i == 7 {
                        self.crtc[7] = (self.crtc[7] & !0x10) | (v & 0x10);
                    }
                } else {
                    self.crtc[i] = v;
                }
            }
            0x3D8 => self.cga_mode = v,
            0x3D9 => self.cga_color = v,
            _ => {}
        }
    }

    /// Mode VGA standard programmé par l'INT 10h (registres du BIOS).
    pub fn set_mode(&mut self, mode: u8, clear: bool) {
        let p = mode_params(mode);
        self.misc = p.misc;
        self.seq[..5].copy_from_slice(&p.seq);
        self.gc[..9].copy_from_slice(&p.gc);
        self.crtc[..25].copy_from_slice(&p.crtc);
        self.attr[..21].copy_from_slice(&p.attr);
        self.attr_data = false;
        self.dac = default_dac();
        // Modes 16 couleurs et texte : palette EGA de 64 couleurs dans le DAC.
        if mode != 0x13 {
            for (i, c) in self.dac.iter_mut().enumerate().take(64) {
                let comp = |hi: usize, lo: usize| ((i >> hi & 1) * 0x2A + (i >> lo & 1) * 0x15) as u8;
                *c = [comp(2, 5), comp(1, 4), comp(0, 3)];
            }
        }
        self.dac_mask = 0xFF;
        self.chain4_was = self.chain4();
        self.epoch += 1;
        if clear {
            for r in 0..REGIONS {
                for o in 0..PLANE {
                    self.vram.set(r, o, 0);
                }
            }
        }
    }
}

struct ModeParams {
    misc: u8,
    seq: [u8; 5],
    gc: [u8; 9],
    crtc: [u8; 25],
    attr: [u8; 21],
}

/// Registres des modes standard (valeurs de référence du BIOS VGA, cf. FreeVGA).
fn mode_params(mode: u8) -> ModeParams {
    let ega_attr = |mode_ctl: u8| {
        let mut a = [0u8; 21];
        for (i, v) in [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x14, 0x07, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F]
            .iter()
            .enumerate()
        {
            a[i] = *v;
        }
        a[0x10] = mode_ctl;
        a[0x12] = 0x0F;
        a
    };
    match mode {
        0x13 => ModeParams {
            misc: 0x63,
            seq: [0x03, 0x01, 0x0F, 0x00, 0x0E],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x05, 0x0F, 0xFF],
            crtc: [
                0x5F, 0x4F, 0x50, 0x82, 0x54, 0x80, 0xBF, 0x1F, 0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9C,
                0x0E, 0x8F, 0x28, 0x40, 0x96, 0xB9, 0xA3, 0xFF,
            ],
            attr: {
                let mut a = [0u8; 21];
                for (i, x) in a.iter_mut().enumerate().take(16) {
                    *x = i as u8;
                }
                a[0x10] = 0x41;
                a[0x12] = 0x0F;
                a
            },
        },
        0x0D | 0x0E | 0x10 | 0x12 => ModeParams {
            misc: if mode == 0x12 { 0xE3 } else { 0x63 },
            seq: [0x03, if mode == 0x0D { 0x09 } else { 0x01 }, 0x0F, 0x00, 0x06],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x0F, 0xFF],
            crtc: match mode {
                0x12 => [
                    0x5F, 0x4F, 0x50, 0x82, 0x54, 0x80, 0x0B, 0x3E, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0xEA, 0x8C, 0xDF, 0x28, 0x00, 0xE7, 0x04, 0xE3, 0xFF,
                ],
                0x10 => [
                    0x5F, 0x4F, 0x50, 0x82, 0x54, 0x80, 0xBF, 0x1F, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x83, 0x85, 0x5D, 0x28, 0x0F, 0x63, 0xBA, 0xE3, 0xFF,
                ],
                _ => [
                    if mode == 0x0D { 0x2D } else { 0x5F },
                    if mode == 0x0D { 0x27 } else { 0x4F },
                    if mode == 0x0D { 0x28 } else { 0x50 },
                    0x90,
                    if mode == 0x0D { 0x2B } else { 0x54 },
                    0x80,
                    0xBF,
                    0x1F,
                    0x00,
                    0xC0,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x9C,
                    0x8E,
                    0x8F,
                    if mode == 0x0D { 0x14 } else { 0x28 },
                    0x00,
                    0x96,
                    0xB9,
                    0xE3,
                    0xFF,
                ],
            },
            attr: ega_attr(0x01),
        },
        // Modes texte et CGA : la mémoire est la RAM B0000–BFFFF.
        _ => ModeParams {
            misc: if mode == 7 { 0x66 } else { 0x67 },
            seq: [0x03, if mode <= 1 { 0x08 } else { 0x00 }, 0x03, 0x00, 0x02],
            gc: [0x00, 0x00, 0x00, 0x00, 0x00, 0x10, if mode == 7 { 0x0A } else { 0x0E }, 0x00, 0xFF],
            crtc: [
                0x5F, 0x4F, 0x50, 0x82, 0x55, 0x81, 0xBF, 0x1F, 0x00, 0x4F, 0x0D, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x9C,
                0x8E, 0x8F, 0x28, 0x1F, 0x96, 0xB9, 0xA3, 0xFF,
            ],
            attr: ega_attr(0x0C),
        },
    }
}

/// Palette par défaut du DAC (6 bits) : 16 couleurs EGA, 16 gris, puis une
/// roue de teintes en 3 saturations × 3 intensités, comme le BIOS VGA.
pub fn default_dac() -> [[u8; 3]; 256] {
    let mut d = [[0u8; 3]; 256];
    for (i, c) in d.iter_mut().enumerate().take(16) {
        let (r, g, b) = (i >> 2 & 1, i >> 1 & 1, i & 1);
        let hi = if i & 8 != 0 { 0x15 } else { 0 };
        *c = [(r * 0x2A + hi) as u8, (g * 0x2A + hi) as u8, (b * 0x2A + hi) as u8];
        if i == 6 {
            *c = [0x2A, 0x15, 0x00];
        }
    }
    const GRAYS: [u8; 16] = [0, 5, 8, 11, 14, 17, 20, 24, 28, 32, 36, 40, 45, 50, 56, 63];
    for (i, g) in GRAYS.iter().enumerate() {
        d[16 + i] = [*g; 3];
    }
    // Roue de 24 teintes : montée et descente de chaque composante.
    let wheel = |hi: u8, lo: u8| -> [[u8; 3]; 24] {
        let steps = [lo, lo + (hi - lo) / 4, lo + (hi - lo) / 2, lo + 3 * (hi - lo) / 4, hi];
        let mut w = [[0u8; 3]; 24];
        let mut k = 0;
        // Bleu → magenta → rouge → jaune → vert → cyan.
        for &s in &steps[..4] {
            w[k] = [s, lo, hi];
            k += 1;
        }
        for &s in steps[1..].iter().rev() {
            w[k] = [hi, lo, s];
            k += 1;
        }
        for &s in &steps[1..4] {
            w[k] = [hi, s, lo];
            k += 1;
        }
        for &s in steps[1..].iter().rev() {
            w[k] = [s, hi, lo];
            k += 1;
        }
        for &s in &steps[1..4] {
            w[k] = [lo, hi, s];
            k += 1;
        }
        for &s in steps[1..].iter().rev() {
            w[k] = [lo, s, hi];
            k += 1;
        }
        w
    };
    let mut at = 32;
    for (hi, levels) in [(63u8, [0u8, 31, 45]), (28, [0, 14, 20]), (16, [0, 8, 11])] {
        for lo in levels {
            for c in wheel(hi, lo) {
                if at < 248 {
                    d[at] = c;
                    at += 1;
                }
            }
        }
    }
    d
}

/// Ports 3B4h–3DFh.
pub struct VgaPorts(pub std::rc::Rc<std::cell::RefCell<Vga>>);

impl IoDevice for VgaPorts {
    fn read(&mut self, port: u16, size: u8) -> u32 {
        let mut v = self.0.borrow_mut();
        let lo = u32::from(v.port_read(port));
        if size == 2 { lo | u32::from(v.port_read(port + 1)) << 8 } else { lo }
    }

    fn write(&mut self, port: u16, size: u8, value: u32) {
        let mut v = self.0.borrow_mut();
        v.port_write(port, value as u8);
        // OUT 16 bits sur un index : index puis donnée (port suivant).
        if size == 2 {
            v.port_write(port + 1, (value >> 8) as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &mut Vga, port: u16, idx: u8, val: u8) {
        v.port_write(port, idx);
        v.port_write(port + 1, val);
    }

    #[test]
    fn write_modes_and_latches() {
        let mut v = Vga::new().unwrap();
        v.set_mode(0x12, true);
        // Mode 0, set/reset sur les plans 0 et 2 (couleur 5), masque de bits 0F.
        set(&mut v, 0x3CE, 0, 0x05);
        set(&mut v, 0x3CE, 1, 0x0F);
        set(&mut v, 0x3CE, 8, 0x0F);
        v.read(10);
        v.write(10, 0x00);
        assert_eq!([0, 1, 2, 3].map(|p| v.plane_byte(p, 10)), [0x0F, 0x00, 0x0F, 0x00]);
        // Mode 1 : copie des latches de l'octet 10 vers l'octet 20.
        set(&mut v, 0x3CE, 5, 0x01);
        v.read(10);
        v.write(20, 0xAA);
        assert_eq!([0, 1, 2, 3].map(|p| v.plane_byte(p, 20)), [0x0F, 0x00, 0x0F, 0x00]);
        // Mode 2 : couleur 0Ah dans les bits F0 seulement.
        set(&mut v, 0x3CE, 5, 0x02);
        set(&mut v, 0x3CE, 8, 0xF0);
        v.read(10);
        v.write(10, 0x0A);
        assert_eq!([0, 1, 2, 3].map(|p| v.plane_byte(p, 10)), [0x0F, 0xF0, 0x0F, 0xF0]);
        // Lecture mode 1 : bits de couleur 5 (plans 0 et 2) dans l'octet 20.
        set(&mut v, 0x3CE, 5, 0x08);
        set(&mut v, 0x3CE, 2, 0x05);
        set(&mut v, 0x3CE, 7, 0x0F);
        assert_eq!(v.read(20), 0x0F);
        // XOR avec les latches, mode 0 simple.
        set(&mut v, 0x3CE, 5, 0x00);
        set(&mut v, 0x3CE, 1, 0x00);
        set(&mut v, 0x3CE, 3, 0x18);
        set(&mut v, 0x3CE, 8, 0xFF);
        set(&mut v, 0x3C4, 2, 0x01);
        v.read(20);
        v.write(20, 0xFF);
        assert_eq!(v.plane_byte(0, 20), 0xF0);
    }

    #[test]
    fn access_and_chain4_switch() {
        let mut v = Vga::new().unwrap();
        v.set_mode(0x13, true);
        assert_eq!(v.access(), Access::Linear);
        v.write(5, 0x42);
        v.write(6, 0x43);
        // Passage en mode X : chain4 coupé, le pixel 5 est dans le plan 1 à 4.
        set(&mut v, 0x3C4, 4, 0x06);
        assert_eq!(v.plane_byte(1, 4), 0x42);
        assert_eq!(v.plane_byte(2, 4), 0x43);
        assert_eq!(v.access(), Access::Emulated);
        set(&mut v, 0x3C4, 2, 0x04);
        assert_eq!(v.access(), Access::Plane(2));
        assert_eq!(v.read_plane(), 0);
        set(&mut v, 0x3C4, 2, 0x0F);
        assert_eq!(v.access(), Access::Emulated);
    }

    #[test]
    fn window_view_follows_region() {
        let mut v = Vga::new().unwrap();
        v.set_mode(0x13, true);
        v.point_window(Access::Plane(3)).unwrap();
        // SAFETY: lecture de la vue de 64 Ko.
        unsafe { v.vram.window.add(7).write_volatile(0x99) };
        assert_eq!(v.plane_byte(3, 7), 0x99);
        v.point_window(Access::Linear).unwrap();
        assert_eq!(unsafe { v.vram.window.add(7).read_volatile() }, v.linear_byte(7));
    }
}
