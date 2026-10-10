use super::*;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

const DESKTOP_READOBJECTS: u32 = 0x0001;

#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: [i32; 2],
    private: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn WindowFromPoint(point: Point) -> Hwnd;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn CloseDesktop(desktop: Hwnd) -> i32;
    fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> Hwnd;
    fn UnregisterClassW(class_name: *const u16, instance: Hinstance) -> i32;
    fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn MsgWaitForMultipleObjects(
        count: u32,
        handles: *const *mut c_void,
        wait_all: i32,
        milliseconds: u32,
        wake_mask: u32,
    ) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn ProcessIdToSessionId(process_id: u32, session_id: *mut u32) -> i32;
    fn CreateEventW(
        security: *const c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn SetEvent(handle: *mut c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

struct SafeEvent(*mut c_void);
unsafe impl Send for SafeEvent {}
unsafe impl Sync for SafeEvent {}
impl Drop for SafeEvent {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct InputWindow {
    parent: NonZeroIsize,
    input: Hwnd,
    instance: Hinstance,
    class_name: Vec<u16>,
}

fn ensure_input_fixture_class(instance: Hinstance) -> Vec<u16> {
    let class_name: Vec<u16> = format!("FerryxInputFixture-{}\0", uuid::Uuid::new_v4())
        .encode_utf16()
        .collect();
    let class = WndClassExW {
        cb_size: std::mem::size_of::<WndClassExW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfn_wnd_proc: Some(DefWindowProcW),
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: instance,
        h_icon: std::ptr::null_mut(),
        h_cursor: std::ptr::null_mut(),
        hbr_background: std::ptr::null_mut(),
        lpsz_menu_name: std::ptr::null(),
        lpsz_class_name: class_name.as_ptr(),
        h_icon_sm: std::ptr::null_mut(),
    };
    let atom = unsafe { RegisterClassExW(&class) };
    assert_ne!(atom, 0, "register input fixture window class");
    class_name
}

impl InputWindow {
    fn new() -> Self {
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let class_name = ensure_input_fixture_class(instance);
        let parent = unsafe {
            CreateWindowExW(
                0x0000_0008 | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                std::ptr::null(),
                0x8000_0000 | 0x1000_0000,
                16,
                16,
                32,
                32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let parent = NonZeroIsize::new(parent as isize).expect("create input parent");
        let mut window = Self {
            parent,
            input: std::ptr::null_mut(),
            instance,
            class_name,
        };
        window.input = unsafe {
            CreateWindowExW(
                0,
                window.class_name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | 0x1000_0000,
                0,
                0,
                32,
                32,
                parent.get() as Hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        assert!(!window.input.is_null(), "create input child");
        unsafe { ShowWindow(parent.get() as Hwnd, SW_SHOWNOACTIVATE) };
        window
    }

    fn pointer_target_from_other_thread(&self) -> isize {
        let mut point = Point { x: 8, y: 8 };
        assert_ne!(
            unsafe { ClientToScreen(self.parent.get() as Hwnd, &mut point) },
            0
        );
        let raw_event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        assert!(!raw_event.is_null(), "create completion event");
        let event = std::sync::Arc::new(SafeEvent(raw_event));
        let child_event = std::sync::Arc::clone(&event);
        let child = std::thread::spawn(move || {
            let target = unsafe { WindowFromPoint(point) as isize };
            assert_ne!(unsafe { SetEvent(child_event.0) }, 0);
            target
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let handles = [event.0];
        const PM_REMOVE: u32 = 1;
        const QS_ALLINPUT: u32 = 0x04ff;
        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_FAILED: u32 = 0xffff_ffff;

        let mut completed = false;
        while std::time::Instant::now() < deadline {
            let mut msg = Msg::default();
            let mut drained = 0;
            while drained < 64 && std::time::Instant::now() < deadline && unsafe {
                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE)
            } != 0 {
                drained += 1;
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            let remaining = deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis() as u32;
            if remaining == 0 {
                break;
            }
            let res = unsafe {
                MsgWaitForMultipleObjects(1, handles.as_ptr(), 0, remaining, QS_ALLINPUT)
            };
            if res == WAIT_OBJECT_0 {
                completed = true;
                break;
            } else if res == WAIT_FAILED {
                panic!(
                    "MsgWaitForMultipleObjects failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }

        assert!(
            completed,
            "timed out waiting for child pointer query completion"
        );
        child.join().expect("join pointer query")
    }
}

impl Drop for InputWindow {
    fn drop(&mut self) {
        assert_ne!(
            unsafe { DestroyWindow(self.parent.get() as Hwnd) },
            0,
            "destroy input fixture parent window"
        );
        assert_ne!(
            unsafe { UnregisterClassW(self.class_name.as_ptr(), self.instance) },
            0,
            "unregister input fixture window class"
        );
    }
}

#[test]
fn presented_terminal_yields_cross_thread_pointer_hit_testing_to_input() {
    let pid = unsafe { GetCurrentProcessId() };
    let mut session_id: u32 = 0;
    let session_ok = unsafe { ProcessIdToSessionId(pid, &mut session_id) };
    let input_desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    assert!(
        session_ok != 0 && session_id != 0 && !input_desktop.is_null(),
        "presented_terminal_yields_cross_thread_pointer_hit_testing_to_input requires an interactive Windows user desktop session (WinSta0); got session_id={session_id} input_desktop_is_null={}",
        input_desktop.is_null()
    );
    unsafe { CloseDesktop(input_desktop) };

    let window = InputWindow::new();
    let target = WindowsCompositorTarget::from_parent_hwnd(window.parent)
        .expect("create production compositor target");
    assert_eq!(
        window.pointer_target_from_other_thread(),
        window.input as isize
    );

    target.update_viewport(Some(LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
        scale_factor: 1.0,
    }));
    target.reveal();

    assert_ne!(
        unsafe { IsWindowVisible(target.handle.hwnd.get() as Hwnd) },
        0
    );
    assert_eq!(
        window.pointer_target_from_other_thread(),
        window.input as isize,
        "the visible renderer must not become the pointer target above WebView2",
    );
}
