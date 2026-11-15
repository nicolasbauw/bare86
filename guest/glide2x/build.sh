#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile la glide2x de Bare86 (i386, sans bibliothèque C) avec clang et lld :
#  - glide2x.dll : DLL PE pour les programmes Win32 sous HX ;
#  - glide2x.ovl : DLL LE pour les jeux DOS sous DOS/4GW (mkovl.py).
# Les DLL compilées sont versionnées : cette étape ne sert qu'à qui modifie
# les sources.
set -e
cd "$(dirname "$0")"
CFLAGS="-O2 -ffreestanding -fno-builtin -nostdlib -mno-sse -fno-stack-protector -Wall -Wextra"
clang --target=i686-pc-windows-msvc $CFLAGS \
    -fuse-ld=lld -shared -Wl,/entry:_DllMainCRTStartup@12 -Wl,/noimplib \
    -o glide2x.dll glide2x.c
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
clang --target=i686-unknown-linux-gnu $CFLAGS -fno-pic -fno-asynchronous-unwind-tables -c -o "$tmp/glide2x.o" glide2x.c
python3 -I mkovl.py stubs "$tmp/glide2x.o" > "$tmp/stubs.c"
clang --target=i686-unknown-linux-gnu $CFLAGS -fno-pic -fno-asynchronous-unwind-tables -Wno-unused-parameter -c -o "$tmp/stubs.o" "$tmp/stubs.c"
ld.lld -m elf_i386 -static -nostdlib -e grGlideInit --emit-relocs -T ovl.ld -o "$tmp/glide2x.elf" "$tmp/glide2x.o" "$tmp/stubs.o"
python3 -I mkovl.py ovl "$tmp/glide2x.elf" glide2x.ovl
