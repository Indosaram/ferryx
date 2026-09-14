//! PIPE_NOWAIT writes have no outstanding kernel operation after return.
use std::{io, os::windows::io::{AsRawHandle, OwnedHandle}};
use windows_sys::Win32::Storage::FileSystem::WriteFile;

pub struct WindowsInput(pub OwnedHandle);
impl WindowsInput {
    pub fn try_write(&self, bytes: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        let ok = unsafe { WriteFile(self.0.as_raw_handle(), bytes.as_ptr(), bytes.len() as u32, &mut written, std::ptr::null_mut()) };
        if ok == 0 { return Err(io::Error::last_os_error()); }
        Ok(written as usize)
    }


}
