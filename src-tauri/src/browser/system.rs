//! Engine-neutral logic behind downloads, per-site zoom, file choosers and page notifications.
//!
//! Pure functions and a small store, so both engines (Chromium and the system webview) share
//! one behaviour and the rules are testable without a window.

use crate::browser::site_permissions::{SitePermission, SitePermissionDecision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const ZOOM_MIN: f64 = 0.25;
pub const ZOOM_MAX: f64 = 5.0;
/// The zoom steps Cmd/Ctrl +/- walk through, as Chrome does.
pub const ZOOM_STEPS: [f64; 15] =
    [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 3.0, 5.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoomDirection {
    In,
    Out,
    Reset,
}

/// The next zoom from `current`: the next step up or down, or 100%.
pub fn step_zoom(current: f64, direction: ZoomDirection) -> f64 {
    let current = if current.is_finite() { current } else { 1.0 };
    match direction {
        ZoomDirection::Reset => 1.0,
        ZoomDirection::In => ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > current + 1e-6)
            .unwrap_or(ZOOM_MAX),
        ZoomDirection::Out => ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current - 1e-6)
            .unwrap_or(ZOOM_MIN),
    }
}

/// What a zoom is remembered under: the host (`example.com:8080`), lowercased. Pages without
/// a host (about:blank, data:) have no per-site zoom.
pub fn zoom_site_key(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// Per-site zoom, persisted as JSON (`<data>/browser-site-zoom.json`). 100% is not stored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SiteZoomStore {
    #[serde(default)]
    sites: BTreeMap<String, f64>,
}

pub const SITE_ZOOM_FILE: &str = "browser-site-zoom.json";
/// Bounds the file; the oldest-sorted entries beyond it are dropped.
const SITE_ZOOM_CAP: usize = 2000;

impl SiteZoomStore {
    pub fn get(&self, url: &str) -> Option<f64> {
        self.sites.get(&zoom_site_key(url)?).copied()
    }

    /// Remembers `zoom` for the page's site; returns false when the page has no site.
    pub fn set(&mut self, url: &str, zoom: f64) -> bool {
        let Some(key) = zoom_site_key(url) else { return false };
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if (zoom - 1.0).abs() < 1e-6 {
            self.sites.remove(&key);
        } else {
            self.sites.insert(key, zoom);
            while self.sites.len() > SITE_ZOOM_CAP {
                let Some(first) = self.sites.keys().next().cloned() else { break };
                self.sites.remove(&first);
            }
        }
        true
    }

    /// Reads the store; a missing or unreadable file is an empty store (zoom is a preference,
    /// not data worth failing a tab over). Blocking: run through `run_blocking`.
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)
                .map(|mut store| {
                    store.sites.retain(|_, zoom| zoom.is_finite() && *zoom >= ZOOM_MIN && *zoom <= ZOOM_MAX);
                    store
                })
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, path = %path.display(), "ignoring an unreadable site zoom file");
                    Self::default()
                }),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, path = %path.display(), "could not read the site zoom file");
                }
                Self::default()
            }
        }
    }

    /// Writes the store through a temp file and a rename. Blocking: run through `run_blocking`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, json)?;
        std::fs::rename(&temp, path)
    }
}

/// A file name a page suggested, reduced to one safe path component.
pub fn safe_file_name(suggested: &str) -> String {
    let base = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|ch| if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { ch })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned
    }
}

/// Where a download lands without asking: `dir/name`, or `name (1).ext`, `name (2).ext`... when
/// taken. `exists` is injected so the rule is testable; callers pass `Path::exists`.
pub fn unique_download_path(dir: &Path, suggested: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = safe_file_name(suggested);
    let candidate = dir.join(&name);
    if !exists(&candidate) {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name.as_str(), ""),
    };
    (1..10_000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|path| !exists(path))
        .unwrap_or_else(|| dir.join(format!("{stem} ({}){ext}", uuid::Uuid::new_v4())))
}

/// One file-dialog filter from an `<input accept>` list: extensions only (MIME types become the
/// extensions the platform dialogs understand for the common families).
pub fn accept_extensions(accept: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |ext: &str| {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        if !ext.is_empty() && ext.chars().all(|ch| ch.is_ascii_alphanumeric()) && !out.contains(&ext) {
            out.push(ext);
        }
    };
    for filter in accept {
        let filter = filter.trim();
        if filter.starts_with('.') {
            push(filter);
            continue;
        }
        let family: &[&str] = match filter.to_ascii_lowercase().as_str() {
            "image/*" => &["png", "jpg", "jpeg", "gif", "webp", "heic", "bmp", "svg"],
            "video/*" => &["mp4", "mov", "webm", "mkv", "avi"],
            "audio/*" => &["mp3", "wav", "m4a", "aac", "ogg", "flac"],
            "image/png" => &["png"],
            "image/jpeg" => &["jpg", "jpeg"],
            "image/gif" => &["gif"],
            "image/webp" => &["webp"],
            "application/pdf" => &["pdf"],
            "text/plain" => &["txt"],
            "text/csv" => &["csv"],
            "application/json" => &["json"],
            "application/zip" => &["zip"],
            // An unknown type: no filter, so the user is never locked out of a valid file.
            _ => return Vec::new(),
        };
        for ext in family {
            push(ext);
        }
    }
    out
}

/// A message from the page's Notification shim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PageNotificationMessage {
    Request,
    Show {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        tag: String,
    },
}

/// Bounds the JSON a page can hand the host.
pub const MAX_PAGE_MESSAGE_BYTES: usize = 4096;

pub fn parse_page_notification(raw: &str) -> Option<PageNotificationMessage> {
    if raw.len() > MAX_PAGE_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_str(raw).ok()
}

/// `scheme://host[:port]` of a page, the unit permissions are decided for.
pub fn page_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    match parsed.origin() {
        origin @ url::Origin::Tuple(..) => Some(origin.ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

/// What the host does with one shim message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageNotificationAction {
    /// Answer the shim with this permission state (`granted`/`denied`/`default`).
    Answer(ferryx_browser_engine_protocol::PagePermission),
    /// Show a Ferryx notification.
    Show { title: String, body: String },
    Ignore,
}

/// The permission state to push into a page that finished loading.
pub fn page_permission_state(
    url: &str,
    decide: impl Fn(&str, SitePermission) -> SitePermissionDecision,
) -> ferryx_browser_engine_protocol::PagePermission {
    use ferryx_browser_engine_protocol::PagePermission;
    match page_origin(url).map(|origin| decide(&origin, SitePermission::Notifications)) {
        Some(SitePermissionDecision::Granted) => PagePermission::Granted,
        Some(SitePermissionDecision::Ask) => PagePermission::Default,
        Some(SitePermissionDecision::Denied) | None => PagePermission::Denied,
    }
}

/// Decides a shim message with `decide` (the site-permissions seam).
pub fn page_notification_action(
    url: &str,
    message: &PageNotificationMessage,
    decide: impl Fn(&str, SitePermission) -> SitePermissionDecision,
) -> PageNotificationAction {
    use ferryx_browser_engine_protocol::PagePermission;
    let Some(origin) = page_origin(url) else {
        return match message {
            PageNotificationMessage::Show { .. } => PageNotificationAction::Ignore,
            _ => PageNotificationAction::Answer(PagePermission::Denied),
        };
    };
    let decision = decide(&origin, SitePermission::Notifications);
    let state = match decision {
        SitePermissionDecision::Granted => PagePermission::Granted,
        SitePermissionDecision::Denied => PagePermission::Denied,
        SitePermissionDecision::Ask => PagePermission::Default,
    };
    match message {
        // A request while undecided stays pending: the policy answers when the user does.
        PageNotificationMessage::Request if decision == SitePermissionDecision::Ask => PageNotificationAction::Ignore,
        PageNotificationMessage::Request => PageNotificationAction::Answer(state),
        PageNotificationMessage::Show { title, body, .. } if decision == SitePermissionDecision::Granted => {
            let host = url::Url::parse(url).ok().and_then(|parsed| parsed.host_str().map(str::to_string));
            let title = crate::notification::sanitize_text(title);
            let body = crate::notification::sanitize_text(body);
            PageNotificationAction::Show {
                title: crate::notification::truncate_text(
                    if title.is_empty() { host.as_deref().unwrap_or("Website") } else { &title },
                    crate::notification::MAX_TITLE_LEN,
                ),
                body: crate::notification::truncate_text(
                    &match (host, body.is_empty()) {
                        (Some(host), true) => host,
                        (Some(host), false) => format!("{host}: {body}"),
                        (None, _) => body,
                    },
                    crate::notification::MAX_BODY_LEN,
                ),
            }
        }
        PageNotificationMessage::Show { .. } => PageNotificationAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferryx_browser_engine_protocol::PagePermission;

    #[test]
    fn zoom_steps_walk_the_ladder_and_clamp() {
        assert_eq!(step_zoom(1.0, ZoomDirection::In), 1.1);
        assert_eq!(step_zoom(1.0, ZoomDirection::Out), 0.9);
        assert_eq!(step_zoom(1.05, ZoomDirection::In), 1.1);
        assert_eq!(step_zoom(1.05, ZoomDirection::Out), 1.0);
        assert_eq!(step_zoom(5.0, ZoomDirection::In), ZOOM_MAX);
        assert_eq!(step_zoom(0.25, ZoomDirection::Out), ZOOM_MIN);
        assert_eq!(step_zoom(3.0, ZoomDirection::Reset), 1.0);
        assert_eq!(step_zoom(f64::NAN, ZoomDirection::In), 1.1);
    }

    #[test]
    fn site_zoom_is_kept_per_host_and_survives_a_reload_of_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SITE_ZOOM_FILE);
        let mut store = SiteZoomStore::load(&path);
        assert_eq!(store.get("https://docs.test/a"), None);
        assert!(store.set("https://Docs.test/a", 1.25));
        assert!(store.set("http://localhost:3000/", 0.8));
        assert!(!store.set("about:blank", 2.0));
        assert_eq!(store.get("https://docs.test/other/page"), Some(1.25));
        assert_eq!(store.get("https://other.test/"), None);
        store.save(&path).unwrap();

        let mut reloaded = SiteZoomStore::load(&path);
        assert_eq!(reloaded, store);
        assert_eq!(reloaded.get("http://localhost:3000/x"), Some(0.8));
        assert_eq!(reloaded.get("http://localhost:4000/x"), None);
        assert!(reloaded.set("https://docs.test/", 1.0));
        assert_eq!(reloaded.get("https://docs.test/"), None, "100% is not stored");
    }

    #[test]
    fn a_corrupt_zoom_file_loads_empty_and_bad_values_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SITE_ZOOM_FILE);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(SiteZoomStore::load(&path), SiteZoomStore::default());
        std::fs::write(&path, br#"{"sites":{"a.test":9.0,"b.test":1.5}}"#).unwrap();
        let store = SiteZoomStore::load(&path);
        assert_eq!(store.get("https://a.test/"), None);
        assert_eq!(store.get("https://b.test/"), Some(1.5));
    }

    #[test]
    fn download_names_are_one_safe_component_and_never_overwrite() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\x\\a?b.txt"), "a_b.txt");
        assert_eq!(safe_file_name("..."), "download");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
        let dir = Path::new("/d");
        let taken = [PathBuf::from("/d/a.tar.gz"), PathBuf::from("/d/a (1).tar.gz")];
        assert_eq!(
            unique_download_path(dir, "a.tar.gz", |path| taken.iter().any(|t| t == path)),
            PathBuf::from("/d/a (2).tar.gz")
        );
        assert_eq!(unique_download_path(dir, "new.bin", |_| false), PathBuf::from("/d/new.bin"));
        assert_eq!(
            unique_download_path(dir, "README", |path| path == Path::new("/d/README")),
            PathBuf::from("/d/README (1)")
        );
    }

    #[test]
    fn accept_lists_become_extension_filters() {
        let accept = |items: &[&str]| accept_extensions(&items.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(accept(&[".PNG", "image/jpeg"]), vec!["png", "jpg", "jpeg"]);
        assert_eq!(accept(&["application/pdf", ".pdf"]), vec!["pdf"]);
        assert!(accept(&["application/x-unknown"]).is_empty());
        assert!(accept(&[]).is_empty());
        assert!(accept(&[".a/b"]).is_empty());
    }

    #[test]
    fn page_notifications_follow_the_site_decision() {
        let show = parse_page_notification(r#"{"op":"show","title":"New mail","body":"From\nBob"}"#).unwrap();
        let request = parse_page_notification(r#"{"op":"request"}"#).unwrap();
        let url = "https://mail.test/inbox";
        let deny = |_: &str, _: SitePermission| SitePermissionDecision::Denied;
        let grant = |origin: &str, permission: SitePermission| {
            assert_eq!((origin, permission), ("https://mail.test", SitePermission::Notifications));
            SitePermissionDecision::Granted
        };
        let ask = |_: &str, _: SitePermission| SitePermissionDecision::Ask;

        assert_eq!(page_permission_state(url, deny), PagePermission::Denied);
        assert_eq!(page_permission_state(url, grant), PagePermission::Granted);
        assert_eq!(page_permission_state(url, ask), PagePermission::Default);
        assert_eq!(page_permission_state("data:text/html,x", grant), PagePermission::Denied);
        assert_eq!(page_notification_action(url, &request, deny), PageNotificationAction::Answer(PagePermission::Denied));
        assert_eq!(page_notification_action(url, &show, deny), PageNotificationAction::Ignore);
        assert_eq!(page_notification_action(url, &request, grant), PageNotificationAction::Answer(PagePermission::Granted));
        assert_eq!(
            page_notification_action(url, &show, grant),
            PageNotificationAction::Show { title: "New mail".into(), body: "mail.test: From Bob".into() }
        );
        assert_eq!(page_notification_action(url, &request, ask), PageNotificationAction::Ignore);
        assert_eq!(
            page_notification_action("about:blank", &request, grant),
            PageNotificationAction::Answer(PagePermission::Denied)
        );
        assert_eq!(parse_page_notification(r#"{"op":"explode"}"#), None);
        assert_eq!(parse_page_notification(&"x".repeat(MAX_PAGE_MESSAGE_BYTES + 1)), None);
    }
}
