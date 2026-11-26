#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Balayage des 264 éléments Win32 de la collection CD PC Team (sous HX) : même procédure que scan-win32b (2026-11-25).
# Usage : tools/sweep-win32.sh NOM_DU_DOSSIER_DE_RÉSULTATS   (ex. scan-win32c, créé dans « CD PC Team »)
# Pré-requis : HX installé par Bare86 (fichiers du fork), `CD PC Team/win-all.txt` (liste des éléments), `runtime-pack/`.
# Le binaire est copié (figé) pour que le balayage ne change pas si on recompile pendant qu'il tourne ; il reprend là où il s'est arrêté.
set -e
cd "$(dirname "$0")/.."
PC="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
OUT=${1:?dossier de résultats}
cp target/release/bare86 "/tmp/claude-1000/bare86-sweep-$OUT"
mkdir -p "$PC/$OUT"
exec systemd-inhibit --what=sleep:idle python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/$OUT" --items "$PC/win-all.txt" --win32 --dll-pack "$PC/runtime-pack" --bare86 "/tmp/claude-1000/bare86-sweep-$OUT" --full --limit 600 --every 2 --idle 60 --jobs 4 --probe 25 --profiles "$PC/profiles.json"
