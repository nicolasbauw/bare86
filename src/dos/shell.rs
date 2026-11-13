// SPDX-License-Identifier: GPL-3.0-or-later
//! Shell interne, côté hôte : invite, commandes internes, lancement des
//! programmes. Il joue le rôle de COMMAND.COM (le PSP racine est le sien).

use crate::backend::Result;
use crate::host::{Host, InputEvent};
use crate::keyboard;
use crate::mem::GuestMem;

use super::fs::{self, Target};
use super::{Dos, con_out, process::psp, psp_addr};

/// Suite à donner à une ligne de commande.
pub enum Action {
    Continue,
    /// Programme à lancer : chemin DOS, ligne de commande.
    Run(Vec<u8>, Vec<u8>),
    Exit,
}

const BAD_COMMAND: &[u8] = b"Commande ou nom de fichier incorrect\r\n";

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
                // Vitesse : sans objet au shell (aucun programme ne tourne).
                InputEvent::Mouse { .. }
                | InputEvent::Speed(_)
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
                    out(b"Lecteur non valide\r\n");
                }
            }
            _ if eq(cmd, "EXIT") => return Ok(Action::Exit),
            _ if eq(cmd, "CLS") => {
                crate::bios::clear_screen(mem);
            }
            _ if eq(cmd, "VER") => out(b"\r\nMS-DOS version 5.00 (DOS en HLE sous KVM)\r\n\r\n"),
            _ if eq(cmd, "ECHO") => {
                let text = rest.strip_prefix(b" ").unwrap_or(rest);
                out(text);
                out(b"\r\n");
            }
            _ if eq(cmd, "SET") && arg.is_empty() => {
                let env = mem.read_u16(psp_addr(self.procs[0].psp, psp::ENV));
                let mut at = usize::from(env) << 4;
                loop {
                    let var = super::read_asciiz(mem, at);
                    if var.is_empty() {
                        break;
                    }
                    at += var.len() + 1;
                    out(&var);
                    out(b"\r\n");
                }
            }
            _ if eq(cmd, "CD") || eq(cmd, "CHDIR") => {
                if arg.is_empty() {
                    let d = self.drives.current;
                    let mut p = fs::canonical(d, &[]);
                    p.extend_from_slice(self.drives.cwd(d));
                    out(&p);
                    out(b"\r\n");
                } else if self.drives.chdir(arg).is_err() {
                    out(b"Repertoire non valide\r\n");
                }
            }
            _ if eq(cmd, "MD") || eq(cmd, "MKDIR") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: false, .. }) if std::fs::create_dir(&h).is_ok() => {}
                _ => out(b"Impossible de creer le repertoire\r\n"),
            },
            _ if eq(cmd, "RD") || eq(cmd, "RMDIR") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if std::fs::remove_dir(&h).is_ok() => {}
                _ => out(b"Chemin non valide, ou repertoire non vide\r\n"),
            },
            _ if eq(cmd, "DEL") || eq(cmd, "ERASE") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if h.is_file() && std::fs::remove_file(&h).is_ok() => {}
                _ => out(b"Fichier introuvable\r\n"),
            },
            // REN source nouveau-nom : le nouveau nom reste dans le répertoire de la source.
            _ if eq(cmd, "REN") || eq(cmd, "RENAME") => {
                let mut words = arg.split(|&c| c == b' ').filter(|w| !w.is_empty());
                let renamed = match (words.next(), words.next()) {
                    (Some(from), Some(to)) => match self.drives.resolve(from) {
                        Ok(Target::Path { host: h, exists: true, .. }) => {
                            let name = String::from_utf8_lossy(to).to_ascii_uppercase();
                            let dest = h.with_file_name(name);
                            !dest.exists() && std::fs::rename(&h, dest).is_ok()
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !renamed {
                    out(b"Fichier introuvable ou nom en double\r\n");
                }
            }
            _ if eq(cmd, "TYPE") => match self.drives.resolve(arg) {
                Ok(Target::Path { host: h, exists: true, .. }) if h.is_file() => {
                    let data = std::fs::read(h).unwrap_or_default();
                    // Le texte s'arrête au premier Ctrl-Z, comme sous DOS.
                    let end = data.iter().position(|&c| c == 0x1A).unwrap_or(data.len());
                    out(&data[..end]);
                }
                _ => out(b"Fichier introuvable\r\n"),
            },
            _ if eq(cmd, "DIR") => self.dir(mem, host, arg),
            _ => {
                // Programme : tout le premier mot, chemin compris (« C:\JEU\X.EXE »).
                let end = line.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(line.len());
                let (word, args) = line.split_at(end);
                return Ok(match self.find_program(word) {
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

    /// Programme à lancer : dans le répertoire courant, puis à la racine de
    /// C: (le PATH maître) ; extensions .COM puis .EXE.
    fn find_program(&self, name: &[u8]) -> Option<Vec<u8>> {
        let has_ext = name.iter().rposition(|&c| c == b'.').is_some_and(|i| !name[i..].contains(&b'\\'));
        let candidates: Vec<Vec<u8>> = if has_ext {
            vec![name.to_vec()]
        } else {
            [&b".COM"[..], b".EXE"].iter().map(|e| [name, e].concat()).collect()
        };
        let dirs: [&[u8]; 2] = [b"", b"C:\\"];
        for dir in dirs {
            if !dir.is_empty() && name.contains(&b'\\') {
                break;
            }
            for c in &candidates {
                let path = [dir, c].concat();
                if let Ok(Target::Path { exists: true, host, dos }) = self.drives.resolve(&path)
                    && host.is_file()
                {
                    let ext = dos.rsplit(|&b| b == b'.').next().unwrap_or(b"");
                    if eq(ext, "COM") || eq(ext, "EXE") {
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
                Ok(Target::Path { host: h, exists: true, .. }) if h.is_dir() && !a.is_empty() => {
                    [a, b"\\*.*"].concat()
                }
                _ if a.is_empty() => b"*.*".to_vec(),
                _ => a.to_vec(),
            }
        };
        let Ok((dir, pat)) = self.drives.search(&pattern) else {
            out(b"Chemin non valide\r\n");
            return;
        };
        let shown = match self.drives.split(&pattern) {
            Ok((d, mut comps)) => {
                comps.pop();
                fs::canonical(d, &comps)
            }
            Err(_) => b"?".to_vec(),
        };
        out(b"\r\n Le volume dans le lecteur ");
        out(&shown[..1]);
        out(b" n'a pas de nom\r\n Repertoire de ");
        out(&shown);
        out(b"\r\n\r\n");
        let (mut files, mut bytes) = (0u32, 0u64);
        for e in fs::list(&dir).into_iter().filter(|e| fs::fcb_matches(&pat, &e.short)) {
            let p = fs::fcb_pattern(&e.short);
            let (time, date) = fs::dos_time(e.meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
            let size = if e.meta.is_dir() {
                "<REP>    ".to_string()
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
        out(format!("{files:>9} fichier(s) {bytes:>12} octets\r\n\r\n").as_bytes());
    }
}
