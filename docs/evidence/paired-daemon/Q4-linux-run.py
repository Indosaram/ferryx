import os, pathlib, subprocess, json, time, traceback
r=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8')
qa=r/'qa'; qa.mkdir(mode=0o700)
env={'PATH':'/home/indo/.cargo/bin:/home/indo/.bun/bin:/usr/local/bin:/usr/bin:/bin','CARGO_HOME':'/home/indo/.cargo','RUSTUP_HOME':'/home/indo/.rustup','CARGO_TARGET_DIR':str(r/'target'),'CARGO_BUILD_JOBS':'3','RUSTC_WRAPPER':'','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','CARGO_INCREMENTAL':'0','GIT_CONFIG_NOSYSTEM':'1','GIT_CONFIG_GLOBAL':'/dev/null','LANG':'C.UTF-8','RUST_TEST_THREADS':'1'}
for key,part in {'HOME':'home','FERRYX_RUNTIME_DIR':'runtime','FERRYX_DATA_DIR':'data','FERRYX_SESSION_DIR':'sessions','XDG_CONFIG_HOME':'config','XDG_DATA_HOME':'xdg-data','XDG_CACHE_HOME':'cache','XDG_STATE_HOME':'state','XDG_RUNTIME_DIR':'xdg-runtime','TMPDIR':'tmp','TMP':'tmp','TEMP':'tmp'}.items():
 p=qa/part;p.mkdir(mode=0o700,exist_ok=True);env[key]=str(p)
(r/'environment.json').write_text(json.dumps(env,indent=2)+'\n')
base=['--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features']
commands=[('remote',['cargo','test',*base,'--lib','remote::','--','--nocapture']),('integration',['cargo','test',*base,*sum((['--test',x] for x in ['machine_catalog_persistence','machine_worktrees','worktree_safety','machine_worktree_legacy_bounds','machine_worktree_transports','relay_pairing_generation_regression']),[]),'--','--nocapture']),('worktree',['cargo','test',*base,'--lib','worktree::','--','--nocapture']),('build',['cargo','build',*base,'--lib','--bin','ferryx-cli','--bin','ferryx-relay'])]
results=[]
try:
 with open(r/'diagnostics.log','w') as f:
  result=subprocess.run(['rust-analyzer','--version'],env=env,stdout=f,stderr=subprocess.STDOUT)
  print('LSP_TOOL_EXIT',result.returncode,file=f)
 for label,cmd in commands:
  with open(r/(label+'.log'),'w') as f:
   print('COMMAND',json.dumps(cmd),'SNAPSHOT_MANIFEST_SHA256',__import__('hashlib').sha256((r/'snapshot-manifest.json').read_bytes()).hexdigest(),file=f,flush=True)
   p=subprocess.Popen(cmd,cwd=r/'source',env=env,stdout=f,stderr=subprocess.STDOUT)
   print('COMMAND_PID',p.pid,file=f,flush=True)
   code=p.wait(timeout=1800)
   print('COMMAND_EXIT',code,file=f,flush=True)
  results.append({'label':label,'command':cmd,'pid':p.pid,'exit':code})
  (r/'results.json').write_text(json.dumps(results,indent=2)+'\n')
  if code and ('could not compile' in (r/(label+'.log')).read_text() or 'failed to run custom build command' in (r/(label+'.log')).read_text()): break
except BaseException:
 traceback.print_exc()
 raise
finally:
 print('MONITOR_COMPLETE',json.dumps(results),flush=True)
