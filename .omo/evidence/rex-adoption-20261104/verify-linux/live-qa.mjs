import {spawn,execFileSync} from 'node:child_process';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,existsSync,rmSync,watch} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {once} from 'node:events';
import net from 'node:net';
import assert from 'node:assert/strict';
const bin=resolve('src-tauri/target/debug/ferryx-cli');
const root=mkdtempSync(join(tmpdir(),'rex-independent-'));
const env={...process.env,HOME:join(root,'home'),XDG_CONFIG_HOME:join(root,'config'),FERRYX_RUNTIME_DIR:join(root,'rt'),FERRYX_DATA_DIR:join(root,'data')};
for(const p of ['home/.claude','home/.codex','home/.omp','rt','data','config/ferryx'])mkdirSync(join(root,p),{recursive:true});
function cli(...args){return execFileSync(bin,args,{env,encoding:'utf8',timeout:20000});}
const children=[];
try{
 console.log('INTEGRATIONS_INSTALL',cli('integrations','skill','install','--all'));
 console.log('INTEGRATIONS_STATUS',cli('integrations','status','--json'));
 const skill=join(env.HOME,'.claude/skills/ferryx/SKILL.md');
 assert(existsSync(skill));
 const before=readFileSync(skill,'utf8');cli('integrations','skill','install','--all');assert.equal(readFileSync(skill,'utf8'),before);
 writeFileSync(skill,before.replace(/^.*ferryx-managed: true.*\n/m,'')+'\nuser takeover\n');
 cli('integrations','skill','uninstall','--all');assert(readFileSync(skill,'utf8').includes('user takeover'));console.log('USER_EDIT_PRESERVED');
 try{cli('integrations','skill','install','nosuchagent');throw Error('unknown agent accepted');}catch(e){assert(e.status!==0);assert(String(e.stderr).includes('UNKNOWN_AGENT'));console.log('UNKNOWN_AGENT_REJECTED',e.status);}
 writeFileSync(join(root,'data/settings.json'),JSON.stringify({scripting:{enabled:true}}));
 writeFileSync(join(root,'config/ferryx/init.luau'),'ferryx.log("verifier startup")');
 const child=spawn(bin,['--daemon'],{env,stdio:['ignore','pipe','pipe']});children.push(child);child.stderr.pipe(process.stderr);
 const lines=createInterface({input:child.stdout});
 await new Promise((res,rej)=>{const t=setTimeout(()=>rej(Error('readiness timeout')),30000);child.once('exit',c=>rej(Error('daemon exited '+c)));lines.on('line',l=>{console.log('DAEMON',l);if(l.includes('FERRYX_DAEMON_READY')){clearTimeout(t);res();}});});
 console.log('SCRIPT_LOG',cli('script','log'));
 const socket=net.createConnection(join(root,'rt/daemon.sock'));await once(socket,'connect');const replies=createInterface({input:socket});
 async function call(obj){const p=once(replies,'line',{signal:AbortSignal.timeout(15000)});socket.write(JSON.stringify(obj)+'\n');return JSON.parse((await p)[0]);}
 assert.equal((await call({type:'handshake',version:5})).type,'handshakeOk');
 console.log('LAYOUT_QA_BEGIN');
 const run=spawn('bun',['scripts/qa/layout-intents.mjs','--runtime',env.FERRYX_RUNTIME_DIR],{env,stdio:'inherit'});const [code]=await once(run,'exit');console.log('LAYOUT_QA_EXIT',code);
 writeFileSync(join(root,'config/ferryx/init.luau'),'ferryx.exec("sh -c rm")');console.log('SCRIPT_BAD_ARGUMENT_RELOAD',await call({type:'script',action:{action:'reload'}}));console.log('SCRIPT_BAD_ARGUMENT_LOG',cli('script','log'));
 writeFileSync(join(root,'config/ferryx/init.luau'),'while true do end');console.log('SCRIPT_INFINITE_RELOAD',await call({type:'script',action:{action:'reload'}}));console.log('SCRIPT_INFINITE_LOG',cli('script','log'));console.log('RESPONSIVE',cli('agents','list','--json'));
 socket.destroy();
}finally{for(const c of children){if(c.exitCode===null){const stopped=once(c,'exit');c.kill('SIGTERM');await stopped;console.log('RECORDED_PID_STOPPED',c.pid);}}rmSync(root,{recursive:true,force:true});}
