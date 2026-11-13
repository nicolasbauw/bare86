// SPDX-License-Identifier: GPL-3.0-or-later
//! Émulateur DOS : le code DOS s'exécute nativement sous KVM ; BIOS, DOS et
//! matériel PC sont fournis par l'hôte.

mod backend;
mod bench;
mod bios;
mod debug;
mod dos;
mod hle;
mod host;
mod interp;
mod io;
mod keyboard;
mod kvm;
mod machine;
mod mem;
mod pic;
mod pit;
mod throttle;
mod timer;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use host::Host;
use machine::{Config, Pc};

const USAGE: &str = "\
usage : dosbox [options] [programme [arguments…]]
  sans programme : shell interne sur le lecteur C:
  --mount DIR    répertoire hôte monté en C: (par défaut : celui du
                 programme, ou le répertoire courant pour le shell)
  --mips N       bridage à N millions d'instructions par seconde
  --mem MO       mémoire totale en Mo (16 par défaut)
  --trace-int    journal des interruptions servies par l'hôte
  --trace-io     signale les ports d'E/S sans périphérique
  --debug        débogueur (registres, mémoire, pas à pas)
  --screen       affiche l'écran texte à la fin du programme
  --keys SCRIPT  clavier scripté au lieu du terminal, puis fin de session :
                 texte, {enter} {esc} {tab} {f1} {up} {alt+f} {ctrl+c}…
  --bench | --throttle | --remap   bancs de mesure de l'étape 0";

fn main() -> ExitCode {
    let mut cfg = Config::default();
    let mut debug = false;
    let mut screen = false;
    let mut keys = None;
    let mut program = None;
    let mut mount: Option<std::path::PathBuf> = None;
    let mut tail = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let bench = match a.as_str() {
            "--bench" => Some(bench::run as fn() -> _),
            "--throttle" => Some(bench::run_throttle as fn() -> _),
            "--remap" => Some(bench::run_remap as fn() -> _),
            _ => None,
        };
        if let Some(bench) = bench {
            return match bench() {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("erreur : {e}");
                    ExitCode::from(1)
                }
            };
        }
        let mut value = |name: &str| -> Option<f64> {
            let v = args.next().and_then(|v| v.parse().ok());
            if v.is_none() {
                eprintln!("{name} : nombre attendu");
            }
            v
        };
        match a.as_str() {
            "--mips" => match value("--mips") {
                Some(v) => cfg.mips = Some(v),
                None => return ExitCode::from(2),
            },
            "--mem" => match value("--mem") {
                Some(v) if v >= 2.0 => cfg.extended_kb = (v as usize - 1) * 1024,
                _ => return ExitCode::from(2),
            },
            "--trace-int" => cfg.trace_int = true,
            "--trace-io" => cfg.trace_io = true,
            "--debug" => debug = true,
            "--screen" => screen = true,
            "--keys" => match args.next().map(|k| keyboard::parse_script(&k)) {
                Some(Ok(ev)) => keys = Some(ev),
                Some(Err(e)) => {
                    eprintln!("--keys : {e}");
                    return ExitCode::from(2);
                }
                None => {
                    eprintln!("--keys : script attendu");
                    return ExitCode::from(2);
                }
            },
            "--mount" => match args.next() {
                Some(d) => mount = Some(d.into()),
                None => {
                    eprintln!("--mount : répertoire attendu");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if a.starts_with("--") => {
                eprintln!("option inconnue : {a}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => {
                // Le reste de la ligne appartient au programme DOS.
                program = Some(a);
                for arg in args.by_ref() {
                    tail.push(b' ');
                    tail.extend_from_slice(arg.as_bytes());
                }
            }
        }
    }
    let host = match keys {
        Some(ev) => Host { input: Box::new(host::ScriptedInput(ev.into())), ..Host::headless(Box::new(host::Stdout)) },
        None => Host::terminal(),
    };
    let result = Pc::new(&cfg, host).and_then(|mut pc| {
        match &program {
            Some(path) => {
                let path = std::path::Path::new(path);
                let root = match &mount {
                    Some(m) => m.clone(),
                    None => path.parent().map(|p| if p.as_os_str().is_empty() { ".".into() } else { p.to_path_buf() }).unwrap_or(".".into()),
                };
                pc.mount(2, root);
                let dos = pc
                    .dos_path_of(path)
                    .ok_or_else(|| format!("{} : introuvable sous le lecteur C:", path.display()))?;
                pc.load_program(&dos, &tail)?;
            }
            None => {
                pc.mount(2, mount.clone().unwrap_or_else(|| ".".into()));
                return pc.shell();
            }
        }
        let code = if debug { debug::repl(&mut pc).map(|c| c.unwrap_or(0)) } else { pc.run() };
        if screen {
            println!("\n{}", pc.screen_text().join("\n").trim_end());
        }
        code
    });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(1)
        }
    }
}
