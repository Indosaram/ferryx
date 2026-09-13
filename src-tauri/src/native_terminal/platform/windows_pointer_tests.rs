use super::*;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn WindowFromPoint(point: Point) -> Hwnd;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        mode: *const c_void,
        flags: u32,
        access: u32,
        security: *const c_void,
    ) -> Hwnd;
    fn GetThreadDesktop(thread: u32) -> Hwnd;
    fn SetThreadDesktop(desktop: Hwnd) -> i32;
    fn CloseDesktop(desktop: Hwnd) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

struct TestDesktop {
    handle: Hwnd,
    previous: Hwnd,
}

impl TestDesktop {
    fn new() -> Self {
        let name: Vec<u16> = format!("FerryxPointerTest-{}\0", uuid::Uuid::new_v4())
            .encode_utf16()
            .collect();
        let handle = unsafe {
            CreateDesktopW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                0x1000_0000,
                std::ptr::null(),
            )
        };
        assert!(!handle.is_null(), "create isolated test desktop");
        let desktop = Self {
            handle,
            previous: unsafe { GetThreadDesktop(GetCurrentThreadId()) },
        };
        assert_ne!(unsafe { SetThreadDesktop(handle) }, 0);
        desktop
    }
}

impl Drop for TestDesktop {
    fn drop(&mut self) {
        unsafe {
            assert_ne!(SetThreadDesktop(self.previous), 0);
            assert_ne!(CloseDesktop(self.handle), 0);
        }
    }
}

struct InputWindow {
    parent: NonZeroIsize,
    input: Hwnd,
    desktop: TestDesktop,
}

impl InputWindow {
    fn new() -> Self {
        let desktop = TestDesktop::new();
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let parent = unsafe {
            CreateWindowExW(
                0x0000_0008 | WS_EX_NOACTIVATE,
                class.as_ptr(),
                std::ptr::null(),
                0x8000_0000,
                16,
                16,
                32,
                32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let parent = NonZeroIsize::new(parent as isize).expect("create input parent");
        let mut window = Self {
            parent,
            input: std::ptr::null_mut(),
            desktop,
        };
        window.input = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | 0x1000_0000,
                0,
                0,
                32,
                32,
                parent.get() as Hwnd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
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
        let desktop = self.desktop.handle as isize;
        std::thread::spawn(move || {
            assert_ne!(unsafe { SetThreadDesktop(desktop as Hwnd) }, 0);
            unsafe { WindowFromPoint(point) as isize }
        })
        .join()
        .expect("join pointer query")
    }
}

impl Drop for InputWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.parent.get() as Hwnd) };
    }
}

#[test]
fn presented_terminal_yields_cross_thread_pointer_hit_testing_to_input() {
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
