#!/bin/bash
# usage example: tools/hx-build/dll-build-debug.sh DDDRAW OUTDIR "-D_DEBUG" "DKRNL32.lib DGDI32.lib VESA32.lib"
# usage: dll-build.sh NAME(UPPER) OUTDIR "extra jwasm opts" "libs"
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
N=$1; B=$2; OPTS=$3; LIBS=$4; n=$(echo $N | tr A-Z a-z)
W=${HX_BUILD:-/tmp/hx-build}; # JWasm, JWlink, HXDEV: as left by the build-linux scripts of the fork
 JW=$W/jwasm/build/GccUnixR/jwasm; LK=$W/jwlink/build/jwlinkLR/jwlink
SRC=$HOME/Dev/HX/SrcEmu/$N
rm -rf $B; mkdir -p $B/src $B/out $B/inc
for f in "$SRC"/*; do [ -f "$f" ] && cp "$f" "$B/src/$(basename "$f" | tr A-Z a-z)"; done
for d in $HOME/Dev/HX/Include $W/hxdev/INCLUDE; do for f in $d/*; do [ -f "$f" ] && cp -n "$f" "$B/inc/$(basename "$f" | tr A-Z a-z)"; done; done
python3 - $B/inc/macros.inc <<'PY'
import sys
p=sys.argv[1]; t=open(p,'rb').read().decode('latin-1')
t=t.replace("\t\t\tmov ax, 0002\r\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi").replace("\t\t\tmov ax, 0002\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi")
t=t.replace("@trace macro x\r\nlocal y, defConst","@trace macro x\r\nlocal y, defConst\r\nexterndef __dbgstr_esi:near")
t=t.replace("invoke OutputDebugStringA, offset y","mov esi, offset y\r\n\t\t\tcall __dbgstr_esi").replace("invoke OutputDebugStringA, x","mov esi, x\r\n\t\t\tcall __dbgstr_esi")
open(p,'wb').write(t.encode('latin-1'))
PY
cp "$HERE/dbgstub.asm" $B/src/dbgstub.asm
cd $B/src
MODS=$(tr -d '\r' < modules.inc | sed 's/\\//g; s#\./##g; s#^\.##; s/\.asm//I' | tr A-Z a-z | tr -s ' \n' ' ')
for m in $MODS $n dbgstub; do m=${m#.}
  $JW -c -coff -nologo -Cp -Sg -I../inc -I. -D?FLAT=1 $OPTS -Fl=../out/$m -Fo=../out/$m.obj $m.asm >../out/$m.log 2>&1 || { echo "ECHEC $m"; tail -3 ../out/$m.log; exit 1; }
done
cd ../out
OBJS=$(for m in $MODS; do echo -n "${m#.}.obj,"; done)
cp ../src/$n.rsp .
LIBLIST=$(for l in $LIBS; do echo -n "$l "; done)
$LK format win pe dll file ${OBJS}$n.obj,dbgstub.obj name $N.DLL libpath $W/hxdev/LIB lib $(echo $LIBS | sed 's/ /,/g') op q, map=$N, implib=$N @$n.rsp 2>&1 | tail -3
ls -l $N.DLL
