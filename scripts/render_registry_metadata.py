#!/usr/bin/env python3
import argparse,hashlib,json,os,re,tempfile
from pathlib import Path
def walk(v,r):
    if isinstance(v,str):
        for a,b in r.items(): v=v.replace(a,b)
    elif isinstance(v,list): v=[walk(x,r) for x in v]
    elif isinstance(v,dict): v={k:walk(x,r) for k,x in v.items()}
    return v
p=argparse.ArgumentParser(); p.add_argument('--template',type=Path,required=True); p.add_argument('--mcpb',type=Path,required=True); p.add_argument('--version',required=True); p.add_argument('--output',type=Path,required=True); a=p.parse_args()
if not re.fullmatch(r'\d+\.\d+\.\d+',a.version): raise SystemExit('invalid version')
d=walk(json.loads(a.template.read_text()),{'@VERSION@':a.version,'@MCPB_SHA256@':hashlib.sha256(a.mcpb.read_bytes()).hexdigest()}); a.output.parent.mkdir(parents=True,exist_ok=True)
with tempfile.NamedTemporaryFile(dir=a.output.parent,delete=False) as h: t=Path(h.name); h.write((json.dumps(d,sort_keys=True,separators=(',',':'))+'\n').encode()); h.flush(); os.fsync(h.fileno())
os.replace(t,a.output)
