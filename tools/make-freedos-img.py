#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Construit une image disque FreeDOS 1.4 prête à l'emploi pour Bare86, avec
Open Watcom C/C++ et NASM.

  python3 -I tools/make-freedos-img.py --out DOSSIER [--size 512] [--sources]

Étapes, toutes reproductibles :
1. téléchargement de la Floppy Edition officielle de FreeDOS 1.4 et des
   paquets WATCOMC et NASM du dépôt FreeDOS 1.4 (ibiblio), empreintes
   SHA-256 vérifiées ;
2. installation de FreeDOS par son installateur officiel, dans Bare86, piloté
   au clavier (partitionnement, formatage, six disquettes) ;
3. installation des paquets par FreeDOS lui-même (FDINST), environnement
   d'Open Watcom appelé depuis FDAUTO.BAT ;
4. exemples HELLO.C et HELLO.ASM dans C:\\EXAMPLES ;
5. vérification : FreeDOS démarre, compile et exécute les deux exemples.

Résultat : `fd14-bare86.img` (image de disque dur, partition FAT16), son
empreinte et `NOTICE.txt` (paquets installés, versions, licences). Avec
`--sources`, les paquets correspondants (qui contiennent leurs sources) sont
réunis dans `sources/` : ceux de la Legacy CD de la même sortie 1.4, plus
WATCOMC et NASM, pour les joindre à une diffusion (GPL).

Utilisation : `bare86 --boot --hda fd14-bare86.img`.

Demande : Bare86 compilé, mtools, unzip, curl.
"""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import urllib.request

REPO = "https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files"
FLOPPY = ("distributions/1.4/FD14-FloppyEdition.zip",
          "45b1fa7c52dd996c3bfa5e352ffcd410781b952a6ad629f15a4c9ec4bbaefc5a")  # verify.txt officiel
# Paquets du dépôt 1.4 : empreintes relevées au téléchargement (2026-11-21).
# Legacy CD de la même sortie : paquets de la 1.4 avec leurs sources.
LEGACY = ("distributions/1.4/FD14-LegacyCD.zip",
          "715b01754b8f5fd24cceb32d023f46aa9456c582c594d7d5fbe34787e1c773d4")  # verify.txt officiel
PACKAGES = [
    ("repositories/1.4/devel/watcomc.zip", "61d26b855c071c9f674b8789eb548983f275fff89b3f6f1190cfd1ff28ab42f6"),
    ("repositories/1.4/devel/nasm.zip", "f3497ec94f76b508a74904926a479cf33d75f6da332c432dfa72d5aaa2903cbe"),
]
PART_OFFSET = 63 * 512  # première partition, créée par l'installateur

HELLO_C = "#include <stdio.h>\r\n\r\nint main(void)\r\n{\r\n    printf(\"Hello from Open Watcom\\n\");\r\n    return 0;\r\n}\r\n"
HELLO_ASM = ("; NASM : nasm -f bin HELLO.ASM -o HELLO.COM\r\n        org 100h\r\n        mov dx, msg\r\n"
             "        mov ah, 9\r\n        int 21h\r\n        ret\r\nmsg     db \"Hello from NASM\", 13, 10, \"$\"\r\n")
README = ("Open Watcom C/C++ (C:\\DEVEL\\WATCOMC) and NASM (C:\\DEVEL\\NASM), installed\r\n"
          "from the FreeDOS 1.4 repository. FDAUTO.BAT calls OWSETENV.BAT at boot.\r\n\r\n"
          "  wcl -bt=dos HELLO.C              16-bit DOS program (HELLO.EXE)\r\n"
          "  wcl386 -bt=dos HELLO.C           32-bit DOS/4GW program\r\n"
          "  nasm -f bin HELLO.ASM -o HELLO.COM\r\n")


def log(msg):
    print(msg, flush=True)


def fetch(cache, rel, sha):
    path = os.path.join(cache, os.path.basename(rel))
    if not os.path.exists(path):
        log(f"téléchargement de {rel}")
        tmp = path + ".part"
        with urllib.request.urlopen(f"{REPO}/{rel}", timeout=600) as r, open(tmp, "wb") as f:
            shutil.copyfileobj(r, f)
        os.replace(tmp, path)
    h = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if h != sha:
        sys.exit(f"{path}: SHA-256 {h}, attendu {sha}")
    return path


def bare86(args, opts, keys, limit, screen=False):
    cmd = [opts.bare86, "--no-realtime", "--limit", str(limit), "--keys", keys, *args]
    if screen:
        cmd.append("--screen")
    r = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    return r.stdout + r.stderr


def mtools(*cmd):
    subprocess.run(list(cmd), check=True, capture_output=True)


def img(path):
    return f"{path}@@{PART_OFFSET}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="dossier de travail et de sortie")
    ap.add_argument("--size", type=int, default=512, help="taille de l'image en Mo (FAT16 : 2047 au plus ; défaut 512)")
    ap.add_argument("--bare86", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "bare86"))
    ap.add_argument("--sources", action="store_true", help="télécharger les paquets sources des paquets installés")
    opts = ap.parse_args()
    out = os.path.abspath(opts.out)
    cache = os.path.join(out, "dl")
    work = os.path.join(out, "work")
    os.makedirs(cache, exist_ok=True)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)

    floppy = fetch(cache, *FLOPPY)
    pkgs = [fetch(cache, *p) for p in PACKAGES]
    subprocess.run(["unzip", "-qq", "-o", floppy, "144m/*", "-d", work], check=True)
    disks = os.path.join(work, "144m")
    hd = os.path.join(work, "fd14-bare86.img")
    with open(hd, "wb") as f:
        f.truncate(opts.size * 1024 * 1024)

    # 2. Installateur officiel, piloté au clavier.
    keys = ("{until:proceed [Y,N]}y{enter}{until:reboot now [Y,N]}y{enter}{wait:6000}"
            "{until:proceed [Y,N]}y{enter}{until:format your drive [Y,N]}y{enter}"
            "{until:install FreeDOS 1.4.}{wait:500}y{enter}"
            f"{{until:Insert diskette #2}}{{fda:{disks}/x86DSK01.img}}{{wait:500}}{{enter}}")
    for part, disk in (("024", 2), ("048", 3), ("072", 4), ("096", 5), ("120", 6)):
        keys += f"{{until:FREEDOS.{part}}}{{fda:{disks}/x86DSK0{disk}.img}}{{wait:500}}{{enter}}"
    keys += "{until:has completed}"
    log("installation de FreeDOS 1.4 (installateur officiel, quelques minutes)")
    shutil.copy(os.path.join(disks, "x86BOOT.img"), os.path.join(work, "boot.img"))
    t = bare86(["--boot", "--fda", os.path.join(work, "boot.img"), "--hda", hd], opts, keys, 1800)
    if "end of input" not in t:
        sys.exit("installation : fin non atteinte\n" + t[-2000:])
    listing = subprocess.run(["mdir", "-i", img(hd), "-b", "::/FREEDOS/BIN"], capture_output=True, text=True).stdout
    if "FDINST.EXE" not in listing.upper():
        sys.exit("installation : C:\\FREEDOS\\BIN incomplet")

    # 3. Paquets, installés par FreeDOS.
    log("installation d'Open Watcom et de NASM (FDINST)")
    mtools("mmd", "-i", img(hd), "::/PKGS")
    keys = "{until:C:\\>}"
    for p in pkgs:
        mtools("mcopy", "-i", img(hd), p, "::/PKGS/")
        keys += f"fdinst install C:\\PKGS\\{os.path.basename(p).upper()}{{enter}}{{wait:2000}}{{until:C:\\>}}"
    bare86(["--boot", "--hda", hd], opts, keys, 900)
    mtools("mdeltree", "-i", img(hd), "::/PKGS")
    for d in ("::/DEVEL/WATCOMC/BINW/WCL.EXE", "::/DEVEL/NASM/NASM.EXE"):
        if subprocess.run(["mdir", "-i", img(hd), d], capture_output=True).returncode:
            sys.exit(f"paquets : {d} absent")

    # Environnement d'Open Watcom au démarrage.
    auto = os.path.join(work, "fdauto.bat")
    mtools("mcopy", "-o", "-i", img(hd), "::/FDAUTO.BAT", auto)
    s = open(auto, "rb").read().decode("ascii")
    anchor = "REM - additional customization and local settings\r\n"
    if anchor not in s:
        sys.exit("FDAUTO.BAT : point d'insertion introuvable")
    s = s.replace(anchor, anchor + "\r\nREM - Open Watcom C/C++ and NASM (C:\\DEVEL), installed with the Bare86 image\r\n"
                  "if exist C:\\DEVEL\\WATCOMC\\OWSETENV.BAT call C:\\DEVEL\\WATCOMC\\OWSETENV.BAT\r\n")
    open(auto, "wb").write(s.encode("ascii"))
    mtools("mcopy", "-o", "-i", img(hd), auto, "::/FDAUTO.BAT")

    # 4. Exemples.
    mtools("mmd", "-i", img(hd), "::/EXAMPLES")
    for name, text in (("HELLO.C", HELLO_C), ("HELLO.ASM", HELLO_ASM), ("README.TXT", README)):
        p = os.path.join(work, name)
        open(p, "wb").write(text.encode("ascii"))
        mtools("mcopy", "-o", "-i", img(hd), p, f"::/EXAMPLES/{name}")

    # 5. Vérification, dans une copie (l'image livrée reste sans fichiers compilés).
    log("vérification : démarrage, compilation et exécution des exemples")
    check = os.path.join(work, "check.img")
    shutil.copy(hd, check)
    keys = ("{until:C:\\>}cd \\EXAMPLES{enter}{wait:500}wcl -bt=dos -q HELLO.C{enter}{wait:2000}"
            "{until:C:\\EXAMPLES>}HELLO{enter}{wait:1000}nasm -f bin HELLO.ASM -o HELLO.COM{enter}{wait:2000}"
            "{until:C:\\EXAMPLES>}HELLO.COM{enter}{wait:2000}{wait:900000}")
    t = bare86(["--boot", "--hda", check], opts, keys, 120, screen=True)
    os.remove(check)
    for want in ("Hello from Open Watcom", "Hello from NASM"):
        if want not in t:
            sys.exit(f"vérification : « {want} » absent\n" + t[-1500:])

    final = os.path.join(out, "fd14-bare86.img")
    os.replace(hd, final)
    digest = hashlib.sha256(open(final, "rb").read()).hexdigest()
    open(final + ".sha256", "w").write(f"{digest}  fd14-bare86.img\n")
    log(f"{final} ({opts.size} Mo), SHA-256 {digest}")

    # Notice : paquets installés, version et licence lues dans leurs fiches LSM.
    lsmdir = os.path.join(work, "lsm")
    os.makedirs(lsmdir, exist_ok=True)
    subprocess.run(["mcopy", "-i", img(final), "::/FREEDOS/APPINFO/*.LSM", lsmdir], capture_output=True)
    pkgs_info = []
    for f in sorted(os.listdir(lsmdir)):
        t = open(os.path.join(lsmdir, f), "rb").read().decode("latin-1")
        field = lambda k: next((l.split(":", 1)[1].strip() for l in t.splitlines() if l.lower().startswith(k.lower() + ":")), "?")
        lic = field("Copying-policy")
        pkgs_info.append((f[:-4].lower(), field("Version"),
                          lic if lic != "?" else "(not stated in APPINFO; see the package documentation)"))
    notice = ["FreeDOS 1.4 disk image for Bare86 (fd14-bare86.img)", "",
              "Built by tools/make-freedos-img.py from the official FreeDOS 1.4 Floppy Edition",
              "and the WATCOMC and NASM packages of the FreeDOS 1.4 repository",
              f"({REPO}).", "Only FDAUTO.BAT was changed (it calls C:\\DEVEL\\WATCOMC\\OWSETENV.BAT);",
              "C:\\EXAMPLES was added.", "",
              "Installed packages (name, version, license as stated in its APPINFO file):", ""]
    notice += [f"  {n:10} {v:28} {c}" for n, v, c in pkgs_info]
    notice += ["", "The source code of these packages is in the FreeDOS 1.4 packages, which",
               "contain it (SOURCE directory): see the sources archive distributed with this",
               "image, or the FreeDOS 1.4 Legacy CD (FD14-LegacyCD.zip) on the repository above.", ""]
    open(os.path.join(out, "NOTICE.txt"), "w").write("\n".join(notice))
    log(f"{len(pkgs_info)} paquets installés ; notice dans {os.path.join(out, 'NOTICE.txt')}")

    if opts.sources:
        # Sources : les paquets de la sortie 1.4 elle-même (Legacy CD, mêmes
        # versions que la Floppy Edition ; le dépôt vivant a évolué depuis),
        # plus les paquets WATCOMC et NASM installés.
        src = os.path.join(out, "sources")
        shutil.rmtree(src, ignore_errors=True)
        os.makedirs(src)
        legacy = fetch(cache, *LEGACY)
        iso_dir = os.path.join(work, "legacy")
        subprocess.run(["unzip", "-qq", "-o", legacy, "FD14LGCY.iso", "-d", iso_dir], check=True)
        iso = os.path.join(iso_dir, "FD14LGCY.iso")
        listing = subprocess.run(["7z", "l", "-slt", iso], capture_output=True, text=True).stdout
        on_cd = {os.path.basename(p)[:-4].lower(): p for p in re.findall(r"^Path = (packages/\S+\.zip)$", listing, re.M | re.I)}
        mine = {os.path.basename(p)[:-4].lower(): p for p in pkgs}
        missing = []
        for name, _, _ in pkgs_info:
            if name in mine:
                shutil.copy(mine[name], os.path.join(src, name + ".zip"))
            elif name in on_cd:
                subprocess.run(["7z", "e", "-y", "-bso0", "-bsp0", iso, on_cd[name], "-o" + src], check=True)
            else:
                missing.append(name)
        os.remove(iso)
        log(f"sources : {len(pkgs_info) - len(missing)} paquets dans {src}")
        if missing:
            log("sans paquet source trouvé : " + ", ".join(missing))
    return 0


if __name__ == "__main__":
    sys.exit(main())
