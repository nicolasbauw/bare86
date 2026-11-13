// SPDX-License-Identifier: GPL-3.0-or-later
//! Horloge temps réel et CMOS MC146818 (ports 70h, 71h) : heure locale de
//! l'hôte en BCD, registres d'état, configuration (mémoire, équipement)
//! avec sa somme de contrôle. Les écritures dans les registres d'heure sont
//! ignorées (l'horloge hôte fait foi) ; les autres sont conservées.

use crate::io::IoDevice;

pub struct Rtc {
    index: u8,
    cmos: [u8; 128],
}

fn bcd(v: i32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

impl Rtc {
    pub fn new(extended_kb: usize) -> Self {
        let mut cmos = [0u8; 128];
        cmos[0x0A] = 0x26;
        // 24 h, BCD.
        cmos[0x0B] = 0x02;
        cmos[0x0D] = 0x80;
        // Équipement : coprocesseur, VGA (0 = EGA/VGA), pas de lecteur.
        cmos[0x14] = 0x02;
        cmos[0x15..0x17].copy_from_slice(&640u16.to_le_bytes());
        let ext = (extended_kb.min(0xFFFF) as u16).to_le_bytes();
        cmos[0x17..0x19].copy_from_slice(&ext);
        cmos[0x30..0x32].copy_from_slice(&ext);
        let sum: u16 = cmos[0x10..0x2E].iter().map(|&b| u16::from(b)).sum();
        cmos[0x2E..0x30].copy_from_slice(&sum.to_be_bytes());
        Self { index: 0, cmos }
    }

    fn read_reg(&mut self, reg: u8) -> u8 {
        let t = crate::bios::local_time();
        match reg {
            0x00 => bcd(t.tm_sec),
            0x02 => bcd(t.tm_min),
            0x04 => bcd(t.tm_hour),
            0x06 => bcd(t.tm_wday + 1),
            0x07 => bcd(t.tm_mday),
            0x08 => bcd(t.tm_mon + 1),
            0x09 => bcd(t.tm_year % 100),
            0x32 => bcd((t.tm_year + 1900) / 100),
            // Mise à jour en cours pendant les 244 dernières µs de chaque seconde.
            0x0A => {
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                self.cmos[0x0A] | if ns >= 999_756_000 { 0x80 } else { 0 }
            }
            // Indicateurs d'interruption : remis à zéro par la lecture.
            0x0C => std::mem::take(&mut self.cmos[0x0C]),
            r => self.cmos[usize::from(r)],
        }
    }
}

impl IoDevice for Rtc {
    fn read(&mut self, port: u16, _: u8) -> u32 {
        if port == 0x71 { u32::from(self.read_reg(self.index)) } else { 0xFF }
    }

    fn write(&mut self, port: u16, _: u8, value: u32) {
        let v = value as u8;
        if port == 0x70 {
            // Bit 7 : masque de la NMI, sans objet ici.
            self.index = v & 0x7F;
            return;
        }
        match self.index {
            0x00..=0x09 | 0x32 | 0x0C | 0x0D => {}
            r => self.cmos[usize::from(r)] = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers() {
        let mut r = Rtc::new(15 * 1024);
        r.write(0x70, 1, 0x17);
        assert_eq!(r.read(0x71, 1), 0x00);
        r.write(0x70, 1, 0x18);
        assert_eq!(r.read(0x71, 1), 0x3C);
        r.write(0x70, 1, 0x0B);
        assert_eq!(r.read(0x71, 1), 0x02);
        r.write(0x70, 1, 0x04);
        let h = r.read(0x71, 1) as u8;
        assert!(h >> 4 <= 2 && h & 15 <= 9, "{h:02X}");
        // Octet libre conservé, registre d'heure ignoré.
        r.write(0x70, 1, 0x40);
        r.write(0x71, 1, 0x5A);
        assert_eq!(r.read(0x71, 1), 0x5A);
    }
}
