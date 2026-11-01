//! The NSApplication subclass and delegate CEF needs on macOS (from the cef-rs cefsimple
//! example). The menu nib gives the engine the standard Edit menu, so copy and paste work.

use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_methods,
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSApp, NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSEvent,
    NSUserInterfaceValidations, NSValidatedUserInterfaceItem,
};
use objc2_foundation::{NSBundle, NSObjectNSThreadPerformAdditions, ns_string};
use std::{cell::Cell, ptr};

define_class! {
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct EngineAppDelegate;

    impl EngineAppDelegate {
        /// Create the application on the UI thread.
        #[unsafe(method(createApplication:))]
        unsafe fn create_application(&self, _object: Option<&AnyObject>) {
            let app = NSApp(MainThreadMarker::new().expect("Not running on the main thread"));
            assert!(app.isKindOfClass(EngineApplication::class()));
            assert!(
                app.delegate()
                    .unwrap()
                    .isKindOfClass(EngineAppDelegate::class())
            );

            let main_bundle = NSBundle::mainBundle();
            let _: Bool = msg_send![&main_bundle,
                loadNibNamed: ns_string!("MainMenu"),
                owner: &*app,
                topLevelObjects: ptr::null_mut::<*const AnyObject>()
            ];
        }
    }

    unsafe impl NSObjectProtocol for EngineAppDelegate {}

    unsafe impl NSApplicationDelegate for EngineAppDelegate {
        #[unsafe(method(applicationShouldTerminate:))]
        unsafe fn application_should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            NSApplicationTerminateReply::TerminateNow
        }

        /// Called when the user clicks the app dock icon while the application is
        /// already running.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        unsafe fn application_should_handle_reopen(&self, _sender: &NSApplication, _has_visible_windows: Bool) -> Bool {
            Bool::NO
        }

        /// Requests that any state restoration archive be created with secure encoding
        /// (macOS 12+ only). See https://crrev.com/c737387656 for details. This also
        /// fixes an issue with macOS default behavior incorrectly restoring windows
        /// after hard reset (holding down the power button).
        #[unsafe(method(applicationSupportsSecureRestorableState:))]
        unsafe fn application_supports_secure_restorable_state(&self, _sender: &NSApplication) -> Bool {
            Bool::YES
        }
    }

    unsafe impl NSUserInterfaceValidations for EngineAppDelegate {
        #[unsafe(method(validateUserInterfaceItem:))]
        unsafe fn validate_user_interface_item(&self, item: &ProtocolObject<dyn NSValidatedUserInterfaceItem>) -> Bool {
            const IDC_FIND: isize = 37000;

            let tag = item.tag();
            if tag == IDC_FIND {
                Bool::YES
            } else {
                Bool::NO
            }
        }
    }
}

impl EngineAppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = EngineAppDelegate::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Instance variables of `EngineApplication`.
#[derive(Default)]
pub struct EngineApplicationIvars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    /// A `NSApplication` subclass that implements the required CEF protocols.
    ///
    /// This class provides the necessary `CefAppProtocol` conformance to
    /// ensure that events are handled correctly by the Chromium framework on macOS.
    #[unsafe(super(NSApplication))]
    #[ivars = EngineApplicationIvars]
    pub struct EngineApplication;

    impl EngineApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was_sending_event = self.is_handling_send_event();
            if !was_sending_event {
                self.set_handling_send_event(true);
            }

            let _: () = msg_send![super(self), sendEvent:event];

            if !was_sending_event {
                self.set_handling_send_event(false);
            }
        }

        /// Cmd+Q and the Dock's Quit reach here. The engine serves every tab of a profile, so it
        /// does not quit on its own: Ferryx ends it by closing stdin.
        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: &AnyObject) {}
    }

    unsafe impl CrAppControlProtocol for EngineApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn _set_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for EngineApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn _is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for EngineApplication {}
);

impl EngineApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> Retained<Self>;

        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling_send_event(&self, handling_send_event: bool);

        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling_send_event(&self) -> bool;
    }
}

pub fn setup_application() {
    // Initialize the EngineApplication instance.
    // SAFETY: mtm ensures that here is the main thread.
    let _ = EngineApplication::shared_application();

    // If there was an invocation to NSApp prior to here,
    // then the NSApp will not be a EngineApplication.
    // The following assertion ensures that this doesn't happen.
    assert!(
        NSApp(MainThreadMarker::new().expect("Not running on the main thread"))
            .isKindOfClass(EngineApplication::class())
    );
}

pub fn setup_delegate() -> Retained<EngineAppDelegate> {
    let mtm = MainThreadMarker::new().expect("Not running on the main thread");

    // Create the application delegate.
    let delegate = EngineAppDelegate::new(mtm);
    let delegate_proto =
        ProtocolObject::<dyn NSApplicationDelegate>::from_retained(delegate.clone());
    let app = NSApp(MainThreadMarker::new().expect("Not running on the main thread"));
    assert!(app.isKindOfClass(EngineApplication::class()));
    app.setDelegate(Some(&delegate_proto));
    assert!(
        app.delegate()
            .unwrap()
            .isKindOfClass(EngineAppDelegate::class())
    );

    unsafe {
        delegate.performSelectorOnMainThread_withObject_waitUntilDone(
            sel!(createApplication:),
            None,
            false,
        );
    }

    delegate
}
