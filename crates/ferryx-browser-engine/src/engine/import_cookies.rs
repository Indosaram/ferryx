//! Applies cookies Ferryx imported from another browser to this engine's profile.
//!
//! Ferryx stages them in a file (format in `ferryx_browser_engine_protocol::cookies`); the
//! engine sets each through the global cookie manager, flushes the store so they survive a
//! kill, and deletes the file so nothing is applied twice.

use cef::*;
use ferryx_browser_engine_protocol::cookies::{SameSite, StagedCookie, STAGED_COOKIES_FILE};
use std::path::Path;

/// Microseconds between 1601-01-01 (CEF's base time) and 1970-01-01.
const WINDOWS_EPOCH_DELTA_MICROS: i64 = 11_644_473_600_000_000;

fn to_cef(cookie: &StagedCookie) -> Cookie {
    let expires = cookie.expires_unix.map(|unix| Basetime {
        val: unix.saturating_mul(1_000_000).saturating_add(WINDOWS_EPOCH_DELTA_MICROS),
    });
    Cookie {
        name: CefString::from(cookie.name.as_str()),
        value: CefString::from(cookie.value.as_str()),
        domain: CefString::from(cookie.domain.as_str()),
        path: CefString::from(cookie.path.as_str()),
        secure: cookie.secure as _,
        httponly: cookie.http_only as _,
        has_expires: expires.is_some() as _,
        expires: expires.unwrap_or_default(),
        same_site: match cookie.same_site {
            SameSite::Unspecified => CookieSameSite::UNSPECIFIED,
            SameSite::NoRestriction => CookieSameSite::NO_RESTRICTION,
            SameSite::Lax => CookieSameSite::LAX_MODE,
            SameSite::Strict => CookieSameSite::STRICT_MODE,
        },
        ..Default::default()
    }
}

/// Sets every cookie in `path` and removes the file. Returns how many CEF accepted.
pub fn apply(path: &Path) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    let Some(manager) = cookie_manager_get_global_manager(None) else {
        eprintln!("ferryx-engine: no cookie manager; staged cookies kept for the next start");
        return 0;
    };
    let mut applied = 0;
    for cookie in text.lines().filter_map(StagedCookie::parse) {
        let url = CefString::from(cookie.url().as_str());
        if manager.set_cookie(Some(&url), Some(&to_cef(&cookie)), None) == 1 {
            applied += 1;
        }
    }
    manager.flush_store(None);
    if let Err(error) = std::fs::remove_file(path) {
        eprintln!("ferryx-engine: could not remove {}: {error}", path.display());
    }
    applied
}

/// Applies a file Ferryx staged while this profile's engine was not running.
pub fn apply_pending(cache_dir: &Path) {
    let path = cache_dir.join(STAGED_COOKIES_FILE);
    if path.is_file() {
        apply(&path);
    }
}
