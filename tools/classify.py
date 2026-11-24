#!/usr/bin/env python3 -I
# SPDX-License-Identifier: GPL-3.0-or-later
"""Classe les éléments d'une collection de démos : exécutable propre à l'élément, musique, image, ANSI, données.

  python3 -I tools/classify.py --src COLLECTION --items LISTE --out categories.json

Reconnaissance par le contenu, pas par le nom :
  - exécutables : en-tête « MZ » (DOS ; « LE »/« LX »/« NE »/« PE » à l'adresse e_lfanew donnent
    DOS étendu, Windows 16 bits, Win32), « .COM » (pas d'en-tête : extension, taille ≤ 64 Ko, aucune
    signature de données) ;
  - modules : « Extended Module: » (XM), « SCRM » à l'octet 44 (S3M), « IMPM » (IT), « M.K. » & co.
    à l'octet 1080 (MOD), « MTM », « 669 » ; images : GIF, PCX, BMP, JPEG, PNG, TGA/LBM par extension ;
  - fichiers « d'accompagnement » : un fichier de même empreinte (SHA-1) dans au moins BOILER éléments
    de noms différents (publicité d'un groupe, DOS4GW.EXE, lecteur de modules…) ne compte pas comme l'exécutable
    de l'élément.
Catégorie d'un élément : « exécutable » s'il a un exécutable qui n'est pas d'accompagnement ; sinon
« musique », « image », « ansi » ou « données » ; « pub seule » si seul un exécutable d'accompagnement
s'y trouve avec des données. Rend aussi les types des exécutables (dos, dos-étendu, win16, win32, com)."""

import argparse
import collections
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import survey  # noqa: E402  (extract, SKIP_NAMES)

BOILER = 3
MOD_EXT = {".mod", ".s3m", ".xm", ".it", ".669", ".mtm", ".stm", ".far", ".med", ".okt", ".ult", ".amf", ".gdm", ".psm", ".dmf"}
IMG_EXT = {".gif", ".pcx", ".bmp", ".jpg", ".jpeg", ".png", ".tga", ".lbm", ".iff", ".tif", ".tiff", ".pic", ".ico"}
ANSI_EXT = {".ans", ".asc", ".ice", ".diz", ".nfo", ".txt", ".doc", ".1st", ".me", ".info", ".bin"}
MOD_TAGS = (b"M.K.", b"M!K!", b"FLT4", b"FLT8", b"4CHN", b"6CHN", b"8CHN", b"CD81", b"OCTA", b"OKTA")


def file_type(path, size):
    """(genre, détail) d'un fichier : exe, com, module, image, texte, autre."""
    ext = os.path.splitext(path)[1].lower()
    try:
        with open(path, "rb") as f:
            head = f.read(4096)
            f.seek(1080)
            mod_tag = f.read(4)
    except OSError:
        return "autre", ""
    if head[:2] in (b"MZ", b"ZM") and size >= 0x1C:
        lfa = int.from_bytes(head[0x3C:0x40], "little") if len(head) >= 0x40 else 0
        detail = "dos"
        if 0x40 <= lfa <= size - 4:
            try:
                with open(path, "rb") as f:
                    f.seek(lfa)
                    sig = f.read(4)
            except OSError:
                sig = b""
            if sig[:4] == b"PE\0\0":
                detail = "win32"
            elif sig[:2] in (b"LE", b"LX"):
                detail = "dos-étendu"
            elif sig[:2] == b"NE":
                detail = "win16"
        return "exe", detail
    if head.startswith(b"Extended Module:") or head[44:48] == b"SCRM" or head.startswith(b"IMPM") or mod_tag in MOD_TAGS \
            or head[:3] == b"MTM" or head[:2] in (b"if", b"JN") or ext in MOD_EXT:
        return "module", ext.lstrip(".")
    if head[:4] in (b"GIF8", b"\x89PNG") or head[:2] == b"BM" or head[:3] == b"\xff\xd8\xff" \
            or (head[:1] == b"\x0a" and ext == ".pcx") or ext in IMG_EXT:
        return "image", ext.lstrip(".")
    if ext == ".com" and size <= 65280:
        return "com", ""
    if ext in ANSI_EXT:
        return "texte", ext.lstrip(".")
    return "autre", ""


def scan_item(src, rel):
    """Fichiers d'un élément : liste de (nom, taille, sha1, genre, détail)."""
    item = os.path.join(src, rel)
    tmp = tempfile.mkdtemp(prefix="classify-")
    try:
        if os.path.isdir(item):
            root = item
        else:
            if not survey.extract(item, tmp):
                return None
            root = tmp
        out = []
        for d, _, files in os.walk(root):
            for f in files:
                p = os.path.join(d, f)
                try:
                    size = os.path.getsize(p)
                    with open(p, "rb") as fh:
                        sha = hashlib.sha1(fh.read()).hexdigest()
                except OSError:
                    continue
                genre, detail = file_type(p, size)
                out.append((f.lower(), size, sha, genre, detail))
        return out
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", required=True)
    ap.add_argument("--items", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    rels = [l.strip() for l in open(args.items, encoding="utf-8") if l.strip()]
    scans = {}
    for i, rel in enumerate(rels, 1):
        scans[rel] = scan_item(args.src, rel)
        if i % 50 == 0:
            print(f"{i}/{len(rels)}", flush=True)
    seen = collections.defaultdict(set)
    for rel, files in scans.items():
        for name, size, sha, genre, _ in files or []:
            if genre in ("exe", "com"):
                seen[sha].add(rel)
    # Un même exécutable sur plusieurs CD (doublon d'une démo) ne compte pas : il faut des éléments de noms différents.
    stem = lambda rel: os.path.splitext(os.path.basename(rel))[0].lower()
    boiler = {sha for sha, items in seen.items() if len({stem(r) for r in items}) >= BOILER}
    result = {}
    for rel, files in scans.items():
        if files is None:
            result[rel] = {"categorie": "archive illisible"}
            continue
        own = [(n, d if g == "exe" else "com") for n, s, sha, g, d in files if g in ("exe", "com") and sha not in boiler
               and not survey.SKIP_NAMES.search(os.path.splitext(n)[0])]
        helpers = [n for n, s, sha, g, d in files if g in ("exe", "com") and sha in boiler]
        kinds = collections.Counter(g for _, _, _, g, _ in files)
        if any(c.lower() in ("mags", "mag", "magazines", "docs") for c in rel.split("/")[1:-1]):
            cat = "magazine"  # magazine disque : lecteur de texte, pas une démo
        elif own:
            cat = "exécutable"
        elif kinds["module"]:
            cat = "musique"
        elif kinds["image"]:
            cat = "image"
        elif kinds["texte"] and not kinds["exe"] and not kinds["com"]:
            cat = "ansi"
        elif helpers:
            cat = "pub seule"
        else:
            cat = "données"
        result[rel] = {"categorie": cat, "executables": [f"{n} ({d})" for n, d in own][:6], "accompagnement": helpers[:4],
                       "fichiers": dict(kinds)}
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1, ensure_ascii=False)
    print(collections.Counter(v["categorie"] for v in result.values()))


if __name__ == "__main__":
    main()
