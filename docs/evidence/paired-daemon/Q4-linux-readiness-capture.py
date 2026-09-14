import asyncio,json,pathlib,os,shutil,uuid,hashlib
R=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8')
E=json.loads((R/'environment.json').read_text())
Q=R/'readiness-qa'; assert not Q.exists(); Q.mkdir(mode=0o700)
for k,v in list(E.items()):
 if v.startswith(str(R/'qa')):
  E[k]=v.replace(str(R/'qa'),str(Q));pathlib.Path(E[k]).mkdir(parents=True,exist_ok=True,mode=0o700)
(R/'readiness-environment.json').write_text(json.dumps(E,indent=2)+'\n')
LIB=str(next(p for p in (R/'target/debug/deps').glob('ferryx_lib-*') if p.suffix==''))
CAT=str(next(p for p in (R/'target/debug/deps').glob('machine_catalog_persistence-*') if p.suffix==''))
async def child(label,exe,name,root,extra,marker,kill,send=None):
 env=E|{'HOME':str(root),'FERRYX_DATA_DIR':str(root/'data'),'FERRYX_RUNTIME_DIR':str(root/'runtime')}|extra
 cmd=[exe,'--exact',name,'--nocapture']; print('COMMAND',label,cmd,'RUST_TEST_THREADS',env['RUST_TEST_THREADS'],flush=True)
 err=open(R/('readiness-'+label+'.stderr'),'wb');raw=bytearray()
 p=await asyncio.create_subprocess_exec(*cmd,env=env,cwd=R/'source',stdin=asyncio.subprocess.PIPE,stdout=asyncio.subprocess.PIPE,stderr=err)
 try:
  async def receive():
   while marker not in raw:
    b=await p.stdout.read(4096)
    if not b:raise AssertionError('EOF before marker')
    raw.extend(b)
  await asyncio.wait_for(receive(),30)
  print('MARKER_BYTES',label,'pid',p.pid,repr(bytes(raw)),flush=True)
  if kill:p.kill()
  elif send:p.stdin.write(send);await p.stdin.drain()
  tail=await asyncio.wait_for(p.stdout.read(),30);raw.extend(tail)
  code=await asyncio.wait_for(p.wait(),10)
  print('RESULT',label,'pid',p.pid,'exit',code,flush=True)
  assert code==(-9 if kill else 0)
 finally:
  if p.returncode is None:p.kill();await p.wait()
  p.stdin.close();err.close();(R/('readiness-'+label+'.stdout.bin')).write_bytes(raw)
async def main():
 root=Q/'catalog';root.mkdir();(root/'plain').mkdir();(root/'git').mkdir()
 p=await asyncio.create_subprocess_exec('git','init','--quiet',str(root/'git'),env=E);assert await p.wait()==0
 await child('catalog',CAT,'catalog_owner_process',root,{'A05_OWNER_ROOT':str(root)},b'A05_READY',False,b'register\n')
 root=Q/'worktree';root.mkdir();(root/'repo').mkdir()
 for args in [['init','--quiet'],['-c','user.name=QA','-c','user.email=qa@example.invalid','commit','--allow-empty','-m','base']]:
  p=await asyncio.create_subprocess_exec('git',*args,cwd=root/'repo',env=E);assert await p.wait()==0
 name='remote::workspace_api::worktrees::authority_tests::interrupted_worktree_owner_child'
 await child('worktree-barrier',LIB,name,root,{'A08_INTERRUPTED_ROOT':str(root)},b'A08_GIT_COMPLETE_BEFORE_PUBLICATION',True)
 await child('worktree-replay',LIB,name,root,{'A08_INTERRUPTED_ROOT':str(root),'A08_INTERRUPTED_REPLAY':'1'},b'A08_RECONCILED_UNKNOWN_NO_REPEAT',False)
 root=Q/'project';root.mkdir();(root/'plain').mkdir()
 await child('project-barrier',LIB,'remote::workspace_api_tests::r12_crash_owner',root,{'R12_ROOT':str(root),'R12_PHASE':'afterCatalog','R12_REQUEST':str(uuid.uuid4())},b'R12_WINDOW',True)
try:asyncio.run(main())
finally:
 shutil.rmtree(Q);print('CAPTURE_QA_REMOVED',not Q.exists(),flush=True)
