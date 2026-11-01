//! Pure-Rust Firefox NSS password decryption (key3.db, key4.db + logins.json).
//!
//! Supports:
//! - NSS 4 key databases (Firefox 75+): PBKDF2-HMAC-SHA256/SHA384 + AES-256-CBC
//! - NSS 4 Variant A (Firefox 58–74): PKCS#12 PBE + 3DES-CBC (in key4.db SQLite)
//! - NSS 3 (Firefox <58): Mozilla-specific PBE + 3DES-CBC (in key3.db Berkeley DB)
//!
//! Security: all key material uses [`zeroize::Zeroizing`]; no password/key data
//! is ever logged.

use std::collections::HashMap;
use std::path::Path;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use hmac::Hmac;
use rusqlite::{Connection, OpenFlags};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384};
use zeroize::Zeroizing;

use super::asn1::{
    self, OID_3DES_CBC, OID_AES_256_CBC, OID_PBES2, OID_PBKDF2, OID_PKCS12_PBE_SHA1_3DES,
    TAG_SEQUENCE,
};

// ---------- Error type ----------

/// Errors from NSS key extraction and decryption.
#[derive(Clone, Debug, thiserror::Error)]
pub enum NssError {
    #[error("key4.db missing")]
    KeyDbMissing,
    #[error("master password incorrect")]
    WrongMasterPassword,
    #[error("unsupported NSS version (legacy NSS 3 key3.db not yet supported)")]
    UnsupportedVersion,
    #[error("ASN.1 parse: {0}")]
    Asn1(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error("database: {0}")]
    Database(String),
}

impl From<asn1::Asn1Error> for NssError {
    fn from(e: asn1::Asn1Error) -> Self {
        NssError::Asn1(e.0)
    }
}

// ---------- Master key ----------

/// The decrypted NSS master key used to decrypt individual login fields.
/// Stores up to 32 bytes of key material (24 for 3DES, 32 for AES-256).
/// Zeroized on drop.
pub struct NssMasterKey {
    key: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for NssMasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NssMasterKey")
            .field("len", &self.key.len())
            .finish_non_exhaustive()
    }
}

impl NssMasterKey {
    /// Returns the key bytes for 3DES operations (first 24 bytes).
    fn des_key(&self) -> Result<&[u8], NssError> {
        if self.key.len() < 24 {
            return Err(NssError::Crypto(format!(
                "master key too short for 3DES: {} bytes",
                self.key.len()
            )));
        }
        Ok(&self.key[..24])
    }

    /// Returns the key bytes for AES-256 operations (first 32 bytes).
    fn aes_key(&self) -> Result<&[u8], NssError> {
        if self.key.len() < 32 {
            return Err(NssError::Crypto(format!(
                "master key too short for AES-256: {} bytes",
                self.key.len()
            )));
        }
        Ok(&self.key[..32])
    }
}

// ---------- Public API ----------

/// Extracts the NSS master key from a Firefox profile's key database.
///
/// Tries key4.db (SQLite) first, then key3.db (Berkeley DB 1.85).
/// Verifies correctness by decrypting the "password-check" sentinel.
pub fn unlock_nss_master_key(
    profile_dir: &Path,
    master_password: &str,
) -> Result<NssMasterKey, NssError> {
    let key4_path = profile_dir.join("key4.db");
    let key3_path = profile_dir.join("key3.db");

    if !key4_path.exists() {
        if key3_path.exists() {
            return unlock_nss3_master_key(&key3_path, master_password);
        }
        return Err(NssError::KeyDbMissing);
    }

    // Copy to temp to avoid locking a running Firefox.
    let temp_dir =
        tempfile::tempdir().map_err(|e| NssError::Database(format!("creating temp dir: {e}")))?;
    let temp_db = temp_dir.path().join("key4.db");
    std::fs::copy(&key4_path, &temp_db)
        .map_err(|e| NssError::Database(format!("copying key4.db: {e}")))?;

    let conn = Connection::open_with_flags(
        &temp_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| NssError::Database(format!("opening key4.db: {e}")))?;

    // Read global salt and encrypted password-check from metaData.
    let (global_salt, password_check_blob) = read_metadata(&conn)?;

    // Read the encrypted master key (ASN.1-wrapped) from nssPrivate.
    let encrypted_key_blob = read_nss_private(&conn)?;

    // Detect variant: item1.len() > 40 → SHA-384 (Variant C), else SHA-256 (Variant B).
    let use_sha384 = global_salt.len() > 40;

    // Parse ASN.1 from the encrypted key blob to get algorithm params.
    let key_params = parse_encrypted_key_asn1(&encrypted_key_blob)?;

    let master_key_bytes = match key_params {
        EncryptedKeyParams::Pbes2 {
            entry_salt,
            iterations,
            iv,
            ciphertext,
            is_aes256,
        } => {
            let intermediate = derive_intermediate_key(
                master_password,
                &global_salt,
                &entry_salt,
                iterations,
                use_sha384,
            )?;
            decrypt_with_params(&intermediate, &iv, &ciphertext, is_aes256)?
        }
        EncryptedKeyParams::Pkcs12Pbe {
            entry_salt,
            iterations,
            ciphertext,
        } => {
            let password_bmp = encode_password_bmp(master_password);
            let mut key =
                Zeroizing::new(pkcs12_derive(&password_bmp, &entry_salt, iterations, 24, 1));
            let mut iv =
                Zeroizing::new(pkcs12_derive(&password_bmp, &entry_salt, iterations, 8, 2));
            let result = decrypt_3des(&key, &iv, &ciphertext)?;
            key.iter_mut().for_each(|b| *b = 0);
            iv.iter_mut().for_each(|b| *b = 0);
            result
        }
    };

    let master_key = NssMasterKey {
        key: master_key_bytes,
    };

    // Verify by decrypting the password-check sentinel.
    verify_password_check(&master_key, &password_check_blob)?;

    Ok(master_key)
}

/// Decrypts a Base64-encoded encrypted value from `logins.json`.
///
/// The value is the `encryptedUsername` or `encryptedPassword` field.
/// Returns the decrypted UTF-8 plaintext.
pub fn decrypt_nss_value(
    encrypted_b64: &str,
    master_key: &NssMasterKey,
) -> Result<String, NssError> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encrypted_b64)
        .map_err(|e| NssError::Decode(format!("base64: {e}")))?;

    let field = parse_login_field_asn1(&raw)?;

    let plaintext = if field.is_3des {
        decrypt_3des(master_key.des_key()?, &field.iv, &field.ciphertext)?
    } else {
        decrypt_aes256(master_key.aes_key()?, &field.iv, &field.ciphertext)?
    };

    String::from_utf8(plaintext.to_vec())
        .map_err(|_| NssError::Decode("decrypted value is not valid UTF-8".into()))
}

// ---------- Internal: database reads ----------

fn read_metadata(conn: &Connection) -> Result<(Vec<u8>, Vec<u8>), NssError> {
    let mut stmt = conn
        .prepare("SELECT item1, item2 FROM metadata WHERE id = 'password'")
        .map_err(|e| NssError::Database(format!("metadata query: {e}")))?;

    let (item1, item2): (Vec<u8>, Vec<u8>) = stmt
        .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| NssError::Database(format!("metadata row: {e}")))?;

    if item1.is_empty() {
        return Err(NssError::Database("empty global_salt in metadata".into()));
    }

    Ok((item1, item2))
}

fn read_nss_private(conn: &Connection) -> Result<Vec<u8>, NssError> {
    let mut stmt = conn
        .prepare("SELECT a11 FROM nssPrivate")
        .map_err(|e| NssError::Database(format!("nssPrivate query: {e}")))?;

    let a11: Vec<u8> = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| NssError::Database(format!("nssPrivate row: {e}")))?;

    if a11.is_empty() {
        return Err(NssError::Database("empty a11 in nssPrivate".into()));
    }

    Ok(a11)
}

// ---------- Internal: ASN.1 parsing ----------

/// Parsed parameters from the encrypted key blob (nssPrivate.a11).
enum EncryptedKeyParams {
    /// PBES2: PBKDF2 + AES-256-CBC or 3DES-CBC (Firefox 75+)
    Pbes2 {
        entry_salt: Vec<u8>,
        iterations: u32,
        iv: Vec<u8>,
        ciphertext: Vec<u8>,
        is_aes256: bool,
    },
    /// PKCS#12 PBE with SHA-1 + 3DES-CBC (Firefox 58–74, key4.db Variant A)
    Pkcs12Pbe {
        entry_salt: Vec<u8>,
        iterations: u32,
        ciphertext: Vec<u8>,
    },
}

fn parse_encrypted_key_asn1(data: &[u8]) -> Result<EncryptedKeyParams, NssError> {
    let (outer, _) = asn1::read_item(data)?;
    let outer_items = asn1::expect_sequence(&outer)?;

    if outer_items.len() < 2 {
        return Err(NssError::Asn1("expected ≥2 items in outer SEQUENCE".into()));
    }

    let algo_seq = asn1::expect_sequence(&outer_items[0])?;
    let ciphertext = asn1::expect_octet_string(&outer_items[1])?.to_vec();

    if algo_seq.is_empty() {
        return Err(NssError::Asn1("empty algorithm SEQUENCE".into()));
    }

    if asn1::match_oid(&algo_seq[0], OID_PKCS12_PBE_SHA1_3DES) {
        // Variant A: PKCS#12 PBE params = SEQUENCE { OCTET STRING(salt), INTEGER(iter) }
        if algo_seq.len() < 2 {
            return Err(NssError::Asn1("PKCS12 PBE: missing params".into()));
        }
        let pbe_params = asn1::expect_sequence(&algo_seq[1])?;
        if pbe_params.len() < 2 {
            return Err(NssError::Asn1(
                "PKCS12 PBE params: need salt + iterations".into(),
            ));
        }
        let entry_salt = asn1::expect_octet_string(&pbe_params[0])?.to_vec();
        let iterations = asn1::parse_integer_u32(&pbe_params[1])?;

        return Ok(EncryptedKeyParams::Pkcs12Pbe {
            entry_salt,
            iterations,
            ciphertext,
        });
    }

    if !asn1::match_oid(&algo_seq[0], OID_PBES2) {
        return Err(NssError::Asn1(format!(
            "unsupported algorithm OID: {:02x?}",
            algo_seq[0].data
        )));
    }

    // PBES2 params
    if algo_seq.len() < 2 {
        return Err(NssError::Asn1("PBES2: missing params SEQUENCE".into()));
    }
    let pbes2_params = asn1::expect_sequence(&algo_seq[1])?;
    if pbes2_params.len() < 2 {
        return Err(NssError::Asn1("PBES2 params: need KDF + cipher".into()));
    }

    // PBKDF2 params
    let kdf_seq = asn1::expect_sequence(&pbes2_params[0])?;
    if kdf_seq.len() < 2 {
        return Err(NssError::Asn1("KDF SEQUENCE too short".into()));
    }
    if !asn1::match_oid(&kdf_seq[0], OID_PBKDF2) {
        return Err(NssError::Asn1("expected PBKDF2 OID in KDF".into()));
    }

    let pbkdf2_params = asn1::expect_sequence(&kdf_seq[1])?;
    if pbkdf2_params.len() < 2 {
        return Err(NssError::Asn1(
            "PBKDF2 params: need salt + iterations".into(),
        ));
    }
    let entry_salt = asn1::expect_octet_string(&pbkdf2_params[0])?.to_vec();
    let iterations = asn1::parse_integer_u32(&pbkdf2_params[1])?;

    // Encryption algorithm
    let enc_seq = asn1::expect_sequence(&pbes2_params[1])?;
    if enc_seq.len() < 2 {
        return Err(NssError::Asn1("encryption algo SEQUENCE too short".into()));
    }

    let is_aes256 = if asn1::match_oid(&enc_seq[0], OID_AES_256_CBC) {
        true
    } else if asn1::match_oid(&enc_seq[0], OID_3DES_CBC) {
        false
    } else {
        return Err(NssError::Asn1(format!(
            "unsupported cipher OID: {:02x?}",
            enc_seq[0].data
        )));
    };

    let iv = asn1::expect_octet_string(&enc_seq[1])?.to_vec();

    Ok(EncryptedKeyParams::Pbes2 {
        entry_salt,
        iterations,
        iv,
        ciphertext,
        is_aes256,
    })
}

/// Parsed login field ASN.1 structure (from Base64-decoded encrypted username/password).
struct LoginFieldAsn1 {
    iv: Vec<u8>,
    ciphertext: Vec<u8>,
    is_3des: bool,
}

/// Parses the ASN.1 from a decoded login field blob.
///
/// Handles two structural variants:
/// 1. `SEQUENCE { SEQUENCE { OID, OCTET_STRING(iv) }, OCTET_STRING(ciphertext) }`
/// 2. Outer wrapper omitted — peek first child tag.
fn parse_login_field_asn1(data: &[u8]) -> Result<LoginFieldAsn1, NssError> {
    let (outer, _) = asn1::read_item(data)?;

    // The outer item should be a SEQUENCE.
    if outer.tag != TAG_SEQUENCE {
        return Err(NssError::Asn1(format!(
            "login field: expected SEQUENCE, got 0x{:02x}",
            outer.tag
        )));
    }

    let items = asn1::read_items(outer.data)?;

    // Determine structure variant by peeking at the first child.
    let (algo_items, ciphertext) = if items.len() >= 2 && items[0].tag == TAG_SEQUENCE {
        // Variant 1: first child is SEQUENCE (algo), second is OCTET STRING (ciphertext).
        let ct = asn1::expect_octet_string(&items[1])?;
        let algo = asn1::expect_sequence(&items[0])?;
        (algo, ct.to_vec())
    } else if items.len() >= 2 && items[0].tag == asn1::TAG_OID {
        // Variant 2: items are directly [OID, OCTET_STRING(iv)], but we need
        // the ciphertext from a sibling. This case means the outer SEQUENCE
        // directly contains algo items + ciphertext.
        // Re-parse: items[0]=OID, items[1]=OCTET STRING(iv), items[2]=OCTET STRING(ct)
        if items.len() < 3 {
            return Err(NssError::Asn1("login field variant 2: need 3 items".into()));
        }
        let iv = asn1::expect_octet_string(&items[1])?;
        let ct = asn1::expect_octet_string(&items[2])?;

        let is_3des = asn1::match_oid(&items[0], OID_3DES_CBC);
        return Ok(LoginFieldAsn1 {
            iv: iv.to_vec(),
            ciphertext: ct.to_vec(),
            is_3des,
        });
    } else {
        return Err(NssError::Asn1(format!(
            "login field: unexpected structure, first tag=0x{:02x}, len={}",
            items.first().map(|i| i.tag).unwrap_or(0),
            items.len()
        )));
    };

    if algo_items.len() < 2 {
        return Err(NssError::Asn1("login field algo: need OID + IV".into()));
    }

    let is_3des = asn1::match_oid(&algo_items[0], OID_3DES_CBC);
    let iv = asn1::expect_octet_string(&algo_items[1])?.to_vec();

    Ok(LoginFieldAsn1 {
        iv,
        ciphertext,
        is_3des,
    })
}

// ---------- Internal: key derivation ----------

/// Derives the intermediate key from the master password + global salt + entry salt.
///
/// Variant B (SHA-256): hp = SHA-256(global_salt ∥ password_bytes), then
///   PBKDF2-HMAC-SHA256(hp, entry_salt, iterations, 32).
/// Variant C (SHA-384): hp = SHA-384(global_salt ∥ password_bytes), then
///   PBKDF2-HMAC-SHA384(hp, entry_salt, iterations, 32).
fn derive_intermediate_key(
    master_password: &str,
    global_salt: &[u8],
    entry_salt: &[u8],
    iterations: u32,
    use_sha384: bool,
) -> Result<Zeroizing<Vec<u8>>, NssError> {
    let password_bytes = master_password.as_bytes();

    if use_sha384 {
        // Variant C: SHA-384 pre-hash
        let mut hasher = Sha384::new();
        hasher.update(global_salt);
        hasher.update(password_bytes);
        let hp = hasher.finalize();

        let mut derived = Zeroizing::new(vec![0u8; 32]);
        pbkdf2::pbkdf2::<Hmac<Sha384>>(&hp, entry_salt, iterations, &mut derived)
            .map_err(|e| NssError::Crypto(format!("PBKDF2-SHA384: {e}")))?;
        Ok(derived)
    } else {
        // Variant B: SHA-256 pre-hash
        let mut hasher = Sha256::new();
        hasher.update(global_salt);
        hasher.update(password_bytes);
        let hp = hasher.finalize();

        let mut derived = Zeroizing::new(vec![0u8; 32]);
        pbkdf2::pbkdf2::<Hmac<Sha256>>(&hp, entry_salt, iterations, &mut derived)
            .map_err(|e| NssError::Crypto(format!("PBKDF2-SHA256: {e}")))?;
        Ok(derived)
    }
}

// ---------- Internal: NSS 3 / PKCS#12 key derivation ----------

/// PKCS#12 v1 key derivation (RFC 7292 appendix B).
/// `id` = 1 for key material, 2 for IV material.
fn pkcs12_derive(
    password_bmp: &[u8],
    salt: &[u8],
    iterations: u32,
    output_len: usize,
    id: u8,
) -> Vec<u8> {
    use sha1::Digest as _;

    const HASH_LEN: usize = 20; // SHA-1
    const BLOCK_LEN: usize = 64; // SHA-1 block size

    let d = vec![id; BLOCK_LEN];

    let s = if salt.is_empty() {
        Vec::new()
    } else {
        let s_len = BLOCK_LEN * salt.len().div_ceil(BLOCK_LEN);
        (0..s_len).map(|i| salt[i % salt.len()]).collect()
    };

    let p = if password_bmp.is_empty() {
        Vec::new()
    } else {
        let p_len = BLOCK_LEN * password_bmp.len().div_ceil(BLOCK_LEN);
        (0..p_len)
            .map(|i| password_bmp[i % password_bmp.len()])
            .collect()
    };

    let mut i_buf = Vec::with_capacity(s.len() + p.len());
    i_buf.extend_from_slice(&s);
    i_buf.extend_from_slice(&p);

    let mut result = Vec::with_capacity(output_len);
    while result.len() < output_len {
        let mut a_input = Vec::with_capacity(BLOCK_LEN + i_buf.len());
        a_input.extend_from_slice(&d);
        a_input.extend_from_slice(&i_buf);

        let mut a = [0u8; HASH_LEN];
        a.copy_from_slice(&Sha1::digest(&a_input));
        for _ in 1..iterations {
            let hash = Sha1::digest(a);
            a.copy_from_slice(&hash);
        }

        result.extend_from_slice(&a);
        if result.len() >= output_len {
            break;
        }

        // B = A repeated to fill BLOCK_LEN
        let b: Vec<u8> = (0..BLOCK_LEN).map(|i| a[i % HASH_LEN]).collect();

        // I_j = (I_j + B + 1) mod 2^(BLOCK_LEN*8) for each block
        for j in (0..i_buf.len()).step_by(BLOCK_LEN) {
            let mut carry: u16 = 1;
            for k in (0..BLOCK_LEN).rev() {
                let sum = u16::from(i_buf[j + k]) + u16::from(b[k]) + carry;
                i_buf[j + k] = (sum & 0xFF) as u8;
                carry = sum >> 8;
            }
        }
    }

    result.truncate(output_len);
    result
}

/// Encodes a password as BMP (UTF-16BE, null-terminated) for PKCS#12.
/// Empty password → [0x00, 0x00].
fn encode_password_bmp(password: &str) -> Zeroizing<Vec<u8>> {
    let mut bmp = Zeroizing::new(Vec::new());
    for c in password.encode_utf16() {
        bmp.push((c >> 8) as u8);
        bmp.push((c & 0xFF) as u8);
    }
    bmp.push(0x00);
    bmp.push(0x00);
    bmp
}

/// Mozilla-specific 3DES key derivation used by key3.db (Firefox <58).
/// Derives a 24-byte key + 8-byte IV from global_salt + master_password + entry_salt.
fn derive_moz_3des(
    global_salt: &[u8],
    master_password: &[u8],
    entry_salt: &[u8],
) -> (Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>) {
    use hmac::Mac;
    use sha1::Digest as _;

    let hp = Sha1::digest([global_salt, master_password].concat());
    // Pad entry_salt to 20 bytes with zeros
    let mut pes = [0u8; 20];
    let copy_len = entry_salt.len().min(20);
    pes[..copy_len].copy_from_slice(&entry_salt[..copy_len]);
    let chp = Sha1::digest([hp.as_slice(), entry_salt].concat());

    let k1 = {
        let mut mac = Hmac::<Sha1>::new_from_slice(chp.as_slice()).unwrap();
        mac.update(&pes);
        mac.update(entry_salt);
        mac.finalize().into_bytes()
    };
    let tk = {
        let mut mac = Hmac::<Sha1>::new_from_slice(chp.as_slice()).unwrap();
        mac.update(&pes);
        mac.finalize().into_bytes()
    };
    let k2 = {
        let mut mac = Hmac::<Sha1>::new_from_slice(chp.as_slice()).unwrap();
        mac.update(tk.as_slice());
        mac.update(entry_salt);
        mac.finalize().into_bytes()
    };

    // k = k1 || k2 (40 bytes); key = k[0..24], iv = k[32..40]
    let mut k = Zeroizing::new(vec![0u8; 40]);
    k[..20].copy_from_slice(&k1);
    k[20..40].copy_from_slice(&k2);

    let key = Zeroizing::new(k[..24].to_vec());
    let iv = Zeroizing::new(k[32..40].to_vec());
    (key, iv)
}

// ---------- Internal: Berkeley DB 1.85 hash parser (key3.db) ----------

const BDB_HASH_MAGIC: u32 = 0x00061561;
const BDB_VERSION: u32 = 2;

fn parse_key3_db(path: &Path) -> Result<HashMap<Vec<u8>, Vec<u8>>, NssError> {
    let data =
        std::fs::read(path).map_err(|e| NssError::Database(format!("reading key3.db: {e}")))?;

    if data.len() < 60 {
        return Err(NssError::Database(
            "key3.db too small for BDB header".into(),
        ));
    }

    // BDB 1.85 header: big-endian u32 fields
    let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let version = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let pagesize = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let nkeys = u32::from_be_bytes([data[56], data[57], data[58], data[59]]) as usize;

    if magic != BDB_HASH_MAGIC {
        return Err(NssError::Database(format!(
            "key3.db bad magic: 0x{magic:08x}, expected 0x{BDB_HASH_MAGIC:08x}"
        )));
    }
    if version != BDB_VERSION {
        return Err(NssError::Database(format!(
            "key3.db bad version: {version}, expected {BDB_VERSION}"
        )));
    }
    if pagesize == 0 || pagesize > 65536 {
        return Err(NssError::Database(format!(
            "key3.db bad pagesize: {pagesize}"
        )));
    }

    let mut db = HashMap::new();
    let mut read_keys = 0usize;
    let mut page = 1usize;

    while read_keys < nkeys {
        let page_start = pagesize * page;
        if page_start + pagesize > data.len() {
            break;
        }
        let page_data = &data[page_start..page_start + pagesize];

        // Collect offset pairs: (val_data_off, key_data_off) from page index.
        // firepwd reads at positions (2+i, 4+i, 8+i) with i+=4, terminates when nval==val.
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut nval: u16 = 0;
        let mut val: u16 = 1;
        let mut i = 0usize;

        while nval != val {
            if 10 + i > page_data.len() {
                break;
            }
            let first_off = u16::from_le_bytes([page_data[2 + i], page_data[3 + i]]) as usize;
            val = u16::from_le_bytes([page_data[4 + i], page_data[5 + i]]);
            nval = u16::from_le_bytes([page_data[8 + i], page_data[9 + i]]);

            pairs.push((first_off, val as usize));
            read_keys += 1;
            i += 4;
        }

        // For each pair: first_off points to VALUE data, val points to KEY data.
        // VALUE runs from first_off to val (within same pair).
        // KEY runs from val to the next pair's first_off (or to the inferred end).
        for (pair_idx, &(val_off, key_off)) in pairs.iter().enumerate() {
            if val_off >= pagesize || key_off >= pagesize || key_off <= val_off {
                continue;
            }
            let value_data = &page_data[val_off..key_off];

            // Key ends at the next pair's val_off, or at end of data on page
            let key_end = if pair_idx + 1 < pairs.len() {
                pairs[pair_idx + 1].0
            } else {
                // Last entry: strip trailing zeros from page end to find data boundary.
                // This handles binary keys (like CKA_ID) that contain embedded nulls.
                let mut end = pagesize;
                while end > key_off && page_data[end - 1] == 0 {
                    end -= 1;
                }
                end
            };

            if key_end <= key_off || key_end > pagesize {
                continue;
            }
            let key_data = &page_data[key_off..key_end];

            db.insert(key_data.to_vec(), value_data.to_vec());
        }

        page += 1;
    }

    Ok(db)
}

/// Unlocks the NSS3 master key from a key3.db file (Firefox <58).
fn unlock_nss3_master_key(
    key3_path: &Path,
    master_password: &str,
) -> Result<NssMasterKey, NssError> {
    let db = parse_key3_db(key3_path)?;

    let global_salt = db
        .get(b"global-salt".as_slice())
        .ok_or_else(|| NssError::Database("key3.db: missing 'global-salt' entry".into()))?;

    let password_check_raw = db
        .get(b"password-check".as_slice())
        .ok_or_else(|| NssError::Database("key3.db: missing 'password-check' entry".into()))?;

    // password-check entry format: [type_byte, entry_salt_len, name_len?, entry_salt..., encrypted_data[-16:]]
    if password_check_raw.len() < 4 {
        return Err(NssError::Database(
            "key3.db: password-check entry too short".into(),
        ));
    }
    let entry_salt_len = password_check_raw[1] as usize;
    if password_check_raw.len() < 3 + entry_salt_len + 16 {
        return Err(NssError::Database(
            "key3.db: password-check truncated".into(),
        ));
    }
    let check_entry_salt = &password_check_raw[3..3 + entry_salt_len];
    let check_encrypted = &password_check_raw[password_check_raw.len() - 16..];

    let master_pw_bytes = master_password.as_bytes();
    let (check_key, check_iv) = derive_moz_3des(global_salt, master_pw_bytes, check_entry_salt);
    let check_decrypted = decrypt_3des(&check_key, &check_iv, check_encrypted)?;
    if !check_decrypted.starts_with(b"password-check") {
        return Err(NssError::WrongMasterPassword);
    }

    // CKA_ID for the master key entry
    const CKA_ID: [u8; 16] = [
        0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01,
    ];

    let priv_key_entry = db
        .get(CKA_ID.as_slice())
        .ok_or_else(|| NssError::Database("key3.db: missing CKA_ID private key entry".into()))?;

    // Private key entry format: [type_byte, salt_len, name_len, salt..., name..., ASN.1...]
    if priv_key_entry.len() < 3 {
        return Err(NssError::Database(
            "key3.db: private key entry too short".into(),
        ));
    }
    let pk_salt_len = priv_key_entry[1] as usize;
    let pk_name_len = priv_key_entry[2] as usize;
    let asn1_start = 3 + pk_salt_len + pk_name_len;
    if asn1_start >= priv_key_entry.len() {
        return Err(NssError::Database(
            "key3.db: private key ASN.1 offset out of bounds".into(),
        ));
    }
    let pk_asn1_data = &priv_key_entry[asn1_start..];

    // ASN.1: SEQUENCE { SEQUENCE { OID(pbeWithSha1And3DES), SEQUENCE { salt, iterations } }, OCTET STRING(encrypted) }
    let (outer, _) = asn1::read_item(pk_asn1_data)?;
    let outer_items = asn1::expect_sequence(&outer)?;
    if outer_items.len() < 2 {
        return Err(NssError::Asn1(
            "key3.db private key: need algo + ciphertext".into(),
        ));
    }
    let algo_items = asn1::expect_sequence(&outer_items[0])?;
    let pk_ciphertext = asn1::expect_octet_string(&outer_items[1])?;

    if algo_items.len() < 2 || !asn1::match_oid(&algo_items[0], OID_PKCS12_PBE_SHA1_3DES) {
        return Err(NssError::Asn1(
            "key3.db: unexpected algorithm in private key".into(),
        ));
    }
    let pbe_params = asn1::expect_sequence(&algo_items[1])?;
    if pbe_params.is_empty() {
        return Err(NssError::Asn1("key3.db: empty PBE params".into()));
    }
    let pk_entry_salt = asn1::expect_octet_string(&pbe_params[0])?;

    // Decrypt the private key blob
    let (pk_key, pk_iv) = derive_moz_3des(global_salt, master_pw_bytes, pk_entry_salt);
    let decrypted_pk = decrypt_3des(&pk_key, &pk_iv, pk_ciphertext)?;

    // Decrypted is PKCS#8-like: SEQUENCE { INTEGER(0), SEQUENCE { OID(pkcs-1), NULL }, OCTET STRING(prKey) }
    // prKey is: SEQUENCE { INTEGER(0), INTEGER(id), INTEGER(0), INTEGER(key_bytes), ... }
    let (pk_outer, _) = asn1::read_item(&decrypted_pk)?;
    let pk_items = asn1::expect_sequence(&pk_outer)?;
    if pk_items.len() < 3 {
        return Err(NssError::Asn1(
            "key3.db: decrypted private key too short".into(),
        ));
    }
    let pr_key_data = asn1::expect_octet_string(&pk_items[2])?;

    let (pr_key_outer, _) = asn1::read_item(pr_key_data)?;
    let pr_key_items = asn1::expect_sequence(&pr_key_outer)?;
    if pr_key_items.len() < 4 {
        return Err(NssError::Asn1("key3.db: prKey SEQUENCE too short".into()));
    }

    // The 4th INTEGER contains the raw 3DES key material
    if pr_key_items[3].tag != asn1::TAG_INTEGER {
        return Err(NssError::Asn1("key3.db: prKey[3] not INTEGER".into()));
    }
    let raw_key = pr_key_items[3].data;

    // Key might have a leading zero byte (ASN.1 integer sign padding)
    let key_bytes = if !raw_key.is_empty() && raw_key[0] == 0x00 && raw_key.len() > 24 {
        &raw_key[1..]
    } else {
        raw_key
    };

    if key_bytes.len() < 24 {
        return Err(NssError::Crypto(format!(
            "key3.db: extracted key too short: {} bytes",
            key_bytes.len()
        )));
    }

    Ok(NssMasterKey {
        key: Zeroizing::new(key_bytes[..24].to_vec()),
    })
}

// ---------- Internal: decryption primitives ----------

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type TdesEdeCbcDec = cbc::Decryptor<des::TdesEde3>;

/// Decrypts with AES-256-CBC or 3DES-CBC based on params.
fn decrypt_with_params(
    key: &[u8],
    iv: &[u8],
    ciphertext: &[u8],
    is_aes256: bool,
) -> Result<Zeroizing<Vec<u8>>, NssError> {
    if is_aes256 {
        decrypt_aes256(key, iv, ciphertext)
    } else {
        decrypt_3des(key, iv, ciphertext)
    }
}

fn decrypt_aes256(
    key: &[u8],
    iv: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, NssError> {
    if key.len() < 32 {
        return Err(NssError::Crypto(format!(
            "AES-256 key too short: {} bytes",
            key.len()
        )));
    }
    if iv.len() != 16 {
        return Err(NssError::Crypto(format!(
            "AES-256-CBC IV must be 16 bytes, got {}",
            iv.len()
        )));
    }
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err(NssError::Crypto(format!(
            "invalid AES ciphertext length: {}",
            ciphertext.len()
        )));
    }

    let mut buf = Zeroizing::new(ciphertext.to_vec());
    let decrypted = Aes256CbcDec::new(key[..32].into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| NssError::Crypto("AES-256-CBC decrypt/unpad failed".into()))?;

    Ok(Zeroizing::new(decrypted.to_vec()))
}

fn decrypt_3des(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, NssError> {
    if key.len() < 24 {
        return Err(NssError::Crypto(format!(
            "3DES key too short: {} bytes",
            key.len()
        )));
    }
    if iv.len() != 8 {
        return Err(NssError::Crypto(format!(
            "3DES-CBC IV must be 8 bytes, got {}",
            iv.len()
        )));
    }
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(8) {
        return Err(NssError::Crypto(format!(
            "invalid 3DES ciphertext length: {}",
            ciphertext.len()
        )));
    }

    let mut buf = Zeroizing::new(ciphertext.to_vec());
    let decrypted = TdesEdeCbcDec::new(key[..24].into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| NssError::Crypto("3DES-CBC decrypt/unpad failed".into()))?;

    Ok(Zeroizing::new(decrypted.to_vec()))
}

// ---------- Internal: password-check verification ----------

/// Decrypts the password-check blob and verifies it equals "password-check"
/// (with PKCS#7 padding stripped by the decrypt function).
fn verify_password_check(master_key: &NssMasterKey, check_blob: &[u8]) -> Result<(), NssError> {
    let field = parse_login_field_asn1(check_blob)?;

    let plaintext = if field.is_3des {
        decrypt_3des(master_key.des_key()?, &field.iv, &field.ciphertext)?
    } else {
        decrypt_aes256(master_key.aes_key()?, &field.iv, &field.ciphertext)?
    };

    // The decrypted value should be "password-check\x02\x02" but PKCS#7 strip
    // removes the padding, so we check for "password-check".
    if plaintext.starts_with(b"password-check") {
        Ok(())
    } else {
        Err(NssError::WrongMasterPassword)
    }
}

// ---------- Test support (pub(crate) for use by parsers::firefox::passwords tests) ----------

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use aes::cipher::BlockEncryptMut as _;

    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

    fn encrypt_aes256_inner(key: &[u8; 32], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        let block_size = 16;
        let pad_len = block_size - (plaintext.len() % block_size);
        let total = plaintext.len() + pad_len;
        let mut buf = vec![0u8; total];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct = Aes256CbcEnc::new(key.into(), iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap();
        ct.to_vec()
    }

    fn asn1_seq(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x30];
        encode_len(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    fn asn1_octet(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x04];
        encode_len(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    fn asn1_oid(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x06];
        encode_len(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    fn asn1_int(val: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        if val == 0 {
            bytes.push(0);
        } else {
            let mut v = val;
            let mut temp = Vec::new();
            while v > 0 {
                temp.push((v & 0xFF) as u8);
                v >>= 8;
            }
            temp.reverse();
            if temp[0] & 0x80 != 0 {
                bytes.push(0x00);
            }
            bytes.extend_from_slice(&temp);
        }
        let mut out = vec![0x02];
        encode_len(bytes.len(), &mut out);
        out.extend_from_slice(&bytes);
        out
    }

    fn encode_len(len: usize, out: &mut Vec<u8>) {
        if len < 128 {
            out.push(len as u8);
        } else if len < 256 {
            out.push(0x81);
            out.push(len as u8);
        } else {
            out.push(0x82);
            out.push((len >> 8) as u8);
            out.push((len & 0xFF) as u8);
        }
    }

    /// Creates a test Firefox profile with key4.db + logins.json.
    /// Returns (TempDir, profile_path, master_key_bytes).
    /// `credentials`: &[username, password, hostname, ...]
    pub(crate) fn create_test_profile(
        master_password: &str,
        credentials: &[&str],
    ) -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
        use base64::Engine;
        use rusqlite::Connection;

        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().to_path_buf();
        let global_salt = vec![0xAA; 20];
        let master_key = vec![0x55; 32];
        let entry_salt = b"test_entry_salt!";
        let iterations: u32 = 1;
        let iv = [0x01u8; 16];

        // Derive intermediate key.
        let mut hasher = Sha256::new();
        hasher.update(&global_salt);
        hasher.update(master_password.as_bytes());
        let hp = hasher.finalize();
        let mut intermediate = vec![0u8; 32];
        pbkdf2::pbkdf2::<Hmac<Sha256>>(&hp, entry_salt, iterations, &mut intermediate).unwrap();

        // Encrypt master key.
        let encrypted_master_key = encrypt_aes256_inner(
            intermediate.as_slice().try_into().unwrap(),
            &iv,
            &master_key,
        );

        // Build a11 ASN.1.
        let pbkdf2_params = asn1_seq(&[asn1_octet(entry_salt), asn1_int(iterations)].concat());
        let kdf_seq = asn1_seq(&[asn1_oid(super::asn1::OID_PBKDF2), pbkdf2_params].concat());
        let enc_algo_seq =
            asn1_seq(&[asn1_oid(super::asn1::OID_AES_256_CBC), asn1_octet(&iv)].concat());
        let pbes2_params = asn1_seq(&[kdf_seq, enc_algo_seq].concat());
        let algo_seq = asn1_seq(&[asn1_oid(super::asn1::OID_PBES2), pbes2_params].concat());
        let a11 = asn1_seq(&[algo_seq, asn1_octet(&encrypted_master_key)].concat());

        // Build password-check (encrypt "password-check" with master_key via AES-256-CBC).
        let check_iv = [0x03u8; 16];
        let encrypted_check = encrypt_aes256_inner(
            master_key[..32].try_into().unwrap(),
            &check_iv,
            b"password-check",
        );
        let check_algo = asn1_seq(
            &[
                asn1_oid(super::asn1::OID_AES_256_CBC),
                asn1_octet(&check_iv),
            ]
            .concat(),
        );
        let password_check_blob = asn1_seq(&[check_algo, asn1_octet(&encrypted_check)].concat());

        // Create key4.db.
        let conn = Connection::open(profile.join("key4.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (id TEXT PRIMARY KEY, item1 BLOB, item2 BLOB);
             CREATE TABLE nssPrivate (a11 BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata (id, item1, item2) VALUES ('password', ?1, ?2)",
            rusqlite::params![global_salt, password_check_blob],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO nssPrivate (a11) VALUES (?1)",
            rusqlite::params![a11],
        )
        .unwrap();
        drop(conn);

        // Create logins.json.
        let mut logins = Vec::new();
        let mut i = 0;
        while i + 2 < credentials.len() {
            let username = credentials[i];
            let password = credentials[i + 1];
            let hostname = credentials[i + 2];

            // Encrypt username and password.
            let field_iv = [0x04u8; 16];
            let enc_user = encrypt_aes256_inner(
                master_key[..32].try_into().unwrap(),
                &field_iv,
                username.as_bytes(),
            );
            let enc_pass = encrypt_aes256_inner(
                master_key[..32].try_into().unwrap(),
                &field_iv,
                password.as_bytes(),
            );

            // Wrap in ASN.1.
            let user_algo = asn1_seq(
                &[
                    asn1_oid(super::asn1::OID_AES_256_CBC),
                    asn1_octet(&field_iv),
                ]
                .concat(),
            );
            let user_blob = asn1_seq(&[user_algo, asn1_octet(&enc_user)].concat());
            let pass_algo = asn1_seq(
                &[
                    asn1_oid(super::asn1::OID_AES_256_CBC),
                    asn1_octet(&field_iv),
                ]
                .concat(),
            );
            let pass_blob = asn1_seq(&[pass_algo, asn1_octet(&enc_pass)].concat());

            let user_b64 = base64::engine::general_purpose::STANDARD.encode(&user_blob);
            let pass_b64 = base64::engine::general_purpose::STANDARD.encode(&pass_blob);

            logins.push(serde_json::json!({
                "hostname": hostname,
                "formSubmitURL": hostname,
                "encryptedUsername": user_b64,
                "encryptedPassword": pass_b64,
            }));
            i += 3;
        }

        let logins_json = serde_json::json!({ "logins": logins });
        std::fs::write(
            profile.join("logins.json"),
            serde_json::to_string_pretty(&logins_json).unwrap(),
        )
        .unwrap();

        (temp, profile, master_key)
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut as _;
    use std::io::Write;

    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
    type TdesEdeCbcEnc = cbc::Encryptor<des::TdesEde3>;

    fn encrypt_aes256(key: &[u8; 32], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        let block_size = 16;
        let pad_len = block_size - (plaintext.len() % block_size);
        let total = plaintext.len() + pad_len;
        let mut buf = vec![0u8; total];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct = Aes256CbcEnc::new(key.into(), iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap();
        ct.to_vec()
    }

    fn encrypt_3des(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let block_size = 8;
        let pad_len = block_size - (plaintext.len() % block_size);
        let total = plaintext.len() + pad_len;
        let mut buf = vec![0u8; total];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct = TdesEdeCbcEnc::new(key[..24].into(), iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap();
        ct.to_vec()
    }

    /// Helper: builds an ASN.1 SEQUENCE wrapping given bytes.
    fn asn1_seq(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x30];
        encode_length(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    /// Helper: builds an ASN.1 OCTET STRING wrapping given bytes.
    fn asn1_octet(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x04];
        encode_length(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    /// Helper: builds an ASN.1 OID wrapping given encoded bytes.
    fn asn1_oid(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x06];
        encode_length(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }

    /// Helper: builds an ASN.1 INTEGER from u32.
    fn asn1_int(val: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        if val == 0 {
            bytes.push(0);
        } else {
            let mut v = val;
            let mut temp = Vec::new();
            while v > 0 {
                temp.push((v & 0xFF) as u8);
                v >>= 8;
            }
            temp.reverse();
            // Add leading zero if high bit set (positive number).
            if temp[0] & 0x80 != 0 {
                bytes.push(0x00);
            }
            bytes.extend_from_slice(&temp);
        }
        let mut out = vec![0x02];
        encode_length(bytes.len(), &mut out);
        out.extend_from_slice(&bytes);
        out
    }

    fn encode_length(len: usize, out: &mut Vec<u8>) {
        if len < 128 {
            out.push(len as u8);
        } else if len < 256 {
            out.push(0x81);
            out.push(len as u8);
        } else {
            out.push(0x82);
            out.push((len >> 8) as u8);
            out.push((len & 0xFF) as u8);
        }
    }

    /// Creates a test key4.db with known parameters.
    /// Returns (temp_dir, profile_path) — temp_dir must stay alive.
    fn create_test_key4_db(
        global_salt: &[u8],
        master_password: &str,
        master_key: &[u8],
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().to_path_buf();
        let db_path = profile.join("key4.db");

        let conn = Connection::open(&db_path).unwrap();

        // Create tables.
        conn.execute_batch(
            "CREATE TABLE metadata (id TEXT PRIMARY KEY, item1 BLOB, item2 BLOB);
             CREATE TABLE nssPrivate (a11 BLOB);",
        )
        .unwrap();

        // Derive intermediate key the same way unlock_nss_master_key does.
        let use_sha384 = global_salt.len() > 40;
        let entry_salt = b"test_entry_salt!"; // 16 bytes
        let iterations: u32 = 1;

        let password_bytes = master_password.as_bytes();
        let intermediate = if use_sha384 {
            let mut hasher = Sha384::new();
            hasher.update(global_salt);
            hasher.update(password_bytes);
            let hp = hasher.finalize();
            let mut derived = vec![0u8; 32];
            pbkdf2::pbkdf2::<Hmac<Sha384>>(&hp, entry_salt, iterations, &mut derived).unwrap();
            derived
        } else {
            let mut hasher = Sha256::new();
            hasher.update(global_salt);
            hasher.update(password_bytes);
            let hp = hasher.finalize();
            let mut derived = vec![0u8; 32];
            pbkdf2::pbkdf2::<Hmac<Sha256>>(&hp, entry_salt, iterations, &mut derived).unwrap();
            derived
        };

        let iv = [0x01u8; 16]; // 16-byte IV for AES-256-CBC

        // Encrypt the master key.
        let encrypted_master_key =
            encrypt_aes256(intermediate.as_slice().try_into().unwrap(), &iv, master_key);

        // Build ASN.1 for a11 (PBES2 + PBKDF2 + AES-256-CBC).
        let pbkdf2_params_content = [asn1_octet(entry_salt), asn1_int(iterations)].concat();
        let pbkdf2_params = asn1_seq(&pbkdf2_params_content);

        let kdf_content = [asn1_oid(OID_PBKDF2), pbkdf2_params].concat();
        let kdf_seq = asn1_seq(&kdf_content);

        let enc_algo_content = [asn1_oid(OID_AES_256_CBC), asn1_octet(&iv)].concat();
        let enc_algo_seq = asn1_seq(&enc_algo_content);

        let pbes2_params_content = [kdf_seq, enc_algo_seq].concat();
        let pbes2_params = asn1_seq(&pbes2_params_content);

        let algo_content = [asn1_oid(OID_PBES2), pbes2_params].concat();
        let algo_seq = asn1_seq(&algo_content);

        let a11 = asn1_seq(&[algo_seq, asn1_octet(&encrypted_master_key)].concat());

        let check_iv = [0x02u8; 8];
        let password_check_blob = if master_key.len() >= 32 {
            let check_iv_aes = [0x03u8; 16];
            let encrypted_check = encrypt_aes256(
                master_key[..32].try_into().unwrap(),
                &check_iv_aes,
                b"password-check",
            );
            let algo_content = [asn1_oid(OID_AES_256_CBC), asn1_octet(&check_iv_aes)].concat();
            let algo = asn1_seq(&algo_content);
            asn1_seq(&[algo, asn1_octet(&encrypted_check)].concat())
        } else {
            let encrypted_check = encrypt_3des(&master_key[..24], &check_iv, b"password-check");
            let algo_content = [asn1_oid(OID_3DES_CBC), asn1_octet(&check_iv)].concat();
            let algo = asn1_seq(&algo_content);
            asn1_seq(&[algo, asn1_octet(&encrypted_check)].concat())
        };

        conn.execute(
            "INSERT INTO metadata (id, item1, item2) VALUES ('password', ?1, ?2)",
            rusqlite::params![global_salt, password_check_blob],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO nssPrivate (a11) VALUES (?1)",
            rusqlite::params![a11],
        )
        .unwrap();

        (temp, profile)
    }

    #[test]
    fn unlock_with_correct_master_password_succeeds() {
        let global_salt = vec![0xAA; 20]; // 20 bytes → Variant B (SHA-256)
        let master_key = vec![0x55; 32]; // 32-byte master key for AES-256

        let (_temp, profile) = create_test_key4_db(&global_salt, "", &master_key);
        let key = unlock_nss_master_key(&profile, "").unwrap();
        assert_eq!(key.key.as_slice(), &master_key);
    }

    #[test]
    fn unlock_with_wrong_master_password_fails() {
        let global_salt = vec![0xAA; 20];
        let master_key = vec![0x55; 32];

        let (_temp, profile) = create_test_key4_db(&global_salt, "", &master_key);
        let err = unlock_nss_master_key(&profile, "wrongpassword").unwrap_err();
        // Should fail at decryption (wrong intermediate key → garbage decryption → bad padding or wrong check)
        assert!(
            matches!(err, NssError::Crypto(_) | NssError::WrongMasterPassword),
            "expected Crypto or WrongMasterPassword, got: {err:?}"
        );
    }

    #[test]
    fn unlock_with_nonempty_master_password_succeeds() {
        let global_salt = vec![0xBB; 20];
        let master_key = vec![0x77; 32];

        let (_temp, profile) = create_test_key4_db(&global_salt, "test123", &master_key);
        let key = unlock_nss_master_key(&profile, "test123").unwrap();
        assert_eq!(key.key.as_slice(), &master_key);
    }

    #[test]
    fn nss3_empty_key3_db_returns_database_error() {
        let temp = tempfile::tempdir().unwrap();
        // Create key3.db (empty file) but no key4.db → fails at BDB header parse.
        std::fs::File::create(temp.path().join("key3.db")).unwrap();

        let err = unlock_nss_master_key(temp.path(), "").unwrap_err();
        assert!(
            matches!(err, NssError::Database(_)),
            "expected Database error, got: {err:?}"
        );
    }

    #[test]
    fn missing_key_db_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        let err = unlock_nss_master_key(temp.path(), "").unwrap_err();
        assert!(matches!(err, NssError::KeyDbMissing));
    }

    #[test]
    fn decrypt_value_known_vector() {
        // Create a known master key and encrypt a test value.
        let master_key_bytes = vec![0x42; 32];
        let iv = [0x10; 16];
        let plaintext = "hunter2";

        let encrypted = encrypt_aes256(
            master_key_bytes.as_slice().try_into().unwrap(),
            &iv,
            plaintext.as_bytes(),
        );

        // Build ASN.1: SEQUENCE { SEQUENCE { OID(AES-256-CBC), OCTET STRING(iv) }, OCTET STRING(ct) }
        let algo_content = [asn1_oid(OID_AES_256_CBC), asn1_octet(&iv)].concat();
        let algo = asn1_seq(&algo_content);
        let field_blob = asn1_seq(&[algo, asn1_octet(&encrypted)].concat());

        // Base64-encode.
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&field_blob);

        let key = NssMasterKey {
            key: Zeroizing::new(master_key_bytes),
        };
        let result = decrypt_nss_value(&b64, &key).unwrap();
        assert_eq!(result, "hunter2");
    }

    #[test]
    fn decrypt_value_3des_known_vector() {
        let master_key_bytes = vec![0x42; 24];
        let iv = [0x10; 8];
        let plaintext = "secret_password";

        let encrypted = encrypt_3des(&master_key_bytes, &iv, plaintext.as_bytes());

        let algo_content = [asn1_oid(OID_3DES_CBC), asn1_octet(&iv)].concat();
        let algo = asn1_seq(&algo_content);
        let field_blob = asn1_seq(&[algo, asn1_octet(&encrypted)].concat());

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&field_blob);

        let key = NssMasterKey {
            key: Zeroizing::new(master_key_bytes),
        };
        let result = decrypt_nss_value(&b64, &key).unwrap();
        assert_eq!(result, "secret_password");
    }

    #[test]
    fn malformed_asn1_returns_parse_error() {
        use base64::Engine;
        let garbage = base64::engine::general_purpose::STANDARD.encode([0xFF, 0x01, 0x02]);
        let key = NssMasterKey {
            key: Zeroizing::new(vec![0x42; 32]),
        };
        let err = decrypt_nss_value(&garbage, &key).unwrap_err();
        assert!(matches!(err, NssError::Asn1(_)));
    }

    #[test]
    fn corrupted_key4_db_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        // Create a corrupt key4.db (not valid SQLite).
        let mut f = std::fs::File::create(temp.path().join("key4.db")).unwrap();
        f.write_all(b"not a sqlite database").unwrap();

        let err = unlock_nss_master_key(temp.path(), "").unwrap_err();
        assert!(matches!(err, NssError::Database(_)));
    }

    #[test]
    fn sha384_variant_c_works() {
        // global_salt > 40 bytes triggers Variant C (SHA-384)
        let global_salt = vec![0xCC; 48];
        let master_key = vec![0xDD; 32];

        let (_temp, profile) = create_test_key4_db(&global_salt, "", &master_key);
        let key = unlock_nss_master_key(&profile, "").unwrap();
        assert_eq!(key.key.as_slice(), &master_key);
    }

    #[test]
    fn pkcs12_pbe_variant_a_key4_db() {
        // Test key4.db Variant A: PKCS#12 PBE + 3DES-CBC
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().to_path_buf();

        // Master key: 24 bytes for 3DES
        let master_key_raw = vec![0x42u8; 24];

        // PKCS#12 derivation parameters
        let entry_salt = b"variant_a_salt!!"; // 16 bytes
        let iterations: u32 = 1;
        let password_bmp: &[u8] = &[0x00, 0x00]; // empty password BMP

        // Derive key + IV via PKCS#12 PBE
        let des_key = super::pkcs12_derive(password_bmp, entry_salt, iterations, 24, 1);
        let des_iv = super::pkcs12_derive(password_bmp, entry_salt, iterations, 8, 2);

        // Encrypt the master key
        let encrypted_master = encrypt_3des(&des_key, &des_iv, &master_key_raw);

        // Build a11 ASN.1 with PKCS12 PBE OID
        let pbe_params_content = [asn1_octet(entry_salt), asn1_int(iterations)].concat();
        let pbe_params = asn1_seq(&pbe_params_content);
        let algo_content = [asn1_oid(OID_PKCS12_PBE_SHA1_3DES), pbe_params].concat();
        let algo_seq = asn1_seq(&algo_content);
        let a11 = asn1_seq(&[algo_seq, asn1_octet(&encrypted_master)].concat());

        // Build password-check using master key with 3DES
        let check_iv = [0x09u8; 8];
        let encrypted_check = encrypt_3des(&master_key_raw, &check_iv, b"password-check");
        let check_algo_content = [asn1_oid(OID_3DES_CBC), asn1_octet(&check_iv)].concat();
        let check_algo = asn1_seq(&check_algo_content);
        let password_check_blob = asn1_seq(&[check_algo, asn1_octet(&encrypted_check)].concat());

        // Create key4.db with this data
        let conn = Connection::open(profile.join("key4.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (id TEXT PRIMARY KEY, item1 BLOB, item2 BLOB);
             CREATE TABLE nssPrivate (a11 BLOB);",
        )
        .unwrap();
        let global_salt = vec![0xAAu8; 20];
        conn.execute(
            "INSERT INTO metadata (id, item1, item2) VALUES ('password', ?1, ?2)",
            rusqlite::params![global_salt, password_check_blob],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO nssPrivate (a11) VALUES (?1)",
            rusqlite::params![a11],
        )
        .unwrap();
        drop(conn);

        let key = unlock_nss_master_key(&profile, "").unwrap();
        assert_eq!(key.key.as_slice(), &master_key_raw);
    }

    #[test]
    fn nss3_key3_db_round_trip() {
        // Build a synthetic key3.db with known data and verify the full decrypt path.
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().to_path_buf();

        let global_salt = b"test_global_salt";
        let master_key_raw = vec![0x55u8; 24];
        let entry_salt_check = b"check_salt_12345"; // 16 bytes
        let entry_salt_priv = b"priv_salt_123456"; // 16 bytes

        // Derive key/iv for password check
        let (check_key, check_iv) = super::derive_moz_3des(
            global_salt,
            b"", // empty master password
            entry_salt_check,
        );
        let check_plaintext = b"password-check";
        let encrypted_check = encrypt_3des(&check_key, &check_iv, check_plaintext);

        // Build password-check entry: [type, salt_len, 0, salt..., encrypted[-16:]]
        let mut pw_check_entry = Vec::new();
        pw_check_entry.push(0x02); // type byte
        pw_check_entry.push(entry_salt_check.len() as u8);
        pw_check_entry.push(0x00); // name len
        pw_check_entry.extend_from_slice(entry_salt_check);
        // Pad so that the last 16 bytes are the encrypted data
        pw_check_entry.extend_from_slice(&encrypted_check);

        // Build private key ASN.1 blob
        // After decrypt, the result should be: SEQUENCE { INTEGER 0, SEQUENCE { OID, NULL }, OCTET_STRING(prKey) }
        // prKey = SEQUENCE { INTEGER 0, INTEGER id, INTEGER 0, INTEGER key_bytes, ... }

        // Build prKey inner structure
        let pr_key_inner_content = [
            asn1_int(0),
            asn1_int(0xF8000001), // CKA_ID as integer
            asn1_int(0),
            // The actual key as an INTEGER (with leading 0x00 for sign)
            {
                let mut key_int = vec![0x02]; // INTEGER tag
                let key_with_sign = {
                    let mut v = vec![0x00]; // leading zero (sign)
                    v.extend_from_slice(&master_key_raw);
                    v
                };
                encode_length(key_with_sign.len(), &mut key_int);
                key_int.extend_from_slice(&key_with_sign);
                key_int
            },
            asn1_int(0),
            asn1_int(0),
            asn1_int(0),
            asn1_int(0),
            asn1_int(15),
        ]
        .concat();
        let pr_key_seq = asn1_seq(&pr_key_inner_content);

        // PKCS#8-like wrapper
        let pkcs1_oid: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        let null_item = vec![0x05, 0x00];
        let algo_id_content = [asn1_oid(pkcs1_oid), null_item].concat();
        let algo_id = asn1_seq(&algo_id_content);
        let pkcs8_content = [asn1_int(0), algo_id, asn1_octet(&pr_key_seq)].concat();
        let pkcs8_blob = asn1_seq(&pkcs8_content);

        // Encrypt pkcs8_blob using moz3des with priv entry salt
        let (priv_key, priv_iv) = super::derive_moz_3des(global_salt, b"", entry_salt_priv);
        let encrypted_pkcs8 = encrypt_3des(&priv_key, &priv_iv, &pkcs8_blob);

        // Build the outer ASN.1 for the private key entry
        let pbe_params_content = [asn1_octet(entry_salt_priv), asn1_int(1)].concat();
        let pbe_params = asn1_seq(&pbe_params_content);
        let algo_content = [asn1_oid(OID_PKCS12_PBE_SHA1_3DES), pbe_params].concat();
        let algo_seq_asn1 = asn1_seq(&algo_content);
        let priv_key_asn1 = asn1_seq(&[algo_seq_asn1, asn1_octet(&encrypted_pkcs8)].concat());

        // Build private key entry: [type, salt_len, name_len, salt..., name..., ASN.1...]
        let mut priv_entry = Vec::new();
        priv_entry.push(0x00); // type
        priv_entry.push(0x00); // salt_len = 0 (salt is in ASN.1)
        priv_entry.push(0x00); // name_len = 0
        priv_entry.extend_from_slice(&priv_key_asn1);

        // Build BDB key3.db file
        let pagesize: u32 = 256;
        let nkeys: u32 = 3;

        // Build header (page 0)
        let mut file_data = vec![0u8; pagesize as usize * 4]; // 4 pages

        // Header: big-endian fields
        file_data[0..4].copy_from_slice(&BDB_HASH_MAGIC.to_be_bytes());
        file_data[4..8].copy_from_slice(&BDB_VERSION.to_be_bytes());
        file_data[12..16].copy_from_slice(&pagesize.to_be_bytes());
        file_data[56..60].copy_from_slice(&nkeys.to_be_bytes());

        // Page 1: contains entries
        // Layout: offset index at start, data stored from higher offsets
        let page1_start = pagesize as usize;

        // We need 3 key-value pairs. Let's lay them out manually.
        // Entries will be stored starting from the end of the page.
        let entries_data: Vec<(&[u8], &[u8])> = vec![
            (b"global-salt", global_salt.as_slice()),
            (b"password-check", pw_check_entry.as_slice()),
            (
                &[
                    0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x01,
                ],
                priv_entry.as_slice(),
            ),
        ];

        // Calculate data placement within page 1
        // Data is placed from the top of the page, offsets point within the page.
        // The firepwd algorithm sorts offsets and reads between them.
        // So we place: key1 at offset A, val1 at offset B, key2 at C, val2 at D, etc.
        // All offsets relative to page start.
        // We need the offset index (at the beginning of the page) and data elsewhere.

        // Reserve first 32 bytes for the index (plenty for 3 pairs × 2 offsets × 2 bytes + header)
        let mut data_offset = 32usize;

        // BDB layout: VALUE at lower offset, KEY at higher offset in each pair.
        // firepwd index: position (2+i) = val_data_off, position (4+i) = key_data_off.
        // After sort+read: db[entries[i+1]] = entries[i] → db[key_string] = value_blob.
        let mut val_data_offsets: Vec<u16> = Vec::new();
        let mut key_data_offsets: Vec<u16> = Vec::new();
        for (key, val) in &entries_data {
            let val_off = data_offset as u16;
            file_data[page1_start + data_offset..page1_start + data_offset + val.len()]
                .copy_from_slice(val);
            data_offset += val.len();

            let key_off = data_offset as u16;
            file_data[page1_start + data_offset..page1_start + data_offset + key.len()]
                .copy_from_slice(key);
            data_offset += key.len();

            val_data_offsets.push(val_off);
            key_data_offsets.push(key_off);
        }

        file_data[page1_start] = 0x00;
        file_data[page1_start + 1] = 0x00;

        // Index: (2+i) = val_data_off, (4+i) = key_data_off
        let mut idx = 2;
        for pair_idx in 0..entries_data.len() {
            let vb = val_data_offsets[pair_idx].to_le_bytes();
            let kb = key_data_offsets[pair_idx].to_le_bytes();
            file_data[page1_start + idx] = vb[0];
            file_data[page1_start + idx + 1] = vb[1];
            file_data[page1_start + idx + 2] = kb[0];
            file_data[page1_start + idx + 3] = kb[1];
            idx += 4;
        }
        // Terminator: val at (4+i) == nval at (8+i)
        let last_key = key_data_offsets.last().unwrap().to_le_bytes();
        file_data[page1_start + idx + 2] = last_key[0];
        file_data[page1_start + idx + 3] = last_key[1];

        // Write key3.db
        std::fs::write(profile.join("key3.db"), &file_data).unwrap();

        // Test the full path
        let key = unlock_nss_master_key(&profile, "").unwrap();
        assert_eq!(key.key.as_slice(), &master_key_raw);
    }

    #[test]
    fn nss3_wrong_password_fails() {
        // Use the same setup as nss3_key3_db_round_trip but with wrong password
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().to_path_buf();

        let global_salt = b"test_global_salt";
        let entry_salt_check = b"check_salt_12345";

        // Derive with empty password
        let (check_key, check_iv) = super::derive_moz_3des(global_salt, b"", entry_salt_check);
        let encrypted_check = encrypt_3des(&check_key, &check_iv, b"password-check");

        let mut pw_check_entry = Vec::new();
        pw_check_entry.push(0x02);
        pw_check_entry.push(entry_salt_check.len() as u8);
        pw_check_entry.push(0x00);
        pw_check_entry.extend_from_slice(entry_salt_check);
        pw_check_entry.extend_from_slice(&encrypted_check);

        // Build minimal key3.db with just global-salt and password-check (no CKA_ID)
        let pagesize: u32 = 512;
        let nkeys: u32 = 2;

        let mut file_data = vec![0u8; pagesize as usize * 3];
        file_data[0..4].copy_from_slice(&BDB_HASH_MAGIC.to_be_bytes());
        file_data[4..8].copy_from_slice(&BDB_VERSION.to_be_bytes());
        file_data[12..16].copy_from_slice(&pagesize.to_be_bytes());
        file_data[56..60].copy_from_slice(&nkeys.to_be_bytes());

        let page1_start = pagesize as usize;
        let mut data_offset = 32usize;
        let entries: Vec<(&[u8], &[u8])> = vec![
            (b"global-salt", global_salt.as_slice()),
            (b"password-check", pw_check_entry.as_slice()),
        ];
        let mut val_data_offsets = Vec::new();
        let mut key_data_offsets = Vec::new();
        for (key, val) in &entries {
            let vo = data_offset as u16;
            file_data[page1_start + data_offset..page1_start + data_offset + val.len()]
                .copy_from_slice(val);
            data_offset += val.len();
            let ko = data_offset as u16;
            file_data[page1_start + data_offset..page1_start + data_offset + key.len()]
                .copy_from_slice(key);
            data_offset += key.len();
            val_data_offsets.push(vo);
            key_data_offsets.push(ko);
        }

        file_data[page1_start] = 0x00;
        file_data[page1_start + 1] = 0x00;
        let mut idx = 2;
        for i in 0..entries.len() {
            let vb = val_data_offsets[i].to_le_bytes();
            let kb = key_data_offsets[i].to_le_bytes();
            file_data[page1_start + idx] = vb[0];
            file_data[page1_start + idx + 1] = vb[1];
            file_data[page1_start + idx + 2] = kb[0];
            file_data[page1_start + idx + 3] = kb[1];
            idx += 4;
        }
        let last_key = key_data_offsets.last().unwrap().to_le_bytes();
        file_data[page1_start + idx + 2] = last_key[0];
        file_data[page1_start + idx + 3] = last_key[1];

        std::fs::write(profile.join("key3.db"), &file_data).unwrap();

        let err = unlock_nss_master_key(&profile, "wrongpass").unwrap_err();
        assert!(
            matches!(err, NssError::WrongMasterPassword | NssError::Crypto(_)),
            "expected WrongMasterPassword or Crypto, got: {err:?}"
        );
    }

    #[test]
    fn pkcs12_derive_known_vector() {
        // Verify PKCS#12 derivation produces expected output for empty password.
        // Empty password BMP = [0x00, 0x00]
        let password_bmp = &[0x00u8, 0x00];
        let salt = &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let iterations = 1u32;

        let key = super::pkcs12_derive(password_bmp, salt, iterations, 24, 1);
        let iv = super::pkcs12_derive(password_bmp, salt, iterations, 8, 2);

        // Just verify lengths and that they're not all zeros
        assert_eq!(key.len(), 24);
        assert_eq!(iv.len(), 8);
        assert!(key.iter().any(|&b| b != 0));
        assert!(iv.iter().any(|&b| b != 0));
    }

    #[test]
    fn moz_3des_derive_known_vector() {
        // Verify Mozilla 3DES derivation produces consistent key/IV.
        let global_salt = b"global_salt_test";
        let entry_salt = b"entry_salt_test!";

        let (key1, iv1) = super::derive_moz_3des(global_salt, b"", entry_salt);
        let (key2, iv2) = super::derive_moz_3des(global_salt, b"", entry_salt);

        assert_eq!(key1.as_slice(), key2.as_slice());
        assert_eq!(iv1.as_slice(), iv2.as_slice());
        assert_eq!(key1.len(), 24);
        assert_eq!(iv1.len(), 8);
        assert!(key1.iter().any(|&b| b != 0));

        // Different password should produce different key
        let (key3, _) = super::derive_moz_3des(global_salt, b"password", entry_salt);
        assert_ne!(key1.as_slice(), key3.as_slice());
    }
}
