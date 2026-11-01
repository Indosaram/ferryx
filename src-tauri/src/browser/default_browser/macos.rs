//! macOS: Launch Services. `LSSetDefaultHandlerForURLScheme` makes macOS show its own
//! "use Ferryx as your default browser?" confirmation; if Launch Services refuses (an
//! unbundled dev build, a policy), the Desktop & Dock settings pane is opened instead.

use objc2_core_foundation::{CFRetained, CFString};

use super::{DefaultBrowserStatus, MakeDefaultOutcome};

/// Desktop & Dock holds "Default web browser" on macOS 13 and later.
const DEFAULT_BROWSER_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Desktop-Settings.extension";
const FALLBACK_BUNDLE_ID: &str = "com.ferryx.app";

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSCopyDefaultHandlerForURLScheme(scheme: &CFString) -> Option<std::ptr::NonNull<CFString>>;
    fn LSSetDefaultHandlerForURLScheme(scheme: &CFString, handler: &CFString) -> i32;
}

fn bundle_id() -> String {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|id| id.to_string())
        .unwrap_or_else(|| FALLBACK_BUNDLE_ID.to_string())
}

fn handler(scheme: &str) -> Option<String> {
    let scheme = CFString::from_str(scheme);
    // SAFETY: the function follows the Copy rule, so the returned string is +1 and owned here.
    let raw = unsafe { LSCopyDefaultHandlerForURLScheme(&scheme) }?;
    // SAFETY: `raw` is a non-null, retained CFString that this call takes ownership of.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(raw) };
    Some(owned.to_string())
}

pub fn status() -> DefaultBrowserStatus {
    super::macos_status_from_handlers(
        &bundle_id(),
        handler("http").as_deref(),
        handler("https").as_deref(),
    )
}

pub fn make_default() -> Result<MakeDefaultOutcome, String> {
    let bundle = CFString::from_str(&bundle_id());
    let refused = ["http", "https"].into_iter().any(|scheme| {
        let scheme = CFString::from_str(scheme);
        // SAFETY: both arguments are valid CFStrings that outlive the call.
        unsafe { LSSetDefaultHandlerForURLScheme(&scheme, &bundle) != 0 }
    });
    if !refused {
        return Ok(MakeDefaultOutcome::Requested);
    }
    crate::ipc::browser::open_system_target(std::ffi::OsStr::new(DEFAULT_BROWSER_SETTINGS_URL))
        .map_err(|error| error.message)?;
    Ok(MakeDefaultOutcome::OpenedSystemSettings)
}
