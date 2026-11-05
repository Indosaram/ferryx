import subprocess,struct,json,os
from pathlib import Path
root=Path(r'C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b');logs=root/'.omo/evidence/rex-adoption-20261104/verify-win/d8313422b'
cli=root/'.qa/pipe/bin/ferryx-cli.exe';wasm=(root/'.qa/wasm/clock.wasm').read_bytes()
p=subprocess.Popen([str(cli),'--plugin-wasm-runner'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={})
p.stdin.write(struct.pack('<I',len(wasm))+wasm);p.stdin.flush()
# stdout line is the precise guest render event, not a timing delay.
line=p.stdout.readline();print('RENDER',line.decode().strip(),flush=True)
p.stdin.write(b'{"ok":true}\n');p.stdin.flush();p.stdin.close()
try: print('EOF_TEARDOWN_EXIT',p.wait(timeout=10),flush=True)
except subprocess.TimeoutExpired:p.kill();print('EOF_TEARDOWN_TIMEOUT',flush=True)
