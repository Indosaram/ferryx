pub mod adoption;
pub mod agent;
#[cfg(feature = "desktop")]
pub mod auth_session;
pub mod bridge;
#[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub mod cef_engine;
pub mod chromium_cookie_source;
pub mod chromium_cookies;
pub mod cookies;
#[cfg(feature = "desktop")]
pub mod default_browser;
pub mod download;
pub mod engine_install;
pub mod extensions;
pub mod find;
pub mod guest;
pub mod hard_reload;
pub mod library;
#[cfg(all(feature = "desktop", target_os = "linux"))]
pub mod linux;
pub mod manager;
pub mod model;
pub mod picker;
pub mod popup;
pub mod remote_bridge_protocol;
pub mod remote_driver;
pub mod remote_input;
pub mod remote_service;
#[cfg(test)]
mod remote_service_tests;
#[cfg(feature = "desktop")]
pub mod screenshot;
pub mod security;
pub mod site_permissions;
pub mod sign_in_handoff;
pub mod snapshot_source;
#[cfg(test)]
mod snapshot_source_tests;
pub mod state_cookies;
// Downloads, print, find, zoom, file chooser and page notifications (both engines).
pub mod system;
#[cfg(test)]
pub mod tests;
pub mod vault;

pub use cookies::{cookie_from_imported, parse_cookie_file, ImportedCookie};
pub use bridge::{TrustedAction, TrustedBridgeKind};
pub use download::download_url_to_path;
pub use find::{browser_find_script, parse_browser_find_callback, BROWSER_CLEAR_FIND_SCRIPT};
pub use guest::{
    browser_guest_bridge_script, parse_browser_guest_action, BrowserGuestAction,
    BROWSER_DOWNLOAD_REQUESTED_EVENT, BROWSER_ELEMENT_PICKED_EVENT,
    BROWSER_LINK_CLICKED_EVENT, BROWSER_OPEN_REQUESTED_EVENT,
    BROWSER_SHORTCUT_REQUESTED_EVENT,
};
pub use manager::BrowserManager;
pub use model::*;
pub use remote_bridge_protocol::*;
pub use remote_driver::*;
pub use remote_input::*;
pub use remote_service::*;
pub use security::{default_desktop_user_agent, validate_url, BrowserError};
pub use snapshot_source::*;
pub use state_cookies::PersistedCookie;

pub mod import;
