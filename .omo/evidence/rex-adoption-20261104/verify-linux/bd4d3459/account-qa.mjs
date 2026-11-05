import {spawn} from 'node:child_process';
import {mkdtempSync,mkdirSync,readdirSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';import {join,resolve} from 'node:path';import {createInterface} from 'node:readline';import {once} from 'node:events';import assert from 'node:assert/strict';
const root=mkdtempSync(join(tmpdir(),'rex-account-'));mkdirSync(join(root,'mail'));
const child=spawn(resolve('src-tauri/target/debug/ferryx-account'),[],{env:{...process.env,FERRYX_ACCOUNT_BIND:'127.0.0.1:0',FERRYX_ACCOUNT_ORIGIN:'http://127.0.0.1',FERRYX_ACCOUNT_DATA_DIR:join(root,'data'),FERRYX_MAIL_DIR:join(root,'mail'),FERRYX_DEPLOYMENT_MODE:'selfhost'},stdio:['ignore','pipe','pipe']});child.stderr.pipe(process.stderr);
try{const lines=createInterface({input:child.stdout});const base=await new Promise((res,rej)=>{const t=setTimeout(()=>rej(Error('readiness deadline')),30000);child.once('exit',c=>rej(Error('account exited '+c)));lines.on('line',l=>{console.log(l);if(l.startsWith('FERRYX_ACCOUNT_READY ')){clearTimeout(t);res('http://'+l.split(' ')[1]);}});});
async function req(path,method,body,token){const r=await fetch(base+path,{method,headers:{'Content-Type':'application/json',...(token?{Authorization:'Bearer '+token}:{})},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});const text=await r.text();console.log(method,path,r.status,text);return {status:r.status,body:JSON.parse(text)};}
assert.equal((await req('/api/v1/account/onboarding','PUT',{completedSteps:['intro'],dismissed:false})).status,401);
assert.equal((await req('/api/account/v1/login/request','POST',{email:'verifier@example.test'})).status,202);
const files=readdirSync(join(root,'mail'));assert.equal(files.length,1);const code=new URL(readFileSync(join(root,'mail',files[0]),'utf8')).searchParams.get('code');
const login=await req('/api/account/v1/login/consume','POST',{code});assert.equal(login.status,200);const token=login.body.token;assert(token);
assert.equal((await req('/api/v1/account/onboarding','PUT',{completedSteps:['intro'],dismissed:false},token)).status,200);
const got=await req('/api/v1/account/onboarding','GET',undefined,token);assert.equal(got.status,200);assert(got.body.completedSteps.includes('intro'));
assert.equal((await req('/api/v1/account/onboarding','PUT',{completedSteps:['enroll'],dismissed:false},token)).status,400);console.log('ACCOUNT_SYNC_LIVE_OK');
}finally{if(child.exitCode===null){const stopped=once(child,'exit');child.kill('SIGTERM');await stopped;}console.log('RECORDED_ACCOUNT_PID_STOPPED',child.pid);rmSync(root,{recursive:true,force:true});}
