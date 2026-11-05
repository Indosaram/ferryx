import subprocess,json,select,time
from pathlib import Path
out=Path(__file__).parent
p=subprocess.Popen(['maho-client','--host','127.0.0.1','--tcp-port','29730','--udp-port','29731','--pairing-id','9929DC13-3841-4182-A399-9E1323F961A2','--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=open(out/'mahord-stderr.log','w'),text=True,bufsize=1)
def rpc(i,method,params):
 p.stdin.write(json.dumps({'jsonrpc':'2.0','id':i,'method':method,'params':params})+'\n');p.stdin.flush()
 deadline=time.monotonic()+45
 while time.monotonic()<deadline:
  if select.select([p.stdout],[],[],max(0,deadline-time.monotonic()))[0]:
   line=p.stdout.readline()
   if not line: raise RuntimeError('MahoRD exited')
   with open(out/'mahord-rpc.log','a') as f:f.write(line)
   try:r=json.loads(line)
   except ValueError:continue
   if r.get('id')==i:return r
 raise TimeoutError(method)
try:
 print(rpc(1,'initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'rex-win-verifier','version':'1'}}))
 print(rpc(2,'tools/list',{}))
finally:
 p.terminate();p.wait(timeout=10)
