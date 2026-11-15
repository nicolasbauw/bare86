#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compile la glide2x.dll de Bare86 (Win32, i386, sans bibliothèque C) avec
# clang et lld. La DLL compilée est versionnée à côté : cette étape ne sert
# qu'à qui modifie glide2x.c.
set -e
cd "$(dirname "$0")"
clang --target=i686-pc-windows-msvc -O2 -ffreestanding -fno-builtin -nostdlib \
    -mno-sse -fno-stack-protector -Wall -Wextra \
    -fuse-ld=lld -shared -Wl,/entry:_DllMainCRTStartup@12 -Wl,/noimplib \
    -o glide2x.dll glide2x.c
