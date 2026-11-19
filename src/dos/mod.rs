// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS en HLE (étape 3), présenté comme MS-DOS 5.00.
//!
//! Les structures que les programmes lisent directement vivent en mémoire
//! invitée : chaîne de MCB, PSP (avec la JFT), environnement, DTA, List of
//! Lists. Le reste (SFT, recherches en cours, lecteurs) est tenu par l'hôte.
//! Les fichiers sont ceux de répertoires hôtes montés comme lecteurs.

mod crtfix;
mod fcb;
mod fs;
mod vfs;
mod handles;
mod memory;
mod mscdex;
mod process;
mod lfn;
mod shell;
mod win32;

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::backend::Result;
use crate::bios;
use crate::hle::{AX, BX, CF, CX, DI, DS, DX, ES, Flow, Regs, SI, SP, SS, lin, set_ret_flag};
use crate::host::Host;
use crate::interp::Cpu;
use crate::mem::GuestMem;

use fs::{Device, Drives, Target};
use handles::{Kind, Sft};
use process::{Process, PspInit, psp, psp_addr};

pub use shell::Action;

/// Segment des données du DOS : zone « swappable » (SDA) en tête, dont
/// l'octet 1 est le drapeau InDOS, puis la List of Lists.
const DOS_SEG: u16 = 0x0060;
const SDA_OFF: u16 = 0x0000;
const INDOS_OFF: u16 = SDA_OFF + 0x01;
/// SDA : DTA courante (dword), PSP courant (word) ; taille annoncée par
/// INT 21h AX=5D06h (CX : toujours sauvegardée, DX : pendant un appel).
const SDA_DTA: u16 = SDA_OFF + 0x0C;
const SDA_PSP: u16 = SDA_OFF + 0x10;
const SDA_SIZE: u16 = 0x18;
const LOL_OFF: u16 = 0x0080;
const NUL_DEVICE_OFF: u16 = LOL_OFF + 0x22;

/// Environnement maître du shell racine.
const MASTER_ENV_SB16: &[u8] = b"COMSPEC=C:\\COMMAND.COM\0PATH=C:\\\0PROMPT=$P$G\0BLASTER=A220 I5 D1 H5 P330 T6\0";
/// Sound Blaster Pro (`--sb pro`) : pas de DMA 16 bits, type 4.
const MASTER_ENV_PRO: &[u8] = b"COMSPEC=C:\\COMMAND.COM\0PATH=C:\\\0PROMPT=$P$G\0BLASTER=A220 I5 D1 T4\0";
pub(super) const MASTER_ENV_PARAS: u16 = 0x20;

const ERR_INVALID_FUNCTION: u16 = 0x01;
const ERR_CURRENT_DIR: u16 = 0x10;
const ERR_NOT_SAME_DEVICE: u16 = 0x11;
const ERR_FILE_EXISTS: u16 = 0x50;

/// Résultat d'un service : CF et AX selon la convention DOS.
enum Out {
    /// Drapeaux inchangés (anciennes fonctions sans CF).
    Keep,
    /// CF = 0.
    Ok,
    /// CF = 1, AX = code d'erreur.
    Err(u16),
    Flow(Flow),
}

impl From<std::result::Result<(), u16>> for Out {
    fn from(r: std::result::Result<(), u16>) -> Self {
        match r {
            Ok(()) => Out::Ok,
            Err(e) => Out::Err(e),
        }
    }
}

fn io_err(e: &std::io::Error) -> u16 {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => fs::ERR_FILE_NOT_FOUND,
        AlreadyExists => ERR_FILE_EXISTS,
        _ => fs::ERR_ACCESS_DENIED,
    }
}

/// Fichier trouvé par 4Eh/4Fh.
struct Found {
    name: Vec<u8>,
    attr: u8,
    time: u16,
    date: u16,
    size: u32,
}

/// Recherches en cours, repérées par un n° écrit dans la DTA.
struct Search {
    id: u16,
    results: Vec<Found>,
}

const MAX_SEARCHES: usize = 64;

pub struct Dos {
    drives: Drives,
    /// Lecteur de CD (image ISO) et extensions MSCDEX.
    cd: Option<mscdex::Mscdex>,
    /// Carte annoncée par `BLASTER` : Sound Blaster Pro (sinon SB16).
    pub sb_pro: bool,
    /// Variables ajoutées à l'environnement maître (`--set NOM=VALEUR`),
    /// prioritaires sur celles par défaut.
    pub env: Vec<(String, String)>,
    /// Changements de disque demandés pendant que le shell attendait une ligne.
    pub pending_media: Vec<crate::host::InputEvent>,
    /// Un programme Win32 a été lancé par HX (pour le reste de la session) :
    /// pas de répétition automatique des touches (voir `Machine::key_in`).
    pub win32_started: bool,
    sft: Sft,
    /// Pile des processus ; le premier est le shell racine.
    procs: Vec<Process>,
    current_psp: u16,
    dta: (u16, u16),
    strategy: u16,
    /// Code de retour du dernier programme terminé (AH=4Dh) : type << 8 | code.
    last_return: u16,
    last_error: u16,
    break_flag: bool,
    verify: bool,
    searches: VecDeque<Search>,
    next_search: u16,
    /// Recherches à noms longs (AX=714Eh), repérées par leur handle.
    lfn_searches: VecDeque<(u16, VecDeque<lfn::Found>)>,
    /// Scancode d'une touche étendue, rendu au prochain appel de lecture.
    pending_scan: Option<u8>,
    /// Ligne saisie à la console pas encore lue par AH=3Fh.
    con_line: VecDeque<u8>,
    /// Ligne en cours de saisie, entre deux reprises de l'appel.
    line_edit: Option<Vec<u8>>,
    /// AH=0Ch en attente : le tampon clavier a déjà été vidé.
    flushed: bool,
}

impl Default for Dos {
    fn default() -> Self {
        Self::new()
    }
}

impl Dos {
    pub fn new() -> Self {
        Self {
            drives: Drives::new(),
            sft: Sft::new(),
            procs: Vec::new(),
            current_psp: 0,
            dta: (0, 0x80),
            strategy: 0,
            last_return: 0,
            last_error: 0,
            break_flag: false,
            verify: false,
            searches: VecDeque::new(),
            next_search: 1,
            lfn_searches: VecDeque::new(),
            pending_scan: None,
            con_line: VecDeque::new(),
            line_edit: None,
            flushed: false,
            cd: None,
            sb_pro: false,
            env: Vec::new(),
            pending_media: Vec::new(),
            win32_started: false,
        }
    }

    /// Pilote EMS présent : le périphérique « EMMXXXX0 » s'ouvre.
    pub fn set_ems(&mut self, on: bool) {
        self.drives.ems = on;
    }

    pub fn mount(&mut self, drive: u8, root: PathBuf) {
        self.drives.mount(drive, root);
    }

    /// Monte une image ISO comme lecteur de CD (lecture seule), servie par
    /// les extensions MSCDEX.
    pub fn mount_cd(&mut self, mem: &GuestMem, drive: u8, disc: crate::cdrom::SharedDisc, player: crate::cdrom::SharedPlayer) -> std::result::Result<(), String> {
        let iso = std::sync::Arc::new(crate::iso9660::IsoFs::new(disc)?);
        let root = vfs::mount_iso(drive, iso.clone());
        self.drives.mount(drive, root.clone());
        let cd = mscdex::Mscdex::new(drive, root, iso, player);
        cd.install(mem);
        self.cd = Some(cd);
        Ok(())
    }

    /// Nouveau disque dans le lecteur de CD (ou éjection), si D: est monté.
    pub fn change_cd(&mut self, disc: Option<crate::cdrom::SharedDisc>) -> std::result::Result<(), String> {
        let Some(cd) = self.cd.as_mut() else { return Ok(()) };
        let iso = disc.map(crate::iso9660::IsoFs::new).transpose()?.map(std::sync::Arc::new);
        let root = cd.change(iso);
        self.drives.mount(cd.drive, root);
        Ok(())
    }

    /// Environnement maître : `BLASTER` selon la carte son, puis les
    /// variables de `--set` (une variable existante est remplacée). Ce qui ne
    /// tient pas dans le bloc est laissé de côté, avec un avertissement.
    fn master_env(&self) -> Vec<u8> {
        let base = if self.sb_pro { MASTER_ENV_PRO } else { MASTER_ENV_SB16 };
        let mut vars: Vec<Vec<u8>> = base.split(|&b| b == 0).filter(|v| !v.is_empty()).map(<[u8]>::to_vec).collect();
        for (name, value) in &self.env {
            let entry = format!("{name}={value}").into_bytes();
            vars.retain(|v| !v.starts_with(format!("{name}=").as_bytes()));
            let size: usize = vars.iter().map(|v| v.len() + 1).sum::<usize>() + entry.len() + 2;
            if size > usize::from(MASTER_ENV_PARAS) * 16 {
                eprintln!("[dos] environment full: {name} not set");
                continue;
            }
            vars.push(entry);
        }
        let mut out = Vec::new();
        for v in vars {
            out.extend_from_slice(&v);
            out.push(0);
        }
        out.push(0);
        out
    }

    /// Étiquette de volume d'un lecteur (celle du CD).
    fn volume_label(&self, drive: u8) -> Option<String> {
        let cd = self.cd.as_ref().filter(|cd| cd.drive == drive)?;
        let label = cd.volume_label()?;
        (!label.is_empty()).then_some(label)
    }

    /// INT 2Fh : MSCDEX (AH=15h) ; `None` pour les autres fonctions.
    pub fn int2f(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Option<crate::hle::Flow> {
        self.cd.as_mut()?.int2f(cpu, mem)
    }

    /// Répertoire courant (lecteur compris) : `Err` si le chemin est invalide.
    pub fn chdir(&mut self, path: &[u8]) -> std::result::Result<(), u16> {
        self.drives.chdir(path)
    }

    /// Chemin DOS d'un fichier hôte situé sous un lecteur monté.
    pub fn dos_path_of(&self, host: &std::path::Path) -> Option<Vec<u8>> {
        self.drives.dos_path_of(host)
    }

    /// Mémoire, données du DOS, shell racine (PSP + environnement maître).
    pub fn boot(&mut self, mem: &GuestMem) {
        memory::init(mem);
        mem.fill(lin(DOS_SEG, 0), 0x100, 0);
        let lol = lin(DOS_SEG, LOL_OFF);
        mem.write_u16(lol - 2, memory::FIRST_MCB);
        for ptr in [0x00, 0x04, 0x08, 0x0C, 0x12, 0x16, 0x1A] {
            mem.write_u32(lol + ptr, 0xFFFF_FFFF);
        }
        mem.write_u16(lol + 0x10, 512);
        mem.write_u8(lol + 0x20, 1);
        mem.write_u8(lol + 0x21, 26);
        // En-tête du périphérique NUL, tête de la chaîne des périphériques.
        let nul = lin(DOS_SEG, NUL_DEVICE_OFF);
        mem.write_u32(nul, 0xFFFF_FFFF);
        mem.write_u16(nul + 4, 0x8004);
        mem.write(nul + 10, b"NUL     ");

        let root = memory::alloc(mem, 0x10, 0xFFFF, 0).expect("shell PSP");
        memory::set_owner(mem, root, root);
        memory::set_name(mem, root, b"COMMAND");
        let env = memory::alloc(mem, MASTER_ENV_PARAS, root, 0).expect("master environment");
        mem.fill(lin(env, 0), usize::from(MASTER_ENV_PARAS) * 16, 0);
        mem.write(lin(env, 0), &self.master_env());
        let init = PspInit {
            parent: root,
            env,
            top: memory::MEM_TOP,
            jft: handles::ROOT_JFT,
            tail: b"",
            fcb1: process::BLANK_FCB,
            fcb2: process::BLANK_FCB,
        };
        process::build_psp(mem, root, &init);
        for (_, idx) in handles::all(mem, root) {
            self.sft.add_ref(idx);
        }
        self.procs = vec![Process { psp: root, parent_cpu: None }];
        self.current_psp = root;
        self.dta = (root, 0x80);
        self.sync_sda(mem);
    }

    /// Recopie le PSP et la DTA courants dans la SDA, où les lisent les
    /// hôtes DPMI (HDPMI, de HX, y prend le PSP pour convertir son
    /// environnement en sélecteur).
    fn sync_sda(&self, mem: &GuestMem) {
        mem.write_u16(lin(DOS_SEG, SDA_PSP), self.current_psp);
        mem.write_u16(lin(DOS_SEG, SDA_DTA), self.dta.1);
        mem.write_u16(lin(DOS_SEG, SDA_DTA + 2), self.dta.0);
    }

    /// Lance un programme du disque depuis l'hôte (sa fin termine la session).
    pub fn start(&mut self, mem: &GuestMem, path: &[u8], tail: &[u8]) -> std::result::Result<Cpu, String> {
        let (image, dos) =
            self.read_program(path).map_err(|e| format!("{}: DOS error {e:02X}h", String::from_utf8_lossy(path)))?;
        self.exec(mem, &image, &dos, tail, 0, None, None).map_err(|e| format!("loading: DOS error {e:02X}h"))
    }

    /// Lance une image en mémoire sous le nom `path` (tests).
    #[cfg(test)]
    pub fn start_image(
        &mut self,
        mem: &GuestMem,
        image: &[u8],
        path: &[u8],
        tail: &[u8],
    ) -> std::result::Result<Cpu, String> {
        self.exec(mem, image, path, tail, 0, None, None).map_err(|e| format!("loading: DOS error {e:02X}h"))
    }

    /// Nom du fichier seul si `path` (canonique) est dans le répertoire
    /// courant de son lecteur, sinon `path` : lignes de commande plus courtes.
    fn short_name<'p>(&self, path: &'p [u8]) -> &'p [u8] {
        let Some(cut) = path.iter().rposition(|&c| c == b'\\') else { return path };
        let (dir, name) = (&path[..cut], &path[cut + 1..]);
        let Some(drive) = path.first().map(|d| d.to_ascii_uppercase().wrapping_sub(b'A')) else { return path };
        if drive != self.drives.current || !self.drives.is_mounted(drive) {
            return path;
        }
        let mut cwd = fs::canonical(drive, &[]);
        cwd.extend_from_slice(self.drives.cwd(drive));
        let cwd = cwd.strip_suffix(b"\\").unwrap_or(&cwd);
        if dir.eq_ignore_ascii_case(cwd) { name } else { path }
    }

    /// Valeur de PATH dans le bloc d'environnement `env` (maître si 0).
    fn env_path(&self, mem: &GuestMem, env: u16) -> Vec<u8> {
        let bytes = process::env_block(mem, env, &self.master_env(), b"");
        bytes
            .split(|&c| c == 0)
            .take_while(|v| !v.is_empty())
            .find_map(|v| v.strip_prefix(b"PATH=").map(<[u8]>::to_vec))
            .unwrap_or_default()
    }

    /// Fichier d'un programme : contenu et chemin DOS canonique.
    fn read_program(&self, path: &[u8]) -> std::result::Result<(Vec<u8>, Vec<u8>), u16> {
        match self.drives.resolve(path)? {
            Target::Path { host, exists: true, dos } if vfs::is_file(&host) => {
                vfs::read(&host).map(|d| (d, dos)).map_err(|e| io_err(&e))
            }
            _ => Err(fs::ERR_FILE_NOT_FOUND),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec(
        &mut self,
        mem: &GuestMem,
        image: &[u8],
        path: &[u8],
        tail: &[u8],
        env_param: u16,
        fcbs: Option<([u8; 16], [u8; 16])>,
        parent_cpu: Option<Cpu>,
    ) -> std::result::Result<Cpu, u16> {
        let parent = self.current_psp;
        let parent_env = if env_param != 0 { env_param } else { mem.read_u16(psp_addr(parent, psp::ENV)) };
        // Programme Win32 : lancé par le chargeur PE de HX DOS Extender.
        let mut win32 = win32::loader_for(image, self.short_name(path), tail, &self.env_path(mem, parent_env), |p| self.read_program(p).ok());
        // HX absent du jeu : celui de Bare86, téléchargé au besoin, monté en Z:.
        if win32.is_none() && win32::is_win32(image) && self.drives.root(win32::HX_DRIVE).is_none() {
            match crate::runtime::hx_dir_or_install() {
                Ok(dir) => {
                    self.drives.mount(win32::HX_DRIVE, dir);
                    win32 = win32::loader_for(image, self.short_name(path), tail, &self.env_path(mem, parent_env), |p| self.read_program(p).ok());
                }
                Err(e) => eprintln!("[win32] HX DOS Extender: {e}"),
            }
        }
        if win32.is_none() && win32::is_win32(image) {
            eprintln!("[win32] {}: Windows program; HX DOS Extender not found", String::from_utf8_lossy(path));
        }
        self.win32_started |= win32.is_some();
        let (image, path, tail) = match &win32 {
            Some(l) => (&l.image[..], &l.path[..], &l.tail[..]),
            None => (image, path, tail),
        };
        let exe = process::parse(image)?;
        let mut env_bytes = process::env_block(mem, parent_env, &self.master_env(), path);
        if let Some(l) = &win32 {
            env_bytes = win32::with_path_dir(&env_bytes, &l.dir);
        }
        let env_paras = (env_bytes.len() as u16).div_ceil(16);
        let env = memory::alloc(mem, env_paras, 0xFFFF, self.strategy).map_err(|(e, _)| e)?;
        mem.write(lin(env, 0), &env_bytes);
        let (seg, paras) = match process::alloc_program(mem, &exe, self.strategy) {
            Ok(r) => r,
            Err(e) => {
                let _ = memory::free(mem, env);
                return Err(e);
            }
        };
        memory::set_owner(mem, env, seg);
        memory::set_name(mem, seg, process::mcb_name(path));
        // Handles hérités, sauf ceux ouverts avec le bit « non hérité ».
        let mut jft = [handles::UNUSED; process::JFT_SIZE];
        for (h, idx) in handles::all(mem, parent) {
            let inherit = self.sft.get(idx).is_some_and(|e| e.mode & 0x80 == 0);
            if usize::from(h) < jft.len() && inherit {
                jft[usize::from(h)] = idx;
                self.sft.add_ref(idx);
            }
        }
        let (fcb1, fcb2) = fcbs.unwrap_or_else(|| default_fcbs(tail));
        let init = PspInit { parent, env, top: seg + paras, jft, tail, fcb1, fcb2 };
        process::build_psp(mem, seg, &init);
        let mut cpu = exe.load(mem, seg, paras);
        cpu.set_w(AX, 0);
        if let Some(p) = &parent_cpu {
            // SS:SP du parent, comme le fait DOS.
            mem.write_u16(psp_addr(parent, psp::SAVED_SS_SP), p.w(SP));
            mem.write_u16(psp_addr(parent, psp::SAVED_SS_SP + 2), p.sreg(SS));
        }
        self.procs.push(Process { psp: seg, parent_cpu });
        self.current_psp = seg;
        self.dta = (seg, 0x80);
        self.sync_sda(mem);
        Ok(cpu)
    }

    /// Fin du programme courant (`kind` : 0 normale, 3 résident).
    fn terminate(&mut self, cpu: &mut Cpu, mem: &GuestMem, code: u8, kind: u8) -> Out {
        // PSP courant créé par AH=55h (et rendu courant par AH=50h), sans
        // EXEC : comme DOS, retour à l'adresse de fin de ce PSP (PSP:0Ah),
        // son parent (PSP:16h) redevenant le PSP courant. Chargeur de Second
        // Reality : chaque partie, lue dans SECOND.EXE, finit ainsi.
        let child = self.current_psp;
        if self.procs.last().is_some_and(|p| p.psp != child) && mem.read_u16(psp_addr(child, 0)) == 0x20CD {
            if kind != 3 {
                for (h, idx) in handles::all(mem, child) {
                    self.sft.release(idx);
                    handles::set(mem, child, h, handles::UNUSED);
                }
                memory::free_owned(mem, child);
            }
            let mut v = [0u8; 12];
            mem.read(psp_addr(child, psp::INT22), &mut v);
            mem.write(0x22 * 4, &v);
            self.last_return = (u16::from(kind) << 8) | u16::from(code);
            let parent = mem.read_u16(psp_addr(child, psp::PARENT));
            self.current_psp = parent;
            self.dta = (parent, 0x80);
            // Le cadre d'IRET de l'appel reprend à l'adresse de fin.
            let frame = lin(cpu.sreg(SS), cpu.w(SP));
            mem.write(frame, &v[..4]);
            return Out::Keep;
        }
        if self.procs.len() <= 1 {
            return Out::Flow(Flow::Exit(code));
        }
        let p = self.procs.pop().expect("process");
        if kind != 3 {
            for (h, idx) in handles::all(mem, p.psp) {
                self.sft.release(idx);
                handles::set(mem, p.psp, h, handles::UNUSED);
            }
            memory::free_owned(mem, p.psp);
        }
        let mut v = [0u8; 12];
        mem.read(psp_addr(p.psp, psp::INT22), &mut v);
        mem.write(0x22 * 4, &v);
        self.last_return = (u16::from(kind) << 8) | u16::from(code);
        let parent = self.procs.last().expect("parent").psp;
        self.current_psp = parent;
        self.dta = (parent, 0x80);
        self.sync_sda(mem);
        match p.parent_cpu {
            Some(mut parent_cpu) => {
                parent_cpu.seg_dirty = true;
                *cpu = parent_cpu;
                set_ret_flag(cpu, mem, CF, false);
                Out::Keep
            }
            None => Out::Flow(Flow::Exit(code)),
        }
    }

    /// Vecteurs servis par le DOS ; `None` : pas une interruption du DOS.
    pub fn call(&mut self, vector: u8, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Option<Flow>> {
        let flow = self.call_inner(vector, cpu, mem, host);
        self.sync_sda(mem);
        flow
    }

    fn call_inner(&mut self, vector: u8, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Option<Flow>> {
        let out = match vector {
            0x20 => self.terminate(cpu, mem, 0, 0),
            0x21 => self.int21(cpu, mem, host)?,
            // Résident : DX = octets à garder depuis le PSP.
            0x27 => {
                let paras = u32::from(cpu.w(DX)).div_ceil(16);
                let _ = memory::resize(mem, self.current_psp, paras.max(0x10) as u16);
                self.terminate(cpu, mem, 0, 3)
            }
            // Sortie console rapide.
            0x29 => {
                con_out(mem, host, &[cpu.lo(AX)]);
                Out::Keep
            }
            // Erreur critique sans gestionnaire : « échec ».
            0x24 => {
                cpu.set_lo(AX, 3);
                Out::Keep
            }
            // Ctrl-C, attente, réseau, multiplex : rien d'installé.
            0x23 | 0x28 | 0x2A | 0x2F => Out::Keep,
            _ => return Ok(None),
        };
        Ok(Some(self.finish(cpu, mem, out)))
    }

    fn finish(&mut self, cpu: &mut Cpu, mem: &GuestMem, out: Out) -> Flow {
        match out {
            Out::Keep => Flow::Continue,
            Out::Ok => {
                set_ret_flag(cpu, mem, CF, false);
                Flow::Continue
            }
            Out::Err(code) => {
                self.last_error = code;
                cpu.set_w(AX, code);
                set_ret_flag(cpu, mem, CF, true);
                Flow::Continue
            }
            Out::Flow(f) => f,
        }
    }

    // ---- Console ----

    /// Lecture console sans écho : les touches étendues donnent 0 puis leur
    /// scancode. `None` : aucune touche, l'appel attendra (`Flow::Wait`).
    fn con_in(&mut self, mem: &GuestMem) -> Option<u8> {
        if let Some(scan) = self.pending_scan.take() {
            return Some(scan);
        }
        let w = bios::read_key(mem)?;
        let ascii = w as u8;
        if ascii == 0 || ascii == 0xE0 {
            self.pending_scan = Some((w >> 8) as u8);
            return Some(0);
        }
        Some(ascii)
    }

    fn key_ready(&self, mem: &GuestMem) -> bool {
        self.pending_scan.is_some() || bios::peek_key(mem).is_some()
    }

    /// Saisie d'une ligne avec écho et retour arrière (AH=0Ah, 3Fh sur CON),
    /// par morceaux : chaque reprise de l'appel consomme les touches arrivées.
    /// `Some(ligne)` à la touche Entrée.
    fn read_line(&mut self, mem: &GuestMem, host: &mut Host, max: usize) -> Option<Vec<u8>> {
        let mut line = self.line_edit.take().unwrap_or_default();
        while let Some(c) = self.con_in(mem) {
            match c {
                b'\r' => return Some(line),
                0x08 | 0x7F => {
                    if line.pop().is_some() {
                        con_out(mem, host, b"\x08 \x08");
                    }
                }
                // Touche étendue : ignorée.
                0 => self.pending_scan = None,
                _ if line.len() + 1 < max => {
                    line.push(c);
                    con_out(mem, host, &[c]);
                }
                _ => con_out(mem, host, &[0x07]),
            }
        }
        self.line_edit = Some(line);
        None
    }

    // ---- Fichiers ----

    fn psp_handle(&mut self, mem: &GuestMem, h: u16) -> std::result::Result<u8, u16> {
        let idx = handles::lookup(mem, self.current_psp, h).ok_or(handles::ERR_BAD_HANDLE)?;
        self.sft.get(idx).map(|_| idx).ok_or(handles::ERR_BAD_HANDLE)
    }

    fn new_handle(&mut self, mem: &GuestMem, kind: Kind, mode: u8) -> std::result::Result<u16, u16> {
        let idx = self.sft.insert(kind, mode).ok_or(handles::ERR_TOO_MANY)?;
        handles::assign(mem, self.current_psp, idx).ok_or_else(|| {
            self.sft.release(idx);
            handles::ERR_TOO_MANY
        })
    }

    /// Ouvre ou crée ; `create` : 0 ouvrir, 1 créer ou tronquer, 2 créer
    /// seulement s'il n'existe pas.
    fn open(&mut self, mem: &GuestMem, path: &[u8], mode: u8, create: u8) -> std::result::Result<u16, u16> {
        let target = self.drives.resolve(path)?;
        self.open_target(mem, target, mode, create)
    }

    /// `open` sur un chemin déjà résolu (noms courts ou longs).
    fn open_target(&mut self, mem: &GuestMem, target: Target, mode: u8, create: u8) -> std::result::Result<u16, u16> {
        match target {
            Target::Device(d) => {
                let kind = match d {
                    Device::Con => Kind::Con,
                    Device::Nul | Device::Ems => Kind::Nul,
                    Device::Aux => Kind::Aux,
                    Device::Prn => Kind::Prn,
                };
                self.new_handle(mem, kind, mode)
            }
            Target::Path { host, exists, .. } => {
                if exists && vfs::is_dir(&host) {
                    return Err(fs::ERR_ACCESS_DENIED);
                }
                if create == 2 && exists {
                    return Err(ERR_FILE_EXISTS);
                }
                if create == 0 && !exists {
                    return Err(fs::ERR_FILE_NOT_FOUND);
                }
                let access = match (create, mode & 3) {
                    (0, 0) => vfs::Access { read: true, write: false, create: false },
                    (0, 1) => vfs::Access { read: false, write: true, create: false },
                    (0, _) => vfs::Access { read: true, write: true, create: false },
                    _ => vfs::Access { read: true, write: true, create: true },
                };
                let file = vfs::open(&host, access).map_err(|e| io_err(&e))?;
                let mode = if create != 0 { (mode & 0x80) | 2 } else { mode };
                self.new_handle(mem, Kind::File(fs::OpenFile { file }), mode)
            }
        }
    }

    /// AH=3Fh ; `None` : lecture de la console en attente d'une ligne.
    fn read_handle(&mut self, cpu: &Cpu, mem: &GuestMem, host: &mut Host) -> Option<std::result::Result<u16, u16>> {
        let idx = match self.psp_handle(mem, cpu.w(BX)) {
            Ok(i) => i,
            Err(e) => return Some(Err(e)),
        };
        let at = lin(cpu.sreg(DS), cpu.w(DX));
        let want = usize::from(cpu.w(CX));
        if matches!(self.sft.get(idx).map(|e| &e.kind), Some(Kind::Con)) {
            if self.con_line.is_empty() {
                let mut line = self.read_line(mem, host, 128)?;
                con_out(mem, host, b"\r\n");
                line.extend_from_slice(b"\r\n");
                self.con_line.extend(line);
            }
            let n = want.min(self.con_line.len());
            let bytes: Vec<u8> = self.con_line.drain(..n).collect();
            mem.write(at, &bytes);
            return Some(Ok(n as u16));
        }
        let Some(e) = self.sft.get(idx) else { return Some(Err(handles::ERR_BAD_HANDLE)) };
        if e.mode & 3 == 1 {
            return Some(Err(fs::ERR_ACCESS_DENIED));
        }
        Some(match &mut e.kind {
            Kind::File(f) => {
                let mut buf = vec![0u8; want];
                let mut n = 0;
                while n < want {
                    match f.file.read(&mut buf[n..]) {
                        Ok(0) => break,
                        Ok(k) => n += k,
                        Err(e) => return Some(Err(io_err(&e))),
                    }
                }
                mem.write(at, &buf[..n]);
                Ok(n as u16)
            }
            _ => Ok(0),
        })
    }

    fn write_handle(&mut self, cpu: &Cpu, mem: &GuestMem, host: &mut Host) -> std::result::Result<u16, u16> {
        let idx = self.psp_handle(mem, cpu.w(BX))?;
        let mut buf = vec![0u8; usize::from(cpu.w(CX))];
        mem.read(lin(cpu.sreg(DS), cpu.w(DX)), &mut buf);
        let e = self.sft.get(idx).ok_or(handles::ERR_BAD_HANDLE)?;
        if e.mode & 3 == 0 && !e.kind.is_device() {
            return Err(fs::ERR_ACCESS_DENIED);
        }
        match &mut e.kind {
            Kind::Con => con_out(mem, host, &buf),
            Kind::File(f) => {
                if buf.is_empty() {
                    // Écrire 0 octet tronque (ou étend) le fichier à la position courante.
                    let pos = f.file.stream_position().map_err(|e| io_err(&e))?;
                    f.file.set_len(pos).map_err(|e| io_err(&e))?;
                } else {
                    f.file.write_all(&buf).map_err(|e| io_err(&e))?;
                }
            }
            Kind::Nul | Kind::Aux | Kind::Prn => {}
        }
        Ok(buf.len() as u16)
    }

    fn close(&mut self, mem: &GuestMem, h: u16) -> std::result::Result<(), u16> {
        let idx = self.psp_handle(mem, h)?;
        handles::set(mem, self.current_psp, h, handles::UNUSED);
        self.sft.release(idx);
        Ok(())
    }

    fn seek(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> std::result::Result<(), u16> {
        let idx = self.psp_handle(mem, cpu.w(BX))?;
        let off = (u32::from(cpu.w(CX)) << 16) | u32::from(cpu.w(DX));
        let e = self.sft.get(idx).ok_or(handles::ERR_BAD_HANDLE)?;
        let pos = match &mut e.kind {
            Kind::File(f) => {
                let from = match cpu.lo(AX) {
                    0 => SeekFrom::Start(u64::from(off)),
                    1 => SeekFrom::Current(i64::from(off as i32)),
                    2 => SeekFrom::End(i64::from(off as i32)),
                    _ => return Err(ERR_INVALID_FUNCTION),
                };
                f.file.seek(from).map_err(|e| io_err(&e))? as u32
            }
            _ => 0,
        };
        cpu.set_w(AX, pos as u16);
        cpu.set_w(DX, (pos >> 16) as u16);
        Ok(())
    }

    fn ioctl(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> std::result::Result<(), u16> {
        let al = cpu.lo(AX);
        match al {
            0x00 | 0x01 | 0x06 | 0x07 => {
                let idx = self.psp_handle(mem, cpu.w(BX))?;
                let con_ready = !self.con_line.is_empty() || self.key_ready(mem);
                let current = self.drives.current;
                let e = self.sft.get(idx).ok_or(handles::ERR_BAD_HANDLE)?;
                let info = match &e.kind {
                    Kind::Con => 0x80D3,
                    Kind::Nul => 0x8084,
                    Kind::Aux => 0x80C0,
                    Kind::Prn => 0xA8C0,
                    Kind::File(_) => u16::from(current),
                };
                match al {
                    0x00 => cpu.set_w(DX, info),
                    0x01 => {}
                    0x06 => {
                        let ready = match &mut e.kind {
                            Kind::Con => con_ready,
                            Kind::File(f) => {
                                let pos = f.file.stream_position().unwrap_or(0);
                                f.file.metadata().map(|m| pos < m.len()).unwrap_or(false)
                            }
                            _ => false,
                        };
                        cpu.set_lo(AX, if ready { 0xFF } else { 0 });
                    }
                    _ => cpu.set_lo(AX, 0xFF),
                }
                Ok(())
            }
            // Support amovible : non (disque fixe).
            0x08 => {
                cpu.set_w(AX, 1);
                Ok(())
            }
            // Lecteur ou handle distant : non.
            0x09 | 0x0A => {
                cpu.set_w(DX, 0);
                Ok(())
            }
            // Lettre logique d'un lecteur : une seule.
            0x0E => {
                cpu.set_lo(AX, 0);
                Ok(())
            }
            _ => Err(ERR_INVALID_FUNCTION),
        }
    }

    fn find(&mut self, cpu: &Cpu, mem: &GuestMem, first: bool) -> std::result::Result<(), u16> {
        let dta = lin(self.dta.0, self.dta.1);
        if first {
            let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
            let attr = cpu.lo(CX);
            let (dir, pattern) = self.drives.search(&path)?;
            let mut results = Vec::new();
            let (drive, comps) = self.drives.split(&path)?;
            let at_root = comps.len() <= 1;
            // Étiquette de volume, rangée comme un nom 8.3 (« BARE86TE.ST »).
            if attr & fs::ATTR_VOLUME != 0
                && at_root
                && let Some(label) = self.volume_label(drive)
            {
                let mut name: Vec<u8> = label.bytes().take(8).collect();
                if label.len() > 8 {
                    name.push(b'.');
                    name.extend(label.bytes().skip(8).take(3));
                }
                let (time, date) = fs::dos_time(std::time::SystemTime::now());
                results.push(Found { name, attr: fs::ATTR_VOLUME, time, date, size: 0 });
            }
            if attr != fs::ATTR_VOLUME {
                if attr & fs::ATTR_DIR != 0 && !at_root {
                    for dot in [&b"."[..], b".."] {
                        if fs::fcb_matches(&pattern, dot) {
                            let (time, date) = fs::dos_time(std::time::SystemTime::now());
                            results.push(Found { name: dot.to_vec(), attr: fs::ATTR_DIR, time, date, size: 0 });
                        }
                    }
                }
                for e in fs::list(&dir) {
                    let a = fs::attributes(&e.meta);
                    if (a & fs::ATTR_DIR != 0 && attr & fs::ATTR_DIR == 0) || !fs::fcb_matches(&pattern, &e.short) {
                        continue;
                    }
                    let (time, date) =
                        fs::dos_time(e.meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
                    let size = e.meta.len().min(u64::from(u32::MAX)) as u32;
                    results.push(Found { name: e.short, attr: a, time, date, size });
                }
            }
            if results.is_empty() {
                return Err(fs::ERR_FILE_NOT_FOUND);
            }
            results.reverse();
            let id = self.new_search(results);
            // Zone réservée de la DTA : lecteur, motif, attributs, puis notre n°.
            mem.write_u8(dta, self.drives.current + 1);
            mem.write(dta + 1, &pattern);
            mem.write_u8(dta + 0x0C, attr);
            mem.write_u16(dta + 0x0D, id);
        }
        let id = mem.read_u16(dta + 0x0D);
        let s = self.searches.iter_mut().find(|s| s.id == id).ok_or(fs::ERR_NO_MORE_FILES)?;
        let f = s.results.pop().ok_or(fs::ERR_NO_MORE_FILES)?;
        mem.write_u8(dta + 0x15, f.attr);
        mem.write_u16(dta + 0x16, f.time);
        mem.write_u16(dta + 0x18, f.date);
        mem.write_u32(dta + 0x1A, f.size);
        let mut name = [0u8; 13];
        name[..f.name.len()].copy_from_slice(&f.name);
        mem.write(dta + 0x1E, &name);
        Ok(())
    }

    /// Enregistre une recherche (résultats en ordre inverse) ; renvoie son n°.
    fn new_search(&mut self, results: Vec<Found>) -> u16 {
        let id = self.next_search;
        self.next_search = self.next_search.wrapping_add(1).max(1);
        if self.searches.len() >= MAX_SEARCHES {
            self.searches.pop_front();
        }
        self.searches.push_back(Search { id, results });
        id
    }

    fn path_arg(&self, mem: &GuestMem, seg: u16, off: u16) -> std::result::Result<(PathBuf, bool), u16> {
        match self.drives.resolve(&read_asciiz(mem, lin(seg, off)))? {
            Target::Path { host, exists, .. } => Ok((host, exists)),
            Target::Device(_) => Err(fs::ERR_ACCESS_DENIED),
        }
    }

    /// Résultat d'une fonction qui rend une valeur dans AX.
    fn ax_result(cpu: &mut Cpu, r: std::result::Result<u16, u16>) -> Out {
        match r {
            Ok(v) => {
                cpu.set_w(AX, v);
                Out::Ok
            }
            Err(e) => Out::Err(e),
        }
    }

    // ---- INT 21h ----

    fn int21(&mut self, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Out> {
        let ah = cpu.hi(AX);
        let al = cpu.lo(AX);
        let ds = cpu.sreg(DS);
        let dx = cpu.w(DX);
        Ok(match ah {
            0x00 => self.terminate(cpu, mem, 0, 0),
            0x01 => match self.con_in(mem) {
                Some(c) => {
                    con_out(mem, host, &[c]);
                    cpu.set_lo(AX, c);
                    Out::Keep
                }
                None => Out::Flow(Flow::Wait),
            },
            0x02 => {
                con_out(mem, host, &[cpu.lo(DX)]);
                Out::Keep
            }
            // E/S console directes : DL = FFh lit sans attendre (ZF = 1 si rien).
            0x06 if cpu.lo(DX) == 0xFF => {
                let c = self.con_in(mem);
                let ready = c.is_some();
                cpu.set_lo(AX, c.unwrap_or(0));
                set_ret_flag(cpu, mem, crate::hle::ZF, !ready);
                Out::Keep
            }
            0x06 => {
                con_out(mem, host, &[cpu.lo(DX)]);
                Out::Keep
            }
            0x07 | 0x08 => match self.con_in(mem) {
                Some(c) => {
                    cpu.set_lo(AX, c);
                    Out::Keep
                }
                None => Out::Flow(Flow::Wait),
            },
            0x09 => {
                let mut s = Vec::new();
                let mut off = dx;
                loop {
                    let c = mem.read_u8(lin(ds, off));
                    if c == b'$' || s.len() > 0xFFFF {
                        break;
                    }
                    s.push(c);
                    off = off.wrapping_add(1);
                }
                con_out(mem, host, &s);
                Out::Keep
            }
            // Saisie bufferisée : DS:DX = [max][lus][caractères… CR].
            0x0A => {
                let at = lin(ds, dx);
                let max = usize::from(mem.read_u8(at));
                if max > 0 {
                    let Some(line) = self.read_line(mem, host, max) else { return Ok(Out::Flow(Flow::Wait)) };
                    con_out(mem, host, b"\r");
                    mem.write_u8(at + 1, line.len() as u8);
                    mem.write(at + 2, &line);
                    mem.write_u8(at + 2 + line.len(), b'\r');
                }
                Out::Keep
            }
            0x0B => {
                let ready = self.key_ready(mem);
                cpu.set_lo(AX, if ready { 0xFF } else { 0 });
                Out::Keep
            }
            // Vide le tampon clavier puis appelle la fonction AL.
            // Reprise après attente : le tampon a déjà été vidé.
            0x0C => {
                if !self.flushed {
                    self.pending_scan = None;
                    while bios::read_key(mem).is_some() {}
                }
                let out = if matches!(al, 0x01 | 0x06 | 0x07 | 0x08 | 0x0A) {
                    cpu.set_hi(AX, al);
                    self.int21(cpu, mem, host)?
                } else {
                    Out::Keep
                };
                self.flushed = matches!(out, Out::Flow(Flow::Wait));
                out
            }
            0x0D => Out::Keep,
            0x0E => {
                if self.drives.is_mounted(cpu.lo(DX)) {
                    self.drives.current = cpu.lo(DX);
                }
                cpu.set_lo(AX, 26);
                Out::Keep
            }
            0x0F..=0x17 | 0x21..=0x24 | 0x27 | 0x28 => {
                self.int21_fcb(cpu, mem);
                Out::Keep
            }
            0x19 => {
                cpu.set_lo(AX, self.drives.current);
                Out::Keep
            }
            0x1A => {
                self.dta = (ds, dx);
                Out::Keep
            }
            0x25 => {
                let at = usize::from(al) * 4;
                mem.write_u16(at, dx);
                mem.write_u16(at + 2, ds);
                // Gestionnaire d'INT 0 du runtime de Turbo Pascal, installé
                // avant l'initialisation de CRT : étalonnage corrigé.
                if al == 0 {
                    crtfix::patch(mem, self.current_psp);
                }
                Out::Keep
            }
            0x29 => {
                self.parse_fcb(cpu, mem);
                Out::Keep
            }
            0x2A => {
                let t = bios::local_time();
                cpu.set_lo(AX, t.tm_wday as u8);
                cpu.set_w(CX, (t.tm_year + 1900) as u16);
                cpu.set_hi(DX, (t.tm_mon + 1) as u8);
                cpu.set_lo(DX, t.tm_mday as u8);
                Out::Keep
            }
            // Réglage de la date ou de l'heure : l'horloge hôte fait foi.
            0x2B | 0x2D => {
                cpu.set_lo(AX, 0);
                Out::Keep
            }
            0x2C => {
                let t = bios::local_time();
                let centis = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|d| d.subsec_millis() / 10)
                    .unwrap_or(0);
                cpu.set_hi(CX, t.tm_hour as u8);
                cpu.set_lo(CX, t.tm_min as u8);
                cpu.set_hi(DX, t.tm_sec as u8);
                cpu.set_lo(DX, centis as u8);
                Out::Keep
            }
            0x2E => {
                self.verify = al != 0;
                Out::Keep
            }
            0x2F => {
                cpu.set_sreg(ES, self.dta.0);
                cpu.set_w(BX, self.dta.1);
                Out::Keep
            }
            // Version : 5.00, OEM IBM (AL=01h : DOS en mémoire basse).
            0x30 => {
                cpu.set_w(AX, 0x0005);
                cpu.set_w(BX, 0);
                cpu.set_w(CX, 0);
                Out::Keep
            }
            0x31 => {
                let _ = memory::resize(mem, self.current_psp, dx.max(0x10));
                self.terminate(cpu, mem, al, 3)
            }
            0x33 => {
                match al {
                    0x00 => cpu.set_lo(DX, u8::from(self.break_flag)),
                    0x01 => self.break_flag = cpu.lo(DX) != 0,
                    0x05 => cpu.set_lo(DX, 3),
                    0x06 => {
                        cpu.set_w(BX, 0x0005);
                        cpu.set_w(DX, 0);
                    }
                    _ => cpu.set_lo(AX, 0xFF),
                }
                Out::Keep
            }
            0x34 => {
                cpu.set_sreg(ES, DOS_SEG);
                cpu.set_w(BX, INDOS_OFF);
                Out::Keep
            }
            0x35 => {
                let at = usize::from(al) * 4;
                cpu.set_w(BX, mem.read_u16(at));
                cpu.set_sreg(ES, mem.read_u16(at + 2));
                Out::Keep
            }
            0x36 => {
                let drive = if cpu.lo(DX) == 0 { self.drives.current } else { cpu.lo(DX) - 1 };
                if self.drives.is_mounted(drive) {
                    // 32 secteurs de 512 octets par unité : 1 Go, libre à moitié.
                    cpu.set_w(AX, 32);
                    cpu.set_w(CX, 512);
                    cpu.set_w(DX, 0xFFFF);
                    cpu.set_w(BX, 0x8000);
                } else {
                    cpu.set_w(AX, 0xFFFF);
                }
                Out::Keep
            }
            0x37 => {
                if al == 0 {
                    cpu.set_lo(DX, b'/');
                }
                cpu.set_lo(AX, 0);
                Out::Keep
            }
            0x38 => {
                // 24 octets seulement, comme MS-DOS (OLD_COUNTRY_SIZE : les
                // 10 octets réservés ne sont pas écrits). DKRNL32 de HX passe
                // un bloc DOS de 32 octets : écrire 34 octets écrasait le MCB
                // suivant (erreur 7 pour toute allocation ensuite, d'où
                // LoadLibrary refusé pour dsound.dll et vesa32.dll).
                mem.write(lin(ds, dx), &country_info()[..24]);
                cpu.set_w(BX, 1);
                Out::Ok
            }
            0x39 => Out::from(self.path_arg(mem, ds, dx).and_then(|(h, exists)| {
                if exists {
                    return Err(fs::ERR_ACCESS_DENIED);
                }
                vfs::create_dir(&h).map_err(|e| io_err(&e))
            })),
            0x3A => Out::from(self.path_arg(mem, ds, dx).and_then(|(h, exists)| {
                if !exists {
                    return Err(fs::ERR_PATH_NOT_FOUND);
                }
                vfs::remove_dir(&h).map_err(|e| match io_err(&e) {
                    fs::ERR_ACCESS_DENIED => ERR_CURRENT_DIR,
                    other => other,
                })
            })),
            0x3B => Out::from(self.drives.chdir(&read_asciiz(mem, lin(ds, dx)))),
            0x3C | 0x3D | 0x5B => {
                let path = read_asciiz(mem, lin(ds, dx));
                let (mode, create) = match ah {
                    0x3C => (2, 1),
                    0x5B => (2, 2),
                    _ => (al, 0),
                };
                let r = self.open(mem, &path, mode, create);
                Self::ax_result(cpu, r)
            }
            0x3E => Out::from(self.close(mem, cpu.w(BX))),
            0x3F => match self.read_handle(cpu, mem, host) {
                Some(r) => Self::ax_result(cpu, r),
                None => Out::Flow(Flow::Wait),
            },
            0x40 => {
                let r = self.write_handle(cpu, mem, host);
                Self::ax_result(cpu, r)
            }
            0x41 => Out::from(self.path_arg(mem, ds, dx).and_then(|(h, exists)| {
                if !exists || vfs::is_dir(&h) {
                    return Err(fs::ERR_FILE_NOT_FOUND);
                }
                vfs::remove_file(&h).map_err(|e| io_err(&e))
            })),
            0x42 => Out::from(self.seek(cpu, mem)),
            // COMMAND.COM virtuel (voir `virtual_comspec`) : présent, archive.
            0x43 if al == 0 && self.virtual_comspec(&read_asciiz(mem, lin(ds, dx))) => {
                cpu.set_w(CX, 0x20);
                Out::Ok
            }
            0x43 => {
                let r = self.path_arg(mem, ds, dx).and_then(|(h, exists)| {
                    if !exists {
                        return Err(fs::ERR_FILE_NOT_FOUND);
                    }
                    let meta = vfs::metadata(&h).map_err(|e| io_err(&e))?;
                    if al == 0 {
                        return Ok(Some(u16::from(fs::attributes(&meta))));
                    }
                    vfs::set_readonly(&h, cpu.lo(CX) & fs::ATTR_READONLY != 0).map_err(|e| io_err(&e))?;
                    Ok(None)
                });
                match r {
                    Ok(attr) => {
                        if let Some(a) = attr {
                            cpu.set_w(CX, a);
                        }
                        Out::Ok
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0x44 => Out::from(self.ioctl(cpu, mem)),
            0x45 => {
                let r = self.psp_handle(mem, cpu.w(BX)).and_then(|idx| {
                    let h = handles::assign(mem, self.current_psp, idx).ok_or(handles::ERR_TOO_MANY)?;
                    self.sft.add_ref(idx);
                    Ok(h)
                });
                Self::ax_result(cpu, r)
            }
            0x46 => {
                let target = cpu.w(CX);
                Out::from(self.psp_handle(mem, cpu.w(BX)).and_then(|idx| {
                    if usize::from(target) >= process::JFT_SIZE {
                        return Err(handles::ERR_BAD_HANDLE);
                    }
                    if self.psp_handle(mem, target).is_ok() {
                        self.close(mem, target)?;
                    }
                    handles::set(mem, self.current_psp, target, idx);
                    self.sft.add_ref(idx);
                    Ok(())
                }))
            }
            0x47 => {
                let drive = if cpu.lo(DX) == 0 { self.drives.current } else { cpu.lo(DX) - 1 };
                if self.drives.is_mounted(drive) {
                    let mut p = self.drives.cwd(drive).to_vec();
                    p.push(0);
                    mem.write(lin(ds, cpu.w(SI)), &p);
                    cpu.set_w(AX, 0x0100);
                    Out::Ok
                } else {
                    Out::Err(fs::ERR_INVALID_DRIVE)
                }
            }
            0x48 => match memory::alloc(mem, cpu.w(BX), self.current_psp, self.strategy) {
                Ok(seg) => {
                    cpu.set_w(AX, seg);
                    Out::Ok
                }
                Err((e, max)) => {
                    cpu.set_w(BX, max);
                    Out::Err(e)
                }
            },
            0x49 => Out::from(memory::free(mem, cpu.sreg(ES))),
            0x4A => match memory::resize(mem, cpu.sreg(ES), cpu.w(BX)) {
                Ok(()) => Out::Ok,
                Err((e, max)) => {
                    cpu.set_w(BX, max);
                    Out::Err(e)
                }
            },
            0x4B => self.int21_exec(cpu, mem, host)?,
            0x4C => self.terminate(cpu, mem, al, 0),
            0x4D => {
                cpu.set_w(AX, self.last_return);
                self.last_return = 0;
                Out::Ok
            }
            0x4E | 0x4F => Out::from(self.find(cpu, mem, ah == 0x4E)),
            0x50 => {
                self.current_psp = cpu.w(BX);
                Out::Keep
            }
            0x51 | 0x62 => {
                cpu.set_w(BX, self.current_psp);
                Out::Keep
            }
            0x52 => {
                cpu.set_sreg(ES, DOS_SEG);
                cpu.set_w(BX, LOL_OFF);
                Out::Keep
            }
            0x54 => {
                cpu.set_lo(AX, u8::from(self.verify));
                Out::Keep
            }
            0x56 => {
                let from = self.path_arg(mem, ds, dx);
                let to = self.path_arg(mem, cpu.sreg(ES), cpu.w(DI));
                Out::from(match (from, to) {
                    (Err(e), _) | (_, Err(e)) => Err(e),
                    (Ok((_, false)), _) => Err(fs::ERR_FILE_NOT_FOUND),
                    (Ok((_, true)), Ok((_, true))) => Err(fs::ERR_ACCESS_DENIED),
                    (Ok((a, true)), Ok((b, false))) => vfs::rename(&a, &b).map_err(|e| {
                        if e.kind() == std::io::ErrorKind::CrossesDevices { ERR_NOT_SAME_DEVICE } else { io_err(&e) }
                    }),
                })
            }
            0x57 => self.file_time(cpu, mem),
            0x58 => match al {
                0x00 => {
                    cpu.set_w(AX, self.strategy);
                    Out::Ok
                }
                0x01 => {
                    self.strategy = cpu.w(BX) & 0x43;
                    Out::Ok
                }
                // Pas d'UMB avant l'étape 6.
                0x02 => {
                    cpu.set_lo(AX, 0);
                    Out::Ok
                }
                0x03 if cpu.w(BX) == 0 => Out::Ok,
                _ => Out::Err(ERR_INVALID_FUNCTION),
            },
            0x59 => {
                cpu.set_w(AX, self.last_error);
                // Classe, action, lieu : génériques.
                cpu.set_w(BX, 0x0D01);
                cpu.set_hi(CX, 1);
                Out::Keep
            }
            0x5A => self.create_temp(cpu, mem),
            // Créer un PSP enfant en DX (bloc déjà réservé par l'appelant),
            // fin de mémoire SI ; il devient le PSP courant. Handles hérités
            // comme par EXEC, environnement partagé avec le parent.
            0x55 => {
                let parent = self.current_psp;
                let mut jft = [handles::UNUSED; process::JFT_SIZE];
                for (h, idx) in handles::all(mem, parent) {
                    let inherit = self.sft.get(idx).is_some_and(|e| e.mode & 0x80 == 0);
                    if usize::from(h) < jft.len() && inherit {
                        jft[usize::from(h)] = idx;
                        self.sft.add_ref(idx);
                    }
                }
                let init = PspInit {
                    parent,
                    env: mem.read_u16(psp_addr(parent, psp::ENV)),
                    top: cpu.w(SI),
                    jft,
                    tail: &[],
                    fcb1: process::BLANK_FCB,
                    fcb2: process::BLANK_FCB,
                };
                process::build_psp(mem, dx, &init);
                self.current_psp = dx;
                Out::Keep
            }
            // Adresse de la SDA (DS:SI), tailles en CX et DX.
            0x5D if al == 0x06 => {
                cpu.set_sreg(DS, DOS_SEG);
                cpu.set_w(SI, SDA_OFF);
                cpu.set_w(CX, SDA_SIZE);
                cpu.set_w(DX, SDA_SIZE);
                Out::Ok
            }
            // Erreur étendue fixée par l'appelant (DS:DX → AX) : seul le code est gardé.
            0x5D if al == 0x0A => {
                self.last_error = mem.read_u16(lin(ds, dx));
                Out::Ok
            }
            0x60 => {
                let path = read_asciiz(mem, lin(ds, cpu.w(SI)));
                match self.drives.split(&path) {
                    Ok((drive, comps)) => {
                        let mut p = fs::canonical(drive, &comps);
                        p.push(0);
                        mem.write(lin(cpu.sreg(ES), cpu.w(DI)), &p);
                        Out::Ok
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0x63 if al == 0x00 => {
                cpu.set_sreg(DS, bios::ROM_SEG);
                cpu.set_w(SI, bios::DBCS_TABLE);
                cpu.set_lo(AX, 0);
                Out::Ok
            }
            0x65 => self.country_ext(cpu, mem),
            0x66 if al == 0x01 => {
                cpu.set_w(BX, 437);
                cpu.set_w(DX, 437);
                Out::Ok
            }
            0x66 if al == 0x02 => Out::Ok,
            0x67 => Out::from(self.set_handle_count(mem, cpu.w(BX))),
            // Écriture sur disque d'un fichier : rien en attente côté hôte.
            0x68 | 0x6A => Out::from(self.psp_handle(mem, cpu.w(BX)).map(|_| ())),
            0x6C => self.ext_open(cpu, mem),
            // Noms longs (API de Windows 95).
            0x71 => self.lfn(cpu, mem),
            // Fonction inconnue : AL = 0, comme MS-DOS.
            _ => {
                cpu.set_lo(AX, 0);
                Out::Flow(Flow::Unhandled(ah))
            }
        })
    }

    /// AH=4Bh : AL=00h charger et exécuter, 03h charger un overlay.
    /// `COMMAND.COM` absent du disque : le shell est interne, mais les
    /// programmes le cherchent (COMSPEC) et le lancent avec « /C commande »
    /// (`system()` des bibliothèques C, installeurs).
    fn virtual_comspec(&self, path: &[u8]) -> bool {
        let name = path.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(b"");
        name.eq_ignore_ascii_case(b"COMMAND.COM")
            && !matches!(self.drives.resolve(path), Ok(Target::Path { exists: true, .. }))
    }

    fn int21_exec(&mut self, cpu: &mut Cpu, mem: &GuestMem, host: &mut Host) -> Result<Out> {
        let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(DX)));
        let block = lin(cpu.sreg(ES), cpu.w(BX));
        let (path, override_tail) = if cpu.lo(AX) == 0 && self.virtual_comspec(&path) {
            // « /C commande arguments » : commande interne exécutée ici,
            // programme lancé à la place de COMMAND.COM.
            let far = |at: usize| lin(mem.read_u16(at + 2), mem.read_u16(at));
            let tail_at = far(block + 2);
            let mut tail = vec![0u8; usize::from(mem.read_u8(tail_at)).min(126)];
            mem.read(tail_at + 1, &mut tail);
            let t = tail.trim_ascii();
            let Some(line) = t.strip_prefix(b"/C").or_else(|| t.strip_prefix(b"/c")) else {
                return Ok(Out::Err(fs::ERR_FILE_NOT_FOUND));
            };
            match self.shell_command(mem, host, line)? {
                shell::Action::Run(prog, args) => (prog, Some(args)),
                _ => {
                    self.last_return = 0;
                    return Ok(Out::Ok);
                }
            }
        } else {
            (path, None)
        };
        let (image, dos) = match self.read_program(&path) {
            Ok(r) => r,
            Err(e) => return Ok(Out::Err(e)),
        };
        Ok(self.exec_loaded(cpu, mem, block, &image, &dos, override_tail))
    }

    fn exec_loaded(&mut self, cpu: &mut Cpu, mem: &GuestMem, block: usize, image: &[u8], dos: &[u8], override_tail: Option<Vec<u8>>) -> Out {
        let (image, dos) = (image.to_vec(), dos.to_vec());
        match cpu.lo(AX) {
            0x00 => {
                let far = |at: usize| lin(mem.read_u16(at + 2), mem.read_u16(at));
                let tail_at = far(block + 2);
                let len = usize::from(mem.read_u8(tail_at)).min(126);
                let mut tail = vec![0u8; len];
                mem.read(tail_at + 1, &mut tail);
                let mut fcb1 = [0u8; 16];
                let mut fcb2 = [0u8; 16];
                mem.read(far(block + 6), &mut fcb1);
                mem.read(far(block + 10), &mut fcb2);
                // Programme lancé à la place de COMMAND.COM : sa ligne de
                // commande et ses FCB viennent des arguments de « /C ».
                let fcbs = match override_tail {
                    Some(args) => {
                        tail = args;
                        None
                    }
                    None => Some((fcb1, fcb2)),
                };
                let mut parent = cpu.clone();
                parent.seg_dirty = true;
                let env = mem.read_u16(block);
                match self.exec(mem, &image, &dos, &tail, env, fcbs, Some(parent)) {
                    Ok(child) => {
                        *cpu = child;
                        Out::Keep
                    }
                    Err(e) => Out::Err(e),
                }
            }
            0x03 => match process::parse(&image) {
                Ok(exe) => {
                    let seg = mem.read_u16(block);
                    let reloc = mem.read_u16(block + 2);
                    if exe.com {
                        mem.write(lin(seg, 0), exe.data);
                    } else {
                        exe.place(mem, seg, reloc);
                    }
                    Out::Ok
                }
                Err(e) => Out::Err(e),
            },
            _ => Out::Err(ERR_INVALID_FUNCTION),
        }
    }

    /// AH=57h : date et heure d'un fichier ouvert (AL=00h lire, 01h écrire).
    fn file_time(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let al = cpu.lo(AX);
        let (cx, dx) = (cpu.w(CX), cpu.w(DX));
        let r = self.psp_handle(mem, cpu.w(BX)).and_then(|idx| {
            let e = self.sft.get(idx).ok_or(handles::ERR_BAD_HANDLE)?;
            let Kind::File(f) = &mut e.kind else {
                return Ok(fs::dos_time(std::time::SystemTime::now()));
            };
            match al {
                0 => {
                    let m = f.file.metadata().map_err(|e| io_err(&e))?;
                    Ok(fs::dos_time(m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)))
                }
                1 => {
                    f.file.set_modified(fs::from_dos_time(cx, dx)).map_err(|e| io_err(&e))?;
                    Ok((cx, dx))
                }
                _ => Err(ERR_INVALID_FUNCTION),
            }
        });
        match r {
            Ok((t, d)) => {
                cpu.set_w(CX, t);
                cpu.set_w(DX, d);
                Out::Ok
            }
            Err(e) => Out::Err(e),
        }
    }

    /// AH=5Ah : fichier temporaire au nom unique dans le répertoire DS:DX.
    fn create_temp(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let at = lin(cpu.sreg(DS), cpu.w(DX));
        let mut base = read_asciiz(mem, at);
        if !base.is_empty() && !base.ends_with(b"\\") {
            base.push(b'\\');
        }
        for n in 0..1000u32 {
            let mut path = base.clone();
            path.extend_from_slice(format!("TMP{:05}.$$$", (std::process::id() + n) % 100_000).as_bytes());
            match self.open(mem, &path, 2, 2) {
                Ok(h) => {
                    path.push(0);
                    mem.write(at, &path);
                    cpu.set_w(AX, h);
                    return Out::Ok;
                }
                Err(ERR_FILE_EXISTS) => {}
                Err(e) => return Out::Err(e),
            }
        }
        Out::Err(fs::ERR_ACCESS_DENIED)
    }

    /// AH=67h : agrandit la JFT du processus (bloc alloué au besoin).
    fn set_handle_count(&mut self, mem: &GuestMem, count: u16) -> std::result::Result<(), u16> {
        let p = self.current_psp;
        let old = mem.read_u16(psp_addr(p, psp::JFT_COUNT));
        if count <= old {
            return Ok(());
        }
        let count = count.min(255);
        let seg = memory::alloc(mem, count.div_ceil(16), p, self.strategy).map_err(|(e, _)| e)?;
        let mut jft = vec![handles::UNUSED; usize::from(count)];
        for (h, idx) in handles::all(mem, p) {
            jft[usize::from(h)] = idx;
        }
        mem.write(lin(seg, 0), &jft);
        mem.write_u16(psp_addr(p, psp::JFT_COUNT), count);
        mem.write_u16(psp_addr(p, psp::JFT_PTR), 0);
        mem.write_u16(psp_addr(p, psp::JFT_PTR + 2), seg);
        Ok(())
    }

    /// AH=6Ch : ouverture étendue (DS:SI nom, BL mode, DL action).
    fn ext_open(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        let path = read_asciiz(mem, lin(cpu.sreg(DS), cpu.w(SI)));
        let action = cpu.lo(DX);
        let mode = cpu.lo(BX);
        let exists =
            matches!(self.drives.resolve(&path), Ok(Target::Path { exists: true, .. }) | Ok(Target::Device(_)));
        let r = match (exists, action & 0x0F, action >> 4) {
            (true, 1, _) => self.open(mem, &path, mode, 0).map(|h| (h, 1)),
            (true, 2, _) => self.open(mem, &path, mode, 1).map(|h| (h, 3)),
            (true, _, _) => Err(ERR_FILE_EXISTS),
            (false, _, 1) => self.open(mem, &path, mode, 1).map(|h| (h, 2)),
            (false, _, _) => Err(fs::ERR_FILE_NOT_FOUND),
        };
        match r {
            Ok((h, status)) => {
                cpu.set_w(AX, h);
                cpu.set_w(CX, status);
                Out::Ok
            }
            Err(e) => Out::Err(e),
        }
    }

    /// AH=65h : informations nationales étendues, conversion en majuscules.
    fn country_ext(&mut self, cpu: &mut Cpu, mem: &GuestMem) -> Out {
        match cpu.lo(AX) {
            0x01 => {
                // N° d'info, taille, pays 1, page de codes 437, puis le bloc de AH=38h.
                let mut info = vec![1u8, 0, 0, 1, 0, 0xB5, 0x01];
                info[1..3].copy_from_slice(&38u16.to_le_bytes());
                info.extend_from_slice(&country_info());
                let n = info.len().min(usize::from(cpu.w(CX)));
                mem.write(lin(cpu.sreg(ES), cpu.w(DI)), &info[..n]);
                cpu.set_w(CX, n as u16);
                Out::Ok
            }
            0x20 => {
                cpu.set_lo(DX, cpu.lo(DX).to_ascii_uppercase());
                Out::Ok
            }
            0x21 | 0x22 => {
                let at = lin(cpu.sreg(DS), cpu.w(DX));
                let len = if cpu.lo(AX) == 0x21 { usize::from(cpu.w(CX)) } else { read_asciiz(mem, at).len() };
                let mut s = vec![0u8; len];
                mem.read(at, &mut s);
                s.make_ascii_uppercase();
                mem.write(at, &s);
                Out::Ok
            }
            _ => Out::Err(ERR_INVALID_FUNCTION),
        }
    }

    /// AH=29h : analyse un nom de fichier (DS:SI) vers un FCB (ES:DI).
    fn parse_fcb(&self, cpu: &mut Cpu, mem: &GuestMem) {
        let flags = cpu.lo(AX);
        let seg = cpu.sreg(DS);
        let mut off = cpu.w(SI);
        let fcb = lin(cpu.sreg(ES), cpu.w(DI));
        let peek = |o: u16| mem.read_u8(lin(seg, o));
        if flags & 1 != 0 {
            while matches!(peek(off), b' ' | b'\t') {
                off += 1;
            }
            if matches!(peek(off), b':' | b'.' | b';' | b',' | b'=' | b'+') {
                off += 1;
            }
        }
        while matches!(peek(off), b' ' | b'\t') {
            off += 1;
        }
        let mut result = 0u8;
        if peek(off + 1) == b':' && peek(off).is_ascii_alphabetic() {
            let d = peek(off).to_ascii_uppercase() - b'A';
            mem.write_u8(fcb, d + 1);
            if !self.drives.is_mounted(d) {
                result = 0xFF;
            }
            off += 2;
        } else if flags & 2 == 0 {
            mem.write_u8(fcb, 0);
        }
        let terminator = |c: u8| c <= b' ' || b".\"/\\[]:|<>+=;,".contains(&c);
        // Champ nom (8) ou extension (3) ; `keep` : garder l'ancien si absent.
        let field = |len: usize, at: usize, keep: bool, off: &mut u16| {
            let mut buf = [b' '; 8];
            let mut n = 0;
            let mut any = false;
            loop {
                let c = peek(*off);
                if terminator(c) {
                    break;
                }
                *off += 1;
                any = true;
                if n < len {
                    if c == b'*' {
                        buf[n..len].fill(b'?');
                        n = len;
                    } else {
                        buf[n] = c.to_ascii_uppercase();
                        n += 1;
                    }
                }
            }
            if any || !keep {
                mem.write(fcb + at, &buf[..len]);
            }
            buf[..len].contains(&b'?')
        };
        let mut wild = field(8, 1, flags & 4 != 0, &mut off);
        if peek(off) == b'.' {
            off += 1;
            wild |= field(3, 9, flags & 8 != 0, &mut off);
        } else if flags & 8 == 0 {
            mem.write(fcb + 9, b"   ");
        }
        if wild && result == 0 {
            result = 1;
        }
        cpu.set_w(SI, off);
        cpu.set_lo(AX, result);
    }

}

/// Chaîne de MCB : (propriétaire, taille) de chaque bloc (tests).
#[cfg(test)]
mod env_tests {
    #[test]
    fn set_adds_and_replaces_variables() {
        let mut dos = super::Dos::new();
        dos.env = vec![("DPMILDR".into(), "16".into()), ("PATH".into(), "C:\\HX\\BIN".into())];
        let env = dos.master_env();
        let vars: Vec<&[u8]> = env.split(|&b| b == 0).filter(|v| !v.is_empty()).collect();
        assert!(vars.contains(&&b"DPMILDR=16"[..]) && vars.contains(&&b"PATH=C:\\HX\\BIN"[..]), "{vars:?}");
        assert_eq!(vars.iter().filter(|v| v.starts_with(b"PATH=")).count(), 1);
        assert!(env.ends_with(&[0, 0]));
        dos.env = vec![("BIG".into(), "x".repeat(600))];
        assert!(!dos.master_env().windows(4).any(|w| w == b"BIG="));
    }
}

#[cfg(test)]
pub fn mcb_chain(mem: &GuestMem) -> Vec<(u16, u16)> {
    memory::chain(mem).expect("MCB chain").iter().map(|m| (m.owner, m.size)).collect()
}

/// FCB 1 et 2 du PSP d'après les deux premiers mots de la ligne de commande.
fn default_fcbs(tail: &[u8]) -> ([u8; 16], [u8; 16]) {
    let mut fcbs = [process::BLANK_FCB; 2];
    let words = tail.split(|&c| c == b' ' || c == b'\t').filter(|w| !w.is_empty() && !w.starts_with(b"/"));
    for (fcb, w) in fcbs.iter_mut().zip(words) {
        let (drive, name) = match w {
            [d, b':', rest @ ..] if d.is_ascii_alphabetic() => (d.to_ascii_uppercase() - b'A' + 1, rest),
            _ => (0, w),
        };
        fcb[0] = drive;
        let base = name.rsplit(|&c| c == b'\\').next().unwrap_or(name);
        fcb[1..12].copy_from_slice(&fs::fcb_pattern(base));
    }
    (fcbs[0], fcbs[1])
}

/// Informations nationales (États-Unis), format de AH=38h.
fn country_info() -> [u8; 34] {
    let mut c = [0u8; 34];
    c[2] = b'$';
    c[7] = b',';
    c[9] = b'.';
    c[11] = b'-';
    c[13] = b':';
    c[16] = 2;
    // Routine de conversion de casse : RETF en ROM.
    c[18..20].copy_from_slice(&bios::CASE_MAP.to_le_bytes());
    c[20..22].copy_from_slice(&bios::ROM_SEG.to_le_bytes());
    c[22] = b',';
    c
}

fn read_asciiz(mem: &GuestMem, at: usize) -> Vec<u8> {
    let mut s = Vec::new();
    while s.len() < 128 {
        let c = mem.read_u8(at + s.len());
        if c == 0 {
            break;
        }
        s.push(c);
    }
    s
}

/// Sortie console : télétype du BIOS sur la page active.
fn con_out(mem: &GuestMem, host: &mut Host, bytes: &[u8]) {
    let page = bios::active_page(mem);
    for &c in bytes {
        bios::tty(mem, host.text.as_mut(), page, c, None);
    }
}

/// Chargeur .COM réduit, sans DOS (bancs de mesure de l'étape 0).
pub fn load_com(mem: &GuestMem, image: &[u8]) -> std::result::Result<Cpu, String> {
    const SEG: u16 = 0x1000;
    if image.len() > 0xFF00 - 2 {
        return Err(".COM file too large".into());
    }
    let p = lin(SEG, 0);
    mem.write(p, &[0xCD, 0x20]);
    mem.write(p + 0x80, &[0, 0x0D]);
    mem.write(p + 0x100, image);
    mem.write_u16(p + 0xFFFE, 0);
    Ok(Cpu::real_mode(SEG, 0x100, 0xFFFE))
}
