#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Éléments dont les captures sont les MÊMES que celles d'un autre élément d'un balayage (publicités de distributeur,
exécutable secondaire qui n'est pas la démo : `#pitrlz#.exe`, `izxcppkm.com`…).

Pour chaque élément, part de ses captures (hors écrans unis : écart-type de gris < 8) dont l'empreinte MD5 existe aussi
dans un autre élément ; un élément est « doublon » si cette part dépasse --share (défaut 0,5). Les groupes de doublons sont
écrits avec l'exécutable de chacun, pour décider lequel est la vraie démo (ou une publicité à ajouter à AD_NAMES).

Usage : tools/dup-shots.py DOSSIER_DE_BALAYAGE [--share 0.5]   (lit DOSSIER/results.jsonl et DOSSIER/shots/*/)"""
import argparse
import collections
import hashlib
import json
import os
import subprocess


def flat(path):
    """True si l'image est (presque) unie."""
    out = subprocess.run(["identify", "-format", "%[fx:standard_deviation*255]", path], capture_output=True, text=True).stdout
    try:
        return float(out) < 8
    except ValueError:
        return True


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scan")
    ap.add_argument("--share", type=float, default=0.5)
    args = ap.parse_args()
    exe = {}
    for l in open(os.path.join(args.scan, "results.jsonl")):
        x = json.loads(l)
        exe[x["id"]] = x.get("exe")
    shots = os.path.join(args.scan, "shots")
    owners = collections.defaultdict(set)
    mine = {}
    for item in sorted(os.listdir(shots)):
        hs = []
        for f in sorted(os.listdir(os.path.join(shots, item))):
            if f.endswith(".png"):
                p = os.path.join(shots, item, f)
                hs.append((hashlib.md5(open(p, "rb").read()).hexdigest(), p))
        mine[item] = hs
        for h, _ in hs:
            owners[h].add(item)
    flat_cache = {}
    groups = collections.defaultdict(set)
    for item, hs in mine.items():
        shared = [(h, p) for h, p in hs if len(owners[h]) > 1]
        shared = [(h, p) for h, p in shared if not flat_cache.setdefault(h, flat(p))]
        solid = [(h, p) for h, p in hs if not flat_cache.setdefault(h, flat(p))]
        if solid and len(shared) / len(solid) >= args.share:
            for h, _ in shared:
                groups[frozenset(owners[h])].add(item)
    seen = set()
    n = 0
    for items in groups.values():
        key = frozenset(items)
        if key in seen:
            continue
        seen.add(key)
        n += 1
        print(f"groupe {n} ({len(items)} éléments) :")
        for it in sorted(items):
            print(f"  {it}  [{exe.get(it)}]")
    print(f"{len(seen)} groupes")


if __name__ == "__main__":
    main()
