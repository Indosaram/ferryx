#[cfg(windows)]
pub fn init() {
    use std::sync::Once;
    use windows_sys::Win32::Networking::WinSock::{WSAStartup, WSADATA};
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        let rc = unsafe { WSAStartup(0x0202, &mut data) };
        assert_eq!(rc, 0, "WSAStartup failed: {rc}");
    });
}

#[cfg(not(windows))]
pub fn init() {}
