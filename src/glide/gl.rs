// SPDX-License-Identifier: GPL-3.0-or-later
//! Contexte OpenGL hors écran (EGL sans surface, Mesa) : le rendu 3D se fait
//! dans un FBO sur le thread du vCPU, puis l'image est relue et passe par
//! le même chemin d'affichage que la VGA. Aucun contexte partagé avec la
//! fenêtre SDL ; marche aussi sans fenêtre (`--shot`, tests).

use glow::HasContext;
use khronos_egl as egl;

/// EGL_PLATFORM_SURFACELESS_MESA.
const PLATFORM_SURFACELESS: egl::Enum = 0x31DD;

pub struct Offscreen {
    egl: egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    context: egl::Context,
    pub gl: glow::Context,
    fbo: glow::Framebuffer,
    /// Deux tampons de couleur (avant et arrière, échangés par `grBufferSwap`).
    colors: [glow::Renderbuffer; 2],
    depth: glow::Renderbuffer,
    /// Tampon de couleur attaché au FBO (cible du dessin).
    pub target: usize,
    pub width: usize,
    pub height: usize,
    /// Profil « compatibilité » (OpenGL 1.1 rejoué tel quel) : les deux
    /// tampons de couleur sont attachés ensemble (COLOR_ATTACHMENT0 et 1),
    /// la profondeur porte un stencil.
    compat: bool,
}

impl Offscreen {
    /// Contexte OpenGL 3.3 « core » et FBO de `width`×`height` (couleur
    /// RGBA8, profondeur 24 bits), rendu courant sur le thread appelant.
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        Self::create(width, height, false)
    }

    /// Contexte OpenGL en profil « compatibilité » (le plus récent que
    /// donne le pilote), pour rejouer les appels d'OpenGL 1.1 ; FBO à deux
    /// tampons de couleur attachés ensemble et profondeur avec stencil.
    pub fn new_compat(width: usize, height: usize) -> Result<Self, String> {
        Self::create(width, height, true)
    }

    fn create(width: usize, height: usize, compat: bool) -> Result<Self, String> {
        // SAFETY: chargement de libEGL, appels EGL avec des listes d'attributs
        // terminées par NONE ; le contexte reste courant sur ce thread.
        unsafe {
            let egl = egl::DynamicInstance::<egl::EGL1_5>::load_required().map_err(|e| format!("libEGL: {e}"))?;
            let display = egl
                .get_platform_display(PLATFORM_SURFACELESS, egl::DEFAULT_DISPLAY, &[egl::ATTRIB_NONE])
                .map_err(|e| format!("EGL display: {e}"))?;
            egl.initialize(display).map_err(|e| format!("EGL initialization: {e}"))?;
            egl.bind_api(egl::OPENGL_API).map_err(|e| format!("EGL OpenGL API: {e}"))?;
            let config = egl
                .choose_first_config(display, &[egl::RENDERABLE_TYPE, egl::OPENGL_BIT, egl::SURFACE_TYPE, egl::PBUFFER_BIT, egl::NONE])
                .map_err(|e| format!("EGL configuration: {e}"))?
                .ok_or("EGL: no OpenGL configuration")?;
            let core = [
                egl::CONTEXT_MAJOR_VERSION,
                3,
                egl::CONTEXT_MINOR_VERSION,
                3,
                egl::CONTEXT_OPENGL_PROFILE_MASK,
                egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
                egl::NONE,
            ];
            let compatibility = [egl::CONTEXT_OPENGL_PROFILE_MASK, egl::CONTEXT_OPENGL_COMPATIBILITY_PROFILE_BIT, egl::NONE];
            let attrs: &[egl::Int] = if compat { &compatibility } else { &core };
            let context = egl.create_context(display, config, None, attrs).map_err(|e| format!("EGL context: {e}"))?;
            egl.make_current(display, None, None, Some(context)).map_err(|e| format!("EGL make current: {e}"))?;
            let gl = glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name).map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            });
            let fbo = gl.create_framebuffer()?;
            let colors = [gl.create_renderbuffer()?, gl.create_renderbuffer()?];
            let depth = gl.create_renderbuffer()?;
            let mut s = Self { egl, display, context, gl, fbo, colors, depth, target: 0, width: 0, height: 0, compat };
            s.resize(width, height);
            Ok(s)
        }
    }

    /// Nouvelle taille du FBO (contenu perdu).
    pub fn resize(&mut self, width: usize, height: usize) {
        let gl = &self.gl;
        // SAFETY: objets créés par ce contexte, courant sur ce thread.
        unsafe {
            for c in self.colors {
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(c));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, width as i32, height as i32);
            }
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(self.depth));
            let depth = if self.compat { glow::DEPTH24_STENCIL8 } else { glow::DEPTH_COMPONENT24 };
            gl.renderbuffer_storage(glow::RENDERBUFFER, depth, width as i32, height as i32);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            if self.compat {
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(self.colors[0]));
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT1, glow::RENDERBUFFER, Some(self.colors[1]));
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_STENCIL_ATTACHMENT, glow::RENDERBUFFER, Some(self.depth));
                // État initial d'une fenêtre neuve : vue et ciseaux à sa taille.
                gl.viewport(0, 0, width as i32, height as i32);
                gl.scissor(0, 0, width as i32, height as i32);
                self.width = width;
                self.height = height;
                return;
            }
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(self.colors[self.target]));
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(self.depth));
            gl.viewport(0, 0, width as i32, height as i32);
        }
        self.width = width;
        self.height = height;
    }

    /// Adresse d'une fonction d'OpenGL (nulle si absente).
    pub fn proc_address(&self, name: &str) -> *const std::ffi::c_void {
        self.egl.get_proc_address(name).map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
    }

    /// Rend ce contexte courant sur le thread appelant s'il ne l'est pas
    /// (Glide et OpenGL ont chacun le leur).
    pub fn make_current(&self) {
        if self.egl.get_current_context() != Some(self.context) {
            let _ = self.egl.make_current(self.display, None, None, Some(self.context));
        }
    }

    /// Dessin dans le tampon de couleur `i` (0 ou 1).
    pub fn attach(&mut self, i: usize) {
        self.target = i;
        // SAFETY: objets de ce contexte.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            self.gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(self.colors[i]));
        }
    }

    /// Pixels RGBA8 du tampon `i`, lignes de bas en haut (ordre d'OpenGL).
    pub fn read_rgba(&mut self, i: usize) -> Vec<u8> {
        let (w, h) = (self.width, self.height);
        let mut rgba = vec![0u8; w * h * 4];
        let back = self.target;
        self.attach(i);
        // SAFETY: tampon de la taille exacte de l'image lue.
        unsafe {
            self.gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut rgba)));
        }
        self.attach(back);
        rgba
    }

    /// Image du tampon `i` en 0x00RRGGBB, ligne du haut en premier.
    pub fn read(&mut self, i: usize, out: &mut Vec<u32>) {
        let (w, h) = (self.width, self.height);
        let rgba = self.read_rgba(i);
        out.clear();
        out.reserve(w * h);
        for y in (0..h).rev() {
            for p in rgba[y * w * 4..(y + 1) * w * 4].chunks_exact(4) {
                out.push(u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]));
            }
        }
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        // SAFETY: objets et contexte créés ici.
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            for c in self.colors {
                self.gl.delete_renderbuffer(c);
            }
            self.gl.delete_renderbuffer(self.depth);
            let _ = self.egl.make_current(self.display, None, None, None);
            let _ = self.egl.destroy_context(self.display, self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_and_read_back() {
        let mut s = match Offscreen::new(64, 48) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no offscreen OpenGL: {e}: test skipped");
                return;
            }
        };
        // SAFETY: contexte courant.
        unsafe {
            s.gl.clear_color(1.0, 0.5, 0.0, 1.0);
            s.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        let mut px = Vec::new();
        s.read(0, &mut px);
        assert_eq!(px.len(), 64 * 48);
        assert!(px.iter().all(|&p| p == 0xFF8000 || p == 0xFF7F00), "{:06X}", px[0]);
    }
}
