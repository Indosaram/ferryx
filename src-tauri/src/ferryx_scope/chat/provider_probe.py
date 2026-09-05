"""Opt-in isolated native protocol evidence. No existing thread or auth store is read."""
import asyncio, json, os, tempfile
from pathlib import Path

async def main():
    with tempfile.TemporaryDirectory(prefix='ferryx-chat-provider-') as root:
        env = dict(os.environ, HOME=root, CODEX_HOME=root + '/codex')
        Path(env['CODEX_HOME']).mkdir(mode=0o700)
        child = await asyncio.create_subprocess_exec('/Users/indo/.local/bin/codex', 'app-server', '--listen', 'stdio://', cwd='/tmp/ferryx-scope-ssh.Tody5Z/qa-one', env=env, stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
        async def send(v):
            child.stdin.write((json.dumps(v)+'\n').encode()); await child.stdin.drain()
        async def receive():
            line=await asyncio.wait_for(child.stdout.readline(),45)
            if not line: raise RuntimeError('provider exited')
            return json.loads(line)
        async def request(id,method,params):
            await send(dict(id=id,method=method,params=params))
            while True:
                v=await receive()
                if v.get('id')==id:
                    if 'error' in v: raise RuntimeError(json.dumps(v['error']))
                    return v['result']
        try:
            result=await request(1,'initialize',{'clientInfo':{'name':'ferryx_chat_boundary','version':'1'},'capabilities':{'experimentalApi':True}})
            print('INITIALIZED',json.dumps(result),flush=True)
            await send({'method':'initialized'})
            thread=await request(2,'thread/start',{'cwd':'/tmp/ferryx-scope-ssh.Tody5Z/qa-one','ephemeral':True,'approvalPolicy':'untrusted','sandbox':'read-only'})
            tid=thread['thread']['id'];print('EPHEMERAL_THREAD',tid,flush=True)
            await request(3,'turn/start',{'threadId':tid,'input':[{'type':'text','text':'Reply exactly FERRYX_SCOPE_PROVIDER_OK. Then request permission to run the harmless command true, and ask me a question with request_user_input if available. Do not read or modify any files.'}]})
            approvals=questions=0;sentinel=False
            while True:
                v=await receive();method=v.get('method','')
                if method=='item/agentMessage/delta': sentinel=sentinel or 'FERRYX_SCOPE_PROVIDER_OK' in v.get('params',{}).get('delta','')
                if 'id' in v and method.endswith('/requestApproval'):
                    approvals+=1;print('APPROVAL_ID',json.dumps(v['id']),flush=True);await send({'id':v['id'],'result':{'decision':'decline'}})
                elif 'id' in v and method=='item/tool/requestUserInput':
                    questions+=1;print('QUESTION_ID',json.dumps(v['id']),flush=True)
                    await send({'id':v['id'],'result':{'answers':{q['id']:{'answers':['QA answer']} for q in v['params']['questions']}}})
                if method in ['error','turn/completed']:
                    print('PROVIDER_RESULT',json.dumps(v),flush=True)
                    if method=='turn/completed':break
            print('OBSERVED',json.dumps(dict(sentinel=sentinel,approvals=approvals,questions=questions)),flush=True)
            return 0 if sentinel else 2
        finally:
            if child.returncode is None: child.kill()
            await child.wait();print('CLEANUP child_reaped=true temporary_home_removed_on_exit=true',flush=True)

try: code=asyncio.run(asyncio.wait_for(main(),100))
except Exception as e: print('UNAVAILABLE',str(e),flush=True);code=2
raise SystemExit(code)
