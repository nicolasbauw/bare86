#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Assemble glide2x.ovl (DLL au format LE, chargée par DOS/4GW) à partir d'un
exécutable ELF i386 lié à l'adresse 0 avec ses relocations (`--emit-relocs`).

  mkovl.py stubs OBJ.o            fonctions vides (C) des exportations absentes
  mkovl.py ovl IMAGE.elf OUT.ovl  DLL LE

Un seul objet (code et données, lecture, écriture, exécution) ; chaque
relocation absolue 32 bits devient une relocation interne LE (type 07h). Les
exportations (numéro et nom) sont celles de ovl_exports.txt ; chacune pointe
sur la fonction C de même nom à la casse près (`_GRGLIDEINIT@0` →
`grGlideInit`), ou sur sa fonction vide."""

import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PAGE = 4096


def exports():
    out = []
    for line in (HERE / "ovl_exports.txt").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        ordinal, name = line.split()
        out.append((int(ordinal), name))
    return out


def base_name(export):
    """`_GRGLIDEINIT@0` → (`grglideinit`, 0) ; `__DLLSTART_` → (`__dllstart_`, None)."""
    if "@" in export:
        name, n = export.rsplit("@", 1)
        return name.lstrip("_").lower(), int(n)
    return export.lower(), None


class Elf:
    def __init__(self, data):
        self.d = data
        assert data[:4] == b"\x7fELF" and data[4] == 1, "ELF32 attendu"
        (self.shoff,) = struct.unpack_from("<I", data, 0x20)
        self.shentsize, self.shnum, self.shstrndx = struct.unpack_from("<HHH", data, 0x2E)
        self.sections = []
        for i in range(self.shnum):
            f = struct.unpack_from("<10I", data, self.shoff + i * self.shentsize)
            self.sections.append(dict(name=f[0], type=f[1], flags=f[2], addr=f[3], off=f[4], size=f[5], link=f[6], info=f[7], entsize=f[9]))
        strtab = self.sections[self.shstrndx]
        for s in self.sections:
            s["name"] = self.cstr(strtab["off"] + s["name"])

    def cstr(self, at):
        return self.d[at : self.d.index(b"\0", at)].decode()

    def symbols(self):
        for s in self.sections:
            if s["type"] != 2:  # SHT_SYMTAB
                continue
            strs = self.sections[s["link"]]
            for k in range(s["size"] // 16):
                name, value, size, info, other, shndx = struct.unpack_from("<IIIBBH", self.d, s["off"] + k * 16)
                yield self.cstr(strs["off"] + name), value, info, shndx


def stubs(obj):
    elf = Elf(Path(obj).read_bytes())
    defined = {n.lower() for n, _, info, shndx in elf.symbols() if shndx != 0 and info >> 4 == 1}
    print("/* Généré par mkovl.py : exportations de glide2x.ovl sans équivalent. */")
    print("typedef unsigned int u32;")
    for _, export in exports():
        name, n = base_name(export)
        if name in defined:
            continue
        cname = "stub_" + name
        if n is None:
            print(f"u32 {cname}(void) {{ return 0; }}")
        else:
            args = ", ".join(f"u32 a{i}" for i in range(n // 4)) or "void"
            used = "".join(f" (void)a{i};" for i in range(n // 4))
            print(f"__attribute__((stdcall)) u32 {cname}({args}) {{{used} return 0; }}")


def ovl(elf_path, out_path):
    elf = Elf(Path(elf_path).read_bytes())
    alloc = [s for s in elf.sections if s["flags"] & 2 and s["size"]]  # SHF_ALLOC
    end = max(s["addr"] + s["size"] for s in alloc)
    init_end = max(s["addr"] + s["size"] for s in alloc if s["type"] != 8)  # hors NOBITS
    image = bytearray(init_end)
    for s in alloc:
        if s["type"] != 8:
            image[s["addr"] : s["addr"] + s["size"]] = elf.d[s["off"] : s["off"] + s["size"]]
    # Relocations absolues : les sections .rel.* visent une section chargée.
    fixups = []
    for s in elf.sections:
        if s["type"] != 9:  # SHT_REL
            continue
        target = elf.sections[s["info"]]
        if not target["flags"] & 2:
            continue
        for k in range(s["size"] // 8):
            r_off, r_info = struct.unpack_from("<II", elf.d, s["off"] + k * 8)
            kind = r_info & 0xFF
            at = r_off  # exécutable lié : adresse virtuelle (base 0)
            if kind == 1:  # R_386_32 : valeur déjà résolue pour la base 0
                fixups.append((at, struct.unpack_from("<I", image, at)[0]))
            elif kind in (2, 4):  # PC32, PLT32 : relatives, rien à faire
                pass
            else:
                sys.exit(f"relocation ELF {kind} non gérée")
    symbols = {}
    for name, value, info, shndx in elf.symbols():
        if shndx != 0 and info >> 4 == 1:
            symbols[name.lower()] = value
    entries = []
    for ordinal, export in exports():
        name, _ = base_name(export)
        addr = symbols.get(name, symbols.get("stub_" + name))
        if addr is None:
            sys.exit(f"{export} : ni fonction ni fonction vide")
        entries.append((ordinal, export, addr))

    npages = (len(image) + PAGE - 1) // PAGE
    vsize = (end + PAGE - 1) // PAGE * PAGE
    image += bytes(npages * PAGE - len(image))

    # Relocations par page (un champ à cheval sur deux pages figure dans les deux).
    per_page = [bytearray() for _ in range(npages)]
    for at, value in sorted(fixups):
        for page in {at // PAGE, (at + 3) // PAGE}:
            if page >= npages:
                continue
            src = at - page * PAGE
            if value > 0xFFFF:
                rec = struct.pack("<BBhBI", 0x07, 0x10, src, 1, value)
            else:
                rec = struct.pack("<BBhBH", 0x07, 0x00, src, 1, value)
            per_page[page] += rec

    def names(items):
        out = bytearray()
        for n, ordinal in items:
            b = n.encode()
            out += bytes([len(b)]) + b + struct.pack("<H", ordinal)
        return out + b"\0"

    # Tables du chargeur, relatives à l'en-tête LE.
    HDR = 0xC4
    objtab = struct.pack("<6I", vsize, 0x10000, 0x2007, 1, npages, 0)
    pagemap = b"".join(bytes([0, (p + 1) >> 8 & 0xFF, (p + 1) & 0xFF, 0]) for p in range(npages))
    resident = names([("glide2x", 0)] + [(e, o) for o, e, _ in entries])
    entry = bytearray()
    i = 0
    ordinals = sorted(entries)
    expected = 1
    while i < len(ordinals):
        if ordinals[i][0] != expected:
            gap = min(ordinals[i][0] - expected, 255)
            entry += bytes([gap, 0])
            expected += gap
            continue
        bundle = []
        while i < len(ordinals) and ordinals[i][0] == expected and len(bundle) < 255:
            bundle.append(ordinals[i][2])
            i += 1
            expected += 1
        entry += bytes([len(bundle), 3]) + struct.pack("<H", 1)
        for addr in bundle:
            entry += bytes([0x01]) + struct.pack("<I", addr)
    entry += b"\0"
    fixpage = bytearray()
    fixrec = bytearray()
    for recs in per_page:
        fixpage += struct.pack("<I", len(fixrec))
        fixrec += recs
    fixpage += struct.pack("<I", len(fixrec))

    o_obj = HDR
    o_pagemap = o_obj + len(objtab)
    o_res = o_pagemap + len(pagemap)
    o_entry = o_res + len(resident)
    o_fixpage = o_entry + len(entry)
    o_fixrec = o_fixpage + len(fixpage)
    o_imp = o_fixrec + len(fixrec)
    loader_size = o_fixpage - o_obj
    fixup_size = o_imp - o_fixpage
    LE_AT = 0x80
    nonres = names([("GLIDE2X.OVL", 0)])
    data_pages = (LE_AT + o_imp + PAGE - 1) // PAGE * PAGE
    o_nonres = data_pages + npages * PAGE

    h = bytearray(HDR)
    h[0:2] = b"LE"
    struct.pack_into("<HH", h, 8, 2, 1)  # 386, OS/2
    struct.pack_into("<I", h, 0x10, 0x8200)  # bibliothèque
    struct.pack_into("<I", h, 0x14, npages)
    struct.pack_into("<I", h, 0x28, PAGE)
    struct.pack_into("<I", h, 0x2C, init_end - (npages - 1) * PAGE)
    struct.pack_into("<I", h, 0x30, fixup_size)
    struct.pack_into("<I", h, 0x38, loader_size)
    struct.pack_into("<II", h, 0x40, o_obj, 1)
    struct.pack_into("<I", h, 0x48, o_pagemap)
    struct.pack_into("<I", h, 0x4C, 0)  # pages itérées
    struct.pack_into("<I", h, 0x58, o_res)
    struct.pack_into("<I", h, 0x5C, o_entry)
    struct.pack_into("<II", h, 0x68, o_fixpage, o_fixrec)
    struct.pack_into("<II", h, 0x70, o_imp, 0)
    struct.pack_into("<I", h, 0x78, o_imp)
    struct.pack_into("<I", h, 0x80, data_pages)
    struct.pack_into("<II", h, 0x88, o_nonres, len(nonres))

    mz = bytearray(LE_AT)
    mz[0:2] = b"MZ"
    struct.pack_into("<HHHHH", mz, 2, 0x80, 1, 0, 4, 0)
    struct.pack_into("<H", mz, 0x0C, 0xFFFF)
    struct.pack_into("<H", mz, 0x18, 0x40)
    struct.pack_into("<I", mz, 0x3C, LE_AT)
    # Talon : « mov ah,4Ch ; int 21h ».
    mz[0x40:0x44] = b"\xb4\x4c\xcd\x21"
    out = mz + h + objtab + pagemap + resident + entry + fixpage + fixrec
    out += bytes(data_pages - len(out))
    out += image + nonres
    Path(out_path).write_bytes(out)
    print(f"{out_path}: {len(entries)} exportations, {len(fixups)} relocations, {npages} pages, {vsize // 1024} Ko en mémoire")


if __name__ == "__main__":
    if sys.argv[1] == "stubs":
        stubs(sys.argv[2])
    else:
        ovl(sys.argv[2], sys.argv[3])
