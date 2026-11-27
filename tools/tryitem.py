#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Essai à la main d'un élément du corpus : extrait (archives imbriquées comprises), lance Bare86 avec des touches, garde les captures
et une planche. Sert à trouver les touches d'un écran de setup avant de les écrire dans le profil.

Usage : tools/tryitem.py ÉLÉMENT [--exe CHEMIN] [--keys '{wait:3000}{enter}{wait:900000}'] [--secs 20] [--every 2] [--opts "--dpmi"]
        [--src DOSSIER] [--out DOSSIER] [--list]      (--list : montre les exécutables trouvés et s'arrête)"""
import argparse
import os
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import survey  # noqa: E402

PC = "/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("item")
    ap.add_argument("--exe")
    ap.add_argument("--keys", default="{wait:900000}")
    ap.add_argument("--secs", type=int, default=20)
    ap.add_argument("--every", type=float, default=2)
    ap.add_argument("--opts", default="")
    ap.add_argument("--src", default=os.path.join(PC, "dos-demos"))
    ap.add_argument("--out", default=None)
    ap.add_argument("--bare86", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "bare86"))
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--dll-pack", default=None, help="dossier de DLL de Windows copiées à côté de l exécutable (démos Win32)")
    a = ap.parse_args()
    a.out = a.out or "/tmp/tryitem-" + os.path.basename(a.item.rstrip("/"))
    work = os.path.join(a.out, "work")
    shutil.rmtree(a.out, ignore_errors=True)
    os.makedirs(work)
    path = os.path.join(a.src, a.item)
    if os.path.isdir(path):
        shutil.copytree(path, work, dirs_exist_ok=True)
    else:
        survey.extract(path, work)
    if a.exe:
        exe = os.path.join(work, a.exe)
    else:
        exe, k, others = survey.pick(work, os.path.basename(a.item))
        print("exécutable :", os.path.relpath(exe, work), "| autres :", others[:8])
    if a.list:
        for d, _, fs in os.walk(work):
            for f in sorted(fs):
                if f.lower().endswith((".exe", ".com", ".bat")):
                    print(" ", os.path.relpath(os.path.join(d, f), work))
        return
    if a.dll_pack and os.path.isdir(a.dll_pack):
        have = {f.lower() for f in os.listdir(os.path.dirname(exe))}
        for f in os.listdir(a.dll_pack):
            if f.lower() not in have:
                shutil.copy(os.path.join(a.dll_pack, f), os.path.join(os.path.dirname(exe), f))
    shots = os.path.join(a.out, "shots")
    os.makedirs(shots)
    cmd = [a.bare86, "--no-realtime", "--limit", str(a.secs), "--shot", os.path.join(shots, "f.png"), "--shot-every", str(a.every),
           "--keys", a.keys, "--screen"] + a.opts.split() + [os.path.basename(exe)]
    p = subprocess.run(cmd, cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, text=True, errors="replace")
    text = (p.stdout + p.stderr)
    print("\n".join(l for l in text.splitlines() if l.strip() and not l.startswith(("EAX", "ES=", "F000", "Stats")))[:1500])
    pics = sorted(f for f in os.listdir(shots) if f.endswith(".png") and f != "f.png")
    print(len(pics), "captures,", len({subprocess.run(["md5sum", os.path.join(shots, f)], capture_output=True, text=True).stdout.split()[0] for f in pics}), "distinctes")
    if pics:
        pick = [pics[0], pics[len(pics) // 3], pics[2 * len(pics) // 3], pics[-1]] if len(pics) >= 4 else pics
        sheet = os.path.join(a.out, "sheet.png")
        subprocess.run(["montage"] + [os.path.join(shots, f) for f in pick] + ["-tile", "4x1", "-geometry", "320x200+3+3", sheet])
        print("planche :", sheet)


if __name__ == "__main__":
    main()
