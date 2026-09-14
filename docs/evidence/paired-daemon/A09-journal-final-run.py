import os, tempfile, subprocess, sys, pathlib
os.umask(0o077)
repo=pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8')
root=pathlib.Path(tempfile.mkdtemp(prefix='a09-final-'))
env={'PATH':'/Users/indo/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin','CARGO_HOME':'/Users/indo/.cargo','RUSTUP_HOME':'/Users/indo/.rustup','CARGO_TARGET_DIR':str(repo/'src-tauri/target'),'CARGO_BUILD_JOBS':'2','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','CARGO_INCREMENTAL':'0','RUSTC_WRAPPER':'','SHELL':'/bin/sh'}
for key in ['HOME','FERRYX_RUNTIME_DIR','FERRYX_DATA_DIR','FERRYX_SESSION_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','TMPDIR']:
 p=root/key;p.mkdir();env[key]=str(p)
env['FERRYX_AGENT_STATE_SOCKET']=str(root/'agent.sock')
log=repo/'docs/evidence/paired-daemon'/('A09-journal-final-'+sys.argv[1]+'.log')
cmd=['cargo','test','--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features','--lib',sys.argv[2],'--','--nocapture']
with log.open('w') as out:
 out.write(f'supervisor={os.getpid()} root={root} command={cmd}\n');out.flush()
 child=subprocess.Popen(cmd,cwd=repo,env=env,stdout=out,stderr=subprocess.STDOUT)
 out.write(f'owned_cargo_pid={child.pid}\n');out.flush()
 try: code=child.wait(timeout=240)
 except subprocess.TimeoutExpired:
  child.terminate();code=child.wait(timeout=30)
 out.write(f'exit={code} cargo_reaped=true\n')
 for p in root.iterdir():
  if p.is_dir() and not list(p.iterdir()):p.rmdir()
 out.write(f'remaining={list(root.iterdir())}\n')
 if not list(root.iterdir()):root.rmdir();out.write('supervisor_root_removed=true\n')
log.with_suffix('.exit').write_text(str(code)+'\n')
print(log,code)
sys.exit(code)
