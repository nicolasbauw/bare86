#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Extracteur minimal des cabinets InstallShield 5 (data1.cab, data2.cab…)
# que unshield 1.6 ne sait pas décompresser (« version 0x01000004 » : blocs
# de 10 Ko, longueur sur 16 bits puis deflate brut ; un fichier coupé entre
# deux volumes reprend au début du suivant).
# Usage : is5x.py RÉPERTOIRE_DES_CAB DESTINATION
import struct,sys,os,zlib
src,dst=sys.argv[1],sys.argv[2]
vols={}
v=1
while os.path.exists(os.path.join(src,f'data{v}.cab')):
    d=open(os.path.join(src,f'data{v}.cab'),'rb').read()
    vh=struct.unpack_from('<10I',d,20)
    vols[v]=dict(data=d,first=vh[2],last=vh[3],ffo=vh[4],ffc=vh[6],lfo=vh[7],lfc=vh[9])
    v+=1
d=vols[1]['data']
cdo=struct.unpack_from('<I',d,12)[0]
fto=struct.unpack_from('<I',d,cdo+0x0C)[0]
base=cdo+fto
desc=d[cdo:cdo+0x40]
u=lambda o: struct.unpack_from('<I',d,o)[0]
def cstr(o):
    e=d.index(b'\0',o); return d[o:e].decode('latin-1')
# nombres de répertoires et de fichiers (descripteur du cabinet)
ndir=u(cdo+0x1C)
nfile=u(cdo+0x28)
dirs=[cstr(base+u(base+4*i)) for i in range(ndir)]
ok=bad=0
for i in range(nfile):
    fdo=u(base+4*(ndir+i))
    name_off,dir_idx=struct.unpack_from('<II',d,base+fdo)
    flags,=struct.unpack_from('<H',d,base+fdo+8)
    exp,comp=struct.unpack_from('<II',d,base+fdo+10)
    data_off,=struct.unpack_from('<I',d,base+fdo+18+0x14)
    if base+name_off>=len(d) or dir_idx>=ndir: continue
    name=cstr(base+name_off)
    if not name or comp==0 and exp==0: continue
    vs=[k for k,vv in vols.items() if vv['first']<=i<=vv['last']]
    if not vs: bad+=1; print('pas de volume',i,name); continue
    k=vs[0]; vv=vols[k]
    if len(vs)>1 and i==vv['last']:
        buf=vv['data'][vv['lfo']:vv['lfo']+vv['lfc']]
        nv=vols[vs[1]]; buf+=nv['data'][nv['ffo']:nv['ffo']+nv['ffc']]
    else:
        buf=vv['data'][data_off:data_off+comp]
    if flags & 4:
        out=bytearray(); p=0
        try:
            while len(out)<exp and p+2<=len(buf):
                n=buf[p]|buf[p+1]<<8; p+=2
                out+=zlib.decompressobj(-15).decompress(buf[p:p+n]); p+=n
        except Exception as e:
            bad+=1; print('erreur',name,e); continue
    else:
        out=buf
    if len(out)!=exp: bad+=1; print('taille',name,len(out),exp); continue
    path=os.path.join(dst,*[x for x in dirs[dir_idx].split('\\') if x],name)
    os.makedirs(os.path.dirname(path),exist_ok=True)
    open(path,'wb').write(out); ok+=1
print('ok',ok,'échecs',bad)
