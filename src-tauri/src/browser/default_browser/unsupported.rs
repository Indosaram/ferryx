//! Platforms without a default-browser registration.

use super::{DefaultBrowserStatus, MakeDefaultOutcome};

pub fn status() -> DefaultBrowserStatus {
    DefaultBrowserStatus::Unsupported
}

pub fn make_default() -> Result<MakeDefaultOutcome, String> {
    Err("setting the default browser is not supported on this platform".into())
}
