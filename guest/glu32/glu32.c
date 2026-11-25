/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * glu32.dll de Bare86 : la bibliothèque utilitaire d'OpenGL (GLU 1.1) pour les programmes Win32 lancés par HX DOS
 * Extender. Au-dessus de l'opengl32.dll de Bare86 : rien que du calcul, puis des appels gl*. Écrite d'après la
 * spécification de GLU 1.2 (Silicon Graphics) ; aucun code de Microsoft ni de SGI.
 *
 * Faits : projections et vue (gluPerspective, gluLookAt, gluOrtho2D, gluPickMatrix, gluProject, gluUnProject),
 * quadriques (gluSphere, gluCylinder, gluDisk, gluPartialDisk), images (gluScaleImage, gluBuild1DMipmaps,
 * gluBuild2DMipmaps), gluErrorString, gluGetString.
 * Exportés mais sans effet : tessellateur de polygones (gluNewTess…) et surfaces NURBS (gluNewNurbsRenderer…), pour que
 * le chargement des programmes qui les importent n'échoue pas.
 *
 * Compilée sans bibliothèque C (guest/glu32/build.sh). Les lignes « GLUAPI » servent à produire glu32.def.
 */

typedef unsigned int u32;
typedef unsigned char u8;
typedef unsigned int GLenum;
typedef unsigned char GLboolean;
typedef int GLint;
typedef unsigned int GLuint;
typedef int GLsizei;
typedef float GLfloat;
typedef double GLdouble;
typedef void GLvoid;
typedef unsigned char GLubyte;
typedef unsigned short GLushort;

#define GLUAPI __stdcall
#define GL_API __stdcall

/* OpenGL 1.1, de l'opengl32.dll de Bare86 (bibliothèque d'import : opengl32.lib, produite par build.sh). */
void GL_API glBegin(GLenum mode);
void GL_API glEnd(void);
void GL_API glVertex3d(GLdouble x, GLdouble y, GLdouble z);
void GL_API glNormal3d(GLdouble x, GLdouble y, GLdouble z);
void GL_API glTexCoord2d(GLdouble s, GLdouble t);
void GL_API glFrustum(GLdouble l, GLdouble r, GLdouble b, GLdouble t, GLdouble n, GLdouble f);
void GL_API glOrtho(GLdouble l, GLdouble r, GLdouble b, GLdouble t, GLdouble n, GLdouble f);
void GL_API glMultMatrixd(const GLdouble *m);
void GL_API glTranslated(GLdouble x, GLdouble y, GLdouble z);
void GL_API glScaled(GLdouble x, GLdouble y, GLdouble z);
void GL_API glGetIntegerv(GLenum pname, GLint *params);
void GL_API glPixelStorei(GLenum pname, GLint param);
void GL_API glTexImage1D(GLenum target, GLint level, GLint internalformat, GLsizei width, GLint border, GLenum format, GLenum type, const GLvoid *pixels);
void GL_API glTexImage2D(GLenum target, GLint level, GLint internalformat, GLsizei width, GLsizei height, GLint border, GLenum format, GLenum type, const GLvoid *pixels);

/* Mémoire : le tas du processus (KERNEL32, de HX). */
void *__stdcall GetProcessHeap(void);
void *__stdcall HeapAlloc(void *heap, u32 flags, u32 size);
int __stdcall HeapFree(void *heap, u32 flags, void *p);

#define GL_POINTS 0
#define GL_LINES 1
#define GL_LINE_LOOP 2
#define GL_LINE_STRIP 3
#define GL_TRIANGLE_FAN 6
#define GL_QUAD_STRIP 8
#define GL_UNPACK_ROW_LENGTH 0x0CF2
#define GL_UNPACK_SKIP_ROWS 0x0CF3
#define GL_UNPACK_SKIP_PIXELS 0x0CF4
#define GL_UNPACK_ALIGNMENT 0x0CF5
#define GL_MAX_TEXTURE_SIZE 0x0D33
#define GL_BYTE 0x1400
#define GL_UNSIGNED_BYTE 0x1401
#define GL_FLOAT 0x1406

#define GLU_SMOOTH 100000
#define GLU_FLAT 100001
#define GLU_NONE 100002
#define GLU_POINT 100010
#define GLU_LINE 100011
#define GLU_FILL 100012
#define GLU_SILHOUETTE 100013
#define GLU_OUTSIDE 100020
#define GLU_INSIDE 100021
#define GLU_INVALID_ENUM 100900
#define GLU_INVALID_VALUE 100901
#define GLU_OUT_OF_MEMORY 100902

/* Sans bibliothèque C. */
int _fltused = 0;
void *memset(void *d, int c, unsigned n) { u8 *p = d; while (n--) *p++ = (u8)c; return d; }
void *memcpy(void *d, const void *s, unsigned n) { u8 *p = d; const u8 *q = s; while (n--) *p++ = *q++; return d; }
int __stdcall _DllMainCRTStartup(void *h, u32 reason, void *r) { (void)h; (void)reason; (void)r; return 1; }

#define PI 3.14159265358979323846

static double d_sqrt(double x) { double r; __asm__("fsqrt" : "=t"(r) : "0"(x)); return r; }
static double d_sin(double x) { __asm__("fsin" : "=t"(x) : "0"(x)); return x; }
static double d_cos(double x) { __asm__("fcos" : "=t"(x) : "0"(x)); return x; }
static double d_tan(double x) { return d_sin(x) / d_cos(x); }

static void *alloc(u32 n) { return HeapAlloc(GetProcessHeap(), 0, n ? n : 1); }
static void release(void *p) { if (p) HeapFree(GetProcessHeap(), 0, p); }

/* ------------------------------------------------------------------------------------------------------------------ */
/* Vue et projection                                                                                                  */

GLUAPI void gluPerspective(GLdouble fovy, GLdouble aspect, GLdouble zNear, GLdouble zFar)
{
    GLdouble ymax = zNear * d_tan(fovy * PI / 360.0), xmax = ymax * aspect;
    glFrustum(-xmax, xmax, -ymax, ymax, zNear, zFar);
}

GLUAPI void gluOrtho2D(GLdouble left, GLdouble right, GLdouble bottom, GLdouble top)
{
    glOrtho(left, right, bottom, top, -1.0, 1.0);
}

GLUAPI void gluLookAt(GLdouble ex, GLdouble ey, GLdouble ez, GLdouble cx, GLdouble cy, GLdouble cz, GLdouble ux, GLdouble uy, GLdouble uz)
{
    GLdouble fx = cx - ex, fy = cy - ey, fz = cz - ez, n = d_sqrt(fx * fx + fy * fy + fz * fz);
    if (n == 0.0) return;
    fx /= n; fy /= n; fz /= n;
    /* s = f × up, puis u = s × f */
    GLdouble sx = fy * uz - fz * uy, sy = fz * ux - fx * uz, sz = fx * uy - fy * ux;
    n = d_sqrt(sx * sx + sy * sy + sz * sz);
    if (n == 0.0) return;
    sx /= n; sy /= n; sz /= n;
    GLdouble tx = sy * fz - sz * fy, ty = sz * fx - sx * fz, tz = sx * fy - sy * fx;
    GLdouble m[16] = { sx, tx, -fx, 0, sy, ty, -fy, 0, sz, tz, -fz, 0, 0, 0, 0, 1 };
    glMultMatrixd(m);
    glTranslated(-ex, -ey, -ez);
}

GLUAPI void gluPickMatrix(GLdouble x, GLdouble y, GLdouble dx, GLdouble dy, GLint viewport[4])
{
    if (dx <= 0.0 || dy <= 0.0) return;
    glTranslated((viewport[2] - 2.0 * (x - viewport[0])) / dx, (viewport[3] - 2.0 * (y - viewport[1])) / dy, 0.0);
    glScaled(viewport[2] / dx, viewport[3] / dy, 1.0);
}

/* Produit de matrices 4×4 en colonnes (OpenGL) : r = a × b. */
static void mat_mul(const GLdouble *a, const GLdouble *b, GLdouble *r)
{
    for (int c = 0; c < 4; c++)
        for (int l = 0; l < 4; l++)
            r[c * 4 + l] = a[0 * 4 + l] * b[c * 4 + 0] + a[1 * 4 + l] * b[c * 4 + 1] + a[2 * 4 + l] * b[c * 4 + 2] + a[3 * 4 + l] * b[c * 4 + 3];
}

/* Inverse d'une matrice 4×4 par Gauss-Jordan avec pivot ; rend 0 si elle est singulière. */
static int mat_inv(const GLdouble *m, GLdouble *out)
{
    GLdouble a[4][8];
    for (int i = 0; i < 4; i++)
        for (int j = 0; j < 4; j++) {
            a[i][j] = m[j * 4 + i];
            a[i][4 + j] = i == j ? 1.0 : 0.0;
        }
    for (int c = 0; c < 4; c++) {
        int p = c;
        for (int i = c + 1; i < 4; i++)
            if ((a[i][c] < 0 ? -a[i][c] : a[i][c]) > (a[p][c] < 0 ? -a[p][c] : a[p][c])) p = i;
        if (a[p][c] == 0.0) return 0;
        if (p != c)
            for (int j = 0; j < 8; j++) { GLdouble t = a[c][j]; a[c][j] = a[p][j]; a[p][j] = t; }
        GLdouble d = a[c][c];
        for (int j = 0; j < 8; j++) a[c][j] /= d;
        for (int i = 0; i < 4; i++)
            if (i != c) {
                GLdouble f = a[i][c];
                for (int j = 0; j < 8; j++) a[i][j] -= f * a[c][j];
            }
    }
    for (int i = 0; i < 4; i++)
        for (int j = 0; j < 4; j++) out[j * 4 + i] = a[i][4 + j];
    return 1;
}

GLUAPI GLint gluProject(GLdouble ox, GLdouble oy, GLdouble oz, const GLdouble model[16], const GLdouble proj[16], const GLint view[4],
                        GLdouble *wx, GLdouble *wy, GLdouble *wz)
{
    GLdouble in[4] = { ox, oy, oz, 1.0 }, t[4], o[4];
    for (int i = 0; i < 4; i++) t[i] = model[i] * in[0] + model[4 + i] * in[1] + model[8 + i] * in[2] + model[12 + i] * in[3];
    for (int i = 0; i < 4; i++) o[i] = proj[i] * t[0] + proj[4 + i] * t[1] + proj[8 + i] * t[2] + proj[12 + i] * t[3];
    if (o[3] == 0.0) return 0;
    o[0] /= o[3]; o[1] /= o[3]; o[2] /= o[3];
    *wx = view[0] + (o[0] * 0.5 + 0.5) * view[2];
    *wy = view[1] + (o[1] * 0.5 + 0.5) * view[3];
    *wz = o[2] * 0.5 + 0.5;
    return 1;
}

GLUAPI GLint gluUnProject(GLdouble wx, GLdouble wy, GLdouble wz, const GLdouble model[16], const GLdouble proj[16], const GLint view[4],
                          GLdouble *ox, GLdouble *oy, GLdouble *oz)
{
    GLdouble pm[16], inv[16], in[4], o[4];
    mat_mul(proj, model, pm);
    if (!mat_inv(pm, inv)) return 0;
    in[0] = (wx - view[0]) / view[2] * 2.0 - 1.0;
    in[1] = (wy - view[1]) / view[3] * 2.0 - 1.0;
    in[2] = wz * 2.0 - 1.0;
    in[3] = 1.0;
    for (int i = 0; i < 4; i++) o[i] = inv[i] * in[0] + inv[4 + i] * in[1] + inv[8 + i] * in[2] + inv[12 + i] * in[3];
    if (o[3] == 0.0) return 0;
    *ox = o[0] / o[3]; *oy = o[1] / o[3]; *oz = o[2] / o[3];
    return 1;
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* Chaînes                                                                                                            */

GLUAPI const GLubyte *gluErrorString(GLenum code)
{
    switch (code) {
    case 0: return (const GLubyte *)"no error";
    case 0x500: case GLU_INVALID_ENUM: return (const GLubyte *)"invalid enumerant";
    case 0x501: case GLU_INVALID_VALUE: return (const GLubyte *)"invalid value";
    case 0x502: return (const GLubyte *)"invalid operation";
    case 0x503: return (const GLubyte *)"stack overflow";
    case 0x504: return (const GLubyte *)"stack underflow";
    case 0x505: case GLU_OUT_OF_MEMORY: return (const GLubyte *)"out of memory";
    case 100903: return (const GLubyte *)"incompatible gl version";
    default: return 0;
    }
}

GLUAPI const unsigned short *gluErrorUnicodeStringEXT(GLenum code)
{
    static unsigned short buf[40];
    const GLubyte *s = gluErrorString(code);
    if (!s) return 0;
    int i = 0;
    for (; s[i] && i < 39; i++) buf[i] = s[i];
    buf[i] = 0;
    return buf;
}

GLUAPI const GLubyte *gluGetString(GLenum name)
{
    if (name == 100800) return (const GLubyte *)"1.1 Bare86";
    if (name == 100801) return (const GLubyte *)"GLU_EXT_object_space_tess";
    return 0;
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* Quadriques                                                                                                         */

typedef struct {
    int used, draw_style, normals, orientation, texture;
} Quadric;

static Quadric quadrics[64];

GLUAPI Quadric *gluNewQuadric(void)
{
    for (int i = 0; i < 64; i++)
        if (!quadrics[i].used) {
            quadrics[i].used = 1;
            quadrics[i].draw_style = GLU_FILL;
            quadrics[i].normals = GLU_SMOOTH;
            quadrics[i].orientation = GLU_OUTSIDE;
            quadrics[i].texture = 0;
            return &quadrics[i];
        }
    return 0;
}

GLUAPI void gluDeleteQuadric(Quadric *q) { if (q) q->used = 0; }
GLUAPI void gluQuadricDrawStyle(Quadric *q, GLenum style) { if (q) q->draw_style = (int)style; }
GLUAPI void gluQuadricNormals(Quadric *q, GLenum normals) { if (q) q->normals = (int)normals; }
GLUAPI void gluQuadricOrientation(Quadric *q, GLenum orientation) { if (q) q->orientation = (int)orientation; }
GLUAPI void gluQuadricTexture(Quadric *q, GLboolean texture) { if (q) q->texture = texture != 0; }
GLUAPI void gluQuadricCallback(Quadric *q, GLenum which, void (*fn)(void)) { (void)q; (void)which; (void)fn; }

static void qnormal(const Quadric *q, GLdouble x, GLdouble y, GLdouble z)
{
    if (q->normals != GLU_NONE) glNormal3d(x, y, z);
}

static void qtex(const Quadric *q, GLdouble s, GLdouble t)
{
    if (q->texture) glTexCoord2d(s, t);
}

GLUAPI void gluSphere(Quadric *q, GLdouble radius, GLint slices, GLint stacks)
{
    if (!q || slices < 2 || stacks < 1 || radius < 0.0) return;
    GLdouble nsign = q->orientation == GLU_INSIDE ? -1.0 : 1.0;
    GLdouble drho = PI / stacks, dtheta = 2.0 * PI / slices;
    if (q->draw_style == GLU_FILL) {
        if (!q->texture) { /* pôle +Z en éventail */
            glBegin(GL_TRIANGLE_FAN);
            qnormal(q, 0, 0, 1);
            glVertex3d(0.0, 0.0, nsign * radius);
            for (int j = 0; j <= slices; j++) {
                GLdouble theta = j == slices ? 0.0 : j * dtheta;
                GLdouble x = -d_sin(theta) * d_sin(drho), y = d_cos(theta) * d_sin(drho), z = nsign * d_cos(drho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                glVertex3d(x * radius, y * radius, z * radius);
            }
            glEnd();
        }
        GLdouble ds = 1.0 / slices, dt = 1.0 / stacks, t = 1.0;
        int imin = q->texture ? 0 : 1, imax = q->texture ? stacks : stacks - 1;
        for (int i = imin; i < imax; i++) {
            GLdouble rho = i * drho, s = 0.0;
            glBegin(GL_QUAD_STRIP);
            for (int j = 0; j <= slices; j++) {
                GLdouble theta = j == slices ? 0.0 : j * dtheta;
                GLdouble x = -d_sin(theta) * d_sin(rho), y = d_cos(theta) * d_sin(rho), z = nsign * d_cos(rho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                qtex(q, s, t);
                glVertex3d(x * radius, y * radius, z * radius);
                x = -d_sin(theta) * d_sin(rho + drho); y = d_cos(theta) * d_sin(rho + drho); z = nsign * d_cos(rho + drho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                qtex(q, s, t - dt);
                s += ds;
                glVertex3d(x * radius, y * radius, z * radius);
            }
            glEnd();
            t -= dt;
        }
        if (!q->texture) { /* pôle -Z */
            glBegin(GL_TRIANGLE_FAN);
            qnormal(q, 0, 0, -1);
            glVertex3d(0.0, 0.0, -radius * nsign);
            GLdouble rho = PI - drho;
            for (int j = slices; j >= 0; j--) {
                GLdouble theta = j == slices ? 0.0 : j * dtheta;
                GLdouble x = -d_sin(theta) * d_sin(rho), y = d_cos(theta) * d_sin(rho), z = nsign * d_cos(rho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                glVertex3d(x * radius, y * radius, z * radius);
            }
            glEnd();
        }
    } else if (q->draw_style == GLU_LINE || q->draw_style == GLU_SILHOUETTE) {
        for (int i = 1; i < stacks; i++) {
            GLdouble rho = i * drho;
            glBegin(GL_LINE_LOOP);
            for (int j = 0; j < slices; j++) {
                GLdouble theta = j * dtheta;
                GLdouble x = d_cos(theta) * d_sin(rho), y = d_sin(theta) * d_sin(rho), z = d_cos(rho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                glVertex3d(x * radius, y * radius, z * radius);
            }
            glEnd();
        }
        for (int j = 0; j < slices; j++) {
            GLdouble theta = j * dtheta;
            glBegin(GL_LINE_STRIP);
            for (int i = 0; i <= stacks; i++) {
                GLdouble rho = i * drho;
                GLdouble x = d_cos(theta) * d_sin(rho), y = d_sin(theta) * d_sin(rho), z = d_cos(rho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                glVertex3d(x * radius, y * radius, z * radius);
            }
            glEnd();
        }
    } else { /* GLU_POINT */
        glBegin(GL_POINTS);
        qnormal(q, 0, 0, nsign);
        glVertex3d(0.0, 0.0, radius);
        glVertex3d(0.0, 0.0, -radius);
        for (int i = 1; i < stacks; i++) {
            GLdouble rho = i * drho;
            for (int j = 0; j < slices; j++) {
                GLdouble theta = j * dtheta;
                GLdouble x = d_cos(theta) * d_sin(rho), y = d_sin(theta) * d_sin(rho), z = d_cos(rho);
                qnormal(q, x * nsign, y * nsign, z * nsign);
                glVertex3d(x * radius, y * radius, z * radius);
            }
        }
        glEnd();
    }
}

GLUAPI void gluCylinder(Quadric *q, GLdouble base, GLdouble top, GLdouble height, GLint slices, GLint stacks)
{
    if (!q || slices < 2 || stacks < 1 || base < 0.0 || top < 0.0 || height < 0.0) return;
    GLdouble nsign = q->orientation == GLU_INSIDE ? -1.0 : 1.0;
    GLdouble da = 2.0 * PI / slices, dr = (top - base) / stacks, dz = height / stacks;
    GLdouble nz = height != 0.0 ? (base - top) / height : 0.0, inv = 1.0 / d_sqrt(1.0 + nz * nz);
    if (q->draw_style == GLU_POINT) {
        glBegin(GL_POINTS);
        for (int i = 0; i < slices; i++) {
            GLdouble x = d_cos(i * da), y = d_sin(i * da), z = 0.0, r = base;
            qnormal(q, x * inv * nsign, y * inv * nsign, nz * inv * nsign);
            for (int j = 0; j <= stacks; j++) {
                glVertex3d(x * r, y * r, z);
                z += dz; r += dr;
            }
        }
        glEnd();
    } else if (q->draw_style == GLU_LINE || q->draw_style == GLU_SILHOUETTE) {
        for (int j = 0; j <= stacks; j++) {
            if (q->draw_style == GLU_SILHOUETTE && j != 0 && j != stacks) continue;
            GLdouble z = j * dz, r = base + j * dr;
            glBegin(GL_LINE_LOOP);
            for (int i = 0; i < slices; i++) {
                GLdouble x = d_cos(i * da), y = d_sin(i * da);
                qnormal(q, x * inv * nsign, y * inv * nsign, nz * inv * nsign);
                glVertex3d(x * r, y * r, z);
            }
            glEnd();
        }
        for (int i = 0; i < slices; i++) {
            GLdouble x = d_cos(i * da), y = d_sin(i * da);
            glBegin(GL_LINE_STRIP);
            for (int j = 0; j <= stacks; j++) {
                GLdouble r = base + j * dr;
                qnormal(q, x * inv * nsign, y * inv * nsign, nz * inv * nsign);
                glVertex3d(x * r, y * r, j * dz);
            }
            glEnd();
        }
    } else { /* GLU_FILL */
        GLdouble ds = 1.0 / slices, dt = 1.0 / stacks, t = 0.0, z = 0.0, r = base;
        for (int j = 0; j < stacks; j++) {
            GLdouble s = 0.0;
            glBegin(GL_QUAD_STRIP);
            for (int i = 0; i <= slices; i++) {
                GLdouble x = i == slices ? 0.0 : d_sin(i * da), y = i == slices ? 1.0 : d_cos(i * da);
                if (q->normals != GLU_NONE) {
                    if (nsign == 1.0) glNormal3d(x * inv, y * inv, nz * inv);
                    else glNormal3d(-x * inv, -y * inv, -nz * inv);
                }
                if (nsign == 1.0) {
                    qtex(q, s, t);
                    glVertex3d(x * r, y * r, z);
                    qtex(q, s, t + dt);
                    glVertex3d(x * (r + dr), y * (r + dr), z + dz);
                } else {
                    qtex(q, s, t);
                    glVertex3d(x * (r + dr), y * (r + dr), z + dz);
                    qtex(q, s, t + dt);
                    glVertex3d(x * r, y * r, z);
                }
                s += ds;
            }
            glEnd();
            r += dr; t += dt; z += dz;
        }
    }
}

GLUAPI void gluDisk(Quadric *q, GLdouble inner, GLdouble outer, GLint slices, GLint loops)
{
    if (!q || slices < 2 || loops < 1 || outer <= 0.0 || inner < 0.0 || inner > outer) return;
    GLdouble nsign = q->orientation == GLU_INSIDE ? -1.0 : 1.0;
    GLdouble da = 2.0 * PI / slices, dr = (outer - inner) / loops;
    qnormal(q, 0, 0, nsign);
    if (q->draw_style == GLU_FILL) {
        GLdouble dtc = 2.0 * outer, r1 = inner;
        for (int l = 0; l < loops; l++) {
            GLdouble r2 = r1 + dr;
            glBegin(GL_QUAD_STRIP);
            for (int s = 0; s <= slices; s++) {
                GLdouble a = s == slices ? 0.0 : s * da, sa = d_sin(a), ca = d_cos(a);
                if (nsign == 1.0) {
                    qtex(q, 0.5 + sa * r2 / dtc, 0.5 + ca * r2 / dtc);
                    glVertex3d(r2 * sa, r2 * ca, 0.0);
                    qtex(q, 0.5 + sa * r1 / dtc, 0.5 + ca * r1 / dtc);
                    glVertex3d(r1 * sa, r1 * ca, 0.0);
                } else {
                    qtex(q, 0.5 + sa * r1 / dtc, 0.5 + ca * r1 / dtc);
                    glVertex3d(r1 * sa, r1 * ca, 0.0);
                    qtex(q, 0.5 + sa * r2 / dtc, 0.5 + ca * r2 / dtc);
                    glVertex3d(r2 * sa, r2 * ca, 0.0);
                }
            }
            glEnd();
            r1 = r2;
        }
    } else if (q->draw_style == GLU_LINE) {
        for (int l = 0; l <= loops; l++) {
            GLdouble r = inner + l * dr;
            glBegin(GL_LINE_LOOP);
            for (int s = 0; s < slices; s++) glVertex3d(r * d_sin(s * da), r * d_cos(s * da), 0.0);
            glEnd();
        }
        for (int s = 0; s < slices; s++) {
            GLdouble sa = d_sin(s * da), ca = d_cos(s * da);
            glBegin(GL_LINE_STRIP);
            for (int l = 0; l <= loops; l++) {
                GLdouble r = inner + l * dr;
                glVertex3d(r * sa, r * ca, 0.0);
            }
            glEnd();
        }
    } else if (q->draw_style == GLU_SILHOUETTE) {
        for (int k = 0; k < 2; k++) {
            GLdouble r = k ? outer : inner;
            if (k == 0 && inner == 0.0) continue;
            glBegin(GL_LINE_LOOP);
            for (int s = 0; s < slices; s++) glVertex3d(r * d_sin(s * da), r * d_cos(s * da), 0.0);
            glEnd();
        }
    } else { /* GLU_POINT */
        glBegin(GL_POINTS);
        for (int s = 0; s < slices; s++) {
            GLdouble sa = d_sin(s * da), ca = d_cos(s * da);
            for (int l = 0; l <= loops; l++) {
                GLdouble r = inner + l * dr;
                glVertex3d(r * sa, r * ca, 0.0);
            }
        }
        glEnd();
    }
}

/* Secteur de disque : angles en degrés, 0° vers +Y, croissants dans le sens des aiguilles d'une montre. */
GLUAPI void gluPartialDisk(Quadric *q, GLdouble inner, GLdouble outer, GLint slices, GLint loops, GLdouble start, GLdouble sweep)
{
    if (!q || slices < 2 || loops < 1 || outer <= 0.0 || inner < 0.0 || inner > outer) return;
    if (sweep < -360.0) sweep = 360.0;
    if (sweep > 360.0) sweep = 360.0;
    GLdouble nsign = q->orientation == GLU_INSIDE ? -1.0 : 1.0;
    if (sweep < 0.0) { start += sweep; sweep = -sweep; }
    GLdouble da = sweep * PI / 180.0 / slices, a0 = start * PI / 180.0, dr = (outer - inner) / loops, dtc = 2.0 * outer;
    qnormal(q, 0, 0, nsign);
    GLdouble r1 = inner;
    for (int l = 0; l < loops; l++) {
        GLdouble r2 = r1 + dr;
        if (q->draw_style == GLU_FILL || q->draw_style == GLU_POINT) glBegin(q->draw_style == GLU_FILL ? GL_QUAD_STRIP : GL_POINTS);
        else glBegin(GL_LINE_STRIP);
        for (int s = 0; s <= slices; s++) {
            GLdouble a = a0 + s * da, sa = d_sin(a), ca = d_cos(a);
            if (nsign == 1.0 || q->draw_style != GLU_FILL) {
                qtex(q, 0.5 + sa * r2 / dtc, 0.5 + ca * r2 / dtc);
                glVertex3d(r2 * sa, r2 * ca, 0.0);
                qtex(q, 0.5 + sa * r1 / dtc, 0.5 + ca * r1 / dtc);
                glVertex3d(r1 * sa, r1 * ca, 0.0);
            } else {
                qtex(q, 0.5 + sa * r1 / dtc, 0.5 + ca * r1 / dtc);
                glVertex3d(r1 * sa, r1 * ca, 0.0);
                qtex(q, 0.5 + sa * r2 / dtc, 0.5 + ca * r2 / dtc);
                glVertex3d(r2 * sa, r2 * ca, 0.0);
            }
        }
        glEnd();
        r1 = r2;
    }
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* Images : gluScaleImage, gluBuild1DMipmaps, gluBuild2DMipmaps                                                       */

#define GL_COLOR_INDEX 0x1900
#define GL_RED 0x1903
#define GL_GREEN 0x1904
#define GL_BLUE 0x1905
#define GL_ALPHA 0x1906
#define GL_RGB 0x1907
#define GL_RGBA 0x1908
#define GL_LUMINANCE 0x1909
#define GL_LUMINANCE_ALPHA 0x190A
#define GL_ABGR_EXT 0x8000
#define GL_BGR_EXT 0x80E0
#define GL_BGRA_EXT 0x80E1
#define GL_UNSIGNED_SHORT 0x1403
#define GL_SHORT 0x1402

static int components(GLenum format)
{
    switch (format) {
    case GL_RED: case GL_GREEN: case GL_BLUE: case GL_ALPHA: case GL_LUMINANCE: return 1;
    case GL_LUMINANCE_ALPHA: return 2;
    case GL_RGB: case GL_BGR_EXT: return 3;
    case GL_RGBA: case GL_BGRA_EXT: case GL_ABGR_EXT: return 4;
    default: return 0;
    }
}

static int type_size(GLenum type)
{
    switch (type) {
    case GL_UNSIGNED_BYTE: case GL_BYTE: return 1;
    case GL_UNSIGNED_SHORT: case GL_SHORT: return 2;
    case GL_FLOAT: return 4;
    default: return 0;
    }
}

/* Lit une valeur (octet, mot, flottant) en flottant 0..1 (les entiers signés : -1..1). */
static GLfloat get_value(const u8 *p, GLenum type)
{
    switch (type) {
    case GL_UNSIGNED_BYTE: return p[0] / 255.0f;
    case GL_BYTE: { signed char v = (signed char)p[0]; return v < 0 ? v / 128.0f : v / 127.0f; }
    case GL_UNSIGNED_SHORT: return (p[0] | (p[1] << 8)) / 65535.0f;
    case GL_SHORT: { short v = (short)(p[0] | (p[1] << 8)); return v < 0 ? v / 32768.0f : v / 32767.0f; }
    default: { GLfloat f; memcpy(&f, p, 4); return f; }
    }
}

static void put_value(u8 *p, GLenum type, GLfloat f)
{
    if (type == GL_FLOAT) { memcpy(p, &f, 4); return; }
    if (f < 0.0f) f = type == GL_UNSIGNED_BYTE || type == GL_UNSIGNED_SHORT ? 0.0f : f < -1.0f ? -1.0f : f;
    if (f > 1.0f) f = 1.0f;
    switch (type) {
    case GL_UNSIGNED_BYTE: p[0] = (u8)(f * 255.0f + 0.5f); break;
    case GL_BYTE: p[0] = (u8)(signed char)(f * 127.0f + (f < 0 ? -0.5f : 0.5f)); break;
    case GL_UNSIGNED_SHORT: { u32 v = (u32)(f * 65535.0f + 0.5f); p[0] = (u8)v; p[1] = (u8)(v >> 8); break; }
    default: { int v = (int)(f * 32767.0f + (f < 0 ? -0.5f : 0.5f)); p[0] = (u8)v; p[1] = (u8)(v >> 8); break; }
    }
}

/* Image décompactée en flottants (w × h × n), selon l'état de lecture des pixels d'OpenGL (rangées, alignement). */
static GLfloat *load_image(GLint w, GLint h, int n, GLenum type, const void *data)
{
    GLint rowlen = 0, align = 0, skip_rows = 0, skip_pixels = 0;
    glGetIntegerv(GL_UNPACK_ROW_LENGTH, &rowlen);
    glGetIntegerv(GL_UNPACK_ALIGNMENT, &align);
    glGetIntegerv(GL_UNPACK_SKIP_ROWS, &skip_rows);
    glGetIntegerv(GL_UNPACK_SKIP_PIXELS, &skip_pixels);
    if (align != 1 && align != 2 && align != 4 && align != 8) align = 4;
    int esize = type_size(type);
    GLint pixels_per_row = rowlen > 0 ? rowlen : w;
    GLint row_bytes = pixels_per_row * n * esize;
    row_bytes = (row_bytes + align - 1) / align * align;
    GLfloat *out = alloc((u32)w * h * n * sizeof(GLfloat));
    if (!out) return 0;
    const u8 *base = (const u8 *)data + (u32)skip_rows * row_bytes + (u32)skip_pixels * n * esize;
    for (GLint y = 0; y < h; y++)
        for (GLint x = 0; x < w * n; x++) out[(u32)y * w * n + x] = get_value(base + (u32)y * row_bytes + (u32)x * esize, type);
    return out;
}

/* Ré-échantillonne une rangée : réduction par moyenne pondérée de la surface couverte, agrandissement linéaire. */
static void resample_line(const GLfloat *src, int sn, int sstride, GLfloat *dst, int dn, int dstride, int n)
{
    for (int c = 0; c < n; c++) {
        for (int i = 0; i < dn; i++) {
            GLfloat v;
            if (dn >= sn) { /* linéaire entre centres de pixels */
                GLfloat pos = (i + 0.5f) * sn / dn - 0.5f;
                int i0 = (int)(pos < 0 ? 0 : pos);
                if (pos < 0) pos = 0;
                int i1 = i0 + 1 < sn ? i0 + 1 : i0;
                GLfloat f = pos - i0;
                if (f > 1.0f) f = 1.0f;
                v = src[(u32)i0 * sstride + c] * (1.0f - f) + src[(u32)i1 * sstride + c] * f;
            } else {
                GLfloat a = (GLfloat)i * sn / dn, b = (GLfloat)(i + 1) * sn / dn, sum = 0.0f;
                int k0 = (int)a;
                for (int k = k0; k < sn && k < b; k++) {
                    GLfloat lo = k > a ? (GLfloat)k : a, hi = k + 1 < b ? (GLfloat)(k + 1) : b;
                    if (hi > lo) sum += src[(u32)k * sstride + c] * (hi - lo);
                }
                v = sum / (b - a);
            }
            dst[(u32)i * dstride + c] = v;
        }
    }
}

/* Image flottante w×h → dw×dh (nouveau tampon). */
static GLfloat *scale_float(const GLfloat *src, int w, int h, int dw, int dh, int n)
{
    if (w == dw && h == dh) {
        GLfloat *copy = alloc((u32)w * h * n * sizeof(GLfloat));
        if (copy) memcpy(copy, src, (u32)w * h * n * sizeof(GLfloat));
        return copy;
    }
    GLfloat *tmp = alloc((u32)dw * h * n * sizeof(GLfloat)), *out = alloc((u32)dw * dh * n * sizeof(GLfloat));
    if (!tmp || !out) { release(tmp); release(out); return 0; }
    for (int y = 0; y < h; y++) resample_line(src + (u32)y * w * n, w, n, tmp + (u32)y * dw * n, dw, n, n);
    for (int x = 0; x < dw; x++)
        for (int c = 0; c < n; c++) {
            GLfloat *col = out + (u32)x * n + c;
            const GLfloat *scol = tmp + (u32)x * n + c;
            /* colonne : pas de dw*n entre deux rangées */
            for (int i = 0; i < dh; i++) {
                GLfloat v;
                if (dh >= h) {
                    GLfloat pos = (i + 0.5f) * h / dh - 0.5f;
                    if (pos < 0) pos = 0;
                    int i0 = (int)pos, i1 = i0 + 1 < h ? i0 + 1 : i0;
                    GLfloat f = pos - i0;
                    v = scol[(u32)i0 * dw * n] * (1.0f - f) + scol[(u32)i1 * dw * n] * f;
                } else {
                    GLfloat a = (GLfloat)i * h / dh, b = (GLfloat)(i + 1) * h / dh, sum = 0.0f;
                    for (int k = (int)a; k < h && k < b; k++) {
                        GLfloat lo = k > a ? (GLfloat)k : a, hi = k + 1 < b ? (GLfloat)(k + 1) : b;
                        if (hi > lo) sum += scol[(u32)k * dw * n] * (hi - lo);
                    }
                    v = sum / (b - a);
                }
                col[(u32)i * dw * n] = v;
            }
        }
    release(tmp);
    return out;
}

static void store_image(const GLfloat *src, int count, GLenum type, void *out)
{
    int esize = type_size(type);
    for (int i = 0; i < count; i++) put_value((u8 *)out + (u32)i * esize, type, src[i]);
}

GLUAPI GLint gluScaleImage(GLenum format, GLint win, GLint hin, GLenum typein, const void *datain, GLint wout, GLint hout, GLenum typeout, void *dataout)
{
    int n = components(format);
    if (win < 1 || hin < 1 || wout < 1 || hout < 1) return GLU_INVALID_VALUE;
    if (!n || !type_size(typein) || !type_size(typeout)) return GLU_INVALID_ENUM;
    GLfloat *in = load_image(win, hin, n, typein, datain);
    if (!in) return GLU_OUT_OF_MEMORY;
    GLfloat *out = scale_float(in, win, hin, wout, hout, n);
    release(in);
    if (!out) return GLU_OUT_OF_MEMORY;
    store_image(out, wout * hout * n, typeout, dataout);
    release(out);
    return 0;
}

/* Puissance de deux la plus proche (celle du dessous à égalité), au plus `max`. */
static int nearest_pow2(int v, int max)
{
    int lo = 1;
    while (lo * 2 <= v) lo *= 2;
    int p = (lo * 2 - v < v - lo) ? lo * 2 : lo;
    return p > max ? max : p;
}

/* Remplit tous les niveaux d'une texture 1D (h == 1, target 1D) ou 2D à partir de l'image `data`. */
static GLint build_mipmaps(int dim, GLenum target, GLint internal, GLint w, GLint h, GLenum format, GLenum type, const void *data)
{
    int n = components(format);
    if (w < 1 || h < 1) return GLU_INVALID_VALUE;
    if (!n || !type_size(type)) return GLU_INVALID_ENUM;
    GLint max = 0;
    glGetIntegerv(GL_MAX_TEXTURE_SIZE, &max);
    if (max < 1) max = 256;
    GLint save_row = 0, save_align = 0, save_sr = 0, save_sp = 0;
    GLfloat *src = load_image(w, h, n, type, data);
    if (!src) return GLU_OUT_OF_MEMORY;
    GLint cw = nearest_pow2(w, max), ch = dim == 1 ? 1 : nearest_pow2(h, max);
    GLfloat *cur = scale_float(src, w, h, cw, ch, n);
    release(src);
    if (!cur) return GLU_OUT_OF_MEMORY;
    glGetIntegerv(GL_UNPACK_ROW_LENGTH, &save_row);
    glGetIntegerv(GL_UNPACK_ALIGNMENT, &save_align);
    glGetIntegerv(GL_UNPACK_SKIP_ROWS, &save_sr);
    glGetIntegerv(GL_UNPACK_SKIP_PIXELS, &save_sp);
    glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
    glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
    glPixelStorei(GL_UNPACK_SKIP_ROWS, 0);
    glPixelStorei(GL_UNPACK_SKIP_PIXELS, 0);
    GLint result = 0;
    for (int level = 0;; level++) {
        void *bytes = alloc((u32)cw * ch * n * type_size(type));
        if (!bytes) { result = GLU_OUT_OF_MEMORY; break; }
        store_image(cur, cw * ch * n, type, bytes);
        if (dim == 1) glTexImage1D(target, level, internal, cw, 0, format, type, bytes);
        else glTexImage2D(target, level, internal, cw, ch, 0, format, type, bytes);
        release(bytes);
        if (cw == 1 && ch == 1) break;
        GLint nw = cw > 1 ? cw / 2 : 1, nh = ch > 1 ? ch / 2 : 1;
        GLfloat *next = scale_float(cur, cw, ch, nw, nh, n);
        release(cur);
        cur = next;
        if (!cur) { result = GLU_OUT_OF_MEMORY; break; }
        cw = nw; ch = nh;
    }
    release(cur);
    glPixelStorei(GL_UNPACK_ROW_LENGTH, save_row);
    glPixelStorei(GL_UNPACK_ALIGNMENT, save_align ? save_align : 4);
    glPixelStorei(GL_UNPACK_SKIP_ROWS, save_sr);
    glPixelStorei(GL_UNPACK_SKIP_PIXELS, save_sp);
    return result;
}

GLUAPI GLint gluBuild1DMipmaps(GLenum target, GLint internal, GLint width, GLenum format, GLenum type, const void *data)
{
    return build_mipmaps(1, target, internal, width, 1, format, type, data);
}

GLUAPI GLint gluBuild2DMipmaps(GLenum target, GLint internal, GLint width, GLint height, GLenum format, GLenum type, const void *data)
{
    return build_mipmaps(2, target, internal, width, height, format, type, data);
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* Exportés sans effet : tessellateur de polygones et NURBS                                                           */

static char stub_objects[16][16];
static int stub_next;

static void *stub_new(void) { return stub_next < 16 ? stub_objects[stub_next++] : stub_objects[15]; }

GLUAPI void *gluNewTess(void) { return stub_new(); }
GLUAPI void gluDeleteTess(void *t) { (void)t; }
GLUAPI void gluTessBeginPolygon(void *t, void *data) { (void)t; (void)data; }
GLUAPI void gluTessBeginContour(void *t) { (void)t; }
GLUAPI void gluTessVertex(void *t, GLdouble *coords, void *data) { (void)t; (void)coords; (void)data; }
GLUAPI void gluTessEndContour(void *t) { (void)t; }
GLUAPI void gluTessEndPolygon(void *t) { (void)t; }
GLUAPI void gluTessCallback(void *t, GLenum which, void (*fn)(void)) { (void)t; (void)which; (void)fn; }
GLUAPI void gluTessProperty(void *t, GLenum which, GLdouble value) { (void)t; (void)which; (void)value; }
GLUAPI void gluGetTessProperty(void *t, GLenum which, GLdouble *value) { (void)t; (void)which; if (value) *value = 0.0; }
GLUAPI void gluTessNormal(void *t, GLdouble x, GLdouble y, GLdouble z) { (void)t; (void)x; (void)y; (void)z; }
GLUAPI void gluBeginPolygon(void *t) { (void)t; }
GLUAPI void gluNextContour(void *t, GLenum type) { (void)t; (void)type; }
GLUAPI void gluEndPolygon(void *t) { (void)t; }

GLUAPI void *gluNewNurbsRenderer(void) { return stub_new(); }
GLUAPI void *gluNewNurbsTessellatorEXT(void) { return stub_new(); }
GLUAPI void gluDeleteNurbsRenderer(void *n) { (void)n; }
GLUAPI void gluDeleteNurbsTessellatorEXT(void *n) { (void)n; }
GLUAPI void gluBeginCurve(void *n) { (void)n; }
GLUAPI void gluEndCurve(void *n) { (void)n; }
GLUAPI void gluBeginSurface(void *n) { (void)n; }
GLUAPI void gluEndSurface(void *n) { (void)n; }
GLUAPI void gluBeginTrim(void *n) { (void)n; }
GLUAPI void gluEndTrim(void *n) { (void)n; }
GLUAPI void gluPwlCurve(void *n, GLint count, GLfloat *array, GLint stride, GLenum type) { (void)n; (void)count; (void)array; (void)stride; (void)type; }
GLUAPI void gluNurbsCurve(void *n, GLint knot_count, GLfloat *knots, GLint stride, GLfloat *ctrl, GLint order, GLenum type) { (void)n; (void)knot_count; (void)knots; (void)stride; (void)ctrl; (void)order; (void)type; }
GLUAPI void gluNurbsSurface(void *n, GLint sk, GLfloat *sknots, GLint tk, GLfloat *tknots, GLint sstride, GLint tstride, GLfloat *ctrl, GLint sorder, GLint torder, GLenum type) { (void)n; (void)sk; (void)sknots; (void)tk; (void)tknots; (void)sstride; (void)tstride; (void)ctrl; (void)sorder; (void)torder; (void)type; }
GLUAPI void gluNurbsProperty(void *n, GLenum property, GLfloat value) { (void)n; (void)property; (void)value; }
GLUAPI void gluGetNurbsProperty(void *n, GLenum property, GLfloat *value) { (void)n; (void)property; if (value) *value = 0.0f; }
GLUAPI void gluNurbsCallback(void *n, GLenum which, void (*fn)(void)) { (void)n; (void)which; (void)fn; }
GLUAPI void gluNurbsCallbackData(void *n, void *data) { (void)n; (void)data; }
GLUAPI void gluNurbsCallbackDataEXT(void *n, void *data) { (void)n; (void)data; }
GLUAPI void gluLoadSamplingMatrices(void *n, const GLfloat *model, const GLfloat *proj, const GLint *view) { (void)n; (void)model; (void)proj; (void)view; }
