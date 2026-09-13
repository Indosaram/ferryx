import pathlib,os,json,hashlib,re,shutil
r=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8')
baseline=json.loads((r/'snapshot-baseline.json').read_text()); manifest=json.loads((r/'snapshot-manifest.json').read_text())
for label,rows in [('baseline',baseline),('full',manifest)]:
 bad=[p for p,v in rows.items() if hashlib.sha256((r/'source'/p).read_bytes()).hexdigest() != (v['sha256'] if isinstance(v,dict) else v)]
 print('SOURCE_AFTER_RUN',label,'checked',len(rows),'mismatches',bad)
 if label=='baseline': assert not bad
 else:
  assert bad==['src-tauri/gen/schemas/linux-schema.json']
  delta={p:{'before':rows[p],'after':hashlib.sha256((r/'source'/p).read_bytes()).hexdigest()} for p in bad}
  (r/'generated-source-delta.json').write_text(json.dumps(delta,indent=2)+'\n')
texts='\n'.join(p.read_text() for p in r.glob('*.log'))
pids=set(int(x) for x in re.findall(r'(?:pid=|git=|leaf=|hook=|intermediate=|owner=|git_group=|COMMAND_PID )(\d+)',texts))
owned=[]; receipts=[]
for entry in pathlib.Path('/proc').iterdir():
 if not entry.name.isdigit() or int(entry.name)==os.getpid():continue
 try:
  exe=os.readlink(entry/'exe')
  cwd=os.readlink(entry/'cwd')
  environ=(entry/'environ').read_bytes().split(b'\0')
  private=exe.startswith(str(r)) or cwd.startswith(str(r)) or any(v.startswith((b'HOME=',b'FERRYX_RUNTIME_DIR=',b'FERRYX_DATA_DIR=')) and str(r).encode() in v for v in environ)
  if private:owned.append({'pid':int(entry.name),'exe':exe,'cwd':cwd})
 except (FileNotFoundError,ProcessLookupError,PermissionError):pass
for pid in sorted(pids):
 p=pathlib.Path('/proc')/str(pid)
 try:
  stat=(p/'stat').read_text();state=stat.rsplit(') ',1)[1].split()[0]
  receipts.append({'pid':pid,'state':state})
 except FileNotFoundError:receipts.append({'pid':pid,'state':'absent'})
print('OWNED_PROCESS_SCAN',json.dumps(owned));print('LOGGED_PID_RECEIPTS',json.dumps(receipts))
assert not owned,'owned process remains; inspect rather than broad kill'
roots=sorted(set(re.findall(r'/tmp/(?:a08-typed-owner-|a08-transports-)[A-Za-z0-9]+',texts)))
print('EXPLICIT_TMP_FIXTURE_ROOTS',json.dumps({p:pathlib.Path(p).exists() for p in roots}))
assert not any(pathlib.Path(p).exists() for p in roots)
print('QA_TREE_BEFORE_REMOVAL',json.dumps([str(p.relative_to(r/'qa')) for p in (r/'qa').rglob('*')]))
shutil.rmtree(r/'qa');print('QA_ROOT_REMOVED',not (r/'qa').exists())
print('RETAINED',str(r),'source archive pinned vendor bundle target logs scripts manifests; no runtime credentials retained in qa')
