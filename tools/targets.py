#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Liste des éléments cibles d'une collection : démos et intros 64k (dossiers de
catégorie `demo`, `demos`, `in64`, `64k`, `64ko`, `intro64`, `accel`, `no-accel`),
sans les intros 4k/1k/512 octets, graphismes, musiques, jeux, disk mags.

  python3 -I tools/targets.py --src COLLECTION > targets.txt

Sort un chemin relatif d'élément par ligne (même forme que `survey.py --items` :
une archive, ou un dossier contenant directement un exécutable).
"""
import argparse
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
CATEGORY = re.compile(r"^(demo|demos|in64|64k|64ko|intro64|accel|no-accel)$", re.I)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", required=True)
    args = ap.parse_args()
    import survey  # noqa: E402  (même dossier)
    src = os.path.abspath(args.src)
    out, rest = [], []
    for item in survey.collect(src):
        rel = os.path.relpath(item, src)
        parts = rel.split("/")
        # La catégorie est un des dossiers qui précèdent l'élément, ou le nom de
        # l'élément lui-même (`am98/IN64.zip`).
        stem = os.path.splitext(parts[-1])[0]
        if any(CATEGORY.match(p) for p in parts[1:-1]) or CATEGORY.match(stem):
            out.append(rel)
        else:
            rest.append(rel)
    print("\n".join(sorted(out)))
    have = {r.split("/")[0] for r in out}
    # CD sans aucun élément classé (démos rangées par événement, sans catégorie) :
    # à trier à la main, listés sur la sortie d'erreur.
    for cd in sorted({r.split("/")[0] for r in rest} - have):
        print(f"{cd}: aucun élément catégorisé ({sum(1 for r in rest if r.startswith(cd + '/'))} éléments non classés)", file=sys.stderr)
    print(len(out), "éléments", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
