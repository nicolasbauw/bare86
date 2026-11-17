#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile l'opengl32.dll de Bare86 (DLL PE i386 pour les programmes Win32
# sous HX, sans bibliothèque C) avec clang et lld. gen.py produit d'abord
# gl_gen.c, opengl32.def et src/opengl/api.rs. La DLL compilée est
# versionnée : cette étape ne sert qu'à qui modifie les sources.
set -e
cd "$(dirname "$0")"
python3 -I gen.py
CFLAGS="-O2 -ffreestanding -fno-builtin -nostdlib -mno-sse -fno-stack-protector -Wall -Wextra -Wno-unused-parameter"
clang --target=i686-pc-windows-msvc $CFLAGS \
    -fuse-ld=lld -shared -Wl,/entry:_DllMainCRTStartup@12 -Wl,/noimplib -Wl,/def:opengl32.def \
    -o opengl32.dll opengl32.c
