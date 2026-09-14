import pathlib, subprocess, hashlib, json, shutil, tarfile, os
root=pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-wave1')
stage=pathlib.Path('/tmp/ferryx-herdr-q4-windows-01a097f8')
expected=json.loads((root/'docs/evidence/paired-daemon/A08-typed-owner-verifier-source-after.json').read_text())
def check(base,label):
    rows={p:{'sha256':hashlib.sha256((base/p).read_bytes()).hexdigest(),'expected':v['sha256']} for p,v in expected.items()}
    assert len(rows)==34 and all(v['sha256']==v['expected'] for v in rows.values())
    (stage/(label+'.json')).write_text(json.dumps(rows,indent=2)+'\n')
check(root,'source-before')
dest=stage/'source'; dest.mkdir(exist_ok=True)
names=subprocess.check_output(['git','-C',str(root),'ls-files','--cached','--others','--exclude-standard','-z']).decode().split('\0')+list(expected)
skipped=[]
for name in sorted(set(names)):
    if not name: continue
    p=pathlib.Path(name)
    if p.parts[0] not in ('src-tauri','ui','scripts','script','remote-helper','docs') and name not in ('package.json','bun.lock','AGENTS.md'): continue
    if p.parts[0]=='docs' and not name.startswith('docs/evidence/paired-daemon/fixtures/'): continue
    src=root/p
    if src.is_symlink() or not src.is_file(): skipped.append(name); continue
    if any(x in p.parts for x in ('target','node_modules','.git','.zig-cache','zig-out')) or p.name.startswith('.env'): skipped.append(name); continue
    out=dest/p; out.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(src,out)
# Build assets are private regular-file copies; never dereference dependency links.
for src in (root/'ui/dist').rglob('*'):
    if src.is_file() and not src.is_symlink():
        out=dest/src.relative_to(root); out.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(src,out)
ghost=root/'src-tauri/vendor/ghostty'
assert not ghost.is_symlink()
sha=subprocess.check_output(['git','-C',str(ghost),'rev-parse','HEAD']).decode().strip()
assert sha=='6a508fd5e34c7e222c052a6d00bb3891ff3feace'
subprocess.run(['git','-C',str(ghost),'bundle','create',str(stage/'ghostty.bundle'),'HEAD'],check=True)
check(root,'source-after'); check(dest,'snapshot-34')
manifest={str(p.relative_to(dest)):{'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size} for p in sorted(dest.rglob('*')) if p.is_file()}
(stage/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
with tarfile.open(stage/'source.tar.gz','w:gz') as tf: tf.add(dest,arcname='source')
receipt={'head':subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD']).decode().strip(),'files':len(manifest),'skipped':skipped,'ghostty':sha,'archive_sha256':hashlib.sha256((stage/'source.tar.gz').read_bytes()).hexdigest(),'bundle_sha256':hashlib.sha256((stage/'ghostty.bundle').read_bytes()).hexdigest()}
(stage/'snapshot-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
