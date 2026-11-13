// SPDX-License-Identifier: GPL-3.0-or-later
//! Pilote souris en HLE (INT 33h, compatible Microsoft 8.05) : position
//! virtuelle (640 × 200 en texte et CGA), boutons, compteurs de mickeys,
//! curseur texte (attribut inversé). Les mouvements arrivent de l'hôte
//! (`event`). Le gestionnaire utilisateur (AX=000Ch) est enregistré mais
//! pas encore appelé.

use crate::hle::{AX, BX, CX, DX, ES, Flow, Regs};
use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Taille de l'état du pilote pour AX=0015h–0017h.
const STATE_SIZE: usize = 24;

#[derive(Clone, Copy, Default)]
struct ButtonLog {
    count: u16,
    x: i32,
    y: i32,
}

pub struct Mouse {
    x: i32,
    y: i32,
    buttons: u8,
    /// Curseur visible quand le compteur vaut 0 (masqué à -1 après reset).
    shown: i32,
    min: (i32, i32),
    max: (i32, i32),
    /// Mickeys pour 8 pixels (horizontal, vertical).
    ratio: (i32, i32),
    /// Compteurs de mouvement depuis le dernier AX=000Bh.
    motion: (i32, i32),
    /// Reste de conversion mickeys → pixels.
    frac: (i32, i32),
    presses: [ButtonLog; 3],
    releases: [ButtonLog; 3],
    /// Gestionnaire utilisateur : masque d'événements, adresse.
    handler: (u16, u16, u16),
    /// Cellule texte sous le curseur affiché, et son attribut d'origine.
    drawn: Option<(usize, u8)>,
}

impl Default for Mouse {
    fn default() -> Self {
        Self::new()
    }
}

impl Mouse {
    pub fn new() -> Self {
        Self {
            x: 320,
            y: 100,
            buttons: 0,
            shown: -1,
            min: (0, 0),
            max: (639, 199),
            ratio: (8, 16),
            motion: (0, 0),
            frac: (0, 0),
            presses: [ButtonLog::default(); 3],
            releases: [ButtonLog::default(); 3],
            handler: (0, 0, 0),
            drawn: None,
        }
    }

    fn clamp(&mut self) {
        self.x = self.x.clamp(self.min.0, self.max.0);
        self.y = self.y.clamp(self.min.1, self.max.1);
    }

    /// Mouvement relatif (mickeys) et état des boutons venus de l'hôte.
    pub fn event(&mut self, mem: &GuestMem, dx: i32, dy: i32, buttons: u8) {
        self.motion.0 += dx;
        self.motion.1 += dy;
        let px = self.frac.0 + dx * 8;
        let py = self.frac.1 + dy * 8;
        self.x += px / self.ratio.0;
        self.y += py / self.ratio.1;
        self.frac = (px % self.ratio.0, py % self.ratio.1);
        self.clamp();
        for b in 0..3 {
            let (was, now) = (self.buttons >> b & 1, buttons >> b & 1);
            let log = match (was, now) {
                (0, 1) => &mut self.presses[b],
                (1, 0) => &mut self.releases[b],
                _ => continue,
            };
            log.count = log.count.wrapping_add(1);
            (log.x, log.y) = (self.x, self.y);
        }
        self.buttons = buttons;
        self.redraw(mem);
    }

    /// Bloc d'état lu par l'INT 33h de la ROM (AX=0003h sans sortie de VM).
    fn publish(&self, mem: &GuestMem) {
        let at = crate::bios::MOUSE_STATE;
        mem.write_u16(at, u16::from(self.buttons));
        mem.write_u16(at + 2, self.x as u16);
        mem.write_u16(at + 4, self.y as u16);
    }

    /// Curseur texte : attribut inversé de la cellule sous la souris ; bloc
    /// d'état de la ROM tenu à jour (appelé après chaque changement).
    fn redraw(&mut self, mem: &GuestMem) {
        self.publish(mem);
        if let Some((at, attr)) = self.drawn.take() {
            mem.write_u8(at, attr);
        }
        let mode = mem.read_u8(0x449);
        if self.shown < 0 || !(mode <= 3 || mode == 7) {
            return;
        }
        let cols = usize::from(mem.read_u16(0x44A)).max(1);
        // 40 colonnes : 16 pixels virtuels par cellule.
        let cw = if cols == 40 { 16 } else { 8 };
        let base = if mode == 7 { 0xB_0000 } else { 0xB_8000 } + usize::from(mem.read_u16(0x44E));
        let at = base + ((self.y as usize / 8) * cols + self.x as usize / cw) * 2 + 1;
        let attr = mem.read_u8(at);
        mem.write_u8(at, attr ^ 0x77);
        self.drawn = Some((at, attr));
    }

    /// État sauvegardé par AX=0016h, en mots de 16 bits.
    fn state(&self) -> [u16; STATE_SIZE / 2] {
        let v = [self.x, self.y, self.shown, self.min.0, self.min.1, self.max.0, self.max.1, self.ratio.0, self.ratio.1];
        let mut out = [0u16; STATE_SIZE / 2];
        for (o, x) in out.iter_mut().zip(v) {
            *o = x as u16;
        }
        (out[9], out[10], out[11]) = self.handler;
        out
    }

    fn set_state(&mut self, s: &[u16]) {
        let i = |k: usize| i32::from(s[k] as i16);
        (self.x, self.y, self.shown) = (i(0), i(1), i(2).min(0));
        (self.min, self.max, self.ratio) = ((i(3), i(4)), (i(5), i(6)), (i(7).max(1), i(8).max(1)));
        self.handler = (s[9], s[10], s[11]);
        self.clamp();
    }

    pub fn int33(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Flow {
        let ax = cpu.w(AX);
        match ax {
            // Réinitialisation : souris présente, deux boutons.
            0x0000 | 0x0021 => {
                let handler = self.handler;
                if let Some((at, attr)) = self.drawn.take() {
                    mem.write_u8(at, attr);
                }
                *self = Self::new();
                self.handler = if ax == 0x21 { handler } else { (0, 0, 0) };
                if mem.read_u8(0x484) == 49 {
                    self.max.1 = 399;
                }
                cpu.set_w(AX, 0xFFFF);
                cpu.set_w(BX, 2);
                self.publish(mem);
            }
            0x0001 => {
                self.shown = (self.shown + 1).min(0);
                self.redraw(mem);
            }
            0x0002 => {
                self.shown -= 1;
                self.redraw(mem);
            }
            0x0003 => {
                cpu.set_w(BX, u16::from(self.buttons));
                cpu.set_w(CX, self.x as u16);
                cpu.set_w(DX, self.y as u16);
            }
            0x0004 => {
                self.x = i32::from(cpu.w(CX) as i16);
                self.y = i32::from(cpu.w(DX) as i16);
                self.clamp();
                self.redraw(mem);
            }
            0x0005 | 0x0006 => {
                let b = usize::from(cpu.w(BX)).min(2);
                let log = if ax == 5 { &mut self.presses[b] } else { &mut self.releases[b] };
                let l = std::mem::take(log);
                cpu.set_w(AX, u16::from(self.buttons));
                cpu.set_w(BX, l.count);
                cpu.set_w(CX, l.x as u16);
                cpu.set_w(DX, l.y as u16);
            }
            0x0007 | 0x0008 => {
                let (a, b) = (i32::from(cpu.w(CX) as i16), i32::from(cpu.w(DX) as i16));
                let (lo, hi) = (a.min(b), a.max(b));
                if ax == 7 {
                    (self.min.0, self.max.0) = (lo, hi);
                } else {
                    (self.min.1, self.max.1) = (lo, hi);
                }
                self.clamp();
                self.redraw(mem);
            }
            // Forme du curseur texte : le nôtre reste l'attribut inversé.
            0x000A => {}
            0x000B => {
                let (dx, dy) = std::mem::take(&mut self.motion);
                cpu.set_w(CX, dx as u16);
                cpu.set_w(DX, dy as u16);
            }
            0x000C => self.handler = (cpu.w(CX), cpu.w(DX), cpu.sreg(ES)),
            0x000F => {
                self.ratio = (i32::from(cpu.w(CX)).max(1), i32::from(cpu.w(DX)).max(1));
            }
            0x0014 => {
                let old = self.handler;
                self.handler = (cpu.w(CX), cpu.w(DX), cpu.sreg(ES));
                cpu.set_w(CX, old.0);
                cpu.set_w(DX, old.1);
                cpu.set_sreg(ES, old.2);
            }
            // Sensibilité, seuil de vitesse double : acceptés.
            0x0013 | 0x001A | 0x001D => {}
            0x001B => {
                cpu.set_w(BX, 50);
                cpu.set_w(CX, 50);
                cpu.set_w(DX, 50);
            }
            0x001E => cpu.set_w(BX, 0),
            // Taille, sauvegarde et restauration de l'état du pilote (ES:DX).
            0x0015 => cpu.set_w(BX, STATE_SIZE as u16),
            0x0016 | 0x0017 => {
                let at = (usize::from(cpu.sreg(ES)) << 4) + usize::from(cpu.w(DX));
                if ax == 0x16 {
                    for (i, w) in self.state().into_iter().enumerate() {
                        mem.write_u16(at + 2 * i, w);
                    }
                } else {
                    let s: Vec<u16> = (0..STATE_SIZE / 2).map(|i| mem.read_u16(at + 2 * i)).collect();
                    self.set_state(&s);
                    self.redraw(mem);
                }
            }
            // Extensions SWIFT de Logitech (CyberMan, sondées par Doom) :
            // absentes, AX inchangé comme sous un pilote qui les ignore.
            0x53C1 => {}
            // Version 8.05, souris PS/2.
            0x0024 => {
                cpu.set_w(BX, 0x0805);
                cpu.set_w(CX, 0x0400);
            }
            _ => return Flow::Unhandled(cpu.hi(AX)),
        }
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_and_buttons() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        mem.write_u8(0x449, 3);
        mem.write_u16(0x44A, 80);
        let mut m = Mouse::new();
        let mut cpu = Cpu::real_mode(0, 0, 0);
        cpu.set_w(AX, 0);
        m.int33(&mut cpu, &mem);
        assert_eq!(cpu.w(AX), 0xFFFF);
        m.event(&mem, 16, 16, 1);
        m.event(&mem, 0, 0, 0);
        cpu.set_w(AX, 3);
        m.int33(&mut cpu, &mem);
        assert_eq!((cpu.w(CX), cpu.w(DX), cpu.w(BX)), (336, 108, 0));
        cpu.set_w(AX, 5);
        cpu.set_w(BX, 0);
        m.int33(&mut cpu, &mem);
        assert_eq!((cpu.w(BX), cpu.w(CX)), (1, 336));
        // Curseur texte affiché : attribut inversé en (13, 42), rétabli au masquage.
        let at = 0xB_8000 + (13 * 80 + 42) * 2 + 1;
        mem.write_u8(at, 0x07);
        cpu.set_w(AX, 1);
        m.int33(&mut cpu, &mem);
        assert_eq!(mem.read_u8(at), 0x70);
        cpu.set_w(AX, 2);
        m.int33(&mut cpu, &mem);
        assert_eq!(mem.read_u8(at), 0x07);
    }
}

/// Souris PS/2 vue par l'interface du BIOS (INT 15h AX=C2xxh), pour les
/// pilotes d'un vrai DOS (CuteMouse) : mouvements cumulés, puis un paquet
/// par IRQ 12, que le gestionnaire INT 74h de la ROM remet à la routine
/// enregistrée par le pilote (AL=07h).
#[derive(Default)]
pub struct Ps2 {
    pub enabled: bool,
    pub handler: bool,
    dx: i32,
    dy: i32,
    buttons: u8,
    /// Mouvement ou boutons non encore livrés.
    dirty: bool,
    /// IRQ 12 levée, paquet pas encore lu par la ROM.
    in_flight: bool,
}

impl Ps2 {
    pub fn event(&mut self, dx: i32, dy: i32, buttons: u8) {
        self.dx += dx;
        self.dy += dy;
        self.dirty |= dx != 0 || dy != 0 || buttons != self.buttons;
        self.buttons = buttons;
    }

    /// Un paquet attend et le pilote l'accepte : vrai une fois par paquet.
    pub fn take_irq(&mut self) -> bool {
        let go = self.enabled && self.handler && self.dirty && !self.in_flight;
        self.in_flight |= go;
        go
    }

    /// Paquet pour la ROM : (état, X, Y) au format PS/2 (Y vers le haut,
    /// signes dans l'octet d'état), ou `None` si rien n'est à livrer.
    pub fn packet(&mut self) -> Option<(u16, u16, u16)> {
        self.in_flight = false;
        if !self.dirty || !self.enabled || !self.handler {
            return None;
        }
        let x = self.dx.clamp(-256, 255);
        let y = (-self.dy).clamp(-256, 255);
        self.dx -= x;
        self.dy += y;
        self.dirty = self.dx != 0 || self.dy != 0;
        let status = (self.buttons & 7) | 0x08 | (u8::from(x < 0) << 4) | (u8::from(y < 0) << 5);
        Some((u16::from(status), (x & 0xFF) as u16, (y & 0xFF) as u16))
    }
}

#[cfg(test)]
mod ps2_tests {
    use super::Ps2;

    #[test]
    fn packets() {
        let mut p = Ps2 { enabled: true, handler: true, ..Ps2::default() };
        p.event(300, 5, 1);
        assert!(p.take_irq());
        assert!(!p.take_irq());
        // X saturé à 255, Y vers le haut (−5 : signe en bit 5).
        assert_eq!(p.packet(), Some((0x29, 255, 0xFB)));
        assert!(p.take_irq());
        assert_eq!(p.packet(), Some((0x09, 45, 0)));
        assert_eq!(p.packet(), None);
    }
}
