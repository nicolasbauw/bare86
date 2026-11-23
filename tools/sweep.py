#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Balayage d'une collection CD par CD (numéro de magazine = mois) : pour chaque
CD, `tools/survey.py --full` (captures des scènes), puis `tools/survey_ask.py`
(question à un modèle de vision local sur chaque planche), puis un bilan
(éléments où le modèle répond « PIG: yes » ou « DONTSMOKE: yes »). La
progression est tenue dans `OUT/progress.json` : un CD terminé est sauté à la
reprise, un CD interrompu reprend où il s'était arrêté (survey et survey_ask
sautent déjà les éléments faits).

  python3 -I tools/sweep.py --src COLLECTION --out RÉSULTATS --items LISTE \\
      [--cds 27-40] [--one] [--jobs 10]

`--items` : fichier, un chemin relatif d'élément par ligne (premier composant :
`pcteam_cdNN`). `--one` : s'arrête après un CD (revue entre deux CD). `--status`
affiche la progression sans rien lancer.
"""
import argparse
import datetime
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
QUESTION = ('These are screenshots of distinct scenes of a 1990s PC demo. Answer on the first line exactly '
            '"PIG: yes" or "PIG: no" (is there a pig, pigs, or a pig-like character?), on the second line exactly '
            '"DONTSMOKE: yes" or "DONTSMOKE: no" (is the text "don\'t smoke" visible?), then describe the images '
            'in one sentence.')


def cd_number(rel):
    m = re.match(r"pcteam_cd(\d+)/", rel)
    return int(m.group(1)) if m else None


def load(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return {}


def hits(out, ids, word):
    """Éléments de `ids` dont une réponse du modèle contient `word`."""
    found = []
    try:
        for line in open(os.path.join(out, "asks.jsonl")):
            a = json.loads(line)
            if a["id"] in ids and word in a["answer"]:
                found.append(a["id"])
    except OSError:
        pass
    return sorted(set(found))


def ident(rel):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", rel).strip("_")[:120]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--items", required=True)
    ap.add_argument("--cds", help="intervalle de numéros de CD, ex. 27-40 (défaut : tous ceux de la liste)")
    ap.add_argument("--one", action="store_true", help="un seul CD (le prochain non terminé)")
    ap.add_argument("--status", action="store_true")
    ap.add_argument("--jobs", type=int, default=10)
    ap.add_argument("--limit", type=int, default=600)
    ap.add_argument("--profiles", help="profils de touches/options par élément (profiles.json de survey.py)")
    ap.add_argument("--model", default="Gemma-4-12B-it-GGUF")
    args = ap.parse_args()

    items = [line.strip() for line in open(args.items) if line.strip()]
    by_cd = {}
    for rel in items:
        n = cd_number(rel)
        if n is not None:
            by_cd.setdefault(n, []).append(rel)
    lo, hi = (int(x) for x in args.cds.split("-")) if args.cds else (min(by_cd), max(by_cd))
    todo = [n for n in sorted(by_cd) if lo <= n <= hi]
    os.makedirs(args.out, exist_ok=True)
    pfile = os.path.join(args.out, "progress.json")
    progress = load(pfile)

    if args.status:
        for n in sorted(by_cd):
            p = progress.get(str(n), {})
            print(f"CD {n:2}: {p.get('state', 'à faire'):8} {len(by_cd[n]):3} éléments"
                  f"  PIG: {len(p.get('pig', []))}  DONTSMOKE: {len(p.get('dontsmoke', []))}  {p.get('finished', '')}")
        return 0

    for n in todo:
        if progress.get(str(n), {}).get("state") == "terminé":
            continue
        rels = by_cd[n]
        print(f"=== CD {n} : {len(rels)} éléments", flush=True)
        progress[str(n)] = {"state": "en cours", "items": len(rels), "started": datetime.datetime.now().isoformat(timespec="seconds")}
        json.dump(progress, open(pfile, "w"), indent=1, ensure_ascii=False)
        listing = os.path.join(args.out, f"items-cd{n}.txt")
        open(listing, "w").write("\n".join(rels) + "\n")
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "survey.py"), "--src", args.src, "--out", args.out,
                        "--items", listing, "--jobs", str(args.jobs), "--limit", str(args.limit), "--every", "2",
                        "--full", "--idle", "60"] + (["--profiles", args.profiles] if args.profiles else []), check=False)
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "survey_ask.py"), "--out", args.out, "--model", args.model,
                        "--question", QUESTION], check=False)
        ids = {ident(r) for r in rels}
        pig, smoke = hits(args.out, ids, "PIG: yes"), hits(args.out, ids, "DONTSMOKE: yes")
        progress[str(n)].update({"state": "terminé", "pig": pig, "dontsmoke": smoke,
                                 "finished": datetime.datetime.now().isoformat(timespec="seconds")})
        json.dump(progress, open(pfile, "w"), indent=1, ensure_ascii=False)
        print(f"=== CD {n} terminé : PIG yes {len(pig)}, DONTSMOKE yes {len(smoke)}", flush=True)
        for i in pig + [x for x in smoke if x not in pig]:
            print("   à regarder :", i, flush=True)
        if args.one:
            break
    return 0


if __name__ == "__main__":
    sys.exit(main())
