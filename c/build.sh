#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile les programmes C de test (c/tests/*.c) avec Open Watcom v2 : en .COM
# (modèle tiny) si le source contient « cible : com », sinon en .EXE (small).
# WATCOM : racine de la release (par défaut ~/Dev/open-watcom-v2/rel, construite
# depuis les sources). Les .COM produits sont versionnés : les tests Rust n'ont
# pas besoin du compilateur.
set -e
WATCOM=${WATCOM:-$HOME/Dev/open-watcom-v2/rel}
export WATCOM INCLUDE="$WATCOM/h" PATH="$WATCOM/binl64:$PATH"
cd "$(dirname "$0")/tests"
for src in *.c; do
    if grep -q "cible : com" "$src"; then
        wcl -q -bt=dos -mt -fe="${src%.c}.com" "$src" -"l=com"
    else
        wcl -q -bt=dos -ms -fe="${src%.c}.exe" "$src"
    fi
done
rm -f ./*.o
# Programmes Win32 (tests/hx_*.rs, lancés sous HX) : c/tests/win32/*.c -> .exe
export INCLUDE="$WATCOM/h:$WATCOM/h/nt"
cd win32
for src in *.c; do
    # sous-système : « cible : console » dans le source, sinon fenêtre (nt_win)
    if grep -q "wcl386 -bt=nt -l=nt)" "$src"; then sub=nt; else sub=nt_win; fi
    wcl386 -q -bt=nt -l=$sub -fe="${src%.c}.exe" "$src" winmm.lib
done
rm -f ./*.o
