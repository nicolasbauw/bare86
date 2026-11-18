// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtre SDL3. Le vCPU tourne sur son propre thread (la machine, ses
//! signaux de minuterie et son état y sont créés) ; le thread principal
//! garde la fenêtre et les événements, comme l'exige SDL. Les entrées
//! passent par un canal ; les images par un tampon partagé, que la fenêtre
//! affiche à l'échelle en 4:3.
//!
//! Clavier : positions physiques (scancodes SDL, usages HID) converties en
//! scancodes PC du jeu 1, préfixe E0h pour les touches étendues. Le DOS
//! voit donc un clavier US (pas de KEYB), quelle que soit la disposition.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sdl3::event::Event;
use sdl3::keyboard::Scancode;
use sdl3::pixels::PixelFormat;
use sdl3::render::FRect;

use crate::host::{Display, Frame, Host, InputEvent, InputSource};

/// Dernière image produite par le vCPU, avec un numéro de version.
#[derive(Default)]
struct SharedFrame {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
    seq: u64,
}

/// Images du vCPU ; `graphics` est levé à la première image graphique
/// (fenêtre cachée de `--window-auto` montrée).
struct SharedDisplay(Arc<Mutex<SharedFrame>>, Arc<AtomicBool>);

impl Display for SharedDisplay {
    fn present(&mut self, frame: &Frame<'_>) {
        if frame.graphics {
            self.1.store(true, Ordering::Relaxed);
        }
        if let Ok(mut f) = self.0.lock() {
            f.width = frame.width;
            f.height = frame.height;
            f.pixels.clear();
            f.pixels.extend_from_slice(frame.pixels);
            f.seq += 1;
        }
    }
}

struct ChannelInput(Receiver<InputEvent>);

/// `--window-auto` : clavier du terminal tant que la fenêtre est cachée,
/// puis celui de la fenêtre (le terminal est alors rendu à son état normal).
struct AutoInput {
    term: Option<crate::host::StdinInput>,
    window: ChannelInput,
    shown: Arc<AtomicBool>,
}

impl AutoInput {
    fn source(&mut self) -> &mut dyn InputSource {
        if self.shown.load(Ordering::Relaxed) {
            self.term = None;
        }
        match &mut self.term {
            Some(t) => t,
            None => &mut self.window,
        }
    }
}

impl InputSource for AutoInput {
    fn poll(&mut self) -> Option<InputEvent> {
        // Fenêtre fermée (ou autre événement de la fenêtre) même cachée.
        if self.term.is_some()
            && let Some(ev) = self.window.poll()
        {
            return Some(ev);
        }
        self.source().poll()
    }
    fn wait(&mut self) -> Option<InputEvent> {
        self.source().wait()
    }
}

/// `--window-auto` : sortie DOS vers le terminal tant que la fenêtre est
/// cachée.
struct AutoText(Arc<AtomicBool>);

impl crate::host::TextSink for AutoText {
    fn write(&mut self, bytes: &[u8]) {
        if !self.0.load(Ordering::Relaxed) {
            crate::host::Stdout.write(bytes);
        }
    }
}

impl InputSource for ChannelInput {
    fn poll(&mut self) -> Option<InputEvent> {
        self.0.try_recv().ok()
    }
    fn wait(&mut self) -> Option<InputEvent> {
        self.0.recv().ok()
    }
}

/// Vitesses de Ctrl+Alt+− / Ctrl+Alt++ (MIPS) ; au-delà, sans bridage.
const SPEEDS: [f64; 8] = [3.0, 6.0, 12.0, 25.0, 50.0, 100.0, 200.0, 400.0];

/// Cran suivant (`faster`) ou précédent de l'échelle des vitesses.
pub(crate) fn step_speed(current: Option<f64>, faster: bool) -> Option<f64> {
    let i = current.map_or(SPEEDS.len(), |m| SPEEDS.iter().position(|&s| s >= m).unwrap_or(SPEEDS.len()));
    if faster {
        SPEEDS.get(i + 1).copied()
    } else {
        Some(SPEEDS[i.saturating_sub(1)])
    }
}

/// Capture de l'image courante : `bare86_NNN.png` dans le répertoire courant.
fn screenshot(frame: &Mutex<SharedFrame>) {
    let Ok(f) = frame.lock() else { return };
    if f.width == 0 {
        return;
    }
    let png = crate::render::png(f.width, f.height, &f.pixels);
    drop(f);
    let Some(path) = (1..10_000).map(|n| format!("bare86_{n:03}.png")).find(|p| !std::path::Path::new(p).exists()) else { return };
    match std::fs::write(&path, png) {
        Ok(()) => eprintln!("screenshot: {path}"),
        Err(e) => eprintln!("{path}: {e}"),
    }
}

/// Coussin visé dans le flux SDL : 40 ms ; reconstitué en silence quand
/// le flux se vide (à-coups du vCPU).
const AUDIO_CUSHION: usize = (crate::audio::RATE / 25 * 2) as usize;

/// Verse la file du mixeur dans le flux SDL.
fn feed_audio(stream: &sdl3::audio::AudioStream, queue: &Mutex<VecDeque<i16>>) {
    let samples: Vec<i16> = match queue.lock() {
        Ok(mut q) => q.drain(..).collect(),
        Err(_) => return,
    };
    if samples.is_empty() {
        return;
    }
    if stream.queued_bytes().unwrap_or(0) == 0 {
        let _ = stream.put_data_i16(&vec![0i16; AUDIO_CUSHION]);
    }
    let _ = stream.put_data_i16(&samples);
}

/// Usage HID (scancode SDL) → scancode PC jeu 1 ; bit 8 = préfixe E0h.
fn set1(hid: i32) -> Option<u16> {
    const LETTERS: [u8; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13,
        0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    let e0 = |c: u8| Some(0x100 | u16::from(c));
    let plain = |c: u8| Some(u16::from(c));
    match hid {
        4..=29 => plain(LETTERS[(hid - 4) as usize]),
        // 1–9, 0.
        30..=39 => plain((hid - 30 + 2) as u8),
        40 => plain(0x1C),
        41 => plain(0x01),
        42 => plain(0x0E),
        43 => plain(0x0F),
        44 => plain(0x39),
        45 => plain(0x0C),
        46 => plain(0x0D),
        47 => plain(0x1A),
        48 => plain(0x1B),
        49 | 50 => plain(0x2B),
        51 => plain(0x27),
        52 => plain(0x28),
        53 => plain(0x29),
        54 => plain(0x33),
        55 => plain(0x34),
        56 => plain(0x35),
        57 => plain(0x3A),
        58..=67 => plain((hid - 58 + 0x3B) as u8),
        68 => plain(0x57),
        69 => plain(0x58),
        70 => e0(0x37),
        71 => plain(0x46),
        73 => e0(0x52),
        74 => e0(0x47),
        75 => e0(0x49),
        76 => e0(0x53),
        77 => e0(0x4F),
        78 => e0(0x51),
        79 => e0(0x4D),
        80 => e0(0x4B),
        81 => e0(0x50),
        82 => e0(0x48),
        83 => plain(0x45),
        84 => e0(0x35),
        85 => plain(0x37),
        86 => plain(0x4A),
        87 => plain(0x4E),
        88 => e0(0x1C),
        89..=97 => plain([0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49][(hid - 89) as usize]),
        98 => plain(0x52),
        99 => plain(0x53),
        100 => plain(0x56),
        224 => plain(0x1D),
        225 => plain(0x2A),
        226 => plain(0x38),
        228 => e0(0x1D),
        229 => plain(0x36),
        230 => e0(0x38),
        _ => None,
    }
}

fn send_key(tx: &Sender<InputEvent>, scancode: Option<Scancode>, release: bool) {
    if crate::keyboard::keylog() {
        eprintln!("[key] window {} {scancode:?}", if release { "up" } else { "down" });
    }
    let Some(code) = scancode.and_then(|s| set1(s as i32)) else { return };
    if code & 0x100 != 0 {
        let _ = tx.send(InputEvent::Key(0xE0));
    }
    let _ = tx.send(InputEvent::Key(code as u8 | if release { 0x80 } else { 0 }));
}

/// Ouvre la fenêtre et exécute `machine` (qui construit et lance la
/// machine avec l'hôte fourni) sur le thread du vCPU. Rend son résultat, ou
/// 0 si la fenêtre est fermée avant la fin du programme. `auto` : fenêtre
/// cachée, terminal pour le texte et le clavier, jusqu'à la première image
/// graphique (un utilitaire en mode texte ne l'ouvre jamais).
pub fn run<F>(title: &str, mut menu_state: crate::menu::State, auto: bool, machine: F) -> Result<u8, String>
where
    F: FnOnce(Host) -> Result<u8, String> + Send + 'static,
{
    let menu_letter = menu_state.menu_letter;
    let menu_hotkey = Scancode::from_name(&menu_letter.to_string()).ok_or(format!("invalid menu key: {menu_letter}"))?;
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    // Haute densité : sur un écran mis à l'échelle (4K à 135 %, par
    // exemple), la fenêtre est dessinée à la résolution réelle et notre mise
    // à l'échelle nette s'applique ; sinon le compositeur agrandissait
    // l'image en la lissant (petits textes du HUD de Doom abîmés).
    let mut builder = video.window(title, 960, 720);
    builder.position_centered().resizable().high_pixel_density();
    if auto {
        builder.hidden();
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas();
    let creator = canvas.texture_creator();
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let graphics_seen = Arc::new(AtomicBool::new(!auto));
    let mut visible = !auto;

    let frame = Arc::new(Mutex::new(SharedFrame::default()));
    // Son : file remplie par le mixeur du vCPU (stéréo, 48 kHz).
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let audio = sdl.audio().map_err(|e| e.to_string())?;
    let spec = sdl3::audio::AudioSpec {
        freq: Some(crate::audio::RATE as i32),
        channels: Some(2),
        format: Some(sdl3::audio::AudioFormat::S16LE),
    };
    let sound = audio.default_playback_device().open_device_stream(Some(&spec)).ok();
    if let Some(s) = &sound {
        let _ = s.resume();
    }
    let vcpu_queue = queue.clone();
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let display = SharedDisplay(frame.clone(), graphics_seen.clone());
    let vcpu_shown = graphics_seen.clone();
    // Joystick : le premier branché, au démarrage ou à son arrivée. Ouvert
    // avant le vCPU : les jeux le détectent dès leur lancement.
    let joysticks = sdl.joystick().ok();
    let mut joystick: Option<sdl3::joystick::Joystick> = None;
    let open_joystick = |id, joystick: &mut Option<sdl3::joystick::Joystick>| {
        if joystick.is_some() {
            return;
        }
        if let Some(j) = joysticks.as_ref().and_then(|s| s.open(id).ok()) {
            let sticks = if j.num_axes() >= 4 { 2 } else { 1 };
            eprintln!("joystick: {} ({} axes)", j.name(), j.num_axes());
            let _ = tx.send(InputEvent::JoyPresent(sticks));
            *joystick = Some(j);
        }
    };
    if let Some(id) = joysticks.as_ref().and_then(|s| s.joysticks().ok()).and_then(|v| v.first().copied()) {
        open_joystick(id, &mut joystick);
    }
    std::thread::Builder::new()
        .name("vcpu".into())
        .spawn(move || {
            let (input, text): (Box<dyn InputSource>, Box<dyn crate::host::TextSink>) = if auto {
                let term = Some(crate::host::StdinInput::new());
                (Box::new(AutoInput { term, window: ChannelInput(rx), shown: vcpu_shown.clone() }), Box::new(AutoText(vcpu_shown)))
            } else {
                (Box::new(ChannelInput(rx)), Box::new(crate::host::Discard))
            };
            let host = Host {
                display: Box::new(display),
                audio: Box::new(crate::audio::QueueSink(vcpu_queue)),
                input,
                text,
                term: None,
                quit: false,
                wants_frames: true,
            };
            let _ = done_tx.send(machine(host));
        })
        .map_err(|e| e.to_string())?;

    let mut texture = None;
    let mut upscaled: Option<((u32, u32), sdl3::render::Texture)> = None;
    let mut size = (0, 0);
    let mut shown = 0u64;
    let mut buttons = 0u8;
    // Souris capturée (mode relatif, curseur masqué) : un clic dans la
    // fenêtre la capture, le bouton central, Ctrl+Alt+G ou la perte du focus
    // la libèrent.
    let mouse = sdl.mouse();
    let mut captured = false;
    let mut swallow_middle = false;
    // Raccourcis (Ctrl+Alt+lettre, Alt+Entrée) : leur relâchement n'est pas
    // transmis à l'invité.
    let mut swallowed: Vec<Scancode> = Vec::new();
    let mut speed = menu_state.speed;
    let mut fullscreen = menu_state.fullscreen;
    // Menu (Ctrl+Alt+M) : ouvert ou non, et touches enfoncées côté invité (leur
    // relâchement lui est transmis même menu ouvert).
    let mut menu: Option<crate::menu::Menu> = None;
    let menu_font = crate::font::Font::builtin();
    let mut held: Vec<Scancode> = Vec::new();
    let mut redraw = false;
    if fullscreen {
        let _ = canvas.window_mut().set_fullscreen(true);
    }
    let set_title = |canvas: &mut sdl3::render::Canvas<sdl3::video::Window>, captured: bool, speed: Option<f64>| {
        let hint = if captured { "middle button or Ctrl+Alt+G releases the mouse" } else { "click to capture the mouse" };
        let mips = speed.map_or("full speed".to_string(), |m| format!("{m} MIPS"));
        let _ = canvas.window_mut().set_title(&format!("{title} — {mips} — {hint} — Ctrl+Alt+{menu_letter} menu"));
    };
    let set_capture = |canvas: &mut sdl3::render::Canvas<sdl3::video::Window>, on: bool, captured: &mut bool, speed: Option<f64>| {
        *captured = on;
        mouse.set_relative_mouse_mode(canvas.window(), on);
        set_title(canvas, on, speed);
    };
    set_capture(&mut canvas, false, &mut captured, speed);
    loop {
        while let Some(ev) = events.poll_event() {
            match ev {
                Event::Quit { .. } => return Ok(0),
                Event::Window { win_event: sdl3::event::WindowEvent::FocusLost, .. } if captured => {
                    set_capture(&mut canvas, false, &mut captured, speed);
                }
                // Raccourcis de Bare86 : Ctrl+Alt+lettre (comme QEMU ; les
                // Ctrl+Fn sont pris par les bureaux, KDE notamment).
                Event::KeyDown { scancode: Some(sc), keymod, repeat, .. } if host_shortcut(sc, keymod, menu_hotkey).is_some() => {
                    swallowed.push(sc);
                    match host_shortcut(sc, keymod, menu_hotkey) {
                        Some(Shortcut::Menu) if !repeat => {
                            menu = match menu {
                                Some(_) => None,
                                None => {
                                    if captured {
                                        set_capture(&mut canvas, false, &mut captured, speed);
                                    }
                                    menu_state.message = None;
                                    Some(crate::menu::Menu::new())
                                }
                            };
                            redraw = true;
                        }
                        Some(Shortcut::ReleaseMouse) => set_capture(&mut canvas, false, &mut captured, speed),
                        Some(Shortcut::Screenshot) if !repeat => screenshot(&frame),
                        Some(Shortcut::Fullscreen) if !repeat => {
                            fullscreen = !fullscreen;
                            let _ = canvas.window_mut().set_fullscreen(fullscreen);
                            redraw = true;
                        }
                        Some(Shortcut::NextDisk) if !repeat => {
                            let _ = tx.send(InputEvent::NextDisk);
                            // Même rotation que la machine (Disks::next_floppy).
                            let list = &menu_state.floppies;
                            if list.len() >= 2 {
                                let i = menu_state.floppy.as_ref().and_then(|f| list.iter().position(|p| p == f)).unwrap_or(0);
                                menu_state.floppy = Some(list[(i + 1) % list.len()].clone());
                            }
                        }
                        Some(s @ (Shortcut::Slower | Shortcut::Faster)) => {
                            speed = step_speed(speed, s == Shortcut::Faster);
                            let _ = tx.send(InputEvent::Speed(speed));
                            set_title(&mut canvas, captured, speed);
                        }
                        _ => {}
                    }
                }
                Event::KeyDown { scancode: Some(sc), .. } if menu.is_some() => {
                    let Some(key) = menu_key(sc) else { continue };
                    let m = menu.as_mut().expect("menu open");
                    menu_state.speed = speed;
                    menu_state.fullscreen = fullscreen;
                    menu_state.message = None;
                    redraw = true;
                    match m.key(key, &menu_state) {
                        crate::menu::Action::None => {}
                        crate::menu::Action::Close => menu = None,
                        crate::menu::Action::Quit => return Ok(0),
                        crate::menu::Action::Speed(s) => {
                            speed = s;
                            let _ = tx.send(InputEvent::Speed(speed));
                            set_title(&mut canvas, captured, speed);
                        }
                        crate::menu::Action::Fullscreen => {
                            fullscreen = !fullscreen;
                            let _ = canvas.window_mut().set_fullscreen(fullscreen);
                        }
                        crate::menu::Action::Screenshot => screenshot(&frame),
                        crate::menu::Action::Floppy(p) => match crate::disk::Disk::open(&p, true) {
                            Ok(_) => {
                                let _ = tx.send(InputEvent::InsertFloppy(p.clone()));
                                menu_state.message = Some(format!("A: = {}", p.file_name().unwrap_or_default().to_string_lossy()));
                                menu_state.floppy = Some(p);
                            }
                            Err(e) => menu_state.message = Some(e),
                        },
                        crate::menu::Action::Cd(p) => match p.as_deref().map(crate::cdrom::Disc::open).transpose() {
                            Ok(_) => {
                                let _ = tx.send(InputEvent::InsertCd(p.clone()));
                                menu_state.message = Some(match &p {
                                    Some(p) => format!("CD = {}", p.file_name().unwrap_or_default().to_string_lossy()),
                                    None => "CD ejected".into(),
                                });
                                menu_state.cd = p;
                            }
                            Err(e) => menu_state.message = Some(e),
                        },
                    }
                    menu_state.speed = speed;
                    menu_state.fullscreen = fullscreen;
                }
                Event::KeyUp { scancode: Some(sc), .. } if menu.is_some() => {
                    if held.contains(&sc) {
                        held.retain(|&h| h != sc);
                        send_key(&tx, Some(sc), true);
                    }
                    swallowed.retain(|&s| s != sc);
                }
                Event::KeyDown { scancode: Some(Scancode::Return), keymod, repeat: false, .. }
                    if keymod.intersects(sdl3::keyboard::Mod::LALTMOD | sdl3::keyboard::Mod::RALTMOD) =>
                {
                    swallowed.push(Scancode::Return);
                    fullscreen = !fullscreen;
                    let _ = canvas.window_mut().set_fullscreen(fullscreen);
                    redraw = true;
                }
                Event::KeyUp { scancode: Some(sc), .. } if swallowed.contains(&sc) => swallowed.retain(|&s| s != sc),
                Event::JoyDeviceAdded { which, .. } => open_joystick(which, &mut joystick),
                Event::JoyDeviceRemoved { which, .. } if joystick.as_ref().is_some_and(|j| j.id() == which) => {
                    joystick = None;
                    let _ = tx.send(InputEvent::JoyPresent(0));
                }
                Event::JoyAxisMotion { axis_idx, value, .. } if axis_idx < 4 => {
                    let _ = tx.send(InputEvent::JoyAxis(axis_idx, value));
                }
                Event::JoyButtonDown { button_idx, .. } => {
                    let _ = tx.send(InputEvent::JoyButton(button_idx, true));
                }
                Event::JoyButtonUp { button_idx, .. } => {
                    let _ = tx.send(InputEvent::JoyButton(button_idx, false));
                }
                Event::MouseButtonDown { mouse_btn: sdl3::mouse::MouseButton::Middle, .. } if captured => {
                    swallow_middle = true;
                    set_capture(&mut canvas, false, &mut captured, speed);
                }
                Event::MouseButtonUp { mouse_btn: sdl3::mouse::MouseButton::Middle, .. } if swallow_middle => {
                    swallow_middle = false;
                }
                // Menu ouvert : la souris reste libre.
                Event::MouseButtonDown { .. } | Event::MouseButtonUp { .. } | Event::MouseMotion { .. } if menu.is_some() => {}
                Event::MouseButtonDown { .. } if !captured => set_capture(&mut canvas, true, &mut captured, speed),
                Event::MouseMotion { .. } | Event::MouseButtonDown { .. } | Event::MouseButtonUp { .. } if !captured => {}
                Event::KeyDown { scancode, repeat, .. } => {
                    // Répétition : le clavier PC renvoie l'appui.
                    let _ = repeat;
                    if let Some(sc) = scancode
                        && !held.contains(&sc)
                    {
                        held.push(sc);
                    }
                    send_key(&tx, scancode, false);
                }
                Event::KeyUp { scancode, .. } => {
                    held.retain(|&h| Some(h) != scancode);
                    send_key(&tx, scancode, true);
                }
                Event::Window { win_event: sdl3::event::WindowEvent::PixelSizeChanged(..) | sdl3::event::WindowEvent::Exposed, .. } => redraw = true,
                Event::MouseMotion { xrel, yrel, .. } => {
                    let _ = tx.send(InputEvent::Mouse { dx: xrel as i32, dy: yrel as i32, buttons });
                }
                Event::MouseButtonDown { mouse_btn, .. } | Event::MouseButtonUp { mouse_btn, .. } => {
                    let bit = match mouse_btn {
                        sdl3::mouse::MouseButton::Left => 1,
                        sdl3::mouse::MouseButton::Right => 2,
                        sdl3::mouse::MouseButton::Middle => 4,
                        _ => 0,
                    };
                    if matches!(ev, Event::MouseButtonDown { .. }) {
                        buttons |= bit;
                    } else {
                        buttons &= !bit;
                    }
                    let _ = tx.send(InputEvent::Mouse { dx: 0, dy: 0, buttons });
                }
                _ => {}
            }
        }
        if let Ok(result) = done_rx.try_recv() {
            return result;
        }
        if !visible && graphics_seen.load(Ordering::Relaxed) {
            visible = true;
            canvas.window_mut().show();
            canvas.window_mut().raise();
            redraw = true;
        }
        if let Some(s) = &sound {
            feed_audio(s, &queue);
        }
        let new = {
            let f = frame.lock().map_err(|e| e.to_string())?;
            (f.seq != shown && f.width > 0).then(|| {
                shown = f.seq;
                let bytes: Vec<u8> = f.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
                (f.width, f.height, bytes)
            })
        };
        if let Some((w, h, bytes)) = new {
            if size != (w, h) || texture.is_none() {
                let mut t =
                    creator.create_texture_streaming(PixelFormat::XRGB8888, w as u32, h as u32).map_err(|e| e.to_string())?;
                t.set_scale_mode(sdl3::render::ScaleMode::Nearest);
                texture = Some(t);
                size = (w, h);
            }
            let t = texture.as_mut().expect("texture");
            t.update(None, &bytes, w * 4).map_err(|e| e.to_string())?;
            redraw = true;
        }
        if visible && redraw && let Some(t) = texture.as_ref() {
            redraw = false;
            let (w, h) = size;
            // Rapport 4:3, centré.
            let (ww, wh) = canvas.output_size().map_err(|e| e.to_string())?;
            let (ww, wh) = (ww as f32, wh as f32);
            let (dw, dh) = if ww * 3.0 > wh * 4.0 { (wh * 4.0 / 3.0, wh) } else { (ww, ww * 3.0 / 4.0) };
            // Mise à l'échelle nette : agrandissement entier sans lissage
            // (chaque ligne répétée le même nombre de fois), puis réduction
            // lissée à la taille finale. Un étirement direct au plus proche
            // voisin doublerait certaines lignes et pas d'autres.
            let kx = ((dw / w as f32).ceil() as usize).max(1);
            let ky = ((dh / h as f32).ceil() as usize).max(1);
            let up_size = ((w * kx) as u32, (h * ky) as u32);
            if upscaled.as_ref().is_none_or(|(sz, _)| *sz != up_size) {
                let mut u = creator.create_texture_target(PixelFormat::XRGB8888, up_size.0, up_size.1).map_err(|e| e.to_string())?;
                u.set_scale_mode(sdl3::render::ScaleMode::Linear);
                upscaled = Some((up_size, u));
            }
            let (_, up) = upscaled.as_mut().expect("upscaled texture");
            let mut copied = Ok(());
            canvas
                .with_texture_canvas(up, |c| copied = c.copy(t, None, None))
                .map_err(|e| e.to_string())?;
            copied.map_err(|e| e.to_string())?;
            canvas.set_draw_color(sdl3::pixels::Color::RGB(0, 0, 0));
            canvas.clear();
            canvas.copy(up, None, FRect::new((ww - dw) / 2.0, (wh - dh) / 2.0, dw, dh)).map_err(|e| e.to_string())?;
            if let Some(m) = &menu {
                draw_menu(&mut canvas, &creator, m, &menu_state, &menu_font, (ww, wh))?;
            }
            canvas.present();
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Lanceur dans une fenêtre (`launcher::Ui`) : rend le profil à lancer, ou
/// `None` (Échap, fenêtre fermée).
pub fn launcher(ui: &mut crate::launcher::Ui) -> Result<Option<std::path::PathBuf>, String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let window = video.window("Bare86", 960, 720).position_centered().resizable().high_pixel_density().build().map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas();
    let creator = canvas.texture_creator();
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let font = crate::font::Font::builtin();
    let mut redraw = true;
    loop {
        while let Some(ev) = events.poll_event() {
            match ev {
                Event::Quit { .. } => return Ok(None),
                Event::KeyDown { scancode: Some(sc), .. } => {
                    if let Some(key) = menu_key(sc) {
                        match ui.key(key) {
                            crate::launcher::UiAction::None => {}
                            crate::launcher::UiAction::Quit => return Ok(None),
                            crate::launcher::UiAction::Launch(conf) => return Ok(Some(conf)),
                        }
                    }
                    redraw = true;
                }
                Event::Window { .. } => redraw = true,
                _ => {}
            }
        }
        if !redraw {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        redraw = false;
        let (w, h, px) = crate::menu::rasterize(&ui.render(), &font);
        let mut t = creator.create_texture_streaming(PixelFormat::XRGB8888, w as u32, h as u32).map_err(|e| e.to_string())?;
        t.set_scale_mode(sdl3::render::ScaleMode::Nearest);
        let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
        t.update(None, &bytes, w * 4).map_err(|e| e.to_string())?;
        let (ww, wh) = canvas.output_size().map_err(|e| e.to_string())?;
        let (ww, wh) = (ww as f32, wh as f32);
        let k = ((ww / w as f32).min(wh / h as f32)).floor().max(1.0);
        let (mw, mh) = (w as f32 * k, h as f32 * k);
        canvas.set_draw_color(sdl3::pixels::Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.copy(&t, None, FRect::new(((ww - mw) / 2.0).floor(), ((wh - mh) / 2.0).floor(), mw, mh)).map_err(|e| e.to_string())?;
        canvas.present();
    }
}

/// Raccourcis de Bare86.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Menu,
    Fullscreen,
    ReleaseMouse,
    Screenshot,
    NextDisk,
    Slower,
    Faster,
}

/// Raccourci désigné par une touche : Ctrl+Alt (Alt Gr compris) + la
/// lettre du menu, F, G, S, D, − ou + (rangée du haut ou pavé numérique).
fn host_shortcut(sc: Scancode, m: sdl3::keyboard::Mod, menu: Scancode) -> Option<Shortcut> {
    use sdl3::keyboard::Mod;
    if !(m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) && m.intersects(Mod::LALTMOD | Mod::RALTMOD)) {
        return None;
    }
    if sc == menu {
        return Some(Shortcut::Menu);
    }
    Some(match sc {
        Scancode::F => Shortcut::Fullscreen,
        Scancode::G => Shortcut::ReleaseMouse,
        Scancode::S => Shortcut::Screenshot,
        Scancode::D => Shortcut::NextDisk,
        Scancode::Minus | Scancode::KpMinus => Shortcut::Slower,
        Scancode::Equals | Scancode::KpPlus => Shortcut::Faster,
        _ => return None,
    })
}

/// Touche SDL → touche du menu.
fn menu_key(sc: Scancode) -> Option<crate::menu::Key> {
    use crate::menu::Key;
    Some(match sc {
        Scancode::Up | Scancode::Kp8 => Key::Up,
        Scancode::Down | Scancode::Kp2 => Key::Down,
        Scancode::Left | Scancode::Kp4 => Key::Left,
        Scancode::Right | Scancode::Kp6 => Key::Right,
        Scancode::Return | Scancode::KpEnter | Scancode::Space => Key::Enter,
        Scancode::Escape => Key::Esc,
        Scancode::PageUp | Scancode::Kp9 => Key::PageUp,
        Scancode::PageDown | Scancode::Kp3 => Key::PageDown,
        Scancode::Home | Scancode::Kp7 => Key::Home,
        Scancode::End | Scancode::Kp1 => Key::End,
        Scancode::Backspace => Key::Backspace,
        Scancode::Delete => Key::Delete,
        Scancode::E | Scancode::F2 => Key::Edit,
        _ => return None,
    })
}

/// Menu par-dessus l'image : fond assombri, boîte agrandie d'un facteur
/// entier (net), centrée.
fn draw_menu(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    creator: &sdl3::render::TextureCreator<sdl3::video::WindowContext>,
    menu: &crate::menu::Menu,
    state: &crate::menu::State,
    font: &crate::font::Font,
    (ww, wh): (f32, f32),
) -> Result<(), String> {
    let (w, h, px) = crate::menu::rasterize(&menu.render(state), font);
    let mut t = creator.create_texture_streaming(PixelFormat::XRGB8888, w as u32, h as u32).map_err(|e| e.to_string())?;
    t.set_scale_mode(sdl3::render::ScaleMode::Nearest);
    let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
    t.update(None, &bytes, w * 4).map_err(|e| e.to_string())?;
    canvas.set_blend_mode(sdl3::render::BlendMode::Blend);
    canvas.set_draw_color(sdl3::pixels::Color::RGBA(0, 0, 0, 140));
    canvas.fill_rect(FRect::new(0.0, 0.0, ww, wh)).map_err(|e| e.to_string())?;
    let k = ((ww * 0.92 / w as f32).min(wh * 0.92 / h as f32)).floor().max(1.0);
    let (mw, mh) = (w as f32 * k, h as f32 * k);
    canvas.copy(&t, None, FRect::new(((ww - mw) / 2.0).floor(), ((wh - mh) / 2.0).floor(), mw, mh)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Shortcut, host_shortcut, step_speed};
    use sdl3::keyboard::{Mod, Scancode};

    #[test]
    fn shortcuts() {
        let ca = Mod::LCTRLMOD | Mod::LALTMOD;
        assert_eq!(host_shortcut(Scancode::M, ca, Scancode::M), Some(Shortcut::Menu));
        // Lettre du menu réglée sur K : M redevient une touche ordinaire.
        assert_eq!(host_shortcut(Scancode::K, ca, Scancode::K), Some(Shortcut::Menu));
        assert_eq!(host_shortcut(Scancode::M, ca, Scancode::K), None);
        // Alt Gr (Alt droit) avec Ctrl droit ; pavé numérique.
        assert_eq!(host_shortcut(Scancode::KpPlus, Mod::RCTRLMOD | Mod::RALTMOD, Scancode::M), Some(Shortcut::Faster));
        assert_eq!(host_shortcut(Scancode::Minus, ca, Scancode::M), Some(Shortcut::Slower));
        assert_eq!(host_shortcut(Scancode::G, ca | Mod::LSHIFTMOD, Scancode::M), Some(Shortcut::ReleaseMouse));
        // Ctrl seul, Alt seul, ou autre touche : pour l'invité.
        assert_eq!(host_shortcut(Scancode::M, Mod::LCTRLMOD, Scancode::M), None);
        assert_eq!(host_shortcut(Scancode::F, Mod::LALTMOD, Scancode::M), None);
        assert_eq!(host_shortcut(Scancode::X, ca, Scancode::M), None);
        assert_eq!(host_shortcut(Scancode::F1, Mod::LCTRLMOD, Scancode::M), None);
    }

    #[test]
    fn speed_ladder() {
        assert_eq!(step_speed(None, false), Some(400.0));
        assert_eq!(step_speed(None, true), None);
        assert_eq!(step_speed(Some(400.0), true), None);
        assert_eq!(step_speed(Some(25.0), true), Some(50.0));
        assert_eq!(step_speed(Some(25.0), false), Some(12.0));
        assert_eq!(step_speed(Some(3.0), false), Some(3.0));
        // Valeur hors échelle (--mips 30) : cran supérieur ou inférieur le plus proche.
        assert_eq!(step_speed(Some(30.0), true), Some(100.0));
        assert_eq!(step_speed(Some(30.0), false), Some(25.0));
    }
}
