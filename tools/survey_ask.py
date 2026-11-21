#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Pose une question sur chaque planche d'un balayage (`tools/survey.py`) à
un modèle de vision local, par une API compatible OpenAI (Lemonade, sur
cette machine : http://localhost:13305/api/v1).

  python3 -I tools/survey_ask.py --out RÉSULTATS --model Gemma-4-12B-it-GGUF \\
      --question "Décris ces images en une phrase. Y voit-on un cochon ?"

Pour chaque élément (dernier résultat de `results.jsonl` qui a une planche),
la planche `sheets/ID.png` est envoyée avec la question ; la réponse est
ajoutée à `asks.jsonl` (id, question, modèle, réponse). Reprend là où il
s'est arrêté (même question et même modèle). `--grep MOT` affiche ensuite
les éléments dont la réponse contient MOT.

Le modèle doit être téléchargé (`lemonade pull MODÈLE`) ; le serveur le
charge à la première requête.
"""
import argparse
import base64
import json
import os
import sys
import urllib.request


def ask(url, model, question, png, timeout):
    data = base64.b64encode(open(png, "rb").read()).decode()
    body = {
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": question},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64," + data}},
            ],
        }],
        "temperature": 0,
        "max_tokens": 300,
    }
    req = urllib.request.Request(url.rstrip("/") + "/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        reply = json.load(r)
    return reply["choices"][0]["message"]["content"].strip()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="dossier de résultats de tools/survey.py")
    ap.add_argument("--model", required=True, help="modèle de vision (nom donné par `lemonade list`)")
    ap.add_argument("--question", help="question posée sur chaque planche")
    ap.add_argument("--url", default="http://localhost:13305/api/v1", help="API compatible OpenAI")
    ap.add_argument("--max", type=int, help="au plus N planches (essais)")
    ap.add_argument("--timeout", type=int, default=600, help="secondes par requête (le premier chargement du modèle est long)")
    ap.add_argument("--grep", help="afficher les éléments dont la réponse contient ce mot (sans interroger le modèle)")
    args = ap.parse_args()
    asks = os.path.join(args.out, "asks.jsonl")
    if args.grep:
        for l in open(asks):
            a = json.loads(l)
            if args.grep.lower() in a["answer"].lower():
                print(f"{a['id']}: {a['answer']}")
        return 0
    if not args.question:
        ap.error("--question requise (sauf avec --grep)")
    last = {}
    for l in open(os.path.join(args.out, "results.jsonl")):
        r = json.loads(l)
        last[r["id"]] = r
    done = set()
    if os.path.exists(asks):
        for l in open(asks):
            a = json.loads(l)
            if a["question"] == args.question and a["model"] == args.model:
                done.add(a["id"])
    todo = [i for i in last if i not in done and os.path.exists(os.path.join(args.out, "sheets", i + ".png"))]
    if args.max:
        todo = todo[:args.max]
    print(f"{len(todo)} planches à interroger ({len(done)} déjà faites)", flush=True)
    for n, i in enumerate(todo, 1):
        try:
            answer = ask(args.url, args.model, args.question, os.path.join(args.out, "sheets", i + ".png"), args.timeout)
        except Exception as e:
            print(f"[{n}/{len(todo)}] {i}: erreur {e}", flush=True)
            continue
        with open(asks, "a") as f:
            f.write(json.dumps({"id": i, "question": args.question, "model": args.model, "answer": answer}, ensure_ascii=False) + "\n")
        print(f"[{n}/{len(todo)}] {i}: {answer[:150]}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
