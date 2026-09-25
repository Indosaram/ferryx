use tauri::Url;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum BrowserError {
    #[error("Browser not found: {0}")]
    NotFound(String),
    #[error("Webview not found for browser session: {0}")]
    WebviewNotFound(String),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Scheme denied for URL: {0} (only http, https, and about:blank allowed)")]
    SchemeDenied(String),
    #[error("Invalid bounds provided")]
    InvalidBounds,
    #[error("Unsupported profile: {0}")]
    UnsupportedProfile(String),
    #[error("Failed to create browser webview: {0}")]
    CreateFailed(String),
    #[error("Navigation failed: {0}")]
    NavigationFailed(String),
    #[error("History navigation failed: {0}")]
    HistoryFailed(String),
    #[error("Find in page failed: {0}")]
    FindFailed(String),
    #[error("Browser download failed: {0}")]
    DownloadFailed(String),
    #[error("Cookie import failed: {0}")]
    CookieImport(String),
    #[error("Failed to close browser: {0}")]
    CloseFailed(String),
    #[error("Platform unsupported: {0}")]
    PlatformUnsupported(String),
    #[error("Browser automation snapshot is stale")]
    AutomationSnapshotStale,
    #[error("Browser automation element not found: {0}")]
    AutomationTargetNotFound(String),
    #[error("Browser automation failed: {0}")]
    AutomationFailed(String),
    #[error("Browser CLI unavailable: {0}")]
    CliUnavailable(String),
    #[error("Internal browser error: {0}")]
    Internal(String),
}

pub fn default_desktop_user_agent() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
    }
    #[cfg(target_os = "windows")]
    {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
    }
    #[cfg(target_os = "linux")]
    {
        static LINUX_USER_AGENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        LINUX_USER_AGENT
            .get_or_init(|| {
                linux_user_agent(
                    linux_arch_token(),
                    linux_display_server_token(
                        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
                        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
                        std::env::var("DISPLAY").ok().as_deref(),
                    ),
                )
            })
            .as_str()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
    }
}

/// Real target architecture, so an aarch64 host does not claim x86_64.
#[cfg(target_os = "linux")]
fn linux_arch_token() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        std::env::consts::ARCH
    }
}

/// Wayland sessions must not be advertised as X11, and vice versa.
#[cfg(target_os = "linux")]
fn linux_display_server_token(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> &'static str {
    let session_type = session_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    match session_type.as_deref() {
        Some("wayland") => "Wayland",
        Some("x11") => "X11",
        _ if wayland_display.is_some_and(|value| !value.trim().is_empty()) => "Wayland",
        _ if display.is_some_and(|value| !value.trim().is_empty()) => "X11",
        _ => "X11",
    }
}

#[cfg(target_os = "linux")]
fn linux_user_agent(arch: &str, display_server: &str) -> String {
    format!(
        "Mozilla/5.0 ({display_server}; Linux {arch}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
    )
}

const MAX_URL_LENGTH_BYTES: usize = 8192;

pub fn validate_url(url_str: &str) -> Result<String, BrowserError> {
    let trimmed = url_str.trim();
    if trimmed.is_empty() {
        return Ok("about:blank".to_string());
    }
    if trimmed == "about:blank" {
        return Ok(trimmed.to_string());
    }
    if trimmed.len() > MAX_URL_LENGTH_BYTES {
        return Err(BrowserError::InvalidUrl(format!(
            "URL exceeds the maximum length of {MAX_URL_LENGTH_BYTES} bytes"
        )));
    }

    let parsed = Url::parse(trimmed).map_err(|e| BrowserError::InvalidUrl(e.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(BrowserError::InvalidUrl(
            "URLs with embedded credentials (user:password@host) are not allowed".to_string(),
        ));
    }
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        "about" if parsed.path() == "blank" => Ok("about:blank".to_string()),
        _ => Err(BrowserError::SchemeDenied(parsed.to_string())),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_linux_user_agent_reports_target_arch() {
        #[cfg(target_arch = "aarch64")]
        assert_eq!(linux_arch_token(), "aarch64");
        #[cfg(target_arch = "x86_64")]
        assert_eq!(linux_arch_token(), "x86_64");
        assert_eq!(linux_arch_token(), std::env::consts::ARCH);

        let ua = linux_user_agent(linux_arch_token(), "X11");
        assert!(
            ua.contains(&format!("(X11; Linux {})", std::env::consts::ARCH)),
            "unexpected Linux user agent: {ua}"
        );
        assert!(
            ua.contains("AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36")
        );
    }

    #[test]
    fn test_linux_user_agent_reports_real_display_server() {
        assert_eq!(linux_display_server_token(Some("wayland"), None, Some(":0")), "Wayland");
        assert_eq!(
            linux_display_server_token(Some("x11"), Some("/run/user/1000/wayland-0"), None),
            "X11"
        );
        assert_eq!(linux_display_server_token(Some("  Wayland "), None, None), "Wayland");
        assert_eq!(
            linux_display_server_token(Some("tty"), Some("/run/user/1000/wayland-0"), None),
            "Wayland"
        );
        assert_eq!(linux_display_server_token(None, None, Some(":0")), "X11");
        assert_eq!(linux_display_server_token(None, None, None), "X11");
        assert_eq!(linux_display_server_token(Some(""), Some(""), Some("")), "X11");

        assert_eq!(
            linux_user_agent("aarch64", "Wayland"),
            "Mozilla/5.0 (Wayland; Linux aarch64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
        );
        assert_eq!(
            linux_user_agent("x86_64", "X11"),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
        );
    }
}
