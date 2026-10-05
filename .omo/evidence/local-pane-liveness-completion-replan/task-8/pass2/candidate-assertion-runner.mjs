import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const [root,out,host,...files]=process.argv.slice(2);
fs.mkdirSync(path.join(out,'logs'),{recursive:true});
for(const file of files){
const argv=['run','--cwd','ui','test',file];
const id=file.replaceAll('/','_');
const log=path.join(out,'logs',id+'.log');
const fd=fs.openSync(log,'w');
const start=new Date().toISOString();
let exe='bun',args=argv;
if(host==='windows'){exe='powershell.exe';args=['-NoProfile','-File',path.join(out,'..','ab-file-windows.ps1'),file];}
console.log('ASSERT_START '+host+' '+file);
const child=spawn(exe,args,{cwd:root,detached:host!=='windows',stdio:['ignore',fd,fd]});
let timedOut=false;
const timer=setTimeout(()=>{timedOut=true;console.log('ASSERT_TIMEOUT '+file+' pid='+child.pid);if(host==='windows')spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F']);else process.kill(-child.pid,'SIGKILL');},600000);
const result=await new Promise(resolve=>child.on('close',(exit,signal)=>resolve({exit,signal})));
clearTimeout(timer);fs.closeSync(fd);
const text=fs.readFileSync(log,'utf8').replace(/\x1b\[[0-9;]*m/g,'');
const native=host==='windows'?Number(text.match(/NATIVE_EXIT=(-?\d+)/)?.[1]??NaN):result.exit;
const assertedLines=text.split(/\r?\n/).filter(x=>/Test Files|Tests |FAIL |AssertionError|TypeError|Error:|NATIVE_EXIT/.test(x));
const selectedCount=Number(text.match(/Tests\s+[^\n]*\((\d+)\)/)?.[1]??NaN);
fs.appendFileSync(path.join(out,'commands.jsonl'),JSON.stringify({pass:2,kind:'candidateAssertionRecovery',commit:'172baa874f5e320ef08f4ed1dc5f11b391898477',host,file,argv:['bun',...argv],start,end:new Date().toISOString(),pid:child.pid,rawNativeExit:native,wrapperExit:result.exit,timedOut,selectedCount:Number.isFinite(selectedCount)?selectedCount:null,assertedLines,log})+'\n');
console.log('ASSERT_DONE '+host+' '+file+' native='+native);console.log(assertedLines.slice(-8).join('\n'));
}
console.log('ALL_ASSERTIONS_DONE '+host);


