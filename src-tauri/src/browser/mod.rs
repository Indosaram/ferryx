pub mod manager;
pub mod model;
pub mod security;
#[cfg(test)]
pub mod tests;

pub use manager::BrowserManager;
pub use model::*;
pub use security::{validate_url, BrowserError};
