// SPDX-License-Identifier: GPL-3.0-or-later
//! Carte VGA : registres (séquenceur, contrôleur graphique, attributs,
//! DAC, divers, CRTC, état), mémoire vidéo et logique des plans (latches,
//! modes de lecture et d'écriture, opérations logiques, masque de bits).
//!
//! Mémoire : un memfd de 6 × 64 Ko, les 4 plans, un tampon linéaire qui
//! fait foi en mode chain4 (13h), puis une scène pour les plans, suivis du
//! framebuffer SVGA de 4 Mo (modes VBE : fenêtre par banques de 64 Ko en
//! A0000, framebuffer linéaire mappé par la machine à `LFB_ADDR`) ; au passage chain4 ↔ non chaîné, le
//! contenu est réparti ou rassemblé (plan a & 3, décalage a & !3, comme le
//! câblage de la VGA). La fenêtre A0000 de l'invité est une vue de 64 Ko
//! sur ce fichier, que la machine fait pointer sur le tampon linéaire ou
//! sur un plan quand la configuration le permet (accès natifs, sans sortie
//! de VM) ; sinon les accès passent par `read` / `write` (MMIO,
//! interpréteur). Pour un plan, la vue ne change pas de cible : elle reste
//! sur la scène, qui tient le contenu du plan choisi ; changer de plan
//! revient à deux copies de 64 Ko (scène → ancien plan, nouveau plan →
//! scène), sans appel système ni faute de page dans KVM. Voir docs/JOURNAL.md, « décision sur la mémoire vidéo ».
//!
//! Texte et CGA restent en RAM ordinaire (B0000–BFFFF).

use std::time::{Duration, Instant};

use crate::io::IoDevice;

pub const PLANE: usize = 0x1_0000;
/// Région du tampon linéaire chain4 dans le fichier.
pub const LINEAR: usize = 4;
/// Scène : tient le contenu du plan `Vram::staged`, qui y fait foi.
const STAGE: usize = 5;
const REGIONS: usize = 6;
/// Framebuffer SVGA, après les régions VGA (une banque = une région de 64 Ko).
pub const FB_SIZE: usize = 4 << 20;
const FB_REGION: usize = REGIONS;
const TOTAL: usize = REGIONS * PLANE + FB_SIZE;

/// Mode VBE en cours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vbe {
    pub mode: u16,
    pub width: usize,
    pub height: usize,
    pub bpp: usize,
    /// Octets par ligne logique (VBE 06h).
    pub pitch: usize,
    /// Début de l'affichage, en octets (VBE 07h).
    pub start: usize,
    /// Banque visible en A0000 (VBE 05h, en unités de 64 Ko).
    pub bank: usize,
    /// Mode demandé avec le framebuffer linéaire (bit 14).
    pub lfb: bool,
}

/// Balayage pour le registre d'état : lignes de 31,778 µs (≈ 31,5 kHz, tous
/// les modes VGA standard), blanc de ligne sur les 20 derniers % de chacune.
/// Géométrie verticale lue dans le CRTC (`Vga::vertical`) ; à défaut (CRTC
/// pas encore programmé, valeurs incohérentes), celle des modes à 400
/// lignes, 70 Hz : 449 lignes dont 400 affichées, synchronisation verticale
/// (bit 3) à partir de la ligne 412 (deux lignes sur la puce, élargies ici :
/// `VSYNC_MIN_LINES`).
const LINES: u128 = 449;
const VISIBLE_LINES: u128 = 400;
const VSYNC_LINES: std::ops::Range<u128> = 412..412 + VSYNC_MIN_LINES as u128;
const LINE_NS: u128 = 31_778;
/// Lignes au moins pendant lesquelles le bit 3 (synchronisation verticale)
/// reste levé. La puce suit le CRTC (deux lignes en mode 13h et en mode
/// 480 lignes, ≈ 64 µs), mais une lecture de port coûte ici ≈ 10 µs (sortie
/// de VM, contre ≈ 1 µs sur le bus ISA) : un gestionnaire qui voit le début
/// de l'impulsion et rend la main laissait la boucle principale la manquer
/// (Religion, scène 165–190 s : une image sur dix). Dix lignes (≈ 320 µs)
/// restent dans le blanc vertical de tous les modes (45 à 49 lignes).
const VSYNC_MIN_LINES: u16 = 10;
const HBLANK_NS: u128 = 6_356;

/// Mémoire vidéo dans un memfd, avec une vue complète pour le modèle et une
/// vue de 64 Ko pour la fenêtre de l'invité.
/// Voir `Vga::plane_mask_fast`.
pub struct PlaneMaskFast {
    pub seq2: *mut u8,
    pub read_intent: *mut u8,
    pub epoch: *mut u64,
    pub planes: [u64; 16],
}

struct Vram {
    fd: i32,
    all: *mut u8,
    window: *mut u8,
    /// Région visée par la vue de la fenêtre.
    window_region: usize,
    /// Plan dont le contenu est dans la scène.
    staged: Option<usize>,
}

impl Vram {
    fn new() -> std::io::Result<Self> {
        // SAFETY: appels système vérifiés ; les vues vivent autant que Vram.
        unsafe {
            let fd = libc::memfd_create(c"vga-vram".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 || libc::ftruncate(fd, TOTAL as i64) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let map = |len: usize, off: usize| {
                let p = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, off as i64);
                if p == libc::MAP_FAILED { Err(std::io::Error::last_os_error()) } else { Ok(p.cast::<u8>()) }
            };
            let all = map(TOTAL, 0)?;
            let window = map(PLANE, 0)?;
            Ok(Self { fd, all, window, window_region: 0, staged: None })
        }
    }

    /// Région où se trouve réellement le contenu de `region`.
    fn locate(&self, region: usize) -> usize {
        if self.staged == Some(region) { STAGE } else { region }
    }

    fn get(&self, region: usize, off: usize) -> u8 {
        debug_assert!(region < STAGE && off < PLANE);
        let region = self.locate(region);
        // SAFETY: dans le mappage ; le vCPU est arrêté pendant les accès hôte.
        unsafe { self.all.add(region * PLANE + off).read_volatile() }
    }

    fn set(&self, region: usize, off: usize, v: u8) {
        debug_assert!(region < STAGE && off < PLANE);
        let region = self.locate(region);
        // SAFETY: idem.
        unsafe { self.all.add(region * PLANE + off).write_volatile(v) }
    }

    /// Fait pointer la vue de la fenêtre sur `region` (même adresse hôte).
    fn point_window(&mut self, region: usize) -> std::io::Result<()> {
        // Un plan passe par la scène : la vue n'est remappée qu'une fois.
        let target = if region < LINEAR { STAGE } else { region };
        if region < LINEAR && self.staged != Some(region) {
            // SAFETY: régions distinctes du mappage complet ; le vCPU est arrêté.
            unsafe {
                let stage = self.all.add(STAGE * PLANE);
                if let Some(old) = self.staged {
                    std::ptr::copy_nonoverlapping(stage, self.all.add(old * PLANE), PLANE);
                }
                std::ptr::copy_nonoverlapping(self.all.add(region * PLANE), stage, PLANE);
            }
            self.staged = Some(region);
        }
        let region = target;
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
            libc::munmap(self.all.cast(), TOTAL);
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
    /// Mode VBE : la banque de 64 Ko du framebuffer, en accès natifs.
    Bank(usize),
    /// Non chaîné, écriture simple vers ce seul plan. Une lecture native
    /// rendrait ce plan même si le plan de lecture diffère : la machine ne
    /// mappe alors que si aucune lecture n'a été vue dans cette configuration.
    Plane(usize),
    /// Non chaîné, lecture simple de ce plan, différent du plan écrit, que
    /// le programme vient de choisir (il va lire) : mappé en lecture seule,
    /// les écritures sortent et sont émulées.
    PlaneRead(usize),
    /// Logique des plans nécessaire : MMIO et interpréteur.
    Emulated,
}

/// Retard maximal, après le début de l'impulsion de synchronisation verticale, avec lequel on
/// recale la date que voit le minuteur (au-delà, le programme a vraiment été retardé).
const EDGE_PEG_MAX_NS: u128 = 150_000;
/// Fenêtre, après la lecture qui a vu le front, où le minuteur montre la date du front.
const EDGE_PEG_WINDOW: Duration = Duration::from_micros(100);
/// Dernier front montant de l'impulsion verticale vu par le programme : (date du front, date de la lecture).
static EDGE: std::sync::Mutex<Option<(Instant, Instant)>> = std::sync::Mutex::new(None);

/// Date que voit le minuteur 8254 à `now` : pour les accès qui suivent de peu la lecture de 3DAh qui
/// a vu le front montant du retour vertical, la date de ce front. Un programme qui mesure la
/// période du balayage (arme le minuteur sur un front, le relit sur le suivant) trouve ainsi
/// toujours la même valeur, comme sur un PC où la scrutation prend une microseconde ; ici chaque
/// lecture de 3DAh coûte une sortie de VM (≈ 10 µs) et la période mesurée variait de ± 40 µs
/// (la démo MISG, qui refuse un écart, déclarait ne pas pouvoir se synchroniser).
pub fn timer_now(now: Instant) -> Instant {
    match *EDGE.lock().unwrap() {
        Some((edge, seen)) if now >= seen && now - seen < EDGE_PEG_WINDOW => edge,
        _ => now,
    }
}

pub struct Vga {
    /// Mode VBE en cours (sinon : modes VGA standard).
    pub vbe: Option<Vbe>,
    /// Mot bas de l'adresse de début d'affichage écrit par l'interface protégée VBE.
    vbe_pm_lo: usize,
    /// Écritures du registre 0Ch du CRTC (adresse de début, octet haut) :
    /// une par changement de page, d'où la cadence d'affichage d'un jeu.
    pub start_writes: u64,
    vram: Vram,
    latch: [u8; 4],
    /// Décalage + 1 de la dernière lecture faite directement dans un plan
    /// (traducteur dynamique) : les latches en seront rechargés avant usage.
    pending_latch: usize,
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
    /// DAC en mode 8 bits (VBE 4F08h) : les écritures et lectures de 3C9h portent 8 bits, le DAC en garde 6.
    pub dac8: bool,
    dac_read: (u8, usize),
    /// Dernière opération sur le DAC : 3 = écriture, 0 = lecture (3C7h).
    dac_state: u8,
    cga_mode: u8,
    cga_color: u8,
    start: Instant,
    /// Bit 3 (synchronisation verticale) vu à la dernière lecture de 3DAh.
    last_vsync: bool,
    /// Incrémenté à chaque changement de registre qui touche l'accès mémoire.
    pub epoch: u64,
    /// Chain4 au dernier changement de configuration (bascule du contenu).
    chain4_was: bool,
    /// Dernier registre de plan choisi : celui de lecture (Read Map) plutôt
    /// que le masque d'écriture (Map Mask). Voir `Access::PlaneRead`.
    read_intent: bool,
}

impl Vga {
    pub fn new() -> std::io::Result<Self> {
        let mut v = Self {
            vbe: None,
            vbe_pm_lo: 0,
            start_writes: 0,
            vram: Vram::new()?,
            latch: [0; 4],
            pending_latch: 0,
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
            dac8: false,
            dac_read: (0, 0),
            dac_state: 0,
            cga_mode: 0x29,
            cga_color: 0x30,
            start: Instant::now(),
            last_vsync: false,
            epoch: 0,
            chain4_was: false,
            read_intent: false,
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

    /// Adresse hôte du framebuffer SVGA (slot du framebuffer linéaire).
    pub fn fb_host_addr(&self) -> u64 {
        // SAFETY: dans le mappage complet.
        unsafe { self.vram.all.add(FB_REGION * PLANE) as u64 }
    }

    /// Framebuffer SVGA, en lecture (rendu).
    pub fn fb(&self) -> &[u8] {
        // SAFETY: dans le mappage complet, vivant autant que self ; le
        // contenu peut changer sous l'invité (rendu seulement).
        unsafe { std::slice::from_raw_parts(self.vram.all.add(FB_REGION * PLANE), FB_SIZE) }
    }

    /// Passe en mode VBE : registres d'un mode 256 couleurs (DAC compris),
    /// framebuffer effacé sauf si `clear` est faux.
    pub fn set_vbe(&mut self, v: Vbe, clear: bool) {
        self.set_mode(0x13, false);
        if clear {
            // SAFETY: région du framebuffer, dans le mappage complet.
            unsafe { std::ptr::write_bytes(self.vram.all.add(FB_REGION * PLANE), 0, FB_SIZE) };
        }
        self.vbe = Some(v);
        self.epoch += 1;
    }

    /// Accès possible à la fenêtre dans la configuration courante.
    pub fn access(&self) -> Access {
        if let Some(v) = &self.vbe {
            return Access::Bank(v.bank.min(FB_SIZE / PLANE - 1));
        }
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
        let read = usize::from(self.gc[4] & 3);
        let single = mask.count_ones() == 1;
        let written = mask.trailing_zeros() as usize;
        if !simple {
            Access::Emulated
        } else if single && written == read {
            Access::Plane(written)
        } else if self.read_intent {
            // Le dernier registre choisi est le plan de lecture : le
            // programme va lire (Doom relit l'écran plan par plan, le masque
            // d'écriture resté sur le dernier plan écrit).
            Access::PlaneRead(read)
        } else if single {
            Access::Plane(written)
        } else {
            Access::Emulated
        }
    }

    /// Adresse hôte où une écriture simple dans la fenêtre A0000 peut se
    /// faire directement (mode d'écriture 0 sans logique, un seul plan
    /// écrit) : le plan écrit, là où il se trouve (scène comprise). Pour le
    /// traducteur dynamique.
    /// Écriture du masque de plans (3C5h, index 2) faite par le traducteur
    /// dynamique sans passer par `port_write` : adresses des champs qu'elle
    /// modifie (comme `port_write` : `seq[2]`, `read_intent`, `epoch` ;
    /// `chain4` n'en dépend pas) et plan d'écriture direct de chacun des 16
    /// masques (0 : aucun). `None` si l'index courant n'est pas 2.
    pub fn plane_mask_fast(&mut self) -> Option<PlaneMaskFast> {
        if self.seq_index & 7 != 2 {
            return None;
        }
        let saved = self.seq[2];
        let mut planes = [0u64; 16];
        for (mask, p) in planes.iter_mut().enumerate() {
            self.seq[2] = mask as u8;
            *p = self.direct_write_window().map_or(0, |p| p as u64);
        }
        self.seq[2] = saved;
        Some(PlaneMaskFast { seq2: &mut self.seq[2], read_intent: &mut self.read_intent as *mut bool as *mut u8, epoch: &mut self.epoch, planes })
    }

    /// memfd de la VRAM, adresse de son mappage complet, régions des plans
    /// (planes, linéaire, scène) que `direct_*_window` peuvent rendre.
    pub fn vram_mapping(&self) -> (i32, u64, usize) {
        (self.vram.fd, self.vram.all as u64, REGIONS)
    }

    pub fn direct_write_window(&self) -> Option<*mut u8> {
        if self.vbe.is_some() || self.chain4() {
            return None;
        }
        let mask = self.seq[2] & 0x0F;
        let simple = self.gc[5] & 0x13 == 0 && self.gc[3] == 0 && self.gc[1] & 0x0F == 0 && self.gc[8] == 0xFF && self.seq[4] & 0x04 != 0;
        if !simple || mask.count_ones() != 1 {
            return None;
        }
        let region = self.vram.locate(mask.trailing_zeros() as usize);
        // SAFETY: région du mappage complet.
        Some(unsafe { self.vram.all.add(region * PLANE) })
    }

    /// Adresse hôte du plan lu, quand une lecture de la fenêtre peut s'y
    /// faire directement (configuration simple comme pour l'écriture
    /// directe, mode de lecture 0). Les latches suivent par `direct_read_at`.
    pub fn direct_read_window(&self) -> Option<*mut u8> {
        if self.vbe.is_some() || self.chain4() {
            return None;
        }
        let simple = self.gc[5] & 0x1B == 0 && self.gc[3] == 0 && self.gc[1] & 0x0F == 0 && self.gc[8] == 0xFF && self.seq[4] & 0x04 != 0;
        if !simple {
            return None;
        }
        let region = self.vram.locate(usize::from(self.gc[4] & 3));
        // SAFETY: région du mappage complet.
        Some(unsafe { self.vram.all.add(region * PLANE) })
    }

    /// Lecture directe faite à `off` par le traducteur : les latches en
    /// seront chargés avant le prochain usage (comme le ferait la carte).
    pub fn direct_read_at(&mut self, off: usize) {
        self.pending_latch = off + 1;
    }

    fn load_pending_latch(&mut self) {
        if self.pending_latch != 0 {
            let off = self.pending_latch - 1;
            self.pending_latch = 0;
            for p in 0..4 {
                self.latch[p] = self.vram.get(p, off);
            }
        }
    }

    /// Oriente la vue de la fenêtre pour `access` (sans effet si Emulated).
    pub fn point_window(&mut self, access: Access) -> std::io::Result<()> {
        match access {
            Access::Linear => self.vram.point_window(LINEAR),
            Access::Plane(p) | Access::PlaneRead(p) => self.vram.point_window(p),
            Access::Bank(b) => self.vram.point_window(FB_REGION + b),
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

    // ---- Accès directs de l'hôte (BIOS vidéo : pixels, caractères en mode graphique) ----

    /// Octet d'un plan (0–3) ou du tampon linéaire (`LINEAR`), sans passer par la logique d'écriture VGA.
    pub fn raw_get(&self, region: usize, off: usize) -> u8 {
        self.vram.get(region, off)
    }

    pub fn raw_set(&mut self, region: usize, off: usize, v: u8) {
        self.vram.set(region, off, v);
    }

    /// Signale que la mémoire vidéo a changé hors des accès du programme (le rendu redessine).
    pub fn touch(&mut self) {
        self.epoch += 1;
    }

    // ---- Mémoire (accès émulés) ----

    pub fn read(&mut self, off: usize) -> u8 {
        if self.chain4() {
            return self.vram.get(LINEAR, off);
        }
        self.pending_latch = 0;
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
        self.load_pending_latch();
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
    #[cfg(test)]
    pub fn plane_byte(&self, plane: usize, off: usize) -> u8 {
        self.vram.get(plane, off)
    }

    /// Octet du tampon linéaire chain4 (tests).
    #[cfg(test)]
    pub fn linear_byte(&self, off: usize) -> u8 {
        self.vram.get(LINEAR, off)
    }

    /// Contenu d'un plan (0–3) ou du tampon linéaire (`LINEAR`), pour le
    /// rendu : à lire vCPU arrêté, comme `plane_byte`.
    pub fn region(&self, region: usize) -> &[u8] {
        assert!(region < STAGE);
        let r = self.vram.locate(region);
        // SAFETY: dans le mappage complet (TOTAL octets), vivant autant que
        // `self` ; le vCPU est arrêté pendant le rendu.
        unsafe { std::slice::from_raw_parts(self.vram.all.add(r * PLANE), PLANE) }
    }

    // ---- Ports ----

    /// Géométrie verticale programmée dans le CRTC : lignes au total,
    /// lignes affichées, lignes de la synchronisation verticale. Les modes à
    /// 480 lignes (525 au total, 59,94 Hz) ne battent pas au rythme des
    /// modes à 400 lignes (449, 70 Hz) : Verses (EMF) cale son minuteur sur
    /// la trame et attend chaque blanc vertical, et ses scènes en 60 Hz
    /// tournaient à moitié de cadence avec une trame fixe à 70 Hz.
    fn vertical(&self) -> (u128, u128, std::ops::Range<u128>) {
        let c = &self.crtc;
        let ovf = u16::from(c[7]);
        let bits = |low: u8, b8: u16, b9: u16| u16::from(low) | (ovf >> b8 & 1) << 8 | (ovf >> b9 & 1) << 9;
        let total = bits(c[6], 0, 5) + 2;
        let visible = bits(c[0x12], 1, 6) + 1;
        let sync = bits(c[0x10], 2, 7);
        // Fin : 4 bits bas de la ligne qui suit la dernière de l'impulsion.
        let mut end = (sync & !0xF) | u16::from(c[0x11] & 0xF);
        if end <= sync {
            end += 16;
        }
        let end = end.max(sync + VSYNC_MIN_LINES);
        if !(200..=1024).contains(&total) || !(100..total).contains(&visible) || !(visible..total).contains(&sync) {
            return (LINES, VISIBLE_LINES, VSYNC_LINES);
        }
        (u128::from(total), u128::from(visible), u128::from(sync)..u128::from(end.min(total)))
    }

    fn status(&mut self) -> u8 {
        self.attr_data = false;
        let (lines, visible, vsync_lines) = self.vertical();
        let now = Instant::now();
        let t = now.saturating_duration_since(self.start).as_nanos();
        let line = t / LINE_NS % lines;
        let vblank = line >= visible;
        let vsync = vsync_lines.contains(&line);
        // Front montant vu par le programme : date réelle de l'impulsion, que le minuteur
        // montre pour les accès qui suivent aussitôt (`edge_time`).
        if vsync && !self.last_vsync {
            let since = (line - vsync_lines.start) * LINE_NS + t % LINE_NS;
            if since < EDGE_PEG_MAX_NS {
                let edge = now - Duration::from_nanos(since as u64);
                *EDGE.lock().unwrap() = Some((edge, now));
            }
        }
        self.last_vsync = vsync;
        let hblank = t % LINE_NS >= LINE_NS - HBLANK_NS;
        // Bit 0 : affichage inactif (blanc de ligne ou vertical) ; bit 3 : synchronisation verticale.
        u8::from(vblank || hblank) | if vsync { 0x08 } else { 0 }
    }

    /// Écriture sur un port de l'interface VBE protégée : banque, ou début
    /// d'affichage : adresse de 32 bits en dwords (CX = bits 0 à 15, DX = bits
    /// 16 à 31, fonction 07h du mode protégé, qui n'est pas celle de INT 10h) ;
    /// le mot haut, écrit en dernier, la fait prendre effet.
    fn vbe_pm_write(&mut self, port: u16, value: u16) {
        let Some(v) = &mut self.vbe else { return };
        match port {
            VBE_PM_WINDOW => v.bank = usize::from(value),
            VBE_PM_START_LO => self.vbe_pm_lo = usize::from(value),
            _ => v.start = ((usize::from(value) << 16) | self.vbe_pm_lo) * 4,
        }
    }

    /// Temps restant avant le prochain blanc vertical (nul pendant celui-ci).
    pub fn until_vblank(&self) -> std::time::Duration {
        let (lines, visible, _) = self.vertical();
        let t = self.start.elapsed().as_nanos() % (lines * LINE_NS);
        let start = visible * LINE_NS;
        std::time::Duration::from_nanos(start.saturating_sub(t) as u64)
    }

    /// Port d'état (3DAh en couleur, 3BAh en mono).
    pub fn is_status_port(&self, port: u16) -> bool {
        matches!(port, 0x3BA | 0x3DA) && self.crtc_port(port)
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
                let v = self.dac[usize::from(i)][c] << if self.dac8 { 2 } else { 0 };
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
                if self.seq_index & 7 == 2 {
                    self.read_intent = false;
                }
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
                self.dac[usize::from(i)][c] = (if self.dac8 { v >> 2 } else { v }) & 0x3F;
                self.dac_write = if c == 2 { (i.wrapping_add(1), 0) } else { (i, c + 1) };
            }
            0x3CE => self.gc_index = v,
            0x3CF => {
                self.gc[usize::from(self.gc_index & 15)] = v;
                if self.gc_index & 15 == 4 {
                    self.read_intent = true;
                }
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
                if i == 0x0C {
                    self.start_writes += 1;
                }
            }
            0x3D8 => self.cga_mode = v,
            0x3D9 => self.cga_color = v,
            _ => {}
        }
    }

    /// Mode VGA standard programmé par l'INT 10h (registres du BIOS).
    /// Hauteur de cellule d'un mode texte (INT 10h AH=11h avec reprogrammation) : lignes de balayage par
    /// caractère et forme du curseur.
    pub fn set_cell_height(&mut self, h: usize) {
        self.crtc[9] = (self.crtc[9] & 0xE0) | (h as u8 - 1);
        self.crtc[0x0A] = h as u8 - 2;
        self.crtc[0x0B] = h as u8 - 1;
        self.epoch += 1;
    }

    pub fn set_mode(&mut self, mode: u8, clear: bool) {
        self.vbe = None;
        let p = mode_params(mode);
        self.misc = p.misc;
        self.seq[..5].copy_from_slice(&p.seq);
        self.gc[..9].copy_from_slice(&p.gc);
        self.crtc[..25].copy_from_slice(&p.crtc);
        self.attr[..21].copy_from_slice(&p.attr);
        self.attr_data = false;
        self.dac = default_dac();
        self.dac8 = false; // un changement de mode remet le DAC à 6 bits
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
            for r in 0..STAGE {
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

/// Ports de l'interface VBE 2.0 en mode protégé (code de la ROM,
/// `asm/bios.S`) : fenêtre, puis début d'affichage (CX et DX de la fonction 07h).
pub const VBE_PM_WINDOW: u16 = 0x1D0;
pub const VBE_PM_START_LO: u16 = 0x1D2;
pub const VBE_PM_START_HI: u16 = 0x1D4;

impl IoDevice for VgaPorts {
    fn read(&mut self, port: u16, size: u8) -> u32 {
        let mut v = self.0.borrow_mut();
        if port == VBE_PM_WINDOW {
            return v.vbe.as_ref().map_or(0, |b| b.bank as u32);
        }
        let lo = u32::from(v.port_read(port));
        if size == 2 { lo | u32::from(v.port_read(port + 1)) << 8 } else { lo }
    }

    fn write(&mut self, port: u16, size: u8, value: u32) {
        let mut v = self.0.borrow_mut();
        if matches!(port, VBE_PM_WINDOW | VBE_PM_START_LO | VBE_PM_START_HI) {
            v.vbe_pm_write(port, value as u16);
            return;
        }
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
    fn dac_width_8_bits_scales_port_3c9() {
        let mut v = Vga::new().unwrap();
        // DAC à 6 bits (défaut) : la valeur est gardée telle quelle (6 bits).
        v.port_write(0x3C8, 5);
        v.port_write(0x3C9, 0x30);
        assert_eq!(v.dac[5][0], 0x30);
        // DAC à 8 bits (VBE 4F08h) : 0xC0 vaut 0x30 sur 6 bits, et la lecture rend 8 bits.
        v.dac8 = true;
        v.port_write(0x3C8, 6);
        v.port_write(0x3C9, 0xC0);
        assert_eq!(v.dac[6][0], 0x30);
        v.port_write(0x3C7, 6);
        assert_eq!(v.port_read(0x3C9), 0xC0);
        // Un changement de mode remet le DAC à 6 bits.
        v.set_mode(0x13, false);
        assert!(!v.dac8);
    }

    #[test]
    fn vbe_protected_mode_ports() {
        let mut v = Vga::new().unwrap();
        let mode = Vbe { mode: 0x101, width: 640, height: 480, bpp: 8, pitch: 640, start: 0, bank: 0, lfb: false };
        v.set_vbe(mode, true);
        v.vbe_pm_write(VBE_PM_WINDOW, 3);
        assert_eq!(v.vbe.as_ref().unwrap().bank, 3);
        // Adresse en dwords : mot bas puis mot haut (qui fait prendre effet).
        v.vbe_pm_write(VBE_PM_START_LO, 0x0010);
        v.vbe_pm_write(VBE_PM_START_HI, 0x0002);
        assert_eq!(v.vbe.as_ref().unwrap().start, ((2 << 16) | 0x10) * 4);
        // Hors mode VBE : sans effet.
        let mut w = Vga::new().unwrap();
        w.vbe_pm_write(VBE_PM_WINDOW, 3);
        assert!(w.vbe.is_none());
    }

    #[test]
    fn vertical_timing_follows_crtc() {
        let mut v = Vga::new().unwrap();
        // CRTC pas encore programmé : 70 Hz par défaut.
        assert_eq!(v.vertical(), (449, 400, 412..422));
        // Mode 13h : 400 lignes, 70 Hz.
        v.set_mode(0x13, true);
        assert_eq!(v.vertical(), (449, 400, 412..422));
        // Mode 12h : 480 lignes affichées sur 525, 59,94 Hz. Impulsion de
        // deux lignes sur la puce, élargie à `VSYNC_MIN_LINES`.
        v.set_mode(0x12, true);
        assert_eq!(v.vertical(), (525, 480, 490..500));
        // Incohérent (fin d'affichage au-delà du total) : retour au défaut.
        set(&mut v, 0x3D4, 0x12, 0xFF);
        set(&mut v, 0x3D4, 0x07, 0xFF);
        assert_eq!(v.vertical(), (449, 400, 412..422));
    }

    #[test]
    fn direct_read_loads_latches_lazily() {
        let mut v = Vga::new().unwrap();
        v.set_mode(0x12, true);
        // Octet 10 : plans 0 et 2 à FFh (set/reset, comme plus haut).
        set(&mut v, 0x3CE, 0, 0x05);
        set(&mut v, 0x3CE, 1, 0x0F);
        v.write(10, 0);
        set(&mut v, 0x3CE, 1, 0x00);
        // Configuration simple : lecture directe du plan 0 possible.
        let p = v.direct_read_window().expect("simple read");
        // SAFETY: plan de 64 Ko.
        assert_eq!(unsafe { p.add(10).read() }, 0xFF);
        v.read(30); // latches de l'octet 30 (vide)
        v.direct_read_at(10);
        // Mode 1 : les latches viennent de la lecture directe de l'octet 10.
        set(&mut v, 0x3CE, 5, 0x01);
        assert!(v.direct_read_window().is_none());
        v.write(20, 0);
        assert_eq!([0, 1, 2, 3].map(|p| v.plane_byte(p, 20)), [0xFF, 0x00, 0xFF, 0x00]);
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

    /// Mode X : décalage fin au pixel (attributs 13h) et écran partagé
    /// (comparaison de ligne), décalage annulé dessous (attributs 10h, bit 5).
    #[test]
    fn pel_panning_and_split_screen() {
        let mem = crate::mem::GuestMem::new(0x10_0000).unwrap();
        let mut v = Vga::new().unwrap();
        v.set_mode(0x13, true);
        set(&mut v, 0x3C4, 4, 0x06); // non chaîné
        for i in 0..256 {
            v.dac[i] = [i as u8 & 0x3F, 0, 0];
        }
        // Pixel x de la ligne : plan x & 3, octet x / 4. En 100h (haut de
        // l'écran) : 10, 11, 12, 13, 20, 21… ; en 0 (écran partagé) : 30…
        for p in 0..4 {
            v.vram.set(p, 0x100, 10 + p as u8);
            v.vram.set(p, 0x101, 20 + p as u8);
            v.vram.set(p, 0, 30 + p as u8);
        }
        set(&mut v, 0x3D4, 0x0C, 0x01);
        set(&mut v, 0x3D4, 0x0D, 0x00);
        let attr = |v: &mut Vga, i: u8, x: u8| {
            v.port_read(0x3DA);
            v.port_write(0x3C0, i | 0x20);
            v.port_write(0x3C0, x);
        };
        attr(&mut v, 0x13, 2); // un pixel
        let red = |c: u32| ((c >> 16) * 63 + 127) / 255;
        let mut out = Vec::new();
        let (w, _) = crate::render::render(&v, &mem, &mut out).unwrap();
        assert_eq!(red(out[0]), 11);
        assert_eq!(red(out[3]), 20);
        // Écran partagé après la ligne de balayage 1 (lignes doublées : à
        // partir de la ligne affichée 1), sans décalage dessous.
        set(&mut v, 0x3D4, 0x18, 0x01);
        let (c7, c9) = (v.crtc[7] & !0x10, v.crtc[9] & !0x40);
        set(&mut v, 0x3D4, 0x07, c7);
        set(&mut v, 0x3D4, 0x09, c9);
        attr(&mut v, 0x10, 0x41 | 0x20);
        crate::render::render(&v, &mem, &mut out).unwrap();
        assert_eq!(red(out[0]), 11);
        assert_eq!(red(out[w]), 30);
        assert_eq!(red(out[w + 1]), 31);
    }

    #[test]
    fn vbe_direct_color_rendering() {
        let mem = crate::mem::GuestMem::new(0x10_0000).unwrap();
        let mut v = Vga::new().unwrap();
        let poke = |v: &Vga, off: usize, b: &[u8]| {
            for (i, x) in b.iter().enumerate() {
                unsafe { v.vram.window.add(off + i).write_volatile(*x) };
            }
        };
        // (bpp, octets du point 1, couleur attendue en 0xRRGGBB)
        let cases: [(usize, &[u8], u32); 4] = [
            (15, &[0x00, 0x7C], 0xFF0000), // rouge 5:5:5
            (16, &[0xE0, 0x07], 0x00FF00), // vert 5:6:5
            (32, &[0x00, 0x80, 0xFF, 0x77], 0xFF8000), // B, V, R, octet réservé ignoré
            (24, &[0xFF, 0x00, 0x00], 0x0000FF), // bleu, ordre B, V, R (en dernier : suite du test)
        ];
        for (bpp, bytes, want) in cases {
            let mode = Vbe { mode: 0x111, width: 640, height: 480, bpp, pitch: 640 * bpp.div_ceil(8), start: 0, bank: 0, lfb: false };
            v.set_vbe(mode, true);
            v.point_window(v.access()).unwrap();
            poke(&v, bytes.len(), bytes);
            let mut out = Vec::new();
            assert_eq!(crate::render::render(&v, &mem, &mut out), Some((640, 480)));
            assert_eq!((out[0], out[1]), (0, want), "{bpp} bits");
        }
        // Début d'affichage : une ligne plus bas, le point 1 de la ligne 1 en tête.
        if let Some(m) = &mut v.vbe {
            m.start = m.pitch;
        }
        poke(&v, 1920 + 3, &[0x11, 0x22, 0x33]);
        let mut out = Vec::new();
        crate::render::render(&v, &mem, &mut out);
        assert_eq!(out[1], 0x332211);
    }

    #[test]
    fn plane_stage_follows_window() {
        let mut v = Vga::new().unwrap();
        v.set_mode(0x12, true);
        let poke = |v: &Vga, off: usize, val: u8| unsafe { v.vram.window.add(off).write_volatile(val) };
        let peek = |v: &Vga, off: usize| unsafe { v.vram.window.add(off).read_volatile() };
        v.point_window(Access::Plane(1)).unwrap();
        poke(&v, 10, 0x11);
        // Le modèle voit l'écriture native tant que le plan est en scène…
        assert_eq!(v.plane_byte(1, 10), 0x11);
        v.point_window(Access::Plane(2)).unwrap();
        poke(&v, 10, 0x22);
        // …et après le changement de plan, chacun garde la sienne.
        assert_eq!((v.plane_byte(1, 10), v.plane_byte(2, 10)), (0x11, 0x22));
        v.point_window(Access::Plane(1)).unwrap();
        assert_eq!(peek(&v, 10), 0x11);
        v.point_window(Access::Linear).unwrap();
        assert_eq!(v.plane_byte(1, 10), 0x11);
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
