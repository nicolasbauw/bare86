#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare une session de Bare86 avec une vidéo de référence (démo, jeu).

Bare86 enregistre l'écran (`--video ours.mkv`, 70 i/s, 640×480) ; la
référence est une vidéo quelconque (capture d'un vrai PC, de DOSBox, vidéo
récupérée par yt-dlp…).

  tools/compare.py --ref ref.mkv --video ours.mkv --out DIR

1. Calage par l'image : chaque fenêtre de `--window` s de notre vidéo est
   retrouvée dans la référence (corrélation de vignettes 8×6 en niveaux de
   gris, une par seconde, puis quatre par seconde autour ; les vignettes
   noires ou unies ne comptent pas). Le son ne convient pas : la musique d'un
   module répète ses motifs. Le décalage de chaque fenêtre dit où en est la
   démo chez nous par rapport à la référence ; s'il change, la démo va plus
   vite ou plus lentement chez nous, ou un passage manque.
2. Fluidité : images distinctes par seconde de part et d'autre (écart moyen
   entre images successives, filtres `tblend` et `signalstats` de ffmpeg,
   au-dessus de `--threshold`, réglé au-dessus du bruit de compression), par
   fenêtre de `--fps-window` s. La référence ne peut dépasser sa propre
   cadence (souvent 50 ou 60 i/s ; la VGA en donne 70).
3. Planches : nos images et celles de la référence aux mêmes instants de la
   démo, toutes les `--step` s (DIR/sheet-NN.png), et un rapport
   (DIR/report.md).

Dépendances : ffmpeg, ffprobe, ImageMagick (montage). Python seul, sans numpy.
"""

import argparse
import math
import os
import re
import subprocess
import sys

TW, TH = 8, 6  # vignettes de calage
FLAT = 6.0  # écart-type (sur 255) en dessous duquel une vignette est unie


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, **kw)


def duration(path):
    out = run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True).stdout
    return float(out.strip())


def thumbs(path, fps):
    """Vignettes centrées et réduites (`None` : unie), `fps` par seconde."""
    raw = run(["ffmpeg", "-v", "error", "-i", path, "-an", "-vf", f"fps={fps},scale={TW}:{TH}:flags=area,format=gray", "-f", "rawvideo", "-"], capture_output=True).stdout
    n = TW * TH
    out = []
    for i in range(0, len(raw) - n + 1, n):
        v = raw[i : i + n]
        m = sum(v) / n
        sd = math.sqrt(sum((x - m) ** 2 for x in v) / n)
        out.append(None if sd < FLAT else [(x - m) / sd for x in v])
    return out


def window_score(seg, ref, pos):
    """Corrélation moyenne des vignettes non unies de `seg` posé en `pos`."""
    tot, cnt = 0.0, 0
    n = TW * TH
    for i, a in enumerate(seg):
        j = pos + i
        if a is None or j < 0 or j >= len(ref) or ref[j] is None:
            continue
        b = ref[j]
        tot += sum(x * y for x, y in zip(a, b)) / n
        cnt += 1
    return (tot / cnt if cnt else -2.0), cnt


def locate(seg, ref, lo, hi, min_count):
    best = (-2.0, None)
    for pos in range(lo, hi + 1):
        s, c = window_score(seg, ref, pos)
        if c >= min_count and s > best[0]:
            best = (s, pos)
    return best


def frame_changes(path):
    """Instant et écart moyen (0–255) de chaque image avec la précédente."""
    p = run(
        ["ffmpeg", "-v", "error", "-i", path, "-an", "-vf", "scale=160:120,format=gray,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-", "-f", "null", "-"],
        capture_output=True,
        text=True,
    ).stdout
    out = []
    t = None
    for line in p.splitlines():
        m = re.search(r"pts_time:([0-9.]+)", line)
        if m:
            t = float(m.group(1))
        m = re.search(r"YAVG=([0-9.]+)", line)
        if m and t is not None:
            out.append((t, float(m.group(1))))
    return out


def rate_at(changes, t0, t1, threshold):
    n = sum(1 for t, d in changes if t0 <= t < t1 and d > threshold)
    return n / (t1 - t0) if t1 > t0 else 0.0


def frame(path, t, out, w, h):
    run(["ffmpeg", "-v", "error", "-ss", f"{max(0.0, t):.3f}", "-i", path, "-frames:v", "1", "-vf", f"scale={w}:{h}:flags=neighbor", "-y", out])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ref", required=True, help="vidéo de référence")
    ap.add_argument("--video", required=True, help="notre vidéo (--video de Bare86)")
    ap.add_argument("--out", required=True, help="répertoire du rapport")
    ap.add_argument("--step", type=float, default=20.0, help="écart des images comparées (s)")
    ap.add_argument("--window", type=int, default=20, help="fenêtre du calage (s)")
    ap.add_argument("--fps-window", type=float, default=5.0, help="fenêtre du décompte d'images (s)")
    ap.add_argument("--threshold", type=float, default=0.3, help="écart moyen (0–255) d'une image distincte")
    ap.add_argument("--rows", type=int, default=6, help="lignes par planche")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)

    print("vignettes…", file=sys.stderr)
    ours1, ref1 = thumbs(a.video, 1), thumbs(a.ref, 1)
    ours4, ref4 = thumbs(a.video, 4), thumbs(a.ref, 4)
    local = []  # (milieu de la fenêtre chez nous, décalage ref - nous, score)
    w = a.window
    for s0 in range(0, max(1, len(ours1) - w // 2), w // 2):
        seg = ours1[s0 : s0 + w]
        lit = sum(1 for x in seg if x is not None)
        if lit < 3:
            continue
        need = max(3, lit // 2)
        score, pos = locate(seg, ref1, -w // 2, len(ref1) - w // 2, need)
        if pos is None:
            continue
        seg4 = ours4[s0 * 4 : (s0 + w) * 4]
        score4, pos4 = locate(seg4, ref4, pos * 4 - 6, pos * 4 + 6, need * 4)
        if pos4 is not None:
            score, pos = score4, pos4 / 4
        local.append((s0 + len(seg) / 2, pos - s0, score))
    good = [x for x in local if x[2] > 0.6]
    offs = sorted(o for _, o, _ in good)
    offset = offs[len(offs) // 2] if offs else 0.0
    print(f"décalage médian : référence = Bare86 + {offset:.2f} s", file=sys.stderr)

    def offset_at(t):
        if not good:
            return offset
        before = [x for x in good if x[0] <= t]
        after = [x for x in good if x[0] > t]
        if before and after:
            (t0, o0, _), (t1, o1, _) = before[-1], after[0]
            return o0 + (o1 - o0) * (t - t0) / (t1 - t0)
        return (before[-1] if before else after[0])[1]

    print("images distinctes…", file=sys.stderr)
    ours_c, ref_c = frame_changes(a.video), frame_changes(a.ref)
    dur = duration(a.video)

    lines = [
        "# Comparaison avec la référence",
        "",
        f"- Référence : `{a.ref}`",
        f"- Bare86 : `{a.video}`",
        f"- Décalage médian : référence = Bare86 + {offset:.2f} s (fenêtres de {a.window} s retrouvées par l'image, score > 0,6)",
        "",
        "## Calage (dérive)",
        "",
        "| Bare86 (s) | décalage (s) | score |",
        "|---:|---:|---:|",
    ]
    lines += [f"| {t:.0f} | {o:+.2f} | {c:.2f} |" for t, o, c in local]
    lines += ["", f"## Images distinctes par seconde (fenêtres de {a.fps_window:.0f} s, seuil {a.threshold})", "", "| Bare86 (s) | Bare86 | référence |", "|---:|---:|---:|"]
    t = 0.0
    while t + a.fps_window <= dur:
        o = offset_at(t)
        lines.append(f"| {t:.0f} | {rate_at(ours_c, t, t + a.fps_window, a.threshold):.1f} | {rate_at(ref_c, t + o, t + o + a.fps_window, a.threshold):.1f} |")
        t += a.fps_window

    print("planches…", file=sys.stderr)
    tmp = os.path.join(a.out, "frames")
    os.makedirs(tmp, exist_ok=True)
    pairs = []
    t = a.step / 2
    while t < dur:
        o = offset_at(t)
        po, pr = os.path.join(tmp, f"o{t:07.1f}.png"), os.path.join(tmp, f"r{t:07.1f}.png")
        frame(a.video, t, po, 320, 240)
        frame(a.ref, t + o, pr, 320, 240)
        fo = rate_at(ours_c, t - 2.5, t + 2.5, a.threshold)
        fr = rate_at(ref_c, t + o - 2.5, t + o + 2.5, a.threshold)
        pairs.append((po, f"Bare86 {t:.0f} s, {fo:.0f} i/s"))
        pairs.append((pr, f"ref. {t + o:.0f} s, {fr:.0f} i/s"))
        t += a.step
    sheets = []
    per = a.rows * 2
    for n, i in enumerate(range(0, len(pairs), per)):
        out = os.path.join(a.out, f"sheet-{n:02d}.png")
        cmd = ["montage", "-geometry", "+4+4", "-tile", "2x", "-pointsize", "14"]
        for p, label in pairs[i : i + per]:
            cmd += ["-label", label, p]
        run(cmd + [out])
        sheets.append(out)
    lines += ["", "## Planches", ""] + [f"![{os.path.basename(s)}]({os.path.basename(s)})" for s in sheets]
    with open(os.path.join(a.out, "report.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(l for l in lines if not l.startswith("![")))
    print(f"rapport : {os.path.join(a.out, 'report.md')}")


if __name__ == "__main__":
    main()
