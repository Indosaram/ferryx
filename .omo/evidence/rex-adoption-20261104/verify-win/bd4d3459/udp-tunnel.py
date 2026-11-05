import socket,subprocess,struct,threading
remote='''import socket,sys,struct,threading
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.connect(("127.0.0.1",19731))
def recv():
 while True:
  b=s.recv(65535);sys.stdout.buffer.write(struct.pack("!I",len(b))+b);sys.stdout.buffer.flush()
threading.Thread(target=recv,daemon=True).start()
while True:
 h=sys.stdin.buffer.read(4)
 if len(h)!=4:break
 n=struct.unpack("!I",h)[0];b=sys.stdin.buffer.read(n)
 if len(b)!=n:break
 s.send(b)
'''
import base64
code=base64.b64encode(remote.encode()).decode()
p=subprocess.Popen(['ssh','maho-win',f'python -u -c "import base64;exec(base64.b64decode(\'{code}\'))"'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.bind(('127.0.0.1',29731));peer=None
def exact(n):
 b=b''
 while len(b)<n:
  x=p.stdout.read(n-len(b))
  if not x:raise EOFError()
  b+=x
 return b
def receive():
 while True:
  b=exact(struct.unpack('!I',exact(4))[0])
  if peer:s.sendto(b,peer)
threading.Thread(target=receive,daemon=True).start()
try:
 while True:
  b,peer=s.recvfrom(65535);p.stdin.write(struct.pack('!I',len(b))+b);p.stdin.flush()
finally:p.terminate()
