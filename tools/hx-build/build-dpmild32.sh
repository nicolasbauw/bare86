#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Construit DPMILD32.EXE (le chargeur PE de HX DOS Extender) sous Linux, depuis les sources de HX,
# et le compare au binaire installé par Bare86. Validé le 2026-11-25 : résultat IDENTIQUE à celui de la 2.23.
# Outils : JWasm et JWlink (Japheth), compilés ici depuis leurs sources (gcc, make).
# Usage : build-dpmild32.sh [DOSSIER_DE_TRAVAIL]   (défaut : /tmp/hx-build)
set -e
W=${1:-/tmp/hx-build}
mkdir -p "$W" && cd "$W"
[ -d HX ] || git clone -q --depth 300 https://github.com/Baron-von-Riedesel/HX.git
[ -d jwasm ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/JWasm.git jwasm
[ -d jwlink ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/jwlink.git jwlink
[ -x jwasm/build/GccUnixR/jwasm ] || (cd jwasm && make -f GccUnix.mak -j8 >/dev/null)
[ -x jwlink/build/jwlinkLR/jwlink ] || (cd jwlink && make -f GccUnix.mak -j8 >/dev/null)
JW=$W/jwasm/build/GccUnixR/jwasm
LK=$W/jwlink/build/jwlinkLR/jwlink
# JWasm cherche les fichiers inclus avec la casse du source : tout en minuscules, dans une copie.
rm -rf b && mkdir -p b/ldr/out b/inc
for f in HX/Src/DPMILDR/*; do [ -f "$f" ] && cp "$f" "b/ldr/$(basename "$f" | tr A-Z a-z)"; done
for f in HX/Include/*; do [ -f "$f" ] && cp "$f" "b/inc/$(basename "$f" | tr A-Z a-z)"; done
cd b/ldr
for m in dpmildr kernel16 peload; do
    $JW -c -nologo -Sg -Fl=out/$m -Fo=out/$m.obj -D?32BIT=1 -D?DEBUG=0 -I../inc $m.asm
done
$LK format dos file "out/dpmildr.obj,out/kernel16.obj,out/peload.obj" name out/DPMILD32.EXE op q, map=out/DPMILD32.MAP
ls -l out/DPMILD32.EXE
REF=${HX_DPMILD32:-$HOME/.local/share/bare86/HX/BIN/DPMILD32.EXE}
if [ -f "$REF" ]; then cmp out/DPMILD32.EXE "$REF" && echo "identique à $REF"; fi
