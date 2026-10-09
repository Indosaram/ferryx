use std::time::{Duration, Instant};

const TOTAL: usize = 1 << 20;

fn payload() -> Vec<u8> {
    let mut v = Vec::with_capacity(TOTAL);
    let mut i: u32 = 0;
    while v.len() < TOTAL {
        let line = format!("{:08}abcdefghijklmnopqrstuvwxyz0123456789\n", i);
        v.extend_from_slice(line.as_bytes());
        i += 1;
    }
    v.truncate(TOTAL);
    v
}

struct Report {
    platform: &'static str,
    partial_writes: u64,
    would_block: u64,
    max_write_call_us: u128,
    reader_paused_ms: u128,
    elapsed_ms: u128,
    received_equal: bool,
    received_len: usize,
}

fn print(r: &Report) {
    println!(
        "P0-1 platform={} total={} partial_writes={} would_block_events={} max_single_write_call_us={} reader_paused_ms={} elapsed_ms={} received_len={} order_and_bytes_equal={}",
        r.platform, TOTAL, r.partial_writes, r.would_block, r.max_write_call_us, r.reader_paused_ms, r.elapsed_ms, r.received_len, r.received_equal
    );
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::ffi::CString;

    pub fn run(pause_ms: u64) -> Report {
        unsafe {
            let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(master >= 0);
            assert_eq!(libc::grantpt(master), 0);
            assert_eq!(libc::unlockpt(master), 0);
            let name = std::ffi::CStr::from_ptr(libc::ptsname(master)).to_owned();
            let out_path = std::env::temp_dir().join(format!("p0_pty_child_{}.bin", std::process::id()));
            let out_c = CString::new(out_path.to_str().unwrap()).unwrap();
            let pid = libc::fork();
            if pid == 0 {
                libc::setsid();
                let slave = libc::open(name.as_ptr(), libc::O_RDWR);
                libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
                let mut t: libc::termios = std::mem::zeroed();
                libc::tcgetattr(slave, &mut t);
                libc::cfmakeraw(&mut t);
                libc::tcsetattr(slave, libc::TCSANOW, &t);
                let fd = libc::open(out_c.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC, 0o600);
                let mut buf = [0u8; 4096];
                let mut got = 0usize;
                let start = libc::time(std::ptr::null_mut());
                let _ = start;
                std::thread::sleep(Duration::from_millis(pause_ms));
                while got < TOTAL {
                    let n = libc::read(slave, buf.as_mut_ptr() as *mut _, buf.len());
                    if n <= 0 { break; }
                    libc::write(fd, buf.as_ptr() as *const _, n as usize);
                    got += n as usize;
                    std::thread::sleep(Duration::from_micros(200));
                }
                libc::close(fd);
                libc::_exit(0);
            }
            let flags = libc::fcntl(master, libc::F_GETFL);
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
            let data = payload();
            let mut off = 0usize;
            let (mut partial, mut wb, mut maxus) = (0u64, 0u64, 0u128);
            let t0 = Instant::now();
            while off < data.len() {
                let chunk = (data.len() - off).min(64 * 1024);
                let c0 = Instant::now();
                let n = libc::write(master, data[off..].as_ptr() as *const _, chunk);
                maxus = maxus.max(c0.elapsed().as_micros());
                if n < 0 {
                    let e = *libc::__errno_location();
                    if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                        wb += 1;
                        let mut pfd = libc::pollfd { fd: master, events: libc::POLLOUT, revents: 0 };
                        libc::poll(&mut pfd, 1, 1000);
                        let mut sink = [0u8; 65536];
                        libc::read(master, sink.as_mut_ptr() as *mut _, sink.len());
                        continue;
                    }
                    if e == libc::EINTR { continue; }
                    panic!("write errno {e}");
                }
                if (n as usize) < chunk { partial += 1; }
                off += n as usize;
                let mut sink = [0u8; 65536];
                libc::read(master, sink.as_mut_ptr() as *mut _, sink.len());
            }
            let mut st = 0;
            loop {
                let r = libc::waitpid(pid, &mut st, libc::WNOHANG);
                if r == pid { break; }
                let mut sink = [0u8; 65536];
                libc::read(master, sink.as_mut_ptr() as *mut _, sink.len());
                std::thread::sleep(Duration::from_millis(2));
            }
            let elapsed = t0.elapsed().as_millis();
            let got = std::fs::read(&out_path).unwrap_or_default();
            let _ = std::fs::remove_file(&out_path);
            Report { platform: "linux-pty", partial_writes: partial, would_block: wb, max_write_call_us: maxus, reader_paused_ms: pause_ms as u128, elapsed_ms: elapsed, received_equal: got == data, received_len: got.len() }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Console::*;
    use windows_sys::Win32::System::IO::*;
    use windows_sys::Win32::System::Pipes::*;
    use windows_sys::Win32::System::Threading::*;

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

    pub fn run(pause_ms: u64) -> Report {
        unsafe {
            let pipe_name = wide(&format!(r"\\.\pipe\ferryx-p0-conpty-in-{}", std::process::id()));
            let server = CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1, 64 * 1024, 64 * 1024, 0, null_mut(),
            );
            assert!(server != INVALID_HANDLE_VALUE, "CreateNamedPipeW {}", GetLastError());
            let client = CreateFileW(pipe_name.as_ptr(), GENERIC_READ, 0, null_mut(), OPEN_EXISTING, 0, null_mut());
            assert!(client != INVALID_HANDLE_VALUE, "CreateFileW {}", GetLastError());
            let (mut out_r, mut out_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
            assert!(CreatePipe(&mut out_r, &mut out_w, null_mut(), 0) != 0);
            let mut hpc: HPCON = 0;
            let hr = CreatePseudoConsole(COORD { X: 200, Y: 60 }, client, out_w, 0, &mut hpc);
            assert!(hr == 0, "CreatePseudoConsole {hr:#x}");
            CloseHandle(client);
            CloseHandle(out_w);
            let out_r_usize = out_r as usize;
            std::thread::spawn(move || {
                let h = out_r_usize as HANDLE;
                let mut buf = vec![0u8; 65536];
                loop { let mut n = 0u32; if ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, null_mut()) == 0 || n == 0 { break; } }
            });

            let mut size: usize = 0;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            let mut attr = vec![0u8; size];
            let list = attr.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            assert!(InitializeProcThreadAttributeList(list, 1, 0, &mut size) != 0);
            assert!(UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize, hpc as *const _, std::mem::size_of::<HPCON>(), null_mut(), null_mut()) != 0);
            let out_path = std::env::temp_dir().join(format!("p0_conpty_child_{}.bin", std::process::id()));
            let exe = std::env::current_exe().unwrap();
            let mut cmd = wide(&format!("\"{}\" child \"{}\" {}", exe.display(), out_path.display(), pause_ms));
            let mut si: STARTUPINFOEXW = std::mem::zeroed();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.lpAttributeList = list;
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = null_mut();
            si.StartupInfo.hStdOutput = null_mut();
            si.StartupInfo.hStdError = null_mut();
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(null_mut(), cmd.as_mut_ptr(), null_mut(), null_mut(), 0, EXTENDED_STARTUPINFO_PRESENT, null_mut(), null_mut(), &si.StartupInfo, &mut pi);
            assert!(ok != 0, "CreateProcessW {}", GetLastError());

            let data = payload();
            let ev = CreateEventW(null_mut(), 1, 0, null_mut());
            let mut off = 0usize;
            let (mut partial, mut pending, mut maxus) = (0u64, 0u64, 0u128);
            let t0 = Instant::now();
            let mut write_done_ms = 0u128;
            while off < data.len() {
                let chunk = (data.len() - off).min(64 * 1024) as u32;
                let mut ov: OVERLAPPED = std::mem::zeroed();
                ov.hEvent = ev;
                ResetEvent(ev);
                let c0 = Instant::now();
                let mut n = 0u32;
                let r = WriteFile(server, data[off..].as_ptr(), chunk, &mut n, &mut ov);
                maxus = maxus.max(c0.elapsed().as_micros());
                if r == 0 {
                    let e = GetLastError();
                    assert!(e == ERROR_IO_PENDING, "WriteFile {e}");
                    pending += 1;
                    loop {
                        let w = WaitForSingleObject(ev, 50);
                        if w == WAIT_OBJECT_0 { break; }
                    }
                    assert!(GetOverlappedResult(server, &ov, &mut n, 0) != 0, "GetOverlappedResult {}", GetLastError());
                }
                if n < chunk { partial += 1; }
                off += n as usize;
            }
            write_done_ms = t0.elapsed().as_millis();
            println!("WRITES_DONE_MS={}", write_done_ms);
            let wr = WaitForSingleObject(pi.hProcess, 60_000);
            let elapsed = t0.elapsed().as_millis();
            let diag = std::fs::read_to_string(format!("{}.diag", out_path.display())).unwrap_or_else(|_| "no-diag".into());
            if wr != WAIT_OBJECT_0 { TerminateProcess(pi.hProcess, 9); }
            println!("CHILD wait_result={} child_diag={}", wr, diag.trim());
            ClosePseudoConsole(hpc);
            let got = std::fs::read(&out_path).unwrap_or_default();
            let _ = std::fs::remove_file(&out_path);
            let _ = std::fs::remove_file(format!("{}.diag", out_path.display()));
            let _ = std::fs::remove_file(format!("{}.done", out_path.display()));
            Report { platform: "windows-conpty", partial_writes: partial, would_block: pending, max_write_call_us: maxus, reader_paused_ms: pause_ms as u128, elapsed_ms: elapsed, received_equal: got == data, received_len: got.len() }
        }
    }

    pub fn child(out: &str, pause_ms: u64) {
        unsafe {
            let hin = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0u32;
            GetConsoleMode(hin, &mut mode);
            SetConsoleMode(hin, ENABLE_VIRTUAL_TERMINAL_INPUT);
            let mut f = std::fs::File::create(out).unwrap();
            let diag = format!("{}.diag", out);
            let _ = std::fs::write(&diag, format!("start stdin_handle={:?} getmode_ok={} mode={:#x}\n", hin, GetConsoleMode(hin, &mut mode), mode));
            std::thread::sleep(Duration::from_millis(pause_ms));
            let mut buf = vec![0u8; 4096];
            let mut got = 0usize;
            let mut reads = 0u64;
            while got < TOTAL {
                let mut n = 0u32;
                let ok = ReadFile(hin, buf.as_mut_ptr(), buf.len() as u32, &mut n, null_mut());
                if ok == 0 || n == 0 {
                    let _ = std::fs::write(&diag, format!("read_end ok={} n={} err={} got={} reads={}\n", ok, n, GetLastError(), got, reads));
                    break;
                }
                reads += 1;
                std::io::Write::write_all(&mut f, &buf[..n as usize]).unwrap();
                got += n as usize;
                if reads % 64 == 0 { let _ = std::fs::write(&diag, format!("progress got={} reads={}\n", got, reads)); }
            }
            let _ = std::fs::write(format!("{}.done", out), format!("got={} reads={}\n", got, reads));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    if args.get(1).map(|s| s.as_str()) == Some("child") {
        imp::child(&args[2], args[3].parse().unwrap());
        return;
    }
    let pause_ms: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1500);
    print(&imp::run(pause_ms));
}
