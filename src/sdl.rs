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

struct SharedDisplay(Arc<Mutex<SharedFrame>>);

impl Display for SharedDisplay {
    fn present(&mut self, frame: &Frame<'_>) {
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

impl InputSource for ChannelInput {
    fn poll(&mut self) -> Option<InputEvent> {
        self.0.try_recv().ok()
    }
    fn wait(&mut self) -> Option<InputEvent> {
        self.0.recv().ok()
    }
}

/// Vitesses de Ctrl+F11 / Ctrl+F12 (MIPS) ; au-delà, sans bridage.
const SPEEDS: [f64; 8] = [3.0, 6.0, 12.0, 25.0, 50.0, 100.0, 200.0, 400.0];

/// Cran suivant (`faster`) ou précédent de l'échelle des vitesses.
fn step_speed(current: Option<f64>, faster: bool) -> Option<f64> {
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
        Ok(()) => eprintln!("capture : {path}"),
        Err(e) => eprintln!("{path} : {e}"),
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
    let Some(code) = scancode.and_then(|s| set1(s as i32)) else { return };
    if code & 0x100 != 0 {
        let _ = tx.send(InputEvent::Key(0xE0));
    }
    let _ = tx.send(InputEvent::Key(code as u8 | if release { 0x80 } else { 0 }));
}

/// Ouvre la fenêtre et exécute `machine` (qui construit et lance la
/// machine avec l'hôte fourni) sur le thread du vCPU. Rend son résultat, ou
/// 0 si la fenêtre est fermée avant la fin du programme.
pub fn run<F>(title: &str, initial_mips: Option<f64>, start_fullscreen: bool, machine: F) -> Result<u8, String>
where
    F: FnOnce(Host) -> Result<u8, String> + Send + 'static,
{
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let window = video.window(title, 960, 720).position_centered().resizable().build().map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas();
    let creator = canvas.texture_creator();
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;

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
    let display = SharedDisplay(frame.clone());
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
            eprintln!("joystick : {} ({} axes)", j.name(), j.num_axes());
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
            let host = Host {
                display: Box::new(display),
                audio: Box::new(crate::audio::QueueSink(vcpu_queue)),
                input: Box::new(ChannelInput(rx)),
                text: Box::new(crate::host::Discard),
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
    // fenêtre la capture, le bouton central, Ctrl+F10 ou la perte du focus
    // la libèrent.
    let mouse = sdl.mouse();
    let mut captured = false;
    let mut swallow_middle = false;
    // Raccourcis (Ctrl+F5, F10, F11, F12, Alt+Entrée) : leur relâchement
    // n'est pas transmis à l'invité.
    let mut swallowed: Vec<Scancode> = Vec::new();
    let mut speed = initial_mips;
    let mut fullscreen = start_fullscreen;
    if fullscreen {
        let _ = canvas.window_mut().set_fullscreen(true);
    }
    let set_title = |canvas: &mut sdl3::render::Canvas<sdl3::video::Window>, captured: bool, speed: Option<f64>| {
        let hint = if captured { "bouton central ou Ctrl+F10 libère la souris" } else { "cliquer pour capturer la souris" };
        let mips = speed.map_or("vitesse max".to_string(), |m| format!("{m} MIPS"));
        let _ = canvas.window_mut().set_title(&format!("{title} — {mips} — {hint}"));
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
                Event::KeyDown { scancode: Some(sc @ (Scancode::F5 | Scancode::F10 | Scancode::F11 | Scancode::F12)), keymod, repeat, .. }
                    if keymod.intersects(sdl3::keyboard::Mod::LCTRLMOD | sdl3::keyboard::Mod::RCTRLMOD)
                        && (sc != Scancode::F10 || captured) =>
                {
                    swallowed.push(sc);
                    match sc {
                        Scancode::F10 => set_capture(&mut canvas, false, &mut captured, speed),
                        Scancode::F5 if !repeat => screenshot(&frame),
                        // Ctrl+F11 : plus lent ; Ctrl+Maj+F11 (ou Ctrl+F12, souvent pris
                        // par le bureau, KDE par exemple) : plus vite.
                        Scancode::F11 | Scancode::F12 => {
                            let shift = keymod.intersects(sdl3::keyboard::Mod::LSHIFTMOD | sdl3::keyboard::Mod::RSHIFTMOD);
                            speed = step_speed(speed, sc == Scancode::F12 || shift);
                            let _ = tx.send(InputEvent::Speed(speed));
                            set_title(&mut canvas, captured, speed);
                        }
                        _ => {}
                    }
                }
                Event::KeyDown { scancode: Some(Scancode::Return), keymod, repeat: false, .. }
                    if keymod.intersects(sdl3::keyboard::Mod::LALTMOD | sdl3::keyboard::Mod::RALTMOD) =>
                {
                    swallowed.push(Scancode::Return);
                    fullscreen = !fullscreen;
                    let _ = canvas.window_mut().set_fullscreen(fullscreen);
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
                Event::MouseButtonDown { .. } if !captured => set_capture(&mut canvas, true, &mut captured, speed),
                Event::MouseMotion { .. } | Event::MouseButtonDown { .. } | Event::MouseButtonUp { .. } if !captured => {}
                Event::KeyDown { scancode, repeat, .. } => {
                    // Répétition : le clavier PC renvoie l'appui.
                    let _ = repeat;
                    send_key(&tx, scancode, false);
                }
                Event::KeyUp { scancode, .. } => send_key(&tx, scancode, true),
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
            let (_, up) = upscaled.as_mut().expect("texture agrandie");
            let mut copied = Ok(());
            canvas
                .with_texture_canvas(up, |c| copied = c.copy(t, None, None))
                .map_err(|e| e.to_string())?;
            copied.map_err(|e| e.to_string())?;
            canvas.set_draw_color(sdl3::pixels::Color::RGB(0, 0, 0));
            canvas.clear();
            canvas.copy(up, None, FRect::new((ww - dw) / 2.0, (wh - dh) / 2.0, dw, dh)).map_err(|e| e.to_string())?;
            canvas.present();
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::step_speed;

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
