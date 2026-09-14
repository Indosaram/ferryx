import hashlib, json, pathlib, shutil, subprocess, tarfile
root = pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-wave1')
stage = pathlib.Path('/tmp/ferryx-herdr-q4-linux-01a097f8')
expected = json.loads((root/'docs/evidence/paired-daemon/A08-typed-owner-verifier-source-after.json').read_text())
def check(base, label):
    rows = {p: {'sha256': hashlib.sha256((base/p).read_bytes()).hexdigest(), 'bytes': (base/p).stat().st_size} for p in expected}
    assert all(rows[p]['sha256'] == expected[p]['sha256'] for p in rows)
    (stage/(label+'.json')).write_text(json.dumps(rows, indent=2)+'\n')
    print(label, len(rows), 'MATCH', flush=True)
check(root, 'source-before')
dest = stage/'source'; dest.mkdir()
paths = subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'], cwd=root).decode().split('\0')
excluded=[]
for name in sorted(set(paths)):
    if not name: continue
    p = pathlib.PurePosixPath(name)
    if not (name.startswith(('src-tauri/', 'ui/', 'scripts/', 'assets/', 'docs/evidence/paired-daemon/fixtures/')) or '/' not in name): continue
    if any(x in p.parts for x in ('target','node_modules','.git','.omo')) or p.name.startswith('.env') or name == 'src-tauri/vendor/ghostty': continue
    src = root/name
    if src.is_symlink(): excluded.append(name); continue
    if not src.is_file(): continue
    out=dest/name; out.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(src,out)
shutil.copytree(root/'ui/dist',dest/'ui/dist',dirs_exist_ok=True,symlinks=True)
assert not any(p.is_symlink() for p in (dest/'ui/dist').rglob('*'))
vendor=dest/'src-tauri/vendor/ghostty'; vendor.mkdir(parents=True)
with open(stage/'ghostty.tar','wb') as f: subprocess.run(['git','archive','HEAD'],cwd=root/'src-tauri/vendor/ghostty',stdout=f,check=True)
with tarfile.open(stage/'ghostty.tar') as t: t.extractall(vendor,filter='data')
subprocess.run(['git','bundle','create',str(stage/'ghostty.bundle'),'HEAD'],cwd=root/'src-tauri/vendor/ghostty',check=True)
check(root,'source-after-capture'); check(dest,'snapshot-baseline')
manifest={str(p.relative_to(dest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(dest.rglob('*')) if p.is_file() and not p.is_symlink()}
(stage/'snapshot-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(stage/'excluded-symlinks.json').write_text(json.dumps(excluded,indent=2)+'\n')
with tarfile.open(stage/'source.tar.gz','w:gz') as t: t.add(dest,arcname='source')
print('snapshot_files',len(manifest),'archive_sha256',hashlib.sha256((stage/'source.tar.gz').read_bytes()).hexdigest(),flush=True)
