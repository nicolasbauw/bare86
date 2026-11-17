/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * glide2x.dll et glide2x.ovl de Bare86 : Glide 2 (3dfx) pour les programmes
 * Win32 lancés par HX DOS Extender (DLL PE), et pour les jeux DOS sous
 * DOS/4GW (DLL au format LE, assemblée par mkovl.py). Aucun accès au matériel : chaque appel est rangé dans
 * un tampon de commandes, transmis à l'hôte par `out 0E2h, al` (EBX =
 * tampon, ECX = longueur, EAX en retour), qui le rejoue en OpenGL. Le
 * protocole est décrit dans src/glide/proto.rs (mêmes numéros ici).
 *
 * Compilé sans bibliothèque C (guest/glide2x/build.sh). Les déclarations
 * sont écrites d'après l'interface publique de Glide 2 (constantes, structures
 * passées par le programme) ; aucun code de 3dfx n'est repris.
 */

typedef unsigned int u32;
typedef int i32;
typedef unsigned short u16;
typedef short i16;
typedef unsigned char u8;

#ifdef _WIN32
#define API __declspec(dllexport) __stdcall
#else
/* DOS/4GW : convention __stdcall de Watcom (pile, dépilée par l'appelée) ;
 * mkovl.py exporte ces fonctions sous les noms de la DLL de 3dfx. */
#define API __attribute__((stdcall))
#endif

/* --- Protocole (src/glide/proto.rs) ------------------------------------ */
enum {
    OP_INIT = 1, OP_WIN_OPEN, OP_WIN_CLOSE, OP_SWAP, OP_CLEAR, OP_RENDER_BUFFER,
    OP_ORIGIN, OP_CLIP, OP_COLOR_COMBINE, OP_ALPHA_COMBINE, OP_TEX_COMBINE,
    OP_BLEND, OP_ALPHA_TEST_FUNC, OP_ALPHA_TEST_REF, OP_CONSTANT_COLOR, OP_CULL,
    OP_DEPTH_BIAS, OP_DEPTH_FUNC, OP_DEPTH_MODE, OP_DEPTH_MASK, OP_COLOR_MASK,
    OP_FOG_COLOR, OP_FOG_MODE, OP_FOG_TABLE, OP_TEX_CLAMP, OP_TEX_FILTER,
    OP_TEX_MIPMAP, OP_TEX_SOURCE, OP_TEX_DOWNLOAD, OP_TEX_DOWNLOAD_PARTIAL,
    OP_TEX_TABLE, OP_TRIANGLE, OP_LINE, OP_POINT, OP_SAVE_STATE,
    OP_RESTORE_STATE, OP_LFB_READ, OP_LFB_WRITE, OP_FOG_GENERATE, OP_SHUTDOWN,
    OP_GAMMA, OP_MISSING, OP_CHROMA_MODE, OP_CHROMA_VALUE,
};
#define VERTEX_WORDS 11

/* --- Types de l'interface Glide 2 -------------------------------------- */
typedef struct { float sow, tow, oow; } GrTmuVertex;
typedef struct {
    float x, y, z, r, g, b, ooz, a, oow;
    GrTmuVertex tmuvtx[2];
} GrVertex;
typedef struct { i32 smallLod, largeLod, aspectRatio, format; void *data; } GrTexInfo;
typedef struct { i32 size; void *lfbPtr; u32 strideInBytes; i32 writeMode, origin; } GrLfbInfo_t;
typedef struct { char pad[312]; } GrState;
typedef struct { i32 tmuRev, tmuRam; } GrTMUConfig_t;
typedef struct {
    i32 num_sst;
    struct {
        i32 type;
        union {
            struct { i32 fbRam, fbiRev, nTexelfx, sliDetect; GrTMUConfig_t tmuConfig[2]; } VoodooConfig;
        } sstBoard;
    } SSTs[4];
} GrHwConfiguration;
typedef struct { u32 pixelsIn, chromaFail, zFuncFail, aFuncFail, pixelsOut; } GrSstPerfStats_t;

/* Sans bibliothèque C : ce que le compilateur peut appeler de lui-même. */
int _fltused = 0;
void *memset(void *d, int c, unsigned n) { u8 *p = d; while (n--) *p++ = (u8)c; return d; }
void *memcpy(void *d, const void *s, unsigned n) { u8 *p = d; const u8 *q = s; while (n--) *p++ = *q++; return d; }
#ifdef _WIN32
int __stdcall _DllMainCRTStartup(void *h, u32 reason, void *r) { (void)h; (void)reason; (void)r; return 1; }
#endif

/* --- Tampon de commandes ----------------------------------------------- */
#define BUF_WORDS (1u << 19)          /* 2 Mo */
static u32 buf[BUF_WORDS];
static u32 pos;

static u32 flush(void) {
    u32 ret;
    __asm__ volatile("outb %%al, $0xE2" : "=a"(ret) : "a"(0), "b"(buf), "c"(pos * 4) : "memory");
    pos = 0;
    return ret;
}

/* Réserve la place d'une commande de `n` mots d'arguments ; rend le pointeur
 * vers ses arguments. */
static u32 *cmd(u32 op, u32 n) {
    if (pos + 1 + n > BUF_WORDS) flush();
    buf[pos] = op << 16 | n;
    u32 *a = &buf[pos + 1];
    pos += 1 + n;
    return a;
}

static void cmd0(u32 op) { cmd(op, 0); }
static void cmd1(u32 op, u32 a) { cmd(op, 1)[0] = a; }
static void cmd2(u32 op, u32 a, u32 b) { u32 *p = cmd(op, 2); p[0] = a; p[1] = b; }
static void cmd3(u32 op, u32 a, u32 b, u32 c) { u32 *p = cmd(op, 3); p[0] = a; p[1] = b; p[2] = c; }
static void cmd4(u32 op, u32 a, u32 b, u32 c, u32 d) { u32 *p = cmd(op, 4); p[0] = a; p[1] = b; p[2] = c; p[3] = d; }
static void cmd5(u32 op, u32 a, u32 b, u32 c, u32 d, u32 e) {
    u32 *p = cmd(op, 5); p[0] = a; p[1] = b; p[2] = c; p[3] = d; p[4] = e;
}
static u32 fbits(float f) { union { float f; u32 u; } v; v.f = f; return v.u; }

/* GR_HINT_STWHINT : la TMU 0 a son propre 1/w (bit 1) ; sinon, celui du sommet. */
static u32 stw_hint;

static void vertex(u32 *p, const GrVertex *v) {
    p[0] = fbits(v->x); p[1] = fbits(v->y); p[2] = fbits(v->ooz); p[3] = fbits(v->oow);
    p[4] = fbits(v->r); p[5] = fbits(v->g); p[6] = fbits(v->b); p[7] = fbits(v->a);
    p[8] = fbits(v->tmuvtx[0].sow); p[9] = fbits(v->tmuvtx[0].tow); p[10] = fbits(stw_hint & 2 ? v->tmuvtx[0].oow : v->oow);
}

/* --- Mémoire de texture (mêmes tailles que src/glide/texmem.rs) -------- */
#define TMU_RAM (4u << 20)
static const u32 mem_size[4][9] = {
    {0x2000, 0x800, 0x200, 0x80, 0x20, 0x10, 0x8, 0x4, 0x4},
    {0x4000, 0x1000, 0x400, 0x100, 0x40, 0x10, 0x8, 0x4, 0x4},
    {0x8000, 0x2000, 0x800, 0x200, 0x80, 0x20, 0x8, 0x4, 0x4},
    {0x10000, 0x4000, 0x1000, 0x400, 0x100, 0x40, 0x10, 0x4, 0x4},
};
static u32 aspect_index(i32 a) { if (a < 0) a = 0; if (a > 6) a = 6; return a > 3 ? 6 - a : a; }
static u32 bpp(i32 format) { return format >= 8 ? 2 : 1; }
static void level_size(i32 lod, i32 aspect, u32 *w, u32 *h) {
    u32 big = 256u >> (lod < 0 ? 0 : lod > 8 ? 8 : lod);
    if (aspect <= 3) { *w = big; *h = big >> (3 - aspect); }
    else { *w = big >> (aspect - 3); *h = big; }
    if (*w == 0) *w = 1;
    if (*h == 0) *h = 1;
}
static u32 mem_required(i32 small, i32 large, i32 aspect, i32 format, u32 even_odd) {
    u32 a = aspect_index(aspect), texels = 0;
    for (i32 l = large; l <= small && l <= 8; l++)
        if (even_odd == 3 || (even_odd == 1 && !(l & 1)) || (even_odd == 2 && (l & 1)))
            texels += mem_size[a][l];
    return (texels * bpp(format) + 7) & ~7u;
}
/* Octets des niveaux large..small tels que le programme les fournit. */
static u32 host_bytes(i32 small, i32 large, i32 aspect, i32 format) {
    u32 n = 0, w, h;
    for (i32 l = large; l <= small && l <= 8; l++) { level_size(l, aspect, &w, &h); n += w * h; }
    return n * bpp(format);
}

/* --- Initialisation et fenêtre ----------------------------------------- */
static u32 present;   /* l'hôte sait rendre Glide (OpenGL) */
static u32 width = 640, height = 480;

API void grGlideInit(void) { cmd0(OP_INIT); present = flush(); }
API void grGlideShutdown(void) { cmd0(OP_SHUTDOWN); flush(); }
API void grGlideGetVersion(char version[80]) {
    const char *v = "2.43 Bare86";
    int i = 0;
    while (v[i]) { version[i] = v[i]; i++; }
    version[i] = 0;
}

/* Seuls les champs de la première carte sont écrits : la taille de la
 * structure varie selon le SDK (nombre de TMU), celle du programme peut être
 * plus petite que la nôtre. */
/* Les deux requêtes peuvent précéder grGlideInit (Turok compte les cartes
 * avec grSstQueryBoards avant toute initialisation) : l'hôte est alors
 * interrogé d'abord. */
API i32 grSstQueryHardware(GrHwConfiguration *hw) {
    if (!present) { cmd0(OP_INIT); present = flush(); }
    if (!present) { hw->num_sst = 0; return 0; }
    hw->num_sst = 1;
    hw->SSTs[0].type = 0; /* Voodoo Graphics */
    hw->SSTs[0].sstBoard.VoodooConfig.fbRam = 4;
    hw->SSTs[0].sstBoard.VoodooConfig.fbiRev = 2;
    hw->SSTs[0].sstBoard.VoodooConfig.nTexelfx = 1;
    hw->SSTs[0].sstBoard.VoodooConfig.sliDetect = 0;
    hw->SSTs[0].sstBoard.VoodooConfig.tmuConfig[0].tmuRev = 1;
    hw->SSTs[0].sstBoard.VoodooConfig.tmuConfig[0].tmuRam = 4;
    return 1;
}
API i32 grSstQueryBoards(GrHwConfiguration *hw) { return grSstQueryHardware(hw); }
API void grSstSelect(i32 which) { (void)which; }

static const u16 res_w[16] = {320, 320, 400, 512, 640, 640, 640, 640, 800, 960, 856, 512, 1024, 1280, 1600, 400};
static const u16 res_h[16] = {200, 240, 256, 384, 200, 350, 400, 480, 600, 720, 480, 256, 768, 1024, 1200, 300};

API i32 grSstWinOpen(u32 hwnd, i32 res, i32 refresh, i32 color_format, i32 origin, i32 ncol, i32 naux) {
    (void)hwnd; (void)refresh; (void)ncol; (void)naux;
    if (!present) return 0;
    if (res < 0 || res > 15) res = 7;
    width = res_w[res];
    height = res_h[res];
    cmd3(OP_WIN_OPEN, res, origin, color_format);
    return flush();
}
API void grSstWinClose(void) { cmd0(OP_WIN_CLOSE); flush(); }
API i32 grSstControl(u32 code) { (void)code; return 1; }
API void grSstIdle(void) { flush(); }
API i32 grSstIsBusy(void) { return 0; }
API u32 grSstStatus(void) { return 0x0FFFF03F; }
API u32 grSstVideoLine(void) { return 0; }
API i32 grSstVRetraceOn(void) { return 1; }
API u32 grSstScreenWidth(void) { return width; }
API u32 grSstScreenHeight(void) { return height; }
API void grSstOrigin(i32 origin) { cmd1(OP_ORIGIN, origin); }
API void grSstPerfStats(GrSstPerfStats_t *s) { memset(s, 0, sizeof *s); }
API void grSstResetPerfStats(void) {}
API void grResetTriStats(void) {}
API void grTriStats(u32 *in, u32 *out) { *in = 0; *out = 0; }
API void grHints(u32 type, u32 mask) {
    if (type == 0) stw_hint = mask;
}
API void grErrorSetCallback(void *fn) { (void)fn; }

/* --- Tampons ----------------------------------------------------------- */
API void grBufferClear(u32 color, u32 alpha, u32 depth) { cmd3(OP_CLEAR, color, alpha & 0xFF, depth & 0xFFFF); }
API void grBufferSwap(i32 interval) { cmd1(OP_SWAP, interval); flush(); }
API i32 grBufferNumPending(void) { return 0; }
API void grRenderBuffer(i32 b) { cmd1(OP_RENDER_BUFFER, b); }
API void grClipWindow(u32 minx, u32 miny, u32 maxx, u32 maxy) { cmd4(OP_CLIP, minx, miny, maxx, maxy); }

/* --- État de rendu ----------------------------------------------------- */
API void grColorCombine(i32 f, i32 fac, i32 local, i32 other, i32 inv) { cmd5(OP_COLOR_COMBINE, f, fac, local, other, inv); }
API void grAlphaCombine(i32 f, i32 fac, i32 local, i32 other, i32 inv) { cmd5(OP_ALPHA_COMBINE, f, fac, local, other, inv); }
API void grTexCombine(i32 tmu, i32 rf, i32 rfac, i32 af, i32 afac, i32 rinv, i32 ainv) {
    u32 *p = cmd(OP_TEX_COMBINE, 7);
    p[0] = tmu; p[1] = rf; p[2] = rfac; p[3] = af; p[4] = afac; p[5] = rinv; p[6] = ainv;
}
API void grTexCombineFunction(i32 tmu, i32 fnc) {
    /* Raccourcis de Glide 2 : 0 zéro, 1 décalque, 2 autre, 3 ajout, 4 produit… */
    static const u8 rgb[11][3] = {
        {0, 0, 0}, {1, 0, 0}, {3, 8, 0}, {4, 1, 0}, {3, 1, 0}, {6, 1, 0},
        {7, 4, 0}, {7, 12, 0}, {7, 5, 0}, {7, 13, 0}, {0, 0, 1},
    };
    if (fnc < 0 || fnc > 10) fnc = 1;
    grTexCombine(tmu, rgb[fnc][0], rgb[fnc][1], rgb[fnc][0], rgb[fnc][1], rgb[fnc][2], rgb[fnc][2]);
}
API void grAlphaBlendFunction(i32 rs, i32 rd, i32 as, i32 ad) { cmd4(OP_BLEND, rs, rd, as, ad); }
API void grAlphaTestFunction(i32 f) { cmd1(OP_ALPHA_TEST_FUNC, f); }
API void grAlphaTestReferenceValue(u32 v) { cmd1(OP_ALPHA_TEST_REF, v & 0xFF); }
API void grConstantColorValue(u32 c) { cmd1(OP_CONSTANT_COLOR, c); }
API void grCullMode(i32 m) { cmd1(OP_CULL, m); }
API void grDepthBiasLevel(i32 l) { cmd1(OP_DEPTH_BIAS, (u32)(u16)l); }
API void grDepthBufferFunction(i32 f) { cmd1(OP_DEPTH_FUNC, f); }
API void grDepthBufferMode(i32 m) { cmd1(OP_DEPTH_MODE, m); }
API void grDepthMask(i32 m) { cmd1(OP_DEPTH_MASK, m); }
API void grColorMask(i32 rgb, i32 a) { cmd2(OP_COLOR_MASK, rgb, a); }
API void grFogColorValue(u32 c) { cmd1(OP_FOG_COLOR, c); }
API void grFogMode(i32 m) { cmd1(OP_FOG_MODE, m); }
API void grFogTable(const u8 *t) {
    u32 *p = cmd(OP_FOG_TABLE, 16);
    memcpy(p, t, 64);
}
API void grDitherMode(i32 m) { (void)m; }
API void grChromakeyMode(i32 m) { cmd1(OP_CHROMA_MODE, m); }
API void grChromakeyValue(u32 v) { cmd1(OP_CHROMA_VALUE, v); }
API void grGammaCorrectionValue(float g) { cmd1(OP_GAMMA, fbits(g)); }
API void grDisableAllEffects(void) {
    grAlphaBlendFunction(4, 0, 4, 0);
    grAlphaTestFunction(7);
    grDepthBufferMode(0);
    grChromakeyMode(0);
    grFogMode(0);
}

/* État complet : l'hôte le garde, la DLL range son identifiant dans GrState. */
static u32 state_id;
API void grGlideGetState(GrState *s) {
    u32 id = ++state_id;
    memcpy(s->pad, &id, 4);
    cmd1(OP_SAVE_STATE, id);
}
API void grGlideSetState(const GrState *s) {
    u32 id;
    memcpy(&id, s->pad, 4);
    cmd1(OP_RESTORE_STATE, id);
}

/* --- Primitives -------------------------------------------------------- */
API void grDrawTriangle(const GrVertex *a, const GrVertex *b, const GrVertex *c) {
    u32 *p = cmd(OP_TRIANGLE, 3 * VERTEX_WORDS);
    vertex(p, a); vertex(p + VERTEX_WORDS, b); vertex(p + 2 * VERTEX_WORDS, c);
}
API void grDrawPlanarPolygonVertexList(i32 n, const GrVertex *v) {
    for (i32 i = 2; i < n; i++) grDrawTriangle(&v[0], &v[i - 1], &v[i]);
}
API void grDrawPolygonVertexList(i32 n, const GrVertex *v) { grDrawPlanarPolygonVertexList(n, v); }
API void grDrawLine(const GrVertex *a, const GrVertex *b) {
    u32 *p = cmd(OP_LINE, 2 * VERTEX_WORDS);
    vertex(p, a); vertex(p + VERTEX_WORDS, b);
}
API void grDrawPoint(const GrVertex *a) { vertex(cmd(OP_POINT, VERTEX_WORDS), a); }
API void grAADrawTriangle(const GrVertex *a, const GrVertex *b, const GrVertex *c, i32 ab, i32 bc, i32 ca) {
    (void)ab; (void)bc; (void)ca;
    grDrawTriangle(a, b, c);
}
API void grAADrawLine(const GrVertex *a, const GrVertex *b) { grDrawLine(a, b); }
API void grAADrawPoint(const GrVertex *a) { grDrawPoint(a); }

/* --- Textures ---------------------------------------------------------- */
API u32 grTexMinAddress(i32 tmu) { (void)tmu; return 0; }
API u32 grTexMaxAddress(i32 tmu) { (void)tmu; return TMU_RAM - 8; }
API u32 grTexCalcMemRequired(i32 small, i32 large, i32 aspect, i32 format) {
    return mem_required(small, large, aspect, format, 3);
}
API u32 grTexTextureMemRequired(u32 even_odd, GrTexInfo *info) {
    return mem_required(info->smallLod, info->largeLod, info->aspectRatio, info->format, even_odd);
}
API void grTexClampMode(i32 tmu, i32 s, i32 t) { cmd3(OP_TEX_CLAMP, tmu, s, t); }
API void grTexFilterMode(i32 tmu, i32 min, i32 mag) { cmd3(OP_TEX_FILTER, tmu, min, mag); }
API void grTexMipMapMode(i32 tmu, i32 mode, i32 blend) { cmd3(OP_TEX_MIPMAP, tmu, mode, blend); }
API void grTexLodBiasValue(i32 tmu, float bias) { (void)tmu; (void)bias; }
API void grTexSource(i32 tmu, u32 start, u32 even_odd, GrTexInfo *info) {
    u32 *p = cmd(OP_TEX_SOURCE, 7);
    p[0] = tmu; p[1] = start; p[2] = even_odd; p[3] = info->smallLod; p[4] = info->largeLod;
    p[5] = info->aspectRatio; p[6] = info->format;
}
API void grTexDownloadMipMap(i32 tmu, u32 start, u32 even_odd, GrTexInfo *info) {
    u32 bytes = host_bytes(info->smallLod, info->largeLod, info->aspectRatio, info->format);
    u32 *p = cmd(OP_TEX_DOWNLOAD, 8 + (bytes + 3) / 4);
    p[0] = tmu; p[1] = start; p[2] = even_odd; p[3] = info->smallLod; p[4] = info->largeLod;
    p[5] = info->aspectRatio; p[6] = info->format; p[7] = bytes;
    memcpy(p + 8, info->data, bytes);
}
API void grTexDownloadMipMapLevel(i32 tmu, u32 start, i32 lod, i32 large, i32 aspect, i32 format, u32 even_odd, void *data) {
    u32 w, h;
    level_size(lod, aspect, &w, &h);
    u32 bytes = w * h * bpp(format);
    u32 *p = cmd(OP_TEX_DOWNLOAD_PARTIAL, 10 + (bytes + 3) / 4);
    p[0] = tmu; p[1] = start; p[2] = lod; p[3] = large; p[4] = aspect; p[5] = format; p[6] = even_odd;
    p[7] = 0; p[8] = h - 1; p[9] = bytes;
    memcpy(p + 10, data, bytes);
}
API void grTexDownloadMipMapLevelPartial(i32 tmu, u32 start, i32 lod, i32 large, i32 aspect, i32 format, u32 even_odd,
                                        void *data, i32 first, i32 last) {
    u32 w, h;
    level_size(lod, aspect, &w, &h);
    if (first < 0) first = 0;
    if (last >= (i32)h) last = h - 1;
    if (last < first) return;
    u32 bytes = (u32)(last - first + 1) * w * bpp(format);
    u32 *p = cmd(OP_TEX_DOWNLOAD_PARTIAL, 10 + (bytes + 3) / 4);
    p[0] = tmu; p[1] = start; p[2] = lod; p[3] = large; p[4] = aspect; p[5] = format; p[6] = even_odd;
    p[7] = first; p[8] = last; p[9] = bytes;
    memcpy(p + 10, data, bytes);
}
API void grTexDownloadTable(i32 tmu, i32 type, void *data) {
    (void)tmu;
    /* Palette : 256 mots ; tables NCC : 48 mots (non rendues par l'hôte). */
    u32 n = type == 2 ? 256 : 48;
    u32 *p = cmd(OP_TEX_TABLE, 1 + n);
    p[0] = type;
    memcpy(p + 1, data, n * 4);
}
API void grTexDownloadTablePartial(i32 tmu, i32 type, void *data, i32 start, i32 end) {
    (void)start; (void)end;
    grTexDownloadTable(tmu, type, data);
}
API void grTexNCCTable(i32 tmu, i32 table) { (void)tmu; (void)table; }
API void grTexDetailControl(i32 tmu, i32 scale, i32 shift, float max) { (void)tmu; (void)scale; (void)shift; (void)max; }
API void grTexMultibase(i32 tmu, i32 enable) { (void)tmu; (void)enable; }

/* --- Brouillard (tables calculées par l'hôte) -------------------------- */
static void fog_generate(u32 kind, float a, float b, u8 *table) {
    u32 *p = cmd(OP_FOG_GENERATE, 4);
    p[0] = kind; p[1] = fbits(a); p[2] = fbits(b); p[3] = (u32)table;
    flush();
}
API void guFogGenerateExp(u8 *t, float density) { fog_generate(0, density, 0, t); }
API void guFogGenerateExp2(u8 *t, float density) { fog_generate(1, density, 0, t); }
API void guFogGenerateLinear(u8 *t, float near, float far) { fog_generate(2, near, far, t); }

/* --- Utilitaires gu* (bibliothèque de 3dfx, au-dessus de l'interface gr*) --
 * Combinaisons prêtes à l'emploi, et gestion de la mémoire de texture par
 * identifiants de mipmap (gutex.c de Glide 2.x) : allocation linéaire par
 * TMU, sans libération autre que guTexMemReset. */
API void guColorCombineFunction(i32 fnc) {
    /* fonction, facteur, local, autre, inversion (GR_COMBINE_*) */
    static const u8 cc[17][5] = {
        {0, 0, 0, 2, 0},  /* ZERO */
        {1, 0, 1, 2, 0},  /* CCRGB */
        {1, 0, 0, 2, 0},  /* ITRGB */
        {1, 0, 0, 2, 0},  /* ITRGB_DELTA0 */
        {3, 8, 1, 1, 0},  /* DECAL_TEXTURE */
        {3, 1, 1, 1, 0},  /* TEXTURE_TIMES_CCRGB */
        {3, 1, 0, 1, 0},  /* TEXTURE_TIMES_ITRGB */
        {3, 1, 0, 1, 0},  /* TEXTURE_TIMES_ITRGB_DELTA0 */
        {5, 1, 0, 1, 0},  /* TEXTURE_TIMES_ITRGB_ADD_ALPHA */
        {3, 3, 0, 1, 0},  /* TEXTURE_TIMES_ALPHA */
        {4, 3, 0, 1, 0},  /* TEXTURE_TIMES_ALPHA_ADD_ITRGB */
        {4, 8, 0, 1, 0},  /* TEXTURE_ADD_ITRGB */
        {6, 8, 0, 1, 0},  /* TEXTURE_SUB_ITRGB */
        {7, 4, 0, 2, 0},  /* CCRGB_BLEND_ITRGB_ON_TEXALPHA */
        {4, 3, 0, 1, 0},  /* DIFF_SPEC_A */
        {5, 1, 0, 1, 0},  /* DIFF_SPEC_B */
        {0, 0, 1, 2, 1},  /* ONE */
    };
    if (fnc < 0 || fnc > 16) return;
    grColorCombine(cc[fnc][0], cc[fnc][1], cc[fnc][2], cc[fnc][3], cc[fnc][4]);
}
API void guAlphaSource(i32 mode) {
    switch (mode) {
    case 0: grAlphaCombine(1, 0, 1, 2, 0); break;   /* CC_ALPHA */
    case 1: grAlphaCombine(1, 0, 0, 2, 0); break;   /* ITERATED_ALPHA */
    case 2: grAlphaCombine(3, 8, 1, 1, 0); break;   /* TEXTURE_ALPHA */
    case 3: grAlphaCombine(3, 1, 0, 1, 0); break;   /* TEXTURE_ALPHA_TIMES_ITERATED_ALPHA */
    }
}
API void guTexCombineFunction(i32 tmu, i32 fnc) { grTexCombineFunction(tmu, fnc); }
API float guFogTableIndexToW(i32 i) { return (float)(1u << (3 + (i >> 2))) / (float)(8 - (i & 3)); }

typedef struct { u8 yRGB[16]; i16 iRGB[4][3]; i16 qRGB[4][3]; u32 packed_data[12]; } GuNccTable;
/* GrMipMapInfo de glide.h (rendu tel quel par guTexGetMipMapInfo). */
typedef struct {
    i32 sst, valid, width, height, aspect_ratio;
    void *data;
    i32 format, mipmap_mode, magfilter_mode, minfilter_mode, s_clamp_mode, t_clamp_mode;
    u32 tLOD, tTextureMode, lod_bias;
    i32 lod_min, lod_max, tmu;
    u32 odd_even_mask, tmu_base_address;
    i32 trilinear;
    GuNccTable ncc_table;
} GrMipMapInfo;

#define MAX_MIPMAPS 1024
static GrMipMapInfo mipmaps[MAX_MIPMAPS];
static i32 mipmap_count;
static u32 tex_next[2];
static i32 tex_current[2] = {-1, -1};

static GrMipMapInfo *mipmap(i32 id) { return id >= 0 && id < mipmap_count && mipmaps[id].valid ? &mipmaps[id] : 0; }
static u32 mipmap_bytes(const GrMipMapInfo *m) {
    return mem_required(m->lod_min, m->lod_max, m->aspect_ratio, m->format, m->odd_even_mask);
}

API void guTexMemReset(void) {
    memset(mipmaps, 0, sizeof mipmaps);
    mipmap_count = 0;
    tex_next[0] = tex_next[1] = 0;
    tex_current[0] = tex_current[1] = -1;
}
API u32 guTexMemQueryAvail(i32 tmu) { return grTexMaxAddress(tmu) - tex_next[tmu & 1]; }
API i32 guTexAllocateMemory(i32 tmu, u8 odd_even, i32 width, i32 height, i32 format, i32 mm_mode, i32 smallest,
                            i32 largest, i32 aspect, i32 s_clamp, i32 t_clamp, i32 min_filter, i32 mag_filter,
                            float lod_bias, i32 trilinear) {
    if (mipmap_count >= MAX_MIPMAPS) return -1;
    if (mm_mode == 0) smallest = largest;   /* GR_MIPMAP_DISABLE : le seul grand niveau */
    GrMipMapInfo *m = &mipmaps[mipmap_count];
    memset(m, 0, sizeof *m);
    m->width = width; m->height = height; m->format = format; m->mipmap_mode = mm_mode;
    m->lod_min = smallest; m->lod_max = largest; m->aspect_ratio = aspect;
    m->s_clamp_mode = s_clamp; m->t_clamp_mode = t_clamp;
    m->minfilter_mode = min_filter; m->magfilter_mode = mag_filter;
    m->lod_bias = fbits(lod_bias); m->trilinear = trilinear;
    m->tmu = tmu & 1; m->odd_even_mask = odd_even ? odd_even : 3;
    u32 need = mipmap_bytes(m), start = tex_next[m->tmu];
    /* Une texture ne franchit pas la limite des 2 Mo (bancs de la TMU). */
    if (start < 0x200000 && start + need > 0x200000) start = 0x200000;
    if (start + need > grTexMaxAddress(tmu)) return -1;
    m->tmu_base_address = start;
    tex_next[m->tmu] = start + need;
    m->valid = 1;
    return mipmap_count++;
}
API i32 guTexChangeAttributes(i32 id, i32 width, i32 height, i32 format, i32 mm_mode, i32 smallest, i32 largest,
                              i32 aspect, i32 s_clamp, i32 t_clamp, i32 min_filter, i32 mag_filter) {
    GrMipMapInfo *m = mipmap(id);
    if (!m) return 0;
    GrMipMapInfo n = *m;
    if (width != -1) n.width = width;
    if (height != -1) n.height = height;
    if (format != -1) n.format = format;
    if (mm_mode != -1) n.mipmap_mode = mm_mode;
    if (smallest != -1) n.lod_min = smallest;
    if (largest != -1) n.lod_max = largest;
    if (aspect != -1) n.aspect_ratio = aspect;
    if (s_clamp != -1) n.s_clamp_mode = s_clamp;
    if (t_clamp != -1) n.t_clamp_mode = t_clamp;
    if (min_filter != -1) n.minfilter_mode = min_filter;
    if (mag_filter != -1) n.magfilter_mode = mag_filter;
    if (n.mipmap_mode == 0) n.lod_min = n.lod_max;
    if (mipmap_bytes(&n) > mipmap_bytes(m)) return 0;   /* plus de place que réservé */
    *m = n;
    return 1;
}
API GrMipMapInfo *guTexGetMipMapInfo(i32 id) { return mipmap(id); }
API i32 guTexGetCurrentMipMap(i32 tmu) { return tex_current[tmu & 1]; }
API void guTexDownloadMipMap(i32 id, const void *src, const GuNccTable *ncc) {
    GrMipMapInfo *m = mipmap(id);
    if (!m) return;
    if (ncc) m->ncc_table = *ncc;
    m->data = (void *)src;
    GrTexInfo info = {m->lod_min, m->lod_max, m->aspect_ratio, m->format, (void *)src};
    grTexDownloadMipMap(m->tmu, m->tmu_base_address, m->odd_even_mask, &info);
}
API void guTexDownloadMipMapLevel(i32 id, i32 lod, const void **src) {
    GrMipMapInfo *m = mipmap(id);
    if (!m) return;
    u32 w, h;
    level_size(lod, m->aspect_ratio, &w, &h);
    grTexDownloadMipMapLevel(m->tmu, m->tmu_base_address, lod, m->lod_max, m->aspect_ratio, m->format,
                             m->odd_even_mask, (void *)*src);
    *src = (const u8 *)*src + w * h * bpp(m->format);
}
API void guTexSource(i32 id) {
    GrMipMapInfo *m = mipmap(id);
    if (!m) return;
    union { u32 u; float f; } bias = {m->lod_bias};
    grTexClampMode(m->tmu, m->s_clamp_mode, m->t_clamp_mode);
    grTexMipMapMode(m->tmu, m->mipmap_mode, m->trilinear);
    grTexFilterMode(m->tmu, m->minfilter_mode, m->magfilter_mode);
    grTexLodBiasValue(m->tmu, bias.f);
    GrTexInfo info = {m->lod_min, m->lod_max, m->aspect_ratio, m->format, m->data};
    grTexSource(m->tmu, m->tmu_base_address, m->odd_even_mask, &info);
    tex_current[m->tmu] = id;
}

/* Le découpage à la fenêtre se fait chez l'hôte (ciseaux). */
API void guDrawTriangleWithClip(const GrVertex *a, const GrVertex *b, const GrVertex *c) { grDrawTriangle(a, b, c); }
API void guAADrawTriangleWithClip(const GrVertex *a, const GrVertex *b, const GrVertex *c) { grDrawTriangle(a, b, c); }
API void guDrawPolygonVertexListWithClip(i32 n, const GrVertex *v) { grDrawPlanarPolygonVertexList(n, v); }

/* --- Tampon d'image linéaire ------------------------------------------- */
/* Comme sur la Voodoo, une ligne du LFB fait toujours 1024 points, quelle que
 * soit la résolution : des jeux supposent ce pas (2048 octets) sans le lire. */
#define LFB_STRIDE 1024
static u8 lfb[LFB_STRIDE * 768 * 4];
static i32 lfb_write_mode = -1, lfb_origin, lfb_buffer;

API i32 grLfbLock(i32 type, i32 buffer, i32 write_mode, i32 origin, i32 pipeline, GrLfbInfo_t *info) {
    (void)pipeline;
    if (buffer > 1) return 0;   /* profondeur, alpha : non servis */
    if (write_mode == 0xFF) write_mode = 0;
    if (origin == 0xFF) origin = 0;
    u32 bytes = (write_mode == 4 || write_mode == 5) ? 4 : 2;
    /* Image courante (aussi pour une écriture : les points non écrits restent). */
    if (bytes == 2) {
        /* Dans le format d'écriture, pour que les points non écrits reviennent intacts. */
        u32 *p = cmd(OP_LFB_READ, 5);
        p[0] = buffer; p[1] = origin; p[2] = (u32)lfb; p[3] = LFB_STRIDE * 2;
        p[4] = (type & 1) && (write_mode == 1 || write_mode == 2) ? (u32)write_mode : 0;
        flush();
    }
    if (type & 1) { lfb_write_mode = write_mode; lfb_origin = origin; lfb_buffer = buffer; }
    info->lfbPtr = lfb;
    info->strideInBytes = LFB_STRIDE * bytes;
    info->writeMode = write_mode;
    info->origin = origin;
    return 1;
}
API i32 grLfbUnlock(i32 type, i32 buffer) {
    if ((type & 1) && lfb_write_mode >= 0 && buffer == lfb_buffer) {
        u32 bytes = (lfb_write_mode == 4 || lfb_write_mode == 5) ? 4 : 2;
        u32 *p = cmd(OP_LFB_WRITE, 5);
        p[0] = buffer; p[1] = lfb_write_mode; p[2] = lfb_origin; p[3] = (u32)lfb; p[4] = LFB_STRIDE * bytes;
        flush();
        lfb_write_mode = -1;
    }
    return 1;
}
API void grLfbConstantAlpha(u32 a) { (void)a; }
API void grLfbConstantDepth(u32 d) { (void)d; }
API void grLfbWriteColorFormat(i32 f) { (void)f; }
API void grLfbWriteColorSwizzle(i32 a, i32 b) { (void)a; (void)b; }
