// One protocol for local and host-helper waits. Capability overrides are a seam for
// exercising old Python/kernel combinations without changing the interpreter.
export function exitWatchSource({ pidfd = "hasattr(os,'pidfd_open')", kqueue = "hasattr(select,'kqueue')", procRoot = '/proc' } = {}) {
  return `import os,sys,select,signal,time,errno
pid=int(sys.argv[1]); stamp=sys.argv[2]; timeout=float(sys.argv[3]); sig=sys.argv[4]
deadline=time.monotonic()+timeout
root=${JSON.stringify(procRoot)}
def current():
 try:
  with open('%s/%d/stat'%(root,pid)) as stream:
   fields=stream.read().rsplit(') ',1)[1].split()
  if len(fields)<20 or not fields[19].isdigit(): raise ValueError('invalid proc start stamp')
  return fields[19]
 except (FileNotFoundError,ProcessLookupError):
  with open('%s/self/stat'%root) as stream: stream.read()
  return None
def remaining():
 return max(0,deadline-time.monotonic())
try:
 fd=None
 if ${pidfd}:
  try: fd=os.pidfd_open(pid)
  except OSError as error:
   if error.errno not in (errno.ENOSYS,errno.EINVAL): raise
 if fd is not None:
  print('WATCH_VIA=pidfd',file=sys.stderr,flush=True)
  if current()!=stamp:
   print('GONE',flush=True); sys.exit(0)
  print('ARMED',flush=True)
  if sig!='none': signal.pidfd_send_signal(fd,getattr(signal,sig))
  print('EXIT' if select.select([fd],[],[],remaining())[0] else 'TIMEOUT',flush=True)
 elif ${kqueue}:
  print('WATCH_VIA=kqueue',file=sys.stderr,flush=True)
  kq=select.kqueue()
  kq.control([select.kevent(pid,filter=select.KQ_FILTER_PROC,flags=select.KQ_EV_ADD|select.KQ_EV_ONESHOT,fflags=select.KQ_NOTE_EXIT)],0,0)
  print('ARMED',flush=True)
  print('EXIT' if kq.control(None,1,remaining()) else 'TIMEOUT',flush=True)
 else:
  print('WATCH_VIA=proc',file=sys.stderr,flush=True)
  if current()!=stamp:
   print('GONE',flush=True); sys.exit(0)
  print('ARMED',flush=True)
  if sig!='none':
   if current()!=stamp:
    print('EXIT',flush=True); sys.exit(0)
   os.kill(pid,getattr(signal,sig))
  while True:
   if current()!=stamp:
    print('EXIT',flush=True); break
   if remaining()<=0:
    print('TIMEOUT',flush=True); break
   time.sleep(min(0.1,remaining()))
except ProcessLookupError:
 print('GONE',flush=True)
`;
}
