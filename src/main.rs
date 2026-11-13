// SPDX-License-Identifier: GPL-3.0-or-later
//! Émulateur DOS : le code DOS s'exécute nativement sous KVM ; BIOS, DOS et
//! matériel PC sont fournis par l'hôte.

mod audio;
mod backend;
mod bench;
mod bios;
mod config;
mod debug;
mod disk;
mod dma;
mod dos;
mod ems;
mod font;
mod gameport;
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
mod opl;
mod pic;
mod pit;
mod render;
mod rtc;
mod sb16;
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
usage: bare86 [options] [program [arguments…]]
  without a program: built-in shell on drive C:
  --config FILE  configuration file (default: ./bare86.conf, then
                 ~/.config/bare86/bare86.conf); one \"option = value\" per
                 line, as on the command line, which takes precedence;
                 \"program = …\" and \"args = …\" give the program to run
  --mount DIR    host directory mounted as C: (default: the program's
                 directory, or the current directory for the shell)
  --fda IMG      floppy image for drive A: (also --fdb for B:); repeat
                 --fda for a set of floppies, swapped with Ctrl+F4
  --hda IMG      hard disk image for drive C:
  --boot         boot from the disk images (A: if present, else C:)
                 instead of the built-in DOS
  --mips N       throttle to N million instructions per second
  --mem MB       total memory in MB (default 16)
  --ems MB       EMS memory in MB, in addition (default 4, 0 to 32)
  --trace-int    log of interrupts serviced by the host
  --trace-io     report I/O ports with no device
  --debug        debugger (registers, memory, single step)
  --screen       print the text screen when the program ends
  --term         show the text screen live in the terminal
  --shot F.png   screenshot when the program ends
  --wav F.wav    record the sound to a file (without a window)
  --window       SDL3 window (vCPU on its own thread)
  --fullscreen   window, full screen from the start
  --stats        machine counters at the end (exits, emulated VRAM…)
  --vbe1         VESA 1.2 card: banked windows only, no linear framebuffer
  --no-interp    no interpreter bursts: one VM exit per emulated VRAM
                 access (to measure what the interpreter brings)
  --limit S      stop after S seconds, with registers and code at CS:IP
  window keys:   Alt+Enter full screen, Ctrl+F5 screenshot,
                 Ctrl+F11 slower, Ctrl+Shift+F11 (or Ctrl+F12) faster,
                 Ctrl+F10 or middle button releases the mouse,
                 Ctrl+F4 next floppy in A:
  --font F.psf   PSF font for text mode (default: built-in Terminus 8×16)
  --keys SCRIPT  scripted keyboard instead of the terminal, then end of session:
                 text, {enter} {esc} {tab} {f1} {up} {alt+f} {ctrl+c}…
                 {wait:MS} pause, {speed:MIPS} or {speed:max} speed change,
                 {disk} next floppy in A:, {until:TEXT} wait for TEXT on
                 the text screen
  --bench | --throttle | --remap   step 0 measurement benches";

fn main() -> ExitCode {
    let mut cfg = Config::default();
    let mut debug = false;
    let mut screen = false;
    let mut term = false;
    let mut shot: Option<String> = None;
    let mut window = false;
    let mut wav: Option<String> = None;
    let mut stats = false;
    let mut font: Option<std::path::PathBuf> = None;
    let mut keys = None;
    let mut program = None;
    let mut mount: Option<std::path::PathBuf> = None;
    let mut tail = Vec::new();
    let mut fullscreen = false;
    let mut boot = false;
    let cli: Vec<String> = std::env::args().skip(1).collect();
    let conf = match config::load(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration : {e}");
            return ExitCode::from(2);
        }
    };
    // Options du fichier d'abord : la ligne de commande l'emporte.
    let skip_config = cli.iter().position(|a| a == "--config").map(|i| [i, i + 1]);
    let cli_args = cli.iter().enumerate().filter(|(i, _)| !skip_config.is_some_and(|s| s.contains(i))).map(|(_, a)| a.clone());
    let mut args = conf.options.clone().into_iter().chain(cli_args);
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
            "--ems" => match value("--ems") {
                Some(v) if (0.0..=32.0).contains(&v) => cfg.ems_kb = (v * 1024.0) as usize,
                _ => return ExitCode::from(2),
            },
            "--trace-int" => cfg.trace_int = true,
            "--limit" => match value("--limit") {
                Some(v) if v > 0.0 => cfg.limit = Some(std::time::Duration::from_secs_f64(v)),
                _ => return ExitCode::from(2),
            },
            "--trace-io" => cfg.trace_io = true,
            "--fda" | "--fdb" | "--hda" => match args.next() {
                Some(p) => {
                    let list = &mut cfg.disks[match a.as_str() { "--fda" => 0, "--fdb" => 1, _ => 2 }];
                    // Seule A: accepte plusieurs images ; ailleurs, la dernière l'emporte.
                    if a != "--fda" {
                        list.clear();
                    }
                    list.push(p.into());
                }
                None => {
                    eprintln!("{a} : image attendue");
                    return ExitCode::from(2);
                }
            },
            "--boot" => boot = true,
            "--no-interp" => cfg.interp = false,
            "--vbe1" => cfg.vbe2 = false,
            "--debug" => debug = true,
            "--screen" => screen = true,
            "--term" => term = true,
            "--shot" => shot = args.next(),
            "--wav" => wav = args.next(),
            "--window" => window = true,
            "--fullscreen" => {
                window = true;
                fullscreen = true;
            }
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
    if program.is_none()
        && let Some(p) = conf.program
    {
        program = Some(p);
        if let Some(a) = conf.args {
            tail.push(b' ');
            tail.extend_from_slice(a.as_bytes());
        }
    }
    let opts = RunOptions { cfg, program, mount, tail, debug, screen, shot, font, stats, boot };
    let result = if window {
        let host_keys = keys;
        let mips = opts.cfg.mips;
        sdl::run("Bare86", mips, fullscreen, move |mut host| {
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
        if let Some(path) = &wav {
            host.audio = Box::new(audio::WavSink::new(path));
        }
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
    boot: bool,
}

fn launch(o: &RunOptions, host: Host) -> Result<u8, String> {
    let mut pc = Pc::new(&o.cfg, host)?;
    pc.set_font(match &o.font {
        Some(p) => font::Font::load(p)?,
        None => font::Font::builtin(),
    });
    match &o.program {
        _ if o.boot => {
            let drive = if !o.cfg.disks[0].is_empty() { 0 } else if !o.cfg.disks[2].is_empty() { 0x80 } else { 1 };
            pc.boot_disk(drive)?;
        }
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
