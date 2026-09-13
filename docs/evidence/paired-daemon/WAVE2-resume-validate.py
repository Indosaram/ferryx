import hashlib, json, os, pathlib, shutil, subprocess, tempfile, time

ROOT = pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8')
E = ROOT / 'docs/evidence/paired-daemon'
def artifact(name, text):
    path = 'docs/evidence/paired-daemon/' + name
    patch = '*** Begin Patch\n*** Add File: '+path+'\n'+''.join('+'+line+'\n' for line in text.splitlines())+'*** End Patch\n'
    subprocess.run(['apply_patch'], input=patch, text=True, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
def manifest():
    files = []
    for base in ['src-tauri/src','src-tauri/tests','src-tauri/examples','ui/src']:
        files += [p for p in (ROOT/base).rglob('*') if p.is_file()]
    files += [ROOT/'src-tauri/Cargo.lock', ROOT/'src-tauri/Cargo.toml']
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(files)}
before = manifest()
artifact('WAVE2-resume-source-before.json', json.dumps(before, indent=2))
seed = json.loads((E/'WAVE2-parent-seed.json').read_text())['files']
artifact('WAVE2-resume-source-delta.json', json.dumps({p:{'seed':v['sha256'],'current':before.get(p),'changed':v['sha256'] != before.get(p)} for p,v in seed.items()},indent=2))
qa = pathlib.Path(tempfile.mkdtemp(prefix='wave2-aggregate-',dir='/tmp'))
env = os.environ.copy()
for key, directory in {'HOME':'home','FERRYX_RUNTIME_DIR':'runtime','FERRYX_DATA_DIR':'data','FERRYX_SESSION_DIR':'sessions','XDG_CONFIG_HOME':'xdg-config','XDG_DATA_HOME':'xdg-data','XDG_CACHE_HOME':'xdg-cache','XDG_RUNTIME_DIR':'xdg-runtime','TMPDIR':'tmp'}.items():
    path=qa/directory; path.mkdir(mode=0o700); env[key]=str(path)
env.update(CARGO_HOME=os.environ.get('CARGO_HOME','/Users/indo/.cargo'),RUSTUP_HOME=os.environ.get('RUSTUP_HOME','/Users/indo/.rustup'),CARGO_TARGET_DIR=str(ROOT/'src-tauri/target'),CARGO_BUILD_JOBS='4',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_INCREMENTAL='0',RUSTC_WRAPPER='',WAVE2_QA_ROOT=str(qa))
shutil.copytree(ROOT/'ui/dist',qa/'ui/dist')
runner = str(E/'WAVE2-resume-runner.sh')
base=['cargo','--config','target.aarch64-apple-darwin.runner=["sh", "'+runner+'"]']
flags=['--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features']
commands=[
 ('integration',base+['test']+flags+sum((['--test',n] for n in ['machine_catalog_persistence','machine_worktrees','machine_sessions','machine_terminal_stream','machine_events']),[])+['--','--nocapture','--test-threads=1']),
 ('remote',base+['test']+flags+['--lib','remote::','--','--nocapture','--test-threads=1']),
 ('authority',base+['test']+flags+['--lib','daemon::session_service::machine_tests','--','--nocapture','--test-threads=1']),
 ('regressions',base+['test']+flags+sum((['--test',n] for n in ['worktree_safety','daemon_persistence_contract','daemon_handover_contract','remote_project_public_contract','relay_pairing_generation_regression','pty_input_cancellation','a10_terminal_wire_codec']),[])+['--','--nocapture','--test-threads=1']),
 ('build',base+['build']+flags+['--lib','--bin','ferryx-cli','--bin','ferryx-relay']),
]
results=[]
try:
    for name,command in commands:
        print('RUN',name,flush=True)
        started=time.time()
        completed=subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        artifact('WAVE2-resume-'+name+'.log', '$ '+__import__('shlex').join(command)+'\n'+completed.stdout+'\nEXIT='+str(completed.returncode)+'\n')
        results.append({'name':name,'command':command,'exit':completed.returncode,'seconds':time.time()-started})
        print('EXIT',name,completed.returncode,flush=True)
        if completed.returncode: break
finally:
    after=manifest()
    artifact('WAVE2-resume-source-after.json',json.dumps(after,indent=2))
    artifact('WAVE2-resume-results.json',json.dumps({'commands':results,'sourceChangesDuringRun':[p for p in before if before[p]!=after.get(p)],'qaRoot':str(qa)},indent=2))
    shutil.rmtree(qa)
    artifact('WAVE2-resume-cleanup.log','Supervisor root '+str(qa)+' removed='+str(not qa.exists())+'\nFixture-specific process and listener receipts are in command logs.\n')
