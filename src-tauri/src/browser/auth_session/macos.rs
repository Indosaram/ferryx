//! ASWebAuthenticationSession: the system sign-in sheet over the Ferryx window.
//!
//! A custom-scheme callback works on macOS 10.15+. An https callback (host + path) needs
//! macOS 14.4+, and Apple requires that host to be an associated domain of the app; when the
//! session cannot match it the user can cancel and the bar falls back to the next in-app action.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSWindow};
use objc2_authentication_services::{
    ASPresentationAnchor, ASWebAuthenticationPresentationContextProviding,
    ASWebAuthenticationSession, ASWebAuthenticationSessionCallback,
    ASWebAuthenticationSessionErrorCode, ASWebAuthenticationSessionErrorDomain,
};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSProcessInfo, NSString, NSURL};
use tauri::Manager;
use tokio::sync::oneshot;

use super::{callback_supported, AuthCallback, AuthSessionOutcome};

/// The running session and its presentation provider, kept alive until the session completes
/// (the session holds the provider weakly). Main thread only.
struct ActiveSession {
    id: u64,
    session: Retained<ASWebAuthenticationSession>,
    _provider: Retained<AnchorProvider>,
}

thread_local! {
    static ACTIVE: RefCell<Option<ActiveSession>> = const { RefCell::new(None) };
}

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

struct AnchorIvars {
    window: Retained<NSWindow>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The presentation-context protocol is MainThreadOnly; AppKit asks for the anchor on
    //   the main thread.
    // - AnchorProvider does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FerryxAuthSessionAnchor"]
    #[ivars = AnchorIvars]
    struct AnchorProvider;

    unsafe impl NSObjectProtocol for AnchorProvider {}

    unsafe impl ASWebAuthenticationPresentationContextProviding for AnchorProvider {
        #[unsafe(method_id(presentationAnchorForWebAuthenticationSession:))]
        fn presentation_anchor(
            &self,
            _session: &ASWebAuthenticationSession,
        ) -> Retained<ASPresentationAnchor> {
            self.ivars().window.clone().into_super().into_super()
        }
    }
);

impl AnchorProvider {
    fn new(mtm: MainThreadMarker, window: Retained<NSWindow>) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(AnchorIvars { window });
        // SAFETY: freshly allocated with ivars set; NSObject's init is always valid.
        unsafe { msg_send![super(this), init] }
    }
}

/// Delivers the outcome once, from whichever path finishes first.
struct Reply(Mutex<Option<oneshot::Sender<AuthSessionOutcome>>>);

impl Reply {
    fn send(&self, outcome: AuthSessionOutcome) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(outcome);
        }
    }
}

fn macos_version() -> (u64, u64) {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    (
        u64::try_from(version.majorVersion).unwrap_or(0),
        u64::try_from(version.minorVersion).unwrap_or(0),
    )
}

pub(super) fn supports(callback: &AuthCallback) -> bool {
    callback_supported(callback, macos_version())
}

pub(super) async fn run<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    authorize_url: String,
    callback: AuthCallback,
) -> AuthSessionOutcome {
    if !supports(&callback) {
        return AuthSessionOutcome::Unavailable(
            "this macOS version cannot complete this sign-in in the system sheet".to_string(),
        );
    }
    let (sender, receiver) = oneshot::channel();
    let reply = Arc::new(Reply(Mutex::new(Some(sender))));
    let main_reply = Arc::clone(&reply);
    let main_app = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let started = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            start_session(&main_app, &authorize_url, &callback, &main_reply)
        }));
        let failure = match started {
            Ok(Ok(())) => return,
            Ok(Err(reason)) => reason,
            Err(Some(exception)) => format!("the sign-in sheet failed to start: {exception}"),
            Err(None) => "the sign-in sheet failed to start".to_string(),
        };
        main_reply.send(AuthSessionOutcome::Unavailable(failure));
    });
    if let Err(error) = scheduled {
        reply.send(AuthSessionOutcome::Unavailable(format!(
            "could not reach the main thread: {error}"
        )));
    }
    // Only the main-thread task and the session keep the sender from here on, so a task that
    // is dropped without running ends the wait as Cancelled.
    drop(reply);
    receiver.await.unwrap_or(AuthSessionOutcome::Cancelled)
}

/// Main thread: creates, retains and starts the session.
fn start_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    authorize_url: &str,
    callback: &AuthCallback,
    reply: &Arc<Reply>,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("the sign-in sheet must start on the main thread")?;
    let window =
        anchor_window(app, mtm).ok_or("no Ferryx window to show the sign-in sheet over")?;
    let url = NSURL::URLWithString(&NSString::from_str(authorize_url))
        .ok_or("the sign-in authorize URL is invalid")?;

    let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let completion_reply = Arc::clone(reply);
    let completion = RcBlock::new(move |callback_url: *mut NSURL, error: *mut NSError| {
        // SAFETY: the session passes either null or a valid NSURL / NSError that stays alive
        // for the duration of the callback.
        let outcome = unsafe { completion_outcome(callback_url.as_ref(), error.as_ref()) };
        completion_reply.send(outcome);
        // Release the session after this callback returns, on the main thread that owns it.
        dispatch2::DispatchQueue::main().exec_async(move || release_session(id));
    });

    // Sent directly so a nil init is an error, not a panic. The scheme initializer's typed
    // binding is deprecated in favour of the 14.4+ callback object, which older systems lack.
    let session: Option<Retained<ASWebAuthenticationSession>> = match callback {
        AuthCallback::Scheme { scheme } => {
            let scheme = NSString::from_str(scheme);
            // SAFETY: documented initializer (macOS 10.15+); the session copies the block.
            unsafe {
                msg_send![
                    ASWebAuthenticationSession::alloc(),
                    initWithURL: &*url,
                    callbackURLScheme: &*scheme,
                    completionHandler: &*completion
                ]
            }
        }
        AuthCallback::Https { host, path } => {
            // SAFETY: supports() confirmed macOS 14.4+, where the callback object and this
            // initializer exist; the session copies the block.
            unsafe {
                let matcher = ASWebAuthenticationSessionCallback::callbackWithHTTPSHost_path(
                    &NSString::from_str(host),
                    &NSString::from_str(path),
                );
                msg_send![
                    ASWebAuthenticationSession::alloc(),
                    initWithURL: &*url,
                    callback: &*matcher,
                    completionHandler: &*completion
                ]
            }
        }
    };
    let session = session.ok_or("could not create the system sign-in sheet")?;

    let provider = AnchorProvider::new(mtm, window);
    // SAFETY: plain property setters on a session that has not started yet.
    unsafe {
        session.setPresentationContextProvider(Some(ProtocolObject::<
            dyn ASWebAuthenticationPresentationContextProviding,
        >::from_ref(&*provider)));
        session.setPrefersEphemeralWebBrowserSession(false);
    }
    // SAFETY: canStart has no side effects.
    if !unsafe { session.canStart() } {
        return Err("the system sign-in sheet cannot start".to_string());
    }

    // A newer request replaces a sheet that is still open; the old caller gets Cancelled.
    let previous = ACTIVE.with(|active| {
        active.borrow_mut().replace(ActiveSession {
            id,
            session: session.clone(),
            _provider: provider,
        })
    });
    if let Some(previous) = previous {
        // SAFETY: cancelling an open or finished session is allowed and idempotent.
        unsafe { previous.session.cancel() };
    }

    // SAFETY: start is called once on a fully configured session, on the main thread.
    if !unsafe { session.start() } {
        release_session(id);
        return Err("the system sign-in sheet did not start".to_string());
    }
    Ok(())
}

/// The "main" Ferryx window, else the key or main AppKit window.
fn anchor_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mtm: MainThreadMarker,
) -> Option<Retained<NSWindow>> {
    let main = app
        .get_webview_window("main")
        .and_then(|window| window.ns_window().ok())
        // SAFETY: tauri returns the live NSWindow of the main window (or null); retaining it
        // keeps it valid for the session.
        .and_then(|raw| unsafe { Retained::retain(raw.cast::<NSWindow>()) });
    main.or_else(|| {
        let application = NSApplication::sharedApplication(mtm);
        application.keyWindow().or_else(|| application.mainWindow())
    })
}

fn completion_outcome(callback_url: Option<&NSURL>, error: Option<&NSError>) -> AuthSessionOutcome {
    if let Some(callback_url) = callback_url {
        return match callback_url.absoluteString() {
            Some(text) => AuthSessionOutcome::Callback(text.to_string()),
            None => {
                AuthSessionOutcome::Unavailable("the sign-in callback URL is empty".to_string())
            }
        };
    }
    let Some(error) = error else {
        return AuthSessionOutcome::Unavailable(
            "the sign-in sheet ended without a result".to_string(),
        );
    };
    // SAFETY: AuthenticationServices exports this constant for the life of the process.
    let domain = unsafe { ASWebAuthenticationSessionErrorDomain };
    if error.domain().isEqualToString(domain)
        && error.code() == ASWebAuthenticationSessionErrorCode::CanceledLogin.0
    {
        AuthSessionOutcome::Cancelled
    } else {
        AuthSessionOutcome::Unavailable(error.localizedDescription().to_string())
    }
}

/// Drops the retained session `id` unless a newer one replaced it.
fn release_session(id: u64) {
    let released = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        if active.as_ref().is_some_and(|current| current.id == id) {
            active.take()
        } else {
            None
        }
    });
    drop(released);
}
