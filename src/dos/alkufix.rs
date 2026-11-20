// SPDX-License-Identifier: GPL-3.0-or-later
//! Fin de l'ouverture de Second Reality (partie ALKU) sur processeur rapide.
//!
//! Après le dernier générique, la boucle principale d'ALKU appelle
//! `do_scroll(0)`, qui rend la main sans attendre la trame, et incrémente à
//! chaque tour son compteur `f` (16 bits). Sur un processeur rapide (le vCPU
//! en fait des millions par seconde, un 486 déjà des centaines de milliers),
//! `f` parcourt toute sa plage pendant les ≈ 1,7 s qui restent : il relance
//! au passage de 0 et de 50 les fondus du copper (`textin`, `textout`), et sa
//! valeur au moment du test de sortie (`f > 7` : fondu final) est livrée au
//! hasard. Une passe sur deux environ, la partie suivante démarre sur la
//! palette d'un fondu inachevé : paysage aux couleurs criardes, bords gris.
//!
//! Correctif en mémoire, déclenché comme celui de CRT par l'installation du
//! gestionnaire d'INT 0 (fait par le démarrage de Borland C) : l'argument
//! `0` de cet appel devient `2`. `do_scroll(2)` attend la trame comme
//! `do_scroll(1)` mais sans redessiner le texte : la boucle avance d'un pas
//! de défilement par tour, comme pendant les génériques, `f` vaut ≈ 30 à la
//! sortie et le fondu final a lieu.

use crate::mem::GuestMem;

use super::crtfix::find;
use super::process::{psp, psp_addr};

/// Fin de la boucle principale (`None` = segment de l'appel lointain).
const PATTERN: [Option<u8>; 29] = {
    const fn b(v: u8) -> Option<u8> {
        Some(v)
    }
    [
        b(0xFF), b(0x46), b(0xFC), // inc word [bp-4]           (f++)
        b(0x83), b(0x3E), b(0xA2), b(0x00), b(0x05), // cmp word [0A2h],5  (tptr)
        b(0x7D), b(0x04), // jge
        b(0x6A), b(0x01), // push 1
        b(0xEB), b(0x02), // jmp
        b(0x6A), b(0x00), // push 0
        b(0x9A), b(0x12), b(0x14), None, None, // call far do_scroll
        b(0x44), b(0x44), // inc sp ×2
        b(0x81), b(0x3E), b(0x9E), b(0x00), b(0x40), b(0x01), // cmp word [9Eh],140h (a)
    ]
};
/// Argument de `push 0`.
const PATCH_AT: usize = 15;

/// Programme dont le PSP est `seg` : boucle d'ALKU corrigée dans son bloc
/// de mémoire. Rend le nombre de corrections.
pub fn patch(mem: &GuestMem, seg: u16) -> usize {
    let start = usize::from(seg) << 4;
    let top = usize::from(mem.read_u16(psp_addr(seg, psp::MEM_TOP))).min(0xA000) << 4;
    if top <= start + PATTERN.len() {
        return 0;
    }
    let mut image = vec![0u8; top - start];
    mem.read(start, &mut image);
    let found = find(&image, &PATTERN);
    for &i in &found {
        mem.write_u8(start + i + PATCH_AT, 2);
    }
    found.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alku_end_loop_waits_for_frames() {
        let mem = GuestMem::new(0x10_0000).unwrap();
        let seg = 0x1000u16;
        mem.write_u16(psp_addr(seg, psp::MEM_TOP), 0x2000);
        // Code d'ALKU.EXE (2344:0469 dans Second Reality).
        let code = [
            0xFF, 0x46, 0xFC, 0x83, 0x3E, 0xA2, 0x00, 0x05, 0x7D, 0x04, 0x6A, 0x01, 0xEB, 0x02, 0x6A, 0x00, 0x9A, 0x12, 0x14, 0x44,
            0x23, 0x44, 0x44, 0x81, 0x3E, 0x9E, 0x00, 0x40, 0x01,
        ];
        let at = 0x1_3469;
        mem.write(at, &code);
        assert_eq!(patch(&mem, seg), 1);
        let mut got = [0u8; 29];
        mem.read(at, &mut got);
        let mut want = code;
        want[PATCH_AT] = 2;
        assert_eq!(got, want);
        // Déjà corrigé : plus rien à faire.
        assert_eq!(patch(&mem, seg), 0);
    }
}
