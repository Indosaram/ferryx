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
        // SAFETY: FFI allocation. The unique, terminated name remains live for
        // the call; all optional desktop parameters are null.
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
        // SAFETY: Both queries refer to this live test thread. It has not created
        // any windows yet. This does NOT switch the user's visible desktop.
        let desktop = Self {
            handle,
            previous: unsafe { GetThreadDesktop(GetCurrentThreadId()) },
        };
        // SAFETY: This new test desktop remains owned through thread restoration.
        assert_ne!(unsafe { SetThreadDesktop(handle) }, 0);
        desktop
    }
}

impl Drop for TestDesktop {
    fn drop(&mut self) {
        // SAFETY: InputWindow destroys its HWNDs before this field is dropped;
        // the query thread has joined. Restore the borrowed desktop, close ours.
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
        // SAFETY: FFI allocation. STATIC is a system class; all optional pointers
        // are null and the class name remains valid throughout both calls.
        let parent = unsafe {
            CreateWindowExW(
                0x0000_0008 | WS_EX_NOACTIVATE, // WS_EX_TOPMOST
                class.as_ptr(),
                std::ptr::null(),
                0x8000_0000, // WS_POPUP; never activate or move the user's pointer.
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
        // SAFETY: FFI allocation. The parent is owned by window; STATIC needs no
        // instance or creation parameter. Parent Drop also destroys this child.
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
        // SAFETY: The owned parent is live and remains on this thread until Drop.
        unsafe { ShowWindow(parent.get() as Hwnd, SW_SHOWNOACTIVATE) };
        window
    }

    fn pointer_target_from_other_thread(&self) -> isize {
        let mut point = Point { x: 8, y: 8 };
        // SAFETY: point is initialized writable storage; the parent is live.
        assert_ne!(
            unsafe { ClientToScreen(self.parent.get() as Hwnd, &mut point) },
            0
        );
        let desktop = self.desktop.handle as isize;
        std::thread::spawn(move || {
            // SAFETY: The owner retains the desktop until this query joins. The
            // new thread owns no windows and never switches the visible desktop.
            assert_ne!(unsafe { SetThreadDesktop(desktop as Hwnd) }, 0);
            // SAFETY: This read-only Win32 query takes a value, retains no Rust
            // pointers, and finishes before the owner can destroy the windows.
            unsafe { WindowFromPoint(point) as isize }
        })
        .join()
        .expect("join pointer query")
    }
}

impl Drop for InputWindow {
    fn drop(&mut self) {
        // SAFETY: This owner-thread fixture owns the parent and all its children.
        unsafe { DestroyWindow(self.parent.get() as Hwnd) };
    }
}

#[test]
fn presented_terminal_yields_cross_thread_pointer_hit_testing_to_input() {
    // Given a real input child, covered by the production compositor child.
    let window = InputWindow::new();
    let target = WindowsCompositorTarget::from_parent_hwnd(window.parent)
        .expect("create production compositor target");
    target.update_viewport(Some(LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: 32.0,
        height: 32.0,
        scale_factor: 1.0,
    }));
    assert_eq!(
        window.pointer_target_from_other_thread(),
        window.input as isize
    );

    // When the terminal's first frame raises it above the input window.
    target.reveal();

    // Then it remains visible but does not intercept cross-thread pointer input.
    // HTTRANSPARENT alone is thread-local; WebView2 input lives on another thread.
    // SAFETY: target owns this live child through the assertion.
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
