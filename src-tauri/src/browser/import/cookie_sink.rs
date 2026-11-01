//! Imported cookies go straight into the target Ferryx profile, in both engines:
//!
//! - Chromium engine (Windows, macOS when installed): written to the profile's staged-cookie
//!   file (`ferryx_browser_engine_protocol::cookies`) in its cache directory; a running engine
//!   is told to apply it now, otherwise it applies it when it next starts.
//! - System webview: set on the profile's open webviews through the caller's `apply` hook
//!   (the webview cookie store is reachable only through a live webview).
//!
//! Google cookies are skipped, as in Ferryx's earlier cookie import: Google binds sessions to
//! the browser that created them and signs the account out everywhere when they move.

use super::{CookieEntry, SinkError};
use crate::browser::cookies::{cookie_from_imported, is_google_cookie, ImportedCookie};
use ferryx_browser_engine_protocol::cookies::{SameSite, StagedCookie, STAGED_COOKIES_FILE};
use parking_lot::Mutex;
use std::io::Write;
use std::path::PathBuf;

/// Applies cookies to the profile's open system webviews; returns how many webviews took them.
pub type WebviewApply = Box<dyn Fn(&[cookie::Cookie<'static>]) -> Result<usize, String> + Send + Sync>;
/// Tells a running Chromium engine to apply the staged file; false when none is running.
pub type EngineNotify = Box<dyn Fn(&std::path::Path) -> bool + Send + Sync>;

pub struct CookieSink {
    /// The profile's Chromium cache directory, when the Chromium engine is installed.
    cef_cache_dir: Option<PathBuf>,
    notify_engine: EngineNotify,
    apply_webviews: WebviewApply,
    now_unix: i64,
    collected: Mutex<Vec<ImportedCookie>>,
    google_skipped: Mutex<usize>,
}

/// What [`CookieSink::finish`] did, for the per-type notes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CookieDelivery {
    pub cookies: usize,
    pub google_skipped: usize,
    pub staged_for_engine: bool,
    pub engine_running: bool,
    pub webviews: usize,
}

/// Chromium and Firefox share these SameSite numbers; Safari reports -1.
fn same_site_name(value: i32) -> Option<&'static str> {
    match value {
        0 => Some("none"),
        1 => Some("lax"),
        2 => Some("strict"),
        _ => None,
    }
}

/// The importable form of a source cookie, or `None` for one with no host or no name, or one
/// that has already expired.
pub fn imported_cookie(entry: &CookieEntry, now_unix: i64) -> Option<ImportedCookie> {
    let name = entry.name.trim();
    let host = entry.host.trim();
    if name.is_empty() || host.is_empty() {
        return None;
    }
    let expires_unix = (entry.expires > 0).then_some(entry.expires);
    if expires_unix.is_some_and(|expires| expires <= now_unix) {
        return None;
    }
    Some(ImportedCookie {
        name: name.to_string(),
        value: entry.value.clone(),
        domain: Some(host.to_string()),
        path: if entry.path.starts_with('/') { entry.path.clone() } else { "/".to_string() },
        secure: entry.is_secure,
        http_only: entry.is_httponly,
        expires_unix,
        same_site: same_site_name(entry.same_site).map(str::to_string),
    })
}

fn staged(cookie: &ImportedCookie) -> StagedCookie {
    StagedCookie {
        domain: cookie.domain.clone().unwrap_or_default(),
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        path: cookie.path.clone(),
        expires_unix: cookie.expires_unix,
        secure: cookie.secure,
        http_only: cookie.http_only,
        same_site: match cookie.same_site.as_deref() {
            Some("none") => SameSite::NoRestriction,
            Some("lax") => SameSite::Lax,
            Some("strict") => SameSite::Strict,
            _ => SameSite::Unspecified,
        },
    }
}

impl CookieSink {
    pub fn new(cef_cache_dir: Option<PathBuf>, notify_engine: EngineNotify, apply_webviews: WebviewApply) -> Self {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        Self {
            cef_cache_dir,
            notify_engine,
            apply_webviews,
            now_unix,
            collected: Mutex::new(Vec::new()),
            google_skipped: Mutex::new(0),
        }
    }

    /// Keeps the cookie for [`Self::finish`]; `Ok(false)` when it is not importable.
    pub fn add(&self, entry: &CookieEntry) -> Result<bool, SinkError> {
        let Some(cookie) = imported_cookie(entry, self.now_unix) else {
            return Ok(false);
        };
        if is_google_cookie(cookie.domain.as_deref(), &cookie.name) {
            *self.google_skipped.lock() += 1;
            return Ok(false);
        }
        self.collected.lock().push(cookie);
        Ok(true)
    }

    fn stage_for_engine(&self, dir: &std::path::Path, cookies: &[ImportedCookie]) -> Result<PathBuf, SinkError> {
        std::fs::create_dir_all(dir).map_err(|error| SinkError::Failed(format!("{}: {error}", dir.display())))?;
        let path = dir.join(STAGED_COOKIES_FILE);
        // Append: a file from an earlier import that the engine has not applied yet stays valid.
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|error| SinkError::Failed(format!("{}: {error}", path.display())))?;
        let mut writer = std::io::BufWriter::new(file);
        for line in cookies.iter().filter_map(|cookie| staged(cookie).to_line()) {
            writeln!(writer, "{line}").map_err(|error| SinkError::Failed(error.to_string()))?;
        }
        writer.flush().map_err(|error| SinkError::Failed(error.to_string()))?;
        Ok(path)
    }

    /// Delivers the collected cookies to both engines.
    pub fn finish(&self) -> Result<CookieDelivery, SinkError> {
        let cookies = std::mem::take(&mut *self.collected.lock());
        let mut delivery = CookieDelivery {
            cookies: cookies.len(),
            google_skipped: std::mem::take(&mut *self.google_skipped.lock()),
            ..Default::default()
        };
        if cookies.is_empty() {
            return Ok(delivery);
        }
        if let Some(dir) = self.cef_cache_dir.as_deref() {
            let path = self.stage_for_engine(dir, &cookies)?;
            delivery.staged_for_engine = true;
            delivery.engine_running = (self.notify_engine)(&path);
        }
        let webview_cookies: Vec<cookie::Cookie<'static>> = cookies
            .into_iter()
            .filter_map(|cookie| cookie_from_imported(cookie).ok())
            .collect();
        delivery.webviews = (self.apply_webviews)(&webview_cookies).map_err(SinkError::Failed)?;
        Ok(delivery)
    }

    /// Drops the collected cookies (a cancelled or failed cookie import).
    pub fn discard(&self) {
        self.collected.lock().clear();
        *self.google_skipped.lock() = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn entry(host: &str, name: &str, expires: i64) -> CookieEntry {
        CookieEntry {
            host: host.into(),
            name: name.into(),
            value: "v".into(),
            path: "/".into(),
            expires,
            is_secure: true,
            is_httponly: false,
            same_site: 1,
        }
    }

    #[test]
    fn expired_nameless_and_hostless_cookies_are_not_importable() {
        assert!(imported_cookie(&entry(".a.test", "sid", 0), 100).is_some(), "session cookie");
        assert!(imported_cookie(&entry(".a.test", "sid", 99), 100).is_none(), "expired");
        assert!(imported_cookie(&entry("", "sid", 0), 100).is_none());
        assert!(imported_cookie(&entry(".a.test", " ", 0), 100).is_none());
        let kept = imported_cookie(&entry(".a.test", "sid", 200), 100).unwrap();
        assert_eq!(kept.expires_unix, Some(200));
        assert_eq!(kept.same_site.as_deref(), Some("lax"));
    }

    #[test]
    fn finish_stages_for_the_engine_notifies_it_and_sets_webview_cookies() {
        let cache = tempfile::tempdir().unwrap();
        let notified = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let applied = Arc::new(Mutex::new(Vec::<String>::new()));
        let (n, a) = (Arc::clone(&notified), Arc::clone(&applied));
        let sink = CookieSink::new(
            Some(cache.path().to_path_buf()),
            Box::new(move |path: &std::path::Path| {
                n.lock().push(path.to_path_buf());
                true
            }),
            Box::new(move |cookies: &[cookie::Cookie<'static>]| -> Result<usize, String> {
                a.lock().extend(cookies.iter().map(|c| c.name().to_string()));
                Ok(1)
            }),
        );
        let far = i64::from(i32::MAX);
        assert!(sink.add(&entry(".a.test", "sid", far)).unwrap());
        assert!(!sink.add(&entry(".google.com", "SID", far)).unwrap());

        let delivery = sink.finish().unwrap();
        assert_eq!(
            delivery,
            CookieDelivery { cookies: 1, google_skipped: 1, staged_for_engine: true, engine_running: true, webviews: 1 }
        );
        let staged_path = cache.path().join(STAGED_COOKIES_FILE);
        assert_eq!(*notified.lock(), vec![staged_path.clone()]);
        let text = std::fs::read_to_string(staged_path).unwrap();
        let parsed: Vec<StagedCookie> = text.lines().filter_map(StagedCookie::parse).collect();
        assert_eq!(parsed.len(), 1);
        assert_eq!((parsed[0].domain.as_str(), parsed[0].name.as_str()), (".a.test", "sid"));
        assert_eq!(parsed[0].same_site, SameSite::Lax);
        assert_eq!(*applied.lock(), vec!["sid".to_string()]);
    }

    #[test]
    fn without_the_chromium_engine_only_webviews_get_the_cookies() {
        let sink = CookieSink::new(
            None,
            Box::new(|_: &std::path::Path| -> bool { panic!("no engine to notify") }),
            Box::new(|c: &[cookie::Cookie<'static>]| -> Result<usize, String> { Ok(c.len()) }),
        );
        sink.add(&entry("a.test", "x", 0)).unwrap();
        let delivery = sink.finish().unwrap();
        assert!(!delivery.staged_for_engine);
        assert_eq!(delivery.webviews, 1);
    }
}
