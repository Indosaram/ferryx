//! OS web-authentication session for sign-in handoffs.
//!
//! macOS runs ASWebAuthenticationSession and hands back the callback URL the identity provider
//! redirected to. Other platforms have no such session and report it as unavailable.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
use unsupported as platform;

pub use crate::browser::sign_in_handoff::AuthCallback;

/// How an OS auth session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSessionOutcome {
    /// The provider redirected to the callback; carries the full callback URL.
    Callback(String),
    /// The user closed the session or declined it.
    Cancelled,
    /// The session could not run; carries the reason.
    Unavailable(String),
}

/// Whether this OS can run an auth session that waits for `callback`.
pub fn supports(callback: &AuthCallback) -> bool {
    platform::supports(callback)
}

/// Runs the OS web-authentication session for `authorize_url`. Never panics.
pub async fn run<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    authorize_url: String,
    callback: AuthCallback,
) -> AuthSessionOutcome {
    platform::run(app, authorize_url, callback).await
}

/// Whether ASWebAuthenticationSession on `macos_version` (major, minor) can wait for `callback`:
/// a custom scheme needs 10.15, an https host and path need 14.4.
#[cfg(any(target_os = "macos", test))]
fn callback_supported(callback: &AuthCallback, macos_version: (u64, u64)) -> bool {
    match callback {
        AuthCallback::Scheme { scheme } => macos_version >= (10, 15) && is_custom_scheme(scheme),
        AuthCallback::Https { host, path } => {
            macos_version >= (14, 4)
                && !host.is_empty()
                && !host.contains(['/', ':', '@', '?', '#'])
                && path.starts_with('/')
        }
    }
}

/// An RFC 3986 scheme (ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )) other than http and https.
#[cfg(any(target_os = "macos", test))]
fn is_custom_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let starts_with_letter = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic());
    starts_with_letter
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !scheme.eq_ignore_ascii_case("http")
        && !scheme.eq_ignore_ascii_case("https")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheme(value: &str) -> AuthCallback {
        AuthCallback::Scheme {
            scheme: value.to_string(),
        }
    }

    fn https(host: &str, path: &str) -> AuthCallback {
        AuthCallback::Https {
            host: host.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn custom_scheme_needs_catalina() {
        let callback = scheme("com.example.app");
        assert!(!callback_supported(&callback, (10, 14)));
        assert!(callback_supported(&callback, (10, 15)));
        assert!(callback_supported(&callback, (11, 0)));
        assert!(callback_supported(&callback, (15, 2)));
    }

    #[test]
    fn scheme_must_be_rfc3986_and_not_web() {
        let version = (15, 0);
        for valid in ["com.example.app", "myapp", "x-callback+v1.2", "A1"] {
            assert!(callback_supported(&scheme(valid), version), "{valid}");
        }
        for invalid in [
            "", "1app", "-app", "my app", "my_app", "app:", "http", "https", "HTTPS",
        ] {
            assert!(!callback_supported(&scheme(invalid), version), "{invalid}");
        }
    }

    #[test]
    fn https_callback_needs_sonoma_14_4() {
        let callback = https("app.example.com", "/oauth/callback");
        assert!(!callback_supported(&callback, (10, 15)));
        assert!(!callback_supported(&callback, (14, 3)));
        assert!(callback_supported(&callback, (14, 4)));
        assert!(callback_supported(&callback, (15, 0)));
    }

    #[test]
    fn https_callback_needs_host_and_absolute_path() {
        let version = (15, 0);
        assert!(callback_supported(&https("app.example.com", "/"), version));
        assert!(!callback_supported(&https("", "/cb"), version));
        assert!(!callback_supported(
            &https("app.example.com:443", "/cb"),
            version
        ));
        assert!(!callback_supported(
            &https("app.example.com/cb", "/cb"),
            version
        ));
        assert!(!callback_supported(
            &https("app.example.com", "cb"),
            version
        ));
        assert!(!callback_supported(&https("app.example.com", ""), version));
    }
}
