#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""DLL importées (table d'imports statique) par des exécutables PE ; « ?packed » si la table est introuvable (compressé).
Usage : tools/pe-imports.py EXE...  (module : imports(chemin) -> {dll: [fonctions]})"""
import struct
import sys

def imports(path):
    d = open(path, 'rb').read()
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    if d[pe:pe+4] != b'PE\0\0':
        return None
    nsec = struct.unpack_from('<H', d, pe+6)[0]
    opt = pe + 24
    osz = struct.unpack_from('<H', d, pe+20)[0]
    irva = struct.unpack_from('<I', d, opt+104)[0]
    secs = []
    for i in range(nsec):
        s = opt + osz + 40*i
        vs, va, rs, ro = struct.unpack_from('<IIII', d, s+8)
        secs.append((va, max(vs, rs), ro, d[s:s+8].rstrip(b'\0').decode('latin1')))
    def off(rva):
        for va, sz, ro, _ in secs:
            if va <= rva < va+sz:
                return rva - va + ro
        return None
    def cstr(o):
        return d[o:d.index(b'\0', o)].decode('latin1')
    res = {}
    o = off(irva) if irva else None
    if o is None:
        return {'?packed': [s[3] for s in secs]}
    while True:
        ilt, _, _, name, iat = struct.unpack_from('<IIIII', d, o)
        if name == 0:
            break
        dll = cstr(off(name)).lower()
        fns = []
        t = off(ilt or iat)
        while t is not None:
            v = struct.unpack_from('<I', d, t)[0]
            if v == 0:
                break
            fns.append('#%d' % (v & 0xffff) if v & 0x80000000 else cstr(off(v)+2))
            t += 4
        res[dll] = fns
        o += 20
    return res

if __name__ == '__main__':
    for p in sys.argv[1:]:
        r = imports(p)
        print(p, ' '.join(sorted(r)) if r else r)
