import sys,os,subprocess,shutil,zipfile,json,collections,re
PC="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
D=PC+"/dos-demos"
S=sys.argv[1]
res=[json.loads(l) for l in open(PC+'/scan-win32/results.jsonl')]
low={json.loads(l)['item']:json.loads(l) for l in open(S+'/low109.jsonl')}
def pe(d):
    for dp,_,fs in os.walk(d):
        for f in fs:
            if f.lower().endswith(('.exe',)):
                p=os.path.join(dp,f)
                try:
                    b=open(p,'rb').read(0x1000); o=int.from_bytes(b[0x3c:0x40],'little')
                    if b[:2]==b'MZ' and b[o:o+2]==b'PE': return p
                except: pass
def imports(it):
    w=S+'/imp'; shutil.rmtree(w,ignore_errors=True); p=os.path.join(D,it)
    if os.path.isdir(p): shutil.copytree(p,w)
    else:
        os.makedirs(w); zipfile.ZipFile(p).extractall(w)
        for dp,_,fs in os.walk(w):
            for f in fs:
                if f.lower().endswith('.zip'):
                    try: zipfile.ZipFile(os.path.join(dp,f)).extractall(dp)
                    except: pass
    e=pe(w)
    if not e: return None
    o=subprocess.run(['llvm-readobj','--coff-imports',e],capture_output=True,text=True).stdout
    return sorted(set(m.lower() for m in re.findall(r'Name: (\S+\.(?:dll|DLL))',o)))
groups=collections.defaultdict(list)
for r in res:
    it=r['item']
    if r.get('kept',0)>=10: g='bon'
    elif it in low and not low[it].get('res') and not low[it]['dl'] and low[it]['bright'] is not None and low[it]['bright']<0.01: g='noir'
    elif it in low and not low[it].get('res') and not low[it]['dl'] and not low[it]['limit']: g='quitte'
    else: continue
    try: im=imports(it)
    except Exception: im=None
    if im: groups[g].append((it,im))
json.dump({g:v for g,v in groups.items()},open(S+'/imports.json','w'))
for g,v in groups.items():
    c=collections.Counter(d for _,im in v for d in im)
    print(g,len(v),{d:n for d,n in c.most_common(14)})
