#!/bin/bash
# Debug DUSER32.DLL (traced calls written to hxstderr.log through dbgstub.asm), from ~/Dev/HX; otherwise as
# build-linux/build-duser32.sh of the fork (needs wrc: WATCOM=~/Dev/open-watcom-v2/rel).
# Usage: tools/hx-build/build-duser32-debug.sh [WORK_DIR]   (default: /tmp/hx-build). Result: WORK_DIR/duser32/out/DUSER32.DLL
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
HX=$HOME/Dev/HX
W=${1:-/tmp/hx-build}
HXDEV_URL=https://github.com/Baron-von-Riedesel/HX/releases/download/v2.24/HXDEV224.zip
HXDEV_SHA256=40f7aebe01afd2ab252a43fb3130326b4a7be8d081873bb099be7767852678f1
[ -n "$WATCOM" ] && export PATH="$WATCOM/binl64:$WATCOM/binl:$PATH"
command -v wrc >/dev/null || { echo "wrc (Open Watcom) not found: set WATCOM or put wrc in the PATH" >&2; exit 1; }
mkdir -p "$W" && cd "$W"
[ -d jwasm ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/JWasm.git jwasm
[ -d jwlink ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/jwlink.git jwlink
[ -x jwasm/build/GccUnixR/jwasm ] || (cd jwasm && make -f GccUnix.mak -j8 >/dev/null)
[ -x jwlink/build/jwlinkLR/jwlink ] || (cd jwlink && make -f GccUnix.mak -j8 >/dev/null)
JW=$W/jwasm/build/GccUnixR/jwasm
LK=$W/jwlink/build/jwlinkLR/jwlink
if [ ! -d hxdev ]; then
    mkdir hxdev && curl -fsSL -o hxdev/hxdev.zip "$HXDEV_URL"
    echo "$HXDEV_SHA256  hxdev/hxdev.zip" | sha256sum -c - >/dev/null || { echo "HXDEV224.zip: unexpected SHA-256" >&2; rm -rf hxdev; exit 1; }
    (cd hxdev && unzip -q hxdev.zip)
fi
B=$W/duser32; rm -rf "$B"; mkdir -p "$B/src" "$B/out" "$B/inc" "$B/lib" "$B/res"
# JWasm looks up included files with the case of the source: lower-case copies.
for f in "$HX"/SrcEmu/DUSER32/*; do [ -f "$f" ] && cp "$f" "$B/src/$(basename "$f" | tr A-Z a-z)"; done
cp "$HX"/SrcEmu/DUSER32/LIBCOFF/libc32u.lib "$B/lib/"
for f in "$HX"/SrcEmu/DUSER32/RES/*; do cp "$f" "$B/res/$(basename "$f" | tr A-Z a-z)"; done
for d in "$HX/Include" "$W/hxdev/INCLUDE"; do for f in "$d"/*; do [ -f "$f" ] && cp -n "$f" "$B/inc/$(basename "$f" | tr A-Z a-z)"; done; done
python3 - "$B/inc/macros.inc" <<'PY'
import sys
p=sys.argv[1]; t=open(p,'rb').read().decode('latin-1')
t=t.replace("\t\t\tmov ax, 0002\r\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi").replace("\t\t\tmov ax, 0002\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi")
t=t.replace("@trace macro x\r\nlocal y, defConst","@trace macro x\r\nlocal y, defConst\r\nexterndef __dbgstr_esi:near")
t=t.replace("invoke OutputDebugStringA, offset y","mov esi, offset y\r\n\t\t\tcall __dbgstr_esi").replace("invoke OutputDebugStringA, x","mov esi, x\r\n\t\t\tcall __dbgstr_esi")
open(p,'wb').write(t.encode('latin-1'))
PY
sed -i 's/@strace <"OemKeyScanA(", wOemChar,/@strace <"OemKeyScanA(",/' "$B/src/keyboard.asm"
cp "$HERE/dbgstub.asm" "$B/src/dbgstub.asm"
cd "$B/src"
MODS=$(tr -d '\r' < modules.inc | sed 's/\\//g; s#\./##g; s#^\.##; s/\.asm//I' | tr A-Z a-z | tr -s ' \n' ' ')
for m in $MODS duser32 dbgstub; do
    $JW -c -coff -nologo -Cp -Sg -D?FLAT=1 -D_USER32_=1 -D_DEBUG -I../inc -I. -Fl=../out/$m -Fo=../out/$m.obj $m.asm >../out/$m.log 2>&1 || { echo "FAILED: $m" >&2; tail -3 ../out/$m.log >&2; exit 1; }
done
# resources: "RES\\52.cur" -> ../res/52.cur ; -zm: Microsoft .res format, which JWlink reads
tr -d '\r' < duser32.rc | sed 's#RES\\\\#../res/#; s#"\(\.\./res/[^"]*\)"#"\L\1"#' > ../out/duser32.rc
cd ../out
wrc -q -r -zm -bt=nt -fo=duser32.res duser32.rc
OBJS=$(for m in $MODS; do echo -n "$m.obj,"; done)
cp ../src/duser32.rsp .
$LK format win pe dll file ${OBJS}duser32.obj,dbgstub.obj res duser32.res name DUSER32.DLL libpath "$B/lib" libpath "$W/hxdev/LIB" lib libc32u.lib,DKRNL32.lib,DGDI32.lib op q, map=DUSER32, implib=DUSER32 @duser32.rsp
ls -l DUSER32.DLL
