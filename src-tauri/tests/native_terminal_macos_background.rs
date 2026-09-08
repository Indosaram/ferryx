#[cfg(target_os = "macos")]
fn main() {
    use ferryx_lib::native_terminal::platform::macos::configure_window_background;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new().expect("the contract runs on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0));
    // SAFETY: The retained window is created on the main thread and never ordered on screen.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            bounds,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: Rust owns the retained window; closing must not release that ownership.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    let content = window.contentView().expect("content view");
    let child = NSView::initWithFrame(mtm.alloc(), bounds);
    content.addSubview(&child);

    for background in [[0.125, 0.25, 0.5, 1.0], [0.75, 0.5, 0.25, 0.0]] {
        configure_window_background(&window, background);
        child.setHidden(true);
        child.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0)));
        child.removeFromSuperview();

        assert!(window.isOpaque(), "the window must remain opaque without a terminal drawable");
        let color = window.backgroundColor();
        assert_eq!(color.alphaComponent(), 1.0, "terminal alpha must not punch through the desktop");
        assert!((color.redComponent() - background[0] as f64).abs() < 0.00001);
        assert!((color.greenComponent() - background[1] as f64).abs() < 0.00001);
        assert!((color.blueComponent() - background[2] as f64).abs() < 0.00001);
        assert!(!window.isVisible(), "the contract must not interact with the user's desktop");
    }
    println!("test native_window_background_survives_resize_hide_detach_and_theme ... ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("native macOS background contract is not applicable on this platform");
}
