//! Windows and Linux have no OS web-authentication session.

use super::{AuthCallback, AuthSessionOutcome};

const UNAVAILABLE: &str = "no OS auth session on this platform";

pub(super) fn supports(_callback: &AuthCallback) -> bool {
    false
}

pub(super) async fn run<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _authorize_url: String,
    _callback: AuthCallback,
) -> AuthSessionOutcome {
    AuthSessionOutcome::Unavailable(UNAVAILABLE.to_string())
}
