exec(open('capture.py').read().split("check(root, 'source-before')")[0])
dest=stage/'source'
shutil.copy2(root/'src-tauri/Cargo.lock',dest/'src-tauri/Cargo.lock')
check(root,'source-after-capture'); check(dest,'snapshot-baseline')
manifest={str(p.relative_to(dest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(dest.rglob('*')) if p.is_file() and not p.is_symlink()}
(stage/'snapshot-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
with tarfile.open(stage/'source.tar.gz','w:gz') as t: t.add(dest,arcname='source')
print('snapshot_files',len(manifest),'archive_sha256',hashlib.sha256((stage/'source.tar.gz').read_bytes()).hexdigest(),flush=True)
