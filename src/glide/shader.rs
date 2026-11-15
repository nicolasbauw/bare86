// SPDX-License-Identifier: GPL-3.0-or-later
//! Shaders du rendu Glide. Les sommets arrivent en coordonnées d'écran, déjà
//! projetés ; comme la Voodoo, toutes les grandeurs sont interpolées
//! linéairement à l'écran (`noperspective`), et la correction de perspective
//! des textures se fait par point : s = (s/w) / (1/w).

pub const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec2 pos;
layout(location = 1) in vec4 col;
layout(location = 2) in vec3 tex;
layout(location = 3) in vec2 zw;
uniform vec2 screen;
uniform int origin_lower;
noperspective out vec4 v_col;
noperspective out vec3 v_tex;
noperspective out vec2 v_zw;
void main() {
    float x = pos.x / screen.x * 2.0 - 1.0;
    float y = pos.y / screen.y * 2.0 - 1.0;
    if (origin_lower == 0) y = -y;
    gl_Position = vec4(x, y, 0.0, 1.0);
    v_col = col / 255.0;
    v_tex = tex;
    v_zw = zw;
}
"#;

pub const FRAGMENT: &str = r#"#version 330 core
noperspective in vec4 v_col;
noperspective in vec3 v_tex;
noperspective in vec2 v_zw;
uniform sampler2D tex0;
uniform int tex_on;
uniform vec2 tex_scale;
// TMU : fonction, facteur, inversion (rgb puis alpha).
uniform ivec3 tmu_rgb;
uniform ivec3 tmu_a;
// FBI : fonction, facteur, local, autre (couleur puis alpha) ; inversions.
uniform ivec4 cc;
uniform ivec4 ac;
uniform ivec2 inv;
uniform vec4 constant;
uniform int at_func;
uniform float at_ref;
uniform int fog_mode;
uniform vec3 fog_color;
uniform float fog_table[64];
uniform int depth_mode;
uniform float depth_bias;
out vec4 frag;

vec4 combine(int f, vec4 fac, vec4 local, vec4 other) {
    if (f == 0) return vec4(0.0);
    if (f == 1) return local;
    if (f == 2) return vec4(local.a);
    if (f == 3) return fac * other;
    if (f == 4) return fac * other + local;
    if (f == 5) return fac * other + vec4(local.a);
    if (f == 6) return fac * (other - local);
    if (f == 7) return fac * (other - local) + local;
    if (f == 8) return fac * (other - local) + vec4(local.a);
    if (f == 9) return local - fac * local;
    if (f == 16) return vec4(local.a) - fac * local;
    return local;
}

// Facteur : 0 zéro, 1 local, 2 alpha de l'autre, 3 alpha local, 4 alpha de
// la texture, 5 rgb de la texture ; bit 3 : un moins ce facteur.
vec4 factor(int code, vec4 local, vec4 other, vec4 texel, bool alpha) {
    int b = code & 7;
    vec4 v = vec4(0.0);
    if (b == 1) v = alpha ? vec4(local.a) : local;
    else if (b == 2) v = vec4(other.a);
    else if (b == 3) v = vec4(local.a);
    else if (b == 4) v = vec4(texel.a);
    else if (b == 5) v = alpha ? vec4(0.0) : texel;
    return (code & 8) != 0 ? vec4(1.0) - v : v;
}

// W en entier de 16 bits flottant de la Voodoo, ramené à [0, 1] : ≈ log2(w) / 16.
float wdepth(float oow) {
    float w = 1.0 / max(oow, 1e-12);
    return clamp(log2(max(w, 1.0)) / 16.0, 0.0, 1.0);
}

bool cmp(int f, float a, float b) {
    if (f == 0) return false;
    if (f == 1) return a < b;
    if (f == 2) return a == b;
    if (f == 3) return a <= b;
    if (f == 4) return a > b;
    if (f == 5) return a != b;
    if (f == 6) return a >= b;
    return true;
}

void main() {
    // TMU 0 : texel local, rien en amont (pas de seconde TMU).
    vec4 texel = vec4(1.0);
    if (tex_on != 0) {
        vec2 st = v_tex.xy / v_tex.z * tex_scale;
        texel = texture(tex0, st);
    }
    vec4 zero = vec4(0.0);
    vec4 t_rgb = combine(tmu_rgb.x, factor(tmu_rgb.y, texel, zero, texel, false), texel, zero);
    vec4 t_a = combine(tmu_a.x, factor(tmu_a.y, texel, zero, texel, true), texel, zero);
    if (tmu_rgb.z != 0) t_rgb = vec4(1.0) - t_rgb;
    if (tmu_a.z != 0) t_a = vec4(1.0) - t_a;
    vec4 tex_out = clamp(vec4(t_rgb.rgb, t_a.a), 0.0, 1.0);

    // FBI : sources locales et autres de la couleur et de l'alpha.
    vec3 local_rgb = cc.z == 0 ? v_col.rgb : cc.z == 1 ? constant.rgb : vec3(v_zw.x / 65535.0);
    vec3 other_rgb = cc.w == 0 ? v_col.rgb : cc.w == 1 ? tex_out.rgb : constant.rgb;
    float local_a = ac.z == 0 ? v_col.a : ac.z == 1 ? constant.a : v_zw.x / 65535.0;
    float other_a = ac.w == 0 ? v_col.a : ac.w == 1 ? tex_out.a : constant.a;
    vec4 local = vec4(local_rgb, local_a);
    vec4 other = vec4(other_rgb, other_a);
    vec3 rgb = combine(cc.x, factor(cc.y, local, other, tex_out, false), local, other).rgb;
    float a = combine(ac.x, factor(ac.y, vec4(local_a), vec4(other_a), tex_out, true), vec4(local_a), vec4(other_a)).a;
    if (inv.x != 0) rgb = vec3(1.0) - rgb;
    if (inv.y != 0) a = 1.0 - a;
    rgb = clamp(rgb, 0.0, 1.0);
    a = clamp(a, 0.0, 1.0);

    if (!cmp(at_func, floor(a * 255.0 + 0.5), at_ref)) discard;

    int fm = fog_mode & 3;
    if (fm != 0) {
        float f = 0.0;
        if (fm == 1) f = v_col.a;
        else if (fm == 3) f = v_zw.x / 65535.0;
        else {
            // Table de 64 entrées indexée par w : entrée i pour w = 2^(3 + i/4) / (8 - i%4) environ.
            float w = 1.0 / max(v_zw.y, 1e-12);
            float x = clamp(log2(max(w, 1.0)) * 4.0, 0.0, 63.0);
            int i = int(x);
            f = mix(fog_table[i], fog_table[min(i + 1, 63)], fract(x)) / 255.0;
        }
        rgb = mix(rgb, fog_color, clamp(f, 0.0, 1.0));
    }

    if (depth_mode == 1) gl_FragDepth = clamp(v_zw.x / 65535.0 + depth_bias, 0.0, 1.0);
    else if (depth_mode == 2) gl_FragDepth = clamp(wdepth(v_zw.y) + depth_bias, 0.0, 1.0);
    else gl_FragDepth = 0.5;
    frag = vec4(rgb, a);
}
"#;

/// Copie d'une image (écriture dans le tampon linéaire, `grLfbUnlock`).
pub const BLIT_VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec2 pos;
out vec2 uv;
void main() {
    uv = pos * 0.5 + 0.5;
    gl_Position = vec4(pos, 0.0, 1.0);
}
"#;

pub const BLIT_FRAGMENT: &str = r#"#version 330 core
in vec2 uv;
uniform sampler2D img;
out vec4 frag;
void main() {
    vec4 c = texture(img, uv);
    if (c.a == 0.0) discard;
    frag = vec4(c.rgb, 1.0);
}
"#;
