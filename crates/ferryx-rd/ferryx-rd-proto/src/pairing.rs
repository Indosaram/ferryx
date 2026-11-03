use crate::{
    codec::{checked_bytes, push_u16, usize_to_u16, usize_to_u8, Decoder},
    CodecError, WireCodec,
};

pub const MAX_PAIRING_NAME_BYTES: usize = 1024;
pub const PAIRING_KEY_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRequest {
    pub name: String,
}

impl WireCodec for PairingRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let name = checked_bytes(&self.name, "pairing request name", MAX_PAIRING_NAME_BYTES)?;
        let mut output = Vec::with_capacity(2 + name.len());
        push_u16(
            &mut output,
            usize_to_u16(name.len(), "pairing request name")?,
        );
        output.extend_from_slice(name);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let length = decoder.u16("pairing request name length")? as usize;
        if length > MAX_PAIRING_NAME_BYTES {
            return Err(CodecError::LengthLimit {
                field: "pairing request name",
                actual: length,
                max: MAX_PAIRING_NAME_BYTES,
            });
        }
        let name = decoder.utf8(length, "pairing request name")?;
        decoder.finish("pairing request")?;
        Ok(Self { name })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingGrant {
    pub pairing_id: String,
    pub host_name: String,
    pub key: [u8; PAIRING_KEY_SIZE],
}

impl WireCodec for PairingGrant {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let pairing_id = checked_bytes(&self.pairing_id, "pairing grant ID", u8::MAX as usize)?;
        let host_name = checked_bytes(
            &self.host_name,
            "pairing grant host name",
            u16::MAX as usize,
        )?;
        let mut output =
            Vec::with_capacity(4 + pairing_id.len() + host_name.len() + PAIRING_KEY_SIZE);
        output.push(usize_to_u8(pairing_id.len(), "pairing grant ID")?);
        output.extend_from_slice(pairing_id);
        push_u16(
            &mut output,
            usize_to_u16(host_name.len(), "pairing grant host name")?,
        );
        output.extend_from_slice(host_name);
        output.push(PAIRING_KEY_SIZE as u8);
        output.extend_from_slice(&self.key);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let pairing_id_length = decoder.u8("pairing grant ID length")? as usize;
        let pairing_id = decoder.utf8(pairing_id_length, "pairing grant ID")?;
        let host_name_length = decoder.u16("pairing grant host name length")? as usize;
        let host_name = decoder.utf8(host_name_length, "pairing grant host name")?;
        let key_length = decoder.u8("pairing key length")? as usize;
        if key_length != PAIRING_KEY_SIZE {
            return Err(CodecError::InvalidLength {
                field: "pairing key",
                actual: key_length,
                expected: PAIRING_KEY_SIZE,
            });
        }
        let key_bytes = decoder.take(key_length, "pairing key")?;
        let mut key = [0_u8; PAIRING_KEY_SIZE];
        key.copy_from_slice(key_bytes);
        decoder.finish("pairing grant")?;
        Ok(Self {
            pairing_id,
            host_name,
            key,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PairingRejectReason {
    DeniedByHost = 0,
    LockedOut = 1,
    PairingDisabled = 2,
}

impl PairingRejectReason {
    pub const ALL: [Self; 3] = [Self::DeniedByHost, Self::LockedOut, Self::PairingDisabled];
}

impl TryFrom<u8> for PairingRejectReason {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::DeniedByHost),
            1 => Ok(Self::LockedOut),
            2 => Ok(Self::PairingDisabled),
            unknown => Err(CodecError::UnknownPairingRejectReason(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingReject {
    pub reason: PairingRejectReason,
}

impl WireCodec for PairingReject {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(vec![self.reason as u8])
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let reason = PairingRejectReason::try_from(decoder.u8("pairing reject reason")?)?;
        decoder.finish("pairing reject")?;
        Ok(Self { reason })
    }
}
