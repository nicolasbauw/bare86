// SPDX-License-Identifier: GPL-3.0-or-later
//! Protocole entre la `glide2x.dll` de Bare86 (guest/glide2x/glide2x.c) et
//! l'hôte. La DLL empile des commandes dans un tampon de mots de 32 bits, puis
//! exécute `out 0E2h, al` avec EBX = adresse du tampon (déplacement dans DS),
//! ECX = longueur en octets. Chaque commande : un en-tête
//! `(opcode << 16) | nombre de mots d'arguments`, puis les arguments (entiers
//! ou flottants). Au retour, EAX = valeur de la dernière commande (1 = succès
//! pour `INIT`). Les deux fichiers doivent rester d'accord sur ces numéros.

/// Port d'appel de Glide.
pub const PORT: u16 = 0xE2;

pub const INIT: u16 = 1;
/// résolution, origine, format de couleur
pub const WIN_OPEN: u16 = 2;
pub const WIN_CLOSE: u16 = 3;
/// intervalle de balayage
pub const SWAP: u16 = 4;
/// couleur, alpha, profondeur
pub const CLEAR: u16 = 5;
pub const RENDER_BUFFER: u16 = 6;
pub const ORIGIN: u16 = 7;
/// minx, miny, maxx, maxy
pub const CLIP: u16 = 8;
/// fonction, facteur, local, autre, inversion
pub const COLOR_COMBINE: u16 = 9;
pub const ALPHA_COMBINE: u16 = 10;
/// tmu, fonction rgb, facteur rgb, fonction alpha, facteur alpha, inversions
pub const TEX_COMBINE: u16 = 11;
/// rgb source, rgb destination, alpha source, alpha destination
pub const BLEND: u16 = 12;
pub const ALPHA_TEST_FUNC: u16 = 13;
pub const ALPHA_TEST_REF: u16 = 14;
pub const CONSTANT_COLOR: u16 = 15;
pub const CULL: u16 = 16;
pub const DEPTH_BIAS: u16 = 17;
pub const DEPTH_FUNC: u16 = 18;
pub const DEPTH_MODE: u16 = 19;
pub const DEPTH_MASK: u16 = 20;
/// rgb, alpha
pub const COLOR_MASK: u16 = 21;
pub const FOG_COLOR: u16 = 22;
pub const FOG_MODE: u16 = 23;
/// 64 octets (16 mots)
pub const FOG_TABLE: u16 = 24;
/// tmu, s, t
pub const TEX_CLAMP: u16 = 25;
/// tmu, réduction, agrandissement
pub const TEX_FILTER: u16 = 26;
/// tmu, mode, mélange de LOD
pub const TEX_MIPMAP: u16 = 27;
/// tmu, adresse, pair/impair, petit LOD, grand LOD, rapport, format
pub const TEX_SOURCE: u16 = 28;
/// tmu, adresse, pair/impair, petit LOD, grand LOD, rapport, format, octets, données…
pub const TEX_DOWNLOAD: u16 = 29;
/// tmu, adresse, ce LOD, grand LOD, rapport, format, pair/impair, première
/// ligne, dernière ligne, octets, données…
pub const TEX_DOWNLOAD_PARTIAL: u16 = 30;
/// type (palette, NCC0, NCC1), données…
pub const TEX_TABLE: u16 = 31;
/// 3 sommets de `VERTEX_WORDS` flottants
pub const TRIANGLE: u16 = 32;
pub const LINE: u16 = 33;
pub const POINT: u16 = 34;
/// identifiant (grGlideGetState)
pub const SAVE_STATE: u16 = 35;
/// identifiant (grGlideSetState)
pub const RESTORE_STATE: u16 = 36;
/// tampon, origine, adresse, pas en octets : l'hôte y écrit l'image en 565
pub const LFB_READ: u16 = 37;
/// tampon, mode d'écriture, origine, adresse, pas en octets : l'hôte relit
pub const LFB_WRITE: u16 = 38;
/// genre (0 exp, 1 exp2, 2 linéaire), paramètre 1, paramètre 2, adresse de
/// la table de 64 octets que l'hôte remplit
pub const FOG_GENERATE: u16 = 39;
pub const SHUTDOWN: u16 = 40;
/// correction gamma de la sortie vidéo (flottant) ; 1,7 à l'ouverture
pub const GAMMA: u16 = 41;

/// Sommet transmis : x, y, ooz, oow, r, g, b, a, sow, tow, oow (TMU 0).
pub const VERTEX_WORDS: usize = 11;
