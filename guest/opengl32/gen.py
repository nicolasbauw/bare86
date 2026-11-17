#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Génère, d'après gl11.txt (les 336 prototypes d'OpenGL 1.1) :
 - guest/opengl32/gl_gen.c : les fonctions de la DLL invitée, qui rangent
   chaque appel dans le tampon de commandes ;
 - guest/opengl32/opengl32.def : la table d'exportation (noms sans décoration,
   comme l'opengl32.dll de Windows) ;
 - src/opengl/api.rs : numéros des commandes, pointeurs des fonctions de
   l'hôte et rejeu des appels sans pointeur vers l'invité.

Genres d'appels :
 - simple : arguments scalaires, rangé dans le tampon ;
 - vecteur : pointeur vers un nombre fixe de valeurs (glColor3fv,
   glLoadMatrixf, glLightfv…), copiées dans la commande ;
 - synchrone : valeur rendue, ou pointeur dont la taille dépend de l'état
   (images, tableaux de sommets, glGet*) : le tampon part aussitôt et l'hôte
   lit ou écrit la mémoire de l'invité (code écrit à la main, src/opengl) ;
 - hôte : rangé dans le tampon, mais rejoué par du code écrit à la main.
"""

import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

# Types scalaires : (octets sur la pile, type Rust, décodage d'un mot `w`).
SCALAR = {
    'GLenum': (4, 'u32', '{w}'),
    'GLbitfield': (4, 'u32', '{w}'),
    'GLuint': (4, 'u32', '{w}'),
    'GLint': (4, 'i32', '{w} as i32'),
    'GLsizei': (4, 'i32', '{w} as i32'),
    'GLbyte': (4, 'i8', '{w} as i8'),
    'GLshort': (4, 'i16', '{w} as i16'),
    'GLubyte': (4, 'u8', '{w} as u8'),
    'GLushort': (4, 'u16', '{w} as u16'),
    'GLboolean': (4, 'u8', '{w} as u8'),
    'GLfloat': (4, 'f32', 'f32::from_bits({w})'),
    'GLclampf': (4, 'f32', 'f32::from_bits({w})'),
    'GLdouble': (8, 'f64', None),
    'GLclampd': (8, 'f64', None),
}
ELEM = {'GLbyte': 1, 'GLubyte': 1, 'GLboolean': 1, 'GLshort': 2, 'GLushort': 2,
        'GLint': 4, 'GLuint': 4, 'GLfloat': 4, 'GLdouble': 8}

# Appels synchrones (code de l'hôte écrit à la main).
SYNC = {
    'glGetPolygonStipple', 'glGetClipPlane', 'glGetBooleanv', 'glGetDoublev',
    'glGetFloatv', 'glGetIntegerv', 'glGetString', 'glCallLists',
    'glGetPointerv', 'glDrawArrays', 'glDrawElements', 'glArrayElement',
    'glGetLightfv', 'glGetLightiv', 'glGetMaterialfv', 'glGetMaterialiv',
    'glPixelMapfv', 'glPixelMapuiv', 'glPixelMapusv', 'glGetPixelMapfv',
    'glGetPixelMapuiv', 'glGetPixelMapusv', 'glBitmap', 'glReadPixels',
    'glDrawPixels', 'glGetTexGendv', 'glGetTexGenfv', 'glGetTexGeniv',
    'glGetTexEnvfv', 'glGetTexEnviv', 'glGetTexParameterfv',
    'glGetTexParameteriv', 'glGetTexLevelParameterfv',
    'glGetTexLevelParameteriv', 'glTexImage1D', 'glTexImage2D',
    'glGetTexImage', 'glGenTextures', 'glDeleteTextures',
    'glPrioritizeTextures', 'glAreTexturesResident', 'glTexSubImage1D',
    'glTexSubImage2D', 'glMap1d', 'glMap1f', 'glMap2d', 'glMap2f',
    'glGetMapdv', 'glGetMapfv', 'glGetMapiv', 'glRenderMode',
    'glFeedbackBuffer', 'glSelectBuffer', 'glGetError', 'glIsEnabled',
    'glIsList', 'glGenLists', 'glIsTexture', 'glFinish', 'glFlush',
}
# Rangés dans le tampon, rejoués par du code écrit à la main.
HOST = {
    'glVertexPointer', 'glNormalPointer', 'glColorPointer', 'glIndexPointer',
    'glTexCoordPointer', 'glEdgeFlagPointer', 'glInterleavedArrays',
    'glEnableClientState', 'glDisableClientState', 'glPushClientAttrib',
    'glPopClientAttrib', 'glDrawBuffer', 'glReadBuffer',
}
# Écrites à la main dans la DLL invitée.
GUEST = {'glGetString'}

# Vecteurs : nombre de valeurs lues par pointeur.
PARAM4 = re.compile(r'^gl(Light|LightModel|Material|TexGen|TexEnv|TexParameter|Fog)[dfi]v$')


def vec_count(name):
    m = re.match(r'^gl(Vertex|TexCoord|RasterPos|Color|Normal|EvalCoord)(\d)(b|s|i|f|d|ub|us|ui)v$', name)
    if m:
        return int(m.group(2))
    if re.match(r'^glIndex(s|i|f|d|ub)v$', name) or name == 'glEdgeFlagv':
        return 1
    if re.match(r'^glRect[sifd]v$', name):
        return 2
    if re.match(r'^gl(Load|Mult)Matrix[fd]$', name):
        return 16
    if name == 'glClipPlane':
        return 4
    if PARAM4.match(name):
        return 4
    if name == 'glPolygonStipple':
        return 128
    return None


def parse():
    out = []
    for line in open(os.path.join(HERE, 'gl11.txt')):
        m = re.match(r'^(.*?)\s*\b(gl\w+)\((.*)\);$', line.strip())
        ret, name, args = m.group(1).strip(), m.group(2), m.group(3).strip()
        params = []
        if args != 'void':
            for a in args.split(','):
                a = a.strip()
                pm = re.match(r'^(const\s+)?(\w+)\s*(\*+)?\s*(\w+)$', a)
                params.append({'const': bool(pm.group(1)), 'type': pm.group(2),
                               'ptr': len(pm.group(3) or ''), 'name': pm.group(4)})
        out.append({'ret': ret, 'name': name, 'params': params})
    return out


def kind(f):
    n = f['name']
    if n in SYNC:
        return 'sync'
    if n in HOST:
        return 'host'
    if any(p['ptr'] for p in f['params']):
        if vec_count(n) is None:
            sys.exit(f'{n}: pointeur sans genre')
        return 'vec'
    if f['ret'] != 'void':
        sys.exit(f'{n}: valeur rendue hors des appels synchrones')
    return 'simple'


def stack_bytes(f):
    return sum(4 if p['ptr'] else SCALAR[p['type']][0] for p in f['params'])


def c_ret(ret):
    return ret


def gen_c(funcs):
    o = ['/* Fichier produit par gen.py : ne pas modifier. */']
    for i, f in enumerate(funcs, 1):
        o.append(f'#define OP_{f["name"]} {i}')
    o.append(f'#define OP_GL_COUNT {len(funcs)}')
    for i, f in enumerate(funcs, 1):
        n, k = f['name'], kind(f)
        if n in GUEST:
            continue
        decl = ', '.join(('const ' if p['const'] else '') + p['type'] + ' ' + '*' * p['ptr'] + p['name']
                         for p in f['params']) or 'void'
        body = []
        words = []  # (expression C, nombre de mots)
        fixed = 0
        vecs = []
        for p in f['params']:
            if p['ptr'] and k == 'vec':
                cnt = vec_count(n)
                nbytes = cnt * ELEM[p['type']]
                vecs.append((p['name'], nbytes))
            elif p['ptr']:
                words.append((f'(u32){p["name"]}', 1))
            elif SCALAR[p['type']][0] == 8:
                words.append((f'dbits({p["name"]})', 2))
            elif p['type'] in ('GLfloat', 'GLclampf'):
                words.append((f'fbits({p["name"]})', 1))
            else:
                words.append((f'(u32){p["name"]}', 1))
        fixed = sum(w for _, w in words)
        total = fixed + sum((nb + 3) // 4 for _, nb in vecs)
        body.append(f'u32 *p = cmd(OP_{n}, {total});' if total else f'cmd(OP_{n}, 0);')
        k_at = 0
        for expr, w in words:
            if w == 2:
                body.append(f'put64(p + {k_at}, {expr});')
            else:
                body.append(f'p[{k_at}] = {expr};')
            k_at += w
        for name, nb in vecs:
            body.append(f'vcopy(p + {k_at}, {name}, {nb});')
            k_at += (nb + 3) // 4
        if k == 'sync':
            if f['ret'] == 'void':
                body.append('flush();')
            else:
                body.append(f'return ({f["ret"]})flush();')
        o.append(f'GLAPI {f["ret"]} glapi {n}({decl}) {{ ' + ' '.join(body) + ' }')
    return '\n'.join(o) + '\n'


WGL = [
    ('wglChoosePixelFormat', 8), ('wglCopyContext', 12), ('wglCreateContext', 4),
    ('wglCreateLayerContext', 8), ('wglDeleteContext', 4), ('wglDescribeLayerPlane', 20),
    ('wglDescribePixelFormat', 16), ('wglGetCurrentContext', 0), ('wglGetCurrentDC', 0),
    ('wglGetDefaultProcAddress', 4), ('wglGetLayerPaletteEntries', 20), ('wglGetPixelFormat', 4),
    ('wglGetProcAddress', 4), ('wglMakeCurrent', 8), ('wglRealizeLayerPalette', 12),
    ('wglSetLayerPaletteEntries', 20), ('wglSetPixelFormat', 12), ('wglShareLists', 8),
    ('wglSwapBuffers', 4), ('wglSwapLayerBuffers', 8), ('wglUseFontBitmapsA', 16),
    ('wglUseFontBitmapsW', 16), ('wglUseFontOutlinesA', 32), ('wglUseFontOutlinesW', 32),
]


def gen_def(funcs):
    o = ['; Fichier produit par gen.py : ne pas modifier.', 'LIBRARY opengl32', 'EXPORTS']
    for f in funcs:
        o.append(f'    {f["name"]}=_{f["name"]}@{stack_bytes(f)}')
    for n, b in WGL:
        o.append(f'    {n}=_{n}@{b}')
    return '\n'.join(o) + '\n'


def rust_ty(p):
    if p['ptr']:
        return '*const c_void'
    return SCALAR[p['type']][1]


def gen_rs(funcs):
    o = ['// SPDX-License-Identifier: GPL-3.0-or-later',
         '//! Fichier produit par guest/opengl32/gen.py : ne pas modifier.',
         '#![allow(non_upper_case_globals, non_snake_case, clippy::all)]',
         '',
         'use std::ffi::c_void;',
         '',
         '/// Numéros des commandes (ordre de gl11.txt).',
         'pub mod op {']
    for i, f in enumerate(funcs, 1):
        o.append(f'    pub const {f["name"]}: u16 = {i};')
    o.append(f'    pub const COUNT: u16 = {len(funcs)};')
    o.append('}')
    o.append('')
    o.append('/// Noms des fonctions, par numéro de commande moins un.')
    o.append('pub const NAMES: [&str; %d] = [' % len(funcs))
    for f in funcs:
        o.append(f'    "{f["name"]}",')
    o.append('];')
    o.append('')
    o.append('/// Fonctions d\'OpenGL 1.1 de l\'hôte.')
    o.append('pub struct Fns {')
    for f in funcs:
        args = ', '.join(rust_ty(p) for p in f['params'])
        ret = '' if f['ret'] == 'void' else ' -> ' + ('*const u8' if '*' in f['ret'] else SCALAR[f['ret']][1])
        o.append(f'    pub {f["name"]}: unsafe extern "C" fn({args}){ret},')
    o.append('}')
    o.append('')
    o.append('impl Fns {')
    o.append('    /// Charge toutes les fonctions ; échoue si l\'une manque.')
    o.append('    pub fn load(get: &dyn Fn(&str) -> *const c_void) -> Result<Self, String> {')
    o.append('        let f = |n: &str| { let p = get(n); if p.is_null() { Err(format!("OpenGL: {n} missing")) } else { Ok(p) } };')
    o.append('        // SAFETY: pointeurs rendus par le chargeur d\'OpenGL pour ces noms,')
    o.append('        // de signatures conformes à gl.h.')
    o.append('        unsafe {')
    o.append('            Ok(Self {')
    for f in funcs:
        o.append(f'                {f["name"]}: std::mem::transmute::<*const c_void, _>(f("{f["name"]}")?),')
    o.append('            })')
    o.append('        }')
    o.append('    }')
    o.append('')
    o.append('    /// Rejoue un appel simple ou à vecteur ; `None` pour ceux qui')
    o.append('    /// demandent du code écrit à la main.')
    o.append('    ///')
    o.append('    /// # Safety')
    o.append('    /// Contexte OpenGL de ces fonctions courant sur ce thread.')
    o.append('    pub unsafe fn call(&self, op: u16, a: &[u32]) -> Option<()> {')
    o.append('        let w = |k: usize| a.get(k).copied().unwrap_or(0);')
    o.append('        let d = |k: usize| f64::from_bits(u64::from(w(k)) | u64::from(w(k + 1)) << 32);')
    o.append('        let v = |k: usize| { let mut b = [0u64; 32]; for (i, x) in a.iter().skip(k).take(64).enumerate() { b[i / 2] |= u64::from(*x) << (32 * (i % 2)); } b };')
    o.append('        let _ = (&d, &v);')
    o.append('        // SAFETY: arguments décodés selon la signature de chaque fonction ;')
    o.append('        // les vecteurs pointent vers des copies locales assez grandes.')
    o.append('        unsafe {')
    o.append('            match op {')
    for i, f in enumerate(funcs, 1):
        k = kind(f)
        if k not in ('simple', 'vec'):
            continue
        args = []
        stmts = []
        at = 0
        vi = 0
        for p in f['params']:
            if p['ptr']:
                nb = vec_count(f['name']) * ELEM[p['type']]
                stmts.append(f'let v{vi} = v({at});')
                args.append(f'v{vi}.as_ptr() as *const c_void')
                at += (nb + 3) // 4
                vi += 1
            elif SCALAR[p['type']][0] == 8:
                args.append(f'd({at})')
                at += 2
            else:
                args.append(SCALAR[p['type']][2].format(w=f'w({at})'))
                at += 1
        call = f'(self.{f["name"]})({", ".join(args)})'
        if stmts:
            o.append(f'                {i} => {{ {" ".join(stmts)} {call}; }}')
        else:
            o.append(f'                {i} => {call},')
    o.append('                _ => return None,')
    o.append('            }')
    o.append('        }')
    o.append('        Some(())')
    o.append('    }')
    o.append('}')
    return '\n'.join(o) + '\n'


def main():
    funcs = parse()
    assert len(funcs) == 336, len(funcs)
    with open(os.path.join(HERE, 'gl_gen.c'), 'w') as fh:
        fh.write(gen_c(funcs))
    with open(os.path.join(HERE, 'opengl32.def'), 'w') as fh:
        fh.write(gen_def(funcs))
    with open(os.path.join(ROOT, 'src', 'opengl', 'api.rs'), 'w') as fh:
        fh.write(gen_rs(funcs))


if __name__ == '__main__':
    main()
