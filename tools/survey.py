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
# Scripts de touches essayés tour à tour (--probe) : la plupart des démos attendent
# une touche (« hit a key to run the demo »), un choix de carte son ou de mode vidéo.
KEY_CANDIDATES = [
    "{wait:900000}",
    "{wait:3000}{enter}{wait:900000}",
    "{wait:3000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:900000}",
    "{wait:3000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:900000}",
    "{wait:3000}2{wait:2000}{enter}{wait:2000}{enter}{wait:900000}",
    "{wait:3000}1{wait:2000}{enter}{wait:2000}{enter}{wait:900000}",
    "{wait:3000} {wait:2000}{enter}{wait:900000}",
    # Choix d'un chiffre sans Entrée (« Select sound card: 1. SB 2. GUS »), puis avec écrans enchaînés
    # (Useless Sound System : Entrée, Entrée, mode 1, Entrée).
    "{wait:3000}2{wait:900000}",
    "{wait:3000}1{wait:900000}",
    "{wait:3000}{enter}{wait:2000}{enter}{wait:3000}1{wait:3000}{enter}{wait:900000}",
    # IMS (Indoor Music System) : « return:start » demande DEUX Entrées à moins de 36 tics (≈ 2 s) l'une
    # de l'autre ; le premier écran (mode vidéo, etc.) se passe avec les Entrées espacées de 2 s.
    "{wait:3000}{enter}{wait:500}{enter}{wait:900000}",
    "{wait:3000}{enter}{wait:2000}{enter}{wait:500}{enter}{wait:900000}",
    "{wait:3000}1{wait:2000}{enter}{wait:2000}{enter}{wait:500}{enter}{wait:900000}",
]
PROFILE_LOCK = threading.Lock()
LAST = threading.local()  # résultat annexe de watch_run (CPU), par thread
EXTENDERS = ("dos4gw", "dos32a", "dos4g", "pmodew")
SKIP_NAMES = re.compile(r"(setup|install|config|cfg|unp|pkunzip|pkunpak|unzip|arj|lha|readme|view|info|nfo|dos4gw|cwsdpmi|pmode|uvconfig|sndsetup|catalog|vesa|univbe|uninst)", re.I)
# Exécutables « publicitaires » d'un groupe (écran fixe ou animé identique d'une archive à l'autre) : jamais choisis comme exécutable
# principal quand l'archive en contient d'autres (cas AMB98 : `exc-whq.exe`, « Future Dimension », choisi à la place de la démo).
AD_NAMES = re.compile(r"^(exc[-_]?whq|cfxadv|#pitrlz#|!art!|zmep5|vnt)$", re.I)
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


def pick(work, name_hint, only=None):
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
    if only:
        # un exécutable précis de l'élément (chemin relatif, sans tenir compte de la casse) : le reste est « others »
        want = only.replace("\\", "/").lower()
        sel = [e for e in found if os.path.relpath(e[0], work).replace("\\", "/").lower() == want]
        if not sel:
            return None, None, []
        rest = [e for e in found if e is not sel[0]]
        return sel[0][0], sel[0][1], [os.path.relpath(p, work) for p, _ in rest]
    if not found:
        return None, None, []
    hint = re.sub(r"[^a-z0-9]", "", name_hint.lower())[:5]

    def score(e):
        p, k = e
        base = os.path.splitext(os.path.basename(p))[0].lower()
        return (
            k == "dos",
            not AD_NAMES.search(base),
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


# Commandes que le shell intégré exécute lui-même (src/dos/batch.rs, shell.rs) ; toute autre
# première ligne d'un .BAT est un programme à lancer.
BAT_INTERNAL = {"echo", "rem", "set", "path", "cls", "cd", "chdir", "md", "mkdir", "rd", "rmdir", "del", "erase", "ren", "rename",
                "type", "dir", "pause", "goto", "call", "shift", "if", "for", "lh", "loadhigh", "break", "verify", "prompt",
                "mode", "ctty", "exit", "ver"}
# Commandes externes ou internes que le shell intégré ne sait pas faire : le .BAT n'est pas lancé tel quel.
BAT_UNSUPPORTED = {"choice", "xcopy", "deltree", "mem", "edit", "format", "fdisk", "scandisk", "attrib", "mkdir", "label", "date", "time"}


def bat_script(exe):
    """Un .BAT du dossier de `exe` qui lance cet exécutable et que le shell intégré sait exécuter :
    (nom, nombre de programmes lancés), ou None. Les BAT d'installation ou de configuration, ou qui
    emploient une commande qu'il ignore (COPY, CHOICE…), sont écartés."""
    d = os.path.dirname(exe)
    stem = os.path.splitext(os.path.basename(exe))[0].lower()
    try:
        bats = sorted(f for f in os.listdir(d) if f.lower().endswith(".bat") and not SKIP_NAMES.search(os.path.splitext(f)[0]))
    except OSError:
        return None
    for b in bats:
        try:
            with open(os.path.join(d, b), encoding="latin-1") as f:
                lines = f.read().splitlines()
        except OSError:
            continue
        programs, mentions, ok = 0, False, True
        for line in lines:
            words = line.strip().lstrip("@").split()
            if not words or words[0].startswith(":") or words[0].lower() == "rem":
                continue
            while words and words[0].lower() in ("call", "lh", "loadhigh", "start"):
                words = words[1:]
            if not words:
                continue
            name = words[0].lower()
            if len(name) == 2 and name[1] == ":":  # changement de lecteur
                continue
            base = os.path.splitext(name)[0] if name.endswith((".exe", ".com")) else name
            if base in BAT_UNSUPPORTED or "|" in line:
                ok = False
                break
            if base in BAT_INTERNAL or base.rstrip(".") in BAT_INTERNAL or base.startswith("echo"):  # « echo. »
                continue
            programs += 1
            if base == stem or (base in EXTENDERS and len(words) > 1 and os.path.splitext(words[1].lower())[0] == stem):
                mentions = True
        if ok and mentions:
            return b, programs
    return None


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


def thumb(path):
    """Vignette 32×20 en niveaux de gris (640 octets) pour comparer des captures."""
    r = subprocess.run(["magick", path, "-resize", "32x20!", "-colorspace", "Gray", "-depth", "8", "gray:-"], capture_output=True)
    return r.stdout


def thumb_diff(a, b):
    """Écart moyen (0 à 255) entre deux vignettes."""
    return sum(abs(x - y) for x, y in zip(a, b)) / max(1, min(len(a), len(b)))


def frame_cap(args, duration, loop, n_scenes=0):
    """Nombre maximal de scènes gardées pour un programme : `--max-frames` s'il est donné ;
    sinon `--min-frames` (48) pour une démo courte ou qui boucle, une scène par
    `--seconds-per-frame` secondes de démo au-delà, et, quand les scènes sont DENSES (une toutes
    les `--dense-seconds` secondes ou moins), toutes celles vues, jusqu'à `--frames-cap`."""
    if args.max_frames:
        return args.max_frames
    if loop:
        return args.min_frames
    cap = max(args.min_frames, min(args.frames_cap, round(duration / args.seconds_per_frame)))
    if n_scenes and duration and duration / n_scenes <= args.dense_seconds:
        cap = max(cap, min(args.frames_cap, n_scenes))
    return cap


def detect_loop(thumbs, n_match, eps):
    """Boucle dans la suite des scènes gardées : les `n_match` dernières scènes
    ressemblent (écart < `eps`) à `n_match` scènes consécutives plus anciennes,
    dans le même ordre, sans recouvrement. Rend la période (en scènes) ou None."""
    n = len(thumbs)
    if n < 2 * n_match:
        return None
    for j in range(n - 1 - n_match, n_match - 2, -1):
        if all(thumb_diff(thumbs[n - 1 - i], thumbs[j - i]) < eps for i in range(n_match)):
            return n - 1 - j
    return None


class CpuSampler(threading.Thread):
    """Échantillonne le CPU du processus (utime + stime de /proc, tous threads) toutes
    les 0,5 s : une moyenne sur la durée cacherait un pic. Résumé : `cpu` moyenne,
    `cpu_peak` maximum d'un échantillon, `cpu_hot` part des échantillons au-dessus de
    0,9 cœur, `cpu_peak_at` seconde du pic (cœurs, 1,0 = 100 % d'un cœur)."""

    STEP = 0.5

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.stop, self.samples = pid, False, []

    def read(self):
        try:
            st = open(f"/proc/{self.pid}/stat").read().rsplit(")", 1)[1].split()
            return (int(st[11]) + int(st[12])) / os.sysconf("SC_CLK_TCK")
        except (OSError, IndexError, ValueError):
            return None

    def run(self):
        t0 = prev_t = time.time()
        prev = self.read() or 0.0
        while not self.stop:
            time.sleep(self.STEP)
            cur, now = self.read(), time.time()
            if cur is None:
                break
            self.samples.append((now - t0, (cur - prev) / max(now - prev_t, 1e-3)))
            prev, prev_t = cur, now

    def summary(self):
        s = self.samples
        if not s:
            return {}
        peak = max(s, key=lambda x: x[1])
        return {"cpu": round(sum(v for _, v in s) / len(s), 2), "cpu_peak": round(peak[1], 2),
                "cpu_peak_at": round(peak[0]), "cpu_hot": round(sum(v > 0.9 for _, v in s) / len(s), 2)}


def watch_run(command, cwd, shots, args, timeout):
    """Lance le programme et surveille les captures (mode --full) : on ne garde
    que celles qui diffèrent de la dernière gardée de plus de `--change`
    (changement de scène), les autres sont effacées au fil de l'eau ; sans
    nouvelle image gardée pendant `--idle` secondes (écran figé : menu final,
    boucle), le programme est arrêté. Rend (sortie, expiré, nombre gardé, arrêté)."""
    for f in os.listdir(shots):
        if f.endswith(".png"):
            os.remove(os.path.join(shots, f))
    proc = subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    chunks = []
    reader = threading.Thread(target=lambda: chunks.append(proc.stdout.read()), daemon=True)
    reader.start()
    kept, kept_thumbs, kept_t, last_thumb, seen = [], [], [], None, set()
    t0 = last_new = last_keep = last_scene = time.time()
    killed = timed = False
    LAST.loop = None
    cpu = CpuSampler(proc.pid)
    cpu.start()
    while proc.poll() is None:
        time.sleep(2)
        now = time.time()
        for f in sorted(os.listdir(shots)):
            path = os.path.join(shots, f)
            if not f.endswith(".png") or f == "f.png" or f in seen or now - os.path.getmtime(path) < 1.5:
                continue
            seen.add(f)
            t = thumb(path)
            if os.environ.get("SURVEY_DEBUG"):
                print(f"[watch] {f} diff={thumb_diff(t, last_thumb) if last_thumb is not None else None}", file=sys.stderr, flush=True)
            d = thumb_diff(t, last_thumb) if last_thumb is not None else None
            # Une scène à petits mouvements (sprite sur fond noir : bel_wazo, ELE, goatzill) change peu entre deux captures : une capture
            # est aussi gardée toutes les 30 s si l'écran a un peu bougé, et un écran qui bouge un peu n'est pas « figé » (pas d'arrêt idle).
            heartbeat = d is not None and d > 0.8 and now - last_keep > 30
            if d is not None and d <= args.change and d > 0.3 and not heartbeat:
                last_new = now
            if last_thumb is None or d > args.change or heartbeat:
                last_keep = now
                if not heartbeat:
                    last_scene = now
                kept.append(f)
                kept_thumbs.append(t)
                kept_t.append(now - t0)
                last_thumb, last_new = t, now
                period = detect_loop(kept_thumbs, args.loop_scenes, args.loop_eps) if args.loop_stop else None
                if period:
                    # Le programme boucle : on garde le premier tour, sans les scènes qui le recommencent.
                    LAST.loop = {"scenes": period, "seconds": round(kept_t[-1] - kept_t[-1 - period], 1)}
                    for g in kept[-args.loop_scenes:]:
                        os.remove(os.path.join(shots, g))
                    del kept[-args.loop_scenes:]
                    del kept_thumbs[-args.loop_scenes:]
                    del kept_t[-args.loop_scenes:]
                    proc.terminate()
                    killed = "loop"
                    break
            else:
                os.remove(path)
        if killed:
            break
        if args.idle and now - last_new > args.idle and now - t0 > 30:
            proc.terminate()
            killed = True
            break
        # Échéance : `timeout`, repoussée jusqu'à `--hard-limit` tant que de nouvelles scènes arrivent.
        deadline = args.hard_limit if args.extend and now - last_scene < 60 else timeout
        if now - t0 > deadline:
            proc.kill()
            timed = True
            break
    try:
        proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        proc.kill()
    reader.join(timeout=10)
    out = (chunks[0] if chunks else b"").decode(errors="replace")
    if killed:
        out += "\ntime limit reached (%s)\n" % ("loop detected" if killed == "loop" else "idle: screen unchanged")
    # Captures restantes non encore traitées (fin de course) : même tri.
    for f in sorted(os.listdir(shots)):
        path = os.path.join(shots, f)
        if f.endswith(".png") and f != "f.png" and f not in seen:
            t = thumb(path)
            if last_thumb is None or thumb_diff(t, last_thumb) > args.change:
                kept.append(f)
                last_thumb = t
            else:
                os.remove(path)
    cpu.stop = True
    cpu.join(timeout=2)
    LAST.cpu = cpu.summary()
    LAST.duration = time.time() - t0
    return out, timed, len(kept), killed


def load_profiles(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return {}


def save_profile(path, rel, profile):
    """Ajoute le profil d'un élément (clavier, options) au fichier partagé."""
    with PROFILE_LOCK:
        data = load_profiles(path)
        data[rel] = profile
        tmp = path + ".tmp"
        with open(tmp, "w") as f:
            json.dump(data, f, indent=1, ensure_ascii=False, sort_keys=True)
        os.replace(tmp, path)


def with_option(command, option, value):
    """Copie de `command` dont la valeur de `option` est remplacée."""
    c = list(command)
    c[c.index(option) + 1] = value
    return c


def probe_keys(command, cwd, shots, args):
    """Essaie les scripts de `KEY_CANDIDATES` quelques secondes chacun et rend
    (meilleur script, score par script) : le score est le nombre de scènes
    distinctes vues ; un programme qui se termine avant la fin de l'essai avec moins
    de deux scènes (une touche l'a fait quitter, plantage) vaut -1."""
    scores = {}
    for keys in KEY_CANDIDATES:
        c = with_option(with_option(command, "--keys", keys), "--limit", str(args.probe))
        out, timed, kept, killed = watch_run(c, cwd, shots, args, args.probe + 60)
        ended_early = "time limit reached" not in out and not killed and not timed
        # Un programme qui rend la main après avoir montré plusieurs scènes a fini normalement
        # (intro courte) : il vaut ses scènes. Seul un arrêt quasi immédiat (plantage, touche
        # qui fait quitter) vaut -1.
        scores[keys] = -1 if ended_early and kept < 2 else kept
        if scores[keys] >= 6:
            break
    best = max(KEY_CANDIDATES, key=lambda k: scores.get(k, -2))
    return best, scores


def status_exit(res):
    return res.get("status") in ("exit", "error")


def run_prep(prep, root, args):
    """Étapes de préparation d'un profil (`prep`), dans le dossier de travail : `remove` (fichiers
    à supprimer), `sub` (substitutions dans un fichier de config), `run` (programme DOS à lancer dans Bare86 avec `keys`, `limit` secondes ; par
    exemple le SETUP.EXE d'une démo pour écrire une configuration adaptée à la machine émulée)."""
    for step in prep:
        for name in step.get("remove", []):
            for dirpath, _, files in os.walk(root):
                for f in files:
                    if f.lower() == name.lower():
                        os.remove(os.path.join(dirpath, f))
        for sub in step.get("sub", []):  # {"file", "pattern", "repl"} : ligne de config d'auteur à corriger
            for dirpath, _, files in os.walk(root):
                for f in files:
                    if f.lower() == sub["file"].lower():
                        p = os.path.join(dirpath, f)
                        with open(p, "r", encoding="latin-1", newline="") as fh:
                            text = fh.read()
                        with open(p, "w", encoding="latin-1", newline="") as fh:
                            fh.write(re.sub(sub["pattern"], sub["repl"], text, flags=re.M | re.I))
        if "run" in step:
            exe = next((os.path.join(d, f) for d, _, fs in os.walk(root) for f in fs if f.lower() == step["run"].lower()), None)
            if exe:
                cmd = [args.bare86, "--no-realtime", "--limit", str(step.get("limit", 20)), "--keys", step.get("keys", "{enter}"), "--gus",
                       "--mips", str(args.mips or 200), os.path.basename(exe)]
                subprocess.run(cmd, cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, timeout=step.get("limit", 20) + 60)


def run_one(item, args, src, only=None):
    rel = os.path.relpath(item, src)
    iid = ident(rel) + (("__" + ident(only)) if only else "")
    res = {"id": iid, "item": rel, "status": None}
    # Profil d'un exécutable précis d'une archive à plusieurs exécutables : clé « élément|exécutable » ; sinon celle de l'élément.
    pkey = f"{rel}|{only}" if only and f"{rel}|{only}" in args.profile_data else rel
    if only:
        res["only"] = only
    work = os.path.join(args.out, "work", iid)
    shots = os.path.join(args.out, "shots", iid)
    shutil.rmtree(work, ignore_errors=True)
    shutil.rmtree(shots, ignore_errors=True)
    os.makedirs(shots)
    t0 = time.time()
    try:
        prep = (args.profile_data.get(pkey) or {}).get("prep")
        if os.path.isdir(item) and prep:
            # Préparation demandée par le profil : sur une copie, jamais dans la collection.
            shutil.copytree(item, work)
            run_dir = work
            run_prep(prep, work, args)
        elif os.path.isdir(item) and args.dll_pack:
            shutil.copytree(item, work)  # des DLL vont être ajoutées : jamais dans la collection
            run_dir = work
        elif os.path.isdir(item):
            run_dir = item
        else:
            if not extract(item, work):
                res["status"] = "bad-archive"
                return res
            run_dir = work
            if prep:
                run_prep(prep, work, args)
        exe, k, others = pick(run_dir, os.path.basename(item), only)
        res["others"] = others[:12]
        if exe is None:
            res["status"] = "no-exe"
            return res
        res["exe"] = os.path.relpath(exe, run_dir)
        res["size"] = os.path.getsize(exe)
        if k == "win32" and not args.win32:
            res["status"] = "win32"
            return res
        if k == "win32" and args.dll_pack:
            # Programmes Windows : DLL d'exécution de Microsoft (MSVCRT…) que HX ne fournit pas, ajoutées à côté de
            # l'exécutable si le programme ne les apporte pas lui-même.
            have = {f.lower() for f in os.listdir(os.path.dirname(exe))}
            for f in os.listdir(args.dll_pack):
                if f.lower() not in have:
                    shutil.copy(os.path.join(args.dll_pack, f), os.path.join(os.path.dirname(exe), f))
        wav = os.path.join(shots, "sound.wav")
        # --screen : le texte de l'écran est aussi dans la sortie, même quand le
        # programme rend la main au DOS avant la limite (message de DPMI absent…).
        cmd = [args.bare86, "--no-realtime", "--limit", str(args.hard_limit if args.full and args.extend else args.limit),
               "--shot", os.path.join(shots, "f.png"),
               "--shot-every", str(args.every), "--keys", args.keys, "--screen"]
        if not args.full:
            cmd += ["--wav", wav]
        if args.gus:
            cmd.append("--gus")
        if args.mips:
            cmd += ["--mips", str(args.mips)]
        # Paramètres : ceux d'un .BAT du dossier qui lance cet exécutable ;
        # le BAT peut aussi lancer un extendeur du dossier sur l'exécutable
        # (le premier mot est alors le programme à lancer).
        extra = bat_args(exe)
        script = bat_script(exe)
        forced = (args.profile_data.get(pkey) or {}).get("run")  # profil : commande à lancer (BAT écrit par un SETUP…)
        if forced:
            cmd.append(forced)
            res["bat"] = forced
            extra = []
        elif script and not (only and script[1] > 2) and (not extra or script[1] > 1):
            # (`only` : exécutable demandé précisément ; un .BAT qui enchaîne plus de deux programmes est une
            # collection (RUNIT.BAT d'ebobr.zip : 11 démos) : le lancer rejouerait toute la série à chaque fois.)
            # Le .BAT lance plus que l'exécutable (LOGO.EXE avant, réglages…) ou ses lignes échappent
            # à `bat_args` (%1, redirections) : on le fait exécuter par le shell intégré.
            cmd.append(script[0])
            res["bat"] = script[0]
            extra = []
        elif extra and os.path.splitext(extra[0].lower())[0] in EXTENDERS:
            cmd += extra
        else:
            cmd.append(os.path.basename(exe))
            cmd += extra
        if extra:
            res["args"] = extra
        # Profil de l'élément (clavier, options) : mémorisé, ou sondé (--probe).
        profile = args.profile_data.get(pkey)
        if profile is None and args.probe:
            best, scores = probe_keys(cmd, os.path.dirname(exe), shots, args)
            profile = {"keys": best, "probe": scores}
            save_profile(args.profiles, f"{rel}|{only}" if only else rel, profile)
        if profile:
            cmd = with_option(cmd, "--keys", profile.get("keys", args.keys))
            opts = profile.get("opts", [])
            if "--mips" in opts and "--mips" in cmd:
                i = cmd.index("--mips")  # le bridage du profil remplace celui par défaut
                del cmd[i:i + 2]
            if "--no-gus" in opts:  # pseudo-option : démo qui s'arrête dès qu'elle voit la GUS
                opts = [o for o in opts if o != "--no-gus"]
                if "--gus" in cmd:
                    cmd.remove("--gus")
            cmd[1:1] = opts
            res["profile"] = {k: v for k, v in profile.items() if k != "probe"}
        out = ""
        timed = False

        def execute(command):
            if args.full:
                out, timed, kept, killed = watch_run(command, os.path.dirname(exe), shots, args, args.limit + 90)
                res["kept"], res["idle_killed"] = kept, bool(killed)
                if os.environ.get("SURVEY_DEBUG"):
                    print(f"[execute] {' '.join(command[-6:])} kept={kept} killed={killed} dur={LAST.duration:.0f}", file=sys.stderr, flush=True)
                res["duration"] = round(LAST.duration)
                if LAST.loop:
                    res["loop"] = LAST.loop
                res.update(LAST.cpu)
                return out, timed
            try:
                p = subprocess.run(command, cwd=os.path.dirname(exe), stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                   errors="replace", timeout=args.limit + 90)
                return p.stdout + p.stderr, False
            except subprocess.TimeoutExpired as e:
                return ((e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")), True

        for attempt in range(3):
            out, timed = execute(cmd)
            if (res.get("kept") or 0) >= 2:
                break  # des scènes : pas de réessai (il effacerait ces captures)
            # Un gestionnaire de mémoire étendue présent est refusé (« KERNEL PANIC: EMM MANAGER DETECTED ») :
            # nouvel essai sans EMS.
            if not timed and re.search(r"emm manager|emm386|qemm|expanded memory manager|ems manager", out, re.I) and "--ems" not in cmd:
                cmd.insert(1, "--ems")
                cmd.insert(2, "0")
                res["ems0"] = True
                continue
            # « No DPMI host present » alors qu'un extendeur du dossier (DOS4GW.EXE…) y est :
            # on lance l'exécutable par lui (« dos4gw movie.exe », comme le dit le NOTE.TXT de
            # rpl_star), avant de se rabattre sur --dpmi.
            if attempt < 2 and re.search(r"no dpmi host|syntax is dos4gw", out, re.I) and not extra and "args" not in res:
                d = os.path.dirname(exe)
                ext = next((f for f in sorted(os.listdir(d)) if os.path.splitext(f.lower())[0] in EXTENDERS and f.lower().endswith(".exe")), None)
                if ext and ext.lower() != os.path.basename(exe).lower():
                    cmd = cmd[:-1] + [ext, os.path.basename(exe)]
                    res["args"] = [ext, os.path.basename(exe)]
                    continue
            # Hôte DPMI demandé (« DPMI? », « no DPMI ») : un second essai avec --dpmi.
            if attempt < 2 and re.search(r"dpmi", out, re.I) and "--dpmi" not in cmd:
                cmd.insert(1, "--dpmi")
                res["dpmi"] = True
                continue
            break
        # Carte GUS annoncée par défaut : certaines démos s'y accrochent (Hypnoman 2 : aucune image en 5 min) ; sans image ou avec
        # une seule, un second essai sans GUS (`--no-gus` d'un profil fait la même chose d'office).
        if args.full and (res.get("kept") or 0) <= 1 and "--gus" in cmd:
            nogus = [a for a in cmd if a != "--gus"]
            out2, timed2 = execute(nogus)
            res["no_gus_retry"] = True
            if (res.get("kept") or 0) >= 2:
                cmd, out, timed = nogus, out2, timed2
                res["no_gus"] = True
        # Extendeur exotique qui exige un hôte DPMI sans le dire (Zurenava 0.42 : « nicht.exe », rien d'affiché) : sans scène,
        # un dernier essai avec l'hôte DPMI de HX résident.
        if args.full and (res.get("kept") or 0) < 2 and "--dpmi" not in cmd and not res.get("dpmi"):
            withdpmi = cmd[:1] + ["--dpmi"] + cmd[1:]
            out3, timed3 = execute(withdpmi)
            res["dpmi_retry"] = True
            if (res.get("kept") or 0) >= 2:
                cmd, out, timed = withdpmi, out3, timed3
                res["dpmi"] = True
        # Rendu au DOS (ou plantage) sans paramètre, alors que le texte en annonce :
        # réessai avec un paramètre neutre, puis avec ceux que le texte propose
        # (Decapitation plante sur argv[1] nul sans paramètre).
        # (Seulement si le premier essai n'a rien donné : chaque réessai EFFACE les captures du précédent ; un programme qui rend la main
        # après de vraies scènes et affiche un texte de fin voyait ses captures détruites par un réessai à vide.)
        if (res.get("kept") or 0) < 2 and not extra and not timed and "time limit reached" not in out and (PARAM_HINT.search(out) or "SIGSEGV" in out):
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
        res["keys"] = res.get("profile", {}).get("keys", args.keys)
        if res["shots"]:
            sheet = os.path.join(args.out, "sheets", iid + ".png")
            files = [os.path.join(shots, f) for f in res["shots"] if f != "f.png"] or [os.path.join(shots, "f.png")]
            if args.full:
                # Scènes distinctes : planches de 6 images (3 × 2, 320×200), `ID.png`
                # (la première, pour la revue) puis `ID.2.png`, `ID.3.png`…
                cap = frame_cap(args, getattr(LAST, "duration", 0), res.get("loop"), len(files))
                res["cap"] = cap
                if len(files) > cap:
                    # Trop de scènes : on en retient `cap` réparties sur toute la
                    # durée (et non les premières : la fin de la démo compte autant).
                    chosen = sorted({i * len(files) // cap for i in range(cap)})
                    for k, f in enumerate(files):
                        if k not in chosen:
                            os.remove(f)
                    files = [files[k] for k in chosen]
                for n in range(0, len(files), 6):
                    name = sheet if n == 0 else os.path.join(args.out, "sheets", f"{iid}.{n // 6 + 1}.png")
                    subprocess.run(["montage", *files[n:n + 6], "-geometry", "320x200+2+2", "-tile", "3x", name],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
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
    ap.add_argument("--full", action="store_true", help="durée complète : captures denses (--every), seules les scènes distinctes sont gardées, arrêt si l'écran ne change plus (--idle)")
    ap.add_argument("--change", type=float, default=5.0, help="--full : écart moyen (0-255, vignettes 32×20) pour compter comme une nouvelle scène (défaut 5)")
    ap.add_argument("--idle", type=int, default=60, help="--full : secondes sans nouvelle scène avant d'arrêter le programme (défaut 60, 0 : jamais)")
    ap.add_argument("--max-frames", type=int, default=0, help="--full : plafond FIXE de scènes par programme (défaut 0 : plafond adaptatif, voir --min-frames)")
    ap.add_argument("--min-frames", type=int, default=48, help="--full : plafond d'une démo courte ou qui boucle (défaut 48, soit 8 planches)")
    ap.add_argument("--seconds-per-frame", type=float, default=10, help="--full : au-delà, une scène de plus par tranche de SECONDES de démo (défaut 10)")
    ap.add_argument("--dense-seconds", type=float, default=5, help="--full : une démo qui donne une scène distincte toutes les SECONDES ou moins garde toutes ses scènes, jusqu'à --frames-cap (défaut 5)")
    ap.add_argument("--frames-cap", type=int, default=300, help="--full : plafond absolu du plafond adaptatif (défaut 300, soit 50 planches)")
    ap.add_argument("--hard-limit", type=int, default=1800, help="--full : durée maximale (secondes) d'un programme qui produit encore de nouvelles scènes après --limit (défaut 1800)")
    ap.add_argument("--no-extend", dest="extend", action="store_false", help="--full : ne pas prolonger au-delà de --limit")
    ap.add_argument("--loop-scenes", type=int, default=4, help="--full : une boucle est reconnue quand les N dernières scènes recommencent d'anciennes scènes (défaut 4)")
    ap.add_argument("--loop-eps", type=float, default=2.5, help="--full : écart moyen (0-255) en dessous duquel deux scènes sont « la même » pour la boucle (défaut 2,5)")
    ap.add_argument("--no-loop-stop", dest="loop_stop", action="store_false", help="--full : ne pas arrêter un programme qui boucle")
    ap.add_argument("--probe", type=int, default=0, help="sonde les scripts de touches (KEY_CANDIDATES), SECONDES chacun, pour les éléments sans profil ; le meilleur est mémorisé dans --profiles")
    ap.add_argument("--profiles", help="fichier JSON des profils par élément (clavier `keys`, options `opts`, notes) : lu, et complété par --probe")
    ap.add_argument("--keys", default=DEFAULT_KEYS, help="script de touches (défaut : aucune touche, entrée gardée ouverte)")
    ap.add_argument("--no-gus", dest="gus", action="store_false", help="sans Gravis UltraSound")
    ap.add_argument("--win32", action="store_true", help="lancer aussi les programmes Windows (par HX)")
    ap.add_argument("--dll-pack", help="dossier de DLL ajoutées à côté des programmes Windows qui ne les apportent pas (MSVCRT, MSVCP50, GLU32…)")
    ap.add_argument("--filter", help="expression régulière sur le chemin relatif des éléments")
    ap.add_argument("--items", help="fichier : un chemin relatif d'élément par ligne (comme `item` dans results.jsonl)")
    ap.add_argument("--exe-list", help="fichier : « chemin relatif de l'élément<TAB>exécutable dans l'élément » par ligne ; lance CET exécutable (un résultat par ligne, identifiant « élément__exécutable ») au lieu de l'exécutable principal")
    ap.add_argument("--max", type=int, help="au plus N éléments (essais)")
    ap.add_argument("--redo", action="store_true", help="refaire les éléments déjà traités")
    ap.add_argument("--retry-static", metavar="KEYS", help="relancer seulement les éléments figés (dernier résultat), avec ce script de touches")
    ap.add_argument("--keep-work", action="store_true", help="garder les dossiers de travail")
    ap.add_argument("--per-page", type=int, default=12, help="éléments par planche de revue")
    ap.add_argument("--review-only", action="store_true", help="refaire seulement les planches de revue")
    args = ap.parse_args()
    if args.probe and not args.profiles:
        ap.error("--probe demande --profiles")
    args.profile_data = load_profiles(args.profiles) if args.profiles else {}
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
    all_items = items
    if args.filter:
        items = [i for i in items if re.search(args.filter, os.path.relpath(i, src), re.I)]
    if args.items:
        wanted = {l.strip() for l in open(args.items) if l.strip()}
        items = [i for i in items if os.path.relpath(i, src) in wanted]
    if args.retry_static:
        args.keys = args.retry_static
        items = [i for i in items if last.get(ident(os.path.relpath(i, src)), {}).get("static")]
    elif not args.redo:
        items = [i for i in items if ident(os.path.relpath(i, src)) not in last]
    done = last
    pairs = [(i, None) for i in items]
    if args.exe_list:
        # un exécutable précis par ligne : on ignore le choix de l'exécutable principal
        byrel = {os.path.relpath(i, src): i for i in all_items}
        pairs = []
        for l in open(args.exe_list):
            if "\t" not in l:
                continue
            r, e = l.rstrip("\n").split("\t", 1)
            if r in byrel and (args.redo or (ident(r) + "__" + ident(e)) not in last):
                pairs.append((byrel[r], e))
    if args.max:
        pairs = pairs[:args.max]
    items = pairs
    print(f"{len(items)} éléments à traiter ({len(done)} déjà faits)", flush=True)
    lock = threading.Lock()
    count = [0]

    def job(pair):
        r = run_one(pair[0], args, src, pair[1])
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
