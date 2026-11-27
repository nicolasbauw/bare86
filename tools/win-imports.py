#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Imports que DPMILD32 ne trouve pas, pour une liste de démos Windows : lance chaque démo quelques secondes et relève TOUTES les lignes
« DPMILD32: import not found: X » avec la DLL réclamée (« DPMILD32: file DLL »). Sortie : TSV DLL, fonction, nombre de démos concernées.
Usage : tools/win-imports.py LISTE.txt [--out docs/win-missing-imports.tsv] [--jobs 4] [--secs 8]"""
import argparse
import collections
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


def one(item, a):
    tmp = tempfile.mkdtemp(prefix="winimp-")
    try:
        work = os.path.join(tmp, "w")
        os.makedirs(work)
        path = os.path.join(a.src, item)
        shutil.copytree(path, work, dirs_exist_ok=True) if os.path.isdir(path) else survey.extract(path, work)
        exe, _, _ = survey.pick(work, os.path.basename(item))
        if exe is None:
            return item, []
        have = {f.lower() for f in os.listdir(os.path.dirname(exe))}
        for f in os.listdir(a.dll_pack):
            if f.lower() not in have:
                shutil.copy(os.path.join(a.dll_pack, f), os.path.join(os.path.dirname(exe), f))
        p = subprocess.run([BARE, "--no-realtime", "--limit", str(a.secs), "--keys", "{wait:900000}", "--screen", os.path.basename(exe)],
                           cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, text=True, errors="replace", timeout=a.secs + 60)
        text = p.stdout + p.stderr
        log = os.path.join(os.path.dirname(exe), "hxstderr.log")
        if os.path.exists(log):
            text += "\n" + open(log, errors="replace").read()
        out, pending = [], []
        for l in text.splitlines():
            m = re.search(r"import not found: (\S+)", l)
            if m:
                pending.append(m.group(1))
                continue
            m = re.search(r"DPMILD32: file (\S+)", l)
            if m and pending:
                out += [(m.group(1).lower(), fn) for fn in pending]
                pending = []
        return item, out
    except Exception:  # noqa: BLE001
        return item, []
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("items")
    ap.add_argument("--out", default="docs/win-missing-imports.tsv")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--secs", type=int, default=8)
    ap.add_argument("--src", default=os.path.join(PC, "dos-demos"))
    ap.add_argument("--dll-pack", default=os.path.join(PC, "runtime-pack"))
    a = ap.parse_args()
    items = [l.strip() for l in open(a.items) if l.strip()]
    cnt, who = collections.Counter(), collections.defaultdict(set)
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as ex:
        for item, miss in ex.map(lambda i: one(i, a), items):
            for k in set(miss):
                cnt[k] += 1
                who[k].add(item)
    with open(a.out, "w") as f:
        f.write("dll\tfonction\tdemos\n")
        for (dll, fn), n in sorted(cnt.items(), key=lambda kv: (-kv[1], kv[0])):
            f.write(f"{dll}\t{fn}\t{n}\n")
    print(len(cnt), "imports manquants distincts ;", len({i for s in who.values() for i in s}), "démos concernées")


if __name__ == "__main__":
    main()
