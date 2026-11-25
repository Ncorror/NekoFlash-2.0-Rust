//! ADB wire header, separate from USB and TCP transfer semantics.
//! Derived from AdbProtocol.kt at f82484dd64fe (Apache-2.0).
use crate::{checksum, checksum_valid};

pub const HEADER_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub command: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub data_length: u32,
    pub data_check: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    HeaderLength,
    InvalidMagic,
    PayloadTooLarge,
    PayloadLength,
    Checksum,
}

impl Header {
    pub fn for_payload(command: u32, arg0: u32, arg1: u32, payload: &[u8], max_payload: u32) -> Result<Self, WireError> {
        let data_length = u32::try_from(payload.len()).map_err(|_| WireError::PayloadTooLarge)?;
        if data_length > max_payload || data_length > i32::MAX as u32 { return Err(WireError::PayloadTooLarge); }
        Ok(Self { command, arg0, arg1, data_length, data_check: checksum(payload) })
    }

    pub fn encode(self) -> [u8; HEADER_SIZE] {
        let mut result = [0; HEADER_SIZE];
        for (chunk, value) in result.as_chunks_mut::<4>().0.iter_mut().zip([self.command, self.arg0, self.arg1, self.data_length, self.data_check, !self.command]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        result
    }

    pub fn decode(input: &[u8], max_payload: u32) -> Result<Self, WireError> {
        let bytes: &[u8; HEADER_SIZE] = input.try_into().map_err(|_| WireError::HeaderLength)?;
        let mut words = [0u32; 6];
        for (word, chunk) in words.iter_mut().zip(bytes.as_chunks::<4>().0.iter()) {
            *word = u32::from_le_bytes(*chunk);
        }
        let [command, arg0, arg1, data_length, data_check, magic] = words;
        if magic != !command { return Err(WireError::InvalidMagic); }
        if data_length > max_payload || data_length > i32::MAX as u32 { return Err(WireError::PayloadTooLarge); }
        Ok(Self { command, arg0, arg1, data_length, data_check })
    }

    pub fn validate_payload(self, payload: &[u8], local: u32, peer: u32) -> Result<(), WireError> {
        if payload.len() != self.data_length as usize { return Err(WireError::PayloadLength); }
        if !checksum_valid(self.data_check, payload, local, peer) { return Err(WireError::Checksum); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VERSION_SKIP_CHECKSUM, VERSION_WITH_CHECKSUM};
    const CNXN: u32 = u32::from_le_bytes(*b"CNXN");

    #[test]
    fn wire_layout_and_roundtrip() {
        let header = Header::for_payload(CNXN, 0x0100_0000, 4096, b"abc", 4096).unwrap();
        let bytes = header.encode();
        assert_eq!(&bytes[..4], b"CNXN");
        assert_eq!(&bytes[12..16], &[3, 0, 0, 0]);
        assert_eq!(&bytes[16..20], &294u32.to_le_bytes());
        assert_eq!(Header::decode(&bytes, 4096), Ok(header));
    }
    #[test]
    fn truncated_and_extra_header() {
        assert_eq!(Header::decode(&[0; 23], 4096), Err(WireError::HeaderLength));
        assert_eq!(Header::decode(&[0; 25], 4096), Err(WireError::HeaderLength));
    }
    #[test]
    fn rejects_magic_mismatch() {
        let mut bytes = Header::for_payload(CNXN, 0, 0, b"", 4096).unwrap().encode();
        bytes[20] ^= 1;
        assert_eq!(Header::decode(&bytes, 4096), Err(WireError::InvalidMagic));
    }
    #[test]
    fn rejects_excessive_payload_before_allocation() {
        let bytes = Header::for_payload(CNXN, 0, 0, b"abc", 4096).unwrap().encode();
        assert_eq!(Header::decode(&bytes, 2), Err(WireError::PayloadTooLarge));
        assert_eq!(Header::for_payload(CNXN, 0, 0, b"abc", 2), Err(WireError::PayloadTooLarge));
    }
    #[test]
    fn validates_length_and_negotiated_checksum() {
        let h = Header::for_payload(CNXN, 0, 0, b"abc", 4096).unwrap();
        assert_eq!(h.validate_payload(b"ab", VERSION_SKIP_CHECKSUM, VERSION_SKIP_CHECKSUM), Err(WireError::PayloadLength));
        assert_eq!(h.validate_payload(b"xyz", VERSION_WITH_CHECKSUM, VERSION_SKIP_CHECKSUM), Err(WireError::Checksum));
        assert!(h.validate_payload(b"xyz", VERSION_SKIP_CHECKSUM, VERSION_SKIP_CHECKSUM).is_ok());
    }
}
