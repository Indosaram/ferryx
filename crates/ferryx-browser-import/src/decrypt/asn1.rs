//! Minimal ASN.1 BER/DER decoder for NSS key database structures.
//!
//! Handles the subset of ASN.1 types needed for PKCS#5 / PBES2 / PBKDF2
//! parameter parsing in Firefox `key4.db` and encrypted login fields.
//! Does NOT support indefinite-length encoding or constructed OCTET STRING.

/// ASN.1 tag constants.
pub const TAG_INTEGER: u8 = 0x02;
pub const TAG_OCTET_STRING: u8 = 0x04;
pub const TAG_OID: u8 = 0x06;
pub const TAG_SEQUENCE: u8 = 0x30;

/// A parsed ASN.1 TLV (tag-length-value) item referencing the original buffer.
#[derive(Clone, Debug)]
pub struct Asn1Item<'a> {
    pub tag: u8,
    pub data: &'a [u8],
}

/// Error from ASN.1 parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asn1Error(pub String);

impl std::fmt::Display for Asn1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ASN.1: {}", self.0)
    }
}

impl std::error::Error for Asn1Error {}

/// Reads a single ASN.1 TLV item from the front of `data`.
/// Returns the item and the number of bytes consumed.
pub fn read_item(data: &[u8]) -> Result<(Asn1Item<'_>, usize), Asn1Error> {
    if data.len() < 2 {
        return Err(Asn1Error("truncated: need at least tag + length".into()));
    }

    let tag = data[0];
    let (content_len, header_len) = read_length(&data[1..])?;
    let total_len = header_len + 1 + content_len; // +1 for tag byte

    if data.len() < total_len {
        return Err(Asn1Error(format!(
            "truncated: need {} bytes, have {}",
            total_len,
            data.len()
        )));
    }

    let content_start = header_len + 1;
    let item = Asn1Item {
        tag,
        data: &data[content_start..content_start + content_len],
    };

    Ok((item, total_len))
}

/// Reads all TLV items sequentially from a buffer (e.g., inside a SEQUENCE).
pub fn read_items(mut data: &[u8]) -> Result<Vec<Asn1Item<'_>>, Asn1Error> {
    let mut items = Vec::new();
    while !data.is_empty() {
        let (item, consumed) = read_item(data)?;
        items.push(item);
        data = &data[consumed..];
    }
    Ok(items)
}

/// Parses the length field of a BER/DER encoding.
/// Returns (content_length, number_of_bytes_consumed_for_length).
fn read_length(data: &[u8]) -> Result<(usize, usize), Asn1Error> {
    if data.is_empty() {
        return Err(Asn1Error("truncated length".into()));
    }

    let first = data[0];

    if first & 0x80 == 0 {
        // Short form: length is directly in the byte.
        Ok((first as usize, 1))
    } else if first == 0x80 {
        // Indefinite length — not supported.
        Err(Asn1Error("indefinite length not supported".into()))
    } else {
        // Long form: low 7 bits = number of subsequent length bytes.
        let num_bytes = (first & 0x7F) as usize;
        if num_bytes > 4 {
            return Err(Asn1Error(format!(
                "length field too large: {num_bytes} bytes"
            )));
        }
        if data.len() < 1 + num_bytes {
            return Err(Asn1Error("truncated multi-byte length".into()));
        }

        let mut length: usize = 0;
        for i in 0..num_bytes {
            length = length
                .checked_shl(8)
                .ok_or_else(|| Asn1Error("length overflow".into()))?;
            length |= data[1 + i] as usize;
        }

        Ok((length, 1 + num_bytes))
    }
}

/// Parses an ASN.1 INTEGER as a `u32`. Only supports non-negative values up to 4 bytes.
pub fn parse_integer_u32(item: &Asn1Item<'_>) -> Result<u32, Asn1Error> {
    if item.tag != TAG_INTEGER {
        return Err(Asn1Error(format!(
            "expected INTEGER (0x02), got 0x{:02x}",
            item.tag
        )));
    }

    let data = item.data;
    if data.is_empty() || data.len() > 5 {
        return Err(Asn1Error(format!(
            "integer length out of range: {}",
            data.len()
        )));
    }

    // Skip leading zero byte (sign padding for positive values).
    let trimmed = if data.len() > 1 && data[0] == 0x00 {
        &data[1..]
    } else {
        data
    };

    if trimmed.len() > 4 {
        return Err(Asn1Error("integer too large for u32".into()));
    }

    let mut val: u32 = 0;
    for &byte in trimmed {
        val = (val << 8) | u32::from(byte);
    }
    Ok(val)
}

/// Checks that an item is an OID and its raw encoding matches `expected`.
pub fn match_oid(item: &Asn1Item<'_>, expected: &[u8]) -> bool {
    item.tag == TAG_OID && item.data == expected
}

/// Expects a SEQUENCE tag and returns items parsed from its content.
pub fn expect_sequence<'a>(item: &Asn1Item<'a>) -> Result<Vec<Asn1Item<'a>>, Asn1Error> {
    if item.tag != TAG_SEQUENCE {
        return Err(Asn1Error(format!(
            "expected SEQUENCE (0x30), got 0x{:02x}",
            item.tag
        )));
    }
    read_items(item.data)
}

/// Expects an OCTET STRING tag and returns the raw bytes.
pub fn expect_octet_string<'a>(item: &Asn1Item<'a>) -> Result<&'a [u8], Asn1Error> {
    if item.tag != TAG_OCTET_STRING {
        return Err(Asn1Error(format!(
            "expected OCTET STRING (0x04), got 0x{:02x}",
            item.tag
        )));
    }
    Ok(item.data)
}

// ---------- Well-known OID encodings ----------

/// PKCS#5 PBES2: 1.2.840.113549.1.5.13
pub const OID_PBES2: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x05, 0x0D];

/// PKCS#5 PBKDF2: 1.2.840.113549.1.5.12
pub const OID_PBKDF2: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x05, 0x0C];

/// 3DES-CBC: 1.2.840.113549.3.7
pub const OID_3DES_CBC: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x03, 0x07];

/// AES-256-CBC: 2.16.840.1.101.3.4.1.42
pub const OID_AES_256_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2A];

/// HMAC-SHA1: 1.2.840.113549.2.7
pub const OID_HMAC_SHA1: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x02, 0x07];

/// HMAC-SHA256: 1.2.840.113549.2.9
pub const OID_HMAC_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x02, 0x09];

/// PKCS#12 PBE SHA-1 + 3DES: 1.2.840.113549.1.12.5.1.3
pub const OID_PKCS12_PBE_SHA1_3DES: &[u8] = &[
    0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x0C, 0x05, 0x01, 0x03,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_short_length() {
        // SEQUENCE { OCTET STRING "hello" }
        let data: &[u8] = &[
            0x30, 0x07, // SEQUENCE, length 7
            0x04, 0x05, b'h', b'e', b'l', b'l', b'o', // OCTET STRING "hello"
        ];
        let (item, consumed) = read_item(data).unwrap();
        assert_eq!(consumed, 9);
        assert_eq!(item.tag, TAG_SEQUENCE);
        assert_eq!(item.data.len(), 7);

        let children = read_items(item.data).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].tag, TAG_OCTET_STRING);
        assert_eq!(children[0].data, b"hello");
    }

    #[test]
    fn read_long_length() {
        // Construct a 200-byte OCTET STRING with 2-byte length
        let mut data = vec![0x04, 0x81, 200]; // tag + long-form length (1 byte = 200)
        data.extend_from_slice(&[0xAB; 200]);

        let (item, consumed) = read_item(&data).unwrap();
        assert_eq!(consumed, 203);
        assert_eq!(item.tag, TAG_OCTET_STRING);
        assert_eq!(item.data.len(), 200);
    }

    #[test]
    fn parse_integer_simple() {
        let item = Asn1Item {
            tag: TAG_INTEGER,
            data: &[0x00, 0x01, 0x00], // = 256 with leading zero
        };
        assert_eq!(parse_integer_u32(&item).unwrap(), 256);
    }

    #[test]
    fn parse_integer_single_byte() {
        let item = Asn1Item {
            tag: TAG_INTEGER,
            data: &[0x2A], // = 42
        };
        assert_eq!(parse_integer_u32(&item).unwrap(), 42);
    }

    #[test]
    fn oid_matching() {
        let item = Asn1Item {
            tag: TAG_OID,
            data: OID_AES_256_CBC,
        };
        assert!(match_oid(&item, OID_AES_256_CBC));
        assert!(!match_oid(&item, OID_3DES_CBC));
    }

    #[test]
    fn truncated_data_errors() {
        let data: &[u8] = &[0x30]; // Only tag, no length
        assert!(read_item(data).is_err());
    }

    #[test]
    fn rejects_indefinite_length() {
        let data: &[u8] = &[0x30, 0x80, 0x00, 0x00]; // indefinite
        assert!(read_item(data).is_err());
    }

    #[test]
    fn nested_sequences() {
        // SEQUENCE { SEQUENCE { INTEGER 10 }, OCTET STRING [0xFF] }
        let data: &[u8] = &[
            0x30, 0x08, // outer SEQUENCE, length 8
            0x30, 0x03, // inner SEQUENCE, length 3
            0x02, 0x01, 0x0A, // INTEGER 10
            0x04, 0x01, 0xFF, // OCTET STRING [0xFF]
        ];
        let (outer, _) = read_item(data).unwrap();
        let children = expect_sequence(&outer).unwrap();
        assert_eq!(children.len(), 2);

        let inner_items = expect_sequence(&children[0]).unwrap();
        assert_eq!(parse_integer_u32(&inner_items[0]).unwrap(), 10);

        let octets = expect_octet_string(&children[1]).unwrap();
        assert_eq!(octets, &[0xFF]);
    }
}
