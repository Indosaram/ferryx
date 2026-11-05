import net from 'node:net';
import {createInterface} from 'node:readline';
import {once} from 'node:events';
import {readFileSync,renameSync,writeFileSync,watch,existsSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
const root='C:\\Users\\sook\\ferryx-wt\\rex-verify-win-bd4d3459';
const qa=root+'\\.qa\\pipe',logs=root+'\\.omo\\evidence\\rex-adoption-20261104\\verify-win\\bd4d3459';
const sid=execFileSync('powershell',['-NoProfile','-Command','[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value'],{encoding:'utf8'}).trim();
const socket=net.createConnection('\\\\.\\pipe\\ferryx-'+sid+'-daemon-dev');
await once(socket,'connect',{signal:AbortSignal.timeout(15000)});const lines=createInterface({input:socket});
async function call(req){const pending=once(lines,'line',{signal:AbortSignal.timeout(15000)});socket.write(JSON.stringify(req)+'\n');const reply=JSON.parse((await pending)[0]);console.log(JSON.stringify({request:req,response:reply}));return reply;}
try {
 await call({type:'handshake',version:5});
 const registered=await call({type:'registerWorkspace',workspaceId:'qa-win',repoRoot:root});
 if(registered.type==='error') {
  renameSync(qa+'\\\\rt\\\\daemon.port',qa+'\\\\rt\\\\daemon.port.hidden');
  console.log('PIPE_ONLY_PORT_FILE_ABSENT '+execFileSync(qa+'\\\\bin\\\\ferryx-cli.exe',['agents','list','--json'],{encoding:'utf8',env:{...process.env,HOME:qa+'\\\\home',USERPROFILE:qa+'\\\\home',FERRYX_RUNTIME_DIR:qa+'\\\\rt',FERRYX_DATA_DIR:qa+'\\\\data'}}));
  throw new Error('OWNER_REGISTER_WORKSPACE_REFUSED: '+registered.message);
 }
 const a=await call({type:'spawn',clientRequestId:'verify-win-a',workspaceId:'qa-win',worktree:null,cwd:root,cols:80,rows:24,shell:'C:\\Windows\\System32\\cmd.exe'});
 const b=await call({type:'spawn',clientRequestId:'verify-win-b',workspaceId:'qa-win',worktree:null,cwd:root,cols:80,rows:24,shell:'C:\\Windows\\System32\\cmd.exe'});
 writeFileSync(logs+'\\qa-session-ids.json',JSON.stringify([a.sessionId,b.sessionId]));
 const cmd=`& '${qa}\\bin\\ferryx-cli.exe' agents send ${b.sessionId} x *> '${logs}\\pane-cross-write.log'; $LASTEXITCODE | Set-Content '${logs}\\pane-cross-write.exit'\r`;
 const completed=new Promise((resolve,reject)=>{const watcher=watch(logs,()=>{if(existsSync(logs+'\\\\pane-cross-write.exit')){watcher.close();clearTimeout(timer);resolve();}});const timer=setTimeout(()=>{watcher.close();reject(new Error('pane command completion timeout'));},15000);});
 await call({type:'write',sessionId:a.sessionId,data:Buffer.from(cmd).toString('base64')});
 await completed;
 console.log('PANE_DENIAL '+readFileSync(logs+'\\\\pane-cross-write.log','utf8'));
 console.log('PANE_EXIT '+readFileSync(logs+'\\\\pane-cross-write.exit','utf8'));
 renameSync(qa+'\\rt\\daemon.port',qa+'\\rt\\daemon.port.hidden');
 const out=execFileSync(qa+'\\bin\\ferryx-cli.exe',['agents','list','--json'],{encoding:'utf8',env:{...process.env,HOME:qa+'\\home',USERPROFILE:qa+'\\home',FERRYX_RUNTIME_DIR:qa+'\\rt',FERRYX_DATA_DIR:qa+'\\data'}});
 console.log('PIPE_ONLY_PORT_FILE_ABSENT '+out);
} finally {lines.close();socket.end();}
