import pathlib,json,hashlib,os,re,shutil
r=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8');m=json.loads((r/'readiness-source-hashes.json').read_text())
for p,v in m.items():assert hashlib.sha256((r/'source'/p).read_bytes()).hexdigest()==v['after']
original=json.loads((r/'snapshot-manifest.json').read_text())
delta=[p for p,v in original.items() if hashlib.sha256((r/'source'/p).read_bytes()).hexdigest()!=v]
assert set(delta)==set(m)|{'src-tauri/gen/schemas/linux-schema.json'};print('EXACT_SOURCE_DELTA',delta)
owned=[]
for p in pathlib.Path('/proc').iterdir():
 if not p.name.isdigit() or int(p.name)==os.getpid():continue
 try:
  exe=os.readlink(p/'exe');cwd=os.readlink(p/'cwd');env=(p/'environ').read_bytes()
  if exe.startswith(str(r)) or cwd.startswith(str(r)) or b'FERRYX_RUNTIME_DIR='+str(r).encode() in env:owned.append(int(p.name))
 except (FileNotFoundError,ProcessLookupError,PermissionError):pass
assert not owned;print('OWNED_LIVE_PROCESSES',owned)
text='\n'.join(p.read_text() for p in r.glob('readiness-*.log'))
pids=set(map(int,re.findall(r'(?:pid[ =]|owner=|git=|hook=|COMMAND_PID )(\d+)',text)))
for pid in sorted(pids):assert not (pathlib.Path('/proc')/str(pid)).exists(),pid
print('LOGGED_PIDS_ABSENT',sorted(pids))
roots=set(re.findall(r'/tmp/a08-typed-owner-[A-Za-z0-9]+',text));assert all(not pathlib.Path(p).exists() for p in roots);print('EXPLICIT_TMP_ROOTS_ABSENT',sorted(roots))
shutil.rmtree(r/'readiness-qa');print('READINESS_QA_REMOVED',not (r/'readiness-qa').exists())
print('RETAINED_PRIVATE_SOURCE_TARGET_LOGS',r)
