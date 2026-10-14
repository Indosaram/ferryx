//! No isolated JS world on this target: the section 3.5 "Unavailable" rules apply.

use super::{BridgeInstallCtx, TrustedBridgeKind};
use crate::browser::security::BrowserError;

pub(crate) fn install<R: tauri::Runtime>(
    _webview: &tauri::Webview<R>,
    _ctx: &BridgeInstallCtx,
) -> TrustedBridgeKind {
    TrustedBridgeKind::Unavailable
}

pub(crate) async fn eval_in_world<R: tauri::Runtime>(
    _webview: tauri::Webview<R>,
    _script: String,
) -> Result<String, BrowserError> {
    Err(BrowserError::PlatformUnsupported(
        "no isolated JS world is available for this browser tab".into(),
    ))
}
