#!/usr/bin/env python3
from __future__ import annotations
import argparse, gzip, hashlib, io, json, os, pathlib, re, stat, subprocess, tarfile, tempfile, zipfile

EPOCH=(1980,1,1,0,0,0)
def zinfo(name,mode):
    i=zipfile.ZipInfo(name,EPOCH); i.compress_type=zipfile.ZIP_DEFLATED; i.create_system=3; i.external_attr=(stat.S_IFREG|mode)<<16; return i
def atomic(path,data):
    with tempfile.NamedTemporaryFile(dir=path.parent,prefix=f'.{path.name}.',delete=False) as h: t=pathlib.Path(h.name); h.write(data); h.flush(); os.fsync(h.fileno())
    os.replace(t,path)
def render(v,repl):
    if isinstance(v,str):
        for a,b in repl.items(): v=v.replace(a,b)
    elif isinstance(v,list): v=[render(x,repl) for x in v]
    elif isinstance(v,dict): v={k:render(x,repl) for k,x in v.items()}
    return v
def tar_asset(repo,dist,version,commit,target,binary):
    root=f'agent-recall-trail_{version}_{target}'; files={f'{root}/art':(binary.read_bytes(),0o755),f'{root}/LICENSE':((repo/'LICENSE').read_bytes(),0o644),f'{root}/scripts/install.sh':((repo/'scripts/install.sh').read_bytes(),0o755)}
    for p in sorted((repo/'plugin/agent-recall-trail').rglob('*')):
        if p.is_file(): files[f'{root}/plugin/agent-recall-trail/{p.relative_to(repo / "plugin/agent-recall-trail")}']=(p.read_bytes(),0o644)
    provenance={"schema_version":"art-install-provenance/1.0","version":version,"commit":commit,"target":target,"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest()}
    files[f'{root}/provenance.json']=((json.dumps(provenance,sort_keys=True,separators=(',',':'))+'\n').encode(),0o644)
    raw=io.BytesIO()
    with tarfile.open(fileobj=raw,mode='w') as tf:
        for name,(data,mode) in sorted(files.items()):
            info=tarfile.TarInfo(name); info.size=len(data); info.mode=mode; info.mtime=info.uid=info.gid=0; info.uname=info.gname=''; tf.addfile(info,io.BytesIO(data))
    out=io.BytesIO()
    with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz: gz.write(raw.getvalue())
    atomic(dist/f'{root}.tar.gz',out.getvalue())
def member(path,name):
    with tarfile.open(path,'r:gz') as tf: return tf.extractfile(name).read()
def aggregate(repo,dist,version,commit):
    archives={t:dist/f'agent-recall-trail_{version}_{t}.tar.gz' for t in ('darwin_arm64','linux_amd64')}
    if not all(p.is_file() for p in archives.values()): return
    files={'LICENSE':((repo/'LICENSE').read_bytes(),0o644)}
    for t,p in archives.items(): files[f'server/art-{t.replace("_","-")}']=(member(p,f'agent-recall-trail_{version}_{t}/art'),0o755)
    for p in sorted((repo/'plugin/agent-recall-trail').rglob('*')):
        if p.is_file(): files[f'plugin/{p.relative_to(repo / "plugin/agent-recall-trail")}']=(p.read_bytes(),0o644)
    template=json.loads((repo/'packaging/mcpb/manifest.json.in').read_text()); files['manifest.json']=((json.dumps(render(template,{'@VERSION@':version,'@COMMIT@':commit}),sort_keys=True,separators=(',',':'))+'\n').encode(),0o644)
    buf=io.BytesIO()
    with zipfile.ZipFile(buf,'w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
        for name,(data,mode) in sorted(files.items()): z.writestr(zinfo(name,mode),data)
    bundle=dist/f'agent-recall-trail_{version}.mcpb'; atomic(bundle,buf.getvalue())
    reg=json.loads((repo/'packaging/mcp-registry/server.json.in').read_text()); reg=render(reg,{'@VERSION@':version,'@MCPB_SHA256@':hashlib.sha256(bundle.read_bytes()).hexdigest()}); atomic(dist/'server.json',(json.dumps(reg,sort_keys=True,separators=(',',':'))+'\n').encode())
    metadata=json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--locked'],cwd=repo,text=True)); packages=[{"name":p["name"],"versionInfo":p["version"]} for p in metadata['packages']]
    sbom={"spdxVersion":"SPDX-2.3","dataLicense":"CC0-1.0","SPDXID":"SPDXRef-DOCUMENT","name":f'agent-recall-trail-{version}',"documentNamespace":f'https://github.com/fantasyce/agent-recall-trail/releases/tag/v{version}#{commit}',"packages":packages}; atomic(dist/'sbom.spdx.json',(json.dumps(sbom,sort_keys=True,separators=(',',':'))+'\n').encode())
    assets=sorted(p for p in dist.iterdir() if p.is_file() and p.name!='SHA256SUMS'); atomic(dist/'SHA256SUMS',''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in assets).encode())
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--repo',type=pathlib.Path,required=True); ap.add_argument('--dist',type=pathlib.Path,required=True); ap.add_argument('--version',required=True); ap.add_argument('--commit',required=True); ap.add_argument('--target',required=True); ap.add_argument('--binary',type=pathlib.Path,required=True); a=ap.parse_args()
    if not re.fullmatch(r'\d+\.\d+\.\d+',a.version) or not re.fullmatch(r'[a-f0-9]{40}',a.commit): raise SystemExit('invalid version or commit')
    a.dist.mkdir(parents=True,exist_ok=True); tar_asset(a.repo.resolve(),a.dist.resolve(),a.version,a.commit,a.target,a.binary.resolve()); aggregate(a.repo.resolve(),a.dist.resolve(),a.version,a.commit)
if __name__=='__main__': main()
