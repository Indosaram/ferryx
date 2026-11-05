import ctypes,ctypes.wintypes as w,json,sys
k=ctypes.WinDLL('kernel32',use_last_error=True);n=ctypes.WinDLL('ntdll')
k.OpenProcess.restype=w.HANDLE;k.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD]
k.ReadProcessMemory.argtypes=[w.HANDLE,w.LPCVOID,w.LPVOID,ctypes.c_size_t,ctypes.POINTER(ctypes.c_size_t)]
h=k.OpenProcess(0x410,False,int(sys.argv[1]));assert h,ctypes.get_last_error()
def read(addr,size):
 b=ctypes.create_string_buffer(size);got=ctypes.c_size_t();assert k.ReadProcessMemory(h,addr,b,size,ctypes.byref(got)),ctypes.get_last_error();return b.raw[:got.value]
class PBI(ctypes.Structure):_fields_=[('reserved',w.LPVOID),('peb',w.LPVOID),('r2',w.LPVOID*2),('pid',ctypes.c_size_t),('r3',w.LPVOID)]
p=PBI();n.NtQueryInformationProcess.argtypes=[w.HANDLE,ctypes.c_uint,w.LPVOID,w.ULONG,w.LPVOID]
assert n.NtQueryInformationProcess(h,0,ctypes.byref(p),ctypes.sizeof(p),None)==0
params=int.from_bytes(read(p.peb+0x20,8),'little');env=int.from_bytes(read(params+0x80,8),'little')
data=b''
for i in range(512):
 data+=read(env+i*1024,1024)
 text=data.decode('utf-16-le',errors='replace')
 if '\0\0' in text:break
values=dict(x.split('=',1) for x in text.split('\0\0')[0].split('\0') if '=' in x and not x.startswith('='))
print(json.dumps({key:value for key,value in values.items() if key.startswith('FERRYX_') or key in ['HOME','USERPROFILE','APPDATA','LOCALAPPDATA']},indent=2))
k.CloseHandle(h)
