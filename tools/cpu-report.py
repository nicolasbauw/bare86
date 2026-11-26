#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Liste des programmes qui poussent le CPU très haut, tirée des `results.jsonl` de survey.py.

survey.py échantillonne le CPU de Bare86 toutes les 0,5 s : `cpu` (moyenne, en cœurs : 1,0 = 100 % d'un cœur), `cpu_peak`
(maximum d'un échantillon), `cpu_hot` (part des échantillons au-dessus de 0,9 cœur), `cpu_peak_at` (seconde du pic).
Ce script les rassemble, sépare deux familles et écrit un fichier Markdown à garder (`docs/cpu-hotlist.md`) :

  - « attente ? » : CPU haut ET rien ne change à l'écran (écran figé, arrêt pour inactivité, 0 à 2 scènes) : probablement une
    boucle d'attente (vsync, clavier, minuterie) que Bare86 pourrait endormir ; c'est la piste d'optimisation la plus rentable ;
  - « calcul » : CPU haut et l'écran bouge : du vrai travail (interprétation, rendu logiciel) ; relève du traducteur (JIT), du
    bridage (`--mips`) ou de la machine.

Usage : tools/cpu-report.py [--out docs/cpu-hotlist.md] [--hot 0.5] [--top 40] RÉSULTATS.jsonl [...]   (plusieurs balayages possibles)
Les profils détaillés (compteurs `--stats`) viennent de `tools/cpu-profile.py` : s'ils existent à côté (`cpu-profiles.jsonl`),
ils sont ajoutés au tableau."""
import argparse
import json
import os
import sys


def kind(x):
    quiet = x.get("static") or x.get("idle_killed") or (x.get("kept") or 0) <= 2
    return "attente ?" if quiet else "calcul"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("results", nargs="+")
    ap.add_argument("--out", default="docs/cpu-hotlist.md")
    ap.add_argument("--hot", type=float, default=0.5, help="seuil de cpu_hot (défaut 0,5 : plus de la moitié du temps au-dessus de 0,9 cœur)")
    ap.add_argument("--top", type=int, default=40)
    args = ap.parse_args()
    lines = ["# Programmes qui poussent le CPU très haut", "",
             "Généré par `tools/cpu-report.py` (survey.py mesure le CPU de Bare86 toutes les 0,5 s ; 1,0 = 100 % d'un cœur). "
             f"« chaud » = `cpu_hot` ≥ {args.hot} (plus de {int(args.hot * 100)} % du temps au-dessus de 0,9 cœur).", ""]
    for path in args.results:
        rows = [json.loads(l) for l in open(path)]
        name = os.path.basename(os.path.dirname(os.path.abspath(path)))
        prof = {}
        pp = os.path.join(os.path.dirname(os.path.abspath(path)), "cpu-profiles.jsonl")
        if os.path.exists(pp):
            for l in open(pp):
                p = json.loads(l)
                prof[p["id"]] = p
        meas = [x for x in rows if "cpu" in x]
        hot = [x for x in meas if x.get("cpu_hot", 0) >= args.hot]
        att = [x for x in hot if kind(x) == "attente ?"]
        lines += [f"## {name}", "",
                  f"{len(rows)} éléments, {len(meas)} mesurés, **{len(hot)} chauds** dont **{len(att)} « attente ? »** (écran figé : boucle d'attente probable) "
                  f"et {len(hot) - len(att)} « calcul ». CPU moyen de l'ensemble : {sum(x['cpu'] for x in meas) / max(1, len(meas)):.2f} cœur.", ""]
        if not hot:
            lines += ["(aucun)", ""]
            continue
        hot.sort(key=lambda x: (-x.get("cpu_hot", 0), -x.get("cpu", 0)))
        lines += ["| élément | exécutable | famille | cpu | pic (à t) | chaud | scènes | durée | profil `--stats` |", "|---|---|---|--:|--:|--:|--:|--:|---|"]
        for x in hot[:args.top]:
            p = prof.get(x["id"])
            ptxt = p["summary"] if p else ""
            lines.append("| `%s` | %s | %s | %.2f | %.2f (%ss) | %.2f | %s | %ss | %s |" % (
                x["id"].replace("pcteam_", "")[:48], x.get("exe", ""), kind(x), x["cpu"], x.get("cpu_peak", 0), x.get("cpu_peak_at", ""),
                x.get("cpu_hot", 0), x.get("kept", ""), round(x.get("seconds") or 0), ptxt))
        if len(hot) > args.top:
            lines.append(f"\n… et {len(hot) - args.top} autres (`--top` pour en voir plus).")
        lines.append("")
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    open(args.out, "w").write("\n".join(lines) + "\n")
    print("\n".join(l for l in lines if l.startswith("##") or l.startswith("N") or "chauds" in l))


if __name__ == "__main__":
    sys.exit(main())
