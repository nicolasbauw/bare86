// SPDX-License-Identifier: GPL-3.0-or-later
//! Opérations arithmétiques et calcul des drapeaux.

pub const CF: u32 = 1 << 0;
pub const PF: u32 = 1 << 2;
pub const AF: u32 = 1 << 4;
pub const ZF: u32 = 1 << 6;
pub const SF: u32 = 1 << 7;
pub const TF: u32 = 1 << 8;
pub const IF: u32 = 1 << 9;
pub const DF: u32 = 1 << 10;
pub const OF: u32 = 1 << 11;
pub const RF: u32 = 1 << 16;
pub const AC: u32 = 1 << 18;
pub const ARITH: u32 = CF | PF | AF | ZF | SF | OF;
/// Bits modifiables par POPF 16 bits en mode réel (CF…OF, IOPL, NT).
pub const POPF16_MASK: u32 = 0x7FD5;
/// Bits modifiables par POPFD en mode réel (+ AC, ID).
pub const POPF32_MASK: u32 = 0x0025_7FD5;

pub fn mask(size: u32) -> u32 {
    if size >= 4 { u32::MAX } else { (1u32 << (8 * size)) - 1 }
}

pub fn sign(size: u32) -> u32 {
    1u32 << (8 * size - 1)
}

/// Extension de signe de `v` (taille `size` octets).
pub fn sext(v: u32, size: u32) -> i64 {
    match size {
        1 => i64::from(v as u8 as i8),
        2 => i64::from(v as u16 as i16),
        _ => i64::from(v as i32),
    }
}

/// ZF, SF, PF du résultat.
pub fn szp(res: u32, size: u32) -> u32 {
    let res = res & mask(size);
    let mut f = 0;
    if res == 0 {
        f |= ZF;
    }
    if res & sign(size) != 0 {
        f |= SF;
    }
    if (res as u8).count_ones().is_multiple_of(2) {
        f |= PF;
    }
    f
}

pub fn add(a: u32, b: u32, c: u32, size: u32) -> (u32, u32) {
    let (a, b) = (a & mask(size), b & mask(size));
    let wide = u64::from(a) + u64::from(b) + u64::from(c);
    let res = wide as u32 & mask(size);
    let mut f = szp(res, size);
    if wide > u64::from(mask(size)) {
        f |= CF;
    }
    if (a ^ res) & (b ^ res) & sign(size) != 0 {
        f |= OF;
    }
    if (a ^ b ^ res) & 0x10 != 0 {
        f |= AF;
    }
    (res, f)
}

pub fn sub(a: u32, b: u32, c: u32, size: u32) -> (u32, u32) {
    let (a, b) = (a & mask(size), b & mask(size));
    let res = a.wrapping_sub(b).wrapping_sub(c) & mask(size);
    let mut f = szp(res, size);
    if u64::from(a) < u64::from(b) + u64::from(c) {
        f |= CF;
    }
    if (a ^ b) & (a ^ res) & sign(size) != 0 {
        f |= OF;
    }
    if (a ^ b ^ res) & 0x10 != 0 {
        f |= AF;
    }
    (res, f)
}

pub fn logic(res: u32, size: u32) -> (u32, u32) {
    let res = res & mask(size);
    (res, szp(res, size))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Rol,
    Ror,
    Rcl,
    Rcr,
    Shl,
    Shr,
    Sar,
}

/// Décalage ou rotation ; `count` déjà masqué sur 5 bits et non nul.
/// Renvoie (résultat, drapeaux, drapeaux touchés).
pub fn shift(op: Shift, a: u32, count: u32, cf_in: bool, size: u32) -> (u32, u32, u32) {
    let w = 8 * size;
    let m = mask(size);
    let a = a & m;
    let msb = |v: u32| v & sign(size) != 0;
    let bit = |b: bool, f: u32| if b { f } else { 0 };
    match op {
        Shift::Shl => {
            let res = if count >= 32 { 0 } else { ((u64::from(a) << count) as u32) & m };
            let cf = count <= w && (u64::from(a) >> (w - count)) & 1 != 0;
            (res, szp(res, size) | bit(cf, CF) | bit(msb(res) != cf, OF), ARITH)
        }
        Shift::Shr => {
            let res = if count >= w { 0 } else { a >> count };
            let cf = count <= w && (a >> (count - 1)) & 1 != 0;
            (res, szp(res, size) | bit(cf, CF) | bit(msb(a), OF), ARITH)
        }
        Shift::Sar => {
            let s = sext(a, size);
            let res = (s >> count.min(63)) as u32 & m;
            let cf = (s >> (count - 1).min(63)) & 1 != 0;
            (res, szp(res, size) | bit(cf, CF), ARITH)
        }
        Shift::Rol => {
            let c = count % w;
            let res = if c == 0 { a } else { ((a << c) | (a >> (w - c))) & m };
            let cf = res & 1 != 0;
            (res, bit(cf, CF) | bit(msb(res) != cf, OF), CF | OF)
        }
        Shift::Ror => {
            let c = count % w;
            let res = if c == 0 { a } else { ((a >> c) | (a << (w - c))) & m };
            let cf = msb(res);
            let second = res & (sign(size) >> 1) != 0;
            (res, bit(cf, CF) | bit(cf != second, OF), CF | OF)
        }
        Shift::Rcl | Shift::Rcr => {
            let c = if size == 4 { count } else { count % (w + 1) };
            let full = (1u64 << (w + 1)) - 1;
            let v = (u64::from(cf_in) << w) | u64::from(a);
            let r = match (c, op) {
                (0, _) => v,
                (_, Shift::Rcl) => ((v << c) | (v >> (w + 1 - c))) & full,
                _ => ((v >> c) | (v << (w + 1 - c))) & full,
            };
            let res = r as u32 & m;
            let cf = (r >> w) & 1 != 0;
            let of = if op == Shift::Rcl { msb(res) != cf } else { msb(a) != cf_in };
            (res, bit(cf, CF) | bit(of, OF), CF | OF)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_add_sub() {
        assert_eq!(add(0xFF, 1, 0, 1), (0, ZF | CF | AF | PF));
        assert_eq!(add(0x7F, 1, 0, 1), (0x80, SF | OF | AF));
        assert_eq!(sub(0, 1, 0, 2), (0xFFFF, SF | CF | AF | PF));
        assert_eq!(sub(0x8000, 1, 0, 2), (0x7FFF, OF | AF | PF));
        assert_eq!(logic(0x0F0, 1), (0xF0, SF | PF));
    }

    #[test]
    fn rotations() {
        assert_eq!(shift(Shift::Rol, 0x81, 1, false, 1).0, 0x03);
        assert_eq!(shift(Shift::Rcl, 0x80, 1, false, 1), (0x00, CF | OF, CF | OF));
        assert_eq!(shift(Shift::Rcr, 0x01, 1, true, 1).0, 0x80);
        assert_eq!(shift(Shift::Sar, 0x8000, 15, false, 2).0, 0xFFFF);
    }
}
