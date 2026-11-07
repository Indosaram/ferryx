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

/// Decryptor for Windows Chrome / Edge / Brave cookies.
pub struct WindowsAppBoundDecryptor {
    aes_key: Zeroizing<Vec<u8>>,
}

impl WindowsAppBoundDecryptor {
    /// Attempts to decrypt the AES key from the browser's Local State.
    ///
    /// Tries the IElevator2 (Chrome 144+ v2) COM interface first, falling back
    /// to IElevator (v1) if available, and finally falling back to user-context DPAPI
    /// (for browsers that do not use App-Bound encryption).
    #[cfg(windows)]
    pub fn for_browser(browser_name: &str, local_state_path: &Path) -> Result<Self, String> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

        let content = std::fs::read_to_string(local_state_path).map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

        // 1. Try to load the app_bound_encrypted_key (v20 Chrome/Edge/Brave)
        if let Some(app_bound_b64) = json["os_crypt"]["app_bound_encrypted_key"].as_str() {
            use base64::Engine as _;
            let encrypted_key = base64::engine::general_purpose::STANDARD
                .decode(app_bound_b64)
                .map_err(|e| e.to_string())?;

            if encrypted_key.starts_with(b"APPB") {
                let payload = &encrypted_key[4..];
                // Try COM decryption
                match decrypt_via_com(browser_name, payload) {
                    Ok(key) => return Ok(Self { aes_key: key }),
                    Err(e) => {
                        tracing::warn!(browser = browser_name, error = %e, "app-bound key decryption failed; falling back to DPAPI");
                    }
                }
            }
        }

        // 2. Fall back to standard DPAPI (v10 / pre-Chrome-117 key format)
        Self::from_dpapi_json(&json).ok_or_else(|| {
            format!("Could not extract decryption key for browser {}", browser_name)
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

#[cfg(windows)]
fn decrypt_via_com(browser_name: &str, encrypted_key: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let config = get_browser_config(browser_name)
        .ok_or_else(|| format!("COM configuration missing for browser {}", browser_name))?;

    unsafe {
        let guid = |text: &str| GUID::try_from(text).map_err(|e| format!("invalid GUID {text}: {e}"));
        let clsid = guid(config.clsid)?;

        let unknown: IUnknown = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)
            .map_err(|e| format!("CoCreateInstance failed: {}", e))?;

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
            return Err(format!(
                "QueryInterface for IElevator failed with HRESULT 0x{:08X}",
                hr.0
            ));
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
            return Err(format!("CoSetProxyBlanket on IElevator failed: {error}"));
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
) -> Result<Zeroizing<Vec<u8>>, String> {
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
        return Err("BSTR allocation failed".into());
    }
    let mut plaintext_raw: *const u16 = std::ptr::null();
    let mut last_error: u32 = 0;

    let hr = decrypt_fn(unknown_ptr, ciphertext.as_ptr(), &mut plaintext_raw, &mut last_error);
    // Owns (and frees on drop) whatever the service returned, even on failure.
    let plaintext = BSTR::from_raw(plaintext_raw);

    if hr < 0 {
        return Err(format!(
            "COM DecryptData failed with HRESULT 0x{:08X}, last_error={}",
            hr, last_error
        ));
    }

    let size = SysStringByteLen(&plaintext) as usize;
    if plaintext_raw.is_null() || size == 0 {
        return Err("COM DecryptData returned null or empty output".into());
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
}
