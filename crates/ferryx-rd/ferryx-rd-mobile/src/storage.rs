use ferryx_rd_app::{PairingRecord, PairingStoreError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecureStorageError {
    #[error("secure item not found: {0}")]
    NotFound(String),
    #[error("keystore or keychain access failure: {0}")]
    PlatformError(String),
    #[error("pairing store error: {0}")]
    PairingError(#[from] PairingStoreError),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub trait SecureStorageBackend: Send + Sync {
    fn store_secret(&self, key: &str, secret: &[u8]) -> Result<(), SecureStorageError>;
    fn retrieve_secret(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError>;
    fn delete_secret(&self, key: &str) -> Result<(), SecureStorageError>;
    fn list_keys(&self) -> Result<Vec<String>, SecureStorageError>;
}

#[derive(Default)]
pub struct MockSecureStorage {
    vault: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockSecureStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecureStorageBackend for MockSecureStorage {
    fn store_secret(&self, key: &str, secret: &[u8]) -> Result<(), SecureStorageError> {
        let mut vault = self
            .vault
            .lock()
            .map_err(|e| SecureStorageError::PlatformError(e.to_string()))?;
        vault.insert(key.to_string(), secret.to_vec());
        Ok(())
    }

    fn retrieve_secret(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let vault = self
            .vault
            .lock()
            .map_err(|e| SecureStorageError::PlatformError(e.to_string()))?;
        Ok(vault.get(key).cloned())
    }

    fn delete_secret(&self, key: &str) -> Result<(), SecureStorageError> {
        let mut vault = self
            .vault
            .lock()
            .map_err(|e| SecureStorageError::PlatformError(e.to_string()))?;
        vault.remove(key);
        Ok(())
    }

    fn list_keys(&self) -> Result<Vec<String>, SecureStorageError> {
        let vault = self
            .vault
            .lock()
            .map_err(|e| SecureStorageError::PlatformError(e.to_string()))?;
        Ok(vault.keys().cloned().collect())
    }
}

pub struct MobilePairingStore {
    backend: Box<dyn SecureStorageBackend>,
}

impl MobilePairingStore {
    pub fn new(backend: Box<dyn SecureStorageBackend>) -> Self {
        Self { backend }
    }

    pub fn save_record(&self, record: &PairingRecord) -> Result<(), SecureStorageError> {
        record.key_array()?;
        let serialized = serde_json::to_vec(record)?;
        let key = format!("ferryx_rd_pairing_{}", record.id);
        self.backend.store_secret(&key, &serialized)?;
        Ok(())
    }

    pub fn load_record(&self, id: &str) -> Result<Option<PairingRecord>, SecureStorageError> {
        let key = format!("ferryx_rd_pairing_{id}");
        match self.backend.retrieve_secret(&key)? {
            Some(bytes) => {
                let record: PairingRecord = serde_json::from_slice(&bytes)?;
                record.key_array()?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    pub fn delete_record(&self, id: &str) -> Result<(), SecureStorageError> {
        let key = format!("ferryx_rd_pairing_{id}");
        self.backend.delete_secret(&key)
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, SecureStorageError> {
        let mut records = Vec::new();
        for key in self.backend.list_keys()? {
            if key.starts_with("ferryx_rd_pairing_") {
                if let Some(bytes) = self.backend.retrieve_secret(&key)? {
                    if let Ok(record) = serde_json::from_slice::<PairingRecord>(&bytes) {
                        if record.key_array().is_ok() {
                            records.push(record);
                        }
                    }
                }
            }
        }
        Ok(records)
    }

    pub fn find_by_host(
        &self,
        host_name: &str,
    ) -> Result<Option<PairingRecord>, SecureStorageError> {
        Ok(self
            .load_all()?
            .into_iter()
            .rfind(|record| record.name.eq_ignore_ascii_case(host_name) || record.id == host_name))
    }

    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub fn default_keychain() -> Self {
        Self::new(Box::new(IosKeychainStorage::new(
            "app.ferryx.rd.pairing",
        )))
    }

    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    pub fn default_keychain() -> Self {
        Self::new(Box::new(MockSecureStorage::new()))
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod security_ffi {
    use std::os::raw::c_void;

    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFDataRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFArrayRef = *const c_void;
    pub type CFIndex = isize;
    pub type OSStatus = i32;

    pub const ERR_SEC_SUCCESS: OSStatus = 0;
    pub const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
    pub const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        pub static kSecClass: CFStringRef;
        pub static kSecClassGenericPassword: CFTypeRef;
        pub static kSecAttrService: CFStringRef;
        pub static kSecAttrAccount: CFStringRef;
        pub static kSecValueData: CFStringRef;
        pub static kSecReturnData: CFStringRef;
        pub static kSecReturnAttributes: CFStringRef;
        pub static kSecMatchLimit: CFStringRef;
        pub static kSecMatchLimitOne: CFTypeRef;
        pub static kSecMatchLimitAll: CFTypeRef;
        pub static kSecAttrAccessible: CFStringRef;
        pub static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: CFTypeRef;

        pub fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemUpdate(
            query: CFDictionaryRef,
            attributesToUpdate: CFDictionaryRef,
        ) -> OSStatus;
        pub fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFBooleanTrue: CFTypeRef;
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        pub fn CFStringCreateWithBytes(
            alloc: CFTypeRef,
            bytes: *const u8,
            numBytes: CFIndex,
            encoding: u32,
            isExternalRepresentation: u8,
        ) -> CFStringRef;
        pub fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        pub fn CFDataGetLength(theData: CFDataRef) -> CFIndex;
        pub fn CFDataGetBytePtr(theData: CFDataRef) -> *const u8;
        pub fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            numValues: CFIndex,
            keyCallBacks: *const c_void,
            valueCallBacks: *const c_void,
        ) -> CFDictionaryRef;
        pub fn CFDictionaryGetValue(theDict: CFDictionaryRef, theKey: CFTypeRef) -> CFTypeRef;
        pub fn CFArrayGetCount(theArray: CFArrayRef) -> CFIndex;
        pub fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        pub fn CFStringGetCString(
            theString: CFStringRef,
            buffer: *mut u8,
            bufferSize: CFIndex,
            encoding: u32,
        ) -> u8;
        pub fn CFStringGetLength(theString: CFStringRef) -> CFIndex;
        pub fn CFRelease(cf: CFTypeRef);
    }
    pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
struct CfWrapper<T>(*const T);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl<T> Drop for CfWrapper<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                security_ffi::CFRelease(self.0 as security_ffi::CFTypeRef);
            }
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_string(s: &str) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            security_ffi::K_CF_STRING_ENCODING_UTF8,
            0,
        );
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_data(bytes: &[u8]) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_dictionary(
    pairs: &[(security_ffi::CFTypeRef, security_ffi::CFTypeRef)],
) -> Option<CfWrapper<std::os::raw::c_void>> {
    let mut keys = Vec::with_capacity(pairs.len());
    let mut values = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        keys.push(*k);
        values.push(*v);
    }
    unsafe {
        let dict = security_ffi::CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as isize,
            &security_ffi::kCFTypeDictionaryKeyCallBacks as *const _ as *const _,
            &security_ffi::kCFTypeDictionaryValueCallBacks as *const _ as *const _,
        );
        if dict.is_null() {
            None
        } else {
            Some(CfWrapper(dict))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn cf_data_to_vec(data: security_ffi::CFDataRef) -> Vec<u8> {
    unsafe {
        let len = security_ffi::CFDataGetLength(data) as usize;
        let ptr = security_ffi::CFDataGetBytePtr(data);
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(ptr, len).to_vec()
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn cf_string_to_string(s: security_ffi::CFStringRef) -> Option<String> {
    unsafe {
        let len = security_ffi::CFStringGetLength(s);
        let max_len = len * 4 + 1;
        let mut buf = vec![0u8; max_len as usize];
        if security_ffi::CFStringGetCString(
            s,
            buf.as_mut_ptr(),
            max_len,
            security_ffi::K_CF_STRING_ENCODING_UTF8,
        ) != 0
        {
            let nul_idx = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8(buf[..nul_idx].to_vec()).ok()
        } else {
            None
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug, Clone)]
pub struct IosKeychainStorage {
    service: String,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl IosKeychainStorage {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl SecureStorageBackend for IosKeychainStorage {
    fn store_secret(&self, key: &str, secret: &[u8]) -> Result<(), SecureStorageError> {
        unsafe {
            let service_cf = make_cf_string(&self.service).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for service".into())
            })?;
            let account_cf = make_cf_string(key).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for account".into())
            })?;
            let data_cf = make_cf_data(secret).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFData for secret".into())
            })?;

            let pairs = [
                (
                    security_ffi::kSecClass as security_ffi::CFTypeRef,
                    security_ffi::kSecClassGenericPassword,
                ),
                (
                    security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                    service_cf.0,
                ),
                (
                    security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    account_cf.0,
                ),
                (
                    security_ffi::kSecValueData as security_ffi::CFTypeRef,
                    data_cf.0,
                ),
                (
                    security_ffi::kSecAttrAccessible as security_ffi::CFTypeRef,
                    security_ffi::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
                ),
            ];
            let dict = make_cf_dictionary(&pairs).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFDictionary".into())
            })?;

            let status = security_ffi::SecItemAdd(dict.0, std::ptr::null_mut());
            if status == security_ffi::ERR_SEC_DUPLICATE_ITEM {
                let query_pairs = [
                    (
                        security_ffi::kSecClass as security_ffi::CFTypeRef,
                        security_ffi::kSecClassGenericPassword,
                    ),
                    (
                        security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                        service_cf.0,
                    ),
                    (
                        security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                        account_cf.0,
                    ),
                ];
                let query_dict = make_cf_dictionary(&query_pairs).ok_or_else(|| {
                    SecureStorageError::PlatformError(
                        "Failed to allocate query CFDictionary".into(),
                    )
                })?;
                let update_pairs = [(
                    security_ffi::kSecValueData as security_ffi::CFTypeRef,
                    data_cf.0,
                )];
                let update_dict = make_cf_dictionary(&update_pairs).ok_or_else(|| {
                    SecureStorageError::PlatformError(
                        "Failed to allocate update CFDictionary".into(),
                    )
                })?;

                let update_status = security_ffi::SecItemUpdate(query_dict.0, update_dict.0);
                if update_status != security_ffi::ERR_SEC_SUCCESS {
                    return Err(SecureStorageError::PlatformError(format!(
                        "SecItemUpdate failed: OSStatus {update_status}"
                    )));
                }
                Ok(())
            } else if status != security_ffi::ERR_SEC_SUCCESS {
                Err(SecureStorageError::PlatformError(format!(
                    "SecItemAdd failed: OSStatus {status}"
                )))
            } else {
                Ok(())
            }
        }
    }

    fn retrieve_secret(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        unsafe {
            let service_cf = make_cf_string(&self.service).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for service".into())
            })?;
            let account_cf = make_cf_string(key).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for account".into())
            })?;

            let query_pairs = [
                (
                    security_ffi::kSecClass as security_ffi::CFTypeRef,
                    security_ffi::kSecClassGenericPassword,
                ),
                (
                    security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                    service_cf.0,
                ),
                (
                    security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    account_cf.0,
                ),
                (
                    security_ffi::kSecReturnData as security_ffi::CFTypeRef,
                    security_ffi::kCFBooleanTrue,
                ),
                (
                    security_ffi::kSecMatchLimit as security_ffi::CFTypeRef,
                    security_ffi::kSecMatchLimitOne,
                ),
            ];
            let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFDictionary".into())
            })?;

            let mut result: security_ffi::CFTypeRef = std::ptr::null();
            let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
            if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
                return Ok(None);
            }
            if status != security_ffi::ERR_SEC_SUCCESS {
                return Err(SecureStorageError::PlatformError(format!(
                    "SecItemCopyMatching failed: OSStatus {status}"
                )));
            }
            if result.is_null() {
                return Ok(None);
            }
            let wrapper = CfWrapper(result);
            let bytes = cf_data_to_vec(wrapper.0);
            Ok(Some(bytes))
        }
    }

    fn delete_secret(&self, key: &str) -> Result<(), SecureStorageError> {
        unsafe {
            let service_cf = make_cf_string(&self.service).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for service".into())
            })?;
            let account_cf = make_cf_string(key).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for account".into())
            })?;

            let query_pairs = [
                (
                    security_ffi::kSecClass as security_ffi::CFTypeRef,
                    security_ffi::kSecClassGenericPassword,
                ),
                (
                    security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                    service_cf.0,
                ),
                (
                    security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    account_cf.0,
                ),
            ];
            let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFDictionary".into())
            })?;

            let status = security_ffi::SecItemDelete(query.0);
            if status == security_ffi::ERR_SEC_SUCCESS
                || status == security_ffi::ERR_SEC_ITEM_NOT_FOUND
            {
                Ok(())
            } else {
                Err(SecureStorageError::PlatformError(format!(
                    "SecItemDelete failed: OSStatus {status}"
                )))
            }
        }
    }

    fn list_keys(&self) -> Result<Vec<String>, SecureStorageError> {
        unsafe {
            let service_cf = make_cf_string(&self.service).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFString for service".into())
            })?;

            let query_pairs = [
                (
                    security_ffi::kSecClass as security_ffi::CFTypeRef,
                    security_ffi::kSecClassGenericPassword,
                ),
                (
                    security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                    service_cf.0,
                ),
                (
                    security_ffi::kSecReturnAttributes as security_ffi::CFTypeRef,
                    security_ffi::kCFBooleanTrue,
                ),
                (
                    security_ffi::kSecMatchLimit as security_ffi::CFTypeRef,
                    security_ffi::kSecMatchLimitAll,
                ),
            ];
            let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
                SecureStorageError::PlatformError("Failed to allocate CFDictionary".into())
            })?;

            let mut result: security_ffi::CFTypeRef = std::ptr::null();
            let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
            if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
                return Ok(Vec::new());
            }
            if status != security_ffi::ERR_SEC_SUCCESS {
                return Err(SecureStorageError::PlatformError(format!(
                    "SecItemCopyMatching list failed: OSStatus {status}"
                )));
            }
            if result.is_null() {
                return Ok(Vec::new());
            }
            let wrapper = CfWrapper(result);
            let count = security_ffi::CFArrayGetCount(wrapper.0);
            let mut keys = Vec::with_capacity(count as usize);
            for i in 0..count {
                let dict = security_ffi::CFArrayGetValueAtIndex(wrapper.0, i);
                if !dict.is_null() {
                    let account_val = security_ffi::CFDictionaryGetValue(
                        dict,
                        security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    );
                    if !account_val.is_null() {
                        if let Some(key_str) = cf_string_to_string(account_val) {
                            keys.push(key_str);
                        }
                    }
                }
            }
            Ok(keys)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_pairing_store_round_trip_and_delete() {
        let backend = Box::new(MockSecureStorage::new());
        let store = MobilePairingStore::new(backend);

        let record = PairingRecord {
            id: "mobile-peer-1".into(),
            name: "Galaxy S24".into(),
            key: vec![0x42; 32],
            added_at_unix_ms: 1700000000000,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        };

        store.save_record(&record).unwrap();
        let loaded = store.load_record("mobile-peer-1").unwrap();
        assert_eq!(loaded, Some(record.clone()));

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "mobile-peer-1");

        store.delete_record("mobile-peer-1").unwrap();
        assert_eq!(store.load_record("mobile-peer-1").unwrap(), None);
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn find_by_host_matches_name_and_id() {
        let backend = Box::new(MockSecureStorage::new());
        let store = MobilePairingStore::new(backend);

        let record = PairingRecord {
            id: "host-uuid-1234".into(),
            name: "Workstation".into(),
            key: vec![0x33; 32],
            added_at_unix_ms: 1700000000000,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        };
        store.save_record(&record).unwrap();

        assert_eq!(
            store.find_by_host("Workstation").unwrap(),
            Some(record.clone())
        );
        assert_eq!(
            store.find_by_host("workstation").unwrap(),
            Some(record.clone())
        );
        assert_eq!(
            store.find_by_host("host-uuid-1234").unwrap(),
            Some(record.clone())
        );
        assert_eq!(store.find_by_host("Laptop").unwrap(), None);
    }
}
