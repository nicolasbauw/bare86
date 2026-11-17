/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * opengl32.dll de Bare86 : OpenGL 1.1 pour les programmes Win32 lancés par
 * HX DOS Extender. Aucun accès au matériel : chaque appel est rangé dans un
 * tampon de commandes, transmis à l'hôte par `out 0E3h, al` (EBX = tampon,
 * ECX = longueur, EAX en retour), qui le rejoue avec son propre OpenGL. Les
 * appels qui rendent une valeur ou passent un pointeur de taille variable
 * vident le tampon aussitôt ; l'hôte lit alors la mémoire du programme.
 * Protocole : src/opengl/mod.rs ; les 336 fonctions d'OpenGL 1.1 sont
 * produites par gen.py (gl_gen.c), à partir des prototypes de gl11.txt.
 *
 * Le GDI32 de HX (DGDI32.DLL) renvoie ChoosePixelFormat, SetPixelFormat,
 * DescribePixelFormat et SwapBuffers aux fonctions wgl* de cette DLL, comme
 * le GDI32 de Windows 9x.
 *
 * Compilé sans bibliothèque C (guest/opengl32/build.sh).
 */

typedef unsigned int u32;
typedef unsigned long long u64;
typedef unsigned char u8;

typedef unsigned int GLenum;
typedef unsigned char GLboolean;
typedef unsigned int GLbitfield;
typedef void GLvoid;
typedef signed char GLbyte;
typedef short GLshort;
typedef int GLint;
typedef unsigned char GLubyte;
typedef unsigned short GLushort;
typedef unsigned int GLuint;
typedef int GLsizei;
typedef float GLfloat;
typedef float GLclampf;
typedef double GLdouble;
typedef double GLclampd;

#define GLAPI /* exportées par opengl32.def, sans décoration */
#define glapi __stdcall

/* Commandes propres à Bare86 (après celles d'OpenGL 1.1). */
enum {
    OP_MAKE_CURRENT = 0x401, /* actif (0/1) ; rend 1 si l'hôte a un OpenGL */
    OP_SWAP,                 /* intervalle de balayage */
    OP_MISSING,              /* nom (8 mots) d'une fonction absente */
    OP_ACTIVE_TEXTURE,       /* unité (GL_TEXTURE0_ARB + n) */
    OP_CLIENT_ACTIVE_TEXTURE,/* unité */
    OP_MULTI_TEXCOORD,       /* unité, s, t, r, q */
    OP_POINT_PARAMETER,      /* pname, 3 valeurs */
    OP_LOCK_ARRAYS,          /* premier, nombre : copie des tableaux */
    OP_UNLOCK_ARRAYS,
    OP_INLINE,               /* fonction, nombre d'arguments, arguments, pixels */
};

/* Sans bibliothèque C : ce que le compilateur peut appeler de lui-même. */
int _fltused = 0;
void *memset(void *d, int c, unsigned n) { u8 *p = d; while (n--) *p++ = (u8)c; return d; }
void *memcpy(void *d, const void *s, unsigned n) { u8 *p = d; const u8 *q = s; while (n--) *p++ = *q++; return d; }
int __stdcall _DllMainCRTStartup(void *h, u32 reason, void *r) { (void)h; (void)reason; (void)r; return 1; }

/* --- Tampon de commandes ----------------------------------------------- */
#define BUF_WORDS (1u << 19)          /* 2 Mo */
static u32 buf[BUF_WORDS];
static u32 pos;

static u32 flush(void) {
    u32 ret;
    __asm__ volatile("outb %%al, $0xE3" : "=a"(ret) : "a"(0), "b"(buf), "c"(pos * 4) : "memory");
    pos = 0;
    return ret;
}

/* En-tête : (numéro << 16) | nombre de mots ; à partir de FFFFh mots, le
 * nombre suit dans le mot d'après. */
static u32 *cmd(u32 op, u32 n) {
    u32 ext = n >= 0xFFFF;
    if (pos + 1 + ext + n > BUF_WORDS) flush();
    buf[pos] = op << 16 | (ext ? 0xFFFF : n);
    if (ext) buf[pos + 1] = n;
    u32 *a = &buf[pos + 1 + ext];
    pos += 1 + ext + n;
    return a;
}

static u32 fbits(float f) { union { float f; u32 u; } v; v.f = f; return v.u; }
static u64 dbits(double d) { union { double d; u64 u; } v; v.d = d; return v.u; }
static void put64(u32 *p, u64 v) { p[0] = (u32)v; p[1] = (u32)(v >> 32); }
static void vcopy(u32 *p, const void *v, u32 n) {
    const u8 *s = v;
    u8 *d = (u8 *)p;
    for (u32 i = 0; i < n; i++) d[i] = s[i];
    for (u32 i = n; i & 3; i++) d[i] = 0;
}

#include "gl_gen.c"

/* --- glArrayElement : synchrone, sauf tableaux verrouillés ------------ */
static int locked;
GLAPI void glapi glArrayElement(GLint i) {
    cmd(OP_glArrayElement, 1)[0] = (u32)i;
    if (!locked) flush();
}

/* --- Images copiées dans le tampon -------------------------------------
 * Les envois d'images (textures surtout : Quake II met à jour ses lightmaps
 * surface par surface) partent avec leurs pixels, sans attendre l'hôte. La
 * DLL suit pour cela l'état de déballage (glPixelStore, glPush/PopClient-
 * Attrib) afin de calculer la taille lue ; l'hôte, dans le même état, lit
 * les pixels au même endroit. Au-delà de INLINE_MAX, appel synchrone.
 */
#define INLINE_MAX (1u << 20)
#define GL_CLIENT_PIXEL_STORE_BIT 1u
enum { ROW_LENGTH, SKIP_ROWS, SKIP_PIXELS, ALIGNMENT };
static GLint unpack[4] = {0, 0, 0, 4};
static struct { GLbitfield mask; GLint unpack[4]; } client_stack[16];
static u32 client_depth;

GLAPI void glapi glPixelStorei(GLenum pname, GLint param) {
    if (pname >= 0x0CF2 && pname <= 0x0CF5) unpack[pname - 0x0CF2] = param;
    u32 *p = cmd(OP_glPixelStorei, 2);
    p[0] = pname; p[1] = (u32)param;
}
GLAPI void glapi glPixelStoref(GLenum pname, GLfloat param) { glPixelStorei(pname, (GLint)param); }
GLAPI void glapi glPushClientAttrib(GLbitfield mask) {
    if (client_depth < 16) {
        client_stack[client_depth].mask = mask;
        memcpy(client_stack[client_depth].unpack, unpack, sizeof unpack);
        client_depth++;
    }
    cmd(OP_glPushClientAttrib, 1)[0] = mask;
}
GLAPI void glapi glPopClientAttrib(void) {
    if (client_depth) {
        client_depth--;
        if (client_stack[client_depth].mask & GL_CLIENT_PIXEL_STORE_BIT)
            memcpy(unpack, client_stack[client_depth].unpack, sizeof unpack);
    }
    cmd(OP_glPopClientAttrib, 0);
}

static u32 type_size(GLenum type) {
    switch (type) {
    case 0x1400: case 0x1401: return 1;
    case 0x1402: case 0x1403: return 2;
    default: return 4;
    }
}

/* Octets lus pour une image (même calcul que image_bytes de l'hôte). */
static u32 image_bytes(GLsizei w, GLsizei h, GLenum format, GLenum type) {
    if (w <= 0 || h <= 0) return 0;
    u32 row_len = unpack[ROW_LENGTH] > 0 ? (u32)unpack[ROW_LENGTH] : (u32)w;
    u32 skip_rows = unpack[SKIP_ROWS] > 0 ? (u32)unpack[SKIP_ROWS] : 0;
    u32 skip_pixels = unpack[SKIP_PIXELS] > 0 ? (u32)unpack[SKIP_PIXELS] : 0;
    u32 align = unpack[ALIGNMENT] > 0 ? (u32)unpack[ALIGNMENT] : 1;
    if (type == 0x1A00) {
        u32 stride = ((row_len + 7) / 8 + align - 1) / align * align;
        return (skip_rows + (u32)h - 1) * stride + (skip_pixels + (u32)w + 7) / 8;
    }
    u32 comps, size;
    switch (format) {
    case 0x1907: case 0x80E0: comps = 3; break;
    case 0x190A: comps = 2; break;
    case 0x1900: case 0x1901: case 0x1902: case 0x1903: case 0x1904:
    case 0x1905: case 0x1906: case 0x1909: comps = 1; break;
    default: comps = 4;
    }
    switch (type) {
    case 0x8032: case 0x8362: comps = 1; size = 1; break;
    case 0x8033: case 0x8034: case 0x8363: case 0x8364: case 0x8365: case 0x8366: comps = 1; size = 2; break;
    case 0x8035: case 0x8036: case 0x8367: case 0x8368: comps = 1; size = 4; break;
    default: size = type_size(type);
    }
    u32 group = comps * size, row = row_len * group;
    u32 stride = size >= align ? row : (row + align - 1) / align * align;
    return (skip_rows + (u32)h - 1) * stride + (skip_pixels + (u32)w) * group;
}

/* Range l'appel `op` (arguments `a`, pixels en dernier) avec ses pixels ;
 * faux si l'image est trop grande (l'appelant passe alors en synchrone). */
static int send_image(u32 op, const u32 *a, u32 nargs, const void *pixels, u32 bytes) {
    if (!pixels) bytes = 0;
    if (bytes > INLINE_MAX) return 0;
    u32 *p = cmd(OP_INLINE, 2 + nargs + (bytes + 3) / 4);
    p[0] = op; p[1] = nargs;
    for (u32 i = 0; i < nargs; i++) p[2 + i] = a[i];
    vcopy(p + 2 + nargs, pixels, bytes);
    return 1;
}
static void send_sync(u32 op, const u32 *a, u32 nargs, const void *pixels) {
    u32 *p = cmd(op, nargs + 1);
    for (u32 i = 0; i < nargs; i++) p[i] = a[i];
    p[nargs] = (u32)pixels;
    flush();
}
#define IMAGE(op, n, px, bytes) do { if (!send_image(op, a, n, px, bytes)) send_sync(op, a, n, px); } while (0)

GLAPI void glapi glTexImage1D(GLenum target, GLint level, GLint internal, GLsizei w, GLint border, GLenum format, GLenum type, const GLvoid *pixels) {
    u32 a[] = {target, (u32)level, (u32)internal, (u32)w, (u32)border, format, type};
    IMAGE(OP_glTexImage1D, 7, pixels, image_bytes(w, 1, format, type));
}
GLAPI void glapi glTexImage2D(GLenum target, GLint level, GLint internal, GLsizei w, GLsizei h, GLint border, GLenum format, GLenum type, const GLvoid *pixels) {
    u32 a[] = {target, (u32)level, (u32)internal, (u32)w, (u32)h, (u32)border, format, type};
    IMAGE(OP_glTexImage2D, 8, pixels, image_bytes(w, h, format, type));
}
GLAPI void glapi glTexSubImage1D(GLenum target, GLint level, GLint x, GLsizei w, GLenum format, GLenum type, const GLvoid *pixels) {
    u32 a[] = {target, (u32)level, (u32)x, (u32)w, format, type};
    IMAGE(OP_glTexSubImage1D, 6, pixels, image_bytes(w, 1, format, type));
}
GLAPI void glapi glTexSubImage2D(GLenum target, GLint level, GLint x, GLint y, GLsizei w, GLsizei h, GLenum format, GLenum type, const GLvoid *pixels) {
    u32 a[] = {target, (u32)level, (u32)x, (u32)y, (u32)w, (u32)h, format, type};
    IMAGE(OP_glTexSubImage2D, 8, pixels, image_bytes(w, h, format, type));
}
GLAPI void glapi glDrawPixels(GLsizei w, GLsizei h, GLenum format, GLenum type, const GLvoid *pixels) {
    u32 a[] = {(u32)w, (u32)h, format, type};
    IMAGE(OP_glDrawPixels, 4, pixels, image_bytes(w, h, format, type));
}
GLAPI void glapi glBitmap(GLsizei w, GLsizei h, GLfloat xo, GLfloat yo, GLfloat xm, GLfloat ym, const GLubyte *bitmap) {
    u32 a[] = {(u32)w, (u32)h, fbits(xo), fbits(yo), fbits(xm), fbits(ym)};
    IMAGE(OP_glBitmap, 6, bitmap, image_bytes(w, h, 0x1900, 0x1A00));
}

/* --- glGetString : l'hôte écrit la chaîne dans un tampon de la DLL ----- */
static char strings[4][4096];

GLAPI const GLubyte *glapi glGetString(GLenum name) {
    if (name < 0x1F00 || name > 0x1F03) return 0;
    char *s = strings[name - 0x1F00];
    u32 *p = cmd(OP_glGetString, 3);
    p[0] = name; p[1] = (u32)s; p[2] = sizeof strings[0];
    return flush() ? (const GLubyte *)s : 0;
}

/* --- WGL ---------------------------------------------------------------- */
typedef struct {
    unsigned short nSize, nVersion;
    u32 dwFlags;
    u8 iPixelType, cColorBits, cRedBits, cRedShift, cGreenBits, cGreenShift,
       cBlueBits, cBlueShift, cAlphaBits, cAlphaShift, cAccumBits,
       cAccumRedBits, cAccumGreenBits, cAccumBlueBits, cAccumAlphaBits,
       cDepthBits, cStencilBits, cAuxBuffers, iLayerType, bReserved;
    u32 dwLayerMask, dwVisibleMask, dwDamageMask;
} PIXELFORMATDESCRIPTOR;

#define CONTEXT 0x0B86A001u   /* seul contexte (valeur arbitraire, non nulle) */
static void *current_dc;
static u32 current;
static u32 pixel_format;
static int swap_interval = 1;

/* Signale à l'hôte une fonction non faite (nommée sur son terminal). */
static void missing(const char *name) {
    u32 *p = cmd(OP_MISSING, 8);
    u32 i = 0;
    for (; i < 31 && name[i]; i++) ((u8 *)p)[i] = (u8)name[i];
    for (; i < 32; i++) ((u8 *)p)[i] = 0;
    flush();
}

/* Un seul format : RGBA 32 bits, double tampon, profondeur 24, stencil 8,
 * accéléré (sans PFD_GENERIC_FORMAT : Quake II refuse un rendu logiciel). */
static void describe(PIXELFORMATDESCRIPTOR *d) {
    memset(d, 0, sizeof *d);
    d->nSize = sizeof *d;
    d->nVersion = 1;
    d->dwFlags = 0x4 | 0x20 | 0x1;   /* DRAW_TO_WINDOW, SUPPORT_OPENGL, DOUBLEBUFFER */
    d->cColorBits = 32;
    d->cRedBits = 8; d->cRedShift = 16;
    d->cGreenBits = 8; d->cGreenShift = 8;
    d->cBlueBits = 8; d->cBlueShift = 0;
    d->cAlphaBits = 8; d->cAlphaShift = 24;
    d->cDepthBits = 24;
    d->cStencilBits = 8;
}

GLAPI int glapi wglChoosePixelFormat(void *dc, const PIXELFORMATDESCRIPTOR *want) { (void)dc; (void)want; return 1; }
GLAPI int glapi wglDescribePixelFormat(void *dc, int format, u32 size, PIXELFORMATDESCRIPTOR *d) {
    (void)dc;
    if (d && format == 1 && size >= sizeof *d) describe(d);
    return 1;
}
GLAPI int glapi wglSetPixelFormat(void *dc, int format, const PIXELFORMATDESCRIPTOR *d) {
    (void)dc; (void)d;
    pixel_format = (u32)format;
    return format == 1;
}
GLAPI int glapi wglGetPixelFormat(void *dc) { (void)dc; return pixel_format ? (int)pixel_format : 1; }

GLAPI u32 glapi wglCreateContext(void *dc) { (void)dc; return CONTEXT; }
GLAPI u32 glapi wglCreateLayerContext(void *dc, int layer) { return layer == 0 ? wglCreateContext(dc) : 0; }
GLAPI int glapi wglDeleteContext(u32 rc) {
    if (rc != CONTEXT) return 0;
    if (current) {
        cmd(OP_MAKE_CURRENT, 1)[0] = 0;
        flush();
        current = 0;
        current_dc = 0;
    }
    return 1;
}
GLAPI int glapi wglMakeCurrent(void *dc, u32 rc) {
    if (rc && rc != CONTEXT) return 0;
    cmd(OP_MAKE_CURRENT, 1)[0] = rc ? 1 : 0;
    u32 ok = flush();
    if (rc && !ok) return 0;
    current = rc;
    current_dc = rc ? dc : 0;
    return 1;
}
GLAPI u32 glapi wglGetCurrentContext(void) { return current; }
GLAPI void *glapi wglGetCurrentDC(void) { return current_dc; }
GLAPI int glapi wglCopyContext(u32 src, u32 dst, u32 mask) { (void)src; (void)dst; (void)mask; return 1; }
GLAPI int glapi wglShareLists(u32 a, u32 b) { (void)a; (void)b; return 1; }

GLAPI int glapi wglSwapBuffers(void *dc) {
    (void)dc;
    cmd(OP_SWAP, 1)[0] = (u32)swap_interval;
    flush();
    return 1;
}
GLAPI int glapi wglSwapLayerBuffers(void *dc, u32 planes) { (void)planes; return wglSwapBuffers(dc); }

/* WGL_EXT_swap_control. */
static int glapi wglSwapIntervalEXT(int interval) { swap_interval = interval < 0 ? 0 : interval; return 1; }
static int glapi wglGetSwapIntervalEXT(void) { return swap_interval; }

static int streq(const char *a, const char *b) {
    while (*a && *a == *b) a++, b++;
    return *a == *b;
}

/* --- GL_ARB_multitexture et GL_SGIS_multitexture --------------------- */
#define GL_TEXTURE0_ARB 0x84C0u
#define GL_TEXTURE0_SGIS 0x835Eu

static void glapi glActiveTextureARB(GLenum unit) { cmd(OP_ACTIVE_TEXTURE, 1)[0] = unit; }
static void glapi glClientActiveTextureARB(GLenum unit) { cmd(OP_CLIENT_ACTIVE_TEXTURE, 1)[0] = unit; }
static void mtc(GLenum unit, float s, float t, float r, float q) {
    u32 *p = cmd(OP_MULTI_TEXCOORD, 5);
    p[0] = unit; p[1] = fbits(s); p[2] = fbits(t); p[3] = fbits(r); p[4] = fbits(q);
}
/* Les quatre types (d, f, i, s), en valeurs et en vecteurs, pour 1 à 4
 * coordonnées ; les absentes valent 0 (t, r) et 1 (q). */
#define MTC(T, ty) \
    static void glapi glMultiTexCoord1##T##ARB(GLenum u, ty s) { mtc(u, (float)s, 0, 0, 1); } \
    static void glapi glMultiTexCoord2##T##ARB(GLenum u, ty s, ty t) { mtc(u, (float)s, (float)t, 0, 1); } \
    static void glapi glMultiTexCoord3##T##ARB(GLenum u, ty s, ty t, ty r) { mtc(u, (float)s, (float)t, (float)r, 1); } \
    static void glapi glMultiTexCoord4##T##ARB(GLenum u, ty s, ty t, ty r, ty q) { mtc(u, (float)s, (float)t, (float)r, (float)q); } \
    static void glapi glMultiTexCoord1##T##vARB(GLenum u, const ty *v) { mtc(u, (float)v[0], 0, 0, 1); } \
    static void glapi glMultiTexCoord2##T##vARB(GLenum u, const ty *v) { mtc(u, (float)v[0], (float)v[1], 0, 1); } \
    static void glapi glMultiTexCoord3##T##vARB(GLenum u, const ty *v) { mtc(u, (float)v[0], (float)v[1], (float)v[2], 1); } \
    static void glapi glMultiTexCoord4##T##vARB(GLenum u, const ty *v) { mtc(u, (float)v[0], (float)v[1], (float)v[2], (float)v[3]); }
MTC(d, GLdouble)
MTC(f, GLfloat)
MTC(i, GLint)
MTC(s, GLshort)

/* SGIS : une seule unité choisie pour les deux usages (état et tableaux). */
static void glapi glSelectTextureSGIS(GLenum target) {
    GLenum unit = GL_TEXTURE0_ARB + (target - GL_TEXTURE0_SGIS);
    glActiveTextureARB(unit);
    glClientActiveTextureARB(unit);
}
static void glapi glMTexCoord2fSGIS(GLenum target, GLfloat s, GLfloat t) {
    mtc(GL_TEXTURE0_ARB + (target - GL_TEXTURE0_SGIS), s, t, 0, 1);
}
static void glapi glMTexCoord2fvSGIS(GLenum target, const GLfloat *v) { glMTexCoord2fSGIS(target, v[0], v[1]); }

/* --- GL_EXT_point_parameters ------------------------------------------- */
static void glapi glPointParameterfEXT(GLenum pname, GLfloat v) {
    u32 *p = cmd(OP_POINT_PARAMETER, 4);
    p[0] = pname; p[1] = fbits(v); p[2] = 0; p[3] = 0;
}
static void glapi glPointParameterfvEXT(GLenum pname, const GLfloat *v) {
    u32 *p = cmd(OP_POINT_PARAMETER, 4);
    /* GL_DISTANCE_ATTENUATION_EXT (8129h) : trois valeurs ; sinon une. */
    p[0] = pname; p[1] = fbits(v[0]);
    p[2] = pname == 0x8129 ? fbits(v[1]) : 0;
    p[3] = pname == 0x8129 ? fbits(v[2]) : 0;
}

/* --- GL_EXT_compiled_vertex_array : l'hôte copie une fois les éléments
 * verrouillés ; tant que le verrou tient, glArrayElement n'attend plus
 * l'hôte (Quake II dessine ainsi ses modèles, un appel par sommet). */
static void glapi glLockArraysEXT(GLint first, GLsizei count) {
    u32 *p = cmd(OP_LOCK_ARRAYS, 2);
    p[0] = (u32)first; p[1] = (u32)count;
    flush();
    locked = 1;
}
static void glapi glUnlockArraysEXT(void) { cmd(OP_UNLOCK_ARRAYS, 0); locked = 0; }

#define P(f) {#f, (void *)f}
#define MTC_P(T) P(glMultiTexCoord1##T##ARB), P(glMultiTexCoord2##T##ARB), P(glMultiTexCoord3##T##ARB), \
    P(glMultiTexCoord4##T##ARB), P(glMultiTexCoord1##T##vARB), P(glMultiTexCoord2##T##vARB), \
    P(glMultiTexCoord3##T##vARB), P(glMultiTexCoord4##T##vARB)
static const struct { const char *name; void *fn; } procs[] = {
    P(wglSwapIntervalEXT), P(wglGetSwapIntervalEXT),
    P(glActiveTextureARB), P(glClientActiveTextureARB),
    MTC_P(d), MTC_P(f), MTC_P(i), MTC_P(s),
    P(glSelectTextureSGIS), P(glMTexCoord2fSGIS), P(glMTexCoord2fvSGIS),
    P(glPointParameterfEXT), P(glPointParameterfvEXT),
    P(glLockArraysEXT), P(glUnlockArraysEXT),
};

GLAPI void *glapi wglGetProcAddress(const char *name) {
    if (!name) return 0;
    for (u32 i = 0; i < sizeof procs / sizeof procs[0]; i++)
        if (streq(name, procs[i].name)) return procs[i].fn;
    return 0;
}
GLAPI void *glapi wglGetDefaultProcAddress(const char *name) { return wglGetProcAddress(name); }

/* Plans de recouvrement et polices : absents. */
GLAPI int glapi wglDescribeLayerPlane(void *dc, int f, int l, u32 n, void *d) { (void)dc; (void)f; (void)l; (void)n; (void)d; return 0; }
GLAPI int glapi wglGetLayerPaletteEntries(void *dc, int l, int s, int n, void *c) { (void)dc; (void)l; (void)s; (void)n; (void)c; return 0; }
GLAPI int glapi wglSetLayerPaletteEntries(void *dc, int l, int s, int n, const void *c) { (void)dc; (void)l; (void)s; (void)n; (void)c; return 0; }
GLAPI int glapi wglRealizeLayerPalette(void *dc, int l, int r) { (void)dc; (void)l; (void)r; return 0; }
GLAPI int glapi wglUseFontBitmapsA(void *dc, u32 a, u32 b, u32 c) { (void)dc; (void)a; (void)b; (void)c; missing("wglUseFontBitmapsA"); return 0; }
GLAPI int glapi wglUseFontBitmapsW(void *dc, u32 a, u32 b, u32 c) { (void)dc; (void)a; (void)b; (void)c; missing("wglUseFontBitmapsW"); return 0; }
GLAPI int glapi wglUseFontOutlinesA(void *dc, u32 a, u32 b, u32 c, float d, float e, int f, void *g) {
    (void)dc; (void)a; (void)b; (void)c; (void)d; (void)e; (void)f; (void)g; missing("wglUseFontOutlinesA"); return 0;
}
GLAPI int glapi wglUseFontOutlinesW(void *dc, u32 a, u32 b, u32 c, float d, float e, int f, void *g) {
    (void)dc; (void)a; (void)b; (void)c; (void)d; (void)e; (void)f; (void)g; missing("wglUseFontOutlinesW"); return 0;
}
