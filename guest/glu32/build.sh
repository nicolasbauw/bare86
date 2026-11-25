#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile la glu32.dll de Bare86 (DLL PE i386 pour les programmes Win32 sous HX, sans bibliothèque C) avec clang et
# lld. La DLL compilée est versionnée : cette étape ne sert qu'à qui modifie les sources. Produit glu32.def à partir
# des lignes « GLUAPI » de glu32.c, et des bibliothèques d'import (opengl32.lib, kernel32.lib) dans un dossier temporaire.
set -e
cd "$(dirname "$0")"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
python3 -I - <<'PY'
import re
out = ["; Fichier produit par build.sh : ne pas modifier.", "LIBRARY glu32", "EXPORTS"]
for line in open("glu32.c"):
    m = re.match(r"GLUAPI\s+[\w\s\*]+?\s*\**(\w+)\((.*)\)\s*(\{.*)?$", line.strip())
    if not m:
        continue
    name, params = m.group(1), m.group(2).strip()
    size = 0
    if params != "void":
        depth, cur, parts = 0, "", []
        for ch in params:
            if ch == "(": depth += 1
            if ch == ")": depth -= 1
            if ch == "," and depth == 0:
                parts.append(cur); cur = ""
            else:
                cur += ch
        parts.append(cur)
        for p in parts:
            size += 8 if ("GLdouble" in p and "*" not in p and "[" not in p) else 4
    out.append(f"    {name}=_{name}@{size}")
open("glu32.def", "w").write("\n".join(out) + "\n")
PY
# Bibliothèques d'import : les noms décorés `_nom@N` sont ceux que le compilateur référence.
for lib in opengl32; do
    python3 -I - "$lib" "$TMP" <<'PY'
import sys, re
lib, tmp = sys.argv[1:3]
src = f"../{lib}/{lib}.def" if lib == "opengl32" else None
lines = ["LIBRARY %s" % lib, "EXPORTS"]
for l in open(src):
    m = re.match(r"\s*(\w+)=_\w+(@\d+)", l)
    if m:
        lines.append(f"    {m.group(1)}{m.group(2)}")
open(f"{tmp}/{lib}.def", "w").write("\n".join(lines) + "\n")
PY
    llvm-dlltool -m i386 -k -d "$TMP/$lib.def" -l "$TMP/$lib.lib"
done
cat > "$TMP/kernel32.def" <<'D'
LIBRARY kernel32
EXPORTS
    GetProcessHeap@0
    HeapAlloc@12
    HeapFree@12
D
llvm-dlltool -m i386 -k -d "$TMP/kernel32.def" -l "$TMP/kernel32.lib"
CFLAGS="-O2 -ffreestanding -fno-builtin -nostdlib -mno-sse -fno-stack-protector -Wall -Wextra -Wno-unused-parameter"
clang --target=i686-pc-windows-msvc $CFLAGS \
    -fuse-ld=lld -shared -Wl,/entry:_DllMainCRTStartup@12 -Wl,/noimplib -Wl,/def:glu32.def \
    -o glu32.dll glu32.c "$TMP/opengl32.lib" "$TMP/kernel32.lib"
