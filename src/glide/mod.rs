// SPDX-License-Identifier: GPL-3.0-or-later
//! Glide 2 (3dfx) servi par l'hôte : la `glide2x.dll` de Bare86 (guest/glide2x)
//! transmet les appels du jeu par un port réservé et un tampon de commandes
//! (`proto`) ; l'hôte les rejoue en OpenGL hors écran (`gl`), et l'image
//! remplace celle de la VGA tant que Glide est ouvert, comme le relais VGA
//! d'une Voodoo.

pub mod gl;
pub mod proto;
mod shader;
pub mod texmem;

use glow::HasContext;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Résolutions de `grSstWinOpen` (GR_RESOLUTION_*).
fn resolution(code: u32) -> (usize, usize) {
    match code {
        0x0 => (320, 200),
        0x1 => (320, 240),
        0x2 => (400, 256),
        0x3 => (512, 384),
        0x4 => (640, 200),
        0x5 => (640, 350),
        0x6 => (640, 400),
        0x8 => (800, 600),
        0x9 => (960, 720),
        0xA => (856, 480),
        0xB => (512, 256),
        0xC => (1024, 768),
        0xD => (1280, 1024),
        0xE => (1600, 1200),
        0xF => (400, 300),
        _ => (640, 480),
    }
}

/// Couleur Glide (format choisi à l'ouverture) en r, g, b, a de 0 à 1.
fn color(format: u32, c: u32) -> [f32; 4] {
    let b = |s: u32| ((c >> s) & 0xFF) as f32 / 255.0;
    match format {
        // ABGR
        1 => [b(0), b(8), b(16), b(24)],
        // RGBA
        2 => [b(24), b(16), b(8), b(0)],
        // BGRA
        3 => [b(8), b(16), b(24), b(0)],
        // ARGB
        _ => [b(16), b(8), b(0), b(24)],
    }
}

/// État de rendu (sauvé et rétabli par grGlideGetState/SetState).
#[derive(Clone, Copy, Debug)]
struct State {
    color_format: u32,
    origin_lower: bool,
    clip: [u32; 4],
    cc: [i32; 5],
    ac: [i32; 5],
    tmu_rgb: [i32; 3],
    tmu_a: [i32; 3],
    blend: [u32; 4],
    alpha_test: u32,
    alpha_ref: u32,
    constant: u32,
    cull: u32,
    depth_bias: i32,
    depth_func: u32,
    depth_mode: u32,
    depth_mask: bool,
    color_mask: [bool; 2],
    fog_color: u32,
    fog_mode: u32,
    fog_table: [u8; 64],
    clamp: [u32; 2],
    filter: [u32; 2],
    mipmap: u32,
    source: Option<TexKey>,
    render_buffer: u32,
}

impl State {
    fn new(width: usize, height: usize) -> Self {
        Self {
            color_format: 0,
            origin_lower: false,
            clip: [0, 0, width as u32, height as u32],
            // Glide au démarrage : couleur itérée, alpha itéré, texel tel quel.
            cc: [1, 0, 0, 0, 0],
            ac: [1, 0, 0, 0, 0],
            tmu_rgb: [1, 0, 0],
            tmu_a: [1, 0, 0],
            blend: [4, 0, 4, 0],
            alpha_test: 7,
            alpha_ref: 0,
            constant: 0xFFFF_FFFF,
            cull: 0,
            depth_bias: 0,
            depth_func: 1,
            depth_mode: 0,
            depth_mask: false,
            color_mask: [true, true],
            fog_color: 0,
            fog_mode: 0,
            fog_table: [0; 64],
            clamp: [0, 0],
            filter: [1, 1],
            mipmap: 0,
            source: None,
            render_buffer: 1,
        }
    }
}

/// Texture choisie par `grTexSource` : adresse et description.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TexKey {
    start: u32,
    even_odd: u32,
    small: u32,
    large: u32,
    aspect: u32,
    format: u32,
}

struct Texture {
    tex: glow::Texture,
    /// Génération des pages de mémoire couvertes (et de la palette) au
    /// moment du décodage.
    stamp: u64,
}

/// Lot de primitives en attente (un appel de dessin par changement d'état).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prim {
    Triangles,
    Lines,
    Points,
}

/// Flottants par sommet dans le tampon de sommets : x, y, r, g, b, a, s, t, q, ooz, oow.
const FLOATS: usize = 11;

struct Gpu {
    off: gl::Offscreen,
    prog: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    blit: glow::Program,
    blit_vao: glow::VertexArray,
    blit_tex: glow::Texture,
    uni: HashMap<&'static str, glow::UniformLocation>,
}

impl Gpu {
    fn new(width: usize, height: usize) -> Result<Self, String> {
        let off = gl::Offscreen::new(width, height)?;
        let g = &off.gl;
        // SAFETY: contexte courant ; sources GLSL valides (vérifiées à la compilation).
        unsafe {
            let compile = |vs: &str, fs: &str| -> Result<glow::Program, String> {
                let p = g.create_program()?;
                for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
                    let s = g.create_shader(kind)?;
                    g.shader_source(s, src);
                    g.compile_shader(s);
                    if !g.get_shader_compile_status(s) {
                        return Err(format!("Glide shader: {}", g.get_shader_info_log(s)));
                    }
                    g.attach_shader(p, s);
                }
                g.link_program(p);
                if !g.get_program_link_status(p) {
                    return Err(format!("Glide program: {}", g.get_program_info_log(p)));
                }
                Ok(p)
            };
            let prog = compile(shader::VERTEX, shader::FRAGMENT)?;
            let blit = compile(shader::BLIT_VERTEX, shader::BLIT_FRAGMENT)?;
            let vao = g.create_vertex_array()?;
            let vbo = g.create_buffer()?;
            g.bind_vertex_array(Some(vao));
            g.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let stride = (FLOATS * 4) as i32;
            for (loc, size, at) in [(0, 2, 0), (1, 4, 2), (2, 3, 6), (3, 2, 9)] {
                g.enable_vertex_attrib_array(loc);
                g.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, at * 4);
            }
            let blit_vao = g.create_vertex_array()?;
            let blit_vbo = g.create_buffer()?;
            g.bind_vertex_array(Some(blit_vao));
            g.bind_buffer(glow::ARRAY_BUFFER, Some(blit_vbo));
            let quad: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
            g.buffer_data_u8_slice(glow::ARRAY_BUFFER, &quad.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>(), glow::STATIC_DRAW);
            g.enable_vertex_attrib_array(0);
            g.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            let blit_tex = g.create_texture()?;
            g.bind_texture(glow::TEXTURE_2D, Some(blit_tex));
            g.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            g.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            let mut uni = HashMap::new();
            for name in [
                "screen", "origin_lower", "tex0", "tex_on", "tex_scale", "tmu_rgb", "tmu_a", "cc", "ac", "inv", "constant", "at_func",
                "at_ref", "fog_mode", "fog_color", "fog_table", "depth_mode", "depth_bias",
            ] {
                if let Some(l) = g.get_uniform_location(prog, name) {
                    uni.insert(name, l);
                }
            }
            Ok(Self { off, prog, vao, vbo, blit, blit_vao, blit_tex, uni })
        }
    }
}

pub struct Glide {
    gpu: Option<Gpu>,
    /// Erreur d'initialisation d'OpenGL (rapportée une fois).
    failed: Option<String>,
    st: State,
    saved: HashMap<u32, State>,
    tmu: Vec<u8>,
    /// Génération de chaque page de 4 Ko de la mémoire de texture.
    page_gen: Vec<u64>,
    generation: u64,
    palette: [u32; 256],
    palette_gen: u64,
    textures: HashMap<TexKey, Texture>,
    /// Glide ouvert : son image remplace celle de la VGA.
    pub active: bool,
    width: usize,
    height: usize,
    /// Tampon affiché (avant) ; l'autre reçoit le dessin.
    front: usize,
    frame: Vec<u32>,
    frame_dirty: bool,
    last_swap: Option<Instant>,
    batch: Vec<f32>,
    batch_prim: Prim,
    /// Table de correction gamma de la sortie vidéo (la Voodoo Graphics la
    /// règle à 1,7 à l'ouverture), appliquée à l'image affichée seulement.
    gamma: [u8; 256],
    /// État du dernier lot envoyé à OpenGL (évite de tout renvoyer).
    applied: bool,
    pub swaps: u64,
}

impl Default for Glide {
    fn default() -> Self {
        Self::new()
    }
}

impl Glide {
    pub fn new() -> Self {
        Self {
            gpu: None,
            failed: None,
            st: State::new(640, 480),
            saved: HashMap::new(),
            tmu: vec![0; texmem::TMU_RAM],
            page_gen: vec![0; texmem::TMU_RAM >> 12],
            generation: 1,
            palette: [0; 256],
            palette_gen: 0,
            textures: HashMap::new(),
            active: false,
            width: 640,
            height: 480,
            front: 0,
            frame: Vec::new(),
            frame_dirty: false,
            last_swap: None,
            batch: Vec::new(),
            batch_prim: Prim::Triangles,
            gamma: gamma_table(1.7),
            applied: false,
            swaps: 0,
        }
    }

    /// Image à afficher (Glide ouvert) : relue après un échange ou un dessin
    /// dans le tampon avant.
    pub fn frame(&mut self) -> Option<(usize, usize, Vec<u32>)> {
        if !self.active {
            return None;
        }
        if self.frame_dirty || self.frame.len() != self.width * self.height {
            self.flush();
            let front = self.front;
            if let Some(g) = &mut self.gpu {
                g.off.read(front, &mut self.frame);
            }
            let t = &self.gamma;
            for p in &mut self.frame {
                let c = |s: u32| u32::from(t[(*p >> s & 0xFF) as usize]) << s;
                *p = c(16) | c(8) | c(0);
            }
            self.frame_dirty = false;
        }
        (self.frame.len() == self.width * self.height).then(|| (self.width, self.height, self.frame.clone()))
    }

    /// Exécute un tampon de commandes ; `read` lit la mémoire de l'invité à
    /// une adresse linéaire, `write` y écrit. Rend la valeur de la dernière
    /// commande.
    pub fn execute(&mut self, words: &[u32], read: &mut dyn FnMut(u32, &mut [u8]), write: &mut dyn FnMut(u32, &[u8])) -> u32 {
        let mut i = 0;
        let mut ret = 0;
        while i < words.len() {
            let op = (words[i] >> 16) as u16;
            let n = (words[i] & 0xFFFF) as usize;
            let Some(args) = words.get(i + 1..i + 1 + n) else { break };
            ret = self.command(op, args, read, write);
            i += 1 + n;
        }
        ret
    }

    fn command(&mut self, op: u16, a: &[u32], read: &mut dyn FnMut(u32, &mut [u8]), write: &mut dyn FnMut(u32, &[u8])) -> u32 {
        use proto::*;
        let arg = |k: usize| a.get(k).copied().unwrap_or(0);
        let f = |k: usize| f32::from_bits(a.get(k).copied().unwrap_or(0));
        // Toute commande autre qu'un dessin change l'état : le lot en cours part d'abord.
        if !matches!(op, TRIANGLE | LINE | POINT) {
            self.flush();
            self.applied = false;
        }
        match op {
            INIT => {
                if self.gpu.is_none() && self.failed.is_none() {
                    match Gpu::new(self.width, self.height) {
                        Ok(g) => self.gpu = Some(g),
                        Err(e) => {
                            eprintln!("[glide] {e}");
                            self.failed = Some(e);
                        }
                    }
                }
                return u32::from(self.gpu.is_some());
            }
            WIN_OPEN => {
                let (w, h) = resolution(arg(0));
                self.width = w;
                self.height = h;
                self.st = State::new(w, h);
                self.st.origin_lower = arg(1) == 1;
                self.st.color_format = arg(2);
                self.front = 0;
                if let Some(g) = &mut self.gpu {
                    g.off.resize(w, h);
                    g.off.attach(1);
                }
                self.active = true;
                self.frame_dirty = true;
                self.gamma = gamma_table(1.7);
                return u32::from(self.gpu.is_some());
            }
            WIN_CLOSE | SHUTDOWN => {
                self.active = false;
                self.frame.clear();
            }
            SWAP => {
                // Intervalle de balayage : au plus une image par 1/60 s et par unité.
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
                self.front ^= 1;
                let back = self.front ^ 1;
                if let Some(g) = &mut self.gpu {
                    g.off.attach(if self.st.render_buffer == 0 { self.front } else { back });
                }
                self.frame_dirty = true;
                self.swaps += 1;
            }
            CLEAR => self.clear(arg(0), arg(1), arg(2)),
            RENDER_BUFFER => {
                self.st.render_buffer = arg(0);
                let target = if arg(0) == 0 { self.front } else { self.front ^ 1 };
                if let Some(g) = &mut self.gpu {
                    g.off.attach(target);
                }
            }
            ORIGIN => self.st.origin_lower = arg(0) == 1,
            CLIP => self.st.clip = [arg(0), arg(1), arg(2), arg(3)],
            COLOR_COMBINE => self.st.cc = [arg(0) as i32, arg(1) as i32, arg(2) as i32, arg(3) as i32, arg(4) as i32],
            ALPHA_COMBINE => self.st.ac = [arg(0) as i32, arg(1) as i32, arg(2) as i32, arg(3) as i32, arg(4) as i32],
            TEX_COMBINE => {
                if arg(0) == 0 {
                    self.st.tmu_rgb = [arg(1) as i32, arg(2) as i32, arg(5) as i32];
                    self.st.tmu_a = [arg(3) as i32, arg(4) as i32, arg(6) as i32];
                }
            }
            BLEND => self.st.blend = [arg(0), arg(1), arg(2), arg(3)],
            ALPHA_TEST_FUNC => self.st.alpha_test = arg(0),
            ALPHA_TEST_REF => self.st.alpha_ref = arg(0) & 0xFF,
            CONSTANT_COLOR => self.st.constant = arg(0),
            CULL => self.st.cull = arg(0),
            DEPTH_BIAS => self.st.depth_bias = arg(0) as i16 as i32,
            DEPTH_FUNC => self.st.depth_func = arg(0),
            DEPTH_MODE => self.st.depth_mode = arg(0),
            DEPTH_MASK => self.st.depth_mask = arg(0) != 0,
            COLOR_MASK => self.st.color_mask = [arg(0) != 0, arg(1) != 0],
            FOG_COLOR => self.st.fog_color = arg(0),
            FOG_MODE => self.st.fog_mode = arg(0),
            FOG_TABLE => {
                for (k, w) in a.iter().take(16).enumerate() {
                    self.st.fog_table[k * 4..k * 4 + 4].copy_from_slice(&w.to_le_bytes());
                }
            }
            TEX_CLAMP => {
                if arg(0) == 0 {
                    self.st.clamp = [arg(1), arg(2)];
                }
            }
            TEX_FILTER => {
                if arg(0) == 0 {
                    self.st.filter = [arg(1), arg(2)];
                }
            }
            TEX_MIPMAP => {
                if arg(0) == 0 {
                    self.st.mipmap = arg(1);
                }
            }
            TEX_SOURCE => {
                if arg(0) == 0 {
                    self.st.source = Some(TexKey { start: arg(1), even_odd: arg(2), small: arg(3), large: arg(4), aspect: arg(5), format: arg(6) });
                }
            }
            TEX_DOWNLOAD => {
                let key = TexKey { start: arg(1), even_odd: arg(2), small: arg(3), large: arg(4), aspect: arg(5), format: arg(6) };
                let bytes = arg(7) as usize;
                let data: Vec<u8> = a.get(8..).unwrap_or(&[]).iter().flat_map(|w| w.to_le_bytes()).take(bytes).collect();
                self.download(key, &data);
            }
            TEX_DOWNLOAD_PARTIAL => {
                // adresse, ce LOD, grand LOD, rapport, format, pair/impair, lignes
                let (start, lod, large, aspect, format, even_odd) = (arg(1), arg(2), arg(3), arg(4), arg(5), arg(6));
                let (first, last) = (arg(7) as usize, arg(8) as usize);
                let bytes = arg(9) as usize;
                let data: Vec<u8> = a.get(10..).unwrap_or(&[]).iter().flat_map(|w| w.to_le_bytes()).take(bytes).collect();
                if let Some(addr) = texmem::level_address(start, large, lod, aspect, format, even_odd) {
                    let (w, _) = texmem::level_size(lod, aspect);
                    let row = w * texmem::bytes_per_texel(format);
                    self.store(addr as usize + first * row, &data[..data.len().min((last + 1 - first) * row)]);
                }
            }
            TEX_TABLE => {
                // Palette : 256 mots 0x00RRGGBB. Tables NCC : ignorées (format YIQ non rendu).
                if arg(0) == 2 {
                    for (k, w) in a.iter().skip(1).take(256).enumerate() {
                        self.palette[k] = *w;
                    }
                    self.generation += 1;
                    self.palette_gen = self.generation;
                }
            }
            TRIANGLE | LINE | POINT => {
                let prim = match op {
                    TRIANGLE => Prim::Triangles,
                    LINE => Prim::Lines,
                    _ => Prim::Points,
                };
                let verts: Vec<&[u32]> = a.chunks_exact(VERTEX_WORDS).collect();
                if prim == Prim::Triangles && verts.len() == 3 && self.culled(&verts) {
                    return 0;
                }
                if self.batch_prim != prim {
                    self.flush();
                    self.batch_prim = prim;
                }
                for v in verts {
                    let g = |k: usize| f32::from_bits(v[k]);
                    // La Voodoo ne garde de x et y que 12 bits entiers (virgule fixe
                    // 12.4) : les programmes ajoutent 3·2^18 pour forcer l'arrondi.
                    let snap = |c: f32| c - (c / 4096.0).floor() * 4096.0;
                    // x, y, r, g, b, a, s, t, q, ooz, oow
                    self.batch.extend_from_slice(&[snap(g(0)), snap(g(1)), g(4), g(5), g(6), g(7), g(8), g(9), g(10), g(2), g(3)]);
                }
            }
            SAVE_STATE => {
                if self.saved.len() > 256 {
                    self.saved.clear();
                }
                self.saved.insert(arg(0), self.st);
            }
            RESTORE_STATE => {
                if let Some(s) = self.saved.get(&arg(0)) {
                    self.st = *s;
                    let target = if self.st.render_buffer == 0 { self.front } else { self.front ^ 1 };
                    if let Some(g) = &mut self.gpu {
                        g.off.attach(target);
                    }
                }
            }
            LFB_READ => self.lfb_read(arg(0), arg(1) == 1, arg(2), arg(3) as usize, write),
            LFB_WRITE => self.lfb_write(arg(0), arg(1), arg(2) == 1, arg(3), arg(4) as usize, read),
            GAMMA => {
                self.gamma = gamma_table(f(0));
                self.frame_dirty = true;
            }
            FOG_GENERATE => {
                let table = fog_table(arg(0), f(1), f(2));
                write(arg(3), &table);
            }
            _ => {}
        }
        0
    }

    /// Le triangle est-il éliminé (faces arrière) ? Règle de la Glide de
    /// 3dfx : aire A = (ax−bx)(by−cy) − (bx−cx)(ay−by) ; « negative » élimine
    /// A < 0, « positive » A > 0 ; une aire nulle n'est jamais dessinée.
    fn culled(&self, v: &[&[u32]]) -> bool {
        let x = |i: usize| f32::from_bits(v[i][0]);
        let y = |i: usize| f32::from_bits(v[i][1]);
        let area = (x(0) - x(1)) * (y(1) - y(2)) - (x(1) - x(2)) * (y(0) - y(1));
        if area == 0.0 {
            return true;
        }
        match self.st.cull {
            1 => area < 0.0,
            2 => area > 0.0,
            _ => false,
        }
    }

    fn store(&mut self, at: usize, data: &[u8]) {
        let end = (at + data.len()).min(self.tmu.len());
        if at >= end {
            return;
        }
        self.tmu[at..end].copy_from_slice(&data[..end - at]);
        self.generation += 1;
        for p in at >> 12..=(end - 1) >> 12 {
            self.page_gen[p] = self.generation;
        }
    }

    /// `grTexDownloadMipMap` : niveaux contigus dans `data` (tailles réelles),
    /// rangés aux adresses de la Voodoo.
    fn download(&mut self, k: TexKey, data: &[u8]) {
        let bpp = texmem::bytes_per_texel(k.format);
        let mut at = 0;
        for lod in k.large..=k.small.min(8) {
            let (w, h) = texmem::level_size(lod, k.aspect);
            let size = w * h * bpp;
            let Some(level) = data.get(at..at + size) else { break };
            at += size;
            if let Some(addr) = texmem::level_address(k.start, k.large, lod, k.aspect, k.format, k.even_odd) {
                self.store(addr as usize, &level.to_vec());
            }
        }
    }

    /// Génération la plus récente des pages couvertes par une texture.
    fn stamp(&self, k: &TexKey) -> u64 {
        let size = texmem::mem_required(k.small, k.large, k.aspect, k.format, k.even_odd) as usize;
        let start = k.start as usize;
        let last = ((start + size.max(1) - 1) >> 12).min(self.page_gen.len() - 1);
        let pages = (start >> 12).min(last)..=last;
        let mem = pages.map(|p| self.page_gen[p]).max().unwrap_or(0);
        if matches!(k.format, 5 | 14) { mem.max(self.palette_gen) } else { mem }
    }

    /// Texture OpenGL de la source courante, décodée si besoin.
    fn texture(&mut self, k: TexKey) -> Option<glow::Texture> {
        let stamp = self.stamp(&k);
        if let Some(t) = self.textures.get(&k)
            && t.stamp == stamp
        {
            return Some(t.tex);
        }
        let g = self.gpu.as_ref()?;
        let gl = &g.off.gl;
        // SAFETY: contexte courant ; tampons de la taille des niveaux envoyés.
        unsafe {
            let tex = match self.textures.remove(&k) {
                Some(t) => t.tex,
                None => gl.create_texture().ok()?,
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            let mut level = 0;
            for lod in k.large..=k.small.min(8) {
                let (w, h) = texmem::level_size(lod, k.aspect);
                let addr = texmem::level_address(k.start, k.large, lod, k.aspect, k.format, k.even_odd);
                let px = match addr {
                    Some(a) => texmem::decode_level(&self.tmu, a, w, h, k.format, &self.palette),
                    // Niveau rangé sur l'autre TMU : à défaut, gris.
                    None => vec![0xFF80_8080; w * h],
                };
                let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    level,
                    glow::RGBA8 as i32,
                    w as i32,
                    h as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&bytes)),
                );
                level += 1;
            }
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_BASE_LEVEL, 0);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAX_LEVEL, level - 1);
            self.textures.insert(k, Texture { tex, stamp });
            Some(tex)
        }
    }

    /// Envoie l'état courant à OpenGL (uniformes, mélange, profondeur…).
    fn apply(&mut self) {
        let st = self.st;
        let tex = st.source.and_then(|k| self.texture(k));
        let (w, h) = (self.width, self.height);
        let Some(g) = &self.gpu else { return };
        let gl = &g.off.gl;
        let u = |n: &str| g.uni.get(n);
        // SAFETY: contexte courant, programme et objets créés par lui.
        unsafe {
            gl.use_program(Some(g.prog));
            gl.bind_vertex_array(Some(g.vao));
            gl.uniform_2_f32(u("screen"), w as f32, h as f32);
            gl.uniform_1_i32(u("origin_lower"), i32::from(st.origin_lower));
            gl.uniform_1_i32(u("tex0"), 0);
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_1_i32(u("tex_on"), i32::from(tex.is_some()));
            if let (Some(t), Some(k)) = (tex, st.source) {
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                let (tw, th) = texmem::level_size(k.large, k.aspect);
                let big = tw.max(th) as f32;
                // s et t vont de 0 à 256 sur le grand côté (proportionnellement sur l'autre).
                gl.uniform_2_f32(u("tex_scale"), 1.0 / (256.0 * tw as f32 / big), 1.0 / (256.0 * th as f32 / big));
                let mip = st.mipmap != 0 && k.small > k.large;
                let min = match (st.filter[0] == 1, mip) {
                    (false, false) => glow::NEAREST,
                    (true, false) => glow::LINEAR,
                    (false, true) => glow::NEAREST_MIPMAP_NEAREST,
                    (true, true) => glow::LINEAR_MIPMAP_NEAREST,
                };
                let mag = if st.filter[1] == 1 { glow::LINEAR } else { glow::NEAREST };
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, min as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, mag as i32);
                let wrap = |c: u32| if c == 1 { glow::CLAMP_TO_EDGE } else { glow::REPEAT } as i32;
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap(st.clamp[0]));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap(st.clamp[1]));
            }
            gl.uniform_3_i32(u("tmu_rgb"), st.tmu_rgb[0], st.tmu_rgb[1], st.tmu_rgb[2]);
            gl.uniform_3_i32(u("tmu_a"), st.tmu_a[0], st.tmu_a[1], st.tmu_a[2]);
            gl.uniform_4_i32(u("cc"), st.cc[0], st.cc[1], st.cc[2], st.cc[3]);
            gl.uniform_4_i32(u("ac"), st.ac[0], st.ac[1], st.ac[2], st.ac[3]);
            gl.uniform_2_i32(u("inv"), st.cc[4], st.ac[4]);
            let c = color(st.color_format, st.constant);
            gl.uniform_4_f32(u("constant"), c[0], c[1], c[2], c[3]);
            gl.uniform_1_i32(u("at_func"), st.alpha_test as i32);
            gl.uniform_1_f32(u("at_ref"), st.alpha_ref as f32);
            gl.uniform_1_i32(u("fog_mode"), st.fog_mode as i32);
            let fc = color(st.color_format, st.fog_color);
            gl.uniform_3_f32(u("fog_color"), fc[0], fc[1], fc[2]);
            let table: Vec<f32> = st.fog_table.iter().map(|&b| f32::from(b)).collect();
            gl.uniform_1_f32_slice(u("fog_table"), &table);
            gl.uniform_1_i32(u("depth_mode"), st.depth_mode as i32 & 3);
            gl.uniform_1_f32(u("depth_bias"), st.depth_bias as f32 / 65535.0);
            // Profondeur : test seulement avec un mode de tampon de profondeur.
            if st.depth_mode != 0 {
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::NEVER + st.depth_func.min(7));
            } else {
                gl.disable(glow::DEPTH_TEST);
            }
            gl.depth_mask(st.depth_mask);
            gl.color_mask(st.color_mask[0], st.color_mask[0], st.color_mask[0], st.color_mask[1]);
            let src = |b: u32| match b {
                0 => glow::ZERO,
                1 => glow::SRC_ALPHA,
                2 => glow::DST_COLOR,
                3 => glow::DST_ALPHA,
                5 => glow::ONE_MINUS_SRC_ALPHA,
                6 => glow::ONE_MINUS_DST_COLOR,
                7 => glow::ONE_MINUS_DST_ALPHA,
                15 => glow::SRC_ALPHA_SATURATE,
                _ => glow::ONE,
            };
            let dst = |b: u32| match b {
                0 => glow::ZERO,
                1 => glow::SRC_ALPHA,
                2 => glow::SRC_COLOR,
                3 => glow::DST_ALPHA,
                5 => glow::ONE_MINUS_SRC_ALPHA,
                6 => glow::ONE_MINUS_SRC_COLOR,
                7 => glow::ONE_MINUS_DST_ALPHA,
                _ => glow::ONE,
            };
            let [rs, rd, as_, ad] = st.blend;
            if (rs, rd, as_, ad) == (4, 0, 4, 0) {
                gl.disable(glow::BLEND);
            } else {
                gl.enable(glow::BLEND);
                gl.blend_func_separate(src(rs), dst(rd), src(as_), dst(ad));
            }
            self.scissor_on(gl);
        }
        self.applied = true;
    }

    /// Fenêtre de découpe (`grClipWindow`), en coordonnées d'OpenGL.
    fn scissor_on(&self, gl: &glow::Context) {
        let [x0, y0, x1, y1] = self.st.clip;
        let (x1, y1) = (x1.min(self.width as u32), y1.min(self.height as u32));
        let (x0, y0) = (x0.min(x1), y0.min(y1));
        let y = if self.st.origin_lower { y0 } else { self.height as u32 - y1 };
        // SAFETY: contexte courant.
        unsafe {
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(x0 as i32, y as i32, (x1 - x0) as i32, (y1 - y0) as i32);
        }
    }

    /// Dessine le lot en attente.
    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        if !self.applied {
            self.apply();
        }
        let prim = self.batch_prim;
        let batch = std::mem::take(&mut self.batch);
        let Some(g) = &self.gpu else { return };
        let gl = &g.off.gl;
        // SAFETY: contexte courant ; tampon de sommets lié au VAO du programme.
        unsafe {
            gl.bind_vertex_array(Some(g.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(g.vbo));
            let bytes: &[u8] = std::slice::from_raw_parts(batch.as_ptr().cast(), batch.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
            let mode = match prim {
                Prim::Triangles => glow::TRIANGLES,
                Prim::Lines => glow::LINES,
                Prim::Points => glow::POINTS,
            };
            gl.draw_arrays(mode, 0, (batch.len() / FLOATS) as i32);
        }
        if self.st.render_buffer == 0 {
            self.frame_dirty = true;
        }
        self.batch = batch;
        self.batch.clear();
    }

    /// `grBufferClear` : couleur, alpha et profondeur, dans la fenêtre de
    /// découpe et selon les masques d'écriture.
    fn clear(&mut self, c: u32, alpha: u32, depth: u32) {
        let st = self.st;
        let Some(g) = &self.gpu else { return };
        let gl = &g.off.gl;
        let rgb = color(st.color_format, c);
        // SAFETY: contexte courant.
        unsafe {
            self.scissor_on(gl);
            gl.color_mask(st.color_mask[0], st.color_mask[0], st.color_mask[0], st.color_mask[1]);
            gl.depth_mask(st.depth_mask);
            gl.clear_color(rgb[0], rgb[1], rgb[2], (alpha & 0xFF) as f32 / 255.0);
            gl.clear_depth_f64(f64::from(depth & 0xFFFF) / 65535.0);
            let mut mask = glow::COLOR_BUFFER_BIT;
            if st.depth_mode != 0 && st.depth_mask {
                mask |= glow::DEPTH_BUFFER_BIT;
            }
            gl.clear(mask);
        }
        if st.render_buffer == 0 {
            self.frame_dirty = true;
        }
    }

    /// Tampon (0 avant, 1 arrière) → index du tampon de couleur.
    fn buffer_index(&self, buffer: u32) -> usize {
        if buffer == 0 { self.front } else { self.front ^ 1 }
    }

    /// `grLfbLock` en lecture : l'image du tampon en RGB 565, ligne par
    /// ligne au pas `stride`, écrite chez l'invité à `addr`.
    fn lfb_read(&mut self, buffer: u32, lower: bool, addr: u32, stride: usize, write: &mut dyn FnMut(u32, &[u8])) {
        let idx = self.buffer_index(buffer);
        let (w, h) = (self.width, self.height);
        let Some(g) = &mut self.gpu else { return };
        let rgba = g.off.read_rgba(idx);
        let mut line = vec![0u8; w * 2];
        for y in 0..h {
            // OpenGL range les lignes de bas en haut.
            let src = if lower { y } else { h - 1 - y };
            for x in 0..w {
                let p = &rgba[(src * w + x) * 4..];
                let v = (u16::from(p[0] >> 3) << 11) | (u16::from(p[1] >> 2) << 5) | u16::from(p[2] >> 3);
                line[x * 2..x * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            write(addr + (y * stride) as u32, &line);
        }
    }

    /// `grLfbUnlock` après une écriture : l'invité a rempli son tampon (mode
    /// 565, 555, 1555, 888 ou 8888) ; copié dans le tampon de couleur.
    fn lfb_write(&mut self, buffer: u32, mode: u32, lower: bool, addr: u32, stride: usize, read: &mut dyn FnMut(u32, &mut [u8])) {
        let idx = self.buffer_index(buffer);
        let (w, h) = (self.width, self.height);
        let bpp = if matches!(mode, 4 | 5) { 4 } else { 2 };
        let mut rgba = vec![0u8; w * h * 4];
        let mut line = vec![0u8; w * bpp];
        for y in 0..h {
            read(addr + (y * stride) as u32, &mut line);
            // Image OpenGL de bas en haut.
            let dst = if lower { y } else { h - 1 - y };
            for x in 0..w {
                let (r, g, b) = match bpp {
                    4 => (line[x * 4 + 2], line[x * 4 + 1], line[x * 4]),
                    _ => {
                        let v = u16::from_le_bytes([line[x * 2], line[x * 2 + 1]]);
                        let ex5 = |x: u16| ((x & 31) * 255 / 31) as u8;
                        match mode {
                            1 | 2 => (ex5(v >> 10), ex5(v >> 5), ex5(v)),
                            _ => (ex5(v >> 11), ((v >> 5 & 63) * 255 / 63) as u8, ex5(v)),
                        }
                    }
                };
                rgba[(dst * w + x) * 4..(dst * w + x) * 4 + 4].copy_from_slice(&[r, g, b, 0xFF]);
            }
        }
        let back = self.gpu.as_ref().map_or(0, |g| g.off.target);
        let Some(g) = &mut self.gpu else { return };
        g.off.attach(idx);
        let gl = &g.off.gl;
        // SAFETY: contexte courant ; image de la taille du tampon.
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::BLEND);
            gl.color_mask(true, true, true, true);
            gl.use_program(Some(g.blit));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(g.blit_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w as i32,
                h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&rgba)),
            );
            gl.bind_vertex_array(Some(g.blit_vao));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
        g.off.attach(back);
        self.applied = false;
        if buffer == 0 {
            self.frame_dirty = true;
        }
    }
}

/// Correction gamma de la sortie vidéo : v' = 255 · (v / 255)^(1/γ).
fn gamma_table(gamma: f32) -> [u8; 256] {
    let g = if gamma.is_finite() && gamma > 0.05 { gamma } else { 1.0 };
    std::array::from_fn(|i| ((i as f32 / 255.0).powf(1.0 / g) * 255.0).round() as u8)
}

/// Tables de brouillard de `guFogGenerateExp` (0), `Exp2` (1), `Linear` (2) :
/// entrée i pour w = 2^(3 + i/4) / (8 − i mod 4), comme `guFogTableIndexToW`.
fn fog_table(kind: u32, p1: f32, p2: f32) -> [u8; 64] {
    let mut t = [0u8; 64];
    let wmax = 2f32.powf(3.0 + 63.0 / 4.0) / (8.0 - 3.0);
    for (i, e) in t.iter_mut().enumerate() {
        let w = 2f32.powf(3.0 + (i >> 2) as f32) / (8 - (i & 3)) as f32;
        let f = match kind {
            0 => 1.0 - (-p1 * w).exp(),
            1 => 1.0 - (-(p1 * w) * (p1 * w)).exp(),
            _ => {
                // linéaire en z entre near et far, ramené sur l'échelle de w
                let (near, far) = (p1, p2);
                if far <= near { 0.0 } else { ((w - near) / (far - near)).clamp(0.0, 1.0) }
            }
        };
        let _ = wmax;
        *e = (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un triangle rouge sur fond bleu, rendu hors écran, relu.
    #[test]
    fn triangle_on_cleared_screen() {
        let mut gl = Glide::new();
        let mut nr = |_: u32, _: &mut [u8]| {};
        let mut nw = |_: u32, _: &[u8]| {};
        let cmd = |op: u16, args: &[u32]| -> Vec<u32> {
            let mut v = vec![(u32::from(op) << 16) | args.len() as u32];
            v.extend_from_slice(args);
            v
        };
        if gl.execute(&cmd(proto::INIT, &[]), &mut nr, &mut nw) == 0 {
            eprintln!("no offscreen OpenGL: test skipped");
            return;
        }
        let mut w = cmd(proto::WIN_OPEN, &[7, 0, 0]);
        w.extend(cmd(proto::CLEAR, &[0x0000_00FF, 0, 0]));
        let v = |x: f32, y: f32| -> Vec<u32> {
            [x, y, 0.0, 1.0, 255.0, 0.0, 0.0, 255.0, 0.0, 0.0, 1.0].iter().map(|f| f.to_bits()).collect()
        };
        let mut tri = v(100.0, 100.0);
        tri.extend(v(300.0, 100.0));
        tri.extend(v(100.0, 300.0));
        w.extend(cmd(proto::TRIANGLE, &tri));
        w.extend(cmd(proto::SWAP, &[0]));
        gl.execute(&w, &mut nr, &mut nw);
        let (fw, fh, px) = gl.frame().expect("image");
        assert_eq!((fw, fh), (640, 480));
        assert_eq!(px[0], 0x0000FF, "fond");
        assert_eq!(px[150 * 640 + 150], 0xFF0000, "triangle (origine en haut à gauche)");
        assert_eq!(px[350 * 640 + 350], 0x0000FF);
    }
}
