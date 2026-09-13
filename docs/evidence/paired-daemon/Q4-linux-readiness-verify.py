import pathlib,json,subprocess,hashlib
R=pathlib.Path('/home/indo/ferryx-herdr-q4-linux-01a097f8');E=json.loads((R/'readiness-environment.json').read_text());Q=R/'readiness-qa';assert not Q.exists();Q.mkdir(mode=0o700)
for k,v in E.items():
 if v.startswith(str(Q)):pathlib.Path(v).mkdir(parents=True,exist_ok=True,mode=0o700)
m=json.loads((R/'readiness-source-hashes.json').read_text())
for p,v in m.items():assert hashlib.sha256((R/'source'/p).read_bytes()).hexdigest()==v['after']
with open(R/'readiness-diagnostics.log','w') as f:
 p=subprocess.run(['rust-analyzer','--version'],env=E,stdout=f,stderr=subprocess.STDOUT);print('EXIT',p.returncode,file=f)
base=['--locked','--manifest-path','src-tauri/Cargo.toml','--no-default-features'];results=[]
for label,cmd in [('remote',['cargo','test',*base,'--lib','remote::','--','--nocapture']),('catalog',['cargo','test',*base,'--test','machine_catalog_persistence','--','--nocapture']),('build',['cargo','build',*base,'--lib','--bin','ferryx-cli','--bin','ferryx-relay'])]:
 with open(R/('readiness-green-'+label+'.log'),'w') as f:
  print('COMMAND',json.dumps(cmd),'SOURCE',json.dumps(m),file=f,flush=True)
  p=subprocess.Popen(cmd,cwd=R/'source',env=E,stdout=f,stderr=subprocess.STDOUT);print('COMMAND_PID',p.pid,file=f,flush=True);code=p.wait(timeout=600);print('COMMAND_EXIT',code,file=f,flush=True)
 results.append({'label':label,'command':cmd,'pid':p.pid,'exit':code});(R/'readiness-results.json').write_text(json.dumps(results,indent=2)+'\n')
print('MONITOR_COMPLETE',json.dumps(results),flush=True)
