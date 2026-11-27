#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Diagnostic rapide des démos Windows sous HX : chaque élément est extrait, lancé ~15 s par Bare86 (--screen, --trace-int réduit à ses
messages), et les lignes utiles (DLL/import introuvable, exceptions, messages du programme, INT non gérées, fonctions de HX en erreur)
sont rangées par FAMILLE. Sortie : TSV `id, exécutable, famille, lignes`.

Usage : tools/win-diag.py LISTE_D_ÉLÉMENTS.txt [--out docs/win-diag.tsv] [--jobs 4] [--secs 15]"""
import argparse
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import survey  # noqa: E402

PC = "/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
BARE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "bare86")

FAMILIES = [
    ("dll-manquante", r"(can.?t|cannot|unable to) (find|load|open) .*\.dll|\.dll.*(not found|missing)|Unable to load ordinal|dll .* not found|Missing import"),
    ("import-non-resolu", r"import.*not (found|resolved)|unresolved|unknown function|function .* not found"),
    ("directdraw", r"ddraw|directdraw|ddram|IDirectDraw"),
    ("direct3d", r"d3d|direct3d|3d device|d3drm|d3dx"),
    ("directsound", r"dsound|directsound"),
    ("opengl", r"opengl|opengl32|glu32|wgl"),
    ("boite-oui-non", r"fullscreen|full screen|windowed|yes\s*no|\(y/n\)|run in"),
    ("exception", r"exception|abnormal program|assert|runtime error|access violation|page fault|stack overflow"),
    ("mode-video", r"video mode|bits per pixel|color depth|bpp|resolution|display mode"),
    ("controles-standard", r"createwindow|class not registered|window class|could not set window hook|RegisterClass"),
    ("int-non-geree", r"not handled"),
]


def classify(text):
    low = text.lower()
    fams = [n for n, p in FAMILIES if re.search(p, low, re.I)]
    return fams or ["rien-de-lisible"]


def one(item, args):
    tmp = tempfile.mkdtemp(prefix="windiag-")
    try:
        work = os.path.join(tmp, "w")
        os.makedirs(work)
        path = os.path.join(args.src, item)
        if os.path.isdir(path):
            shutil.copytree(path, work, dirs_exist_ok=True)
        else:
            survey.extract(path, work)
        exe, k, others = survey.pick(work, os.path.basename(item))
        if exe is None:
            return item, "", ["pas-d-exe"], ""
        have = {f.lower() for f in os.listdir(os.path.dirname(exe))}
        if os.path.isdir(args.dll_pack):
            for f in os.listdir(args.dll_pack):
                if f.lower() not in have:
                    shutil.copy(os.path.join(args.dll_pack, f), os.path.join(os.path.dirname(exe), f))
        cmd = [BARE, "--no-realtime", "--limit", str(args.secs), "--keys", "{wait:900000}", "--screen", os.path.basename(exe)]
        p = subprocess.run(cmd, cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, text=True, errors="replace", timeout=args.secs + 60)
        text = p.stdout + p.stderr
        log = os.path.join(os.path.dirname(exe), "hxstderr.log")
        if os.path.exists(log):
            text += "\n" + open(log, errors="replace").read()
        lines = [l.strip() for l in text.splitlines() if l.strip() and not l.startswith(("EAX", "ES=", "F000", "Stats", "DPMI loader", "Copyright", "HDPMI32"))]
        return item, os.path.relpath(exe, work), classify(text), " | ".join(lines[:6])[:300]
    except Exception as e:  # noqa: BLE001
        return item, "", ["erreur-outil"], str(e)[:200]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("items")
    ap.add_argument("--out", default="docs/win-diag.tsv")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--secs", type=int, default=15)
    ap.add_argument("--src", default=os.path.join(PC, "dos-demos"))
    ap.add_argument("--dll-pack", default=os.path.join(PC, "runtime-pack"))
    args = ap.parse_args()
    items = [l.strip() for l in open(args.items) if l.strip()]
    rows = []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as ex:
        for r in ex.map(lambda i: one(i, args), items):
            rows.append(r)
            print(r[0][7:50], r[2], flush=True)
    with open(args.out, "w") as f:
        f.write("item\texe\tfamilles\tlignes\n")
        for item, exe, fams, lines in rows:
            f.write(f"{item}\t{exe}\t{','.join(fams)}\t{lines}\n")


if __name__ == "__main__":
    main()
