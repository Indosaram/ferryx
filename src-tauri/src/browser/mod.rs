pub mod cookies;
pub mod download;
pub mod find;
pub mod guest;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod manager;
pub mod model;
pub mod remote_bridge_protocol;
pub mod remote_driver;
pub mod remote_input;
pub mod remote_service;
#[cfg(test)]
mod remote_service_tests;
pub mod screenshot;
pub mod security;
pub mod snapshot_source;
#[cfg(test)]
mod snapshot_source_tests;
#[cfg(test)]
pub mod tests;

pub use cookies::{cookie_from_imported, parse_cookie_file, ImportedCookie};
pub use download::download_url_to_path;
pub use find::{browser_find_script, parse_browser_find_callback, BROWSER_CLEAR_FIND_SCRIPT};
pub use guest::{
    browser_guest_bridge_script, parse_browser_guest_action, BrowserGuestAction,
    BROWSER_DOWNLOAD_REQUESTED_EVENT, BROWSER_OPEN_REQUESTED_EVENT,
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
