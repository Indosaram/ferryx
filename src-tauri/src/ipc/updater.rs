use crate::ipc::error::IpcError;
use tauri::utils::{config::BundleType, platform::bundle_type};

/// Host platform of the running binary, as a plain value so the ownership decision below can be
/// unit tested for every platform from any platform.
enum UpdateHost {
    Windows,
    MacOs,
    Linux,
}

impl UpdateHost {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Pure decision behind [`updater_managed_externally`]: whether another channel owns update
/// delivery for this install.
///
/// A Linux distribution package is owned by the package manager: `tauri-plugin-updater` routes a
/// `Deb`/`Rpm` bundle to `dpkg`/`rpm`, which reject the raw AppImage Ferryx publishes, so an
/// in-app update there can never succeed. An AppImage is replaced in place, and an unpackaged
/// build reports no bundle type and keeps self-updating. Windows Store ownership is a
/// package-identity fact rather than a bundle type (`BundleType::Msi` is the self-updating MSI
/// installer), so the bundle decision never claims a Windows install.
fn managed_externally_for(host: UpdateHost, bundle: Option<BundleType>) -> bool {
    match host {
        UpdateHost::Linux => matches!(bundle, Some(BundleType::Deb | BundleType::Rpm)),
        UpdateHost::Windows | UpdateHost::MacOs => false,
    }
}

#[cfg(windows)]
fn has_package_identity() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentPackageFullName(
            packageFullNameLength: *mut u32,
            packageFullName: *mut u16,
        ) -> i32;
    }
    let mut len: u32 = 0;
    let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    rc == 122 || rc == 0
}

/// Store (MSIX) installs live under the write-protected `WindowsApps` tree where the Store
/// owns update delivery; self-updating those installs would fail or duplicate the app. The
/// bundle term is always false here and keeps the whole decision in one place.
#[cfg(windows)]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
        || has_package_identity()
        || std::env::var_os("FERRYX_STORE_BUILD").is_some()
        || std::env::current_exe()
            .map(|exe| {
                let p = exe.to_string_lossy().to_lowercase();
                p.contains(r"\windowsapps\")
                    || p.contains("windowsapps")
                    || exe.parent().map(|d| d.join("msix.marker").exists()).unwrap_or(false)
            })
            .unwrap_or(false)
}

/// macOS self-updates from the `.app`. On Linux only the AppImage is replaced in place by the
/// in-app updater; a distribution package is updated by `dpkg`/`rpm` instead.
#[cfg(not(windows))]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
}

#[tauri::command]
pub async fn cmd_updater_managed_externally() -> Result<bool, IpcError> {
    Ok(updater_managed_externally())
}

/// Windows ships through two channels with different update ownership: MSIX installs are
/// updated by the Microsoft Store, NSIS installs by the in-app updater. macOS and the Linux
/// AppImage self-update; a Linux distribution package keeps the same `native` channel while
/// `updater_managed_externally` hides the in-app update card.
pub fn distribution_channel() -> &'static str {
    if cfg!(windows) {
        if updater_managed_externally() {
            "store"
        } else {
            "installer"
        }
    } else {
        "native"
    }
}

#[tauri::command]
pub async fn cmd_distribution_channel() -> Result<&'static str, IpcError> {
    Ok(distribution_channel())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distribution_channel_non_windows() {
        if !cfg!(windows) {
            assert_eq!(distribution_channel(), "native");
            // A test binary is never packaged as a distribution package, so it keeps the
            // in-app updater.
            assert!(!updater_managed_externally());
        }
    }

    #[test]
    fn test_managed_externally_for_linux_packages() {
        // dpkg/rpm own these installs: the plugin routes the bundle type to the package
        // manager, which rejects the raw AppImage Ferryx publishes.
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Deb)
        ));
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Rpm)
        ));
        // The AppImage is replaced in place, and an unpackaged build has no channel but itself.
        assert!(!managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::AppImage)
        ));
        assert!(!managed_externally_for(UpdateHost::Linux, None));
        // Windows Store ownership is a package-identity fact, not a bundle type; macOS
        // self-updates from the `.app`.
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Msi)
        ));
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Nsis)
        ));
        assert!(!managed_externally_for(
            UpdateHost::MacOs,
            Some(BundleType::App)
        ));
    }

    #[test]
    fn test_distribution_channel_windows() {
        if cfg!(windows) {
            std::env::set_var("FERRYX_STORE_BUILD", "1");
            assert_eq!(distribution_channel(), "store");
            assert!(updater_managed_externally());
            std::env::remove_var("FERRYX_STORE_BUILD");
        }
    }
}
