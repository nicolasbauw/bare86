import os,struct,sys
def rebase(d,new):
    pe=struct.unpack_from('<I',d,0x3c)[0]
    nsec=struct.unpack_from('<H',d,pe+6)[0]; opt=struct.unpack_from('<H',d,pe+20)[0]
    base=struct.unpack_from('<I',d,pe+24+28)[0]
    rva,sz=struct.unpack_from('<II',d,pe+24+96+5*8)
    secs=[]; o=pe+24+opt
    for i in range(nsec):
        vs,va,rs,rp=struct.unpack_from('<IIII',d,o+8); secs.append((va,max(vs,rs),rp,rs)); o+=40
    def off(r):
        for va,vs,rp,rs in secs:
            if va<=r<va+vs and r-va<rs: return rp+r-va
        return None
    delta=(new-base)&0xFFFFFFFF
    p=0; n=0
    while p<sz:
        o=off(rva+p)
        if o is None: break
        page,blk=struct.unpack_from('<II',d,o)
        if blk<8: break
        for k in range((blk-8)//2):
            e=struct.unpack_from('<H',d,o+8+2*k)[0]; t=e>>12; r=page+(e&0xFFF)
            if t==3:
                fo=off(r)
                if fo is None: continue
                v=struct.unpack_from('<I',d,fo)[0]; struct.pack_into('<I',d,fo,(v+delta)&0xFFFFFFFF); n+=1
            elif t!=0: print("type",t)
        p+=blk
    struct.pack_into('<I',d,pe+24+28,new)
    return n
if __name__=="__main__":
    i=0
    for f in sorted(os.listdir('.')):
        if not f.upper().endswith('.DLL'): continue
        d=bytearray(open(f,'rb').read())
        if d[:2]!=b'MZ': continue
        pe=struct.unpack_from('<I',d,0x3c)[0]
        if d[pe:pe+2] not in (b'PE',b'PX'): continue
        if struct.unpack_from('<I',d,pe+24+28)[0]!=0x10000000: continue
        n=rebase(d,0x60000000+i*0x400000); i+=1
        open(f,'wb').write(d)
    print(i,"DLL rebasées")
