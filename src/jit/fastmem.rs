// SPDX-License-Identifier: GPL-3.0-or-later
//! Accès mémoire directs du code traduit, sans contrôle (docs/JOURNAL.md,
//! point 7, « Accès par faute »).
//!
//! La mémoire de l'invité (un memfd, `GuestMem`) est mappée une seconde fois
//! dans 4 Go réservés : RAM en lecture-écriture, A0000–FFFFF (fenêtre VGA,
//! ROM) et tout ce qui dépasse la RAM sans accès, pages de code traduit en
//! lecture seule. Un accès direct (`[r8+r13]`) qui tombe sur une page
//! protégée fait une faute ; le gestionnaire de `SIGSEGV` retrouve le site
//! dans la table du thread (adresse hôte de l'instruction → sortie de repli)
//! et y reprend : le bloc sort sans avoir rien exécuté de l'instruction, qui
//! sera faite par l'interpréteur, et le site est ensuite traduit avec ses
//! contrôles (une faute par site, pas par accès).

use std::cell::Cell;
use std::io;
use std::sync::OnceLock;

/// Octets réservés : 4 Go d'adresses de l'invité, plus de quoi absorber le
/// débordement d'un accès qui commence juste avant 4 Go.
const RESERVE: usize = (1 << 32) + (64 << 10);

/// Zone toujours sans accès dans la vue : fenêtre VGA, mémoire et ROM des
/// cartes, BIOS (le code traduit n'y accède qu'avec contrôles).
const HOLE: std::ops::Range<usize> = 0xA_0000..0x10_0000;

/// Garde sans accès sous la vue : une copie descendante (`rep movs`,
/// DF = 1) qui passe sous l'adresse 0 y faute au lieu de sortir de la
/// réservation.
const BELOW: usize = 64 << 10;

/// Seconde vue de la mémoire de l'invité.
pub struct View {
    base: *mut u8,
    size: usize,
}

impl View {
    /// Réserve 4 Go et y mappe le memfd `fd` de `size` octets.
    pub fn new(fd: libc::c_int, size: usize) -> io::Result<Self> {
        install_handler()?;
        // SAFETY: réservation anonyme sans accès, puis mappage fixe du memfd
        // dans ses bornes ; tout est vérifié.
        unsafe {
            let below = libc::mmap(
                std::ptr::null_mut(),
                BELOW + RESERVE,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            );
            if below == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            let base = below.cast::<u8>().add(BELOW).cast::<libc::c_void>();
            let view = Self { base: base.cast(), size };
            let ram = libc::mmap(base, size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED | libc::MAP_FIXED, fd, 0);
            if ram == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            libc::madvise(base, size, libc::MADV_HUGEPAGE);
            if size > HOLE.start {
                let end = HOLE.end.min(size);
                view.protect(HOLE.start, end - HOLE.start, libc::PROT_NONE)?;
            }
            Ok(view)
        }
    }

    pub fn base(&self) -> u64 {
        self.base as u64
    }

    /// Page `page` (4 Ko) de RAM : lecture seule (`true`, code traduit) ou
    /// lecture-écriture.
    pub fn set_readonly(&self, page: u32, readonly: bool) -> io::Result<()> {
        let at = (page as usize) << 12;
        if at + 0x1000 > self.size || HOLE.contains(&at) {
            return Ok(());
        }
        let prot = if readonly { libc::PROT_READ } else { libc::PROT_READ | libc::PROT_WRITE };
        self.protect(at, 0x1000, prot)
    }

    fn protect(&self, at: usize, len: usize, prot: libc::c_int) -> io::Result<()> {
        // SAFETY: plage dans la réservation de `new`.
        if unsafe { libc::mprotect(self.base.add(at).cast(), len, prot) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Vues des plans de la carte VGA : pour chaque région du memfd de la VRAM
/// (`Vram`), 4 Go réservés dont seuls les 64 premiers Ko montrent la région,
/// puis une vue entièrement vide. Le code traduit accède à la fenêtre par
/// `[vue + (adresse − A0000h)]` : choisir le plan, c'est choisir la base
/// (sans remappage) ; hors fenêtre (l'écart replié sur 32 bits tombe au-delà
/// des 64 Ko) ou sans plan direct (vue vide), l'accès faute et reprend sur
/// le chemin contrôlé.
pub struct VgaViews {
    base: *mut u8,
    regions: usize,
    /// Adresse hôte du mappage complet de la VRAM (région 0).
    all: u64,
}

impl VgaViews {
    pub fn new(fd: libc::c_int, all: u64, regions: usize) -> io::Result<Self> {
        install_handler()?;
        let len = (regions + 1) * RESERVE;
        // SAFETY: réservation anonyme sans accès, puis une région du memfd
        // au début de chaque vue ; tout est vérifié.
        unsafe {
            let base = libc::mmap(std::ptr::null_mut(), len, libc::PROT_NONE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0);
            if base == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            let views = Self { base: base.cast(), regions, all };
            for r in 0..regions {
                let at = views.base.add(r * RESERVE);
                let p = libc::mmap(at.cast(), PLANE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED | libc::MAP_FIXED, fd, (r * PLANE) as libc::off_t);
                if p == libc::MAP_FAILED {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(views)
        }
    }

    /// Base de la vue qui montre le plan d'adresse hôte `plane` (0 : aucun
    /// plan direct, vue vide).
    pub fn view_of(&self, plane: u64) -> u64 {
        let region = plane.wrapping_sub(self.all) / PLANE as u64;
        let r = if plane == 0 || region >= self.regions as u64 || plane.wrapping_sub(self.all) % PLANE as u64 != 0 { self.regions } else { region as usize };
        self.base as u64 + (r * RESERVE) as u64
    }
}

impl Drop for VgaViews {
    fn drop(&mut self) {
        // SAFETY: réservation de `new`.
        unsafe { libc::munmap(self.base.cast(), (self.regions + 1) * RESERVE) };
    }
}

/// Taille d'une région de la VRAM (un plan).
const PLANE: usize = crate::vga::PLANE;

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: réservation de `new`, garde comprise (le memfd y est
        // mappé en entier).
        unsafe { libc::munmap(self.base.sub(BELOW).cast(), BELOW + RESERVE) };
    }
}

thread_local! {
    /// Sites d'accès direct du code traduit en cours d'exécution sur ce
    /// thread : [adresse de l'instruction, reprise, EIP de l'invité | bit 32
    /// si la reprise est en ligne (chemin contrôlé du même bloc)], triés.
    /// Posé par `Jit::run` avant chaque appel du code traduit.
    static SITES: Cell<(*const [u64; 3], usize)> = const { Cell::new((std::ptr::null(), 0)) };
    /// Reprises en ligne depuis le dernier relevé, et EIP de la dernière.
    static INLINE: Cell<(u64, u32)> = const { Cell::new((0, 0)) };
}

/// Reprise en ligne (bit de `SITES`).
pub const INLINE_BIT: u64 = 1 << 32;

/// Désigne la table des sites du thread (tranche triée, vivante pendant
/// l'exécution du code traduit).
pub fn set_sites(sites: &[[u64; 3]]) {
    SITES.with(|s| s.set((sites.as_ptr(), sites.len())));
}

/// Reprises en ligne depuis l'appel précédent (nombre, EIP de la dernière) :
/// un site qui faute souvent repasse aux contrôles.
pub fn take_inline() -> (u64, u32) {
    INLINE.with(|c| c.replace((0, 0)))
}

/// Gestionnaire précédent, rétabli pour une faute qui n'est pas la nôtre.
static PREVIOUS: OnceLock<libc::sigaction> = OnceLock::new();

fn install_handler() -> io::Result<()> {
    static DONE: OnceLock<Result<(), i32>> = OnceLock::new();
    let r = DONE.get_or_init(|| {
        // SAFETY: sigaction entièrement initialisée ; l'ancienne action est
        // gardée pour les fautes étrangères au code traduit.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_fault as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut sa.sa_mask);
            let mut old: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGSEGV, &sa, &mut old) != 0 {
                return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0));
            }
            let _ = PREVIOUS.set(old);
            Ok(())
        }
    });
    r.map_err(io::Error::from_raw_os_error)
}

/// Faute de page : sur un site d'accès direct, reprise à sa sortie de repli ;
/// sinon l'action précédente est rétablie et l'instruction refaute.
extern "C" fn on_fault(_: libc::c_int, _: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    // SAFETY: contexte fourni par le noyau pour un gestionnaire SA_SIGINFO ;
    // la table des sites n'est modifiée que hors de l'exécution du code
    // traduit, sur ce même thread (faute synchrone).
    unsafe {
        let uc = ctx.cast::<libc::ucontext_t>();
        let rip = &mut (*uc).uc_mcontext.gregs[libc::REG_RIP as usize];
        let (ptr, len) = SITES.with(|s| s.get());
        if !ptr.is_null() {
            let sites = std::slice::from_raw_parts(ptr, len);
            if let Ok(i) = sites.binary_search_by_key(&(*rip as u64), |s| s[0]) {
                *rip = sites[i][1] as i64;
                if sites[i][2] & INLINE_BIT != 0 {
                    INLINE.with(|c| c.set((c.get().0 + 1, sites[i][2] as u32)));
                }
                return;
            }
        }
        match PREVIOUS.get() {
            Some(old) => libc::sigaction(libc::SIGSEGV, old, std::ptr::null_mut()),
            None => {
                let mut dfl: libc::sigaction = std::mem::zeroed();
                dfl.sa_sigaction = libc::SIG_DFL;
                libc::sigaction(libc::SIGSEGV, &dfl, std::ptr::null_mut())
            }
        };
    }
}
