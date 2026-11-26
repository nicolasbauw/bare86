import json,re,os,subprocess,sys
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0,'/tmp/claude-1000/exc')
from iid import KNOWN
ROOT="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
res={json.loads(l)['id']:json.loads(l) for l in open(ROOT+"/scan-win32b/results.jsonl")}
o=json.load(open('/tmp/claude-1000/ocr-win32b.json'))
ids=[k for k,v in o.items() if re.search(r'exception\s*\S+,',v,re.I) and not re.search(r'tamper|virus',v,re.I)]
B="/home/nicolasb/Dev/bare86/target/release/bare86"; X="/tmp/claude-1000/xdg-dbg"
def guid(m):
    a=[int(x,16) for x in m]
    d1,d2,d3,d4=a
    b=[(d3>>(8*i))&255 for i in range(4)]+[(d4>>(8*i))&255 for i in range(4)]
    g='%08X-%04X-%04X-%02X%02X-%s'%(d1,d2&0xFFFF,d2>>16,b[0],b[1],''.join('%02X'%x for x in b[2:]))
    return g
def run(i):
    x=res[i]; d=f"/tmp/claude-1000/exc/{i}"; exe=None
    for r,_,fs in os.walk(d):
        for f in fs:
            if f.lower()==os.path.basename(x['exe']).lower(): exe=os.path.join(r,f)
    keys=(x.get('profile') or {}).get('keys','{wait:900000}'); cwd=os.path.dirname(exe)
    try: subprocess.run([B,'--no-realtime','--limit','20','--keys',keys,os.path.basename(exe)],cwd=cwd,env=dict(os.environ,XDG_DATA_HOME=X),capture_output=True,timeout=80)
    except Exception: pass
    log=open(cwd+'/hxstderr.log','rb').read().decode('latin1') if os.path.exists(cwd+'/hxstderr.log') else ''
    lines=[l for l in log.splitlines() if l.strip() and 'EnumDisplayModesCallback' not in l and 'dkrnl32' not in l and not l.startswith('\t')]
    out=[]
    for l in lines[-7:]:
        m=re.search(r'QueryInterface\(\w+ , \w+  \[(\w+)  (\w+)  (\w+)  (\w+) \]',l)
        if m:
            g=guid(m.groups()); r=re.search(r'\)=(\w+)',l); l=f"QueryInterface IID {g} = {KNOWN.get(g,'?')} -> {r.group(1) if r else ''}"
        out.append(l[:150])
    return i,out
with ThreadPoolExecutor(3) as ex: r=list(ex.map(run,ids))
for i,out in r:
    print('==',i.split('pcteam_')[1][:40]); [print('   ',l) for l in out]
