import struct,sys
KNOWN={ 'F5049E77-4861-11D2-A407-00A0C90629A8':'IDirect3D7','BB223240-E72B-11D0-A9B4-00AA00C0993E':'IDirect3D3','6AAE1EC1-662A-11D0-889D-00AA00BBB76A':'IDirect3D2','3BBA0080-2421-11CF-A31A-00AA00B93356':'IDirect3D',
 'DA044E00-69B2-11D0-A1D5-00AA00B8DFBB':'IDirectDraw','B3A6F3E0-2B43-11CF-A2DE-00AA00B93356':'IDirectDraw2','9C59509A-39BD-11D1-8C4A-00C04FD930C5':'IDirectDraw4','15E65EC0-3B9C-11D2-B92F-00609797EA5B':'IDirectDraw7',
 '6C14DB80-A733-11CE-A521-0020AF0BE560':'IDirectDraw(old)','6C14DB81-A733-11CE-A521-0020AF0BE560':'IDirectDrawSurface','6C14DB82-A733-11CE-A521-0020AF0BE560':'IDirectDrawPalette',
 '84E63DE0-46AA-11CF-816F-0000C020156E':'IDirect3DHALDevice','A4665C60-2673-11CF-A31A-00AA00B93356':'IDirect3DRGBDevice','F5049E78-4861-11D2-A407-00A0C90629A8':'IDirect3DTnLHalDevice',
 '5B11F5C0-ED3B-11CF-8B5C-00A0C9007005':'?','0EF0B6A0-...':'?','279AFA83-4981-11CE-A521-0020AF0BE560':'IDirectDrawClipper','6C14DB84-A733-11CE-A521-0020AF0BE560':'IDirectDrawColorControl?'}
def va2off(b,va):
    pe=struct.unpack_from('<I',b,0x3c)[0]; ns,=struct.unpack_from('<H',b,pe+6); so,=struct.unpack_from('<H',b,pe+20); ib=struct.unpack_from('<I',b,pe+24+28)[0]
    for j in range(ns):
        s=pe+24+so+40*j; vs,rva,rs,rp=struct.unpack_from('<IIII',b,s+8)
        if rva<=va-ib<rva+max(vs,rs): return rp+(va-ib-rva)
def iid(b,va):
    o=va2off(b,va); 
    if o is None: return '?'
    d,w1,w2=struct.unpack_from('<IHH',b,o); t=b[o+8:o+16]
    g='%08X-%04X-%04X-%02X%02X-%s'%(d,w1,w2,t[0],t[1],t[2:].hex().upper())
    return g+' = '+KNOWN.get(g,'inconnu')
if __name__=='__main__':
    b=open(sys.argv[1],'rb').read()
    for a in sys.argv[2:]: print(a,iid(b,int(a,16)))
