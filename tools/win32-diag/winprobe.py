import sys,os,subprocess,shutil,zipfile,re,json
from concurrent.futures import ThreadPoolExecutor
PC="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team/dos-demos"
S=sys.argv[1]; items=[l.strip() for l in open(sys.argv[2]) if l.strip()]
BARE=os.environ["BARE"]; PACK=os.environ.get("PACK"); KEYS=os.environ.get("KEYS","{wait:30000}")
def pe_exes(d):
    r=[]
    for dp,_,fs in os.walk(d):
        for f in fs:
            if f.lower().endswith('.exe'):
                p=os.path.join(dp,f)
                try:
                    b=open(p,'rb').read(0x1000); o=int.from_bytes(b[0x3c:0x40],'little')
                    if b[:2]==b'MZ' and o<0x1000-4 and b[o:o+2]==b'PE': r.append(p)
                except: pass
    return r
def work(a):
    i,it=a
    w=os.path.join(S,'wq',str(i)); shutil.rmtree(w,ignore_errors=True); os.makedirs(w)
    p=os.path.join(PC,it)
    if os.path.isdir(p): shutil.rmtree(w); shutil.copytree(p,w)
    else:
        try: zipfile.ZipFile(p).extractall(w)
        except Exception as e: return {'item':it,'res':'bad'}
    for dp,_,fs in os.walk(w):
        for f in fs:
            if f.lower().endswith('.zip'):
                try: zipfile.ZipFile(os.path.join(dp,f)).extractall(dp)
                except: pass
    ex=pe_exes(w)
    if not ex: return {'item':it,'res':'no-pe'}
    e=ex[0]; d=os.path.dirname(e)
    if PACK:
        have={f.lower() for f in os.listdir(d)}
        for f in os.listdir(PACK):
            if f.lower() not in have: shutil.copy(os.path.join(PACK,f),os.path.join(d,f))
    shot=os.path.join(w,'s.png')
    try:
        r=subprocess.run([BARE,'--no-realtime','--limit','28','--keys',KEYS,'--shot',shot,'--screen',os.path.basename(e)],cwd=d,capture_output=True,text=True,timeout=120,stdin=subprocess.DEVNULL)
        txt=r.stdout+r.stderr
    except Exception as x: txt='timeout'
    dl=[l.strip() for l in txt.splitlines() if l.startswith('DPMILD32') or l.startswith('[win32]') or l.startswith('[gl') or l.startswith('[opengl') or 'not handled' in l or 'not implemented' in l or l.startswith('dkrnl32') or 'ip = Module' in l or 'caused by' in l][:8]
    bright=None
    if os.path.exists(shot):
        o=subprocess.run(['magick',shot,'-colorspace','Gray','-format','%[fx:mean]','info:'],capture_output=True,text=True).stdout
        try: bright=float(o)
        except: pass
    scr=txt.split('screen:')[-1].strip()[:200] if 'screen:' in txt else ''
    res={'item':it,'exe':os.path.basename(e),'dl':dl,'bright':bright,'limit':'time limit' in txt,'scr':scr}
    shutil.copy(shot,os.path.join(S,'wq-shots',f'{i}.png')) if os.path.exists(shot) else None
    shutil.rmtree(w,ignore_errors=True)
    return res
os.makedirs(os.path.join(S,'wq-shots'),exist_ok=True)
with ThreadPoolExecutor(6) as ex, open(sys.argv[3],'w') as out:
    for r in ex.map(work,list(enumerate(items))):
        out.write(json.dumps(r)+"\n"); out.flush()
