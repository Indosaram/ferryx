import os, pathlib, tempfile, subprocess, json, shutil, hashlib
repo = pathlib.Path(__file__).resolve().parents[3]
os.chdir(repo)
root = pathlib.Path(tempfile.mkdtemp(prefix='a11-followthrough-clean-', dir='/tmp'))
env = {k:v for k,v in os.environ.items() if not k.startswith('FERRYX_')}
for k,n in {'HOME':'home','FERRYX_RUNTIME_DIR':'run','FERRYX_DATA_DIR':'data','FERRYX_SESSION_DIR':'sessions','XDG_CONFIG_HOME':'config','XDG_DATA_HOME':'xdgdata','XDG_CACHE_HOME':'cache','TMPDIR':'tmp','FERRYX_AGENT_STATE_SOCKET':'agent/state.sock'}.items():
    p=root/n
    (p.parent if k.endswith('SOCKET') else p).mkdir(parents=True,exist_ok=True)
    env[k]=str(p)
env.update(CARGO_HOME='/Users/indo/.cargo',RUSTUP_HOME='/Users/indo/.rustup',CARGO_TARGET_DIR=str(repo/'src-tauri/target'),CARGO_BUILD_JOBS='2',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_INCREMENTAL='0',RUSTC_WRAPPER='')
env['DYLD_LIBRARY_PATH']=str(next((repo/'src-tauri/target').rglob('libghostty-vt.dylib')).parent)
evidence=repo/'docs/evidence/paired-daemon'
common=['--locked','--manifest-path',str(repo/'src-tauri/Cargo.toml'),'--no-default-features']
commands=[['cargo','test']+common+['--test','relay_pairing_generation_regression','--','--nocapture','--test-threads=1'],['cargo','test']+common+['--lib','remote::relay_server::tests::a11_expired_ticket_wire_boundary','--','--nocapture'],['cargo','check']+common+['--bin','ferryx-cli','--bin','ferryx-relay'],['cargo','build']+common+['--bin','ferryx-cli','--bin','ferryx-relay']]
paths=['src-tauri/src/remote/relay_server.rs','src-tauri/src/remote/relay_client.rs','src-tauri/tests/relay_pairing_generation_regression.rs']
def hashes(): return {p:hashlib.sha256((repo/p).read_bytes()).hexdigest() for p in paths}
receipt={'before':hashes(),'environment':{k:v for k,v in env.items() if k.startswith(('FERRYX_','CARGO_','XDG_','RUSTUP_')) or k in ['HOME','TMPDIR','RUSTC_WRAPPER','DYLD_LIBRARY_PATH']},'commands':[]}
try:
    with (evidence/'A11-followthrough-owned-isolated.log').open('w') as log:
        for command in commands:
            log.write('\nCOMMAND='+repr(command)+'\n');log.flush()
            result=subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT)
            log.write('\nEXIT='+str(result.returncode)+'\n');log.flush()
            receipt['commands'].append({'command':command,'exit':result.returncode})
            if result.returncode: break
finally:
    receipt['after']=hashes()
    shutil.rmtree(root)
    receipt['supervisorRemoved']=not root.exists()
    (evidence/'A11-followthrough-owned-isolated.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt['commands'],indent=2))
