pub mod cookies;
pub mod download;
pub mod find;
pub mod guest;
pub mod manager;
pub mod model;
pub mod security;
#[cfg(test)]
pub mod tests;

pub use cookies::{cookie_from_imported, parse_cookie_file, ImportedCookie};
pub use download::download_url_to_path;
pub use find::{browser_find_script, parse_browser_find_callback, BROWSER_CLEAR_FIND_SCRIPT};
pub use guest::{
    parse_browser_guest_action, BrowserGuestAction, BROWSER_DOWNLOAD_REQUESTED_EVENT,
    BROWSER_GUEST_BRIDGE_SCRIPT, BROWSER_OPEN_REQUESTED_EVENT, BROWSER_SHORTCUT_REQUESTED_EVENT,
};
pub use manager::BrowserManager;
pub use model::*;
pub use security::{default_desktop_user_agent, validate_url, BrowserError};
