#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile les programmes C de test (c/tests/*.c) en .COM avec Open Watcom v2.
# WATCOM : racine de la release (par défaut ~/Dev/open-watcom-v2/rel, construite
# depuis les sources). Les .COM produits sont versionnés : les tests Rust n'ont
# pas besoin du compilateur.
set -e
WATCOM=${WATCOM:-$HOME/Dev/open-watcom-v2/rel}
export WATCOM INCLUDE="$WATCOM/h" PATH="$WATCOM/binl64:$PATH"
cd "$(dirname "$0")/tests"
for src in *.c; do
    wcl -q -bt=dos -mt -fe="${src%.c}.com" "$src" -"l=com"
done
rm -f ./*.o
