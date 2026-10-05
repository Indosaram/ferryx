// Task 8 pass 3 host runner — candidate 21dea3c01d1ec423498bc48eb2d29107be75eddf
import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const [root,out,platform]=process.argv.slice(2);
const COMMIT='21dea3c01d1ec423498bc48eb2d29107be75eddf';
const PASS=3; const PHASE='resume';
fs.mkdirSync(path.join(out,'logs'),{recursive:true});
const recordsPath=path.join(out,'commands.jsonl');
const manifestPath=path.join(out,'host-manifest.json');

function psGate(id,exe,args){
  if(platform!=='windows') return null;
  const q=s=>"'"+String(s).replace(/'/g,"''")+"'";
  const p=path.join(out,'gate-'+id+'.ps1');
  fs.writeFileSync(p,'$ErrorActionPreference = \'Stop\'\n& '+q(exe)+' '+args.map(q).join(' ')+'\n$native=$LASTEXITCODE\nWrite-Output "NATIVE_EXIT=$native"\nexit $native\n');
  return p;
}
const UI_LINE=/^\s*[\u2713\u00d7\u2757\u276f\u2795]\s+(\S+\.(test|spec)\.(tsx?|mts))|^\s*[\u2713\u00d7]\s+(\S+)/;
function lastFileLine(text){
  const lines=text.split(/\r?\n/);
  for(let i=lines.length-1;i>=0;i--){const l=lines[i].trim(); if(/^[\u2713\u00d7]\s+\S/.test(l)) return l;}
  return null;
}
function countSelected(clean){
  return (clean.match(/: test\r?$/gm)||[]).length;
}
async function runGate(id,exe,args,opts={}){
  const deadlineMs=opts.deadlineMs||900000;
  const start=new Date().toISOString();
  const log=path.join(out,'logs',id+'.log');
  const fd=fs.openSync(log,'w');
  let command=exe,argv=args;
  if(platform==='windows'){const p=psGate(id,exe,args);command='powershell.exe';argv=['-NoProfile','-File',p];}
  const child=spawn(command,argv,{cwd:root,env:process.env,detached:platform!=='windows',stdio:['ignore',fd,fd]});
  const pid=child.pid;
  let timedOut=false;
  const heartbeat=setInterval(()=>{
    try{
      const text=fs.readFileSync(log,'utf8');
      const lf=lastFileLine(text);
      fs.appendFileSync(path.join(out,'progress-'+id+'.log'),JSON.stringify({at:new Date().toISOString(),bytes:text.length,lastFile:lf})+ '\n');
    }catch{}
  },30000);
  const timer=setTimeout(()=>{
    timedOut=true;
    try{
      const text=fs.readFileSync(log,'utf8');
      fs.writeFileSync(path.join(out,'timeout-'+id+'.json'),JSON.stringify({id,at:new Date().toISOString(),pid,bytes:text.length,lastFile:lastFileLine(text),tail:text.slice(-8000)},null,2));
      if(platform!=='windows'){
        const lsof=spawn('lsof',['-p',String(pid)]);let s='';lsof.stdout.on('data',d=>s+=d);lsof.on('close',()=>{try{fs.writeFileSync(path.join(out,'timeout-'+id+'-lsof.txt'),s);}catch{}});
        const smp=spawn('sample',[String(pid),'3','-f',path.join(out,'timeout-'+id+'-sample.txt')]);smp.on('error',()=>{});
      }
    }catch{}
    if(platform==='windows'){spawn('taskkill.exe',['/PID',String(pid),'/T','/F']);}
    else {try{process.kill(-pid,'SIGKILL');}catch{try{child.kill('SIGKILL');}catch{}}}
  },deadlineMs);
  const result=await new Promise(resolve=>{child.on('error',e=>resolve({exit:null,error:String(e)}));child.on('close',(exit,signal)=>resolve({exit,signal}));});
  clearTimeout(timer);clearInterval(heartbeat);fs.closeSync(fd);
  const text=fs.readFileSync(log,'utf8');
  const capture=platform==='windows'?text.match(/NATIVE_EXIT=(-?\d+)/):null;
  const nativeExit=platform==='windows'?(capture?Number(capture[1]):null):result.exit;
  const clean=text.replace(/\x1b\[[0-9;]*m/g,'');
  let selectedCount=null;
  if(id.endsWith('-list')) selectedCount = nativeExit===0 ? countSelected(clean) : null;
  else if(/\.mjs$|^runner$/.test(id)||opts.ui) { const m=clean.match(/Tests\s+(?:\d+ failed \| )?(\d+) passed \((\d+)\)/); const m2=clean.match(/Tests\s+(\d+) failed[^(]*\((\d+)\)/); selectedCount = m?Number(m[2]):(m2?Number(m2[2]):(/No test files found/.test(clean)?0:null)); }
  else { const m=clean.match(/test result: ok\. (\d+) passed/); selectedCount=m?Number(m[1]):null; }
  const asserted=text.split(/\r?\n/).filter(s=>/test result:|Test Files|Tests |Finished |built in |error\[|^error:|^Error:|NATIVE_EXIT=/.test(s));
  const status = timedOut?'TIMED_OUT':(nativeExit===0?'RAN_PASSED':'RAN_FAILED');
  const rec={pass:PASS,commit:COMMIT,id,host:platform,argv:[exe,...args],cwd:root,start,end:new Date().toISOString(),pid,rawNativeExit:nativeExit,wrapperExit:result.exit,signal:result.signal||null,timedOut,selectedCount,selectionStatus:id.endsWith('-list')?(selectedCount>0?'SELECTED':(nativeExit===0?'ZERO_SELECTED':'UNKNOWN_COMPILE_OR_FAILED')):'NOT_APPLICABLE',status,assertedLines:asserted.slice(-16),log};
  fs.appendFileSync(recordsPath,JSON.stringify(rec)+'\n');
  console.log('GATE_DONE '+platform+' '+id+' native='+nativeExit+' count='+selectedCount+' status='+status);
  console.log(asserted.slice(-10).join('\n'));
  return rec;
}
function blocked(id,exe,args,reason,causeLog){
  const rec={pass:PASS,commit:COMMIT,id,host:platform,argv:[exe,...args],cwd:root,start:new Date().toISOString(),end:new Date().toISOString(),pid:null,rawNativeExit:null,wrapperExit:null,signal:null,timedOut:false,selectedCount:null,selectionStatus:'NOT_RUN_BLOCKED',status:'NOT_RUN_BLOCKED',reason,causeLog,assertedLines:[],log:null};
  fs.appendFileSync(recordsPath,JSON.stringify(rec)+'\n');
  console.log('GATE_NOT_RUN_BLOCKED '+platform+' '+id+' cause='+causeLog);
}

const envInfo={platform,host:(await import('node:os')).hostname(),commit:COMMIT,root,out,startedAt:new Date().toISOString(),cargoTargetDir:process.env.CARGO_TARGET_DIR||null,node:process.version};
fs.writeFileSync(manifestPath,JSON.stringify(envInfo,null,2));

// ---- RESUME: UI build/scoped/runner already recorded; only Rust + full-ui remain ----
if(false){
const uiBuild=await runGate('ui-build','bun',['run','--cwd','ui','build'],{deadlineMs:900000});
if(uiBuild.rawNativeExit!==0){
  const dep=[['ui-split','bun',['run','--cwd','ui','test','src/lib/localSplitLifecycle.test.ts']],
    ['ui-lifecycle','bun',['run','--cwd','ui','test','src/lib/sessionPersistence.test.ts','src/lib/sessionLifecycle.test.ts','src/lib/nativeTerminalLifecycle.test.ts','src/components/NativeTerminalPane.lifecycle.test.tsx']],
    ['runner','bun',['run','--cwd','ui','test','--config','../scripts/qa/pane-liveness-vitest.config.mjs']]];
  for(const [id,exe,args] of dep) blocked(id,exe,args,'UI build failed; dependent gate not run',uiBuild.log);
  console.log('DEPENDENT_GATES_NOT_RUN '+platform+' UI_BUILD_FAILED');
} else {
  await runGate('ui-split','bun',['run','--cwd','ui','test','src/lib/localSplitLifecycle.test.ts'],{deadlineMs:900000,ui:true});
  await runGate('ui-lifecycle','bun',['run','--cwd','ui','test','src/lib/sessionPersistence.test.ts','src/lib/sessionLifecycle.test.ts','src/lib/nativeTerminalLifecycle.test.ts','src/components/NativeTerminalPane.lifecycle.test.tsx'],{deadlineMs:900000,ui:true});
  await runGate('runner','bun',['run','--cwd','ui','test','--config','../scripts/qa/pane-liveness-vitest.config.mjs'],{deadlineMs:900000,ui:true});
}

}
// ---- Rust groups: one compile prerequisite per distinct configuration ----
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
  const pr=await runGate(pid+'-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml',...pargs,'--','--list'],{deadlineMs:900000});
  if(pr.rawNativeExit!==0||!(pr.selectedCount>0)){
    for(const [id,args] of g.gates) blocked(id,'cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--nocapture','--test-threads=1'],'prerequisite compile/list gate failed for configuration '+g.key,pr.log);
    if(g.key==='lib'){
      for(const [id,exe,args] of [['all-targets','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets']],['full-lib','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','--','--test-threads=1']]])
        blocked(id,exe,args,'prerequisite compile gate failed for the default lib configuration',pr.log);
    }
    continue;
  }
  for(const [id,args] of g.gates) await runGate(id,'cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--nocapture','--test-threads=1'],{deadlineMs:1200000});
  if(g.key==='lib'){
    await runGate('all-targets','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets'],{deadlineMs:1200000});
    await runGate('full-lib','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','--','--test-threads=1'],{deadlineMs:1800000});
  }
}
if(platform==='linux'){
  const extra=await runGate('unix-suspension-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--list'],{deadlineMs:900000});
  if(extra.rawNativeExit===0&&extra.selectedCount>0){await runGate('unix-suspension','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--nocapture','--test-threads=1'],{deadlineMs:900000});}
  else blocked('unix-suspension','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--nocapture','--test-threads=1'],'suspension list gate did not select cases',extra.log);
  const jr=await runGate('journal-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--list'],{deadlineMs:900000});
  if(jr.rawNativeExit===0&&jr.selectedCount>0){await runGate('journal','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--nocapture','--test-threads=1'],{deadlineMs:900000});}
  else blocked('journal','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--nocapture','--test-threads=1'],'split_journal list gate did not select cases',jr.log);
}
// ---- full UI last ----
await runGate('full-ui','bun',['run','--cwd','ui','test'],{deadlineMs:2400000,ui:true});
console.log('ALL_GATES_DONE '+platform);
