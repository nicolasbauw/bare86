#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Modèles de boîtes de dialogue (RT_DIALOG) d'un exécutable PE : style, identifiant, classe et texte de chaque élément
(pour savoir quel bouton une démo attend). Exécutable compressé par UPX : le décompresser d'abord (upx -d).
Usage : tools/win-dialogs.py EXE"""
import struct
import sys

def load(path):
    d = open(path, 'rb').read()
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    nsec = struct.unpack_from('<H', d, pe+6)[0]
    osz = struct.unpack_from('<H', d, pe+20)[0]
    opt = pe+24
    rrva = struct.unpack_from('<I', d, opt+96+16)[0]
    secs = []
    for i in range(nsec):
        s = opt+osz+40*i
        vs, va, rs, ro = struct.unpack_from('<IIII', d, s+8)
        secs.append((va, max(vs, rs), ro))
    def off(rva):
        for va, sz, ro in secs:
            if va <= rva < va+sz:
                return rva-va+ro
    return d, off, rrva

def walk(d, off, base, o, depth, path, out):
    n1, n2 = struct.unpack_from('<HH', d, o+12)
    for i in range(n1+n2):
        nm, ofs = struct.unpack_from('<II', d, o+16+8*i)
        if nm & 0x80000000:
            so = base + (nm & 0x7fffffff); ln = struct.unpack_from('<H', d, so)[0]
            key = d[so+2:so+2+2*ln].decode('utf-16le')
        else:
            key = nm
        if ofs & 0x80000000:
            walk(d, off, base, base+(ofs & 0x7fffffff), depth+1, path+[key], out)
        else:
            rva, size = struct.unpack_from('<II', d, base+ofs)
            out.append((path+[key], off(rva), size))

def sz(b, o):
    if struct.unpack_from('<H', b, o)[0] == 0xffff:
        return '#%d' % struct.unpack_from('<H', b, o+2)[0], o+4
    e = o
    while struct.unpack_from('<H', b, e)[0]:
        e += 2
    return b[o:e].decode('utf-16le'), e+2

def dialog(b):
    o = 0
    ex = struct.unpack_from('<HH', b, 0) == (1, 0xffff)
    if ex:
        _, _, hid, exs, st, n, x, y, cx, cy = struct.unpack_from('<HHIIIHhhhh', b, 0); o = 26
    else:
        st, exs, n, x, y, cx, cy = struct.unpack_from('<IIHhhhh', b, 0); o = 18
    menu, o = sz(b, o); cls, o = sz(b, o); title, o = sz(b, o)
    print('  DIALOG%s style=%08X n=%d %dx%d title=%r' % ('EX' if ex else '', st, n, cx, cy, title))
    if st & 0x40 or st & 0x48:
        o += 2 if not ex else 6
        _, o = sz(b, o)
    for i in range(n):
        o = (o+3) & ~3
        if ex:
            hid, exs, cst, x, y, cx, cy, cid = struct.unpack_from('<IIIhhhhI', b, o); o += 24
        else:
            cst, exs, x, y, cx, cy, cid = struct.unpack_from('<IIhhhhH', b, o); o += 18
        c, o = sz(b, o); t, o = sz(b, o)
        extra = struct.unpack_from('<H', b, o)[0]; o += 2 + extra
        print('    id=%-5d cls=%-10s style=%08X %r' % (cid, c, cst, t))

if __name__ == '__main__':
    d, off, rrva = load(sys.argv[1])
    out = []
    base = off(rrva)
    walk(d, off, base, base, 0, [], out)
    for p, o, s in out:
        if p[0] == 5:
            print(p)
            dialog(d[o:o+s])
