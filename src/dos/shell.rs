// SPDX-License-Identifier: GPL-3.0-or-later
//! Shell interne, côté hôte : invite, commandes internes, lancement des
//! programmes. Il joue le rôle de COMMAND.COM (le PSP racine est le sien).

use crate::backend::Result;
use crate::host::{Host, InputEvent};
use crate::keyboard;
use crate::mem::GuestMem;

use super::fs::{self, Target};
use super::vfs;
use super::{Dos, con_out, process::psp, psp_addr};

/// Suite à donner à une ligne de commande.
pub enum Action {
    Continue,
    /// Programme à lancer : chemin DOS, ligne de commande.
    Run(Vec<u8>, Vec<u8>),
    /// `PAUSE` : message, puis attente d'une touche.
    Pause,
    Exit,
}

const BAD_COMMAND: &[u8] = b"Bad command or file name\r\n";

fn eq(a: &[u8], b: &str) -> bool {
    a.eq_ignore_ascii_case(b.as_bytes())
}

impl Dos {
    /// Invite « C:\REP> ».
    pub fn prompt(&mut self, mem: &GuestMem, host: &mut Host) {
        let d = self.drives.current;
        let mut p = fs::canonical(d, &[]);
        p.extend_from_slice(self.drives.cwd(d));
        p.push(b'>');
        con_out(mem, host, &p);
    }

    /// Lit une ligne au clavier ; `None` quand l'entrée est épuisée. Le shell
    /// tourne côté hôte, sans programme invité pour servir l'IRQ 1 : il lit
    /// donc l'entrée de l'hôte directement.
    /// `redraw` rafraîchit l'affichage (terminal, fenêtre) avant chaque attente.
    pub fn shell_read(
        &mut self,
        mem: &GuestMem,
        host: &mut Host,
        redraw: &mut dyn FnMut(&GuestMem, &mut Host),
    ) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        let mut shift = 0u8;
        loop {
            redraw(mem, host);
            let ascii = match host.input.wait()? {
                InputEvent::Text(c) => keyboard::char_word(c).map(|w| w as u8),
                InputEvent::Key(code) => {
                    let (scan, make) = (code & 0x7F, code & 0x80 == 0);
                    if let Some(bit) = keyboard::modifier(scan) {
                        shift = if make { shift | bit } else { shift & !bit };
                        None
                    } else if make {
                        keyboard::key_word(scan, shift).map(|w| w as u8)
                    } else {
                        None
                    }
                }
                InputEvent::Quit => {
                    host.quit = true;
                    return None;
                }
                // Disque changé à l'invite : appliqué par la machine avant la commande.
                e @ (InputEvent::InsertFloppy(_) | InputEvent::InsertCd(_)) => {
                    self.pending_media.push(e);
                    None
                }
                // Vitesse : sans objet au shell (aucun programme ne tourne).
                InputEvent::Mouse { .. }
                | InputEvent::Speed(_)
                | InputEvent::NextDisk
                | InputEvent::Until(_)
                | InputEvent::JoyPresent(_)
                | InputEvent::JoyAxis(..)
                | InputEvent::JoyButton(..) => None,
                InputEvent::Wait(ms) => {
                    std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
                    None
                }
            };
            match ascii {
                Some(b'\r') => {
                    con_out(mem, host, b"\r\n");
                    return Some(line);
                }
                Some(0x08 | 0x7F) => {
                    if line.pop().is_some() {
                        con_out(mem, host, b"\x08 \x08");
                    }
                }
                Some(c) if c >= b' ' && line.len() < 127 => {
                    line.push(c);
                    con_out(mem, host, &[c]);
                }
                _ => {}
            }
        }
    }

    /// Attend une touche (`PAUSE`) ; faux quand l'entrée est épuisée.
    pub fn shell_wait_key(&mut self, mem: &GuestMem, host: &mut Host, redraw: &mut dyn FnMut(&GuestMem, &mut Host)) -> bool {
        loop {
            redraw(mem, host);
            match host.input.wait() {
                None => return false,
                Some(InputEvent::Text(_)) => return true,
                Some(InputEvent::Key(code)) => {
                    if code & 0x80 == 0 && keyboard::modifier(code & 0x7F).is_none() {
                        return true;
                    }
                }
                Some(InputEvent::Quit) => {
                    host.quit = true;
                    return false;
                }
                Some(InputEvent::Wait(ms)) => std::thread::sleep(std::time::Duration::from_millis(u64::from(ms))),
                Some(e @ (InputEvent::InsertFloppy(_) | InputEvent::InsertCd(_))) => self.pending_media.push(e),
                Some(_) => {}
            }
        }
    }

    pub fn shell_command(&mut self, mem: &GuestMem, host: &mut Host, line: &[u8]) -> Result<Action> {
        let line = line.trim_ascii();
        if line.is_empty() {
            return Ok(Action::Continue);
        }
        // « CD.. », « DIR/W » : la commande s'arrête aussi sur « . », « / », « \ ».
        let split = line.iter().position(|&c| c == b' ' || c == b'/' || c == b'\\' || c == b'.').unwrap_or(line.len());
        let (cmd, rest) = line.split_at(split);
        let arg = rest.trim_ascii();
        let mut out = |s: &[u8]| con_out(mem, host, s);
        match cmd {
            [d, b':'] if d.is_ascii_alphabetic() && arg.is_empty() => {
                let n = d.to_ascii_uppercase() - b'A';
                if self.drives.is_mounted(n) {
                    self.drives.current = n;
                } else {
                    out(b"Invalid drive specification\r\n");
                }
            }
            _ if eq(cmd, "EXIT") => return Ok(Action::Exit),
            _ if eq(cmd, "CLS") => {
                crate::bios::clear_screen(mem);
            }
            _ if eq(cmd, "VER") => out(b"\r\nMS-DOS version 5.00 (HLE DOS under KVM)\r\n\r\n"),
            _ if eq(cmd, "ECHO") => {
                if arg.eq_ignore_ascii_case(b"off") {
                    self.echo = false;
                } else if arg.eq_ignore_ascii_case(b"on") {
                    self.echo = true;
                } else if arg.is_empty() {
                    out(if self.echo { b"ECHO is on\r\n" } else { b"ECHO is off\r\n" });
                } else {
                    // « ECHO. » et « ECHO texte » ; « > fichier » écrit le texte dans le fichier.
                    let text = rest.strip_prefix(b" ").or_else(|| rest.strip_prefix(b".")).unwrap_or(rest);
                    let (text, target) = match text.iter().position(|&c| c == b'>') {
                        Some(i) => (&text[..i], Some(&text[i..])),
                        None => (text, None),
                    };
                    match target {
                        None => {
                            out(text);
                            out(b"\r\n");
                        }
                        Some(t) => {
                            let append = t.starts_with(b">>");
                            let name = t.iter().position(|&c| c != b'>').map_or(&t[t.len()..], |i| &t[i..]).trim_ascii();
                            if !name.eq_ignore_ascii_case(b"nul")
                                && let Ok(Target::Path { host: h, .. }) = self.drives.resolve(name)
                            {
                                let mut data = if append { vfs::read(&h).unwrap_or_default() } else { Vec::new() };
                                data.extend_from_slice(text.trim_ascii_end());
                                data.extend_from_slice(b"\r\n");
                                if !vfs::is_virtual(&h) {
                                    let _ = std::fs::write(&h, &data);
                                }
                            }
                        }
                    }
                }
            }
            _ if eq(cmd, "REM") || eq(cmd, "BREAK") || eq(cmd, "VERIFY") || eq(cmd, "PROMPT") || eq(cmd, "MODE") || eq(cmd, "CTTY") => {}
            _ if eq(cmd, "PAUSE") => return Ok(Action::Pause),
            _ if eq(cmd, "GOTO") => self.batch_goto(mem, host, arg),
            _ if eq(cmd, "CALL") => return self.batch_call(mem, host, arg),
            _ if eq(cmd, "SHIFT") => self.batch_shift(),
            _ if eq(cmd, "IF") => return self.batch_if(mem, host, arg),
            _ if eq(cmd, "FOR") => self.batch_for(mem, host, arg),
            // Chargement en mémoire haute : sans objet, le programme est lancé tel quel.
            _ if eq(cmd, "LH") || eq(cmd, "LOADHIGH") => return self.shell_command(mem, host, arg),
            _ if eq(cmd, "PATH") => {
                let line = if arg.is_empty() { b"PATH".to_vec() } else { [&b"SET PATH="[..], arg.strip_prefix(b"=").unwrap_or(arg)].concat() };
                if !arg.is_empty() {
                    return self.shell_command(mem, host, &line);
                }
            }
            _ if eq(cmd, "SET") && arg.is_empty() => {
                for var in self.master_vars(mem) {
                    out(&var);
                    out(b"\r\n");
                }
            }
            // SET NOM=VALEUR : nom en majuscules, valeur telle quelle ; une
            // valeur vide retire la variable.
            _ if eq(cmd, "SET") => match arg.iter().position(|&c| c == b'=') {
                Some(i) if i > 0 => {
                    let name = arg[..i].trim_ascii().to_ascii_uppercase();
                    let value = &arg[i + 1..];
                    let mut vars: Vec<Vec<u8>> =
                        self.master_vars(mem).into_iter().filter(|v| !v.starts_with(&[&name[..], b"="].concat())).collect();
                    if !value.is_empty() {
                        vars.push([&name[..], b"=", value].concat());
                    }
                    if !self.write_master_vars(mem, &vars) {
                        out(b"Out of environment space\r\n");
                    }
                }
                _ => out(b"Syntax error\r\n"),
            },
            _ if eq(cmd, "CD") || eq(cmd, "CHDIR") => {
                if arg.is_empty() {
                    let d = self.drives.current;
                    let mut p = fs::canonical(d, &[]);
                    p.extend_from_slice(self.drives.cwd(d));
                    out(&p);
                    out(b"\r\n");
                } else if self.drives.chdir(arg).is_err() {
                    out(b"Invalid directory\r\n");
                }
            }
            _ if eq(cmd, "MD") || eq(cmd, "MKDIR") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: false, .. }) if vfs::create_dir(&h).is_ok() => {}
                _ => out(b"Unable to create directory\r\n"),
            },
            _ if eq(cmd, "RD") || eq(cmd, "RMDIR") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if vfs::remove_dir(&h).is_ok() => {}
                _ => out(b"Invalid path, not directory,\r\nor directory not empty\r\n"),
            },
            _ if eq(cmd, "DEL") || eq(cmd, "ERASE") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if vfs::is_file(&h) && vfs::remove_file(&h).is_ok() => {}
                _ => out(b"File not found\r\n"),
            },
            // REN source nouveau-nom : le nouveau nom reste dans le répertoire de la source.
            _ if eq(cmd, "REN") || eq(cmd, "RENAME") => {
                let mut words = arg.split(|&c| c == b' ').filter(|w| !w.is_empty());
                let renamed = match (words.next(), words.next()) {
                    (Some(from), Some(to)) => match self.drives.resolve(from) {
                        Ok(Target::Path { host: h, exists: true, .. }) => {
                            let name = String::from_utf8_lossy(to).to_ascii_uppercase();
                            let dest = h.with_file_name(name);
                            !vfs::exists(&dest) && vfs::rename(&h, &dest).is_ok()
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !renamed {
                    out(b"Duplicate file name or file not found\r\n");
                }
            }
            _ if eq(cmd, "TYPE") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if vfs::is_file(&h) => {
                    let data = vfs::read(&h).unwrap_or_default();
                    // Le texte s'arrête au premier Ctrl-Z, comme sous DOS.
                    let end = data.iter().position(|&c| c == 0x1A).unwrap_or(data.len());
                    out(&data[..end]);
                }
                _ => out(b"File not found\r\n"),
            },
            _ if eq(cmd, "DIR") => self.dir(mem, host, arg),
            _ => {
                // Programme : tout le premier mot, chemin compris (« C:\JEU\X.EXE »).
                let end = line.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(line.len());
                let (word, args) = line.split_at(end);
                return Ok(match self.find_program(word, mem) {
                    Some(path) if path.rsplit(|&b| b == b'.').next().is_some_and(|e| eq(e, "BAT")) => {
                        if !self.start_batch(&path, args) {
                            out(BAD_COMMAND);
                        }
                        Action::Continue
                    }
                    Some(path) => Action::Run(path, args.to_vec()),
                    None => {
                        out(BAD_COMMAND);
                        Action::Continue
                    }
                });
            }
        }
        Ok(Action::Continue)
    }

    /// Variables de l'environnement maître (celui du shell, hérité par les
    /// programmes qu'il lance).
    pub(super) fn master_vars(&self, mem: &GuestMem) -> Vec<Vec<u8>> {
        let Some(root) = self.procs.first() else { return Vec::new() };
        let env = mem.read_u16(psp_addr(root.psp, psp::ENV));
        let mut at = usize::from(env) << 4;
        let mut vars = Vec::new();
        loop {
            let var = super::read_asciiz(mem, at);
            if var.is_empty() {
                break;
            }
            at += var.len() + 1;
            vars.push(var);
        }
        vars
    }

    /// Réécrit l'environnement maître ; faux s'il ne tient pas dans son bloc.
    fn write_master_vars(&self, mem: &GuestMem, vars: &[Vec<u8>]) -> bool {
        let mut block: Vec<u8> = vars.iter().flat_map(|v| v.iter().copied().chain([0])).collect();
        block.push(0);
        if block.len() > usize::from(super::MASTER_ENV_PARAS) * 16 {
            return false;
        }
        let env = mem.read_u16(psp_addr(self.procs[0].psp, psp::ENV));
        let at = usize::from(env) << 4;
        mem.fill(at, usize::from(super::MASTER_ENV_PARAS) * 16, 0);
        mem.write(at, &block);
        true
    }

    /// Programme à lancer : dans le répertoire courant, puis dans les
    /// répertoires du PATH maître ; extensions .COM puis .EXE.
    pub(super) fn find_program(&self, name: &[u8], mem: &GuestMem) -> Option<Vec<u8>> {
        let has_ext = name.iter().rposition(|&c| c == b'.').is_some_and(|i| !name[i..].contains(&b'\\'));
        let candidates: Vec<Vec<u8>> = if has_ext {
            vec![name.to_vec()]
        } else {
            [&b".COM"[..], b".EXE", b".BAT"].iter().map(|e| [name, e].concat()).collect()
        };
        let path = self.master_vars(mem).into_iter().find_map(|v| v.strip_prefix(b"PATH=").map(<[u8]>::to_vec)).unwrap_or_default();
        let mut dirs: Vec<Vec<u8>> = vec![Vec::new()];
        for d in path.split(|&c| c == b';').map(<[u8]>::trim_ascii).filter(|d| !d.is_empty()) {
            dirs.push(if d.ends_with(b"\\") { d.to_vec() } else { [d, b"\\"].concat() });
        }
        for dir in &dirs {
            if !dir.is_empty() && name.contains(&b'\\') {
                break;
            }
            for c in &candidates {
                let path = [&dir[..], c].concat();
                if let Ok(Target::Path { exists: true, host, dos }) = self.drives.resolve(&path)
                    && vfs::is_file(&host)
                {
                    let ext = dos.rsplit(|&b| b == b'.').next().unwrap_or(b"");
                    if eq(ext, "COM") || eq(ext, "EXE") || eq(ext, "BAT") {
                        return Some(dos);
                    }
                }
            }
        }
        None
    }

    fn dir(&mut self, mem: &GuestMem, host: &mut Host, arg: &[u8]) {
        let mut out = |s: &[u8]| con_out(mem, host, s);
        let pattern: Vec<u8> = {
            // Un répertoire seul : son contenu.
            let a = arg.split(|&c| c == b'/').next().unwrap_or(b"").trim_ascii();
            match self.drives.resolve(a) {
                Ok(Target::Path { host: h, exists: true, .. }) if vfs::is_dir(&h) && !a.is_empty() => {
                    [a, b"\\*.*"].concat()
                }
                _ if a.is_empty() => b"*.*".to_vec(),
                _ => a.to_vec(),
            }
        };
        let Ok((dir, pat)) = self.drives.search(&pattern) else {
            out(b"Invalid path\r\n");
            return;
        };
        let shown = match self.drives.split(&pattern) {
            Ok((d, mut comps)) => {
                comps.pop();
                fs::canonical(d, &comps)
            }
            Err(_) => b"?".to_vec(),
        };
        out(b"\r\n Volume in drive ");
        out(&shown[..1]);
        match self.volume_label(shown[0].wrapping_sub(b'A')) {
            Some(label) => {
                out(b" is ");
                out(label.as_bytes());
                out(b"\r\n Directory of ");
            }
            None => out(b" has no label\r\n Directory of "),
        }
        out(&shown);
        out(b"\r\n\r\n");
        let (mut files, mut bytes) = (0u32, 0u64);
        for e in fs::list(&dir).into_iter().filter(|e| fs::fcb_matches(&pat, &e.short)) {
            let p = fs::fcb_pattern(&e.short);
            let (time, date) = fs::dos_time(e.meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
            let size = if e.meta.is_dir() {
                "<DIR>    ".to_string()
            } else {
                files += 1;
                bytes += e.meta.len();
                format!("{:>9}", e.meta.len())
            };
            let line = format!(
                "{} {} {} {:02}/{:02}/{:02}  {:2}:{:02}\r\n",
                String::from_utf8_lossy(&p[..8]),
                String::from_utf8_lossy(&p[8..]),
                size,
                (date >> 5) & 15,
                date & 31,
                ((date >> 9) + 80) % 100,
                time >> 11,
                (time >> 5) & 63
            );
            out(line.as_bytes());
        }
        out(format!("{files:>9} file(s) {bytes:>12} bytes\r\n\r\n").as_bytes());
    }
}
