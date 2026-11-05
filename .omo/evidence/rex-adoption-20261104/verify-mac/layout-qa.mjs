import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,mkdir,rm} from 'node:fs/promises';
import {createInterface} from 'node:readline';
import net from 'node:net';
import assert from 'node:assert/strict';
const binary=process.env.FERRYX_CLI;
const root=await mkdtemp('/tmp/rex-mac-layout-');
const rt=root+'/rt'; await mkdir(rt);
const child=spawn(binary,['--daemon'],{env:{...process.env,FERRYX_RUNTIME_DIR:rt,FERRYX_DATA_DIR:root+'/data'},stdio:['ignore','pipe','pipe']});
child.stderr.pipe(process.stderr);
console.log(JSON.stringify({root,pid:child.pid}));
const signal=AbortSignal.timeout(90000);
const lines=createInterface({input:child.stdout});
const ready=new Promise((resolve,reject)=>{lines.on('line',line=>{if(line==='FERRYX_DAEMON_READY')resolve();});child.once('exit',c=>reject(Error('daemon exit '+c)));signal.addEventListener('abort',()=>reject(signal.reason),{once:true});});
try {await ready;const qa=spawn('bun',['scripts/qa/layout-intents.mjs','--runtime',rt],{stdio:'inherit'});const [code]=await once(qa,'exit',{signal});assert.equal(code,0);console.log('LAYOUT_EXIT=0');}
finally {const exit=once(child,'exit');child.kill('SIGTERM');await exit;await rm(root,{recursive:true,force:true});console.log('ISOLATED_DAEMON_CLEANED');}
