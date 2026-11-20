// SPDX-License-Identifier: GPL-3.0-or-later
//! « Runtime error 200 » de l'unité CRT de Turbo/Borland Pascal 7.
//!
//! À l'initialisation, CRT compte les tours de sa boucle d'attente pendant
//! un tic de 55 ms et divise le compte (DX:AX) par 55 pour obtenir celui
//! d'une milliseconde. Au-delà de 55 × 65 536 tours par tic (tout processeur
//! plus rapide qu'un Pentium 200 environ, et le vCPU à la vitesse de
//! l'hôte), le quotient ne tient plus sur 16 bits : exception de division,
//! que le programme rapporte en « Runtime error 200 » avant même de
//! démarrer (Second Reality, de nombreux jeux et utilitaires).
//!
//! Correctif à la manière de TPPATCH, mais en mémoire : quand le programme
//! installe son gestionnaire d'INT 0 (`int 21h AX=2500h`, toujours fait par
//! le runtime avant l'initialisation des unités), la séquence de CRT est
//! cherchée dans son bloc de mémoire et `not ax; not dx; mov cx,55; div cx`
//! devient un appel lointain à `crt_delay_fix` (asm/bios.S), qui plafonne
//! le compte à FFFFh. Le contexte exigé (`mov ax,FFE4h; cwd; call …` avant,
//! `mov [DelayCnt],ax` après) écarte les données qui ressembleraient à la
//! séquence (parties compressées de Second Reality).

use crate::mem::GuestMem;

use super::process::{psp, psp_addr};

/// Adresse de `crt_delay_fix` dans la ROM (asm/bios.S).
const FIX_SEG: u16 = crate::bios::ROM_SEG;
const FIX_OFF: u16 = 0xEAF0;

/// Séquence de CRT : `None` = octet quelconque (cible du `call`, adresse
/// de `DelayCnt`). Le remplacement porte sur `PATCH_AT..PATCH_AT + 9`.
const PATTERN: [Option<u8>; 17] = {
    const fn b(v: u8) -> Option<u8> {
        Some(v)
    }
    [
        b(0xB8), b(0xE4), b(0xFF), // mov ax,0FFE4h
        b(0x99), // cwd
        b(0xE8), None, None, // call DelayLoop
        b(0xF7), b(0xD0), // not ax
        b(0xF7), b(0xD2), // not dx
        b(0xB9), b(0x37), b(0x00), // mov cx,55
        b(0xF7), b(0xF1), // div cx
        b(0xA3), // mov [DelayCnt],ax
    ]
};
const PATCH_AT: usize = 7;

/// Programme dont le PSP est `seg` : séquences de CRT corrigées dans son
/// bloc de mémoire (du PSP à la fin de mémoire notée en PSP:02h, au plus
/// jusqu'à A0000h). Rend le nombre de corrections.
pub fn patch(mem: &GuestMem, seg: u16) -> usize {
    let start = usize::from(seg) << 4;
    let top = usize::from(mem.read_u16(psp_addr(seg, psp::MEM_TOP))).min(0xA000) << 4;
    if top <= start + PATTERN.len() {
        return 0;
    }
    let mut image = vec![0u8; top - start];
    mem.read(start, &mut image);
    let mut call = [0x9A, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90];
    call[1..3].copy_from_slice(&FIX_OFF.to_le_bytes());
    call[3..5].copy_from_slice(&FIX_SEG.to_le_bytes());
    let found = find(&image, &PATTERN);
    for &i in &found {
        mem.write(start + i + PATCH_AT, &call);
    }
    found.len()
}

/// Positions de `pattern` dans `image` (`None` = octet quelconque).
pub(super) fn find(image: &[u8], pattern: &[Option<u8>]) -> Vec<usize> {
    image
        .windows(pattern.len())
        .enumerate()
        .filter(|(_, w)| w.iter().zip(pattern).all(|(&v, p)| p.is_none_or(|p| p == v)))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_crt_delay_calibration_only_in_context() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        let seg = 0x1000u16;
        mem.write_u16(psp_addr(seg, psp::MEM_TOP), 0x2000);
        // Code de CRT (Second Reality), et la même suite de 9 octets sans
        // son contexte (données compressées) : seule la première change.
        let code = [0xB8, 0xE4, 0xFF, 0x99, 0xE8, 0x3C, 0x02, 0xF7, 0xD0, 0xF7, 0xD2, 0xB9, 0x37, 0x00, 0xF7, 0xF1, 0xA3, 0xD4, 0xFC];
        let data = [0xB8, 0xE4, 0xFF, 0x99, 0xE8, 0x3C, 0x02, 0x00, 0x0C, 0xF7, 0xD0, 0xF7, 0xD2, 0xB9, 0x37, 0x00, 0xF7, 0xF1, 0xA3];
        let (at_code, at_data) = (0x1_0400, 0x1_8000);
        mem.write(at_code, &code);
        mem.write(at_data, &data);
        assert_eq!(patch(&mem, seg), 1);
        let mut got = [0u8; 19];
        mem.read(at_code, &mut got);
        assert_eq!(got[..7], code[..7]);
        assert_eq!(got[7..16], [0x9A, 0xF0, 0xEA, 0x00, 0xF0, 0x90, 0x90, 0x90, 0x90]);
        assert_eq!(got[16..], code[16..]);
        mem.read(at_data, &mut got);
        assert_eq!(got, data);
        // Déjà corrigé : plus rien à faire.
        assert_eq!(patch(&mem, seg), 0);
    }
}
