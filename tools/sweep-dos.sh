#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Balayage DOS des exécutables jamais lancés (liste `exe-list-dos.tsv`) : mêmes réglages que scan-win32b/c, binaire figé.
# Usage : tools/sweep-dos.sh NOM_DU_DOSSIER_DE_RÉSULTATS   (ex. scan-dos2, créé dans « CD PC Team » ; reprend où il s'est arrêté)
set -e
cd "$(dirname "$0")/.."
PC="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
OUT=${1:?dossier de résultats}
cp target/release/bare86 "/tmp/claude-1000/bare86-sweep-$OUT"
mkdir -p "$PC/$OUT"
exec systemd-inhibit --what=sleep:idle python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/$OUT" --exe-list "$PC/exe-list-dos.tsv" --full --limit 600 --every 2 --idle 60 --jobs 4 --probe 25 --profiles "$PC/profiles.json" --bare86 "/tmp/claude-1000/bare86-sweep-$OUT"
