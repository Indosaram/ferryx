// Copyright 2026 The Maho Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Windows cookie decryption using DPAPI and Chrome's COM elevator service.

#![cfg(any(windows, test))]

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
#[cfg(windows)]
use serde_json::Value;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use windows::core::{IUnknown, Interface, GUID};
#[cfg(windows)]
use windows::core::BSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{LocalFree, SysAllocStringByteLen, SysStringByteLen, HLOCAL};
#[cfg(windows)]
use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
#[cfg(windows)]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
    EOAC_DYNAMIC_CLOAKING, RPC_C_AUTHN_LEVEL_PKT_PRIVACY, RPC_C_IMP_LEVEL_IMPERSONATE,
};
#[cfg(windows)]
use zeroize::Zeroize;
use zeroize::Zeroizing;

/// Why the app-bound (`v20`) key in `Local State` could not be released.
///
/// The browser's elevation service releases an app-bound key only to its own signed image, so
/// any other caller gets `kValidationDidNotPass` (`0x8004A007`) back. That is a refusal, not a
/// missing key: the `v10` DPAPI key cannot open a `v20` row, so falling back to it only turned
/// every `v20` cookie into a generic decryption failure. The refusal is therefore typed and
/// carried to the importer, which reports the `v20` rows as seen and not applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppBoundKeyError {
    #[error("the browser's elevation service refused the app-bound key (HRESULT 0x{hresult:08X})")]
    Refused { hresult: u32 },
    /// The elevator could not be reached at all: no COM config for this browser, the service
    /// is not installed, or the interface is not proxied.
    #[error("{0}")]
    Unavailable(String),
    /// The key material in `Local State` is not an app-bound key this code can present.
    #[error("{0}")]
    Malformed(String),
}

impl AppBoundKeyError {
    /// Stable machine-readable code, surfaced as `details.reason` of the cookie import error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused { .. } => "app-bound-key-refused",
            Self::Unavailable(_) | Self::Malformed(_) => "app-bound-key-unavailable",
        }
    }

    /// The measured COM status of a refusal, when the service answered.
    pub fn hresult(&self) -> Option<u32> {
        match self {
            Self::Refused { hresult } => Some(*hresult),
            Self::Unavailable(_) | Self::Malformed(_) => None,
        }
    }
}

/// Decryptor for Windows Chrome / Edge / Brave cookies.
pub struct WindowsAppBoundDecryptor {
    aes_key: Zeroizing<Vec<u8>>,
}

impl WindowsAppBoundDecryptor {
    /// Attempts to decrypt the AES key from the browser's Local State.
    ///
    /// Tries the IElevator2 (Chrome 144+ v2) COM interface first, falling back
    /// to IElevator (v1) if available. When `Local State` holds an app-bound key, a refusal is
    /// returned as [`AppBoundKeyError::Refused`] and NOT degraded into the `v10` DPAPI key: the
    /// `v10` key cannot decrypt a `v20` row, so the fallback hid the reason. Only a profile
    /// without an app-bound key (pre-Chrome-117) uses the DPAPI key alone.
    #[cfg(windows)]
    pub fn for_browser(
        browser_name: &str,
        local_state_path: &Path,
    ) -> Result<Self, AppBoundKeyError> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

        let content = std::fs::read_to_string(local_state_path)
            .map_err(|e| AppBoundKeyError::Unavailable(format!("reading Local State: {e}")))?;
        let json: Value = serde_json::from_str(&content)
            .map_err(|e| AppBoundKeyError::Unavailable(format!("parsing Local State: {e}")))?;

        // 1. The app_bound_encrypted_key (v20 Chrome/Edge/Brave).
        if let Some(app_bound_b64) = json["os_crypt"]["app_bound_encrypted_key"].as_str() {
            use base64::Engine as _;
            let encrypted_key = base64::engine::general_purpose::STANDARD
                .decode(app_bound_b64)
                .map_err(|e| {
                    AppBoundKeyError::Malformed(format!(
                        "app_bound_encrypted_key is not base64: {e}"
                    ))
                })?;

            if let Some(payload) = encrypted_key.strip_prefix(b"APPB") {
                let key = decrypt_key(browser_name, payload)?;
                return Ok(Self { aes_key: key });
            }
        }

        // 2. No app-bound key in this profile: the DPAPI key is the only one (v10 / pre-Chrome-117).
        Self::from_dpapi_json(&json).ok_or_else(|| {
            AppBoundKeyError::Unavailable(format!(
                "Local State of {browser_name} holds no decryption key"
            ))
        })
    }

    /// The `v10` key only: `os_crypt.encrypted_key`, protected by the user's DPAPI. Chrome
    /// still encrypts `Login Data` passwords with it after cookies moved to the app-bound key.
    #[cfg(windows)]
    pub fn for_v10(local_state_path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(local_state_path).map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        Self::from_dpapi_json(&json).ok_or_else(|| "Local State has no DPAPI key".to_string())
    }

    #[cfg(windows)]
    fn from_dpapi_json(json: &Value) -> Option<Self> {
        if let Some(encrypted_key_b64) = json["os_crypt"]["encrypted_key"].as_str() {
            use base64::Engine as _;
            let encrypted_key = base64::engine::general_purpose::STANDARD
                .decode(encrypted_key_b64)
                .ok()?;

            if encrypted_key.starts_with(b"DPAPI") {
                let payload = &encrypted_key[5..];
                let input_blob = CRYPT_INTEGER_BLOB {
                    cbData: payload.len() as u32,
                    pbData: payload.as_ptr() as *mut u8,
                };
                let mut output_blob = CRYPT_INTEGER_BLOB {
                    cbData: 0,
                    pbData: ptr::null_mut(),
                };

                let ok = unsafe {
                    CryptUnprotectData(&input_blob, None, None, None, None, 0, &mut output_blob)
                };

                if ok.is_ok() {
                    let key = unsafe {
                        let slice = std::slice::from_raw_parts_mut(
                            output_blob.pbData,
                            output_blob.cbData as usize,
                        );
                        let key = Zeroizing::new(slice.to_vec());
                        // The DPAPI buffer holds the key too; clear it before handing it back.
                        slice.zeroize();
                        let _ = LocalFree(Some(HLOCAL(output_blob.pbData as *mut std::ffi::c_void)));
                        key
                    };
                    return Some(Self { aes_key: key });
                }
            }
        }
        None
    }

    /// Decrypts a cookie value using AES-256-GCM.
    pub fn decrypt_cookie(&self, encrypted_value: &[u8]) -> Result<String, String> {
        let decrypted = self.decrypt_bytes(encrypted_value)?;
        std::str::from_utf8(&decrypted)
            .map(str::to_owned)
            .map_err(|e| format!("Invalid UTF-8 in decrypted cookie: {}", e))
    }

    /// Like [`Self::decrypt_cookie`], for cookie databases of version 24 and later, whose
    /// plaintext starts with SHA-256 of the cookie's host: the binding is checked and removed.
    pub fn decrypt_bound_cookie(
        &self,
        encrypted_value: &[u8],
        database_version: i64,
        host: &str,
    ) -> Result<String, String> {
        let decrypted = self.decrypt_bytes(encrypted_value)?;
        let value = if database_version >= 24 {
            use sha2::{Digest, Sha256};
            let expected = Sha256::digest(host.as_bytes());
            if decrypted.get(..32) != Some(expected.as_slice()) {
                return Err("cookie host binding mismatch".into());
            }
            &decrypted[32..]
        } else {
            &decrypted[..]
        };
        std::str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|e| format!("Invalid UTF-8 in decrypted cookie: {}", e))
    }

    fn decrypt_bytes(&self, encrypted_value: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        // Windows cookie format for GCM:
        // ciphertext starts with 3-byte prefix (e.g. "v10" or "v20"), which is stripped.
        // The remaining payload is: 12-byte IV + ciphertext + 16-byte authentication tag.
        if encrypted_value.len() < 12 + 16 {
            return Err("Encrypted cookie payload too short".into());
        }

        let iv = &encrypted_value[..12];
        let payload = &encrypted_value[12..];

        let cipher = Aes256Gcm::new_from_slice(&self.aes_key)
            .map_err(|_| "Invalid AES-256 key length".to_string())?;
        let nonce = Nonce::from_slice(iv);

        cipher
            .decrypt(nonce, payload)
            .map(Zeroizing::new)
            .map_err(|e| format!("AES-GCM decryption failed: {}", e))
    }
}

// Browser config containing CLSID, the IElevator IIDs to query (in order) and the vtable slot
// of DecryptData. The elevation service only registers proxies for its brand-specific IIDs:
// on Chrome 156 (maho-win) the generic IElevator/IElevator2 IIDs answer E_NOINTERFACE through
// COM, while IElevator2Chrome reaches DecryptData.
#[cfg(any(windows, test))]
struct BrowserComConfig {
    clsid: &'static str,
    iids: &'static [&'static str],
    vtable_slot: usize,
}

#[cfg(any(windows, test))]
fn get_browser_config(browser_name: &str) -> Option<BrowserComConfig> {
    match browser_name {
        "Chrome" | "google-chrome" => Some(BrowserComConfig {
            clsid: "708860E0-F641-4611-8895-7D867DD3675B",
            iids: &[
                // IElevator2Chrome, IElevatorChrome, then the generic IElevator2 and IElevator.
                "1BF5208B-295F-4992-B5F4-3A9BB6494838",
                "463ABECF-410D-407F-8AF5-0DF35A005CC8",
                "8F7B6792-784D-4047-845D-1782EFBEF205",
                "A949CB4E-C4F9-44C4-B213-6BF8AA9AC69C",
                "A99222FE-D225-45C6-896B-D9A3B5EE5250",
            ],
            vtable_slot: 5,
        }),
        "Edge" | "microsoft-edge" => Some(BrowserComConfig {
            clsid: "1FCBE96C-1697-43AF-9140-2897C7C69767",
            iids: &[
                // IElevatorEdge, IElevator2Edge, then the generic IElevator2 and IElevator.
                "C9C2B807-7731-4F34-81B7-44FF7779522B",
                "05EB264E-4D60-4F34-ADB7-1F983E930A79",
                "8F7B6792-784D-4047-845D-1782EFBEF205",
                "A949CB4E-C4F9-44C4-B213-6BF8AA9AC69C",
                "A99222FE-D225-45C6-896B-D9A3B5EE5250",
            ],
            vtable_slot: 8,
        }),
        "Brave" | "brave-browser" => Some(BrowserComConfig {
            clsid: "576B31AF-6369-4B6B-8560-E4B203A97A8B",
            iids: &[
                // IElevator2Chromium, IElevatorChromium, then the generic IElevator2 and IElevator.
                "BB19A0E5-00C6-4966-94B2-5AFEC6FED93A",
                "B88C45B9-8825-4629-B83E-77CC67D9CEED",
                "8F7B6792-784D-4047-845D-1782EFBEF205",
                "A949CB4E-C4F9-44C4-B213-6BF8AA9AC69C",
                "A99222FE-D225-45C6-896B-D9A3B5EE5250",
            ],
            vtable_slot: 5,
        }),
        _ => None,
    }
}

/// Asks the browser's elevation service for the app-bound key.
///
/// Tests replace the COM round-trip with [`COM_DECRYPT_OVERRIDE`] so the refusal path is
/// exercised without a browser, its service, or a real profile; production always goes through
/// [`decrypt_via_com`].
#[cfg(windows)]
fn decrypt_key(
    browser_name: &str,
    encrypted_key: &[u8],
) -> Result<Zeroizing<Vec<u8>>, AppBoundKeyError> {
    #[cfg(any(test, feature = "testing"))]
    if let Some(override_fn) = *COM_DECRYPT_OVERRIDE.lock().unwrap() {
        return override_fn(browser_name, encrypted_key);
    }
    decrypt_via_com(browser_name, encrypted_key)
}

/// Test seam: the COM round-trip that releases the app-bound key. A test that installs an
/// override holds [`COM_DECRYPT_LOCK`] for its whole body, so the process-wide override cannot
/// leak into a test running beside it.
///
/// `pub` rather than `pub(crate)` because a dependent crate's tests (src-tauri) drive the same
/// refusal path; the `testing` feature is what makes this reachable outside this crate, and it is
/// only enabled by a dev-dependency. `#[doc(hidden)]` keeps it out of the public API docs.
#[doc(hidden)]
#[cfg(any(test, feature = "testing"))]
#[cfg(windows)]
pub type ComDecryptOverride =
    fn(&str, &[u8]) -> Result<Zeroizing<Vec<u8>>, AppBoundKeyError>;

#[doc(hidden)]
#[cfg(any(test, feature = "testing"))]
#[cfg(windows)]
pub static COM_DECRYPT_OVERRIDE: std::sync::Mutex<Option<ComDecryptOverride>> =
    std::sync::Mutex::new(None);

#[doc(hidden)]
#[cfg(any(test, feature = "testing"))]
#[cfg(windows)]
pub static COM_DECRYPT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
fn decrypt_via_com(
    browser_name: &str,
    encrypted_key: &[u8],
) -> Result<Zeroizing<Vec<u8>>, AppBoundKeyError> {
    let config = get_browser_config(browser_name).ok_or_else(|| {
        AppBoundKeyError::Unavailable(format!(
            "COM configuration missing for browser {}",
            browser_name
        ))
    })?;

    unsafe {
        let guid = |text: &str| {
            GUID::try_from(text)
                .map_err(|e| AppBoundKeyError::Unavailable(format!("invalid GUID {text}: {e}")))
        };
        let clsid = guid(config.clsid)?;

        let unknown: IUnknown = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)
            .map_err(|e| AppBoundKeyError::Unavailable(format!("CoCreateInstance failed: {}", e)))?;

        // Query the IElevator interfaces in order; the first one the service proxies wins.
        let mut elevator_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut hr = windows::core::HRESULT(0);
        for iid in config.iids {
            hr = unknown.query(&guid(iid)?, &mut elevator_ptr);
            if hr.is_ok() && !elevator_ptr.is_null() {
                break;
            }
        }

        if hr.is_err() || elevator_ptr.is_null() {
            return Err(AppBoundKeyError::Unavailable(format!(
                "QueryInterface for IElevator failed with HRESULT 0x{:08X}",
                hr.0
            )));
        }

        // DecryptData impersonates the caller to undo the user-level DPAPI layer; without an
        // impersonation blanket it fails with 0x8004A003 (could not decrypt with user context).
        let elevator = IUnknown::from_raw_borrowed(&elevator_ptr).expect("non-null interface");
        if let Err(error) = CoSetProxyBlanket(
            elevator,
            u32::MAX, // RPC_C_AUTHN_DEFAULT
            u32::MAX, // RPC_C_AUTHZ_DEFAULT
            None,
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_DYNAMIC_CLOAKING,
        ) {
            let _ = IUnknown::from_raw(elevator_ptr);
            return Err(AppBoundKeyError::Unavailable(format!(
                "CoSetProxyBlanket on IElevator failed: {error}"
            )));
        }

        let result = call_decrypt_data(elevator_ptr, config.vtable_slot, encrypted_key);

        // Balance the reference added by QueryInterface; `unknown` releases on drop.
        let _ = IUnknown::from_raw(elevator_ptr);

        result
    }
}

/// Calls `IElevator::DecryptData` through the vtable. Chromium's IDL
/// (`chrome/elevation_service/elevation_service_idl.idl`) declares
/// `HRESULT DecryptData([in] const BSTR ciphertext, [out] BSTR* plaintext, [out] DWORD* last_error)`;
/// both buffers are byte BSTRs.
#[cfg(windows)]
unsafe fn call_decrypt_data(
    unknown_ptr: *mut std::ffi::c_void,
    slot: usize,
    input: &[u8],
) -> Result<Zeroizing<Vec<u8>>, AppBoundKeyError> {
    let vtable_ptr = *(unknown_ptr as *mut *mut *mut std::ffi::c_void);
    let func_ptr = *vtable_ptr.add(slot);

    type DecryptDataFn = unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        ciphertext: *const u16,
        plaintext: *mut *const u16,
        last_error: *mut u32,
    ) -> i32;

    let decrypt_fn: DecryptDataFn = std::mem::transmute(func_ptr);

    let ciphertext: BSTR = SysAllocStringByteLen(Some(input));
    if ciphertext.is_empty() && !input.is_empty() {
        return Err(AppBoundKeyError::Malformed("BSTR allocation failed".into()));
    }
    let mut plaintext_raw: *const u16 = std::ptr::null();
    let mut last_error: u32 = 0;

    let hr = decrypt_fn(unknown_ptr, ciphertext.as_ptr(), &mut plaintext_raw, &mut last_error);
    // Owns (and frees on drop) whatever the service returned, even on failure.
    let plaintext = BSTR::from_raw(plaintext_raw);

    if hr < 0 {
        // The service answered and refused the key: 0x8004A007 (kValidationDidNotPass) for any
        // caller that is not the browser's own image.
        tracing::warn!(
            hresult = format!("0x{:08X}", hr),
            last_error,
            "elevation service refused the app-bound key"
        );
        return Err(AppBoundKeyError::Refused {
            hresult: hr as u32,
        });
    }

    if plaintext_raw.is_null() {
        return Err(AppBoundKeyError::Malformed(
            "COM DecryptData returned null or empty output".into(),
        ));
    }
    let size = SysStringByteLen(&plaintext) as usize;
    if size == 0 {
        return Err(AppBoundKeyError::Malformed(
            "COM DecryptData returned null or empty output".into(),
        ));
    }

    let key = Zeroizing::new(std::slice::from_raw_parts(plaintext_raw as *const u8, size).to_vec());
    // The BSTR frees on drop without clearing; wipe the key out of it first.
    std::slice::from_raw_parts_mut(plaintext_raw as *mut u8, size).zeroize();
    Ok(key)
}

#[cfg(test)]
mod review_tests {
    use super::*;

    /// Every browser asks for its brand-specific IElevator interface before the generic ones,
    /// and each IID parses as a GUID.
    #[test]
    fn brand_elevator_iids_are_queried_first() {
        let first = |name| get_browser_config(name).unwrap().iids[0];
        assert_eq!(first("Chrome"), "1BF5208B-295F-4992-B5F4-3A9BB6494838");
        assert_eq!(first("Edge"), "C9C2B807-7731-4F34-81B7-44FF7779522B");
        assert_eq!(first("Brave"), "BB19A0E5-00C6-4966-94B2-5AFEC6FED93A");
        let slot = |name| get_browser_config(name).unwrap().vtable_slot;
        assert_eq!(slot("Chrome"), 5);
        assert_eq!(slot("Edge"), 8);
        assert_eq!(slot("Brave"), 5);
        for name in ["Chrome", "Edge", "Brave"] {
            let config = get_browser_config(name).unwrap();
            assert!(windows_core_guid_ok(config.clsid));
            assert!(config.iids.iter().all(|iid| windows_core_guid_ok(iid)));
        }
    }

    fn windows_core_guid_ok(text: &str) -> bool {
        let parts: Vec<&str> = text.split('-').collect();
        parts.iter().map(|p| p.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
            && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
    }

    #[test]
    fn review_invalid_windows_key_returns_error_without_panicking() {
        for len in [0, 16, 31, 33] {
            let decryptor = WindowsAppBoundDecryptor { aes_key: Zeroizing::new(vec![0; len]) };
            assert!(decryptor.decrypt_cookie(&[0; 28]).is_err());
        }
    }

    #[test]
    fn bound_cookie_strips_the_host_hash_and_rejects_another_host() {
        use sha2::{Digest, Sha256};
        let key = [7u8; 32];
        let iv = [3u8; 12];
        let mut plain = Sha256::digest(b".example.com").to_vec();
        plain.extend_from_slice(b"session-value");
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut blob = iv.to_vec();
        blob.extend(cipher.encrypt(Nonce::from_slice(&iv), plain.as_slice()).unwrap());
        let decryptor = WindowsAppBoundDecryptor { aes_key: Zeroizing::new(key.to_vec()) };

        assert_eq!(
            decryptor.decrypt_bound_cookie(&blob, 24, ".example.com").unwrap(),
            "session-value"
        );
        assert!(decryptor.decrypt_bound_cookie(&blob, 24, ".other.com").is_err());
    }

    /// The refusal keeps the measured HRESULT and reports the stable code the cookie importer
    /// hands to the shell, while an unreachable service does not claim to be a refusal.
    #[test]
    fn a_refusal_is_typed_with_its_hresult_and_never_collapses_into_a_missing_key() {
        let refused = AppBoundKeyError::Refused { hresult: 0x8004_A007 };
        assert_eq!(refused.code(), "app-bound-key-refused");
        assert_eq!(refused.hresult(), Some(0x8004_A007));
        assert!(
            refused.to_string().contains("0x8004A007"),
            "the reason must carry the measured status: {refused}"
        );

        let unreachable = AppBoundKeyError::Unavailable("CoCreateInstance failed".into());
        assert_eq!(unreachable.code(), "app-bound-key-unavailable");
        assert_eq!(unreachable.hresult(), None);

        let malformed = AppBoundKeyError::Malformed("not base64".into());
        assert_eq!(malformed.code(), "app-bound-key-unavailable");
    }

    #[cfg(windows)]
    mod windows_key_selection {
        use super::*;
        use base64::Engine as _;
        use std::io::Write;
        use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

        /// A `Local State` holding `os_crypt.encrypted_key` (the `v10` DPAPI key) and, when
        /// `app_bound` is given, `os_crypt.app_bound_encrypted_key` (the `APPB`-prefixed key).
        fn local_state(app_bound: Option<Vec<u8>>, dpapi_key: Option<&[u8]>) -> (tempfile::TempDir, std::path::PathBuf) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("Local State");
            let mut os_crypt = serde_json::Map::new();
            if let Some(key) = dpapi_key {
                let mut wrapped = b"DPAPI".to_vec();
                wrapped.extend_from_slice(&dpapi_protect(key));
                os_crypt.insert(
                    "encrypted_key".into(),
                    Value::String(base64::engine::general_purpose::STANDARD.encode(wrapped)),
                );
            }
            if let Some(app_bound) = app_bound {
                let mut wrapped = b"APPB".to_vec();
                wrapped.extend_from_slice(&app_bound);
                os_crypt.insert(
                    "app_bound_encrypted_key".into(),
                    Value::String(base64::engine::general_purpose::STANDARD.encode(wrapped)),
                );
            }
            let json = serde_json::json!({ "os_crypt": Value::Object(os_crypt) });
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(json.to_string().as_bytes()).unwrap();
            (dir, path)
        }

        /// DPAPI-protects `plain` in the current user context, as Chrome writes its `v10` key.
        fn dpapi_protect(plain: &[u8]) -> Vec<u8> {
            let input = CRYPT_INTEGER_BLOB {
                cbData: plain.len() as u32,
                pbData: plain.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
            unsafe {
                CryptProtectData(&input, None, None, None, None, 0, &mut out).unwrap();
                let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
                let _ = LocalFree(Some(HLOCAL(out.pbData as *mut std::ffi::c_void)));
                bytes
            }
        }

        /// A `v10` AES-256-GCM cookie blob for `value` on `host` under `key`, host-bound the way
        /// Chromium writes it for database version 24.
        fn v10_cookie(key: &[u8], host: &str, value: &str) -> Vec<u8> {
            use sha2::{Digest, Sha256};
            let iv = [9u8; 12];
            let mut plain = Sha256::digest(host.as_bytes()).to_vec();
            plain.extend_from_slice(value.as_bytes());
            let cipher = Aes256Gcm::new_from_slice(key).unwrap();
            let mut blob = b"v10".to_vec();
            blob.extend_from_slice(&iv);
            blob.extend(cipher.encrypt(Nonce::from_slice(&iv), plain.as_slice()).unwrap());
            blob
        }

        /// Characterization (passes before and after the fix): a profile whose `Local State` has
        /// only the DPAPI key still yields a working decryptor, so `v10` cookies keep importing.
        #[test]
        fn v10_dpapi_key_alone_still_decrypts_a_v10_cookie() {
            let key = [0x5Au8; 32];
            let (_dir, path) = local_state(None, Some(&key));
            let decryptor = WindowsAppBoundDecryptor::for_browser("Chrome", &path)
                .expect("a DPAPI-only profile must open");
            let blob = v10_cookie(&key, ".example.com", "v10-value");
            assert_eq!(
                decryptor
                    .decrypt_bound_cookie(&blob[3..], 24, ".example.com")
                    .unwrap(),
                "v10-value"
            );
        }

        /// Regression: an `APPB` key the elevation service refuses must NOT degrade into the
        /// DPAPI key. Before the fix `for_browser` logged a warning, handed back the `v10`
        /// decryptor and every `v20` row then failed with a generic AES-GCM error.
        #[test]
        fn a_refused_app_bound_key_is_reported_instead_of_falling_back_to_dpapi() {
            let _guard = COM_DECRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let (_dir, path) = local_state(Some(b"sealed-app-bound".to_vec()), Some(&[0x11u8; 32]));

            *COM_DECRYPT_OVERRIDE.lock().unwrap() = Some(|_browser, _key| {
                Err(AppBoundKeyError::Refused { hresult: 0x8004_A007 })
            });
            let result = WindowsAppBoundDecryptor::for_browser("Chrome", &path);
            *COM_DECRYPT_OVERRIDE.lock().unwrap() = None;

            match result {
                Err(AppBoundKeyError::Refused { hresult }) => assert_eq!(hresult, 0x8004_A007),
                Err(other) => panic!("a refusal must stay a refusal, got {other:?}"),
                Ok(_) => panic!("the DPAPI key cannot open v20 rows; it must not stand in"),
            }
        }

        /// The seam still drives the success path: a released key opens `v20` cookies.
        #[test]
        fn a_released_app_bound_key_decrypts_a_v20_cookie() {
            static KEY: [u8; 32] = [0x77; 32];
            let _guard = COM_DECRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let (_dir, path) = local_state(Some(b"sealed-app-bound".to_vec()), None);

            *COM_DECRYPT_OVERRIDE.lock().unwrap() = Some(|_browser, payload| {
                assert_eq!(payload, b"sealed-app-bound", "the APPB prefix is stripped");
                Ok(Zeroizing::new(KEY.to_vec()))
            });
            let decryptor = WindowsAppBoundDecryptor::for_browser("Chrome", &path);
            *COM_DECRYPT_OVERRIDE.lock().unwrap() = None;
            let decryptor = decryptor.expect("a released key opens");

            use sha2::{Digest, Sha256};
            let iv = [4u8; 12];
            let mut plain = Sha256::digest(b".example.com").to_vec();
            plain.extend_from_slice(b"v20-value");
            let cipher = Aes256Gcm::new_from_slice(&KEY).unwrap();
            let mut blob = b"v20".to_vec();
            blob.extend_from_slice(&iv);
            blob.extend(cipher.encrypt(Nonce::from_slice(&iv), plain.as_slice()).unwrap());
            assert_eq!(
                decryptor.decrypt_bound_cookie(&blob[3..], 24, ".example.com").unwrap(),
                "v20-value"
            );
        }
    }
}
