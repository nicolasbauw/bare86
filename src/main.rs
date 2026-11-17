// SPDX-License-Identifier: GPL-3.0-or-later
//! Émulateur DOS : le code DOS s'exécute nativement sous KVM ; BIOS, DOS et
//! matériel PC sont fournis par l'hôte.

mod audio;
mod backend;
mod bench;
mod binfmt;
mod bios;
mod cdrom;
mod config;
mod debug;
mod disk;
mod dma;
mod dos;
mod ems;
mod font;
mod gameport;
mod glide;
mod hle;
mod host;
mod ide;
mod iso9660;
mod interp;
mod io;
mod kbc;
mod keyboard;
mod kvm;
mod launcher;
mod machine;
mod mem;
mod menu;
mod mouse;
mod midisynth;
mod mpu401;
mod opengl;
mod ne2000;
mod net;
mod opl;
mod pic;
mod pit;
mod render;
mod rtc;
mod runtime;
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
                 --fda for a set of floppies, swapped with Ctrl+Alt+D
  --hda IMG      hard disk image for drive C:
  --cdrom IMG    CD-ROM image (.iso, or .cue with BIN files and audio
                 tracks, mixed with the sound card): drive D: with MSCDEX under the
                 built-in DOS; with --boot, an ATAPI drive (secondary IDE
                 master, 170h, IRQ 15) for the DOS's own CD drivers
  --boot         boot from the disk images (A: if present, else C:)
                 instead of the built-in DOS
  --net          NE2000 network card (port 300h, IRQ 3) with user-mode NAT:
                 guest 10.0.2.15 by DHCP, gateway 10.0.2.2, DNS 10.0.2.3
  --net-fwd tcp:HOST:GUEST   forward a host port (127.0.0.1) to the
                 guest (also udp:…; implies --net; repeatable)
  --sb 16|pro2|pro1|ess   sound card: Sound Blaster 16 (default), Pro 2
                 (also pro: DSP 3.02, OPL3), Pro 1 (two OPL2 chips), or ESS
                 AudioDrive ES1688 (Sound Blaster Pro compatible, ESFM FM)
  --opl opl2|dualopl2|opl3|cqm|esfm   FM chip, instead of the card's own:
                 OPL2 (YM3812, AdLib and early Sound Blasters), two OPL2, OPL3,
                 Creative's CQM (Sound Blaster 16 Vibra, AWE64) or ESS's ESFM
                 (AudioDrive cards), both OPL3-compatible
  --midi opl3|off   General MIDI music (MPU-401, 330h) played on OPL3 FM
                 chips (default) or not played
  --midi-bank N  FM instrument bank (default 58: Windows 9x FM synth);
                 --midi-bank list prints the banks
  --midi-out F.mid   record the MIDI music sent to the MPU-401
  --set NAME=VALUE   environment variable for the built-in DOS (repeatable;
                 in a profile, one \"set = NAME=VALUE\" line each), e.g.
                 DPMILDR=16 for some Win32 programs under HX
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
  --window-auto  window shown at the first graphics mode (VGA, VESA, Glide,
                 OpenGL); until then, and for text-mode programs, the
                 terminal
  --fullscreen   window, full screen from the start
  --stats        machine counters at the end (exits, emulated VRAM…)
  --vbe1         VESA 1.2 card: banked windows only, no linear framebuffer
  --no-interp    no interpreter bursts: one VM exit per emulated VRAM
                 access (to measure what the interpreter brings)
  --limit S      stop after S seconds, with registers and code at CS:IP
  window keys:   Ctrl+Alt+M menu (floppy and CD change, speed, full
                 screen, screenshot, shortcuts), Alt+Enter or Ctrl+Alt+F
                 full screen, Ctrl+Alt+S screenshot, Ctrl+Alt+G (or middle
                 button) releases the mouse, Ctrl+Alt+D next floppy in A:,
                 Ctrl+Alt+- slower, Ctrl+Alt++ faster
  --menu-key L   letter of the menu shortcut (Ctrl+Alt+L, default M)
  --launcher [DIR]  list the games of DIR (default: current directory),
                 each a subdirectory with a bare86.conf, run the chosen one,
                 create or edit their profiles
  --binfmt       print the binfmt_misc rules that let Linux run DOS and
                 Win32 programs directly (./GAME.EXE), and how to install
                 them; such a program gets its bare86.conf (next to it, or
                 a parent's profile that mounts it) and a window
  --font F.psf   PSF font for text mode (default: built-in Terminus 8×16)
  --keys SCRIPT  scripted keyboard instead of the terminal, then end of session:
                 text, {enter} {esc} {tab} {f1} {up} {alt+f} {ctrl+c}…
                 {wait:MS} pause, {speed:MIPS} or {speed:max} speed change,
                 {disk} next floppy in A:, {fda:IMG} {cd:IMG} {cd:} change
                 floppy or CD (empty: eject), {until:TEXT} wait for TEXT on
                 the text screen, {mouse:DX,DY,BUTTONS} mouse motion
  --bench | --throttle | --remap   step 0 measurement benches";

fn main() -> ExitCode {
    let mut cfg = Config::default();
    let mut debug = false;
    let mut screen = false;
    let mut term = false;
    let mut shot: Option<String> = None;
    let mut window = false;
    // `--window` explicite : fenêtre montrée d'emblée, même avec `--window-auto`.
    let mut window_now = false;
    let mut wav: Option<String> = None;
    let mut stats = false;
    let mut font: Option<std::path::PathBuf> = None;
    let mut keys = None;
    let mut program = None;
    let mut mount: Option<std::path::PathBuf> = None;
    let mut tail = Vec::new();
    let mut fullscreen = false;
    let mut boot = false;
    let mut menu_letter = 'M';
    // Puce FM : celle de la carte (`--sb`), sauf choix explicite (`--opl`).
    let mut card_opl = opl::OplModel::Opl3;
    let mut opl_choice: Option<opl::OplModel> = None;
    let mut cli: Vec<String> = std::env::args().skip(1).collect();
    // Lancé par binfmt_misc (`./JEU.EXE`) : profil du programme, fenêtre.
    if let Some(program) = binfmt::invoked(&cli) {
        let head = std::fs::read(&program).map(|b| b[..b.len().min(4096)].to_vec()).unwrap_or_default();
        if binfmt::is_dotnet(&head) {
            let Some(mono) = binfmt::dotnet_target() else {
                eprintln!("{}: .NET program, and Mono is not installed", program.display());
                return ExitCode::from(126);
            };
            use std::os::unix::process::CommandExt;
            let e = std::process::Command::new(mono).args(&cli).exec();
            eprintln!("mono: {e}");
            return ExitCode::from(126);
        }
        let mut options = Vec::new();
        if let Some(conf) = binfmt::profile_for(&program) {
            if cli.len() == 1 {
                cli.extend(binfmt::profile_args(&conf, &program));
            }
            options.extend(["--config".to_string(), conf.to_string_lossy().into_owned()]);
        }
        if binfmt::wants_window() && !cli.iter().any(|a| a == "--term") {
            options.push("--window-auto".into());
        }
        cli.splice(0..0, options);
    }
    // Lanceur, seulement sur demande : le profil choisi devient
    // `--config PROFIL --window`, le reste de la ligne de commande est gardé.
    if let Some(root) = launcher::root_of(&cli) {
        let mut ui = launcher::Ui::new(root);
        match sdl::launcher(&mut ui) {
            Ok(Some(conf)) => cli = launcher::command_line(&cli, &conf),
            Ok(None) => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("launcher: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let conf = match config::load(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration: {e}");
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
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            };
        }
        let mut value = |name: &str| -> Option<f64> {
            let v = args.next().and_then(|v| v.parse().ok());
            if v.is_none() {
                eprintln!("{name}: number expected");
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
                    eprintln!("{a}: image expected");
                    return ExitCode::from(2);
                }
            },
            "--boot" => boot = true,
            "--sb" => match args.next().as_deref() {
                // SB Pro 2 (OPL3), SB Pro 1 (deux OPL2), SB16 (OPL3).
                Some("pro" | "pro2") => (cfg.sb_pro, cfg.sb_ess, card_opl) = (true, false, opl::OplModel::Opl3),
                Some("pro1") => (cfg.sb_pro, cfg.sb_ess, card_opl) = (true, false, opl::OplModel::DualOpl2),
                Some("16") => (cfg.sb_pro, cfg.sb_ess, card_opl) = (false, false, opl::OplModel::Opl3),
                Some("ess") => (cfg.sb_pro, cfg.sb_ess, card_opl) = (false, true, opl::OplModel::Esfm),
                _ => {
                    eprintln!("--sb: 16, pro2 (or pro), pro1 or ess expected");
                    return ExitCode::from(2);
                }
            },
            "--opl" => match args.next().map(|m| m.parse::<opl::OplModel>()) {
                Some(Ok(m)) => opl_choice = Some(m),
                Some(Err(e)) => {
                    eprintln!("--opl: {e}");
                    return ExitCode::from(2);
                }
                None => {
                    eprintln!("--opl: opl2, dualopl2, opl3, cqm or esfm expected");
                    return ExitCode::from(2);
                }
            },
            "--midi" => match args.next().as_deref() {
                Some("opl3") => cfg.midi_bank = cfg.midi_bank.or(Some(midisynth::DEFAULT_BANK)),
                Some("off") => cfg.midi_bank = None,
                _ => {
                    eprintln!("--midi: opl3 or off expected");
                    return ExitCode::from(2);
                }
            },
            "--midi-bank" => match args.next().as_deref() {
                Some("list") => {
                    for (i, n) in midisynth::bank_names().iter().enumerate() {
                        println!("{i:3}  {n}");
                    }
                    return ExitCode::SUCCESS;
                }
                Some(n) => match n.parse() {
                    Ok(b) => cfg.midi_bank = Some(b),
                    Err(_) => {
                        eprintln!("--midi-bank: number or \"list\" expected");
                        return ExitCode::from(2);
                    }
                },
                None => {
                    eprintln!("--midi-bank: number or \"list\" expected");
                    return ExitCode::from(2);
                }
            },
            "--midi-out" => match args.next() {
                Some(p) => cfg.midi_out = Some(p.into()),
                None => {
                    eprintln!("--midi-out: file expected");
                    return ExitCode::from(2);
                }
            },
            "--menu-key" => match args.next().and_then(|k| k.chars().next()).filter(char::is_ascii_alphabetic) {
                Some(c) => menu_letter = c.to_ascii_uppercase(),
                None => {
                    eprintln!("--menu-key: letter expected");
                    return ExitCode::from(2);
                }
            },
            "--cdrom" => match args.next() {
                Some(p) => cfg.cdrom = Some(p.into()),
                None => {
                    eprintln!("--cdrom: image expected");
                    return ExitCode::from(2);
                }
            },
            "--net" => {
                cfg.net.get_or_insert_with(Vec::new);
            }
            "--net-fwd" => match args.next().map(|f| f.parse::<net::Forward>()) {
                Some(Ok(f)) => cfg.net.get_or_insert_with(Vec::new).push(f),
                Some(Err(e)) => {
                    eprintln!("--net-fwd: {e}");
                    return ExitCode::from(2);
                }
                None => {
                    eprintln!("--net-fwd: tcp:HOST:GUEST expected");
                    return ExitCode::from(2);
                }
            },
            "--set" => match args.next().as_deref().and_then(|v| v.split_once('=')) {
                Some((name, value)) if !name.trim().is_empty() => cfg.env.push((name.trim().to_ascii_uppercase(), value.to_string())),
                _ => {
                    eprintln!("--set: NAME=VALUE expected");
                    return ExitCode::from(2);
                }
            },
            "--no-interp" => cfg.interp = false,
            "--vbe1" => cfg.vbe2 = false,
            "--debug" => debug = true,
            "--screen" => screen = true,
            "--term" => term = true,
            "--shot" => shot = args.next(),
            "--wav" => wav = args.next(),
            "--window" => {
                window = true;
                window_now = true;
            }
            "--window-auto" => window = true,
            "--fullscreen" => {
                window = true;
                window_now = true;
                fullscreen = true;
            }
            "--stats" => stats = true,
            "--font" => font = args.next().map(Into::into),
            "--keys" => match args.next().map(|k| keyboard::parse_script(&k)) {
                Some(Ok(ev)) => keys = Some(ev),
                Some(Err(e)) => {
                    eprintln!("--keys: {e}");
                    return ExitCode::from(2);
                }
                None => {
                    eprintln!("--keys: script expected");
                    return ExitCode::from(2);
                }
            },
            "--mount" => match args.next() {
                Some(d) => mount = Some(d.into()),
                None => {
                    eprintln!("--mount: directory expected");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--binfmt" => {
                let exe = match std::env::current_exe() {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("--binfmt: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                print!("{}", binfmt::rules(&exe));
                eprintln!("{}", binfmt::HELP);
                return ExitCode::SUCCESS;
            }
            _ if a.starts_with("--") => {
                eprintln!("unknown option: {a}\n{USAGE}");
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
    cfg.opl = opl_choice.unwrap_or(card_opl);
    let opts = RunOptions { cfg, program, mount, tail, debug, screen, shot, font, stats, boot };
    let result = if window {
        let host_keys = keys;
        let menu = menu::State {
            floppy: opts.cfg.disks[0].first().cloned(),
            floppies: opts.cfg.disks[0].clone(),
            cd_drive: opts.cfg.cdrom.is_some(),
            cd: opts.cfg.cdrom.clone(),
            speed: opts.cfg.mips,
            fullscreen,
            menu_letter,
            message: None,
        };
        sdl::run("Bare86", menu, !window_now, move |mut host| {
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
            eprintln!("error: {e}");
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
        // DOS intégré : l'image du CD est aussi montée en D: (MSCDEX).
        _ if o.cfg.cdrom.is_some()
            && let Err(e) = pc.mount_cd(3) =>
        {
            return Err(e.to_string());
        }
        Some(path) => {
            let path = std::path::Path::new(path);
            let root = match &o.mount {
                Some(m) => m.clone(),
                None => path.parent().map(|p| if p.as_os_str().is_empty() { ".".into() } else { p.to_path_buf() }).unwrap_or(".".into()),
            };
            pc.mount(2, root);
            let dos = pc.dos_path_of(path).ok_or_else(|| format!("{}: not found under drive C:", path.display()))?;
            // Le programme démarre dans son répertoire (avec --mount, il peut
            // être plus bas que la racine) : les jeux y cherchent leurs données.
            if let Some(cut) = dos.iter().rposition(|&c| c == b'\\') {
                let _ = pc.chdir(&dos[..cut.max(2)]);
            }
            pc.load_program(&dos, &o.tail)?;
        }
        None => {
            pc.mount(2, o.mount.clone().unwrap_or_else(|| ".".into()));
        }
    }
    // Sans programme : le shell interne ; `--shot` et `--stats` valent aussi
    // pour une session de shell (fin de l'entrée, EXIT ou limite de temps).
    let code = if o.program.is_none() && !o.boot {
        pc.shell()
    } else if o.debug {
        debug::repl(&mut pc).map(|c| c.unwrap_or(0))
    } else {
        pc.run()
    };
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
                    eprintln!("{path}: {e}");
                }
            }
            None => eprintln!("--shot: text mode without a font (--font)"),
        }
    }
    Ok(code?)
}
