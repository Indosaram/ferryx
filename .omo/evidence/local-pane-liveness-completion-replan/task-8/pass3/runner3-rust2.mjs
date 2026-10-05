// Task 8 pass 3 RUST half — delta candidate (21dea3c0 + two-line surface_host repair).
import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const [root,out,platform]=process.argv.slice(2);
const PASS='3-rust';
fs.mkdirSync(path.join(out,'logs'),{recursive:true});
const recordsPath=path.join(out,'rust-commands.jsonl');
function psGate(id,exe,args){
  if(platform!=='windows') return null;
  const q=s=>"'"+String(s).replace(/'/g,"''")+"'";
  const p=path.join(out,'rgate-'+id+'.ps1');
  fs.writeFileSync(p,'$ErrorActionPreference = \'Stop\'\n& '+q(exe)+' '+args.map(q).join(' ')+'\n$native=$LASTEXITCODE\nWrite-Output "NATIVE_EXIT=$native"\nexit $native\n');
  return p;
}
function countSelected(clean){ return (clean.match(/: test\r?$/gm)||[]).length; }
async function runGate(id,exe,args,opts={}){
  const deadlineMs=opts.deadlineMs||1800000;
  const start=new Date().toISOString();
  const log=path.join(out,'logs',id+'.log');
  const fd=fs.openSync(log,'w');
  let command=exe,argv=args;
  if(platform==='windows'){const p=psGate(id,exe,args);command='powershell.exe';argv=['-NoProfile','-File',p];}
  const child=spawn(command,argv,{cwd:root,env:process.env,detached:platform!=='windows',stdio:['ignore',fd,fd]});
  const pid=child.pid; let timedOut=false;
  const hb=setInterval(()=>{try{const t=fs.readFileSync(log,'utf8');fs.appendFileSync(path.join(out,'rprogress-'+id+'.log'),JSON.stringify({at:new Date().toISOString(),bytes:t.length})+ '\n');}catch{}},30000);
  const timer=setTimeout(()=>{timedOut=true;try{const t=fs.readFileSync(log,'utf8');fs.writeFileSync(path.join(out,'rtimeout-'+id+'.json'),JSON.stringify({id,at:new Date().toISOString(),pid,bytes:t.length,tail:t.slice(-8000)},null,2));}catch{}
    if(platform==='windows'){spawn('taskkill.exe',['/PID',String(pid),'/T','/F']);}else{try{process.kill(-pid,'SIGKILL');}catch{try{child.kill('SIGKILL');}catch{}}}},deadlineMs);
  const result=await new Promise(r=>{child.on('error',e=>r({exit:null,error:String(e)}));child.on('close',(exit,signal)=>r({exit,signal}));});
  clearTimeout(timer);clearInterval(hb);fs.closeSync(fd);
  const text=fs.readFileSync(log,'utf8');
  const cap=platform==='windows'?text.match(/NATIVE_EXIT=(-?\d+)/):null;
  const nativeExit=platform==='windows'?(cap?Number(cap[1]):null):result.exit;
  const clean=text.replace(/\x1b\[[0-9;]*m/g,'');
  let selectedCount=null;
  if(id.endsWith('-list')) selectedCount = nativeExit===0 ? countSelected(clean) : null;
  else { const m=clean.match(/test result: ok\. (\d+) passed/); selectedCount=m?Number(m[1]):null; }
  const asserted=text.split(/\r?\n/).filter(s=>/test result:|running \d+ tests|Finished |built in |error\[|^error:|^Error:|NATIVE_EXIT=/.test(s));
  const status = timedOut?'TIMED_OUT':(nativeExit===0?'RAN_PASSED':'RAN_FAILED');
  const rec={pass:PASS,id,host:platform,argv:[exe,...args],cwd:root,start,end:new Date().toISOString(),pid,rawNativeExit:nativeExit,wrapperExit:result.exit,signal:result.signal||null,timedOut,selectedCount,selectionStatus:id.endsWith('-list')?(selectedCount>0?'SELECTED':(nativeExit===0?'ZERO_SELECTED':'COMPILE_FAILED')):'NOT_APPLICABLE',status,assertedLines:asserted.slice(-14),log};
  fs.appendFileSync(recordsPath,JSON.stringify(rec)+'\n');
  console.log('RGATE_DONE '+platform+' '+id+' native='+nativeExit+' count='+selectedCount+' status='+status);
  console.log(asserted.slice(-8).join('\n'));
  return rec;
}
function blocked(id,exe,args,reason,causeLog){
  const rec={pass:PASS,id,host:platform,argv:[exe,...args],cwd:root,start:new Date().toISOString(),end:new Date().toISOString(),pid:null,rawNativeExit:null,wrapperExit:null,signal:null,timedOut:false,selectedCount:null,selectionStatus:'NOT_RUN_BLOCKED',status:'NOT_RUN_BLOCKED',reason,causeLog,assertedLines:[],log:null};
  fs.appendFileSync(recordsPath,JSON.stringify(rec)+'\n');
  console.log('RGATE_NOT_RUN_BLOCKED '+platform+' '+id+' cause='+causeLog);
}
const groups=[
  {key:'lib', prereq:['local_split_reliability_',['--lib','local_split_reliability_']],
   gates:[['local_split_reliability_',['--lib','local_split_reliability_']],['pane_liveness_',['--lib','pane_liveness_']]]},
  {key:'qa', prereq:['qa_barrier',['--lib','--features','local-split-qa','qa_barrier']],
   gates:[['qa_barrier',['--lib','--features','local-split-qa','qa_barrier']]]},
  {key:'handover', prereq:['handover',['--test','daemon_handover_transfer_contract']],
   gates:[['handover',['--test','daemon_handover_transfer_contract']]]},
];
for(const g of groups){
  const [pid,pargs]=g.prereq;
  const pr=await runGate(pid+'-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml',...pargs,'--','--list'],{deadlineMs:1800000});
  if(pr.rawNativeExit!==0||!(pr.selectedCount>0)){
    for(const [id,args] of g.gates) blocked(id,'cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--nocapture','--test-threads=1'],'prerequisite compile/list gate failed for configuration '+g.key,pr.log);
    if(g.key==='lib'){
      for(const [id,exe,args] of [['all-targets','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets']],['full-lib','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','--','--test-threads=1']]])
        blocked(id,exe,args,'prerequisite compile gate failed for the default lib configuration',pr.log);
    }
    continue;
  }
  for(const [id,args] of g.gates) await runGate(id,'cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--nocapture','--test-threads=1'],{deadlineMs:1800000});
  if(g.key==='lib'){
    await runGate('all-targets','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets'],{deadlineMs:1800000});
    await runGate('full-lib','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','--','--test-threads=1'],{deadlineMs:2400000});
  }
}
// ---- integration test targets (all-targets compile coverage) ----
const INTEG=[['daemon_handover_contract'],['daemon_persistence_contract'],['zero_config_gen4_audit']];
for(const [name] of INTEG){
  const lr=await runGate(name+'-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test',name,'--','--list'],{deadlineMs:1800000});
  if(lr.rawNativeExit!==0||!(lr.selectedCount>0)){
    blocked(name,'cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test',name,'--','--nocapture','--test-threads=1'],'integration target list gate failed for '+name,lr.log);
    continue;
  }
  await runGate(name,'cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test',name,'--','--nocapture','--test-threads=1'],{deadlineMs:1800000});
}
if(platform==='linux'){
  const ex=await runGate('unix-suspension-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--list'],{deadlineMs:1800000});
  if(ex.rawNativeExit===0&&ex.selectedCount>0){await runGate('unix-suspension','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--nocapture','--test-threads=1'],{deadlineMs:1800000});}
  else blocked('unix-suspension','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--nocapture','--test-threads=1'],'suspension list gate did not select cases',ex.log);
  const jr=await runGate('journal-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--list'],{deadlineMs:1800000});
  if(jr.rawNativeExit===0&&jr.selectedCount>0){await runGate('journal','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--nocapture','--test-threads=1'],{deadlineMs:1800000});}
  else blocked('journal','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--nocapture','--test-threads=1'],'split_journal list gate did not select cases',jr.log);
}
console.log('ALL_RUST_GATES_DONE '+platform);
