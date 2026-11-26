#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Profil détaillé (`--stats` de Bare86) des programmes chauds d'un balayage, pour savoir POURQUOI ils occupent un cœur.

survey.py mesure le CPU mais ne garde pas les compteurs de Bare86 (il arrête le programme par un signal, et Bare86 n'écrit ses
compteurs qu'à sa fin normale). Ce script relance chaque programme chaud 20 s, jusqu'à la limite, avec `--stats`, mesure le CPU
de l'hôte (getrusage) et range un résumé dans `cpu-profiles.jsonl` à côté des résultats ; `tools/cpu-report.py` l'ajoute à sa liste.

Lecture des compteurs (par seconde) : `exits` sorties de KVM ; `host_calls` services de DOS/BIOS traités par l'hôte (une boucle
d'attente par INT 21h/2Fh/16h en fait des dizaines de milliers) ; `irqs` ; `vga_yields`, `pit_yields`, `spin_sleeps` : fois où Bare86 a
endormi le vCPU (à 0 pour un programme chaud = rien ne l'a reconnu comme une attente) ; `interpreted` : instructions interprétées
(chemin lent) ; `jit_*` : traducteur ; ports les plus lus (`3DAh` = attente de retour de balayage, `060h` clavier, `040h` minuterie…).
`--trace-int` (ou `BARE86_PROFILE_IP`) permet d'aller plus loin sur un programme précis.

Usage : tools/cpu-profile.py --src COLLECTION RÉSULTATS.jsonl [--top 15] [--hot 0.5] [--seconds 20] [--redo] [--bare86 BIN] [--win32 --dll-pack DIR]"""
import argparse
import json
import os
import re
import resource
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import importlib.util

_spec = importlib.util.spec_from_file_location("survey", os.path.join(os.path.dirname(os.path.abspath(__file__)), "survey.py"))
survey = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(survey)


def parse_stats(out):
    m = re.search(r"Stats \{(.*?)\n\}", out, re.S)
    if not m:
        return None
    body = m.group(1)
    st = {k: int(v) for k, v in re.findall(r"\b(\w+): (\d+)", body)}
    st["ports"] = re.findall(r'"(\w+h): (\d+)"', body)[:4]
    return st


def summary(st, seconds):
    if not st:
        return "pas de compteurs (le programme s'est arrêté avant la fin ?)"
    per = lambda k: st.get(k, 0) / seconds
    parts = [f"exits {per('exits'):.0f}/s", f"hôte {per('host_calls'):.0f}/s", f"irq {per('irqs'):.0f}/s"]
    sleeps = st.get("vga_yields", 0) + st.get("pit_yields", 0) + st.get("spin_sleeps", 0)
    parts.append(f"endormi {sleeps}")
    if st.get("interpreted"):
        parts.append(f"interprété {st['interpreted']}")
    if st.get("jit_bursts"):
        parts.append(f"jit {st['jit_bursts']}")
    if st.get("ports"):
        parts.append("ports " + " ".join(f"{p}={n}" for p, n in st["ports"][:3]))
    return ", ".join(parts)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("results")
    ap.add_argument("--src", required=True, help="collection (comme --src de survey.py)")
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument("--hot", type=float, default=0.5)
    ap.add_argument("--seconds", type=int, default=20)
    ap.add_argument("--redo", action="store_true")
    ap.add_argument("--bare86", default=os.path.expanduser("~/Dev/bare86/target/release/bare86"))
    ap.add_argument("--mips", type=int, default=200)
    ap.add_argument("--win32", action="store_true")
    ap.add_argument("--dll-pack")
    args = ap.parse_args()
    rows = [json.loads(l) for l in open(args.results)]
    out_path = os.path.join(os.path.dirname(os.path.abspath(args.results)), "cpu-profiles.jsonl")
    done = {}
    if os.path.exists(out_path):
        for l in open(out_path):
            p = json.loads(l)
            done[p["id"]] = p
    hot = [x for x in rows if x.get("cpu_hot", 0) >= args.hot and x.get("exe")]
    hot.sort(key=lambda x: (-x.get("cpu_hot", 0), -x.get("cpu", 0)))
    for x in hot[:args.top]:
        if x["id"] in done and not args.redo:
            continue
        item = os.path.join(os.path.abspath(args.src), x["item"])
        work = tempfile.mkdtemp(prefix="cpuprof-")
        try:
            if os.path.isdir(item):
                shutil.copytree(item, os.path.join(work, "w"))
            else:
                os.makedirs(os.path.join(work, "w"))
                if not survey.extract(item, os.path.join(work, "w")):
                    continue
            run_dir = os.path.join(work, "w")
            exe = os.path.join(run_dir, x["exe"])
            if not os.path.exists(exe):
                # casse du nom : chercher sans tenir compte de la casse
                low = x["exe"].replace("\\", "/").lower()
                cand = [os.path.join(r, f) for r, _, fs in os.walk(run_dir) for f in fs if os.path.relpath(os.path.join(r, f), run_dir).replace("\\", "/").lower() == low]
                if not cand:
                    continue
                exe = cand[0]
            if args.dll_pack:
                have = {f.lower() for f in os.listdir(os.path.dirname(exe))}
                for f in os.listdir(args.dll_pack):
                    if f.lower() not in have:
                        shutil.copy(os.path.join(args.dll_pack, f), os.path.join(os.path.dirname(exe), f))
            cmd = [args.bare86, "--no-realtime", "--limit", str(args.seconds), "--keys", x.get("keys") or "{wait:900000}", "--stats"]
            if args.mips:
                cmd += ["--mips", str(args.mips)]
            cmd.append(os.path.basename(exe))
            r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
            t0 = time.time()
            # l'entrée standard reste ouverte : fermée, la fin de fichier validerait une boîte de message
            proc = subprocess.Popen(cmd, cwd=os.path.dirname(exe), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            try:
                out = proc.communicate(timeout=args.seconds + 60)[0].decode(errors="replace")
            except subprocess.TimeoutExpired:
                proc.kill()
                out = proc.communicate()[0].decode(errors="replace")
            wall = time.time() - t0
            r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
            cpu = (r1.ru_utime + r1.ru_stime - r0.ru_utime - r0.ru_stime) / wall
            st = parse_stats(out)
            rec = {"id": x["id"], "exe": x["exe"], "seconds": round(wall, 1), "cpu": round(cpu, 2), "stats": st,
                   "summary": f"{cpu:.2f} c. : " + summary(st, wall)}
            with open(out_path, "a") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            print(f"{x['id'][:50]:50} {rec['summary']}", flush=True)
        finally:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
