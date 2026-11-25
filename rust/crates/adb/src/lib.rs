//! Port of AdbPacketChecksum.kt (Apache-2.0).
#![forbid(unsafe_code)]

pub mod wire;
pub const VERSION_WITH_CHECKSUM: u32 = 0x0100_0000;
pub const VERSION_SKIP_CHECKSUM: u32 = 0x0100_0001;

pub fn checksum(payload: &[u8]) -> u32 {
    payload
        .iter()
        .fold(0u32, |sum, &byte| sum.wrapping_add(u32::from(byte)))
}

pub fn checksum_required(local: u32, peer: u32) -> bool {
    local.min(peer) < VERSION_SKIP_CHECKSUM
}

pub fn checksum_valid(expected: u32, payload: &[u8], local: u32, peer: u32) -> bool {
    !checksum_required(local, peer) || expected == checksum(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_bytes() {
        assert_eq!(checksum(&[0, 128, 255]), 383);
        assert_eq!(checksum(&[]), 0);
    }
    #[test]
    fn classic_peer_requires_checksum() {
        assert!(checksum_valid(
            255,
            &[255],
            VERSION_SKIP_CHECKSUM,
            VERSION_WITH_CHECKSUM
        ));
        assert!(!checksum_valid(
            0,
            &[255],
            VERSION_SKIP_CHECKSUM,
            VERSION_WITH_CHECKSUM
        ));
    }
    #[test]
    fn negotiated_new_version_skips_checksum() {
        assert!(checksum_valid(
            0,
            &[255],
            VERSION_SKIP_CHECKSUM,
            VERSION_SKIP_CHECKSUM
        ));
    }
}
