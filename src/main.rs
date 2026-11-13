// SPDX-License-Identifier: GPL-3.0-or-later
//! Émulateur DOS : le code DOS s'exécute nativement sous KVM ; BIOS, DOS et
//! matériel PC sont fournis par l'hôte.

mod backend;
mod bench;
mod bios;
mod debug;
mod dos;
mod font;
mod hle;
mod host;
mod interp;
mod io;
mod kbc;
mod keyboard;
mod kvm;
mod machine;
mod mem;
mod mouse;
mod pic;
mod pit;
mod render;
mod rtc;
mod sdl;
mod term;
mod throttle;
mod timer;
mod vga;
mod xms;
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
  --term         affiche l'écran texte en direct dans le terminal
  --shot F.png   capture de l'écran à la fin du programme
  --window       fenêtre SDL3 (vCPU sur son propre thread)
  --stats        compteurs de la machine à la fin (sorties, VRAM émulée…)
  --limit S      arrêt au bout de S secondes, avec registres et code à CS:IP
  --font F.psf   police PSF du mode texte (par défaut : Terminus 8×16 embarquée)
  --keys SCRIPT  clavier scripté au lieu du terminal, puis fin de session :
                 texte, {enter} {esc} {tab} {f1} {up} {alt+f} {ctrl+c}…
  --bench | --throttle | --remap   bancs de mesure de l'étape 0";

fn main() -> ExitCode {
    let mut cfg = Config::default();
    let mut debug = false;
    let mut screen = false;
    let mut term = false;
    let mut shot: Option<String> = None;
    let mut window = false;
    let mut stats = false;
    let mut font: Option<std::path::PathBuf> = None;
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
            "--limit" => match value("--limit") {
                Some(v) if v > 0.0 => cfg.limit = Some(std::time::Duration::from_secs_f64(v)),
                _ => return ExitCode::from(2),
            },
            "--trace-io" => cfg.trace_io = true,
            "--debug" => debug = true,
            "--screen" => screen = true,
            "--term" => term = true,
            "--shot" => shot = args.next(),
            "--window" => window = true,
            "--stats" => stats = true,
            "--font" => font = args.next().map(Into::into),
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
    let opts = RunOptions { cfg, program, mount, tail, debug, screen, shot, font, stats };
    let result = if window {
        let host_keys = keys;
        sdl::run("DOS sous KVM", move |mut host| {
            if let Some(ev) = host_keys {
                host.input = Box::new(host::ScriptedInput(ev.into()));
            }
            launch(&opts, host)
        })
    } else {
        let mut host = match keys {
            Some(ev) => Host { input: Box::new(host::ScriptedInput(ev.into())), ..Host::headless(Box::new(host::Stdout)) },
            None => Host::terminal(),
        };
        if term {
            host.text = Box::new(host::Discard);
            host.term = Some(term::TermScreen::new());
        }
        launch(&opts, host)
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(1)
        }
    }
}

/// Ce qu'il faut pour construire et lancer la machine (sur le thread du vCPU).
struct RunOptions {
    cfg: Config,
    program: Option<String>,
    mount: Option<std::path::PathBuf>,
    tail: Vec<u8>,
    debug: bool,
    screen: bool,
    shot: Option<String>,
    font: Option<std::path::PathBuf>,
    stats: bool,
}

fn launch(o: &RunOptions, host: Host) -> Result<u8, String> {
    let mut pc = Pc::new(&o.cfg, host)?;
    pc.set_font(match &o.font {
        Some(p) => font::Font::load(p)?,
        None => font::Font::builtin(),
    });
    match &o.program {
        Some(path) => {
            let path = std::path::Path::new(path);
            let root = match &o.mount {
                Some(m) => m.clone(),
                None => path.parent().map(|p| if p.as_os_str().is_empty() { ".".into() } else { p.to_path_buf() }).unwrap_or(".".into()),
            };
            pc.mount(2, root);
            let dos = pc.dos_path_of(path).ok_or_else(|| format!("{} : introuvable sous le lecteur C:", path.display()))?;
            pc.load_program(&dos, &o.tail)?;
        }
        None => {
            pc.mount(2, o.mount.clone().unwrap_or_else(|| ".".into()));
            return Ok(pc.shell()?);
        }
    }
    let code = if o.debug { debug::repl(&mut pc).map(|c| c.unwrap_or(0)) } else { pc.run() };
    if o.screen {
        println!("\n{}", pc.screen_text().join("\n").trim_end());
    }
    if o.stats {
        eprintln!("{:#?}", pc.stats);
    }
    if let Some(path) = &o.shot {
        match pc.frame() {
            Some((w, h, px)) => {
                if let Err(e) = std::fs::write(path, render::png(w, h, &px)) {
                    eprintln!("{path} : {e}");
                }
            }
            None => eprintln!("--shot : mode texte sans police (--font)"),
        }
    }
    Ok(code?)
}
