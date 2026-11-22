#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Passe en revue une collection de démos DOS avec Bare86, sans fenêtre.

Chaque élément (archive .zip, ou dossier qui contient directement un
exécutable DOS) est décompressé dans son propre dossier de travail, son
exécutable principal lancé pendant `--limit` secondes avec des captures
toutes les `--every` secondes (`--shot-every`), puis une planche d'images
et une ligne de résultat sont écrites. Reprend là où il s'est arrêté
(éléments déjà traités sautés).

  python3 -I tools/survey.py --src COLLECTION --out RÉSULTATS [--jobs 4]

Résultats dans RÉSULTATS :
  results.jsonl      une ligne par élément (état, exécutable, message, son…)
  shots/ID/          captures (f-0005.png…, f.png à la fin)
  sheets/ID.png      planche des captures de l'élément
  review/page-NNN.png  planches de revue : une ligne par élément, `--per-page`
                     éléments par page, avec leur identifiant
  review/index.md    liste des pages et des éléments

États : `ok` (lancée jusqu'à la limite), `exit` (rendue au DOS avant),
`error` (message d'erreur de Bare86), `timeout`, `no-exe`, `win32`
(programme Windows, non lancé sans `--win32`), `bad-archive`.

Paramètres : si le programme rend la main (ou plante) sans paramètre alors que son texte en
annonce, un réessai avec un paramètre neutre puis ceux du texte (`argprobe`). Sinon, si un .BAT du dossier lance l'exécutable avec des arguments
(GO.BAT…), ils sont repris (`args` dans le résultat). `--screen` met le texte
de l'écran dans la sortie : un message sur le DPMI déclenche un second essai
avec `--dpmi`, et une demande de paramètres (« usage… ») est notée `usage`
pour un examen à la main.

Les archives sont des données non fiables : décompressées chacune dans un
dossier vide, jamais exécutées hors de Bare86. Les dossiers de travail sont
effacés après chaque élément (sauf `--keep-work`). Prévoir une destination
sur un disque où la place ne manque pas.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import wave
import array
from concurrent.futures import ThreadPoolExecutor

ARCHIVES = (".zip", ".arj", ".rar", ".lzh", ".lha")
# Exécutables écartés du choix de l'exécutable principal.
EXTENDERS = ("dos4gw", "dos32a", "dos4g", "pmodew")
SKIP_NAMES = re.compile(r"(setup|install|config|cfg|unp|pkunzip|pkunpak|unzip|arj|lha|readme|view|info|nfo|dos4gw|cwsdpmi|pmode|uvconfig|sndsetup|catalog|vesa|univbe|uninst)", re.I)
DEFAULT_KEYS = "{wait:900000}"


def ident(rel):
    """Identifiant de fichier stable d'un chemin relatif."""
    return re.sub(r"[^A-Za-z0-9._-]+", "_", rel).strip("_")[:120]


def collect(src):
    """Éléments à passer en revue : archives, et dossiers qui contiennent
    directement un .exe/.com (démos déjà décompressées)."""
    items = []
    for root, dirs, files in os.walk(src):
        dirs.sort()
        lower = [f.lower() for f in files]
        for f in sorted(files):
            if f.lower().endswith(ARCHIVES):
                items.append(os.path.join(root, f))
        if any(f.endswith((".exe", ".com")) for f in lower):
            items.append(root)
    return items


def extract(path, dest, depth=0):
    """Décompresse `path` dans `dest` (vide), puis les archives qu'elle
    contient, une fois. Vrai si quelque chose a été extrait."""
    os.makedirs(dest, exist_ok=True)
    low = path.lower()
    if low.endswith(".zip"):
        cmd = ["unzip", "-qq", "-o", "-LL", path, "-d", dest]
    elif low.endswith(".rar"):
        cmd = ["unrar", "x", "-idq", "-o+", path, dest + "/"]
    elif low.endswith((".lzh", ".lha")):
        cmd = ["lha", "xqw=" + dest, path]
    else:
        cmd = ["7z", "x", "-y", "-bso0", "-bsp0", "-o" + dest, path]
    r = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=300)
    ok = any(os.scandir(dest))
    if depth == 0:
        for root, _, files in os.walk(dest):
            for f in files:
                if f.lower().endswith(ARCHIVES):
                    inner = os.path.join(root, f)
                    extract(inner, os.path.join(root, os.path.splitext(f)[0] + "_x"), 1)
    return ok or r.returncode == 0


def kind(path):
    """'dos', 'win32' ou None (pas un exécutable)."""
    try:
        with open(path, "rb") as f:
            head = f.read(4096)
    except OSError:
        return None
    if path.lower().endswith(".com"):
        return "dos"
    if not head.startswith((b"MZ", b"ZM")):
        return None
    if len(head) >= 0x40:
        pe = int.from_bytes(head[0x3C:0x40], "little")
        if pe + 4 <= len(head) and head[pe:pe + 4] == b"PE\0\0":
            return "win32"
    return "dos"


def pick(work, name_hint):
    """Exécutable principal : nom proche de celui de l'archive, puis le plus
    gros ; rend (chemin, genre, autres)."""
    found = []
    for root, _, files in os.walk(work):
        for f in files:
            if f.lower().endswith((".exe", ".com")):
                p = os.path.join(root, f)
                k = kind(p)
                if k:
                    found.append((p, k))
    if not found:
        return None, None, []
    hint = re.sub(r"[^a-z0-9]", "", name_hint.lower())[:5]

    def score(e):
        p, k = e
        base = os.path.splitext(os.path.basename(p))[0].lower()
        return (
            k == "dos",
            not SKIP_NAMES.search(base),
            bool(hint) and base.startswith(hint[:3]),
            os.path.getsize(p),
        )

    found.sort(key=score, reverse=True)
    main, k = found[0]
    return main, k, [os.path.relpath(p, work) for p, _ in found[1:]]


def bat_args(exe):
    """Arguments avec lesquels un .BAT du même dossier lance cet exécutable
    (GO.BAT, RUN.BAT…) : liste vide s'il n'y en a pas. Les lignes qui
    dépendent de paramètres du .BAT (%1…) sont ignorées."""
    d = os.path.dirname(exe)
    stem = os.path.splitext(os.path.basename(exe))[0].lower()
    try:
        bats = sorted(f for f in os.listdir(d) if f.lower().endswith(".bat"))
    except OSError:
        return []
    for b in bats:
        try:
            with open(os.path.join(d, b), encoding="latin-1") as f:
                lines = f.read().splitlines()
        except OSError:
            continue
        for line in lines:
            words = line.strip().lstrip("@").split()
            if words and words[0].lower() in ("call", "start"):
                words = words[1:]
            if len(words) < 2 or "%" in line or "|" in line or "<" in line or ">" in line:
                continue
            name = words[0].lower()
            if name.endswith((".exe", ".com")):
                name = name[:-4]
            if name == stem:
                return words[1:]
            # « DOS4GW PROG.EXE args » : l'extendeur du dossier lance le
            # programme, qui n'a pas d'amorce (NT2.BAT de NRU-NT2).
            if name in EXTENDERS and len(words) > 1 and os.path.splitext(words[1].lower())[0] == stem:
                present = {f.lower(): f for f in os.listdir(d)}
                for ext in (".exe", ".com"):
                    if name + ext in present:
                        return [present[name + ext]] + words[1:]
    return []


def nearly_same(a, b):
    """Deux captures presque identiques (au plus 0,5 % de pixels différents :
    un curseur qui clignote ne compte pas)."""
    r = subprocess.run(["magick", "compare", "-metric", "AE", "-fuzz", "8%", a, b, "null:"], capture_output=True, text=True)
    try:
        diff = float(r.stderr.split()[0])
    except (ValueError, IndexError):
        return False
    w = subprocess.run(["magick", "identify", "-format", "%w %h", a], capture_output=True, text=True).stdout.split()
    total = int(w[0]) * int(w[1]) if len(w) == 2 else 307200
    return diff <= total * 0.005


def sound_peak(path):
    try:
        with wave.open(path) as w:
            a = array.array("h", w.readframes(w.getnframes()))
        return max((abs(x) for x in a[::7]), default=0)
    except Exception:
        return -1


PARAM_HINT = re.compile(r"param|usage|argument|command.?line", re.I)


def param_candidates(out):
    """Paramètres à essayer : un neutre, puis ceux que le texte propose
    (« param. 8 for … », « usage: PROG x »), trois au plus."""
    found = []
    for m in re.finditer(r"(?im)^\W*(?:param(?:eter)?s?\.?|arg(?:ument)?s?\.?)\s*[:=]?\s*['\"\[<]?([\w/-]{1,12})['\"\]>]?\s+(?:for|to|=|-)\b", out):
        if m.group(1) not in found:
            found.append(m.group(1))
    return ["x"] + found[:3]


def status_exit(res):
    return res.get("status") in ("exit", "error")


def run_one(item, args, src):
    rel = os.path.relpath(item, src)
    iid = ident(rel)
    res = {"id": iid, "item": rel, "status": None}
    work = os.path.join(args.out, "work", iid)
    shots = os.path.join(args.out, "shots", iid)
    shutil.rmtree(work, ignore_errors=True)
    shutil.rmtree(shots, ignore_errors=True)
    os.makedirs(shots)
    t0 = time.time()
    try:
        if os.path.isdir(item):
            run_dir = item
        else:
            if not extract(item, work):
                res["status"] = "bad-archive"
                return res
            run_dir = work
        exe, k, others = pick(run_dir, os.path.basename(item))
        res["others"] = others[:12]
        if exe is None:
            res["status"] = "no-exe"
            return res
        res["exe"] = os.path.relpath(exe, run_dir)
        res["size"] = os.path.getsize(exe)
        if k == "win32" and not args.win32:
            res["status"] = "win32"
            return res
        wav = os.path.join(shots, "sound.wav")
        # --screen : le texte de l'écran est aussi dans la sortie, même quand le
        # programme rend la main au DOS avant la limite (message de DPMI absent…).
        cmd = [args.bare86, "--no-realtime", "--limit", str(args.limit), "--shot", os.path.join(shots, "f.png"),
               "--shot-every", str(args.every), "--wav", wav, "--keys", args.keys, "--screen"]
        if args.gus:
            cmd.append("--gus")
        if args.mips:
            cmd += ["--mips", str(args.mips)]
        # Paramètres : ceux d'un .BAT du dossier qui lance cet exécutable ;
        # le BAT peut aussi lancer un extendeur du dossier sur l'exécutable
        # (le premier mot est alors le programme à lancer).
        extra = bat_args(exe)
        if extra and os.path.splitext(extra[0].lower())[0] in EXTENDERS:
            cmd += extra
        else:
            cmd.append(os.path.basename(exe))
            cmd += extra
        if extra:
            res["args"] = extra
        out = ""
        timed = False

        def execute(command):
            try:
                p = subprocess.run(command, cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                   errors="replace", timeout=args.limit + 90)
                return p.stdout + p.stderr, False
            except subprocess.TimeoutExpired as e:
                return ((e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")), True

        for attempt in range(2):
            out, timed = execute(cmd)
            # Hôte DPMI demandé (« DPMI? », « no DPMI ») : un second essai avec --dpmi.
            if attempt == 0 and re.search(r"dpmi", out, re.I) and "--dpmi" not in cmd:
                cmd.insert(1, "--dpmi")
                res["dpmi"] = True
                continue
            break
        # Rendu au DOS (ou plantage) sans paramètre, alors que le texte en annonce :
        # réessai avec un paramètre neutre, puis avec ceux que le texte propose
        # (Decapitation plante sur argv[1] nul sans paramètre).
        if not extra and not timed and "time limit reached" not in out and (PARAM_HINT.search(out) or "SIGSEGV" in out):
            for cand in param_candidates(out):
                out2, timed2 = execute(cmd + [cand])
                if "time limit reached" in out2:
                    out, timed, res["args"], res["argprobe"] = out2, timed2, [cand], True
                    break
        lines = [l for l in out.splitlines() if l.strip()]
        if timed:
            res["status"] = "timeout"
        elif any("time limit reached" in l for l in lines):
            res["status"] = "ok"
        elif any(l.startswith("error:") or "panic" in l for l in lines):
            res["status"] = "error"
        else:
            res["status"] = "exit"
        msgs = [l for l in lines if l.startswith("error:") and "time limit" not in l] + [l for l in lines if "not handled" in l]
        res["message"] = (msgs[0] if msgs else "")[:200]
        if "screen:" in out:
            screen = out.split("screen:", 1)[1]
            res["screen"] = " | ".join(s.strip() for s in screen.splitlines() if s.strip())[:300]
        # Le programme réclame des paramètres : à regarder à la main.
        use = next((l.strip() for l in lines if re.search(r"usage|parameter|argument|syntax|command.?line", l, re.I) and "[int]" not in l), "")
        if use and status_exit(res):
            res["usage"] = use[:120]
        res["sound_peak"] = sound_peak(wav)
        try:
            os.remove(wav)
        except OSError:
            pass
        return res
    except Exception as e:
        res["status"] = "error"
        res["message"] = f"survey: {e}"[:200]
        return res
    finally:
        res["seconds"] = round(time.time() - t0, 1)
        res["shots"] = sorted(f for f in os.listdir(shots) if f.endswith(".png")) if os.path.isdir(shots) else []
        # Figé : toutes les captures identiques (écran de configuration qui
        # attend une touche, plantage…) ; `--retry-static` les relance.
        periodic = [f for f in res["shots"] if f != "f.png"]
        if periodic:
            res["static"] = all(nearly_same(os.path.join(shots, a), os.path.join(shots, b)) for a, b in zip(periodic, periodic[1:]))
        res["keys"] = args.keys
        if res["shots"]:
            sheet = os.path.join(args.out, "sheets", iid + ".png")
            files = [os.path.join(shots, f) for f in res["shots"] if f != "f.png"] or [os.path.join(shots, "f.png")]
            subprocess.run(["montage", *files, "-geometry", "160x120+2+2", "-tile", f"{len(files)}x1", sheet],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not args.keep_work:
            shutil.rmtree(work, ignore_errors=True)


def review(args):
    """Planches de revue : une ligne par élément (sa planche), identifiant
    en légende ; et un index en Markdown."""
    last = {}
    for l in open(os.path.join(args.out, "results.jsonl")):
        r = json.loads(l)
        last[r["id"]] = r
    rows = list(last.values())
    rows = [r for r in rows if os.path.exists(os.path.join(args.out, "sheets", r["id"] + ".png"))]
    rdir = os.path.join(args.out, "review")
    shutil.rmtree(rdir, ignore_errors=True)
    os.makedirs(rdir)
    index = ["# Revue", ""]
    for n in range(0, len(rows), args.per_page):
        page = rows[n:n + args.per_page]
        name = f"page-{n // args.per_page + 1:03}.png"
        cmd = ["montage"]
        for r in page:
            tag = r["status"] + (", figé" if r.get("static") else "")
            cmd += ["-label", f"{r['id']} [{tag}]", os.path.join(args.out, "sheets", r["id"] + ".png")]
        cmd += ["-tile", "1x", "-geometry", "+2+6", "-pointsize", "13", os.path.join(rdir, name)]
        subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        index.append(f"## {name}")
        index += [f"- `{r['id']}` — {r['status']}{', figé' if r.get('static') else ''}, {r.get('exe', '')}, son {r.get('sound_peak', '')}, touches `{r.get('keys', '')}`" for r in page]
        index.append("")
    open(os.path.join(rdir, "index.md"), "w").write("\n".join(index))
    print(f"{len(rows)} éléments, {(len(rows) + args.per_page - 1) // args.per_page} pages dans {rdir}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", required=True, help="collection (archives et dossiers, parcourue récursivement)")
    ap.add_argument("--out", required=True, help="résultats (sur un disque qui a de la place)")
    ap.add_argument("--bare86", default=os.path.expanduser("~/Dev/bare86/target/release/bare86"))
    ap.add_argument("--jobs", type=int, default=4, help="éléments en parallèle (défaut 4)")
    ap.add_argument("--limit", type=int, default=60, help="secondes par élément (défaut 60)")
    ap.add_argument("--every", type=float, default=5, help="secondes entre deux captures (défaut 5)")
    ap.add_argument("--mips", type=int, default=200, help="bridage (défaut 200, la règle des démos ; 0 : pleine vitesse)")
    ap.add_argument("--keys", default=DEFAULT_KEYS, help="script de touches (défaut : aucune touche, entrée gardée ouverte)")
    ap.add_argument("--no-gus", dest="gus", action="store_false", help="sans Gravis UltraSound")
    ap.add_argument("--win32", action="store_true", help="lancer aussi les programmes Windows (par HX)")
    ap.add_argument("--filter", help="expression régulière sur le chemin relatif des éléments")
    ap.add_argument("--max", type=int, help="au plus N éléments (essais)")
    ap.add_argument("--redo", action="store_true", help="refaire les éléments déjà traités")
    ap.add_argument("--retry-static", metavar="KEYS", help="relancer seulement les éléments figés (dernier résultat), avec ce script de touches")
    ap.add_argument("--keep-work", action="store_true", help="garder les dossiers de travail")
    ap.add_argument("--per-page", type=int, default=12, help="éléments par planche de revue")
    ap.add_argument("--review-only", action="store_true", help="refaire seulement les planches de revue")
    args = ap.parse_args()
    src = os.path.abspath(args.src)
    args.out = os.path.abspath(args.out)
    os.makedirs(os.path.join(args.out, "sheets"), exist_ok=True)
    results = os.path.join(args.out, "results.jsonl")
    if args.review_only:
        review(args)
        return
    last = {}
    if os.path.exists(results):
        for l in open(results):
            r = json.loads(l)
            last[r["id"]] = r
    items = collect(src)
    if args.filter:
        items = [i for i in items if re.search(args.filter, os.path.relpath(i, src), re.I)]
    if args.retry_static:
        args.keys = args.retry_static
        items = [i for i in items if last.get(ident(os.path.relpath(i, src)), {}).get("static")]
    elif not args.redo:
        items = [i for i in items if ident(os.path.relpath(i, src)) not in last]
    done = last
    if args.max:
        items = items[:args.max]
    print(f"{len(items)} éléments à traiter ({len(done)} déjà faits)", flush=True)
    lock = threading.Lock()
    count = [0]

    def job(item):
        r = run_one(item, args, src)
        with lock:
            with open(results, "a") as f:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
            count[0] += 1
            print(f"[{count[0]}/{len(items)}] {r['status']:11} {r['id']}", flush=True)

    with ThreadPoolExecutor(max_workers=args.jobs) as ex:
        list(ex.map(job, items))
    review(args)


if __name__ == "__main__":
    sys.exit(main())
