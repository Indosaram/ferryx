import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const [root,out,platform]=process.argv.slice(2);
fs.mkdirSync(path.join(out,'logs'),{recursive:true});
const gates=[['ui-build','bun',['run','--cwd','ui','build']],['ui-split','bun',['run','--cwd','ui','test','src/lib/localSplitLifecycle.test.ts']],['ui-lifecycle','bun',['run','--cwd','ui','test','src/lib/sessionPersistence.test.ts','src/lib/sessionLifecycle.test.ts','src/lib/nativeTerminalLifecycle.test.ts','src/components/NativeTerminalPane.lifecycle.test.tsx']],['runner','bun',['run','--cwd','ui','test','--config','../scripts/qa/pane-liveness-vitest.config.mjs']]];
const filters=[['local_split_reliability_', ['--lib','local_split_reliability_']],['pane_liveness_', ['--lib','pane_liveness_']],['qa_barrier',['--lib','--features','local-split-qa','qa_barrier']],['handover',['--test','daemon_handover_transfer_contract']]];
for(const [id,args] of filters){gates.push([id+'-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--list']]);gates.push([id,'cargo',['test','--manifest-path','src-tauri/Cargo.toml',...args,'--','--nocapture','--test-threads=1']]);}
gates.push(['all-targets','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets']],['full-lib','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','--','--test-threads=1']],['full-ui','bun',['run','--cwd','ui','test']]);
if(platform==='linux') gates.push(['unix-suspension-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--list']],['unix-suspension','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','suspension','--','--nocapture','--test-threads=1']],['journal-list','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--list']],['journal','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--lib','split_journal','--','--nocapture','--test-threads=1']]);
for(const [id,exe,args] of gates){
 if(platform === "mac" && gates.findIndex(g => g[0] === id) < 4) continue;
 const start=new Date().toISOString(); console.log('START '+platform+' '+id+' '+JSON.stringify([exe,...args]));
 const log=path.join(out,'logs',id+'.log'); const fd=fs.openSync(log,'w'); let command=exe,argv=args;
 if(platform==='windows'){command='powershell.exe';argv=['-NoProfile','-File',path.join(out,'gate-'+id+'.ps1')];}
 const child=spawn(command,argv,{cwd:root,env:process.env,detached:platform!=='windows',stdio:['ignore',fd,fd]});
 const pid=child.pid; let timedOut=false;
 const timer=setTimeout(()=>{timedOut=true;console.log('TIMEOUT '+id+' pid='+pid);if(platform==='windows')spawn('taskkill.exe',['/PID',String(pid),'/T','/F']);else try{process.kill(-pid,'SIGKILL');}catch{}}, id==='full-lib'||id==='full-ui'?1200000:900000);
 const result=await new Promise(resolve=>{child.on('error',e=>resolve({exit:null,error:String(e)}));child.on('close',(exit,signal)=>resolve({exit,signal}));});clearTimeout(timer);fs.closeSync(fd);
 const text=fs.readFileSync(log,'utf8');const capture=platform==='windows'?text.match(/NATIVE_EXIT=(-?\d+)/):null;
 const nativeExit=platform==='windows'?(capture?Number(capture[1]):null):result.exit;
 const count=id.endsWith('-list')?(text.match(/: test\r?$/gm)||[]).length:null;
 const asserted=text.split(/\r?\n/).filter(s=>/test result:|Test Files|Tests |Finished |built in |error\[|error:|Error:|NATIVE_EXIT=/.test(s));
 const record={id,host:platform,argv:[exe,...args],cwd:root,start,end:new Date().toISOString(),pid,rawNativeExit:nativeExit,wrapperExit:result.exit,signal:result.signal,timedOut,selectedCount:count,assertedLines:asserted,log};
 fs.appendFileSync(path.join(out,'commands.jsonl'),JSON.stringify(record)+'\n');console.log('GATE_DONE '+platform+' '+id+' native='+nativeExit+' count='+count);console.log(asserted.slice(-12).join('\n'));
}
console.log('ALL_GATES_DONE '+platform);
