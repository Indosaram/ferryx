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
    #[error("Cookie import failed: {0}")]
    CookieImport(String),
    #[error("Failed to close browser: {0}")]
    CloseFailed(String),
    #[error("Platform unsupported: {0}")]
    PlatformUnsupported(String),
    #[error("Internal browser error: {0}")]
    Internal(String),
}

pub fn validate_url(url_str: &str) -> Result<String, BrowserError> {
    let trimmed = url_str.trim();
    if trimmed.is_empty() {
        return Ok("about:blank".to_string());
    }
    if trimmed == "about:blank" {
        return Ok(trimmed.to_string());
    }

    let parsed = Url::parse(trimmed).map_err(|e| BrowserError::InvalidUrl(e.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        "about" if parsed.path() == "blank" => Ok("about:blank".to_string()),
        _ => Err(BrowserError::SchemeDenied(parsed.to_string())),
    }
}
