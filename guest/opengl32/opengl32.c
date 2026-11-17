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
    OP_MISSING,              /* nom (4 mots) d'une fonction absente */
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

static u32 *cmd(u32 op, u32 n) {
    if (pos + 1 + n > BUF_WORDS) flush();
    buf[pos] = op << 16 | n;
    u32 *a = &buf[pos + 1];
    pos += 1 + n;
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

static const struct { const char *name; void *fn; } procs[] = {
    {"wglSwapIntervalEXT", (void *)wglSwapIntervalEXT},
    {"wglGetSwapIntervalEXT", (void *)wglGetSwapIntervalEXT},
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
