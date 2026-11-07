#[cfg(target_os = "macos")]
fn main() {
    use ferryx_lib::browser::site_permissions::macos::ui_delegate_class;
    use ferryx_lib::browser::webview_download::wry_download_delegate;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{sel, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use wry::raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    };
    use wry::{WebViewBuilder, WebViewExtMacOS};

    /// The window's content view, the parent wry builds a child webview in.
    struct ContentView(NonNull<c_void>);

    impl HasWindowHandle for ContentView {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
            // SAFETY: the content view belongs to a window that lives until `main` returns, past
            // every use of this handle.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

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
    let content = window.contentView().expect("content view");
    let parent = ContentView(NonNull::from(&*content).cast());
    let webview = WebViewBuilder::new()
        .with_download_started_handler(|_, _| true)
        .build_as_child(&parent)
        .expect("wry builds a child webview");
    let wkwebview = webview.webview();
    let view: &AnyObject = &wkwebview;

    let delegate = ui_delegate_class(view).expect("wry's webview has a UI delegate");
    assert!(
        delegate
            .instance_method(sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))
            .is_some(),
        "wry's UI delegate answers media capture requests; Ferryx replaces that method"
    );
    assert!(
        delegate.name().to_string_lossy().contains("WryWebViewUIDelegate"),
        "the live delegate is wry's class, got {:?}",
        delegate.name()
    );
    assert!(
        AnyClass::get(c"WryWebViewUIDelegate").is_none(),
        "objc2 names wry's class after its module path and version, so a bare-name lookup misses it"
    );
    println!("test wry_ui_delegate_class_is_found_through_the_live_webview ... ok");

    let download = wry_download_delegate().expect("wry registered its download delegate class");
    assert!(
        download
            .instance_method(sel!(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))
            .is_some(),
        "wry's download delegate decides download destinations; Ferryx wraps that method"
    );
    println!("test wry_download_delegate_class_is_found_by_its_generated_name ... ok");

    drop(webview);
    window.close();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("wry delegate class contract is not applicable on this platform");
}
