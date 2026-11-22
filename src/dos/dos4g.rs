// SPDX-License-Identifier: GPL-3.0-or-later
//! Programmes liés à l'amorce de DOS/4G(W) qui ne fournissent pas
//! `DOS4GW.EXE` : l'amorce charge l'extendeur en recouvrement (AH=4Bh AL=03h)
//! et l'appelle par l'interface privée de Rational (INT 21h AH=FFh), que
//! DOS/32A, servi sous le nom `DOS4GW.EXE`, ne comprend pas (« syntax is
//! DOS32A <execname.xxx> »). DOS/32A lit en revanche l'en-tête LE du
//! programme lui-même : on le lance donc à la place de l'amorce, avec le
//! programme pour argument, comme `DOS32A PROG.EXE` à la main.

/// Exécutable LE/LX dont l'amorce MZ nomme `dos4gw.exe` (l'extendeur est
/// donc cherché sur le disque, et non lié au programme).
pub fn needs_extender(image: &[u8]) -> bool {
    if !image.starts_with(b"MZ") {
        return false;
    }
    let Some(new) = image.get(0x3C..0x40).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize) else { return false };
    if new < 0x40 || !matches!(image.get(new..new + 2), Some(b"LE" | b"LX")) {
        return false;
    }
    image[..new].windows(10).any(|w| w.eq_ignore_ascii_case(b"dos4gw.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(stub: &[u8], sig: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8; 0x100];
        v[..2].copy_from_slice(b"MZ");
        v[0x40..0x40 + stub.len()].copy_from_slice(stub);
        v[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        v[0x80..0x80 + sig.len()].copy_from_slice(sig);
        v
    }

    #[test]
    fn stub_naming_the_extender() {
        assert!(needs_extender(&le(b"DOS4GW.EXE", b"LE")));
        assert!(needs_extender(&le(b"xdos4gw.exe", b"LX")));
    }

    #[test]
    fn other_programs_untouched() {
        assert!(!needs_extender(&le(b"nothing here", b"LE")));
        assert!(!needs_extender(&le(b"dos4gw.exe", b"PE")));
        assert!(!needs_extender(b"MZ"));
        assert!(!needs_extender(b"\x7fELF"));
    }
}
