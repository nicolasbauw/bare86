#!/usr/bin/env python3 -I
# SPDX-License-Identifier: GPL-3.0-or-later
"""Rapport d'avancement d'un balayage de CD (sweep.py) : un fichier texte (ETAT.md) et une page HTML
autonome (ETAT.html) dans le dossier de résultats.

  python3 -I tools/sweep_report.py --out RÉSULTATS --items LISTE [--profiles profiles.json]

Pour chaque CD de la liste : état (à faire, en cours, terminé), éléments traités / à traiter, répartition
des statuts, nombre de scènes, durée, puis les éléments « à revoir » (statut inhabituel, avertissement du
BIOS/DOS, peu de scènes, image statique, arrêt très rapide). Peut être relancé à tout moment, y compris
pendant un balayage : il ne lit que results.jsonl, progress.json et la liste. sweep.py l'appelle seul à la
fin de chaque CD."""

import argparse
import collections
import datetime
import html
import json
import os
import re


def cd_number(rel):
    m = re.match(r"pcteam_cd(\d+)", rel)
    return int(m.group(1)) if m else None


def ident(rel):
    return re.sub(r"[^A-Za-z0-9_.-]", "_", rel.replace("/", "_"))


def load_json(path, default):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def last_results(path, overlays=()):
    """Dernier résultat de chaque élément (un élément refait remplace le précédent)."""
    out = {}
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                try:
                    x = json.loads(line)
                except ValueError:
                    continue
                out[x.get("item") or x.get("id")] = x
    except OSError:
        pass
    for extra in overlays:  # balayages plus récents d'éléments déjà vus : leurs résultats remplacent les anciens
        out.update(last_results(os.path.join(extra, "results.jsonl")))
    return out


TINY = {"1ko", "512b", "in4k", "4k", "4ko", "in16", "5k", "in4", "in1k", "1k"}  # dossiers d'intros minuscules : peu de scènes = normal


def folder(rel):
    p = rel.split("/")
    return (p[2] if len(p) > 3 else "").lower()


def to_review(x, rel=""):
    """Raisons pour lesquelles un résultat mérite un coup d'œil (liste vide : rien à signaler).
    Un avertissement `[int]` seul n'en est pas une (informatif) ; « peu de scènes » dépend du genre
    (intros 1K/4K : moins de 2 scènes seulement)."""
    why = []
    if x.get("status") not in ("exit", "ok"):
        why.append(str(x.get("status")))
    few = 2 if folder(rel or x.get("item", "")) in TINY else 8
    if x.get("kept", 0) < few:
        why.append(f"{x.get('kept', 0)} scène(s)")
    if x.get("static"):
        why.append("statique")
    if x.get("status") == "exit" and x.get("seconds", 999) < 45 and x.get("kept", 0) < few:
        why.append("arrêt rapide")
    if why and x.get("message"):
        why.append(x["message"].strip().splitlines()[0][:70])
    return why


def bar(done, total, width=20):
    n = round(width * done / total) if total else 0
    return "█" * n + "░" * (width - n)


def build(args):
    items = [l.strip() for l in open(args.items, encoding="utf-8") if l.strip()]
    by_cd = collections.defaultdict(list)
    for rel in items:
        n = cd_number(rel)
        if n is not None:
            by_cd[n].append(rel)
    overlays = list(args.overlay)
    try:  # overlays.txt du dossier de résultats : un dossier de résultats plus récents par ligne
        overlays += [l.strip() for l in open(os.path.join(args.out, "overlays.txt"), encoding="utf-8") if l.strip()]
    except OSError:
        pass
    results = last_results(os.path.join(args.out, "results.jsonl"), overlays)
    verdicts = load_json(os.path.join(args.out, "verdicts.json"), {})  # {élément: {"verdict": ok|reprendre|hors-sujet, "note": …}}
    progress = load_json(os.path.join(args.out, "progress.json"), {})
    profiles = load_json(args.profiles, {}) if args.profiles else {}
    cats = load_json(os.path.join(args.out, "categories.json"), {})  # tools/classify.py : exécutable, musique, image…
    rows, review = [], {}
    for n in sorted(by_cd):
        rels = by_cd[n]
        done = [results[r] for r in rels if r in results]
        status = collections.Counter(x.get("status") for x in done)
        state = progress.get(str(n), {}).get("state", "à faire")
        if state != "terminé" and done and len(done) == len(rels):
            state = "terminé"
        elif state == "à faire" and done:
            state = "en cours"
        rows.append({
            "cd": n, "state": state, "items": len(rels), "done": len(done), "status": status,
            "scenes": sum(x.get("kept", 0) for x in done), "seconds": sum(x.get("seconds", 0) for x in done),
            "profiles": sum(1 for r in rels if r in profiles),
            "finished": progress.get(str(n), {}).get("finished", ""), "rels": rels, "results": results,
        })
        review[n] = [(r, to_review(results[r], r), results[r]) for r in rels
                     if r in results and to_review(results[r], r) and r not in verdicts
                     and cats.get(r, {}).get("categorie", "exécutable") == "exécutable"]
        rows[-1]["verdicts"] = collections.Counter(verdicts[r]["verdict"] for r in rels if r in verdicts)
        rows[-1]["hors"] = sum(1 for r in rels if cats.get(r, {}).get("categorie", "exécutable") != "exécutable")
    return rows, review


def text(rows, review, verdicts=None):
    total = sum(r["items"] for r in rows)
    done = sum(r["done"] for r in rows)
    scenes = sum(r["scenes"] for r in rows)
    hours = sum(r["seconds"] for r in rows) / 3600
    lines = [f"# État du balayage — {datetime.datetime.now():%Y-%m-%d %H:%M}", "",
             f"Éléments traités : {done} / {total}  {bar(done, total, 30)}  {100 * done // max(total, 1)} %",
             f"Scènes capturées : {scenes} · temps d'exécution cumulé : {hours:.1f} h", "",
             "| CD | état | traités | progression | exit | ok | autres | scènes | à revoir | vus (verdict) | hors score |",
             "|---:|:--|--:|:--|--:|--:|--:|--:|--:|--:|--:|"]
    for r in rows:
        other = r["done"] - r["status"].get("exit", 0) - r["status"].get("ok", 0)
        lines.append(f"| {r['cd']} | {r['state']} | {r['done']}/{r['items']} | {bar(r['done'], r['items'], 12)} | "
                     f"{r['status'].get('exit', 0)} | {r['status'].get('ok', 0)} | {other} | {r['scenes']} | "
                     f"{len(review[r['cd']])} | {sum(r.get('verdicts', {}).values())} | {r.get('hors', 0)} |")
    for r in rows:
        if r["state"] == "en cours":
            lines += ["", f"## CD {r['cd']} en cours : {r['done']}/{r['items']}  {bar(r['done'], r['items'], 20)}", "",
                      "| démo | état | scènes | durée | remarque |", "|:--|:--|--:|--:|:--|"]
            for rel in r["rels"]:
                x = r["results"].get(rel)
                if x is None:
                    lines.append(f"| {os.path.basename(rel)} | … en attente ou en cours | | | |")
                else:
                    lines.append(f"| {os.path.basename(rel)} | {x.get('status')} | {x.get('kept', 0)} | {x.get('seconds', 0):.0f} s | "
                                 + " ; ".join(to_review(x)) + " |")
    lines += ["", "## À revoir (exécutables seulement : statut inhabituel, avertissement, peu de scènes, statique, arrêt rapide)", ""]
    for r in rows:
        if not review[r["cd"]]:
            continue
        lines.append(f"### CD {r['cd']}")
        for rel, why, x in review[r["cd"]]:
            lines.append(f"- {os.path.basename(rel)} : {x.get('status')}, {x.get('seconds', 0):.0f} s — " + " ; ".join(why))
        lines.append("")
    if verdicts:
        lines += ["## Verdicts (éléments vus : écartés de « à revoir »)", ""]
        for v, titre in (("reprendre", "À reprendre si la démo aux cochons n'est pas trouvée"), ("ok", "Vus, OK"), ("hors-sujet", "Hors sujet")):
            sel = sorted((k, x) for k, x in verdicts.items() if x.get("verdict") == v)
            if sel:
                lines.append(f"### {titre} ({len(sel)})")
                lines += [f"- {k.split('/', 1)[-1]} : {x.get('note', '')}" for k, x in sel]
                lines.append("")
    return "\n".join(lines) + "\n"


def page(rows, review):
    total = sum(r["items"] for r in rows)
    done = sum(r["done"] for r in rows)
    esc = html.escape
    body = []
    for r in rows:
        pct = 100 * r["done"] // max(r["items"], 1)
        ok, ex = r["status"].get("ok", 0), r["status"].get("exit", 0)
        oth = r["done"] - ok - ex
        seg = lambda n, c: f'<span class="seg {c}" style="width:{100 * n / max(r["items"], 1):.1f}%"></span>'
        body.append(f'<tr><td class="n">{r["cd"]}</td><td>{esc(r["state"])}</td><td class="n">{r["done"]}/{r["items"]}</td>'
                    f'<td><div class="bar">{seg(ex, "exit")}{seg(ok, "ok")}{seg(oth, "bad")}</div></td>'
                    f'<td class="n">{r["scenes"]}</td><td class="n">{len(review[r["cd"]])}</td><td class="n">{r.get("hors", 0)}</td></tr>')
    running = ""
    for r in rows:
        if r["state"] == "en cours":
            lis = []
            for rel in r["rels"]:
                x = r["results"].get(rel)
                if x is None:
                    lis.append(f'<li class="wait">{esc(os.path.basename(rel))} <small>… en attente ou en cours</small></li>')
                else:
                    why = " ; ".join(to_review(x))
                    lis.append(f'<li>{esc(os.path.basename(rel))} — {esc(str(x.get("status")))}, {x.get("kept", 0)} scène(s), {x.get("seconds", 0):.0f} s'
                               + (f' <small>({esc(why)})</small>' if why else "") + "</li>")
            running += f'<h2>CD {r["cd"]} en cours : {r["done"]}/{r["items"]}</h2><ul>' + "".join(lis) + "</ul>"
    revs = []
    for r in rows:
        if review[r["cd"]]:
            revs.append(f'<h3>CD {r["cd"]}</h3><ul>' + "".join(
                f'<li><b>{esc(os.path.basename(rel))}</b> — {esc(str(x.get("status")))}, {x.get("seconds", 0):.0f} s : {esc(" ; ".join(why))}</li>'
                for rel, why, x in review[r["cd"]]) + "</ul>")
    return f"""<!doctype html><html lang="fr"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>État du balayage</title>
<style>
:root{{--bg:#fff;--fg:#222;--mut:#777;--exit:#4a90d9;--ok:#e0a030;--bad:#d9534f;--bar:#e6e6e6}}
@media(prefers-color-scheme:dark){{:root{{--bg:#16181c;--fg:#ddd;--mut:#999;--bar:#2a2d33}}}}
body{{background:var(--bg);color:var(--fg);font:15px system-ui,sans-serif;margin:0 auto;max-width:900px;padding:16px}}
table{{border-collapse:collapse;width:100%}}td,th{{padding:4px 8px;border-bottom:1px solid var(--bar);text-align:left}}
.n{{text-align:right;font-variant-numeric:tabular-nums}}.bar{{display:flex;height:12px;background:var(--bar);border-radius:3px;overflow:hidden;min-width:160px}}
.seg.exit{{background:var(--exit)}}.seg.ok{{background:var(--ok)}}.seg.bad{{background:var(--bad)}}
small{{color:var(--mut)}}li{{margin:2px 0}}li.wait{{color:var(--mut)}}.auto{{margin:0 0 8px}}
</style>
<h1>État du balayage</h1>
<p class="auto"><label><input type="checkbox" id="auto"> rafraîchir automatiquement</label> <small id="count"></small></p>
<p>{done} / {total} éléments traités ({100 * done // max(total, 1)} %) <small>— {datetime.datetime.now():%Y-%m-%d %H:%M}</small></p>
<p><small>Barres : <span style="color:var(--exit)">■</span> sorti seul · <span style="color:var(--ok)">■</span> arrêté (inactif ou limite) · <span style="color:var(--bad)">■</span> autre.</small></p>
<table><tr><th>CD</th><th>état</th><th>traités</th><th>statuts</th><th>scènes</th><th>à revoir</th><th>hors score</th></tr>{"".join(body)}</table>
{running}<h2>À revoir</h2>{"".join(revs) or "<p>Rien.</p>"}<script>
(function () {{
  var box = document.getElementById("auto"), out = document.getElementById("count"), left = 0, EVERY = 20;
  try {{ box.checked = localStorage.getItem("etat-auto") !== "0"; }} catch (e) {{ box.checked = true; }}
  box.addEventListener("change", function () {{
    try {{ localStorage.setItem("etat-auto", box.checked ? "1" : "0"); }} catch (e) {{}}
    left = EVERY; show();
  }});
  function show() {{ out.textContent = box.checked ? "(dans " + left + " s)" : ""; }}
  left = EVERY; show();
  setInterval(function () {{
    if (!box.checked) return;
    left -= 1; show();
    if (left <= 0) location.reload();
  }}, 1000);
}})();
</script>
</html>
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True)
    ap.add_argument("--items", required=True)
    ap.add_argument("--profiles")
    ap.add_argument("--overlay", action="append", default=[], help="dossier de résultats plus récents pour des éléments déjà vus (répétable)")
    args = ap.parse_args()
    rows, review = build(args)
    with open(os.path.join(args.out, "ETAT.md"), "w", encoding="utf-8") as f:
        f.write(text(rows, review, load_json(os.path.join(args.out, 'verdicts.json'), {})))
    with open(os.path.join(args.out, "ETAT.html"), "w", encoding="utf-8") as f:
        f.write(page(rows, review))
    print(f"{os.path.join(args.out, 'ETAT.md')} et ETAT.html écrits")


if __name__ == "__main__":
    main()
