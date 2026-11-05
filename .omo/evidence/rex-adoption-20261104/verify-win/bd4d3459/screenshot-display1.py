import subprocess,json,select,time
from pathlib import Path
out=Path(__file__).parent
p=subprocess.Popen(['maho-client','--host','127.0.0.1','--tcp-port','29730','--udp-port','29731','--pairing-id','9929DC13-3841-4182-A399-9E1323F961A2','--display','1','--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=open(out/'mahord-stderr.log','w'),text=True,bufsize=1)
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
 rpc(1,'initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'rex-win-verifier','version':'1'}})
 rpc(3,'tools/call',{'name':'remote_wait_for_screen_change','arguments':{'timeout_ms':15000}})
 r=rpc(2,'tools/call',{'name':'remote_take_screenshot','arguments':{}})
 import base64
 for item in r.get('result',{}).get('content',[]):
  if item.get('type')=='image':(out/'desktop-display1.png').write_bytes(base64.b64decode(item['data']))
 print('screenshot captured')
finally:
 p.terminate();p.wait(timeout=10)
