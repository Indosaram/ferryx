//! Read the current directory of a same-architecture child process.
use std::{ffi::c_void, os::windows::ffi::OsStringExt, path::PathBuf};

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn ReadProcessMemory(handle: *mut c_void, address: *const c_void, buffer: *mut c_void,
        size: usize, read: *mut usize) -> i32;
    fn IsWow64Process(handle: *mut c_void, wow64: *mut i32) -> i32;
}
#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(handle: *mut c_void, class: u32, buffer: *mut c_void,
        size: u32, returned: *mut u32) -> i32;
}

struct Process(*mut c_void);
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: owned non-null handle from OpenProcess; closed exactly once.
        unsafe { CloseHandle(self.0); }
    }
}
impl Process {
    fn read(&self, address: usize, buffer: &mut [u8]) -> Option<()> {
        let mut count = 0;
        // SAFETY: remote addresses are never dereferenced locally. The OS copies
        // at most buffer.len() bytes into this exclusively borrowed initialized slice.
        let ok = unsafe { ReadProcessMemory(self.0, address as *const c_void,
            buffer.as_mut_ptr().cast(), buffer.len(), &mut count) };
        (ok != 0 && count == buffer.len()).then_some(())
    }
    fn pointer(&self, address: usize, width: usize) -> Option<usize> {
        let mut bytes = [0u8; 8];
        self.read(address, &mut bytes[..width])?;
        usize::try_from(u64::from_le_bytes(bytes)).ok()
    }
}

pub(super) fn process_cwd(pid: u32) -> Option<PathBuf> {
    // SAFETY: scalar inputs, no borrowed buffers; the returned handle is owned.
    let raw = unsafe { OpenProcess(0x0400 | 0x0010, 0, pid) };
    if raw.is_null() { return None; }
    let process = Process(raw);
    let mut wow64 = 0;
    // SAFETY: initialized writable scalar, valid process handle.
    if unsafe { IsWow64Process(raw, &mut wow64) } == 0 { return None; }
    let native_width = std::mem::size_of::<usize>();
    let mut basic = [0usize; 6];
    // SAFETY: PROCESS_BASIC_INFORMATION is six pointer-sized slots on supported
    // Windows architectures. Buffer is aligned, initialized, and its exact size passed.
    if unsafe { NtQueryInformationProcess(raw, 0, basic.as_mut_ptr().cast(),
        u32::try_from(std::mem::size_of_val(&basic)).ok()?, std::ptr::null_mut()) } < 0 { return None; }
    let (peb, width) = if wow64 != 0 && native_width == 8 {
        let mut peb32 = 0usize;
        // SAFETY: ProcessWow64Information writes one pointer-sized value.
        if unsafe { NtQueryInformationProcess(raw, 26, (&mut peb32 as *mut usize).cast(),
            8, std::ptr::null_mut()) } < 0 { return None; }
        (peb32, 4)
    } else { (basic[1], native_width) };
    let parameters = process.pointer(peb.checked_add(if width == 8 { 0x20 } else { 0x10 })?, width)?;
    let directory = parameters.checked_add(if width == 8 { 0x38 } else { 0x24 })?;
    let mut length = [0u8; 2];
    process.read(directory, &mut length)?;
    let length = usize::from(u16::from_le_bytes(length));
    if length == 0 || length % 2 != 0 { return None; }
    let buffer = process.pointer(directory.checked_add(width)?, width)?;
    let mut bytes = vec![0u8; length];
    process.read(buffer, &mut bytes)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    Some(PathBuf::from(std::ffi::OsString::from_wide(&units)))
}
