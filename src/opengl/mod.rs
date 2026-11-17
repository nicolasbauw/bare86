// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenGL 1.1 servi par l'hôte : l'`opengl32.dll` de Bare86
//! (guest/opengl32) range les appels du programme dans un tampon de
//! commandes et l'envoie par `out 0E3h, al` (EBX = tampon dans DS, ECX =
//! longueur en octets, EAX = valeur rendue). Chaque commande : un en-tête
//! `(numéro << 16) | nombre de mots d'arguments`, puis les arguments. Les
//! numéros 1 à 336 sont les fonctions d'OpenGL 1.1 dans l'ordre de
//! guest/opengl32/gl11.txt (`api::op`) ; au-delà, les commandes de Bare86.
//!
//! L'hôte rejoue les appels dans un contexte OpenGL hors écran en profil
//! « compatibilité » : presque tout passe tel quel (`api.rs`, produit par
//! guest/opengl32/gen.py). Le code écrit ici traite ce qui touche à la
//! mémoire du programme (images, tableaux de sommets, glGet*), les tampons
//! avant et arrière (deux tampons de couleur d'un FBO, échangés par
//! `wglSwapBuffers`) et les chaînes de glGetString. L'image du tampon avant
//! remplace celle de la VGA tant que le contexte est courant.

#[allow(dead_code)]
mod api;

use crate::glide::gl::Offscreen;
use api::{Fns, op};
use glow::HasContext;
use std::collections::HashSet;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Port d'appel d'OpenGL.
pub const PORT: u16 = 0xE3;

/// actif (0/1) ; rend 1 si l'hôte a un OpenGL
const MAKE_CURRENT: u16 = 0x401;
/// intervalle de balayage
const SWAP: u16 = 0x402;
/// nom (32 octets) d'une fonction absente de la DLL
const MISSING: u16 = 0x403;

/// Extensions annoncées (sans nouveau point d'entrée, sauf
/// `wglSwapIntervalEXT`, servie par la DLL).
const EXTENSIONS: &str = "GL_EXT_abgr GL_EXT_bgra GL_EXT_packed_pixels GL_EXT_texture_env_add WGL_EXT_swap_control";

// Énumérations d'OpenGL utilisées ici.
const GL_FRONT_LEFT: u32 = 0x400;
const GL_FRONT_RIGHT: u32 = 0x401;
const GL_FRONT: u32 = 0x404;
const GL_BACK: u32 = 0x405;
const GL_LEFT: u32 = 0x406;
const GL_FRONT_AND_BACK: u32 = 0x408;
const GL_DRAW_BUFFER: u32 = 0x0C01;
const GL_READ_BUFFER: u32 = 0x0C02;
const GL_DOUBLEBUFFER: u32 = 0x0C32;
const GL_FEEDBACK: u32 = 0x1C01;
const GL_SELECT: u32 = 0x1C02;
const GL_RENDERER: u32 = 0x1F01;
const GL_TEXTURE_WIDTH: u32 = 0x1000;
const GL_TEXTURE_HEIGHT: u32 = 0x1001;
const GL_TEXTURE_1D: u32 = 0x0DE0;
const GL_CLIENT_PIXEL_STORE_BIT: u32 = 1;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_PACK_ROW_LENGTH: u32 = 0x0D02;
const GL_PACK_SKIP_ROWS: u32 = 0x0D03;
const GL_PACK_SKIP_PIXELS: u32 = 0x0D04;
const GL_UNPACK_ROW_LENGTH: u32 = 0x0CF2;

/// Tableaux de sommets du client, dans l'ordre de `CAPS`.
const VERTEX: usize = 0;
const NORMAL: usize = 1;
const COLOR: usize = 2;
const INDEX: usize = 3;
const TEXCOORD: usize = 4;
const EDGE: usize = 5;
/// GL_VERTEX_ARRAY… GL_EDGE_FLAG_ARRAY.
const CAPS: [u32; 6] = [0x8074, 0x8075, 0x8076, 0x8077, 0x8078, 0x8079];
/// GL_VERTEX_ARRAY_POINTER… GL_EDGE_FLAG_ARRAY_POINTER.
const POINTERS: [u32; 6] = [0x808E, 0x808F, 0x8090, 0x8091, 0x8092, 0x8093];

/// Taille d'une valeur d'un type de données d'OpenGL.
fn type_size(ty: u32) -> usize {
    match ty {
        0x1400 | 0x1401 => 1,          // BYTE, UNSIGNED_BYTE
        0x1402 | 0x1403 | 0x1407 => 2, // SHORT, UNSIGNED_SHORT, 2_BYTES
        0x1408 => 3,                   // 3_BYTES
        0x140A => 8,                   // DOUBLE
        _ => 4,                        // INT, UNSIGNED_INT, FLOAT, 4_BYTES
    }
}

/// Tableau du client : guest = adresse dans le programme.
#[derive(Clone, Copy, Debug, Default)]
struct Array {
    size: i32,
    ty: u32,
    stride: i32,
    ptr: u32,
}

impl Array {
    fn elem(&self) -> usize {
        self.size.max(1) as usize * type_size(self.ty)
    }
    fn step(&self) -> usize {
        if self.stride > 0 { self.stride as usize } else { self.elem() }
    }
}

#[derive(Clone, Debug, Default)]
struct Client {
    arrays: [Array; 6],
    enabled: [bool; 6],
}

/// Pixels d'une image : (composantes par pixel, octets par composante),
/// les types « tassés » comptant pour une seule composante.
fn pixel_layout(format: u32, ty: u32) -> (usize, usize) {
    let comps = match format {
        0x1907 | 0x80E0 => 3,                   // RGB, BGR
        0x1908 | 0x80E1 | 0x8000 => 4,          // RGBA, BGRA, ABGR_EXT
        0x190A => 2,                            // LUMINANCE_ALPHA
        0x1900..=0x1906 | 0x1909 => 1,          // index, stencil, profondeur, R, G, B, A, L
        _ => 4,
    };
    match ty {
        0x8032 | 0x8362 => (1, 1),
        0x8033 | 0x8034 | 0x8363..=0x8366 => (1, 2),
        0x8035 | 0x8036 | 0x8367 | 0x8368 => (1, 4),
        _ => (comps, type_size(ty)),
    }
}

/// Octets lus ou écrits pour une image de `w`×`h` selon l'état de
/// rangement (`base` = GL_UNPACK_ROW_LENGTH ou GL_PACK_ROW_LENGTH ; les
/// trois suivants : lignes sautées, pixels sautés, alignement).
fn image_bytes(f: &Fns, base: u32, w: i32, h: i32, format: u32, ty: u32) -> usize {
    if w <= 0 || h <= 0 {
        return 0;
    }
    let get = |p: u32| {
        let mut v = 0i32;
        // SAFETY: un entier pour ces quatre paramètres.
        unsafe { (f.glGetIntegerv)(p, &mut v as *mut i32 as *const c_void) };
        v.max(0) as usize
    };
    let (row_len, skip_rows, skip_pixels, align) = (get(base), get(base + 1), get(base + 2), get(base + 3).max(1));
    let (w, h) = (w as usize, h as usize);
    let row_len = if row_len > 0 { row_len } else { w };
    if ty == 0x1A00 {
        // GL_BITMAP : un bit par pixel.
        let stride = row_len.div_ceil(8).next_multiple_of(align);
        return (skip_rows + h - 1) * stride + (skip_pixels + w).div_ceil(8);
    }
    let (comps, size) = pixel_layout(format, ty);
    let group = comps * size;
    let row = row_len * group;
    let stride = if size >= align { row } else { row.next_multiple_of(align) };
    (skip_rows + h - 1) * stride + (skip_pixels + w) * group
}

/// Valeurs écrites par une fonction glGet* : appelée deux fois sur des
/// tampons remplis de motifs différents, ce qu'elle a écrit est ce qui a
/// changé dans l'un ou l'autre ; rend les octets des valeurs écrites.
fn sentinel_get(es: usize, cap: usize, call: &mut dyn FnMut(*mut c_void)) -> Vec<u8> {
    let mut a = vec![0xA5u8; es * cap];
    call(a.as_mut_ptr().cast());
    let mut b = vec![0x5Au8; es * cap];
    call(b.as_mut_ptr().cast());
    let n = (0..cap)
        .rev()
        .find(|&i| a[i * es..(i + 1) * es].iter().any(|&x| x != 0xA5) || b[i * es..(i + 1) * es].iter().any(|&x| x != 0x5A))
        .map_or(0, |i| i + 1);
    a.truncate(n * es);
    a
}

struct Gpu {
    off: Offscreen,
    f: Rc<Fns>,
}

pub struct OpenGl {
    gpu: Option<Gpu>,
    failed: Option<String>,
    /// Contexte courant : l'image du tampon avant remplace celle de la VGA.
    pub active: bool,
    width: usize,
    height: usize,
    /// Tampon de couleur arrière (0 ou 1) ; l'autre est affiché.
    back: usize,
    /// Tampons demandés par glDrawBuffer et glReadBuffer.
    draw: u32,
    read: u32,
    frame: Vec<u32>,
    frame_dirty: bool,
    client: Client,
    client_stack: Vec<Client>,
    render_mode: u32,
    feedback: Option<(u32, Vec<f32>)>,
    select: Option<(u32, Vec<u32>)>,
    /// Copies des tableaux du client pour le dessin en cours.
    scratch: Vec<Vec<u8>>,
    last_swap: Option<Instant>,
    missing: HashSet<String>,
    pub swaps: u64,
}

impl Default for OpenGl {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenGl {
    pub fn new() -> Self {
        Self {
            gpu: None,
            failed: None,
            active: false,
            width: 640,
            height: 480,
            back: 0,
            draw: GL_BACK,
            read: GL_BACK,
            frame: Vec::new(),
            frame_dirty: false,
            client: Client::default(),
            client_stack: Vec::new(),
            render_mode: 0x1C00,
            feedback: None,
            select: None,
            scratch: Vec::new(),
            last_swap: None,
            missing: HashSet::new(),
            swaps: 0,
        }
    }

    /// Image à afficher (contexte courant) : relue après un échange ou un
    /// dessin dans le tampon avant.
    pub fn frame(&mut self) -> Option<(usize, usize, Vec<u32>)> {
        if !self.active {
            return None;
        }
        if self.frame_dirty || self.frame.len() != self.width * self.height {
            let (w, h) = (self.width, self.height);
            let front = self.back ^ 1;
            let g = self.gpu.as_ref()?;
            g.off.make_current();
            let mut rgba = vec![0u8; w * h * 4];
            // SAFETY: contexte courant ; tampon de la taille de l'image ;
            // l'état de rangement du programme est sauvé puis rétabli.
            unsafe {
                (g.f.glPushClientAttrib)(GL_CLIENT_PIXEL_STORE_BIT);
                (g.f.glPixelStorei)(GL_PACK_ALIGNMENT, 4);
                (g.f.glPixelStorei)(GL_PACK_ROW_LENGTH, 0);
                (g.f.glPixelStorei)(GL_PACK_SKIP_ROWS, 0);
                (g.f.glPixelStorei)(GL_PACK_SKIP_PIXELS, 0);
                g.off.gl.read_buffer(glow::COLOR_ATTACHMENT0 + front as u32);
                g.off.gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut rgba)));
                (g.f.glPopClientAttrib)();
            }
            self.apply_buffers();
            self.frame.clear();
            self.frame.reserve(w * h);
            for y in (0..h).rev() {
                for p in rgba[y * w * 4..(y + 1) * w * 4].chunks_exact(4) {
                    self.frame.push(u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]));
                }
            }
            self.frame_dirty = false;
        }
        (self.frame.len() == self.width * self.height).then(|| (self.width, self.height, self.frame.clone()))
    }

    /// Tampons de couleur réels des tampons demandés (avant, arrière).
    fn buffers_of(&self, mode: u32) -> Vec<u32> {
        let front = glow::COLOR_ATTACHMENT0 + (self.back ^ 1) as u32;
        let back = glow::COLOR_ATTACHMENT0 + self.back as u32;
        match mode {
            0 => vec![glow::NONE],
            GL_FRONT_LEFT | GL_FRONT_RIGHT | GL_FRONT => vec![front],
            GL_LEFT | GL_FRONT_AND_BACK => vec![front, back],
            _ => vec![back],
        }
    }

    fn apply_buffers(&self) {
        let Some(g) = &self.gpu else { return };
        let draw = self.buffers_of(self.draw);
        let read = self.buffers_of(self.read)[0];
        // SAFETY: contexte courant ; attachements existants du FBO.
        unsafe {
            g.off.gl.draw_buffers(&draw);
            g.off.gl.read_buffer(read);
        }
    }

    fn draws_front(&self) -> bool {
        matches!(self.draw, GL_FRONT_LEFT | GL_FRONT_RIGHT | GL_FRONT | GL_LEFT | GL_FRONT_AND_BACK)
    }

    /// Exécute un tampon de commandes. `screen` : taille du mode vidéo en
    /// cours (celle du FBO à `wglMakeCurrent`) ; `read` et `write` lisent et
    /// écrivent la mémoire de l'invité à une adresse de son segment de
    /// données. Rend la valeur de la dernière commande.
    pub fn execute(
        &mut self,
        words: &[u32],
        screen: (usize, usize),
        read: &mut dyn FnMut(u32, &mut [u8]),
        write: &mut dyn FnMut(u32, &[u8]),
    ) -> u32 {
        if let Some(g) = &self.gpu {
            g.off.make_current();
        }
        let mut i = 0;
        let mut ret = 0;
        while i < words.len() {
            let op = (words[i] >> 16) as u16;
            let n = (words[i] & 0xFFFF) as usize;
            let Some(args) = words.get(i + 1..i + 1 + n) else { break };
            ret = self.command(op, args, screen, read, write);
            i += 1 + n;
        }
        if self.draws_front() {
            self.frame_dirty = true;
        }
        ret
    }

    fn command(
        &mut self,
        op: u16,
        a: &[u32],
        screen: (usize, usize),
        read: &mut dyn FnMut(u32, &mut [u8]),
        write: &mut dyn FnMut(u32, &[u8]),
    ) -> u32 {
        let arg = |k: usize| a.get(k).copied().unwrap_or(0);
        match op {
            MAKE_CURRENT => {
                if arg(0) == 0 {
                    self.active = false;
                    self.frame.clear();
                    return 1;
                }
                let (w, h) = (screen.0.max(1), screen.1.max(1));
                if self.gpu.is_none() && self.failed.is_none() {
                    match Offscreen::new_compat(w, h).and_then(|off| {
                        let f = Fns::load(&|name| {
                            off.proc_address(name)
                        })?;
                        Ok(Gpu { off, f: Rc::new(f) })
                    }) {
                        Ok(g) => {
                            // SAFETY: chaîne statique du pilote, terminée par NUL.
                            let renderer = unsafe { std::ffi::CStr::from_ptr((g.f.glGetString)(GL_RENDERER).cast()) };
                            eprintln!("[opengl] host renderer: {}", renderer.to_string_lossy());
                            self.gpu = Some(g);
                            self.width = w;
                            self.height = h;
                            self.back = 0;
                            self.apply_buffers();
                        }
                        Err(e) => {
                            eprintln!("[opengl] {e}");
                            self.failed = Some(e);
                        }
                    }
                } else if let Some(g) = &mut self.gpu
                    && (g.off.width, g.off.height) != (w, h)
                {
                    g.off.resize(w, h);
                    self.width = w;
                    self.height = h;
                    self.apply_buffers();
                }
                self.active = self.gpu.is_some();
                self.frame_dirty = true;
                return u32::from(self.gpu.is_some());
            }
            MISSING => {
                let bytes: Vec<u8> = a.iter().flat_map(|w| w.to_le_bytes()).take_while(|&b| b != 0).collect();
                let name = String::from_utf8_lossy(&bytes).into_owned();
                if self.missing.insert(name.clone()) {
                    eprintln!("[opengl] {name}: not implemented");
                }
                return 0;
            }
            _ => {}
        }
        let Some(f) = self.gpu.as_ref().map(|g| g.f.clone()) else { return 0 };
        if op == SWAP {
            let interval = arg(0).min(4);
            if interval > 0
                && let Some(t) = self.last_swap
            {
                let next = t + Duration::from_micros(16_667) * interval;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
            }
            self.last_swap = Some(Instant::now());
            self.back ^= 1;
            self.apply_buffers();
            self.frame_dirty = true;
            self.swaps += 1;
            return 1;
        }
        // SAFETY: contexte de `f` courant (execute) ; arguments décodés
        // selon gl.h.
        if unsafe { f.call(op, a) }.is_some() {
            return 0;
        }
        // SAFETY: idem ; les pointeurs passés à OpenGL désignent des copies
        // locales de la taille calculée pour l'appel.
        unsafe { self.special(&f, op, a, read, write) }
    }

    /// Appels qui touchent à la mémoire du programme ou à l'état propre à
    /// Bare86.
    unsafe fn special(&mut self, f: &Fns, op: u16, a: &[u32], read: &mut dyn FnMut(u32, &mut [u8]), write: &mut dyn FnMut(u32, &[u8])) -> u32 {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let i = |k: usize| w(k) as i32;
        let fl = |k: usize| f32::from_bits(w(k));
        let d = |k: usize| f64::from_bits(u64::from(w(k)) | u64::from(w(k + 1)) << 32);
        let mut fetch = |at: u32, len: usize| {
            let mut v = vec![0u8; len];
            if at != 0 && len > 0 {
                read(at, &mut v);
            }
            v
        };
        let ptr = |at: u32, v: &Vec<u8>| if at == 0 { std::ptr::null() } else { v.as_ptr().cast::<c_void>() };
        // SAFETY: voir `command`.
        unsafe {
            match op {
                op::glGetString => {
                    let s = match w(0) {
                        0x1F00 => "Bare86".to_string(),
                        0x1F01 => {
                            let r = std::ffi::CStr::from_ptr((f.glGetString)(GL_RENDERER).cast());
                            format!("Bare86 OpenGL ({})", r.to_string_lossy())
                        }
                        0x1F02 => "1.1.0".to_string(),
                        0x1F03 => EXTENSIONS.to_string(),
                        _ => return 0,
                    };
                    let mut bytes = s.into_bytes();
                    bytes.truncate((w(2) as usize).saturating_sub(1));
                    bytes.push(0);
                    write(w(1), &bytes);
                    return 1;
                }
                op::glGetError => return (f.glGetError)(),
                op::glIsEnabled => return u32::from((f.glIsEnabled)(w(0))),
                op::glIsList => return u32::from((f.glIsList)(w(0))),
                op::glIsTexture => return u32::from((f.glIsTexture)(w(0))),
                op::glGenLists => return (f.glGenLists)(i(0)),
                op::glFinish => (f.glFinish)(),
                op::glFlush => (f.glFlush)(),
                op::glRenderMode => {
                    let ret = (f.glRenderMode)(w(0));
                    match self.render_mode {
                        GL_FEEDBACK => {
                            if let Some((at, buf)) = &self.feedback {
                                let bytes: Vec<u8> = buf.iter().flat_map(|x| x.to_le_bytes()).collect();
                                write(*at, &bytes);
                            }
                        }
                        GL_SELECT => {
                            if let Some((at, buf)) = &self.select {
                                let bytes: Vec<u8> = buf.iter().flat_map(|x| x.to_le_bytes()).collect();
                                write(*at, &bytes);
                            }
                        }
                        _ => {}
                    }
                    self.render_mode = w(0);
                    return ret as u32;
                }
                op::glFeedbackBuffer => {
                    let buf = vec![0f32; i(0).max(0) as usize];
                    (f.glFeedbackBuffer)(i(0), w(1), buf.as_ptr().cast());
                    self.feedback = Some((w(2), buf));
                }
                op::glSelectBuffer => {
                    let buf = vec![0u32; i(0).max(0) as usize];
                    (f.glSelectBuffer)(i(0), buf.as_ptr().cast());
                    self.select = Some((w(1), buf));
                }
                // --- Tampons avant et arrière ---
                op::glDrawBuffer => {
                    self.draw = w(0);
                    self.apply_buffers();
                }
                op::glReadBuffer => {
                    self.read = w(0);
                    self.apply_buffers();
                }
                // --- Valeurs de l'état ---
                op::glGetBooleanv | op::glGetIntegerv | op::glGetFloatv | op::glGetDoublev => {
                    let es = match op {
                        op::glGetBooleanv => 1,
                        op::glGetDoublev => 8,
                        _ => 4,
                    };
                    let pname = w(0);
                    let patched = match pname {
                        GL_DRAW_BUFFER => Some(self.draw),
                        GL_READ_BUFFER => Some(self.read),
                        GL_DOUBLEBUFFER => Some(1),
                        _ => None,
                    };
                    let bytes = if let Some(v) = patched {
                        match op {
                            op::glGetBooleanv => vec![u8::from(v != 0)],
                            op::glGetIntegerv => v.to_le_bytes().to_vec(),
                            op::glGetFloatv => (v as f32).to_le_bytes().to_vec(),
                            _ => f64::from(v).to_le_bytes().to_vec(),
                        }
                    } else {
                        sentinel_get(es, 256, &mut |p| match op {
                            op::glGetBooleanv => (f.glGetBooleanv)(pname, p),
                            op::glGetIntegerv => (f.glGetIntegerv)(pname, p),
                            op::glGetFloatv => (f.glGetFloatv)(pname, p),
                            _ => (f.glGetDoublev)(pname, p),
                        })
                    };
                    write(w(1), &bytes);
                }
                op::glGetLightfv | op::glGetLightiv | op::glGetMaterialfv | op::glGetMaterialiv | op::glGetTexGendv | op::glGetTexGenfv
                | op::glGetTexGeniv | op::glGetTexEnvfv | op::glGetTexEnviv | op::glGetTexParameterfv | op::glGetTexParameteriv => {
                    let es = if op == op::glGetTexGendv { 8 } else { 4 };
                    let (x, y) = (w(0), w(1));
                    let bytes = sentinel_get(es, 16, &mut |p| match op {
                        op::glGetLightfv => (f.glGetLightfv)(x, y, p),
                        op::glGetLightiv => (f.glGetLightiv)(x, y, p),
                        op::glGetMaterialfv => (f.glGetMaterialfv)(x, y, p),
                        op::glGetMaterialiv => (f.glGetMaterialiv)(x, y, p),
                        op::glGetTexGendv => (f.glGetTexGendv)(x, y, p),
                        op::glGetTexGenfv => (f.glGetTexGenfv)(x, y, p),
                        op::glGetTexGeniv => (f.glGetTexGeniv)(x, y, p),
                        op::glGetTexEnvfv => (f.glGetTexEnvfv)(x, y, p),
                        op::glGetTexEnviv => (f.glGetTexEnviv)(x, y, p),
                        op::glGetTexParameterfv => (f.glGetTexParameterfv)(x, y, p),
                        _ => (f.glGetTexParameteriv)(x, y, p),
                    });
                    write(w(2), &bytes);
                }
                op::glGetTexLevelParameterfv | op::glGetTexLevelParameteriv => {
                    let (x, l, y) = (w(0), i(1), w(2));
                    let bytes = sentinel_get(4, 16, &mut |p| {
                        if op == op::glGetTexLevelParameterfv {
                            (f.glGetTexLevelParameterfv)(x, l, y, p)
                        } else {
                            (f.glGetTexLevelParameteriv)(x, l, y, p)
                        }
                    });
                    write(w(3), &bytes);
                }
                op::glGetMapdv | op::glGetMapfv | op::glGetMapiv => {
                    let es = if op == op::glGetMapdv { 8 } else { 4 };
                    let (x, y) = (w(0), w(1));
                    let bytes = sentinel_get(es, 4096, &mut |p| match op {
                        op::glGetMapdv => (f.glGetMapdv)(x, y, p),
                        op::glGetMapfv => (f.glGetMapfv)(x, y, p),
                        _ => (f.glGetMapiv)(x, y, p),
                    });
                    write(w(2), &bytes);
                }
                op::glGetPixelMapfv | op::glGetPixelMapuiv | op::glGetPixelMapusv => {
                    let mut n = 0i32;
                    (f.glGetIntegerv)(w(0) + 0x40, (&mut n as *mut i32).cast());
                    let es = if op == op::glGetPixelMapusv { 2 } else { 4 };
                    let buf = vec![0u8; n.max(0) as usize * es];
                    let p = buf.as_ptr().cast();
                    match op {
                        op::glGetPixelMapfv => (f.glGetPixelMapfv)(w(0), p),
                        op::glGetPixelMapuiv => (f.glGetPixelMapuiv)(w(0), p),
                        _ => (f.glGetPixelMapusv)(w(0), p),
                    }
                    write(w(1), &buf);
                }
                op::glPixelMapfv | op::glPixelMapuiv | op::glPixelMapusv => {
                    let es = if op == op::glPixelMapusv { 2 } else { 4 };
                    let v = fetch(w(2), i(1).max(0) as usize * es);
                    match op {
                        op::glPixelMapfv => (f.glPixelMapfv)(w(0), i(1), v.as_ptr().cast()),
                        op::glPixelMapuiv => (f.glPixelMapuiv)(w(0), i(1), v.as_ptr().cast()),
                        _ => (f.glPixelMapusv)(w(0), i(1), v.as_ptr().cast()),
                    }
                }
                op::glGetPolygonStipple => {
                    let buf = [0u8; 128];
                    (f.glGetPolygonStipple)(buf.as_ptr().cast());
                    write(w(0), &buf);
                }
                op::glGetClipPlane => {
                    let buf = [0u8; 32];
                    (f.glGetClipPlane)(w(0), buf.as_ptr().cast());
                    write(w(1), &buf);
                }
                op::glGetPointerv => {
                    let pname = w(0);
                    let v = if let Some(k) = POINTERS.iter().position(|&p| p == pname) {
                        self.client.arrays[k].ptr
                    } else if pname == 0x0DF0 {
                        self.feedback.as_ref().map_or(0, |b| b.0)
                    } else if pname == 0x0DF3 {
                        self.select.as_ref().map_or(0, |b| b.0)
                    } else {
                        0
                    };
                    write(w(1), &v.to_le_bytes());
                }
                // --- Textures ---
                op::glGenTextures => {
                    let n = i(0).max(0) as usize;
                    let buf = vec![0u32; n];
                    (f.glGenTextures)(n as i32, buf.as_ptr().cast());
                    let bytes: Vec<u8> = buf.iter().flat_map(|x| x.to_le_bytes()).collect();
                    write(w(1), &bytes);
                }
                op::glDeleteTextures => {
                    let v = fetch(w(1), i(0).max(0) as usize * 4);
                    (f.glDeleteTextures)(i(0), v.as_ptr().cast());
                }
                op::glPrioritizeTextures => {
                    let n = i(0).max(0) as usize;
                    let t = fetch(w(1), n * 4);
                    let p = fetch(w(2), n * 4);
                    (f.glPrioritizeTextures)(i(0), t.as_ptr().cast(), p.as_ptr().cast());
                }
                op::glAreTexturesResident => {
                    let n = i(0).max(0) as usize;
                    let t = fetch(w(1), n * 4);
                    let mut r = vec![0u8; n];
                    let ret = (f.glAreTexturesResident)(i(0), t.as_ptr().cast(), r.as_mut_ptr().cast());
                    if ret == 0 {
                        write(w(2), &r);
                    }
                    return u32::from(ret);
                }
                op::glTexImage1D => {
                    let v = fetch(w(7), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(3), 1, w(5), w(6)));
                    (f.glTexImage1D)(w(0), i(1), i(2), i(3), i(4), w(5), w(6), ptr(w(7), &v));
                }
                op::glTexImage2D => {
                    let v = fetch(w(8), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(3), i(4), w(6), w(7)));
                    (f.glTexImage2D)(w(0), i(1), i(2), i(3), i(4), i(5), w(6), w(7), ptr(w(8), &v));
                }
                op::glTexSubImage1D => {
                    let v = fetch(w(6), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(3), 1, w(4), w(5)));
                    (f.glTexSubImage1D)(w(0), i(1), i(2), i(3), w(4), w(5), ptr(w(6), &v));
                }
                op::glTexSubImage2D => {
                    let v = fetch(w(8), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(4), i(5), w(6), w(7)));
                    (f.glTexSubImage2D)(w(0), i(1), i(2), i(3), i(4), i(5), w(6), w(7), ptr(w(8), &v));
                }
                op::glGetTexImage => {
                    let (mut tw, mut th) = (0i32, 1i32);
                    (f.glGetTexLevelParameteriv)(w(0), i(1), GL_TEXTURE_WIDTH, (&mut tw as *mut i32).cast());
                    if w(0) != GL_TEXTURE_1D {
                        (f.glGetTexLevelParameteriv)(w(0), i(1), GL_TEXTURE_HEIGHT, (&mut th as *mut i32).cast());
                    }
                    let buf = vec![0u8; image_bytes(f, GL_PACK_ROW_LENGTH, tw, th, w(2), w(3))];
                    (f.glGetTexImage)(w(0), i(1), w(2), w(3), buf.as_ptr().cast());
                    write(w(4), &buf);
                }
                // --- Pixels ---
                op::glDrawPixels => {
                    let v = fetch(w(4), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(0), i(1), w(2), w(3)));
                    (f.glDrawPixels)(i(0), i(1), w(2), w(3), ptr(w(4), &v));
                }
                op::glReadPixels => {
                    let buf = vec![0u8; image_bytes(f, GL_PACK_ROW_LENGTH, i(2), i(3), w(4), w(5))];
                    (f.glReadPixels)(i(0), i(1), i(2), i(3), w(4), w(5), buf.as_ptr().cast());
                    write(w(6), &buf);
                }
                op::glBitmap => {
                    let v = fetch(w(6), image_bytes(f, GL_UNPACK_ROW_LENGTH, i(0), i(1), 0x1900, 0x1A00));
                    (f.glBitmap)(i(0), i(1), fl(2), fl(3), fl(4), fl(5), ptr(w(6), &v));
                }
                // --- Listes ---
                op::glCallLists => {
                    let v = fetch(w(2), i(0).max(0) as usize * type_size(w(1)));
                    (f.glCallLists)(i(0), w(1), v.as_ptr().cast());
                }
                // --- Évaluateurs ---
                op::glMap1d | op::glMap1f => {
                    let dbl = op == op::glMap1d;
                    let (target, at) = (w(0), if dbl { 5 } else { 3 });
                    let (stride, order, p) = (i(at), i(at + 1), w(at + 2));
                    let es = if dbl { 8 } else { 4 };
                    let n = ((order.max(1) - 1) * stride.max(0)) as usize + map_components(target);
                    let v = fetch(p, n * es);
                    if dbl {
                        (f.glMap1d)(target, d(1), d(3), stride, order, v.as_ptr().cast());
                    } else {
                        (f.glMap1f)(target, fl(1), fl(2), stride, order, v.as_ptr().cast());
                    }
                }
                op::glMap2d | op::glMap2f => {
                    let target = w(0);
                    if op == op::glMap2d {
                        let (us, uo, vs, vo) = (i(5), i(6), i(11), i(12));
                        let n = ((uo.max(1) - 1) * us.max(0) + (vo.max(1) - 1) * vs.max(0)) as usize + map_components(target);
                        let v = fetch(w(13), n * 8);
                        (f.glMap2d)(target, d(1), d(3), us, uo, d(7), d(9), vs, vo, v.as_ptr().cast());
                    } else {
                        let (us, uo, vs, vo) = (i(3), i(4), i(7), i(8));
                        let n = ((uo.max(1) - 1) * us.max(0) + (vo.max(1) - 1) * vs.max(0)) as usize + map_components(target);
                        let v = fetch(w(9), n * 4);
                        (f.glMap2f)(target, fl(1), fl(2), us, uo, fl(5), fl(6), vs, vo, v.as_ptr().cast());
                    }
                }
                // --- Tableaux de sommets ---
                op::glEnableClientState | op::glDisableClientState => {
                    let on = op == op::glEnableClientState;
                    if let Some(k) = CAPS.iter().position(|&c| c == w(0)) {
                        self.client.enabled[k] = on;
                    }
                    if on { (f.glEnableClientState)(w(0)) } else { (f.glDisableClientState)(w(0)) }
                }
                op::glVertexPointer => self.client.arrays[VERTEX] = Array { size: i(0), ty: w(1), stride: i(2), ptr: w(3) },
                op::glColorPointer => self.client.arrays[COLOR] = Array { size: i(0), ty: w(1), stride: i(2), ptr: w(3) },
                op::glTexCoordPointer => self.client.arrays[TEXCOORD] = Array { size: i(0), ty: w(1), stride: i(2), ptr: w(3) },
                op::glNormalPointer => self.client.arrays[NORMAL] = Array { size: 3, ty: w(0), stride: i(1), ptr: w(2) },
                op::glIndexPointer => self.client.arrays[INDEX] = Array { size: 1, ty: w(0), stride: i(1), ptr: w(2) },
                op::glEdgeFlagPointer => self.client.arrays[EDGE] = Array { size: 1, ty: 0x1401, stride: i(0), ptr: w(1) },
                op::glInterleavedArrays => self.interleaved(f, w(0), i(1), w(2)),
                op::glPushClientAttrib => {
                    self.client_stack.push(self.client.clone());
                    (f.glPushClientAttrib)(w(0));
                }
                op::glPopClientAttrib => {
                    if let Some(c) = self.client_stack.pop() {
                        self.client = c;
                    }
                    (f.glPopClientAttrib)();
                }
                op::glDrawArrays => {
                    if i(2) > 0 {
                        self.bind_arrays(f, i(1).max(0) as usize, (i(1) + i(2) - 1) as usize, &mut fetch);
                        (f.glDrawArrays)(w(0), i(1), i(2));
                    }
                }
                op::glArrayElement => {
                    let k = i(0).max(0) as usize;
                    self.bind_arrays(f, k, k, &mut fetch);
                    (f.glArrayElement)(i(0));
                }
                op::glDrawElements => {
                    let n = i(1).max(0) as usize;
                    let isz = type_size(w(2));
                    let idx = fetch(w(3), n * isz);
                    let get = |k: usize| match isz {
                        1 => usize::from(idx[k]),
                        2 => usize::from(u16::from_le_bytes([idx[2 * k], idx[2 * k + 1]])),
                        _ => u32::from_le_bytes([idx[4 * k], idx[4 * k + 1], idx[4 * k + 2], idx[4 * k + 3]]) as usize,
                    };
                    if n > 0 {
                        let (lo, hi) = (0..n).map(get).fold((usize::MAX, 0), |(l, h), x| (l.min(x), h.max(x)));
                        self.bind_arrays(f, lo, hi, &mut fetch);
                        (f.glDrawElements)(w(0), i(1), w(2), idx.as_ptr().cast());
                    }
                }
                _ => {}
            }
        }
        self.scratch.clear();
        0
    }

    /// Copie les éléments `lo..=hi` des tableaux actifs et les donne à
    /// OpenGL (pointeurs décalés : l'élément `k` est lu à l'indice `k`).
    fn bind_arrays(&mut self, f: &Fns, lo: usize, hi: usize, fetch: &mut dyn FnMut(u32, usize) -> Vec<u8>) {
        // Plafond de sécurité : un indice aberrant ne fait pas tout copier.
        let hi = hi.min(lo + (1 << 20));
        self.scratch.clear();
        for k in 0..6 {
            let arr = self.client.arrays[k];
            if !self.client.enabled[k] || arr.ptr == 0 {
                continue;
            }
            let step = arr.step();
            let start = arr.ptr.wrapping_add((lo * step) as u32);
            let data = fetch(start, (hi - lo) * step + arr.elem());
            let p = data.as_ptr().wrapping_sub(lo * step).cast::<c_void>();
            let stride = step as i32;
            // SAFETY: OpenGL ne lit que les éléments lo..=hi, copiés dans `data`
            // (gardé dans `scratch` jusqu'à la fin du dessin).
            unsafe {
                match k {
                    VERTEX => (f.glVertexPointer)(arr.size, arr.ty, stride, p),
                    NORMAL => (f.glNormalPointer)(arr.ty, stride, p),
                    COLOR => (f.glColorPointer)(arr.size, arr.ty, stride, p),
                    INDEX => (f.glIndexPointer)(arr.ty, stride, p),
                    TEXCOORD => (f.glTexCoordPointer)(arr.size, arr.ty, stride, p),
                    _ => (f.glEdgeFlagPointer)(stride, p),
                }
            }
            self.scratch.push(data);
        }
    }

    /// glInterleavedArrays décomposé en tableaux séparés (spécification
    /// d'OpenGL 1.1, table 2.5).
    fn interleaved(&mut self, f: &Fns, format: u32, stride: i32, ptr: u32) {
        // (texcoord, couleur (taille, type), normale, sommet, décalages c, n, v)
        let (t, c, n, v, pc, pn, pv): (i32, (i32, u32), bool, i32, u32, u32, u32) = match format {
            0x2A20 => (0, (0, 0), false, 2, 0, 0, 0),
            0x2A21 => (0, (0, 0), false, 3, 0, 0, 0),
            0x2A22 => (0, (4, 0x1401), false, 2, 0, 0, 4),
            0x2A23 => (0, (4, 0x1401), false, 3, 0, 0, 4),
            0x2A24 => (0, (3, 0x1406), false, 3, 0, 0, 12),
            0x2A25 => (0, (0, 0), true, 3, 0, 0, 12),
            0x2A26 => (0, (4, 0x1406), true, 3, 0, 16, 28),
            0x2A27 => (2, (0, 0), false, 3, 0, 0, 8),
            0x2A28 => (4, (0, 0), false, 4, 0, 0, 16),
            0x2A29 => (2, (4, 0x1401), false, 3, 8, 0, 12),
            0x2A2A => (2, (3, 0x1406), false, 3, 8, 0, 20),
            0x2A2B => (2, (0, 0), true, 3, 0, 8, 20),
            0x2A2C => (2, (4, 0x1406), true, 3, 8, 24, 36),
            0x2A2D => (4, (4, 0x1406), true, 4, 16, 32, 44),
            _ => return,
        };
        let stride = if stride > 0 { stride } else { pv as i32 + v * 4 };
        let mut set = |k: usize, on: bool, a: Array| {
            self.client.enabled[k] = on;
            if on {
                self.client.arrays[k] = a;
            }
            // SAFETY: énumérations valides.
            unsafe {
                if on { (f.glEnableClientState)(CAPS[k]) } else { (f.glDisableClientState)(CAPS[k]) }
            }
        };
        set(EDGE, false, Array::default());
        set(INDEX, false, Array::default());
        set(TEXCOORD, t > 0, Array { size: t, ty: 0x1406, stride, ptr });
        set(COLOR, c.0 > 0, Array { size: c.0, ty: c.1, stride, ptr: ptr.wrapping_add(pc) });
        set(NORMAL, n, Array { size: 3, ty: 0x1406, stride, ptr: ptr.wrapping_add(pn) });
        set(VERTEX, true, Array { size: v, ty: 0x1406, stride, ptr: ptr.wrapping_add(pv) });
    }
}

/// Composantes d'un point de contrôle d'un évaluateur (GL_MAP1_* et
/// GL_MAP2_* dans le même ordre).
fn map_components(target: u32) -> usize {
    match target & 0xF {
        0 | 6 | 8 => 4,
        1 | 3 => 1,
        4 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_counts_written_values() {
        let v = sentinel_get(4, 16, &mut |p| {
            // SAFETY: tampon de 16 valeurs.
            unsafe {
                let p = p.cast::<u32>();
                *p = 0xA5A5_A5A5;
                *p.add(2) = 7;
            }
        });
        assert_eq!(v.len(), 12);
        assert_eq!(&v[..4], &0xA5A5_A5A5u32.to_le_bytes());
    }

    /// Tampon de commandes tel que l'écrit la DLL.
    struct Cmds(Vec<u32>);
    impl Cmds {
        fn op(&mut self, op: u16, args: &[u32]) -> &mut Self {
            self.0.push(u32::from(op) << 16 | args.len() as u32);
            self.0.extend_from_slice(args);
            self
        }
    }
    fn fb(x: f32) -> u32 {
        x.to_bits()
    }

    #[test]
    fn draw_swap_get_and_arrays() {
        let mut gl = OpenGl::new();
        let mut mem = vec![0u8; 0x1000];
        // Tableau de sommets à 0x100 : un triangle couvrant l'écran.
        for (k, v) in [-1.0f32, -1.0, 3.0, -1.0, -1.0, 3.0].iter().enumerate() {
            mem[0x100 + 4 * k..0x104 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        let mem = std::cell::RefCell::new(mem);
        let run = |gl: &mut OpenGl, c: &Cmds| {
            let mut read = |at: u32, buf: &mut [u8]| buf.copy_from_slice(&mem.borrow()[at as usize..at as usize + buf.len()]);
            let mut write = |at: u32, data: &[u8]| mem.borrow_mut()[at as usize..at as usize + data.len()].copy_from_slice(data);
            gl.execute(&c.0, (64, 48), &mut read, &mut write)
        };
        if run(&mut gl, Cmds(Vec::new()).op(MAKE_CURRENT, &[1])) == 0 {
            eprintln!("no offscreen OpenGL: test skipped");
            return;
        }
        let mut c = Cmds(Vec::new());
        c.op(op::glClearColor, &[fb(0.0), fb(0.0), fb(1.0), fb(1.0)])
            .op(op::glClear, &[0x4000])
            .op(op::glColor3f, &[fb(1.0), fb(0.0), fb(0.0)])
            .op(op::glBegin, &[4])
            .op(op::glVertex2f, &[fb(-1.0), fb(-1.0)])
            .op(op::glVertex2f, &[fb(1.0), fb(-1.0)])
            .op(op::glVertex2f, &[fb(-1.0), fb(1.0)])
            .op(op::glEnd, &[])
            .op(SWAP, &[0]);
        run(&mut gl, &c);
        let (w, h, px) = gl.frame().unwrap();
        assert_eq!((w, h), (64, 48));
        // Triangle rouge en bas à gauche, fond bleu en haut à droite.
        assert_eq!(px[(h - 2) * w + 1], 0xFF0000, "{:06X}", px[(h - 2) * w + 1]);
        assert_eq!(px[w + w - 2], 0x0000FF, "{:06X}", px[w + w - 2]);
        // glGetIntegerv(GL_VIEWPORT) : quatre entiers écrits à 0x200.
        run(&mut gl, Cmds(Vec::new()).op(op::glGetIntegerv, &[0x0BA2, 0x200]));
        let vp: Vec<i32> = mem.borrow()[0x200..0x214].chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(vp, [0, 0, 64, 48, 0]);
        assert_eq!(run(&mut gl, Cmds(Vec::new()).op(op::glGetString, &[0x1F02, 0x300, 64])), 1);
        assert_eq!(&mem.borrow()[0x300..0x306], b"1.1.0\0");
        // glDrawArrays sur le tableau de l'invité, en vert, dans le tampon arrière.
        let mut c = Cmds(Vec::new());
        c.op(op::glColor3f, &[fb(0.0), fb(1.0), fb(0.0)])
            .op(op::glEnableClientState, &[0x8074])
            .op(op::glVertexPointer, &[2, 0x1406, 0, 0x100])
            .op(op::glDrawArrays, &[4, 0, 3])
            .op(SWAP, &[0]);
        run(&mut gl, &c);
        let (_, _, px) = gl.frame().unwrap();
        assert!(px.iter().all(|&p| p == 0x00FF00), "{:06X}", px[0]);
        assert_eq!(run(&mut gl, Cmds(Vec::new()).op(op::glGetError, &[])), 0);
    }

    #[test]
    fn interleaved_layout() {
        assert_eq!(pixel_layout(0x1908, 0x1401), (4, 1));
        assert_eq!(pixel_layout(0x80E1, 0x8367), (1, 4));
        assert_eq!(map_components(0x0D97), 3);
        assert_eq!(map_components(0x0DB0), 4);
    }
}
