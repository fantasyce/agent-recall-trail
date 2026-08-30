#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 3 ]] || { echo 'usage: verify_release_assets.sh DIST VERSION COMMIT' >&2; exit 64; }
dist="$(cd "$1" && pwd -P)"; version="$2"; commit="$3"
for n in "agent-recall-trail_${version}_darwin_arm64.tar.gz" "agent-recall-trail_${version}_linux_amd64.tar.gz" "agent-recall-trail_${version}.mcpb" server.json sbom.spdx.json SHA256SUMS; do [[ -s "$dist/$n" ]] || { echo "missing release asset: $n" >&2; exit 1; }; done
(cd "$dist" && shasum -a 256 -c SHA256SUMS)
python3 - "$dist" "$version" "$commit" <<'PY'
import hashlib,json,pathlib,re,sys,tarfile,zipfile
d,v,c=pathlib.Path(sys.argv[1]),sys.argv[2],sys.argv[3]
bins={}
private_build_paths = re.compile(
    rb'(?:/' + b'Users/[^/\x00]+|/home/' + b'runner|/private/' + b'tmp)/'
)
for t in ('darwin_arm64','linux_amd64'):
 p=d/f'agent-recall-trail_{v}_{t}.tar.gz'; root=f'agent-recall-trail_{v}_{t}'
 with tarfile.open(p,'r:gz') as a:
  ms=a.getmembers(); assert [m.name for m in ms]==sorted(m.name for m in ms); assert all(m.mtime==m.uid==m.gid==0 for m in ms)
  bins[t]=a.extractfile(f'{root}/art').read(); prov=json.load(a.extractfile(f'{root}/provenance.json'))
  assert prov['version']==v and prov['commit']==c and prov['target']==t and prov['binary_sha256']==hashlib.sha256(bins[t]).hexdigest()
  assert not private_build_paths.search(bins[t]), f'{t} binary contains an absolute build-host path'
with zipfile.ZipFile(d/f'agent-recall-trail_{v}.mcpb') as a:
 assert a.namelist()==sorted(a.namelist()); m=json.loads(a.read('manifest.json')); assert m['version']==v and len(m['tools'])==6
 assert a.read('server/art-darwin-arm64')==bins['darwin_arm64']; assert a.read('server/art-linux-amd64')==bins['linux_amd64']
r=json.loads((d/'server.json').read_text()); assert r['version']==v and r['packages'][0]['fileSha256']==hashlib.sha256((d/f'agent-recall-trail_{v}.mcpb').read_bytes()).hexdigest()
s=json.loads((d/'sbom.spdx.json').read_text()); assert s['spdxVersion']=='SPDX-2.3' and s['packages']
PY
echo 'release assets verified'
