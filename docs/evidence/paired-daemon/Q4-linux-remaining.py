import pathlib,json,subprocess
r=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8'); env=json.loads((r/'environment.json').read_text())
cmd=['cargo','test','--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features','--test','machine_worktrees','--test','worktree_safety','--test','machine_worktree_legacy_bounds','--test','machine_worktree_transports','--test','relay_pairing_generation_regression','--','--nocapture']
with open(r/'remaining-integration.log','w') as f:
 print('COMMAND',json.dumps(cmd),'ONLY_PREVIOUSLY_UNEXECUTED_TARGETS',file=f,flush=True)
 p=subprocess.Popen(cmd,cwd=r/'source',env=env,stdout=f,stderr=subprocess.STDOUT);print('COMMAND_PID',p.pid,file=f,flush=True)
 code=p.wait(timeout=600);print('COMMAND_EXIT',code,file=f,flush=True)
(r/'remaining-results.json').write_text(json.dumps({'command':cmd,'pid':p.pid,'exit':code},indent=2)+'\n')
