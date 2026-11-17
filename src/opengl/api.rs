// SPDX-License-Identifier: GPL-3.0-or-later
//! Fichier produit par guest/opengl32/gen.py : ne pas modifier.
#![allow(non_upper_case_globals, non_snake_case, clippy::all)]

use std::ffi::c_void;

/// Numéros des commandes (ordre de gl11.txt).
pub mod op {
    pub const glClearIndex: u16 = 1;
    pub const glClearColor: u16 = 2;
    pub const glClear: u16 = 3;
    pub const glIndexMask: u16 = 4;
    pub const glColorMask: u16 = 5;
    pub const glAlphaFunc: u16 = 6;
    pub const glBlendFunc: u16 = 7;
    pub const glLogicOp: u16 = 8;
    pub const glCullFace: u16 = 9;
    pub const glFrontFace: u16 = 10;
    pub const glPointSize: u16 = 11;
    pub const glLineWidth: u16 = 12;
    pub const glLineStipple: u16 = 13;
    pub const glPolygonMode: u16 = 14;
    pub const glPolygonOffset: u16 = 15;
    pub const glPolygonStipple: u16 = 16;
    pub const glGetPolygonStipple: u16 = 17;
    pub const glEdgeFlag: u16 = 18;
    pub const glEdgeFlagv: u16 = 19;
    pub const glScissor: u16 = 20;
    pub const glClipPlane: u16 = 21;
    pub const glGetClipPlane: u16 = 22;
    pub const glDrawBuffer: u16 = 23;
    pub const glReadBuffer: u16 = 24;
    pub const glEnable: u16 = 25;
    pub const glDisable: u16 = 26;
    pub const glIsEnabled: u16 = 27;
    pub const glEnableClientState: u16 = 28;
    pub const glDisableClientState: u16 = 29;
    pub const glGetBooleanv: u16 = 30;
    pub const glGetDoublev: u16 = 31;
    pub const glGetFloatv: u16 = 32;
    pub const glGetIntegerv: u16 = 33;
    pub const glPushAttrib: u16 = 34;
    pub const glPopAttrib: u16 = 35;
    pub const glPushClientAttrib: u16 = 36;
    pub const glPopClientAttrib: u16 = 37;
    pub const glRenderMode: u16 = 38;
    pub const glGetError: u16 = 39;
    pub const glGetString: u16 = 40;
    pub const glFinish: u16 = 41;
    pub const glFlush: u16 = 42;
    pub const glHint: u16 = 43;
    pub const glClearDepth: u16 = 44;
    pub const glDepthFunc: u16 = 45;
    pub const glDepthMask: u16 = 46;
    pub const glDepthRange: u16 = 47;
    pub const glClearAccum: u16 = 48;
    pub const glAccum: u16 = 49;
    pub const glMatrixMode: u16 = 50;
    pub const glOrtho: u16 = 51;
    pub const glFrustum: u16 = 52;
    pub const glViewport: u16 = 53;
    pub const glPushMatrix: u16 = 54;
    pub const glPopMatrix: u16 = 55;
    pub const glLoadIdentity: u16 = 56;
    pub const glLoadMatrixd: u16 = 57;
    pub const glLoadMatrixf: u16 = 58;
    pub const glMultMatrixd: u16 = 59;
    pub const glMultMatrixf: u16 = 60;
    pub const glRotated: u16 = 61;
    pub const glRotatef: u16 = 62;
    pub const glScaled: u16 = 63;
    pub const glScalef: u16 = 64;
    pub const glTranslated: u16 = 65;
    pub const glTranslatef: u16 = 66;
    pub const glIsList: u16 = 67;
    pub const glDeleteLists: u16 = 68;
    pub const glGenLists: u16 = 69;
    pub const glNewList: u16 = 70;
    pub const glEndList: u16 = 71;
    pub const glCallList: u16 = 72;
    pub const glCallLists: u16 = 73;
    pub const glListBase: u16 = 74;
    pub const glBegin: u16 = 75;
    pub const glEnd: u16 = 76;
    pub const glVertex2d: u16 = 77;
    pub const glVertex2f: u16 = 78;
    pub const glVertex2i: u16 = 79;
    pub const glVertex2s: u16 = 80;
    pub const glVertex3d: u16 = 81;
    pub const glVertex3f: u16 = 82;
    pub const glVertex3i: u16 = 83;
    pub const glVertex3s: u16 = 84;
    pub const glVertex4d: u16 = 85;
    pub const glVertex4f: u16 = 86;
    pub const glVertex4i: u16 = 87;
    pub const glVertex4s: u16 = 88;
    pub const glVertex2dv: u16 = 89;
    pub const glVertex2fv: u16 = 90;
    pub const glVertex2iv: u16 = 91;
    pub const glVertex2sv: u16 = 92;
    pub const glVertex3dv: u16 = 93;
    pub const glVertex3fv: u16 = 94;
    pub const glVertex3iv: u16 = 95;
    pub const glVertex3sv: u16 = 96;
    pub const glVertex4dv: u16 = 97;
    pub const glVertex4fv: u16 = 98;
    pub const glVertex4iv: u16 = 99;
    pub const glVertex4sv: u16 = 100;
    pub const glNormal3b: u16 = 101;
    pub const glNormal3d: u16 = 102;
    pub const glNormal3f: u16 = 103;
    pub const glNormal3i: u16 = 104;
    pub const glNormal3s: u16 = 105;
    pub const glNormal3bv: u16 = 106;
    pub const glNormal3dv: u16 = 107;
    pub const glNormal3fv: u16 = 108;
    pub const glNormal3iv: u16 = 109;
    pub const glNormal3sv: u16 = 110;
    pub const glIndexd: u16 = 111;
    pub const glIndexf: u16 = 112;
    pub const glIndexi: u16 = 113;
    pub const glIndexs: u16 = 114;
    pub const glIndexub: u16 = 115;
    pub const glIndexdv: u16 = 116;
    pub const glIndexfv: u16 = 117;
    pub const glIndexiv: u16 = 118;
    pub const glIndexsv: u16 = 119;
    pub const glIndexubv: u16 = 120;
    pub const glColor3b: u16 = 121;
    pub const glColor3d: u16 = 122;
    pub const glColor3f: u16 = 123;
    pub const glColor3i: u16 = 124;
    pub const glColor3s: u16 = 125;
    pub const glColor3ub: u16 = 126;
    pub const glColor3ui: u16 = 127;
    pub const glColor3us: u16 = 128;
    pub const glColor4b: u16 = 129;
    pub const glColor4d: u16 = 130;
    pub const glColor4f: u16 = 131;
    pub const glColor4i: u16 = 132;
    pub const glColor4s: u16 = 133;
    pub const glColor4ub: u16 = 134;
    pub const glColor4ui: u16 = 135;
    pub const glColor4us: u16 = 136;
    pub const glColor3bv: u16 = 137;
    pub const glColor3dv: u16 = 138;
    pub const glColor3fv: u16 = 139;
    pub const glColor3iv: u16 = 140;
    pub const glColor3sv: u16 = 141;
    pub const glColor3ubv: u16 = 142;
    pub const glColor3uiv: u16 = 143;
    pub const glColor3usv: u16 = 144;
    pub const glColor4bv: u16 = 145;
    pub const glColor4dv: u16 = 146;
    pub const glColor4fv: u16 = 147;
    pub const glColor4iv: u16 = 148;
    pub const glColor4sv: u16 = 149;
    pub const glColor4ubv: u16 = 150;
    pub const glColor4uiv: u16 = 151;
    pub const glColor4usv: u16 = 152;
    pub const glTexCoord1d: u16 = 153;
    pub const glTexCoord1f: u16 = 154;
    pub const glTexCoord1i: u16 = 155;
    pub const glTexCoord1s: u16 = 156;
    pub const glTexCoord2d: u16 = 157;
    pub const glTexCoord2f: u16 = 158;
    pub const glTexCoord2i: u16 = 159;
    pub const glTexCoord2s: u16 = 160;
    pub const glTexCoord3d: u16 = 161;
    pub const glTexCoord3f: u16 = 162;
    pub const glTexCoord3i: u16 = 163;
    pub const glTexCoord3s: u16 = 164;
    pub const glTexCoord4d: u16 = 165;
    pub const glTexCoord4f: u16 = 166;
    pub const glTexCoord4i: u16 = 167;
    pub const glTexCoord4s: u16 = 168;
    pub const glTexCoord1dv: u16 = 169;
    pub const glTexCoord1fv: u16 = 170;
    pub const glTexCoord1iv: u16 = 171;
    pub const glTexCoord1sv: u16 = 172;
    pub const glTexCoord2dv: u16 = 173;
    pub const glTexCoord2fv: u16 = 174;
    pub const glTexCoord2iv: u16 = 175;
    pub const glTexCoord2sv: u16 = 176;
    pub const glTexCoord3dv: u16 = 177;
    pub const glTexCoord3fv: u16 = 178;
    pub const glTexCoord3iv: u16 = 179;
    pub const glTexCoord3sv: u16 = 180;
    pub const glTexCoord4dv: u16 = 181;
    pub const glTexCoord4fv: u16 = 182;
    pub const glTexCoord4iv: u16 = 183;
    pub const glTexCoord4sv: u16 = 184;
    pub const glRasterPos2d: u16 = 185;
    pub const glRasterPos2f: u16 = 186;
    pub const glRasterPos2i: u16 = 187;
    pub const glRasterPos2s: u16 = 188;
    pub const glRasterPos3d: u16 = 189;
    pub const glRasterPos3f: u16 = 190;
    pub const glRasterPos3i: u16 = 191;
    pub const glRasterPos3s: u16 = 192;
    pub const glRasterPos4d: u16 = 193;
    pub const glRasterPos4f: u16 = 194;
    pub const glRasterPos4i: u16 = 195;
    pub const glRasterPos4s: u16 = 196;
    pub const glRasterPos2dv: u16 = 197;
    pub const glRasterPos2fv: u16 = 198;
    pub const glRasterPos2iv: u16 = 199;
    pub const glRasterPos2sv: u16 = 200;
    pub const glRasterPos3dv: u16 = 201;
    pub const glRasterPos3fv: u16 = 202;
    pub const glRasterPos3iv: u16 = 203;
    pub const glRasterPos3sv: u16 = 204;
    pub const glRasterPos4dv: u16 = 205;
    pub const glRasterPos4fv: u16 = 206;
    pub const glRasterPos4iv: u16 = 207;
    pub const glRasterPos4sv: u16 = 208;
    pub const glRectd: u16 = 209;
    pub const glRectf: u16 = 210;
    pub const glRecti: u16 = 211;
    pub const glRects: u16 = 212;
    pub const glRectdv: u16 = 213;
    pub const glRectfv: u16 = 214;
    pub const glRectiv: u16 = 215;
    pub const glRectsv: u16 = 216;
    pub const glVertexPointer: u16 = 217;
    pub const glNormalPointer: u16 = 218;
    pub const glColorPointer: u16 = 219;
    pub const glIndexPointer: u16 = 220;
    pub const glTexCoordPointer: u16 = 221;
    pub const glEdgeFlagPointer: u16 = 222;
    pub const glGetPointerv: u16 = 223;
    pub const glArrayElement: u16 = 224;
    pub const glDrawArrays: u16 = 225;
    pub const glDrawElements: u16 = 226;
    pub const glInterleavedArrays: u16 = 227;
    pub const glShadeModel: u16 = 228;
    pub const glLightf: u16 = 229;
    pub const glLighti: u16 = 230;
    pub const glLightfv: u16 = 231;
    pub const glLightiv: u16 = 232;
    pub const glGetLightfv: u16 = 233;
    pub const glGetLightiv: u16 = 234;
    pub const glLightModelf: u16 = 235;
    pub const glLightModeli: u16 = 236;
    pub const glLightModelfv: u16 = 237;
    pub const glLightModeliv: u16 = 238;
    pub const glMaterialf: u16 = 239;
    pub const glMateriali: u16 = 240;
    pub const glMaterialfv: u16 = 241;
    pub const glMaterialiv: u16 = 242;
    pub const glGetMaterialfv: u16 = 243;
    pub const glGetMaterialiv: u16 = 244;
    pub const glColorMaterial: u16 = 245;
    pub const glPixelZoom: u16 = 246;
    pub const glPixelStoref: u16 = 247;
    pub const glPixelStorei: u16 = 248;
    pub const glPixelTransferf: u16 = 249;
    pub const glPixelTransferi: u16 = 250;
    pub const glPixelMapfv: u16 = 251;
    pub const glPixelMapuiv: u16 = 252;
    pub const glPixelMapusv: u16 = 253;
    pub const glGetPixelMapfv: u16 = 254;
    pub const glGetPixelMapuiv: u16 = 255;
    pub const glGetPixelMapusv: u16 = 256;
    pub const glBitmap: u16 = 257;
    pub const glReadPixels: u16 = 258;
    pub const glDrawPixels: u16 = 259;
    pub const glCopyPixels: u16 = 260;
    pub const glStencilFunc: u16 = 261;
    pub const glStencilMask: u16 = 262;
    pub const glStencilOp: u16 = 263;
    pub const glClearStencil: u16 = 264;
    pub const glTexGend: u16 = 265;
    pub const glTexGenf: u16 = 266;
    pub const glTexGeni: u16 = 267;
    pub const glTexGendv: u16 = 268;
    pub const glTexGenfv: u16 = 269;
    pub const glTexGeniv: u16 = 270;
    pub const glGetTexGendv: u16 = 271;
    pub const glGetTexGenfv: u16 = 272;
    pub const glGetTexGeniv: u16 = 273;
    pub const glTexEnvf: u16 = 274;
    pub const glTexEnvi: u16 = 275;
    pub const glTexEnvfv: u16 = 276;
    pub const glTexEnviv: u16 = 277;
    pub const glGetTexEnvfv: u16 = 278;
    pub const glGetTexEnviv: u16 = 279;
    pub const glTexParameterf: u16 = 280;
    pub const glTexParameteri: u16 = 281;
    pub const glTexParameterfv: u16 = 282;
    pub const glTexParameteriv: u16 = 283;
    pub const glGetTexParameterfv: u16 = 284;
    pub const glGetTexParameteriv: u16 = 285;
    pub const glGetTexLevelParameterfv: u16 = 286;
    pub const glGetTexLevelParameteriv: u16 = 287;
    pub const glTexImage1D: u16 = 288;
    pub const glTexImage2D: u16 = 289;
    pub const glGetTexImage: u16 = 290;
    pub const glGenTextures: u16 = 291;
    pub const glDeleteTextures: u16 = 292;
    pub const glBindTexture: u16 = 293;
    pub const glPrioritizeTextures: u16 = 294;
    pub const glAreTexturesResident: u16 = 295;
    pub const glIsTexture: u16 = 296;
    pub const glTexSubImage1D: u16 = 297;
    pub const glTexSubImage2D: u16 = 298;
    pub const glCopyTexImage1D: u16 = 299;
    pub const glCopyTexImage2D: u16 = 300;
    pub const glCopyTexSubImage1D: u16 = 301;
    pub const glCopyTexSubImage2D: u16 = 302;
    pub const glMap1d: u16 = 303;
    pub const glMap1f: u16 = 304;
    pub const glMap2d: u16 = 305;
    pub const glMap2f: u16 = 306;
    pub const glGetMapdv: u16 = 307;
    pub const glGetMapfv: u16 = 308;
    pub const glGetMapiv: u16 = 309;
    pub const glEvalCoord1d: u16 = 310;
    pub const glEvalCoord1f: u16 = 311;
    pub const glEvalCoord1dv: u16 = 312;
    pub const glEvalCoord1fv: u16 = 313;
    pub const glEvalCoord2d: u16 = 314;
    pub const glEvalCoord2f: u16 = 315;
    pub const glEvalCoord2dv: u16 = 316;
    pub const glEvalCoord2fv: u16 = 317;
    pub const glMapGrid1d: u16 = 318;
    pub const glMapGrid1f: u16 = 319;
    pub const glMapGrid2d: u16 = 320;
    pub const glMapGrid2f: u16 = 321;
    pub const glEvalPoint1: u16 = 322;
    pub const glEvalPoint2: u16 = 323;
    pub const glEvalMesh1: u16 = 324;
    pub const glEvalMesh2: u16 = 325;
    pub const glFogf: u16 = 326;
    pub const glFogi: u16 = 327;
    pub const glFogfv: u16 = 328;
    pub const glFogiv: u16 = 329;
    pub const glFeedbackBuffer: u16 = 330;
    pub const glPassThrough: u16 = 331;
    pub const glSelectBuffer: u16 = 332;
    pub const glInitNames: u16 = 333;
    pub const glLoadName: u16 = 334;
    pub const glPushName: u16 = 335;
    pub const glPopName: u16 = 336;
    pub const COUNT: u16 = 336;
}

/// Noms des fonctions, par numéro de commande moins un.
pub const NAMES: [&str; 336] = [
    "glClearIndex",
    "glClearColor",
    "glClear",
    "glIndexMask",
    "glColorMask",
    "glAlphaFunc",
    "glBlendFunc",
    "glLogicOp",
    "glCullFace",
    "glFrontFace",
    "glPointSize",
    "glLineWidth",
    "glLineStipple",
    "glPolygonMode",
    "glPolygonOffset",
    "glPolygonStipple",
    "glGetPolygonStipple",
    "glEdgeFlag",
    "glEdgeFlagv",
    "glScissor",
    "glClipPlane",
    "glGetClipPlane",
    "glDrawBuffer",
    "glReadBuffer",
    "glEnable",
    "glDisable",
    "glIsEnabled",
    "glEnableClientState",
    "glDisableClientState",
    "glGetBooleanv",
    "glGetDoublev",
    "glGetFloatv",
    "glGetIntegerv",
    "glPushAttrib",
    "glPopAttrib",
    "glPushClientAttrib",
    "glPopClientAttrib",
    "glRenderMode",
    "glGetError",
    "glGetString",
    "glFinish",
    "glFlush",
    "glHint",
    "glClearDepth",
    "glDepthFunc",
    "glDepthMask",
    "glDepthRange",
    "glClearAccum",
    "glAccum",
    "glMatrixMode",
    "glOrtho",
    "glFrustum",
    "glViewport",
    "glPushMatrix",
    "glPopMatrix",
    "glLoadIdentity",
    "glLoadMatrixd",
    "glLoadMatrixf",
    "glMultMatrixd",
    "glMultMatrixf",
    "glRotated",
    "glRotatef",
    "glScaled",
    "glScalef",
    "glTranslated",
    "glTranslatef",
    "glIsList",
    "glDeleteLists",
    "glGenLists",
    "glNewList",
    "glEndList",
    "glCallList",
    "glCallLists",
    "glListBase",
    "glBegin",
    "glEnd",
    "glVertex2d",
    "glVertex2f",
    "glVertex2i",
    "glVertex2s",
    "glVertex3d",
    "glVertex3f",
    "glVertex3i",
    "glVertex3s",
    "glVertex4d",
    "glVertex4f",
    "glVertex4i",
    "glVertex4s",
    "glVertex2dv",
    "glVertex2fv",
    "glVertex2iv",
    "glVertex2sv",
    "glVertex3dv",
    "glVertex3fv",
    "glVertex3iv",
    "glVertex3sv",
    "glVertex4dv",
    "glVertex4fv",
    "glVertex4iv",
    "glVertex4sv",
    "glNormal3b",
    "glNormal3d",
    "glNormal3f",
    "glNormal3i",
    "glNormal3s",
    "glNormal3bv",
    "glNormal3dv",
    "glNormal3fv",
    "glNormal3iv",
    "glNormal3sv",
    "glIndexd",
    "glIndexf",
    "glIndexi",
    "glIndexs",
    "glIndexub",
    "glIndexdv",
    "glIndexfv",
    "glIndexiv",
    "glIndexsv",
    "glIndexubv",
    "glColor3b",
    "glColor3d",
    "glColor3f",
    "glColor3i",
    "glColor3s",
    "glColor3ub",
    "glColor3ui",
    "glColor3us",
    "glColor4b",
    "glColor4d",
    "glColor4f",
    "glColor4i",
    "glColor4s",
    "glColor4ub",
    "glColor4ui",
    "glColor4us",
    "glColor3bv",
    "glColor3dv",
    "glColor3fv",
    "glColor3iv",
    "glColor3sv",
    "glColor3ubv",
    "glColor3uiv",
    "glColor3usv",
    "glColor4bv",
    "glColor4dv",
    "glColor4fv",
    "glColor4iv",
    "glColor4sv",
    "glColor4ubv",
    "glColor4uiv",
    "glColor4usv",
    "glTexCoord1d",
    "glTexCoord1f",
    "glTexCoord1i",
    "glTexCoord1s",
    "glTexCoord2d",
    "glTexCoord2f",
    "glTexCoord2i",
    "glTexCoord2s",
    "glTexCoord3d",
    "glTexCoord3f",
    "glTexCoord3i",
    "glTexCoord3s",
    "glTexCoord4d",
    "glTexCoord4f",
    "glTexCoord4i",
    "glTexCoord4s",
    "glTexCoord1dv",
    "glTexCoord1fv",
    "glTexCoord1iv",
    "glTexCoord1sv",
    "glTexCoord2dv",
    "glTexCoord2fv",
    "glTexCoord2iv",
    "glTexCoord2sv",
    "glTexCoord3dv",
    "glTexCoord3fv",
    "glTexCoord3iv",
    "glTexCoord3sv",
    "glTexCoord4dv",
    "glTexCoord4fv",
    "glTexCoord4iv",
    "glTexCoord4sv",
    "glRasterPos2d",
    "glRasterPos2f",
    "glRasterPos2i",
    "glRasterPos2s",
    "glRasterPos3d",
    "glRasterPos3f",
    "glRasterPos3i",
    "glRasterPos3s",
    "glRasterPos4d",
    "glRasterPos4f",
    "glRasterPos4i",
    "glRasterPos4s",
    "glRasterPos2dv",
    "glRasterPos2fv",
    "glRasterPos2iv",
    "glRasterPos2sv",
    "glRasterPos3dv",
    "glRasterPos3fv",
    "glRasterPos3iv",
    "glRasterPos3sv",
    "glRasterPos4dv",
    "glRasterPos4fv",
    "glRasterPos4iv",
    "glRasterPos4sv",
    "glRectd",
    "glRectf",
    "glRecti",
    "glRects",
    "glRectdv",
    "glRectfv",
    "glRectiv",
    "glRectsv",
    "glVertexPointer",
    "glNormalPointer",
    "glColorPointer",
    "glIndexPointer",
    "glTexCoordPointer",
    "glEdgeFlagPointer",
    "glGetPointerv",
    "glArrayElement",
    "glDrawArrays",
    "glDrawElements",
    "glInterleavedArrays",
    "glShadeModel",
    "glLightf",
    "glLighti",
    "glLightfv",
    "glLightiv",
    "glGetLightfv",
    "glGetLightiv",
    "glLightModelf",
    "glLightModeli",
    "glLightModelfv",
    "glLightModeliv",
    "glMaterialf",
    "glMateriali",
    "glMaterialfv",
    "glMaterialiv",
    "glGetMaterialfv",
    "glGetMaterialiv",
    "glColorMaterial",
    "glPixelZoom",
    "glPixelStoref",
    "glPixelStorei",
    "glPixelTransferf",
    "glPixelTransferi",
    "glPixelMapfv",
    "glPixelMapuiv",
    "glPixelMapusv",
    "glGetPixelMapfv",
    "glGetPixelMapuiv",
    "glGetPixelMapusv",
    "glBitmap",
    "glReadPixels",
    "glDrawPixels",
    "glCopyPixels",
    "glStencilFunc",
    "glStencilMask",
    "glStencilOp",
    "glClearStencil",
    "glTexGend",
    "glTexGenf",
    "glTexGeni",
    "glTexGendv",
    "glTexGenfv",
    "glTexGeniv",
    "glGetTexGendv",
    "glGetTexGenfv",
    "glGetTexGeniv",
    "glTexEnvf",
    "glTexEnvi",
    "glTexEnvfv",
    "glTexEnviv",
    "glGetTexEnvfv",
    "glGetTexEnviv",
    "glTexParameterf",
    "glTexParameteri",
    "glTexParameterfv",
    "glTexParameteriv",
    "glGetTexParameterfv",
    "glGetTexParameteriv",
    "glGetTexLevelParameterfv",
    "glGetTexLevelParameteriv",
    "glTexImage1D",
    "glTexImage2D",
    "glGetTexImage",
    "glGenTextures",
    "glDeleteTextures",
    "glBindTexture",
    "glPrioritizeTextures",
    "glAreTexturesResident",
    "glIsTexture",
    "glTexSubImage1D",
    "glTexSubImage2D",
    "glCopyTexImage1D",
    "glCopyTexImage2D",
    "glCopyTexSubImage1D",
    "glCopyTexSubImage2D",
    "glMap1d",
    "glMap1f",
    "glMap2d",
    "glMap2f",
    "glGetMapdv",
    "glGetMapfv",
    "glGetMapiv",
    "glEvalCoord1d",
    "glEvalCoord1f",
    "glEvalCoord1dv",
    "glEvalCoord1fv",
    "glEvalCoord2d",
    "glEvalCoord2f",
    "glEvalCoord2dv",
    "glEvalCoord2fv",
    "glMapGrid1d",
    "glMapGrid1f",
    "glMapGrid2d",
    "glMapGrid2f",
    "glEvalPoint1",
    "glEvalPoint2",
    "glEvalMesh1",
    "glEvalMesh2",
    "glFogf",
    "glFogi",
    "glFogfv",
    "glFogiv",
    "glFeedbackBuffer",
    "glPassThrough",
    "glSelectBuffer",
    "glInitNames",
    "glLoadName",
    "glPushName",
    "glPopName",
];

/// Fonctions d'OpenGL 1.1 de l'hôte.
pub struct Fns {
    pub glClearIndex: unsafe extern "C" fn(f32),
    pub glClearColor: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glClear: unsafe extern "C" fn(u32),
    pub glIndexMask: unsafe extern "C" fn(u32),
    pub glColorMask: unsafe extern "C" fn(u8, u8, u8, u8),
    pub glAlphaFunc: unsafe extern "C" fn(u32, f32),
    pub glBlendFunc: unsafe extern "C" fn(u32, u32),
    pub glLogicOp: unsafe extern "C" fn(u32),
    pub glCullFace: unsafe extern "C" fn(u32),
    pub glFrontFace: unsafe extern "C" fn(u32),
    pub glPointSize: unsafe extern "C" fn(f32),
    pub glLineWidth: unsafe extern "C" fn(f32),
    pub glLineStipple: unsafe extern "C" fn(i32, u16),
    pub glPolygonMode: unsafe extern "C" fn(u32, u32),
    pub glPolygonOffset: unsafe extern "C" fn(f32, f32),
    pub glPolygonStipple: unsafe extern "C" fn(*const c_void),
    pub glGetPolygonStipple: unsafe extern "C" fn(*const c_void),
    pub glEdgeFlag: unsafe extern "C" fn(u8),
    pub glEdgeFlagv: unsafe extern "C" fn(*const c_void),
    pub glScissor: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glClipPlane: unsafe extern "C" fn(u32, *const c_void),
    pub glGetClipPlane: unsafe extern "C" fn(u32, *const c_void),
    pub glDrawBuffer: unsafe extern "C" fn(u32),
    pub glReadBuffer: unsafe extern "C" fn(u32),
    pub glEnable: unsafe extern "C" fn(u32),
    pub glDisable: unsafe extern "C" fn(u32),
    pub glIsEnabled: unsafe extern "C" fn(u32) -> u8,
    pub glEnableClientState: unsafe extern "C" fn(u32),
    pub glDisableClientState: unsafe extern "C" fn(u32),
    pub glGetBooleanv: unsafe extern "C" fn(u32, *const c_void),
    pub glGetDoublev: unsafe extern "C" fn(u32, *const c_void),
    pub glGetFloatv: unsafe extern "C" fn(u32, *const c_void),
    pub glGetIntegerv: unsafe extern "C" fn(u32, *const c_void),
    pub glPushAttrib: unsafe extern "C" fn(u32),
    pub glPopAttrib: unsafe extern "C" fn(),
    pub glPushClientAttrib: unsafe extern "C" fn(u32),
    pub glPopClientAttrib: unsafe extern "C" fn(),
    pub glRenderMode: unsafe extern "C" fn(u32) -> i32,
    pub glGetError: unsafe extern "C" fn() -> u32,
    pub glGetString: unsafe extern "C" fn(u32) -> *const u8,
    pub glFinish: unsafe extern "C" fn(),
    pub glFlush: unsafe extern "C" fn(),
    pub glHint: unsafe extern "C" fn(u32, u32),
    pub glClearDepth: unsafe extern "C" fn(f64),
    pub glDepthFunc: unsafe extern "C" fn(u32),
    pub glDepthMask: unsafe extern "C" fn(u8),
    pub glDepthRange: unsafe extern "C" fn(f64, f64),
    pub glClearAccum: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glAccum: unsafe extern "C" fn(u32, f32),
    pub glMatrixMode: unsafe extern "C" fn(u32),
    pub glOrtho: unsafe extern "C" fn(f64, f64, f64, f64, f64, f64),
    pub glFrustum: unsafe extern "C" fn(f64, f64, f64, f64, f64, f64),
    pub glViewport: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glPushMatrix: unsafe extern "C" fn(),
    pub glPopMatrix: unsafe extern "C" fn(),
    pub glLoadIdentity: unsafe extern "C" fn(),
    pub glLoadMatrixd: unsafe extern "C" fn(*const c_void),
    pub glLoadMatrixf: unsafe extern "C" fn(*const c_void),
    pub glMultMatrixd: unsafe extern "C" fn(*const c_void),
    pub glMultMatrixf: unsafe extern "C" fn(*const c_void),
    pub glRotated: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glRotatef: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glScaled: unsafe extern "C" fn(f64, f64, f64),
    pub glScalef: unsafe extern "C" fn(f32, f32, f32),
    pub glTranslated: unsafe extern "C" fn(f64, f64, f64),
    pub glTranslatef: unsafe extern "C" fn(f32, f32, f32),
    pub glIsList: unsafe extern "C" fn(u32) -> u8,
    pub glDeleteLists: unsafe extern "C" fn(u32, i32),
    pub glGenLists: unsafe extern "C" fn(i32) -> u32,
    pub glNewList: unsafe extern "C" fn(u32, u32),
    pub glEndList: unsafe extern "C" fn(),
    pub glCallList: unsafe extern "C" fn(u32),
    pub glCallLists: unsafe extern "C" fn(i32, u32, *const c_void),
    pub glListBase: unsafe extern "C" fn(u32),
    pub glBegin: unsafe extern "C" fn(u32),
    pub glEnd: unsafe extern "C" fn(),
    pub glVertex2d: unsafe extern "C" fn(f64, f64),
    pub glVertex2f: unsafe extern "C" fn(f32, f32),
    pub glVertex2i: unsafe extern "C" fn(i32, i32),
    pub glVertex2s: unsafe extern "C" fn(i16, i16),
    pub glVertex3d: unsafe extern "C" fn(f64, f64, f64),
    pub glVertex3f: unsafe extern "C" fn(f32, f32, f32),
    pub glVertex3i: unsafe extern "C" fn(i32, i32, i32),
    pub glVertex3s: unsafe extern "C" fn(i16, i16, i16),
    pub glVertex4d: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glVertex4f: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glVertex4i: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glVertex4s: unsafe extern "C" fn(i16, i16, i16, i16),
    pub glVertex2dv: unsafe extern "C" fn(*const c_void),
    pub glVertex2fv: unsafe extern "C" fn(*const c_void),
    pub glVertex2iv: unsafe extern "C" fn(*const c_void),
    pub glVertex2sv: unsafe extern "C" fn(*const c_void),
    pub glVertex3dv: unsafe extern "C" fn(*const c_void),
    pub glVertex3fv: unsafe extern "C" fn(*const c_void),
    pub glVertex3iv: unsafe extern "C" fn(*const c_void),
    pub glVertex3sv: unsafe extern "C" fn(*const c_void),
    pub glVertex4dv: unsafe extern "C" fn(*const c_void),
    pub glVertex4fv: unsafe extern "C" fn(*const c_void),
    pub glVertex4iv: unsafe extern "C" fn(*const c_void),
    pub glVertex4sv: unsafe extern "C" fn(*const c_void),
    pub glNormal3b: unsafe extern "C" fn(i8, i8, i8),
    pub glNormal3d: unsafe extern "C" fn(f64, f64, f64),
    pub glNormal3f: unsafe extern "C" fn(f32, f32, f32),
    pub glNormal3i: unsafe extern "C" fn(i32, i32, i32),
    pub glNormal3s: unsafe extern "C" fn(i16, i16, i16),
    pub glNormal3bv: unsafe extern "C" fn(*const c_void),
    pub glNormal3dv: unsafe extern "C" fn(*const c_void),
    pub glNormal3fv: unsafe extern "C" fn(*const c_void),
    pub glNormal3iv: unsafe extern "C" fn(*const c_void),
    pub glNormal3sv: unsafe extern "C" fn(*const c_void),
    pub glIndexd: unsafe extern "C" fn(f64),
    pub glIndexf: unsafe extern "C" fn(f32),
    pub glIndexi: unsafe extern "C" fn(i32),
    pub glIndexs: unsafe extern "C" fn(i16),
    pub glIndexub: unsafe extern "C" fn(u8),
    pub glIndexdv: unsafe extern "C" fn(*const c_void),
    pub glIndexfv: unsafe extern "C" fn(*const c_void),
    pub glIndexiv: unsafe extern "C" fn(*const c_void),
    pub glIndexsv: unsafe extern "C" fn(*const c_void),
    pub glIndexubv: unsafe extern "C" fn(*const c_void),
    pub glColor3b: unsafe extern "C" fn(i8, i8, i8),
    pub glColor3d: unsafe extern "C" fn(f64, f64, f64),
    pub glColor3f: unsafe extern "C" fn(f32, f32, f32),
    pub glColor3i: unsafe extern "C" fn(i32, i32, i32),
    pub glColor3s: unsafe extern "C" fn(i16, i16, i16),
    pub glColor3ub: unsafe extern "C" fn(u8, u8, u8),
    pub glColor3ui: unsafe extern "C" fn(u32, u32, u32),
    pub glColor3us: unsafe extern "C" fn(u16, u16, u16),
    pub glColor4b: unsafe extern "C" fn(i8, i8, i8, i8),
    pub glColor4d: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glColor4f: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glColor4i: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glColor4s: unsafe extern "C" fn(i16, i16, i16, i16),
    pub glColor4ub: unsafe extern "C" fn(u8, u8, u8, u8),
    pub glColor4ui: unsafe extern "C" fn(u32, u32, u32, u32),
    pub glColor4us: unsafe extern "C" fn(u16, u16, u16, u16),
    pub glColor3bv: unsafe extern "C" fn(*const c_void),
    pub glColor3dv: unsafe extern "C" fn(*const c_void),
    pub glColor3fv: unsafe extern "C" fn(*const c_void),
    pub glColor3iv: unsafe extern "C" fn(*const c_void),
    pub glColor3sv: unsafe extern "C" fn(*const c_void),
    pub glColor3ubv: unsafe extern "C" fn(*const c_void),
    pub glColor3uiv: unsafe extern "C" fn(*const c_void),
    pub glColor3usv: unsafe extern "C" fn(*const c_void),
    pub glColor4bv: unsafe extern "C" fn(*const c_void),
    pub glColor4dv: unsafe extern "C" fn(*const c_void),
    pub glColor4fv: unsafe extern "C" fn(*const c_void),
    pub glColor4iv: unsafe extern "C" fn(*const c_void),
    pub glColor4sv: unsafe extern "C" fn(*const c_void),
    pub glColor4ubv: unsafe extern "C" fn(*const c_void),
    pub glColor4uiv: unsafe extern "C" fn(*const c_void),
    pub glColor4usv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord1d: unsafe extern "C" fn(f64),
    pub glTexCoord1f: unsafe extern "C" fn(f32),
    pub glTexCoord1i: unsafe extern "C" fn(i32),
    pub glTexCoord1s: unsafe extern "C" fn(i16),
    pub glTexCoord2d: unsafe extern "C" fn(f64, f64),
    pub glTexCoord2f: unsafe extern "C" fn(f32, f32),
    pub glTexCoord2i: unsafe extern "C" fn(i32, i32),
    pub glTexCoord2s: unsafe extern "C" fn(i16, i16),
    pub glTexCoord3d: unsafe extern "C" fn(f64, f64, f64),
    pub glTexCoord3f: unsafe extern "C" fn(f32, f32, f32),
    pub glTexCoord3i: unsafe extern "C" fn(i32, i32, i32),
    pub glTexCoord3s: unsafe extern "C" fn(i16, i16, i16),
    pub glTexCoord4d: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glTexCoord4f: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glTexCoord4i: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glTexCoord4s: unsafe extern "C" fn(i16, i16, i16, i16),
    pub glTexCoord1dv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord1fv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord1iv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord1sv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord2dv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord2fv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord2iv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord2sv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord3dv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord3fv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord3iv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord3sv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord4dv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord4fv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord4iv: unsafe extern "C" fn(*const c_void),
    pub glTexCoord4sv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos2d: unsafe extern "C" fn(f64, f64),
    pub glRasterPos2f: unsafe extern "C" fn(f32, f32),
    pub glRasterPos2i: unsafe extern "C" fn(i32, i32),
    pub glRasterPos2s: unsafe extern "C" fn(i16, i16),
    pub glRasterPos3d: unsafe extern "C" fn(f64, f64, f64),
    pub glRasterPos3f: unsafe extern "C" fn(f32, f32, f32),
    pub glRasterPos3i: unsafe extern "C" fn(i32, i32, i32),
    pub glRasterPos3s: unsafe extern "C" fn(i16, i16, i16),
    pub glRasterPos4d: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glRasterPos4f: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glRasterPos4i: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glRasterPos4s: unsafe extern "C" fn(i16, i16, i16, i16),
    pub glRasterPos2dv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos2fv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos2iv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos2sv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos3dv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos3fv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos3iv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos3sv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos4dv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos4fv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos4iv: unsafe extern "C" fn(*const c_void),
    pub glRasterPos4sv: unsafe extern "C" fn(*const c_void),
    pub glRectd: unsafe extern "C" fn(f64, f64, f64, f64),
    pub glRectf: unsafe extern "C" fn(f32, f32, f32, f32),
    pub glRecti: unsafe extern "C" fn(i32, i32, i32, i32),
    pub glRects: unsafe extern "C" fn(i16, i16, i16, i16),
    pub glRectdv: unsafe extern "C" fn(*const c_void, *const c_void),
    pub glRectfv: unsafe extern "C" fn(*const c_void, *const c_void),
    pub glRectiv: unsafe extern "C" fn(*const c_void, *const c_void),
    pub glRectsv: unsafe extern "C" fn(*const c_void, *const c_void),
    pub glVertexPointer: unsafe extern "C" fn(i32, u32, i32, *const c_void),
    pub glNormalPointer: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glColorPointer: unsafe extern "C" fn(i32, u32, i32, *const c_void),
    pub glIndexPointer: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glTexCoordPointer: unsafe extern "C" fn(i32, u32, i32, *const c_void),
    pub glEdgeFlagPointer: unsafe extern "C" fn(i32, *const c_void),
    pub glGetPointerv: unsafe extern "C" fn(u32, *const c_void),
    pub glArrayElement: unsafe extern "C" fn(i32),
    pub glDrawArrays: unsafe extern "C" fn(u32, i32, i32),
    pub glDrawElements: unsafe extern "C" fn(u32, i32, u32, *const c_void),
    pub glInterleavedArrays: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glShadeModel: unsafe extern "C" fn(u32),
    pub glLightf: unsafe extern "C" fn(u32, u32, f32),
    pub glLighti: unsafe extern "C" fn(u32, u32, i32),
    pub glLightfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glLightiv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetLightfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetLightiv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glLightModelf: unsafe extern "C" fn(u32, f32),
    pub glLightModeli: unsafe extern "C" fn(u32, i32),
    pub glLightModelfv: unsafe extern "C" fn(u32, *const c_void),
    pub glLightModeliv: unsafe extern "C" fn(u32, *const c_void),
    pub glMaterialf: unsafe extern "C" fn(u32, u32, f32),
    pub glMateriali: unsafe extern "C" fn(u32, u32, i32),
    pub glMaterialfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glMaterialiv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetMaterialfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetMaterialiv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glColorMaterial: unsafe extern "C" fn(u32, u32),
    pub glPixelZoom: unsafe extern "C" fn(f32, f32),
    pub glPixelStoref: unsafe extern "C" fn(u32, f32),
    pub glPixelStorei: unsafe extern "C" fn(u32, i32),
    pub glPixelTransferf: unsafe extern "C" fn(u32, f32),
    pub glPixelTransferi: unsafe extern "C" fn(u32, i32),
    pub glPixelMapfv: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glPixelMapuiv: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glPixelMapusv: unsafe extern "C" fn(u32, i32, *const c_void),
    pub glGetPixelMapfv: unsafe extern "C" fn(u32, *const c_void),
    pub glGetPixelMapuiv: unsafe extern "C" fn(u32, *const c_void),
    pub glGetPixelMapusv: unsafe extern "C" fn(u32, *const c_void),
    pub glBitmap: unsafe extern "C" fn(i32, i32, f32, f32, f32, f32, *const c_void),
    pub glReadPixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *const c_void),
    pub glDrawPixels: unsafe extern "C" fn(i32, i32, u32, u32, *const c_void),
    pub glCopyPixels: unsafe extern "C" fn(i32, i32, i32, i32, u32),
    pub glStencilFunc: unsafe extern "C" fn(u32, i32, u32),
    pub glStencilMask: unsafe extern "C" fn(u32),
    pub glStencilOp: unsafe extern "C" fn(u32, u32, u32),
    pub glClearStencil: unsafe extern "C" fn(i32),
    pub glTexGend: unsafe extern "C" fn(u32, u32, f64),
    pub glTexGenf: unsafe extern "C" fn(u32, u32, f32),
    pub glTexGeni: unsafe extern "C" fn(u32, u32, i32),
    pub glTexGendv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexGenfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexGeniv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexGendv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexGenfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexGeniv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexEnvf: unsafe extern "C" fn(u32, u32, f32),
    pub glTexEnvi: unsafe extern "C" fn(u32, u32, i32),
    pub glTexEnvfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexEnviv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexEnvfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexEnviv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexParameterf: unsafe extern "C" fn(u32, u32, f32),
    pub glTexParameteri: unsafe extern "C" fn(u32, u32, i32),
    pub glTexParameterfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glTexParameteriv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexParameterfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexParameteriv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetTexLevelParameterfv: unsafe extern "C" fn(u32, i32, u32, *const c_void),
    pub glGetTexLevelParameteriv: unsafe extern "C" fn(u32, i32, u32, *const c_void),
    pub glTexImage1D: unsafe extern "C" fn(u32, i32, i32, i32, i32, u32, u32, *const c_void),
    pub glTexImage2D: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    pub glGetTexImage: unsafe extern "C" fn(u32, i32, u32, u32, *const c_void),
    pub glGenTextures: unsafe extern "C" fn(i32, *const c_void),
    pub glDeleteTextures: unsafe extern "C" fn(i32, *const c_void),
    pub glBindTexture: unsafe extern "C" fn(u32, u32),
    pub glPrioritizeTextures: unsafe extern "C" fn(i32, *const c_void, *const c_void),
    pub glAreTexturesResident: unsafe extern "C" fn(i32, *const c_void, *const c_void) -> u8,
    pub glIsTexture: unsafe extern "C" fn(u32) -> u8,
    pub glTexSubImage1D: unsafe extern "C" fn(u32, i32, i32, i32, u32, u32, *const c_void),
    pub glTexSubImage2D: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    pub glCopyTexImage1D: unsafe extern "C" fn(u32, i32, u32, i32, i32, i32, i32),
    pub glCopyTexImage2D: unsafe extern "C" fn(u32, i32, u32, i32, i32, i32, i32, i32),
    pub glCopyTexSubImage1D: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32),
    pub glCopyTexSubImage2D: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32),
    pub glMap1d: unsafe extern "C" fn(u32, f64, f64, i32, i32, *const c_void),
    pub glMap1f: unsafe extern "C" fn(u32, f32, f32, i32, i32, *const c_void),
    pub glMap2d: unsafe extern "C" fn(u32, f64, f64, i32, i32, f64, f64, i32, i32, *const c_void),
    pub glMap2f: unsafe extern "C" fn(u32, f32, f32, i32, i32, f32, f32, i32, i32, *const c_void),
    pub glGetMapdv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetMapfv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glGetMapiv: unsafe extern "C" fn(u32, u32, *const c_void),
    pub glEvalCoord1d: unsafe extern "C" fn(f64),
    pub glEvalCoord1f: unsafe extern "C" fn(f32),
    pub glEvalCoord1dv: unsafe extern "C" fn(*const c_void),
    pub glEvalCoord1fv: unsafe extern "C" fn(*const c_void),
    pub glEvalCoord2d: unsafe extern "C" fn(f64, f64),
    pub glEvalCoord2f: unsafe extern "C" fn(f32, f32),
    pub glEvalCoord2dv: unsafe extern "C" fn(*const c_void),
    pub glEvalCoord2fv: unsafe extern "C" fn(*const c_void),
    pub glMapGrid1d: unsafe extern "C" fn(i32, f64, f64),
    pub glMapGrid1f: unsafe extern "C" fn(i32, f32, f32),
    pub glMapGrid2d: unsafe extern "C" fn(i32, f64, f64, i32, f64, f64),
    pub glMapGrid2f: unsafe extern "C" fn(i32, f32, f32, i32, f32, f32),
    pub glEvalPoint1: unsafe extern "C" fn(i32),
    pub glEvalPoint2: unsafe extern "C" fn(i32, i32),
    pub glEvalMesh1: unsafe extern "C" fn(u32, i32, i32),
    pub glEvalMesh2: unsafe extern "C" fn(u32, i32, i32, i32, i32),
    pub glFogf: unsafe extern "C" fn(u32, f32),
    pub glFogi: unsafe extern "C" fn(u32, i32),
    pub glFogfv: unsafe extern "C" fn(u32, *const c_void),
    pub glFogiv: unsafe extern "C" fn(u32, *const c_void),
    pub glFeedbackBuffer: unsafe extern "C" fn(i32, u32, *const c_void),
    pub glPassThrough: unsafe extern "C" fn(f32),
    pub glSelectBuffer: unsafe extern "C" fn(i32, *const c_void),
    pub glInitNames: unsafe extern "C" fn(),
    pub glLoadName: unsafe extern "C" fn(u32),
    pub glPushName: unsafe extern "C" fn(u32),
    pub glPopName: unsafe extern "C" fn(),
}

impl Fns {
    /// Charge toutes les fonctions ; échoue si l'une manque.
    pub fn load(get: &dyn Fn(&str) -> *const c_void) -> Result<Self, String> {
        let f = |n: &str| { let p = get(n); if p.is_null() { Err(format!("OpenGL: {n} missing")) } else { Ok(p) } };
        // SAFETY: pointeurs rendus par le chargeur d'OpenGL pour ces noms,
        // de signatures conformes à gl.h.
        unsafe {
            Ok(Self {
                glClearIndex: std::mem::transmute::<*const c_void, _>(f("glClearIndex")?),
                glClearColor: std::mem::transmute::<*const c_void, _>(f("glClearColor")?),
                glClear: std::mem::transmute::<*const c_void, _>(f("glClear")?),
                glIndexMask: std::mem::transmute::<*const c_void, _>(f("glIndexMask")?),
                glColorMask: std::mem::transmute::<*const c_void, _>(f("glColorMask")?),
                glAlphaFunc: std::mem::transmute::<*const c_void, _>(f("glAlphaFunc")?),
                glBlendFunc: std::mem::transmute::<*const c_void, _>(f("glBlendFunc")?),
                glLogicOp: std::mem::transmute::<*const c_void, _>(f("glLogicOp")?),
                glCullFace: std::mem::transmute::<*const c_void, _>(f("glCullFace")?),
                glFrontFace: std::mem::transmute::<*const c_void, _>(f("glFrontFace")?),
                glPointSize: std::mem::transmute::<*const c_void, _>(f("glPointSize")?),
                glLineWidth: std::mem::transmute::<*const c_void, _>(f("glLineWidth")?),
                glLineStipple: std::mem::transmute::<*const c_void, _>(f("glLineStipple")?),
                glPolygonMode: std::mem::transmute::<*const c_void, _>(f("glPolygonMode")?),
                glPolygonOffset: std::mem::transmute::<*const c_void, _>(f("glPolygonOffset")?),
                glPolygonStipple: std::mem::transmute::<*const c_void, _>(f("glPolygonStipple")?),
                glGetPolygonStipple: std::mem::transmute::<*const c_void, _>(f("glGetPolygonStipple")?),
                glEdgeFlag: std::mem::transmute::<*const c_void, _>(f("glEdgeFlag")?),
                glEdgeFlagv: std::mem::transmute::<*const c_void, _>(f("glEdgeFlagv")?),
                glScissor: std::mem::transmute::<*const c_void, _>(f("glScissor")?),
                glClipPlane: std::mem::transmute::<*const c_void, _>(f("glClipPlane")?),
                glGetClipPlane: std::mem::transmute::<*const c_void, _>(f("glGetClipPlane")?),
                glDrawBuffer: std::mem::transmute::<*const c_void, _>(f("glDrawBuffer")?),
                glReadBuffer: std::mem::transmute::<*const c_void, _>(f("glReadBuffer")?),
                glEnable: std::mem::transmute::<*const c_void, _>(f("glEnable")?),
                glDisable: std::mem::transmute::<*const c_void, _>(f("glDisable")?),
                glIsEnabled: std::mem::transmute::<*const c_void, _>(f("glIsEnabled")?),
                glEnableClientState: std::mem::transmute::<*const c_void, _>(f("glEnableClientState")?),
                glDisableClientState: std::mem::transmute::<*const c_void, _>(f("glDisableClientState")?),
                glGetBooleanv: std::mem::transmute::<*const c_void, _>(f("glGetBooleanv")?),
                glGetDoublev: std::mem::transmute::<*const c_void, _>(f("glGetDoublev")?),
                glGetFloatv: std::mem::transmute::<*const c_void, _>(f("glGetFloatv")?),
                glGetIntegerv: std::mem::transmute::<*const c_void, _>(f("glGetIntegerv")?),
                glPushAttrib: std::mem::transmute::<*const c_void, _>(f("glPushAttrib")?),
                glPopAttrib: std::mem::transmute::<*const c_void, _>(f("glPopAttrib")?),
                glPushClientAttrib: std::mem::transmute::<*const c_void, _>(f("glPushClientAttrib")?),
                glPopClientAttrib: std::mem::transmute::<*const c_void, _>(f("glPopClientAttrib")?),
                glRenderMode: std::mem::transmute::<*const c_void, _>(f("glRenderMode")?),
                glGetError: std::mem::transmute::<*const c_void, _>(f("glGetError")?),
                glGetString: std::mem::transmute::<*const c_void, _>(f("glGetString")?),
                glFinish: std::mem::transmute::<*const c_void, _>(f("glFinish")?),
                glFlush: std::mem::transmute::<*const c_void, _>(f("glFlush")?),
                glHint: std::mem::transmute::<*const c_void, _>(f("glHint")?),
                glClearDepth: std::mem::transmute::<*const c_void, _>(f("glClearDepth")?),
                glDepthFunc: std::mem::transmute::<*const c_void, _>(f("glDepthFunc")?),
                glDepthMask: std::mem::transmute::<*const c_void, _>(f("glDepthMask")?),
                glDepthRange: std::mem::transmute::<*const c_void, _>(f("glDepthRange")?),
                glClearAccum: std::mem::transmute::<*const c_void, _>(f("glClearAccum")?),
                glAccum: std::mem::transmute::<*const c_void, _>(f("glAccum")?),
                glMatrixMode: std::mem::transmute::<*const c_void, _>(f("glMatrixMode")?),
                glOrtho: std::mem::transmute::<*const c_void, _>(f("glOrtho")?),
                glFrustum: std::mem::transmute::<*const c_void, _>(f("glFrustum")?),
                glViewport: std::mem::transmute::<*const c_void, _>(f("glViewport")?),
                glPushMatrix: std::mem::transmute::<*const c_void, _>(f("glPushMatrix")?),
                glPopMatrix: std::mem::transmute::<*const c_void, _>(f("glPopMatrix")?),
                glLoadIdentity: std::mem::transmute::<*const c_void, _>(f("glLoadIdentity")?),
                glLoadMatrixd: std::mem::transmute::<*const c_void, _>(f("glLoadMatrixd")?),
                glLoadMatrixf: std::mem::transmute::<*const c_void, _>(f("glLoadMatrixf")?),
                glMultMatrixd: std::mem::transmute::<*const c_void, _>(f("glMultMatrixd")?),
                glMultMatrixf: std::mem::transmute::<*const c_void, _>(f("glMultMatrixf")?),
                glRotated: std::mem::transmute::<*const c_void, _>(f("glRotated")?),
                glRotatef: std::mem::transmute::<*const c_void, _>(f("glRotatef")?),
                glScaled: std::mem::transmute::<*const c_void, _>(f("glScaled")?),
                glScalef: std::mem::transmute::<*const c_void, _>(f("glScalef")?),
                glTranslated: std::mem::transmute::<*const c_void, _>(f("glTranslated")?),
                glTranslatef: std::mem::transmute::<*const c_void, _>(f("glTranslatef")?),
                glIsList: std::mem::transmute::<*const c_void, _>(f("glIsList")?),
                glDeleteLists: std::mem::transmute::<*const c_void, _>(f("glDeleteLists")?),
                glGenLists: std::mem::transmute::<*const c_void, _>(f("glGenLists")?),
                glNewList: std::mem::transmute::<*const c_void, _>(f("glNewList")?),
                glEndList: std::mem::transmute::<*const c_void, _>(f("glEndList")?),
                glCallList: std::mem::transmute::<*const c_void, _>(f("glCallList")?),
                glCallLists: std::mem::transmute::<*const c_void, _>(f("glCallLists")?),
                glListBase: std::mem::transmute::<*const c_void, _>(f("glListBase")?),
                glBegin: std::mem::transmute::<*const c_void, _>(f("glBegin")?),
                glEnd: std::mem::transmute::<*const c_void, _>(f("glEnd")?),
                glVertex2d: std::mem::transmute::<*const c_void, _>(f("glVertex2d")?),
                glVertex2f: std::mem::transmute::<*const c_void, _>(f("glVertex2f")?),
                glVertex2i: std::mem::transmute::<*const c_void, _>(f("glVertex2i")?),
                glVertex2s: std::mem::transmute::<*const c_void, _>(f("glVertex2s")?),
                glVertex3d: std::mem::transmute::<*const c_void, _>(f("glVertex3d")?),
                glVertex3f: std::mem::transmute::<*const c_void, _>(f("glVertex3f")?),
                glVertex3i: std::mem::transmute::<*const c_void, _>(f("glVertex3i")?),
                glVertex3s: std::mem::transmute::<*const c_void, _>(f("glVertex3s")?),
                glVertex4d: std::mem::transmute::<*const c_void, _>(f("glVertex4d")?),
                glVertex4f: std::mem::transmute::<*const c_void, _>(f("glVertex4f")?),
                glVertex4i: std::mem::transmute::<*const c_void, _>(f("glVertex4i")?),
                glVertex4s: std::mem::transmute::<*const c_void, _>(f("glVertex4s")?),
                glVertex2dv: std::mem::transmute::<*const c_void, _>(f("glVertex2dv")?),
                glVertex2fv: std::mem::transmute::<*const c_void, _>(f("glVertex2fv")?),
                glVertex2iv: std::mem::transmute::<*const c_void, _>(f("glVertex2iv")?),
                glVertex2sv: std::mem::transmute::<*const c_void, _>(f("glVertex2sv")?),
                glVertex3dv: std::mem::transmute::<*const c_void, _>(f("glVertex3dv")?),
                glVertex3fv: std::mem::transmute::<*const c_void, _>(f("glVertex3fv")?),
                glVertex3iv: std::mem::transmute::<*const c_void, _>(f("glVertex3iv")?),
                glVertex3sv: std::mem::transmute::<*const c_void, _>(f("glVertex3sv")?),
                glVertex4dv: std::mem::transmute::<*const c_void, _>(f("glVertex4dv")?),
                glVertex4fv: std::mem::transmute::<*const c_void, _>(f("glVertex4fv")?),
                glVertex4iv: std::mem::transmute::<*const c_void, _>(f("glVertex4iv")?),
                glVertex4sv: std::mem::transmute::<*const c_void, _>(f("glVertex4sv")?),
                glNormal3b: std::mem::transmute::<*const c_void, _>(f("glNormal3b")?),
                glNormal3d: std::mem::transmute::<*const c_void, _>(f("glNormal3d")?),
                glNormal3f: std::mem::transmute::<*const c_void, _>(f("glNormal3f")?),
                glNormal3i: std::mem::transmute::<*const c_void, _>(f("glNormal3i")?),
                glNormal3s: std::mem::transmute::<*const c_void, _>(f("glNormal3s")?),
                glNormal3bv: std::mem::transmute::<*const c_void, _>(f("glNormal3bv")?),
                glNormal3dv: std::mem::transmute::<*const c_void, _>(f("glNormal3dv")?),
                glNormal3fv: std::mem::transmute::<*const c_void, _>(f("glNormal3fv")?),
                glNormal3iv: std::mem::transmute::<*const c_void, _>(f("glNormal3iv")?),
                glNormal3sv: std::mem::transmute::<*const c_void, _>(f("glNormal3sv")?),
                glIndexd: std::mem::transmute::<*const c_void, _>(f("glIndexd")?),
                glIndexf: std::mem::transmute::<*const c_void, _>(f("glIndexf")?),
                glIndexi: std::mem::transmute::<*const c_void, _>(f("glIndexi")?),
                glIndexs: std::mem::transmute::<*const c_void, _>(f("glIndexs")?),
                glIndexub: std::mem::transmute::<*const c_void, _>(f("glIndexub")?),
                glIndexdv: std::mem::transmute::<*const c_void, _>(f("glIndexdv")?),
                glIndexfv: std::mem::transmute::<*const c_void, _>(f("glIndexfv")?),
                glIndexiv: std::mem::transmute::<*const c_void, _>(f("glIndexiv")?),
                glIndexsv: std::mem::transmute::<*const c_void, _>(f("glIndexsv")?),
                glIndexubv: std::mem::transmute::<*const c_void, _>(f("glIndexubv")?),
                glColor3b: std::mem::transmute::<*const c_void, _>(f("glColor3b")?),
                glColor3d: std::mem::transmute::<*const c_void, _>(f("glColor3d")?),
                glColor3f: std::mem::transmute::<*const c_void, _>(f("glColor3f")?),
                glColor3i: std::mem::transmute::<*const c_void, _>(f("glColor3i")?),
                glColor3s: std::mem::transmute::<*const c_void, _>(f("glColor3s")?),
                glColor3ub: std::mem::transmute::<*const c_void, _>(f("glColor3ub")?),
                glColor3ui: std::mem::transmute::<*const c_void, _>(f("glColor3ui")?),
                glColor3us: std::mem::transmute::<*const c_void, _>(f("glColor3us")?),
                glColor4b: std::mem::transmute::<*const c_void, _>(f("glColor4b")?),
                glColor4d: std::mem::transmute::<*const c_void, _>(f("glColor4d")?),
                glColor4f: std::mem::transmute::<*const c_void, _>(f("glColor4f")?),
                glColor4i: std::mem::transmute::<*const c_void, _>(f("glColor4i")?),
                glColor4s: std::mem::transmute::<*const c_void, _>(f("glColor4s")?),
                glColor4ub: std::mem::transmute::<*const c_void, _>(f("glColor4ub")?),
                glColor4ui: std::mem::transmute::<*const c_void, _>(f("glColor4ui")?),
                glColor4us: std::mem::transmute::<*const c_void, _>(f("glColor4us")?),
                glColor3bv: std::mem::transmute::<*const c_void, _>(f("glColor3bv")?),
                glColor3dv: std::mem::transmute::<*const c_void, _>(f("glColor3dv")?),
                glColor3fv: std::mem::transmute::<*const c_void, _>(f("glColor3fv")?),
                glColor3iv: std::mem::transmute::<*const c_void, _>(f("glColor3iv")?),
                glColor3sv: std::mem::transmute::<*const c_void, _>(f("glColor3sv")?),
                glColor3ubv: std::mem::transmute::<*const c_void, _>(f("glColor3ubv")?),
                glColor3uiv: std::mem::transmute::<*const c_void, _>(f("glColor3uiv")?),
                glColor3usv: std::mem::transmute::<*const c_void, _>(f("glColor3usv")?),
                glColor4bv: std::mem::transmute::<*const c_void, _>(f("glColor4bv")?),
                glColor4dv: std::mem::transmute::<*const c_void, _>(f("glColor4dv")?),
                glColor4fv: std::mem::transmute::<*const c_void, _>(f("glColor4fv")?),
                glColor4iv: std::mem::transmute::<*const c_void, _>(f("glColor4iv")?),
                glColor4sv: std::mem::transmute::<*const c_void, _>(f("glColor4sv")?),
                glColor4ubv: std::mem::transmute::<*const c_void, _>(f("glColor4ubv")?),
                glColor4uiv: std::mem::transmute::<*const c_void, _>(f("glColor4uiv")?),
                glColor4usv: std::mem::transmute::<*const c_void, _>(f("glColor4usv")?),
                glTexCoord1d: std::mem::transmute::<*const c_void, _>(f("glTexCoord1d")?),
                glTexCoord1f: std::mem::transmute::<*const c_void, _>(f("glTexCoord1f")?),
                glTexCoord1i: std::mem::transmute::<*const c_void, _>(f("glTexCoord1i")?),
                glTexCoord1s: std::mem::transmute::<*const c_void, _>(f("glTexCoord1s")?),
                glTexCoord2d: std::mem::transmute::<*const c_void, _>(f("glTexCoord2d")?),
                glTexCoord2f: std::mem::transmute::<*const c_void, _>(f("glTexCoord2f")?),
                glTexCoord2i: std::mem::transmute::<*const c_void, _>(f("glTexCoord2i")?),
                glTexCoord2s: std::mem::transmute::<*const c_void, _>(f("glTexCoord2s")?),
                glTexCoord3d: std::mem::transmute::<*const c_void, _>(f("glTexCoord3d")?),
                glTexCoord3f: std::mem::transmute::<*const c_void, _>(f("glTexCoord3f")?),
                glTexCoord3i: std::mem::transmute::<*const c_void, _>(f("glTexCoord3i")?),
                glTexCoord3s: std::mem::transmute::<*const c_void, _>(f("glTexCoord3s")?),
                glTexCoord4d: std::mem::transmute::<*const c_void, _>(f("glTexCoord4d")?),
                glTexCoord4f: std::mem::transmute::<*const c_void, _>(f("glTexCoord4f")?),
                glTexCoord4i: std::mem::transmute::<*const c_void, _>(f("glTexCoord4i")?),
                glTexCoord4s: std::mem::transmute::<*const c_void, _>(f("glTexCoord4s")?),
                glTexCoord1dv: std::mem::transmute::<*const c_void, _>(f("glTexCoord1dv")?),
                glTexCoord1fv: std::mem::transmute::<*const c_void, _>(f("glTexCoord1fv")?),
                glTexCoord1iv: std::mem::transmute::<*const c_void, _>(f("glTexCoord1iv")?),
                glTexCoord1sv: std::mem::transmute::<*const c_void, _>(f("glTexCoord1sv")?),
                glTexCoord2dv: std::mem::transmute::<*const c_void, _>(f("glTexCoord2dv")?),
                glTexCoord2fv: std::mem::transmute::<*const c_void, _>(f("glTexCoord2fv")?),
                glTexCoord2iv: std::mem::transmute::<*const c_void, _>(f("glTexCoord2iv")?),
                glTexCoord2sv: std::mem::transmute::<*const c_void, _>(f("glTexCoord2sv")?),
                glTexCoord3dv: std::mem::transmute::<*const c_void, _>(f("glTexCoord3dv")?),
                glTexCoord3fv: std::mem::transmute::<*const c_void, _>(f("glTexCoord3fv")?),
                glTexCoord3iv: std::mem::transmute::<*const c_void, _>(f("glTexCoord3iv")?),
                glTexCoord3sv: std::mem::transmute::<*const c_void, _>(f("glTexCoord3sv")?),
                glTexCoord4dv: std::mem::transmute::<*const c_void, _>(f("glTexCoord4dv")?),
                glTexCoord4fv: std::mem::transmute::<*const c_void, _>(f("glTexCoord4fv")?),
                glTexCoord4iv: std::mem::transmute::<*const c_void, _>(f("glTexCoord4iv")?),
                glTexCoord4sv: std::mem::transmute::<*const c_void, _>(f("glTexCoord4sv")?),
                glRasterPos2d: std::mem::transmute::<*const c_void, _>(f("glRasterPos2d")?),
                glRasterPos2f: std::mem::transmute::<*const c_void, _>(f("glRasterPos2f")?),
                glRasterPos2i: std::mem::transmute::<*const c_void, _>(f("glRasterPos2i")?),
                glRasterPos2s: std::mem::transmute::<*const c_void, _>(f("glRasterPos2s")?),
                glRasterPos3d: std::mem::transmute::<*const c_void, _>(f("glRasterPos3d")?),
                glRasterPos3f: std::mem::transmute::<*const c_void, _>(f("glRasterPos3f")?),
                glRasterPos3i: std::mem::transmute::<*const c_void, _>(f("glRasterPos3i")?),
                glRasterPos3s: std::mem::transmute::<*const c_void, _>(f("glRasterPos3s")?),
                glRasterPos4d: std::mem::transmute::<*const c_void, _>(f("glRasterPos4d")?),
                glRasterPos4f: std::mem::transmute::<*const c_void, _>(f("glRasterPos4f")?),
                glRasterPos4i: std::mem::transmute::<*const c_void, _>(f("glRasterPos4i")?),
                glRasterPos4s: std::mem::transmute::<*const c_void, _>(f("glRasterPos4s")?),
                glRasterPos2dv: std::mem::transmute::<*const c_void, _>(f("glRasterPos2dv")?),
                glRasterPos2fv: std::mem::transmute::<*const c_void, _>(f("glRasterPos2fv")?),
                glRasterPos2iv: std::mem::transmute::<*const c_void, _>(f("glRasterPos2iv")?),
                glRasterPos2sv: std::mem::transmute::<*const c_void, _>(f("glRasterPos2sv")?),
                glRasterPos3dv: std::mem::transmute::<*const c_void, _>(f("glRasterPos3dv")?),
                glRasterPos3fv: std::mem::transmute::<*const c_void, _>(f("glRasterPos3fv")?),
                glRasterPos3iv: std::mem::transmute::<*const c_void, _>(f("glRasterPos3iv")?),
                glRasterPos3sv: std::mem::transmute::<*const c_void, _>(f("glRasterPos3sv")?),
                glRasterPos4dv: std::mem::transmute::<*const c_void, _>(f("glRasterPos4dv")?),
                glRasterPos4fv: std::mem::transmute::<*const c_void, _>(f("glRasterPos4fv")?),
                glRasterPos4iv: std::mem::transmute::<*const c_void, _>(f("glRasterPos4iv")?),
                glRasterPos4sv: std::mem::transmute::<*const c_void, _>(f("glRasterPos4sv")?),
                glRectd: std::mem::transmute::<*const c_void, _>(f("glRectd")?),
                glRectf: std::mem::transmute::<*const c_void, _>(f("glRectf")?),
                glRecti: std::mem::transmute::<*const c_void, _>(f("glRecti")?),
                glRects: std::mem::transmute::<*const c_void, _>(f("glRects")?),
                glRectdv: std::mem::transmute::<*const c_void, _>(f("glRectdv")?),
                glRectfv: std::mem::transmute::<*const c_void, _>(f("glRectfv")?),
                glRectiv: std::mem::transmute::<*const c_void, _>(f("glRectiv")?),
                glRectsv: std::mem::transmute::<*const c_void, _>(f("glRectsv")?),
                glVertexPointer: std::mem::transmute::<*const c_void, _>(f("glVertexPointer")?),
                glNormalPointer: std::mem::transmute::<*const c_void, _>(f("glNormalPointer")?),
                glColorPointer: std::mem::transmute::<*const c_void, _>(f("glColorPointer")?),
                glIndexPointer: std::mem::transmute::<*const c_void, _>(f("glIndexPointer")?),
                glTexCoordPointer: std::mem::transmute::<*const c_void, _>(f("glTexCoordPointer")?),
                glEdgeFlagPointer: std::mem::transmute::<*const c_void, _>(f("glEdgeFlagPointer")?),
                glGetPointerv: std::mem::transmute::<*const c_void, _>(f("glGetPointerv")?),
                glArrayElement: std::mem::transmute::<*const c_void, _>(f("glArrayElement")?),
                glDrawArrays: std::mem::transmute::<*const c_void, _>(f("glDrawArrays")?),
                glDrawElements: std::mem::transmute::<*const c_void, _>(f("glDrawElements")?),
                glInterleavedArrays: std::mem::transmute::<*const c_void, _>(f("glInterleavedArrays")?),
                glShadeModel: std::mem::transmute::<*const c_void, _>(f("glShadeModel")?),
                glLightf: std::mem::transmute::<*const c_void, _>(f("glLightf")?),
                glLighti: std::mem::transmute::<*const c_void, _>(f("glLighti")?),
                glLightfv: std::mem::transmute::<*const c_void, _>(f("glLightfv")?),
                glLightiv: std::mem::transmute::<*const c_void, _>(f("glLightiv")?),
                glGetLightfv: std::mem::transmute::<*const c_void, _>(f("glGetLightfv")?),
                glGetLightiv: std::mem::transmute::<*const c_void, _>(f("glGetLightiv")?),
                glLightModelf: std::mem::transmute::<*const c_void, _>(f("glLightModelf")?),
                glLightModeli: std::mem::transmute::<*const c_void, _>(f("glLightModeli")?),
                glLightModelfv: std::mem::transmute::<*const c_void, _>(f("glLightModelfv")?),
                glLightModeliv: std::mem::transmute::<*const c_void, _>(f("glLightModeliv")?),
                glMaterialf: std::mem::transmute::<*const c_void, _>(f("glMaterialf")?),
                glMateriali: std::mem::transmute::<*const c_void, _>(f("glMateriali")?),
                glMaterialfv: std::mem::transmute::<*const c_void, _>(f("glMaterialfv")?),
                glMaterialiv: std::mem::transmute::<*const c_void, _>(f("glMaterialiv")?),
                glGetMaterialfv: std::mem::transmute::<*const c_void, _>(f("glGetMaterialfv")?),
                glGetMaterialiv: std::mem::transmute::<*const c_void, _>(f("glGetMaterialiv")?),
                glColorMaterial: std::mem::transmute::<*const c_void, _>(f("glColorMaterial")?),
                glPixelZoom: std::mem::transmute::<*const c_void, _>(f("glPixelZoom")?),
                glPixelStoref: std::mem::transmute::<*const c_void, _>(f("glPixelStoref")?),
                glPixelStorei: std::mem::transmute::<*const c_void, _>(f("glPixelStorei")?),
                glPixelTransferf: std::mem::transmute::<*const c_void, _>(f("glPixelTransferf")?),
                glPixelTransferi: std::mem::transmute::<*const c_void, _>(f("glPixelTransferi")?),
                glPixelMapfv: std::mem::transmute::<*const c_void, _>(f("glPixelMapfv")?),
                glPixelMapuiv: std::mem::transmute::<*const c_void, _>(f("glPixelMapuiv")?),
                glPixelMapusv: std::mem::transmute::<*const c_void, _>(f("glPixelMapusv")?),
                glGetPixelMapfv: std::mem::transmute::<*const c_void, _>(f("glGetPixelMapfv")?),
                glGetPixelMapuiv: std::mem::transmute::<*const c_void, _>(f("glGetPixelMapuiv")?),
                glGetPixelMapusv: std::mem::transmute::<*const c_void, _>(f("glGetPixelMapusv")?),
                glBitmap: std::mem::transmute::<*const c_void, _>(f("glBitmap")?),
                glReadPixels: std::mem::transmute::<*const c_void, _>(f("glReadPixels")?),
                glDrawPixels: std::mem::transmute::<*const c_void, _>(f("glDrawPixels")?),
                glCopyPixels: std::mem::transmute::<*const c_void, _>(f("glCopyPixels")?),
                glStencilFunc: std::mem::transmute::<*const c_void, _>(f("glStencilFunc")?),
                glStencilMask: std::mem::transmute::<*const c_void, _>(f("glStencilMask")?),
                glStencilOp: std::mem::transmute::<*const c_void, _>(f("glStencilOp")?),
                glClearStencil: std::mem::transmute::<*const c_void, _>(f("glClearStencil")?),
                glTexGend: std::mem::transmute::<*const c_void, _>(f("glTexGend")?),
                glTexGenf: std::mem::transmute::<*const c_void, _>(f("glTexGenf")?),
                glTexGeni: std::mem::transmute::<*const c_void, _>(f("glTexGeni")?),
                glTexGendv: std::mem::transmute::<*const c_void, _>(f("glTexGendv")?),
                glTexGenfv: std::mem::transmute::<*const c_void, _>(f("glTexGenfv")?),
                glTexGeniv: std::mem::transmute::<*const c_void, _>(f("glTexGeniv")?),
                glGetTexGendv: std::mem::transmute::<*const c_void, _>(f("glGetTexGendv")?),
                glGetTexGenfv: std::mem::transmute::<*const c_void, _>(f("glGetTexGenfv")?),
                glGetTexGeniv: std::mem::transmute::<*const c_void, _>(f("glGetTexGeniv")?),
                glTexEnvf: std::mem::transmute::<*const c_void, _>(f("glTexEnvf")?),
                glTexEnvi: std::mem::transmute::<*const c_void, _>(f("glTexEnvi")?),
                glTexEnvfv: std::mem::transmute::<*const c_void, _>(f("glTexEnvfv")?),
                glTexEnviv: std::mem::transmute::<*const c_void, _>(f("glTexEnviv")?),
                glGetTexEnvfv: std::mem::transmute::<*const c_void, _>(f("glGetTexEnvfv")?),
                glGetTexEnviv: std::mem::transmute::<*const c_void, _>(f("glGetTexEnviv")?),
                glTexParameterf: std::mem::transmute::<*const c_void, _>(f("glTexParameterf")?),
                glTexParameteri: std::mem::transmute::<*const c_void, _>(f("glTexParameteri")?),
                glTexParameterfv: std::mem::transmute::<*const c_void, _>(f("glTexParameterfv")?),
                glTexParameteriv: std::mem::transmute::<*const c_void, _>(f("glTexParameteriv")?),
                glGetTexParameterfv: std::mem::transmute::<*const c_void, _>(f("glGetTexParameterfv")?),
                glGetTexParameteriv: std::mem::transmute::<*const c_void, _>(f("glGetTexParameteriv")?),
                glGetTexLevelParameterfv: std::mem::transmute::<*const c_void, _>(f("glGetTexLevelParameterfv")?),
                glGetTexLevelParameteriv: std::mem::transmute::<*const c_void, _>(f("glGetTexLevelParameteriv")?),
                glTexImage1D: std::mem::transmute::<*const c_void, _>(f("glTexImage1D")?),
                glTexImage2D: std::mem::transmute::<*const c_void, _>(f("glTexImage2D")?),
                glGetTexImage: std::mem::transmute::<*const c_void, _>(f("glGetTexImage")?),
                glGenTextures: std::mem::transmute::<*const c_void, _>(f("glGenTextures")?),
                glDeleteTextures: std::mem::transmute::<*const c_void, _>(f("glDeleteTextures")?),
                glBindTexture: std::mem::transmute::<*const c_void, _>(f("glBindTexture")?),
                glPrioritizeTextures: std::mem::transmute::<*const c_void, _>(f("glPrioritizeTextures")?),
                glAreTexturesResident: std::mem::transmute::<*const c_void, _>(f("glAreTexturesResident")?),
                glIsTexture: std::mem::transmute::<*const c_void, _>(f("glIsTexture")?),
                glTexSubImage1D: std::mem::transmute::<*const c_void, _>(f("glTexSubImage1D")?),
                glTexSubImage2D: std::mem::transmute::<*const c_void, _>(f("glTexSubImage2D")?),
                glCopyTexImage1D: std::mem::transmute::<*const c_void, _>(f("glCopyTexImage1D")?),
                glCopyTexImage2D: std::mem::transmute::<*const c_void, _>(f("glCopyTexImage2D")?),
                glCopyTexSubImage1D: std::mem::transmute::<*const c_void, _>(f("glCopyTexSubImage1D")?),
                glCopyTexSubImage2D: std::mem::transmute::<*const c_void, _>(f("glCopyTexSubImage2D")?),
                glMap1d: std::mem::transmute::<*const c_void, _>(f("glMap1d")?),
                glMap1f: std::mem::transmute::<*const c_void, _>(f("glMap1f")?),
                glMap2d: std::mem::transmute::<*const c_void, _>(f("glMap2d")?),
                glMap2f: std::mem::transmute::<*const c_void, _>(f("glMap2f")?),
                glGetMapdv: std::mem::transmute::<*const c_void, _>(f("glGetMapdv")?),
                glGetMapfv: std::mem::transmute::<*const c_void, _>(f("glGetMapfv")?),
                glGetMapiv: std::mem::transmute::<*const c_void, _>(f("glGetMapiv")?),
                glEvalCoord1d: std::mem::transmute::<*const c_void, _>(f("glEvalCoord1d")?),
                glEvalCoord1f: std::mem::transmute::<*const c_void, _>(f("glEvalCoord1f")?),
                glEvalCoord1dv: std::mem::transmute::<*const c_void, _>(f("glEvalCoord1dv")?),
                glEvalCoord1fv: std::mem::transmute::<*const c_void, _>(f("glEvalCoord1fv")?),
                glEvalCoord2d: std::mem::transmute::<*const c_void, _>(f("glEvalCoord2d")?),
                glEvalCoord2f: std::mem::transmute::<*const c_void, _>(f("glEvalCoord2f")?),
                glEvalCoord2dv: std::mem::transmute::<*const c_void, _>(f("glEvalCoord2dv")?),
                glEvalCoord2fv: std::mem::transmute::<*const c_void, _>(f("glEvalCoord2fv")?),
                glMapGrid1d: std::mem::transmute::<*const c_void, _>(f("glMapGrid1d")?),
                glMapGrid1f: std::mem::transmute::<*const c_void, _>(f("glMapGrid1f")?),
                glMapGrid2d: std::mem::transmute::<*const c_void, _>(f("glMapGrid2d")?),
                glMapGrid2f: std::mem::transmute::<*const c_void, _>(f("glMapGrid2f")?),
                glEvalPoint1: std::mem::transmute::<*const c_void, _>(f("glEvalPoint1")?),
                glEvalPoint2: std::mem::transmute::<*const c_void, _>(f("glEvalPoint2")?),
                glEvalMesh1: std::mem::transmute::<*const c_void, _>(f("glEvalMesh1")?),
                glEvalMesh2: std::mem::transmute::<*const c_void, _>(f("glEvalMesh2")?),
                glFogf: std::mem::transmute::<*const c_void, _>(f("glFogf")?),
                glFogi: std::mem::transmute::<*const c_void, _>(f("glFogi")?),
                glFogfv: std::mem::transmute::<*const c_void, _>(f("glFogfv")?),
                glFogiv: std::mem::transmute::<*const c_void, _>(f("glFogiv")?),
                glFeedbackBuffer: std::mem::transmute::<*const c_void, _>(f("glFeedbackBuffer")?),
                glPassThrough: std::mem::transmute::<*const c_void, _>(f("glPassThrough")?),
                glSelectBuffer: std::mem::transmute::<*const c_void, _>(f("glSelectBuffer")?),
                glInitNames: std::mem::transmute::<*const c_void, _>(f("glInitNames")?),
                glLoadName: std::mem::transmute::<*const c_void, _>(f("glLoadName")?),
                glPushName: std::mem::transmute::<*const c_void, _>(f("glPushName")?),
                glPopName: std::mem::transmute::<*const c_void, _>(f("glPopName")?),
            })
        }
    }

    /// Rejoue un appel simple ou à vecteur ; `None` pour ceux qui
    /// demandent du code écrit à la main.
    ///
    /// # Safety
    /// Contexte OpenGL de ces fonctions courant sur ce thread.
    pub unsafe fn call(&self, op: u16, a: &[u32]) -> Option<()> {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let d = |k: usize| f64::from_bits(u64::from(w(k)) | u64::from(w(k + 1)) << 32);
        let v = |k: usize| { let mut b = [0u64; 32]; for (i, x) in a.iter().skip(k).take(64).enumerate() { b[i / 2] |= u64::from(*x) << (32 * (i % 2)); } b };
        let _ = (&d, &v);
        // SAFETY: arguments décodés selon la signature de chaque fonction ;
        // les vecteurs pointent vers des copies locales assez grandes.
        unsafe {
            match op {
                1 => (self.glClearIndex)(f32::from_bits(w(0))),
                2 => (self.glClearColor)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                3 => (self.glClear)(w(0)),
                4 => (self.glIndexMask)(w(0)),
                5 => (self.glColorMask)(w(0) as u8, w(1) as u8, w(2) as u8, w(3) as u8),
                6 => (self.glAlphaFunc)(w(0), f32::from_bits(w(1))),
                7 => (self.glBlendFunc)(w(0), w(1)),
                8 => (self.glLogicOp)(w(0)),
                9 => (self.glCullFace)(w(0)),
                10 => (self.glFrontFace)(w(0)),
                11 => (self.glPointSize)(f32::from_bits(w(0))),
                12 => (self.glLineWidth)(f32::from_bits(w(0))),
                13 => (self.glLineStipple)(w(0) as i32, w(1) as u16),
                14 => (self.glPolygonMode)(w(0), w(1)),
                15 => (self.glPolygonOffset)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                16 => { let v0 = v(0); (self.glPolygonStipple)(v0.as_ptr() as *const c_void); }
                18 => (self.glEdgeFlag)(w(0) as u8),
                19 => { let v0 = v(0); (self.glEdgeFlagv)(v0.as_ptr() as *const c_void); }
                20 => (self.glScissor)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                21 => { let v0 = v(1); (self.glClipPlane)(w(0), v0.as_ptr() as *const c_void); }
                25 => (self.glEnable)(w(0)),
                26 => (self.glDisable)(w(0)),
                34 => (self.glPushAttrib)(w(0)),
                35 => (self.glPopAttrib)(),
                43 => (self.glHint)(w(0), w(1)),
                44 => (self.glClearDepth)(d(0)),
                45 => (self.glDepthFunc)(w(0)),
                46 => (self.glDepthMask)(w(0) as u8),
                47 => (self.glDepthRange)(d(0), d(2)),
                48 => (self.glClearAccum)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                49 => (self.glAccum)(w(0), f32::from_bits(w(1))),
                50 => (self.glMatrixMode)(w(0)),
                51 => (self.glOrtho)(d(0), d(2), d(4), d(6), d(8), d(10)),
                52 => (self.glFrustum)(d(0), d(2), d(4), d(6), d(8), d(10)),
                53 => (self.glViewport)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                54 => (self.glPushMatrix)(),
                55 => (self.glPopMatrix)(),
                56 => (self.glLoadIdentity)(),
                57 => { let v0 = v(0); (self.glLoadMatrixd)(v0.as_ptr() as *const c_void); }
                58 => { let v0 = v(0); (self.glLoadMatrixf)(v0.as_ptr() as *const c_void); }
                59 => { let v0 = v(0); (self.glMultMatrixd)(v0.as_ptr() as *const c_void); }
                60 => { let v0 = v(0); (self.glMultMatrixf)(v0.as_ptr() as *const c_void); }
                61 => (self.glRotated)(d(0), d(2), d(4), d(6)),
                62 => (self.glRotatef)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                63 => (self.glScaled)(d(0), d(2), d(4)),
                64 => (self.glScalef)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                65 => (self.glTranslated)(d(0), d(2), d(4)),
                66 => (self.glTranslatef)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                68 => (self.glDeleteLists)(w(0), w(1) as i32),
                70 => (self.glNewList)(w(0), w(1)),
                71 => (self.glEndList)(),
                72 => (self.glCallList)(w(0)),
                74 => (self.glListBase)(w(0)),
                75 => (self.glBegin)(w(0)),
                76 => (self.glEnd)(),
                77 => (self.glVertex2d)(d(0), d(2)),
                78 => (self.glVertex2f)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                79 => (self.glVertex2i)(w(0) as i32, w(1) as i32),
                80 => (self.glVertex2s)(w(0) as i16, w(1) as i16),
                81 => (self.glVertex3d)(d(0), d(2), d(4)),
                82 => (self.glVertex3f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                83 => (self.glVertex3i)(w(0) as i32, w(1) as i32, w(2) as i32),
                84 => (self.glVertex3s)(w(0) as i16, w(1) as i16, w(2) as i16),
                85 => (self.glVertex4d)(d(0), d(2), d(4), d(6)),
                86 => (self.glVertex4f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                87 => (self.glVertex4i)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                88 => (self.glVertex4s)(w(0) as i16, w(1) as i16, w(2) as i16, w(3) as i16),
                89 => { let v0 = v(0); (self.glVertex2dv)(v0.as_ptr() as *const c_void); }
                90 => { let v0 = v(0); (self.glVertex2fv)(v0.as_ptr() as *const c_void); }
                91 => { let v0 = v(0); (self.glVertex2iv)(v0.as_ptr() as *const c_void); }
                92 => { let v0 = v(0); (self.glVertex2sv)(v0.as_ptr() as *const c_void); }
                93 => { let v0 = v(0); (self.glVertex3dv)(v0.as_ptr() as *const c_void); }
                94 => { let v0 = v(0); (self.glVertex3fv)(v0.as_ptr() as *const c_void); }
                95 => { let v0 = v(0); (self.glVertex3iv)(v0.as_ptr() as *const c_void); }
                96 => { let v0 = v(0); (self.glVertex3sv)(v0.as_ptr() as *const c_void); }
                97 => { let v0 = v(0); (self.glVertex4dv)(v0.as_ptr() as *const c_void); }
                98 => { let v0 = v(0); (self.glVertex4fv)(v0.as_ptr() as *const c_void); }
                99 => { let v0 = v(0); (self.glVertex4iv)(v0.as_ptr() as *const c_void); }
                100 => { let v0 = v(0); (self.glVertex4sv)(v0.as_ptr() as *const c_void); }
                101 => (self.glNormal3b)(w(0) as i8, w(1) as i8, w(2) as i8),
                102 => (self.glNormal3d)(d(0), d(2), d(4)),
                103 => (self.glNormal3f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                104 => (self.glNormal3i)(w(0) as i32, w(1) as i32, w(2) as i32),
                105 => (self.glNormal3s)(w(0) as i16, w(1) as i16, w(2) as i16),
                106 => { let v0 = v(0); (self.glNormal3bv)(v0.as_ptr() as *const c_void); }
                107 => { let v0 = v(0); (self.glNormal3dv)(v0.as_ptr() as *const c_void); }
                108 => { let v0 = v(0); (self.glNormal3fv)(v0.as_ptr() as *const c_void); }
                109 => { let v0 = v(0); (self.glNormal3iv)(v0.as_ptr() as *const c_void); }
                110 => { let v0 = v(0); (self.glNormal3sv)(v0.as_ptr() as *const c_void); }
                111 => (self.glIndexd)(d(0)),
                112 => (self.glIndexf)(f32::from_bits(w(0))),
                113 => (self.glIndexi)(w(0) as i32),
                114 => (self.glIndexs)(w(0) as i16),
                115 => (self.glIndexub)(w(0) as u8),
                116 => { let v0 = v(0); (self.glIndexdv)(v0.as_ptr() as *const c_void); }
                117 => { let v0 = v(0); (self.glIndexfv)(v0.as_ptr() as *const c_void); }
                118 => { let v0 = v(0); (self.glIndexiv)(v0.as_ptr() as *const c_void); }
                119 => { let v0 = v(0); (self.glIndexsv)(v0.as_ptr() as *const c_void); }
                120 => { let v0 = v(0); (self.glIndexubv)(v0.as_ptr() as *const c_void); }
                121 => (self.glColor3b)(w(0) as i8, w(1) as i8, w(2) as i8),
                122 => (self.glColor3d)(d(0), d(2), d(4)),
                123 => (self.glColor3f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                124 => (self.glColor3i)(w(0) as i32, w(1) as i32, w(2) as i32),
                125 => (self.glColor3s)(w(0) as i16, w(1) as i16, w(2) as i16),
                126 => (self.glColor3ub)(w(0) as u8, w(1) as u8, w(2) as u8),
                127 => (self.glColor3ui)(w(0), w(1), w(2)),
                128 => (self.glColor3us)(w(0) as u16, w(1) as u16, w(2) as u16),
                129 => (self.glColor4b)(w(0) as i8, w(1) as i8, w(2) as i8, w(3) as i8),
                130 => (self.glColor4d)(d(0), d(2), d(4), d(6)),
                131 => (self.glColor4f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                132 => (self.glColor4i)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                133 => (self.glColor4s)(w(0) as i16, w(1) as i16, w(2) as i16, w(3) as i16),
                134 => (self.glColor4ub)(w(0) as u8, w(1) as u8, w(2) as u8, w(3) as u8),
                135 => (self.glColor4ui)(w(0), w(1), w(2), w(3)),
                136 => (self.glColor4us)(w(0) as u16, w(1) as u16, w(2) as u16, w(3) as u16),
                137 => { let v0 = v(0); (self.glColor3bv)(v0.as_ptr() as *const c_void); }
                138 => { let v0 = v(0); (self.glColor3dv)(v0.as_ptr() as *const c_void); }
                139 => { let v0 = v(0); (self.glColor3fv)(v0.as_ptr() as *const c_void); }
                140 => { let v0 = v(0); (self.glColor3iv)(v0.as_ptr() as *const c_void); }
                141 => { let v0 = v(0); (self.glColor3sv)(v0.as_ptr() as *const c_void); }
                142 => { let v0 = v(0); (self.glColor3ubv)(v0.as_ptr() as *const c_void); }
                143 => { let v0 = v(0); (self.glColor3uiv)(v0.as_ptr() as *const c_void); }
                144 => { let v0 = v(0); (self.glColor3usv)(v0.as_ptr() as *const c_void); }
                145 => { let v0 = v(0); (self.glColor4bv)(v0.as_ptr() as *const c_void); }
                146 => { let v0 = v(0); (self.glColor4dv)(v0.as_ptr() as *const c_void); }
                147 => { let v0 = v(0); (self.glColor4fv)(v0.as_ptr() as *const c_void); }
                148 => { let v0 = v(0); (self.glColor4iv)(v0.as_ptr() as *const c_void); }
                149 => { let v0 = v(0); (self.glColor4sv)(v0.as_ptr() as *const c_void); }
                150 => { let v0 = v(0); (self.glColor4ubv)(v0.as_ptr() as *const c_void); }
                151 => { let v0 = v(0); (self.glColor4uiv)(v0.as_ptr() as *const c_void); }
                152 => { let v0 = v(0); (self.glColor4usv)(v0.as_ptr() as *const c_void); }
                153 => (self.glTexCoord1d)(d(0)),
                154 => (self.glTexCoord1f)(f32::from_bits(w(0))),
                155 => (self.glTexCoord1i)(w(0) as i32),
                156 => (self.glTexCoord1s)(w(0) as i16),
                157 => (self.glTexCoord2d)(d(0), d(2)),
                158 => (self.glTexCoord2f)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                159 => (self.glTexCoord2i)(w(0) as i32, w(1) as i32),
                160 => (self.glTexCoord2s)(w(0) as i16, w(1) as i16),
                161 => (self.glTexCoord3d)(d(0), d(2), d(4)),
                162 => (self.glTexCoord3f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                163 => (self.glTexCoord3i)(w(0) as i32, w(1) as i32, w(2) as i32),
                164 => (self.glTexCoord3s)(w(0) as i16, w(1) as i16, w(2) as i16),
                165 => (self.glTexCoord4d)(d(0), d(2), d(4), d(6)),
                166 => (self.glTexCoord4f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                167 => (self.glTexCoord4i)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                168 => (self.glTexCoord4s)(w(0) as i16, w(1) as i16, w(2) as i16, w(3) as i16),
                169 => { let v0 = v(0); (self.glTexCoord1dv)(v0.as_ptr() as *const c_void); }
                170 => { let v0 = v(0); (self.glTexCoord1fv)(v0.as_ptr() as *const c_void); }
                171 => { let v0 = v(0); (self.glTexCoord1iv)(v0.as_ptr() as *const c_void); }
                172 => { let v0 = v(0); (self.glTexCoord1sv)(v0.as_ptr() as *const c_void); }
                173 => { let v0 = v(0); (self.glTexCoord2dv)(v0.as_ptr() as *const c_void); }
                174 => { let v0 = v(0); (self.glTexCoord2fv)(v0.as_ptr() as *const c_void); }
                175 => { let v0 = v(0); (self.glTexCoord2iv)(v0.as_ptr() as *const c_void); }
                176 => { let v0 = v(0); (self.glTexCoord2sv)(v0.as_ptr() as *const c_void); }
                177 => { let v0 = v(0); (self.glTexCoord3dv)(v0.as_ptr() as *const c_void); }
                178 => { let v0 = v(0); (self.glTexCoord3fv)(v0.as_ptr() as *const c_void); }
                179 => { let v0 = v(0); (self.glTexCoord3iv)(v0.as_ptr() as *const c_void); }
                180 => { let v0 = v(0); (self.glTexCoord3sv)(v0.as_ptr() as *const c_void); }
                181 => { let v0 = v(0); (self.glTexCoord4dv)(v0.as_ptr() as *const c_void); }
                182 => { let v0 = v(0); (self.glTexCoord4fv)(v0.as_ptr() as *const c_void); }
                183 => { let v0 = v(0); (self.glTexCoord4iv)(v0.as_ptr() as *const c_void); }
                184 => { let v0 = v(0); (self.glTexCoord4sv)(v0.as_ptr() as *const c_void); }
                185 => (self.glRasterPos2d)(d(0), d(2)),
                186 => (self.glRasterPos2f)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                187 => (self.glRasterPos2i)(w(0) as i32, w(1) as i32),
                188 => (self.glRasterPos2s)(w(0) as i16, w(1) as i16),
                189 => (self.glRasterPos3d)(d(0), d(2), d(4)),
                190 => (self.glRasterPos3f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2))),
                191 => (self.glRasterPos3i)(w(0) as i32, w(1) as i32, w(2) as i32),
                192 => (self.glRasterPos3s)(w(0) as i16, w(1) as i16, w(2) as i16),
                193 => (self.glRasterPos4d)(d(0), d(2), d(4), d(6)),
                194 => (self.glRasterPos4f)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                195 => (self.glRasterPos4i)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                196 => (self.glRasterPos4s)(w(0) as i16, w(1) as i16, w(2) as i16, w(3) as i16),
                197 => { let v0 = v(0); (self.glRasterPos2dv)(v0.as_ptr() as *const c_void); }
                198 => { let v0 = v(0); (self.glRasterPos2fv)(v0.as_ptr() as *const c_void); }
                199 => { let v0 = v(0); (self.glRasterPos2iv)(v0.as_ptr() as *const c_void); }
                200 => { let v0 = v(0); (self.glRasterPos2sv)(v0.as_ptr() as *const c_void); }
                201 => { let v0 = v(0); (self.glRasterPos3dv)(v0.as_ptr() as *const c_void); }
                202 => { let v0 = v(0); (self.glRasterPos3fv)(v0.as_ptr() as *const c_void); }
                203 => { let v0 = v(0); (self.glRasterPos3iv)(v0.as_ptr() as *const c_void); }
                204 => { let v0 = v(0); (self.glRasterPos3sv)(v0.as_ptr() as *const c_void); }
                205 => { let v0 = v(0); (self.glRasterPos4dv)(v0.as_ptr() as *const c_void); }
                206 => { let v0 = v(0); (self.glRasterPos4fv)(v0.as_ptr() as *const c_void); }
                207 => { let v0 = v(0); (self.glRasterPos4iv)(v0.as_ptr() as *const c_void); }
                208 => { let v0 = v(0); (self.glRasterPos4sv)(v0.as_ptr() as *const c_void); }
                209 => (self.glRectd)(d(0), d(2), d(4), d(6)),
                210 => (self.glRectf)(f32::from_bits(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), f32::from_bits(w(3))),
                211 => (self.glRecti)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32),
                212 => (self.glRects)(w(0) as i16, w(1) as i16, w(2) as i16, w(3) as i16),
                213 => { let v0 = v(0); let v1 = v(4); (self.glRectdv)(v0.as_ptr() as *const c_void, v1.as_ptr() as *const c_void); }
                214 => { let v0 = v(0); let v1 = v(2); (self.glRectfv)(v0.as_ptr() as *const c_void, v1.as_ptr() as *const c_void); }
                215 => { let v0 = v(0); let v1 = v(2); (self.glRectiv)(v0.as_ptr() as *const c_void, v1.as_ptr() as *const c_void); }
                216 => { let v0 = v(0); let v1 = v(1); (self.glRectsv)(v0.as_ptr() as *const c_void, v1.as_ptr() as *const c_void); }
                228 => (self.glShadeModel)(w(0)),
                229 => (self.glLightf)(w(0), w(1), f32::from_bits(w(2))),
                230 => (self.glLighti)(w(0), w(1), w(2) as i32),
                231 => { let v0 = v(2); (self.glLightfv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                232 => { let v0 = v(2); (self.glLightiv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                235 => (self.glLightModelf)(w(0), f32::from_bits(w(1))),
                236 => (self.glLightModeli)(w(0), w(1) as i32),
                237 => { let v0 = v(1); (self.glLightModelfv)(w(0), v0.as_ptr() as *const c_void); }
                238 => { let v0 = v(1); (self.glLightModeliv)(w(0), v0.as_ptr() as *const c_void); }
                239 => (self.glMaterialf)(w(0), w(1), f32::from_bits(w(2))),
                240 => (self.glMateriali)(w(0), w(1), w(2) as i32),
                241 => { let v0 = v(2); (self.glMaterialfv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                242 => { let v0 = v(2); (self.glMaterialiv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                245 => (self.glColorMaterial)(w(0), w(1)),
                246 => (self.glPixelZoom)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                247 => (self.glPixelStoref)(w(0), f32::from_bits(w(1))),
                248 => (self.glPixelStorei)(w(0), w(1) as i32),
                249 => (self.glPixelTransferf)(w(0), f32::from_bits(w(1))),
                250 => (self.glPixelTransferi)(w(0), w(1) as i32),
                260 => (self.glCopyPixels)(w(0) as i32, w(1) as i32, w(2) as i32, w(3) as i32, w(4)),
                261 => (self.glStencilFunc)(w(0), w(1) as i32, w(2)),
                262 => (self.glStencilMask)(w(0)),
                263 => (self.glStencilOp)(w(0), w(1), w(2)),
                264 => (self.glClearStencil)(w(0) as i32),
                265 => (self.glTexGend)(w(0), w(1), d(2)),
                266 => (self.glTexGenf)(w(0), w(1), f32::from_bits(w(2))),
                267 => (self.glTexGeni)(w(0), w(1), w(2) as i32),
                268 => { let v0 = v(2); (self.glTexGendv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                269 => { let v0 = v(2); (self.glTexGenfv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                270 => { let v0 = v(2); (self.glTexGeniv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                274 => (self.glTexEnvf)(w(0), w(1), f32::from_bits(w(2))),
                275 => (self.glTexEnvi)(w(0), w(1), w(2) as i32),
                276 => { let v0 = v(2); (self.glTexEnvfv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                277 => { let v0 = v(2); (self.glTexEnviv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                280 => (self.glTexParameterf)(w(0), w(1), f32::from_bits(w(2))),
                281 => (self.glTexParameteri)(w(0), w(1), w(2) as i32),
                282 => { let v0 = v(2); (self.glTexParameterfv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                283 => { let v0 = v(2); (self.glTexParameteriv)(w(0), w(1), v0.as_ptr() as *const c_void); }
                293 => (self.glBindTexture)(w(0), w(1)),
                299 => (self.glCopyTexImage1D)(w(0), w(1) as i32, w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32),
                300 => (self.glCopyTexImage2D)(w(0), w(1) as i32, w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32, w(7) as i32),
                301 => (self.glCopyTexSubImage1D)(w(0), w(1) as i32, w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32),
                302 => (self.glCopyTexSubImage2D)(w(0), w(1) as i32, w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32, w(7) as i32),
                310 => (self.glEvalCoord1d)(d(0)),
                311 => (self.glEvalCoord1f)(f32::from_bits(w(0))),
                312 => { let v0 = v(0); (self.glEvalCoord1dv)(v0.as_ptr() as *const c_void); }
                313 => { let v0 = v(0); (self.glEvalCoord1fv)(v0.as_ptr() as *const c_void); }
                314 => (self.glEvalCoord2d)(d(0), d(2)),
                315 => (self.glEvalCoord2f)(f32::from_bits(w(0)), f32::from_bits(w(1))),
                316 => { let v0 = v(0); (self.glEvalCoord2dv)(v0.as_ptr() as *const c_void); }
                317 => { let v0 = v(0); (self.glEvalCoord2fv)(v0.as_ptr() as *const c_void); }
                318 => (self.glMapGrid1d)(w(0) as i32, d(1), d(3)),
                319 => (self.glMapGrid1f)(w(0) as i32, f32::from_bits(w(1)), f32::from_bits(w(2))),
                320 => (self.glMapGrid2d)(w(0) as i32, d(1), d(3), w(5) as i32, d(6), d(8)),
                321 => (self.glMapGrid2f)(w(0) as i32, f32::from_bits(w(1)), f32::from_bits(w(2)), w(3) as i32, f32::from_bits(w(4)), f32::from_bits(w(5))),
                322 => (self.glEvalPoint1)(w(0) as i32),
                323 => (self.glEvalPoint2)(w(0) as i32, w(1) as i32),
                324 => (self.glEvalMesh1)(w(0), w(1) as i32, w(2) as i32),
                325 => (self.glEvalMesh2)(w(0), w(1) as i32, w(2) as i32, w(3) as i32, w(4) as i32),
                326 => (self.glFogf)(w(0), f32::from_bits(w(1))),
                327 => (self.glFogi)(w(0), w(1) as i32),
                328 => { let v0 = v(1); (self.glFogfv)(w(0), v0.as_ptr() as *const c_void); }
                329 => { let v0 = v(1); (self.glFogiv)(w(0), v0.as_ptr() as *const c_void); }
                331 => (self.glPassThrough)(f32::from_bits(w(0))),
                333 => (self.glInitNames)(),
                334 => (self.glLoadName)(w(0)),
                335 => (self.glPushName)(w(0)),
                336 => (self.glPopName)(),
                _ => return None,
            }
        }
        Some(())
    }
}
