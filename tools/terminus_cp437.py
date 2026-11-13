#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Extrait de Terminus (ter-u16n.bdf, OFL 1.1) les 256 glyphes CP437 8x16 :
fonts/terminus-cp437-8x16.bin, 16 octets par caractère, bit 7 = point de gauche.
Usage : tools/terminus_cp437.py chemin/ter-u16n.bdf > fonts/terminus-cp437-8x16.bin"""
import sys

LOW = "☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼"
HIGH = ("ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐"
        "└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ")

def cp437(b):
    if b == 0: return " "
    if b < 0x20: return LOW[b - 1]
    if b < 0x7F: return chr(b)
    if b == 0x7F: return "⌂"
    return HIGH[b - 0x80]

glyphs, enc, rows = {}, None, None
for line in open(sys.argv[1], encoding="ascii"):
    w = line.split()
    if not w: continue
    if w[0] == "ENCODING": enc = int(w[1])
    elif w[0] == "BBX": assert w[1:] == ["8", "16", "0", "-4"], line
    elif w[0] == "BITMAP": rows = []
    elif w[0] == "ENDCHAR": glyphs[enc] = bytes(rows); rows = None
    elif rows is not None: rows.append(int(w[0], 16))

out, missing = bytearray(), []
for b in range(256):
    c = ord(cp437(b))
    if c not in glyphs:
        missing.append(f"{b:02X}:{chr(c)}")
        c = 0xFFFD
    out += glyphs[c]
if missing:
    sys.exit("glyphes absents : " + " ".join(missing))
sys.stdout.buffer.write(out)
