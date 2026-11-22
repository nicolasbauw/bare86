#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Contrôle de fiabilité d'un balayage (results.jsonl de tools/survey.py).

Une démo n'est « fiable » (échantillon utilisable par Gemma) que si on a vu plus
que des écrans d'erreur ou de configuration. Chaque élément reçoit des drapeaux :

  FEW     3 scènes gardées ou moins
  ERR     message d'erreur de Bare86 ou texte d'erreur à l'écran final
  EARLY   rendu au DOS en moins de 20 s
  CPU     pic de plus de 0,9 cœur sur 0,5 s (champs cpu*, attente active de l'invité ?)
  STATIC  écran figé (attend une touche)
  TEXT    fini sur un écran de texte (message, menu, retour au DOS)

  python3 -I tools/audit.py DOSSIER_RÉSULTATS [--bad] [--ids]

`--bad` n'affiche que les éléments douteux, `--ids` seulement leurs chemins
(un par ligne, utilisable par `survey.py --items`).
"""
import argparse
import json
import os
import re

ERR = re.compile(r"(error|no data|unable|not found|failed|failure|cannot|can't|couldn't|missing|usage|invalid|"
                 r"bad |not enough|insufficient|out of memory|please|not supported|no such|unsupported|abort)", re.I)


def flags(r):
    f = []
    n = r.get("kept", len(r.get("shots", [])))
    if r.get("status") in ("no-exe", "win32", "bad-archive"):
        return [r["status"].upper()]
    if n <= 3:
        f.append("FEW")
    if r.get("message") or r.get("status") in ("error", "timeout"):
        f.append("ERR")
    text = r.get("screen", "")
    if text:
        f.append("TEXT")
        if ERR.search(text):
            f.append("ERR")
    if r.get("status") == "exit" and r.get("seconds", 99) < 20:
        f.append("EARLY")
    if r.get("cpu_peak", r.get("cpu", 0)) > 0.9:
        f.append("CPU")
    if r.get("static"):
        f.append("STATIC")
    return sorted(set(f))


def load(path):
    last = {}
    for l in open(os.path.join(path, "results.jsonl")):
        r = json.loads(l)
        last[r["id"]] = r
    return list(last.values())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out")
    ap.add_argument("--bad", action="store_true")
    ap.add_argument("--ids", action="store_true")
    a = ap.parse_args()
    rows = load(a.out)
    bad = 0
    for r in rows:
        f = flags(r)
        if f:
            bad += 1
        if a.bad and not f:
            continue
        if a.ids:
            print(r["item"])
            continue
        print(f"{r['id'][:44]:44} {r.get('status',''):6} {r.get('kept', len(r.get('shots', []))):4} sc {r.get('seconds', 0):6.0f}s "
              f"{','.join(f):18} {(r.get('message') or r.get('screen', ''))[:80]}")
    if not a.ids:
        print(f"\n{len(rows)} éléments, {bad} douteux")


if __name__ == "__main__":
    main()
