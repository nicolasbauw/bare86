#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tri des résultats d'un balayage d'exécutables (`scan-dos2`…) : pour chaque ligne, une CLASSE et l'ACTION à mener.

Classes : magazine (diskmag : exclu du périmètre, verdict manuel), remplace (relancé plus tard), court (programme qui rend la main en moins de 12 s : trop peu de captures à 2 s d'intervalle), pub (exécutable publicitaire connu), doublon (mêmes captures qu'un autre élément : `dup-shots.py`), outil (pilote,
utilitaire, lecteur), a-relancer (message « not handled » d'une fonction BIOS ajoutée depuis), setup (le texte lu à l'écran par OCR
demande une carte son/une configuration/une touche), win32 (programme Windows, jamais lancé : relancer avec --win32), vide (0–1 capture
sans texte lisible : à examiner), peu (2–4 scènes), bon (≥ 5 scènes).

Usage : tools/triage.py DOSSIER_DE_BALAYAGE [--out docs/triage-NOM.tsv]   (lit DOSSIER/results.jsonl, shots/ ; OCR : tesseract,
variable TESSDATA_PREFIX si les données `eng` ne sont pas installées)"""
import argparse
import collections
import json
import os
import re
import subprocess

PUB = re.compile(r"pitrlz|!art!|zmep5|(^|/)vnt\.exe$", re.I)
TOOL = re.compile(r"aweutil|s3vbe|restore\.com|(^|/)mouse\.exe$|testapp|setup|install|config|(^|/)file_id\.exe$|(^|/)dos32\.exe$|(^|/)p\.exe$", re.I)
SETUP = re.compile(r"sound ?card|select sound|sound output|setup|configuration|gmidi|esc for no music|press any key|choose|select", re.I)
NOTHANDLED = re.compile(r"INT 10h AH=(0C|0D|1B|04|0B|14)h not handled|INT 1[47]h|INT 16h AH=(04|09|0A)h")


def notes(path):
    """Verdicts posés à la main (`docs/triage-notes.tsv` : sous-chaîne de l'identifiant, classe, remarque)."""
    out = []
    if os.path.exists(path):
        for l in open(path):
            if l.strip() and not l.startswith("#"):
                p = l.rstrip("\n").split("\t")
                if len(p) >= 2:
                    out.append((p[0], p[1], p[2] if len(p) > 2 else ""))
    return out


def ocr(path):
    out = subprocess.run(["tesseract", path, "-", "--psm", "6"], capture_output=True, text=True).stdout
    return " | ".join(l.strip() for l in out.splitlines() if len(l.strip()) > 3)[:160]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scan")
    ap.add_argument("--out")
    ap.add_argument("--newer", action="append", default=[], help="balayage plus récent : ses éléments remplacent ceux-ci (classe `remplace`)")
    args = ap.parse_args()
    # une ligne par identifiant (un balayage repris ou relancé en ajoute d'autres : la dernière fait foi)
    last = {}
    for l in open(os.path.join(args.scan, "results.jsonl")):
        x = json.loads(l)
        last[x["id"]] = x
    rows = list(last.values())
    newer = set()
    for d in args.newer:
        newer |= {json.loads(l)["id"] for l in open(os.path.join(d, "results.jsonl"))}
    dup = set()
    try:
        txt = subprocess.run(["python3", "-I", os.path.join(os.path.dirname(__file__), "dup-shots.py"), args.scan], capture_output=True, text=True).stdout
        dup = {m.group(1) for m in re.finditer(r"^  (\S+)  \[", txt, re.M)}
    except OSError:
        pass
    out, count = [], collections.Counter()
    manual = notes(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "triage-notes.tsv"))
    for x in rows:
        exe, k, msg = x.get("exe") or "", x.get("kept") or 0, x.get("message") or ""
        text = ""
        shots = os.path.join(args.scan, "shots", x["id"])
        if k <= 1 and os.path.isdir(shots):
            pics = sorted(f for f in os.listdir(shots) if f.endswith(".png") and f != "f.png")
            if pics:
                text = ocr(os.path.join(shots, pics[-1]))
        verdict = next(((c, n) for key, c, n in manual if key in x["id"]), None)
        short = x["status"] == "exit" and (x.get("seconds") or 99) < 12
        if verdict:
            cls, act = verdict[0], verdict[1] or "verdict manuel"
        elif x["id"] in newer:
            cls, act = "remplace", "relancé dans un balayage plus récent"
        elif PUB.search(exe):
            cls, act = "pub", "ignorer (publicité du distributeur)"
        elif x["id"] in dup:
            cls, act = "doublon", "même démo/écran qu'un autre élément : garder un seul résultat"
        elif x["status"] == "win32":
            cls, act = "win32", "relancer avec --win32"
        elif NOTHANDLED.search(msg):
            cls, act = "a-relancer", "fonction BIOS ajoutée depuis : relancer"
        elif TOOL.search(exe):
            cls, act = "outil", "pas une démo (pilote/utilitaire) : écarter ou vérifier"
        elif k <= 1 and SETUP.search(text):
            cls, act = "setup", "bloqué sur un menu : trouver les touches"
        elif k <= 1 and short:
            cls, act = "court", "se termine en moins de 12 s (captures toutes les 2 s) : relancer avec --every 0.5"
        elif k <= 1:
            cls, act = "vide", "0–1 capture : examiner (sortie immédiate, écran noir…)"
        elif k <= 4:
            cls, act = "peu", "2–4 scènes : vérifier"
        else:
            cls, act = "bon", ""
        count[cls] += 1
        out.append((x["id"], exe, str(k), x["status"], cls, act, text))
    path = args.out or f"docs/triage-{os.path.basename(os.path.abspath(args.scan))}.tsv"
    with open(path, "w") as f:
        f.write("id\texe\tscenes\tstatus\tclasse\taction\ttexte_ecran\n")
        for r in out:
            f.write("\t".join(r) + "\n")
    print(path, dict(count))


if __name__ == "__main__":
    main()
