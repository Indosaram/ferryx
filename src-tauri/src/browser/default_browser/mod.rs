//! Ferryx as the system default web browser: registration per OS, the status shown in
//! Settings > Browser, and the http(s) URLs the OS hands to the app.
//!
//! The pure parts (URL classification, the registry values, the `.desktop` entry and the
//! commands each OS runs) live here and are unit tested on every host. The OS calls live in
//! one module per platform; every platform has a working fallback that opens the system's
//! default-apps settings instead of failing silently.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(target_os = "macos")]
use self::macos as platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use self::unsupported as platform;
#[cfg(target_os = "windows")]
use self::windows as platform;

#[cfg(test)]
mod tests;

/// The app event that tells the UI inbound URLs are queued: it drains them with
/// `cmd_default_browser_take_inbound_urls` and opens each in a new browser tab. The payload is
/// empty; draining (rather than carrying the URL) also covers URLs that arrive before the UI
/// listens, such as the one a cold launch was started with.
pub const INBOUND_URL_EVENT: &str = "ferryx:inbound-url";

/// Name Ferryx registers under (Windows StartMenuInternet key, Linux desktop file id).
pub const CLIENT_NAME: &str = "Ferryx";
pub const LINUX_DESKTOP_FILE: &str = "ferryx-browser.desktop";

static INBOUND_URLS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Queues a URL the OS asked Ferryx to open until the UI drains it.
pub fn queue_inbound_url(url: String) {
    INBOUND_URLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(url);
}

/// Takes every queued inbound URL, oldest first.
pub fn take_inbound_urls() -> Vec<String> {
    std::mem::take(
        &mut *INBOUND_URLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// What Settings > Browser shows in the "Default browser" row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DefaultBrowserStatus {
    /// Ferryx handles both http and https.
    Default,
    /// Another app (named when the OS says which) handles http or https.
    #[serde(rename_all = "camelCase")]
    NotDefault { current: Option<String> },
    /// This platform or package cannot register a browser.
    Unsupported,
}

/// What "Make Ferryx the default browser" did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum MakeDefaultOutcome {
    /// The OS accepted the change (macOS may still show its own confirmation prompt).
    Requested,
    /// The OS needs the user to pick Ferryx; its default-apps settings were opened.
    OpenedSystemSettings,
}

/// The http(s) URL among process arguments, if the OS launched Ferryx to open one.
///
/// Only a single `http://` or `https://` argument counts; flags and anything else are left
/// to the normal command-line handling. The URL must parse and carry a host.
pub fn inbound_url_from_args<T: AsRef<str>>(args: &[T]) -> Option<String> {
    let mut found = None;
    for arg in args.iter().skip(1) {
        let arg = arg.as_ref();
        if arg.starts_with("-psn_") {
            continue;
        }
        let url = classify_inbound_url(arg)?;
        if found.replace(url).is_some() {
            return None;
        }
    }
    found
}

/// Accepts a URL the OS asked Ferryx to open: http or https with a host. Returns it normalized.
pub fn classify_inbound_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some_and(|host| !host.is_empty()) => {
            Some(parsed.to_string())
        }
        _ => None,
    }
}

/// One `HKCU` value Windows needs to list Ferryx as a browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValue {
    /// Path under `HKEY_CURRENT_USER`.
    pub key: String,
    /// Value name; `None` is the key's default value.
    pub name: Option<String>,
    pub data: String,
}

/// The per-user registration Windows' Default Apps page reads: a ProgId for http(s), a
/// `StartMenuInternet` client with `Capabilities/URLAssociations`, and `RegisteredApplications`.
pub fn windows_registry_values(exe_path: &str) -> Vec<RegistryValue> {
    let prog_id = "FerryxURL";
    let client = format!(r"Software\Clients\StartMenuInternet\{CLIENT_NAME}");
    let caps = format!(r"{client}\Capabilities");
    let open_command = format!("\"{exe_path}\" \"%1\"");
    let value = |key: &str, name: Option<&str>, data: &str| RegistryValue {
        key: key.to_string(),
        name: name.map(str::to_string),
        data: data.to_string(),
    };
    let prog_key = format!(r"Software\Classes\{prog_id}");
    vec![
        value(&prog_key, None, "Ferryx URL"),
        value(&prog_key, Some("URL Protocol"), ""),
        value(
            &format!(r"{prog_key}\DefaultIcon"),
            None,
            &format!("\"{exe_path}\",0"),
        ),
        value(
            &format!(r"{prog_key}\shell\open\command"),
            None,
            &open_command,
        ),
        value(&client, None, CLIENT_NAME),
        value(
            &format!(r"{client}\DefaultIcon"),
            None,
            &format!("\"{exe_path}\",0"),
        ),
        value(
            &format!(r"{client}\shell\open\command"),
            None,
            &format!("\"{exe_path}\""),
        ),
        value(&caps, Some("ApplicationName"), CLIENT_NAME),
        value(
            &caps,
            Some("ApplicationDescription"),
            "Ferryx workspace browser",
        ),
        value(&format!(r"{caps}\URLAssociations"), Some("http"), prog_id),
        value(&format!(r"{caps}\URLAssociations"), Some("https"), prog_id),
        value(
            &format!(r"{caps}\StartMenu"),
            Some("StartMenuInternet"),
            CLIENT_NAME,
        ),
        value(r"Software\RegisteredApplications", Some(CLIENT_NAME), &caps),
    ]
}

/// The Windows page where the user confirms a default browser (Windows does not let apps set it).
pub const WINDOWS_DEFAULT_APPS_URI: &str = "ms-settings:defaultapps?registeredAppUser=Ferryx";

/// The `.desktop` entry that lets Linux desktops pick Ferryx for http(s) links.
pub fn linux_desktop_entry(exe_path: &str) -> String {
    let exec = exe_path.replace('\\', "\\\\").replace(' ', "\\s");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Ferryx\n\
         GenericName=Web Browser\n\
         Comment=Open web links in Ferryx\n\
         Exec={exec} %u\n\
         Icon=ferryx\n\
         Terminal=false\n\
         NoDisplay=true\n\
         Categories=Network;WebBrowser;\n\
         MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;\n"
    )
}

/// Commands Linux runs after writing the desktop file, in order. The first is the one
/// `xdg-settings` users expect; the `xdg-mime` pair covers desktops without `xdg-settings`.
pub fn linux_registration_commands() -> Vec<(&'static str, Vec<String>)> {
    vec![
        (
            "xdg-settings",
            vec![
                "set".into(),
                "default-web-browser".into(),
                LINUX_DESKTOP_FILE.into(),
            ],
        ),
        (
            "xdg-mime",
            vec![
                "default".into(),
                LINUX_DESKTOP_FILE.into(),
                "x-scheme-handler/http".into(),
            ],
        ),
        (
            "xdg-mime",
            vec![
                "default".into(),
                LINUX_DESKTOP_FILE.into(),
                "x-scheme-handler/https".into(),
            ],
        ),
    ]
}

/// Reads `xdg-settings get default-web-browser` output into a status.
pub fn linux_status_from_xdg(output: &str) -> DefaultBrowserStatus {
    let current = output.trim();
    if current == LINUX_DESKTOP_FILE {
        DefaultBrowserStatus::Default
    } else {
        DefaultBrowserStatus::NotDefault {
            current: browser_display_name(current).map(str::to_string),
        }
    }
}

/// Handler ids matched whole (after lowercasing and dropping a `.desktop` suffix).
const EXACT_BROWSER_IDS: &[(&str, &str)] = &[
    ("ie.http", "Internet Explorer"),
    ("ie.https", "Internet Explorer"),
    // The pre-Chromium Edge package ProgId.
    ("appxq0fevzme2pys62n3e0fbqa7peapykr8v", "Microsoft Edge"),
    ("zen", "Zen"),
    ("archtml", "Arc"),
    ("epiphany", "GNOME Web"),
    ("org.gnome.epiphany", "GNOME Web"),
];

/// Handler id prefixes, most specific first: Windows ProgIds (`MSEdgeHTM`, `ChromeHTML`,
/// `FirefoxURL-<hash>`), macOS bundle ids and Linux desktop file ids.
const BROWSER_ID_PREFIXES: &[(&str, &str)] = &[
    ("com.apple.safaritechnologypreview", "Safari Technology Preview"),
    ("com.apple.safari", "Safari"),
    ("com.google.chrome", "Google Chrome"),
    ("google-chrome", "Google Chrome"),
    ("chromehtml", "Google Chrome"),
    ("chromebhtml", "Google Chrome"),
    ("chromesshtm", "Google Chrome"),
    ("chromebetahtml", "Google Chrome"),
    ("chromedevhtml", "Google Chrome"),
    ("org.chromium.chromium", "Chromium"),
    ("chromium", "Chromium"),
    ("com.microsoft.edge", "Microsoft Edge"),
    ("microsoft-edge", "Microsoft Edge"),
    ("msedge", "Microsoft Edge"),
    ("org.mozilla.firefox", "Firefox"),
    ("firefox", "Firefox"),
    ("io.gitlab.librewolf", "LibreWolf"),
    ("librewolf", "LibreWolf"),
    ("com.brave.browser", "Brave"),
    ("brave", "Brave"),
    ("company.thebrowser.browser", "Arc"),
    ("com.operasoftware.operagx", "Opera GX"),
    ("opera gx", "Opera GX"),
    ("com.operasoftware.opera", "Opera"),
    ("opera", "Opera"),
    ("com.vivaldi.vivaldi", "Vivaldi"),
    ("vivaldi", "Vivaldi"),
    ("app.zen-browser.zen", "Zen"),
    ("app.zen_browser.zen", "Zen"),
    ("com.kagi.kagimacos", "Orion"),
    ("org.torproject.torbrowser", "Tor Browser"),
    ("org.kde.falkon", "Falkon"),
    ("falkon", "Falkon"),
];

/// The name to show for the app that handles http(s) links, from the OS handler id: a Windows
/// ProgId, a macOS bundle id or a Linux desktop file id. `None` for ids it does not know, so the
/// UI never shows a raw id.
pub fn browser_display_name(handler: &str) -> Option<&'static str> {
    let lowered = handler.trim().to_ascii_lowercase();
    let id = lowered.strip_suffix(".desktop").unwrap_or(&lowered);
    if id.is_empty() {
        return None;
    }
    EXACT_BROWSER_IDS
        .iter()
        .find(|(known, _)| *known == id)
        .or_else(|| BROWSER_ID_PREFIXES.iter().find(|(prefix, _)| id.starts_with(prefix)))
        .map(|(_, name)| *name)
}

/// Whether the bundle ids handling http and https are both Ferryx's (case-insensitive, as
/// Launch Services compares them).
pub fn macos_status_from_handlers(
    own_bundle_id: &str,
    http: Option<&str>,
    https: Option<&str>,
) -> DefaultBrowserStatus {
    let is_own =
        |handler: Option<&str>| handler.is_some_and(|id| id.eq_ignore_ascii_case(own_bundle_id));
    if is_own(http) && is_own(https) {
        DefaultBrowserStatus::Default
    } else {
        let other = [http, https]
            .into_iter()
            .flatten()
            .find(|id| !id.eq_ignore_ascii_case(own_bundle_id));
        DefaultBrowserStatus::NotDefault {
            current: other.and_then(browser_display_name).map(str::to_string),
        }
    }
}

/// Whether the per-user `UrlAssociations\{scheme}\UserChoice` ProgIds both point at Ferryx.
pub fn windows_status_from_prog_ids(
    http: Option<&str>,
    https: Option<&str>,
) -> DefaultBrowserStatus {
    let is_own = |id: Option<&str>| id.is_some_and(|id| id.eq_ignore_ascii_case("FerryxURL"));
    if is_own(http) && is_own(https) {
        DefaultBrowserStatus::Default
    } else {
        DefaultBrowserStatus::NotDefault {
            current: [http, https]
                .into_iter()
                .flatten()
                .find(|id| !is_own(Some(*id)))
                .and_then(browser_display_name)
                .map(str::to_string),
        }
    }
}

/// Current default-browser status. Blocking: reads the registry, Launch Services or runs
/// `xdg-settings`; call through `crate::ipc::run_blocking`.
pub fn status() -> DefaultBrowserStatus {
    platform::status()
}

/// Registers Ferryx and asks the OS to make it the default browser. Blocking.
pub fn make_default() -> Result<MakeDefaultOutcome, String> {
    platform::make_default()
}

/// `reg.exe add` arguments that write one [`RegistryValue`] under `HKCU`.
pub fn windows_reg_add_args(value: &RegistryValue) -> Vec<String> {
    let mut args = vec!["add".to_string(), format!(r"HKCU\{}", value.key)];
    match &value.name {
        Some(name) => {
            args.push("/v".into());
            args.push(name.clone());
        }
        None => args.push("/ve".into()),
    }
    args.extend([
        "/t".into(),
        "REG_SZ".into(),
        "/d".into(),
        value.data.clone(),
        "/f".into(),
    ]);
    args
}

/// The `HKCU` key that holds the user's chosen ProgId for `scheme`.
pub fn windows_user_choice_key(scheme: &str) -> String {
    format!(
        r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice"
    )
}

/// The `ProgId` value from `reg.exe query <key> /v ProgId` output.
pub fn windows_prog_id_from_reg_query(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next()? == "ProgId" && parts.next()? == "REG_SZ")
            .then(|| parts.collect::<Vec<_>>().join(" "))
            .filter(|id| !id.is_empty())
    })
}
