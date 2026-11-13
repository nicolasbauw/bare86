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

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sdl3::event::Event;
use sdl3::keyboard::Scancode;
use sdl3::pixels::PixelFormat;
use sdl3::render::FRect;

use crate::host::{Display, Frame, Host, InputEvent, InputSource, NullAudio};

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

const AUDIO_RATE: u32 = 48_000;
/// Son en attente visé : 40 ms, par blocs de 10 ms.
const AUDIO_TARGET: i32 = (AUDIO_RATE / 25 * 2) as i32;
const AUDIO_BLOCK: usize = (AUDIO_RATE / 100) as usize;

/// Complète le flux audio d'une onde carrée à `centi_hz` / 100 Hz (0 : silence).
fn feed_speaker(stream: &sdl3::audio::AudioStream, centi_hz: u32, phase: &mut f64) {
    let freq = f64::from(centi_hz) / 100.0;
    while stream.queued_bytes().unwrap_or(AUDIO_TARGET) < AUDIO_TARGET {
        let mut block = [0i16; AUDIO_BLOCK];
        // Au-delà de 20 kHz (modulation de largeur d'impulsion), silence.
        if freq > 0.0 && freq < 20_000.0 {
            for s in block.iter_mut() {
                *s = if *phase < 0.5 { 6000 } else { -6000 };
                *phase = (*phase + freq / f64::from(AUDIO_RATE)).fract();
            }
        }
        if stream.put_data_i16(&block).is_err() {
            return;
        }
    }
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
pub fn run<F>(title: &str, machine: F) -> Result<u8, String>
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
    // Haut-parleur : onde carrée générée ici d'après la fréquence publiée par le vCPU.
    let speaker = Arc::new(AtomicU32::new(0));
    let audio = sdl.audio().map_err(|e| e.to_string())?;
    let spec = sdl3::audio::AudioSpec {
        freq: Some(AUDIO_RATE as i32),
        channels: Some(1),
        format: Some(sdl3::audio::AudioFormat::S16LE),
    };
    let sound = audio.default_playback_device().open_device_stream(Some(&spec)).ok();
    if let Some(s) = &sound {
        let _ = s.resume();
    }
    let mut phase = 0.0f64;
    let vcpu_speaker = speaker.clone();
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let display = SharedDisplay(frame.clone());
    std::thread::Builder::new()
        .name("vcpu".into())
        .spawn(move || {
            let host = Host {
                display: Box::new(display),
                audio: Box::new(NullAudio::default()),
                input: Box::new(ChannelInput(rx)),
                text: Box::new(crate::host::Discard),
                term: None,
                quit: false,
                wants_frames: true,
                speaker: Some(vcpu_speaker),
            };
            let _ = done_tx.send(machine(host));
        })
        .map_err(|e| e.to_string())?;

    let mut texture = None;
    let mut size = (0, 0);
    let mut shown = 0u64;
    let mut buttons = 0u8;
    loop {
        while let Some(ev) = events.poll_event() {
            match ev {
                Event::Quit { .. } => return Ok(0),
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
            feed_speaker(s, speaker.load(Ordering::Relaxed), &mut phase);
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
            canvas.set_draw_color(sdl3::pixels::Color::RGB(0, 0, 0));
            canvas.clear();
            canvas.copy(t, None, FRect::new((ww - dw) / 2.0, (wh - dh) / 2.0, dw, dh)).map_err(|e| e.to_string())?;
            canvas.present();
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
