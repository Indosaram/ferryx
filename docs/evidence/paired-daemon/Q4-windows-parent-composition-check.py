import os, pathlib, subprocess, json
root=pathlib.Path('/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8')
stage=pathlib.Path('/tmp/ferryx-herdr-q4-windows-01a097f8/parent-composition-runtime')
assert not stage.exists()
stage.mkdir()
env=os.environ.copy()
for key,name in {'HOME':'home','FERRYX_RUNTIME_DIR':'runtime','FERRYX_DATA_DIR':'data','FERRYX_SESSION_DIR':'sessions','XDG_CONFIG_HOME':'config','XDG_DATA_HOME':'data','XDG_CACHE_HOME':'cache','XDG_STATE_HOME':'state','XDG_RUNTIME_DIR':'runtime','TMPDIR':'temp','TMP':'temp','TEMP':'temp'}.items():
    p=stage/name;p.mkdir(exist_ok=True);env[key]=str(p)
env.update(CARGO_HOME='/Users/indo/.cargo',RUSTUP_HOME='/Users/indo/.rustup',CARGO_TARGET_DIR=str(root/'src-tauri/target'),CARGO_BUILD_JOBS='3',RUSTC_WRAPPER='',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_INCREMENTAL='0',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL=str(stage/'home/.gitconfig'))
base=['cargo','test','--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features']
commands=[('paths',base+['--test','q4_windows_paths','--','--nocapture']),('filesystem',base+['--lib','remote::filesystem_tests','--','--nocapture']),('worktree',base+['--lib','worktree::','--','--nocapture']),('relay-close',base+['--lib','test_relay_browser_ws_terminal_bridge_success','--','--nocapture']),('integration',base+['--test','worktree_safety','--test','machine_worktrees','--test','relay_pairing_generation_regression','--','--nocapture']),('build',['cargo','build','--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features','--lib','--bin','ferryx-cli','--bin','ferryx-relay'])]
e=root/'docs/evidence/paired-daemon';results=[]
for name,cmd in commands:
    with (e/f'Q4-windows-parent-composition-{name}.log').open('w') as log:
        log.write('COMMAND='+json.dumps(cmd)+'\n');log.flush()
        p=subprocess.Popen(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
        try: code=p.wait(timeout=1200)
        except subprocess.TimeoutExpired:
            p.terminate();code=p.wait(timeout=30)
            results.append({'name':name,'pid':p.pid,'exit':code,'timeout':True});break
        results.append({'name':name,'pid':p.pid,'exit':code,'waited':True})
    (e/'Q4-windows-parent-composition-exits.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2),flush=True)
