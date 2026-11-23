// SPDX-License-Identifier: GPL-3.0-or-later
//! Fichiers de commandes (.BAT) du shell interne : lignes lues une à une par le shell (`batch_next`),
//! paramètres `%0`–`%9`, `%*` et variables `%NOM%`, `GOTO`/étiquettes, `CALL`, `IF`, `FOR`, `SHIFT`.
//! Les lignes passent ensuite par `shell_command` comme si elles étaient tapées ; un `FOR` empile
//! une trame synthétique de lignes déjà développées.

use crate::backend::Result;
use crate::host::Host;
use crate::mem::GuestMem;

use super::fs::{self, Target};
use super::shell::Action;
use super::{Dos, con_out};

/// Fichier de commandes en cours d'exécution.
pub struct Frame {
    lines: Vec<Vec<u8>>,
    pc: usize,
    /// `args[0]` : le nom tel que tapé ; puis les paramètres (`SHIFT` les décale).
    args: Vec<Vec<u8>>,
    /// Lignes déjà développées (trame d'un `FOR`) : `%` laissé tel quel.
    expanded: bool,
}

fn is_sep(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b',' | b';' | b'=')
}

fn words(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|&c| is_sep(c)).filter(|w| !w.is_empty()).map(<[u8]>::to_vec).collect()
}

fn lower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

/// Premier mot de `s` (jusqu'à un espace) et le reste, sans espaces de tête.
fn first_word(s: &[u8]) -> (&[u8], &[u8]) {
    let s = s.trim_ascii_start();
    let end = s.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(s.len());
    (&s[..end], s[end..].trim_ascii_start())
}

impl Dos {
    /// Un fichier de commandes est en cours.
    #[cfg(test)]
    pub fn batch_active(&self) -> bool {
        !self.batch.is_empty()
    }

    /// Lance le fichier de commandes `path` (chemin DOS) avec les paramètres `tail` (texte après le
    /// nom). Faux si le fichier est illisible.
    pub fn start_batch(&mut self, path: &[u8], tail: &[u8]) -> bool {
        let Some(data) = self.read_file(path) else { return false };
        let end = data.iter().position(|&c| c == 0x1A).unwrap_or(data.len());
        let lines = data[..end].split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
        let mut args = vec![path.to_vec()];
        args.extend(words(tail));
        self.batch.push(Frame { lines, pc: 0, args, expanded: false });
        true
    }

    /// Ligne suivante du fichier de commandes en cours, développée, avec l'indication « à afficher »
    /// (écho actif et ligne ne commençant pas par `@`). `None` quand il n'y en a plus.
    pub fn batch_next(&mut self, mem: &GuestMem) -> Option<(Vec<u8>, bool)> {
        loop {
            let frame = self.batch.last_mut()?;
            let Some(raw) = frame.lines.get(frame.pc).cloned() else {
                self.batch.pop();
                continue;
            };
            frame.pc += 1;
            let line = if frame.expanded { raw } else { let args = frame.args.clone(); self.expand(mem, &args, &raw) };
            let t = line.trim_ascii();
            if t.is_empty() || t.starts_with(b":") {
                continue;
            }
            let quiet = t.starts_with(b"@");
            let t = t.strip_prefix(b"@").unwrap_or(t).trim_ascii();
            if t.is_empty() {
                continue;
            }
            return Some((t.to_vec(), self.echo && !quiet));
        }
    }

    /// Développe `%0`–`%9`, `%*`, `%NOM%` et `%%`.
    fn expand(&self, mem: &GuestMem, args: &[Vec<u8>], line: &[u8]) -> Vec<u8> {
        let vars = self.master_vars(mem);
        let mut out = Vec::new();
        let mut i = 0;
        while i < line.len() {
            if line[i] != b'%' {
                out.push(line[i]);
                i += 1;
                continue;
            }
            match line.get(i + 1) {
                None => i += 1,
                Some(b'%') => {
                    out.push(b'%');
                    i += 2;
                }
                Some(d @ b'0'..=b'9') => {
                    if let Some(a) = args.get(usize::from(d - b'0')) {
                        out.extend_from_slice(a);
                    }
                    i += 2;
                }
                Some(b'*') => {
                    out.extend_from_slice(&args.iter().skip(1).map(|a| a.as_slice()).collect::<Vec<_>>().join(&b' '));
                    i += 2;
                }
                Some(_) => match line[i + 1..].iter().position(|&c| c == b'%') {
                    Some(n) => {
                        let name = &line[i + 1..i + 1 + n];
                        let key = [&name.to_ascii_uppercase()[..], b"="].concat();
                        if let Some(v) = vars.iter().find_map(|v| v.strip_prefix(&key[..])) {
                            out.extend_from_slice(v);
                        }
                        i += n + 2;
                    }
                    None => i += 1,
                },
            }
        }
        out
    }

    /// `GOTO étiquette` (`:EOF` termine le fichier courant).
    pub(super) fn batch_goto(&mut self, mem: &GuestMem, host: &mut Host, arg: &[u8]) {
        let label = lower(first_word(arg).0.strip_prefix(b":").unwrap_or(first_word(arg).0));
        let Some(frame) = self.batch.last_mut() else { return };
        if label == b"eof" {
            frame.pc = frame.lines.len();
            return;
        }
        let found = frame.lines.iter().position(|l| {
            let t = l.trim_ascii();
            t.strip_prefix(b":").is_some_and(|rest| lower(first_word(rest).0) == label)
        });
        match found {
            Some(i) => frame.pc = i + 1,
            None => {
                con_out(mem, host, b"Label not found\r\n");
                frame.pc = frame.lines.len();
            }
        }
    }

    /// `CALL cible paramètres` : un fichier de commandes ou une étiquette s'empile ; autre chose est
    /// exécuté comme une commande.
    pub(super) fn batch_call(&mut self, mem: &GuestMem, host: &mut Host, arg: &[u8]) -> Result<Action> {
        let (name, rest) = first_word(arg);
        if let Some(label) = name.strip_prefix(b":") {
            if let Some(frame) = self.batch.last() {
                let mut args = vec![name.to_vec()];
                args.extend(words(rest));
                let lines = frame.lines.clone();
                self.batch.push(Frame { lines, pc: 0, args, expanded: false });
                self.batch_goto(mem, host, &[b":", label].concat());
            }
            return Ok(Action::Continue);
        }
        if let Some(path) = self.find_program(name, mem)
            && path.rsplit(|&c| c == b'.').next().is_some_and(|e| e.eq_ignore_ascii_case(b"BAT"))
        {
            self.start_batch(&path, rest);
            return Ok(Action::Continue);
        }
        self.shell_command(mem, host, arg)
    }

    pub(super) fn batch_shift(&mut self) {
        if let Some(f) = self.batch.last_mut()
            && f.args.len() > 1
        {
            f.args.remove(1);
        }
    }

    /// `IF [NOT] ERRORLEVEL n | EXIST fichier | a==b commande`.
    pub(super) fn batch_if(&mut self, mem: &GuestMem, host: &mut Host, arg: &[u8]) -> Result<Action> {
        let mut a = arg.trim_ascii();
        let mut not = false;
        let (w, rest) = first_word(a);
        if w.eq_ignore_ascii_case(b"not") {
            not = true;
            a = rest;
        }
        let (w, rest) = first_word(a);
        let (cond, then) = if w.eq_ignore_ascii_case(b"errorlevel") {
            let (n, then) = first_word(rest);
            let n: u32 = String::from_utf8_lossy(n).trim().parse().unwrap_or(0);
            (u32::from(self.errorlevel) >= n, then)
        } else if w.eq_ignore_ascii_case(b"exist") {
            let (name, then) = first_word(rest);
            let found = match self.drives.resolve(name) {
                Ok(Target::Path { exists: true, .. }) => true,
                // Motif (« EXIST *.DAT ») : au moins une entrée.
                _ => self.drives.search(name).is_ok_and(|(dir, pat)| fs::list(&dir).iter().any(|e| fs::fcb_matches(&pat, &e.short))),
            };
            (found, then)
        } else {
            match a.windows(2).position(|p| p == b"==") {
                Some(i) => {
                    let left = &a[..i];
                    let (right, then) = first_word(&a[i + 2..]);
                    (left == right, then)
                }
                None => {
                    con_out(mem, host, b"Syntax error\r\n");
                    return Ok(Action::Continue);
                }
            }
        };
        if cond != not && !then.is_empty() {
            return self.shell_command(mem, host, then);
        }
        Ok(Action::Continue)
    }

    /// `FOR %x IN (liste) DO commande` : la commande, une fois par élément (les motifs sont
    /// développés), exécutée par une trame synthétique.
    pub(super) fn batch_for(&mut self, mem: &GuestMem, host: &mut Host, arg: &[u8]) {
        let bad = |host: &mut Host| con_out(mem, host, b"Syntax error\r\n");
        let (var, rest) = first_word(arg);
        let (kw, rest) = first_word(rest);
        let (Some(var), true, Some(open)) = (var.strip_prefix(b"%"), kw.eq_ignore_ascii_case(b"in"), rest.iter().position(|&c| c == b'(')) else {
            return bad(host);
        };
        let Some(close) = rest.iter().position(|&c| c == b')') else { return bad(host) };
        let items = words(&rest[open + 1..close]);
        let (kw, command) = first_word(&rest[close + 1..]);
        if !kw.eq_ignore_ascii_case(b"do") || var.is_empty() {
            return bad(host);
        }
        let mut names: Vec<Vec<u8>> = Vec::new();
        for it in items {
            if it.contains(&b'*') || it.contains(&b'?') {
                if let Ok((dir, pat)) = self.drives.search(&it) {
                    let prefix: &[u8] = it.iter().rposition(|&c| c == b'\\').map_or(&[], |i| &it[..=i]);
                    names.extend(fs::list(&dir).into_iter().filter(|e| fs::fcb_matches(&pat, &e.short) && !e.meta.is_dir()).map(|e| {
                        let p = fs::fcb_pattern(&e.short);
                        let mut n = prefix.to_vec();
                        n.extend(String::from_utf8_lossy(&p[..8]).trim_end().bytes());
                        let ext = String::from_utf8_lossy(&p[8..]).trim_end().to_string();
                        if !ext.is_empty() {
                            n.push(b'.');
                            n.extend(ext.bytes());
                        }
                        n
                    }));
                }
            } else {
                names.push(it);
            }
        }
        let pat = [b"%", var].concat();
        let lines: Vec<Vec<u8>> = names
            .iter()
            .map(|n| {
                let mut out = Vec::new();
                let mut i = 0;
                while i < command.len() {
                    if command[i..].starts_with(&pat) {
                        out.extend_from_slice(n);
                        i += pat.len();
                    } else {
                        out.push(command[i]);
                        i += 1;
                    }
                }
                out
            })
            .collect();
        let args = self.batch.last().map(|f| f.args.clone()).unwrap_or_default();
        self.batch.push(Frame { lines, pc: 0, args, expanded: true });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dos_with(lines: &[&str], args: &str) -> Dos {
        let mut d = Dos::new();
        let mut a = vec![b"TEST.BAT".to_vec()];
        a.extend(words(args.as_bytes()));
        d.batch.push(Frame { lines: lines.iter().map(|l| l.as_bytes().to_vec()).collect(), pc: 0, args: a, expanded: false });
        d
    }

    fn mem() -> GuestMem {
        GuestMem::new(1 << 20).unwrap()
    }

    #[test]
    fn expansion_and_skipped_lines() {
        let m = mem();
        let mut d = dos_with(&["@echo off", "REM x", ":label", "", "run %1 and %2 %%1 100%% %*"], "a b");
        assert_eq!(d.batch_next(&m), Some((b"echo off".to_vec(), false)));
        assert_eq!(d.batch_next(&m), Some((b"REM x".to_vec(), true)));
        assert_eq!(d.batch_next(&m), Some((b"run a and b %1 100% a b".to_vec(), true)));
        assert_eq!(d.batch_next(&m), None);
        assert!(!d.batch_active());
    }

    #[test]
    fn shift_moves_parameters() {
        let m = mem();
        let mut d = dos_with(&["x %1", "x %1"], "a b");
        assert_eq!(d.batch_next(&m).unwrap().0, b"x a");
        d.batch_shift();
        assert_eq!(d.batch_next(&m).unwrap().0, b"x b");
    }
}
