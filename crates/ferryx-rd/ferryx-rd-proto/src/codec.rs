use crate::CodecError;

pub(crate) struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.input.len() - self.offset
    }

    pub(crate) fn finish(self, field: &'static str) -> Result<(), CodecError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(CodecError::TrailingBytes {
                field,
                remaining: self.remaining(),
            })
        }
    }

    pub(crate) fn take(
        &mut self,
        length: usize,
        field: &'static str,
    ) -> Result<&'a [u8], CodecError> {
        if self.remaining() < length {
            return Err(CodecError::Truncated {
                field,
                needed: length,
                remaining: self.remaining(),
            });
        }
        let start = self.offset;
        self.offset += length;
        Ok(&self.input[start..self.offset])
    }

    pub(crate) fn take_remaining(&mut self) -> &'a [u8] {
        let remaining = &self.input[self.offset..];
        self.offset = self.input.len();
        remaining
    }

    pub(crate) fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        Ok(self.take(1, field)?[0])
    }

    pub(crate) fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        let bytes = self.take(2, field)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        let bytes = self.take(4, field)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn i32(&mut self, field: &'static str) -> Result<i32, CodecError> {
        let bytes = self.take(4, field)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn u64(&mut self, field: &'static str) -> Result<u64, CodecError> {
        let bytes = self.take(8, field)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(crate) fn f32(&mut self, field: &'static str) -> Result<f32, CodecError> {
        Ok(f32::from_bits(self.u32(field)?))
    }

    pub(crate) fn utf8(
        &mut self,
        length: usize,
        field: &'static str,
    ) -> Result<String, CodecError> {
        let bytes = self.take(length, field)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| CodecError::InvalidUtf8 { field })
    }
}

pub(crate) fn checked_bytes<'a>(
    value: &'a str,
    field: &'static str,
    max: usize,
) -> Result<&'a [u8], CodecError> {
    let bytes = value.as_bytes();
    if bytes.len() > max {
        Err(CodecError::LengthLimit {
            field,
            actual: bytes.len(),
            max,
        })
    } else {
        Ok(bytes)
    }
}

pub(crate) fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_f32(output: &mut Vec<u8>, value: f32) {
    push_u32(output, value.to_bits());
}

pub(crate) fn usize_to_u8(value: usize, field: &'static str) -> Result<u8, CodecError> {
    u8::try_from(value).map_err(|_| CodecError::LengthLimit {
        field,
        actual: value,
        max: u8::MAX as usize,
    })
}

pub(crate) fn usize_to_u16(value: usize, field: &'static str) -> Result<u16, CodecError> {
    u16::try_from(value).map_err(|_| CodecError::LengthLimit {
        field,
        actual: value,
        max: u16::MAX as usize,
    })
}

pub(crate) fn usize_to_u32(value: usize, field: &'static str) -> Result<u32, CodecError> {
    u32::try_from(value).map_err(|_| CodecError::LengthLimit {
        field,
        actual: value,
        max: u32::MAX as usize,
    })
}
