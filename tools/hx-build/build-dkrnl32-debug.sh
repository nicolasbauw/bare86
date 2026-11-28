#!/bin/bash
# Debug DKRNL32.DLL (each API call traced to hxstderr.log through dbgstub-k.asm), from ~/Dev/HX; else as build-linux/build-dkrnl32.sh of the fork: the "SBEMU" variant shipped in HX\BIN
# (thread.asm and thread3.asm assembled with ?DIRECTDISP=1), linked in the "PE HX" format (signature PX) like the official one.
# Tools: JWasm and JWlink (as for build-dpmild32.sh). All the modules are given to the linker (no static library).
# Usage: tools/hx-build/build-dkrnl32-debug.sh [WORK_DIR]   (default: /tmp/hx-build). Result: WORK_DIR/dkrnl32dbg/out/DKRNL32.DLL
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
HX=/home/nicolasb/Dev/HX
W=${1:-/tmp/hx-build}
mkdir -p "$W" && cd "$W"
[ -d jwasm ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/JWasm.git jwasm
[ -d jwlink ] || git clone -q --depth 1 https://github.com/Baron-von-Riedesel/jwlink.git jwlink
[ -x jwasm/build/GccUnixR/jwasm ] || (cd jwasm && make -f GccUnix.mak -j8 >/dev/null)
[ -x jwlink/build/jwlinkLR/jwlink ] || (cd jwlink && make -f GccUnix.mak -j8 >/dev/null)
JW=$W/jwasm/build/GccUnixR/jwasm
LK=$W/jwlink/build/jwlinkLR/jwlink
B=$W/dkrnl32dbg; rm -rf "$B"; mkdir -p "$B/src" "$B/out" "$B/inc"
for f in "$HX"/SrcEmu/DKRNL32/* "$HX"/SrcEmu/NTLFNHLP/INT21LFN.ASM; do [ -f "$f" ] && cp "$f" "$B/src/$(basename "$f" | tr A-Z a-z)"; done
for f in "$HX"/Include/*; do [ -f "$f" ] && cp "$f" "$B/inc/$(basename "$f" | tr A-Z a-z)"; done
python3 - "$B/inc/macros.inc" <<'PY'
import sys
p=sys.argv[1]; t=open(p,'rb').read().decode('latin-1')
t=t.replace("\t\t\tmov ax, 0002\r\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi").replace("\t\t\tmov ax, 0002\n\t\t\tint 41h","\t\t\tcall __dbgstr_esi")
t=t.replace("@trace macro x\r\nlocal y, defConst","@trace macro x\r\nlocal y, defConst\r\nexterndef __dbgstr_esi:near")
t=t.replace("invoke OutputDebugStringA, offset y","mov esi, offset y\r\n\t\t\tcall __dbgstr_esi").replace("invoke OutputDebugStringA, x","mov esi, x\r\n\t\t\tcall __dbgstr_esi")
open(p,'wb').write(t.encode('latin-1'))
PY
cp "$HERE/dbgstub-k.asm" "$B/src/dbgstub.asm"
python3 - "$B/src/dw2a.asm" <<'PY'
import sys
p=sys.argv[1]; t=open(p,'rb').read().decode('latin-1')
t=t.replace("\tmov ax,2\r\n\tint 41h","\textern __dbgstr_esi:near\r\n\tcall __dbgstr_esi")
open(p,'wb').write(t.encode('latin-1'))
PY
cd "$B/src"
MODS=$(tr -d '\r' < modules.inc | sed 's/\\//g; s#\./##g; s#^\.##; s/\.asm//I' | tr A-Z a-z | tr -s ' \n' ' ')
OPT="-zze -c -coff -nologo -Sg -D_DEBUG -D_KERNEL32_=1 -I../inc -D?FLAT=1"
for m in $MODS dkrnl32 procfeat dbgstub; do
    [ -f $m.asm ] || continue
    extra=""
    case $m in thread|thread3) extra="-D?DIRECTDISP=1";; esac
    $JW $OPT $extra -Fl=../out/$m -Fo=../out/$m.obj $m.asm >../out/$m.log 2>&1 || { echo "FAILED: $m" >&2; tail -3 ../out/$m.log >&2; exit 1; }
done
$JW -c -nologo -coff -D?FLAT=1 -Cp -Sg -I../inc -Fl=../out/int21lfn -Fo=../out/int21lfn.obj int21lfn.asm >../out/int21lfn.log 2>&1 || { echo "FAILED: int21lfn" >&2; tail -3 ../out/int21lfn.log >&2; exit 1; }
cd ../out
OBJS=$(for m in dkrnl32 procfeat $MODS int21lfn dbgstub; do [ -f $m.obj ] && echo -n "$m.obj,"; done | awk -v RS=, '!s[$0]++' | paste -sd,)
# JWlink does not resolve the short names of the .RSP (OP FUZZYEXPORT): exports with their decorated names
# (stdcall: _Name@N, cdecl: _Name), keeping the ordinals (".N") and NONAME of the .RSP.
python3 - <<'PY'
import re,subprocess,glob,sys
syms={}
for o in glob.glob('*.obj'):
    t=subprocess.run(['objdump','-t',o],capture_output=True,text=True).stdout
    for m in re.finditer(r'\(sec\s+[1-9]\d*\)\(fl 0x00\)\(ty\s+\w+\)\(scl\s+2\) \(nx 0\) 0x[0-9a-f]+ (\S+)$',t,re.M):
        s=m.group(1); b=re.match(r'_?(\w+?)(?:@\d+)?$',s)
        if b: syms.setdefault(b.group(1),s)
out=['OPTION MODNAME=KERNEL32','EXPORT {']
miss=[]
for l in open('../src/dkrnl32.rsp',errors='replace'):
    m=re.fullmatch(r'\s*(\w+)(?:\.(\d+))?(\s+NONAME)?\s*',l)
    if not m or m.group(1) in ('EXPORT','OPTION','OP'): continue
    n=m.group(1)
    if n not in syms: miss.append(n); continue
    out.append(" %s%s='%s'%s"%(n,('.'+m.group(2)) if m.group(2) else '',syms[n],' NONAME' if m.group(3) else ''))
out.append('}')
open('dkrnl32.exp','w').write('\n'.join(out)+'\n')
if miss: print('exports without a symbol:',' '.join(miss),file=sys.stderr)
PY
$LK format windows pe hx dll name DKRNL32.DLL file "$OBJS" op q, map=DKRNL32, implib=DKRNL32 @dkrnl32.exp
ls -l DKRNL32.DLL
