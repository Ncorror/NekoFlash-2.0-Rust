//! Ports of FastbootDataReplyPolicy, FastbootDownloadSizePolicy and FastbootValueParser.
#![forbid(unsafe_code)]

pub mod inventory;
pub mod wire;
pub const WIRE_MAX_BYTES: u64 = u32::MAX as u64;

#[derive(Debug, PartialEq, Eq)]
pub enum DataReply<'a> {
    Accepted(u64),
    Invalid(&'a str),
    Mismatch { expected: u64, actual: u64 },
}

/// `reply` is the payload following DATA, not the full response.
pub fn validate_data_reply(reply: &str, expected: u64) -> DataReply<'_> {
    if reply.len() != 8 || !reply.bytes().all(|b| b.is_ascii_hexdigit()) {
        return DataReply::Invalid(reply);
    }
    match u64::from_str_radix(reply, 16) {
        Ok(actual) if actual == expected => DataReply::Accepted(actual),
        Ok(actual) => DataReply::Mismatch { expected, actual },
        Err(_) => DataReply::Invalid(reply),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadRejection {
    Empty,
    WireLimit { payload: u64, max: u64 },
    DeviceLimit { payload: u64, max: u64 },
}

pub fn validate_download(payload: i64, device_max: Option<i64>) -> Result<(), DownloadRejection> {
    if payload <= 0 {
        return Err(DownloadRejection::Empty);
    }
    let payload = payload as u64;
    if payload > WIRE_MAX_BYTES {
        return Err(DownloadRejection::WireLimit {
            payload,
            max: WIRE_MAX_BYTES,
        });
    }
    if let Some(max) = device_max.filter(|&n| n > 0).map(|n| n as u64) {
        if payload > max {
            return Err(DownloadRejection::DeviceLimit { payload, max });
        }
    }
    Ok(())
}

pub fn parse_boolean(raw: Option<&str>) -> Option<bool> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotState {
    None,
    Snapshotted,
    Merging,
    Unknown,
}

pub fn parse_snapshot_state(raw: Option<&str>) -> SnapshotState {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "none" | "cancelled" => SnapshotState::None,
        "snapshotted" => SnapshotState::Snapshotted,
        "merging" => SnapshotState::Merging,
        _ => SnapshotState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_acceptance() {
        assert_eq!(
            validate_data_reply("0000ABcd", 0xabcd),
            DataReply::Accepted(0xabcd)
        );
        assert_eq!(
            validate_data_reply("FFFFFFFF", WIRE_MAX_BYTES),
            DataReply::Accepted(WIRE_MAX_BYTES)
        );
    }
    #[test]
    fn malformed_data() {
        for s in ["", "1234", "123456789", "0000000g", "DATA0000", "000000０"] {
            assert_eq!(validate_data_reply(s, 0), DataReply::Invalid(s));
        }
    }
    #[test]
    fn data_mismatch() {
        assert_eq!(
            validate_data_reply("00000002", 1),
            DataReply::Mismatch {
                expected: 1,
                actual: 2
            }
        );
    }
    #[test]
    fn size_boundaries() {
        assert_eq!(validate_download(0, None), Err(DownloadRejection::Empty));
        assert_eq!(validate_download(-1, None), Err(DownloadRejection::Empty));
        assert!(validate_download(WIRE_MAX_BYTES as i64, None).is_ok());
        assert_eq!(
            validate_download(WIRE_MAX_BYTES as i64 + 1, None),
            Err(DownloadRejection::WireLimit {
                payload: WIRE_MAX_BYTES + 1,
                max: WIRE_MAX_BYTES
            })
        );
    }
    #[test]
    fn device_limit() {
        assert!(validate_download(100, Some(100)).is_ok());
        assert!(validate_download(100, Some(0)).is_ok());
        assert!(validate_download(100, Some(-1)).is_ok());
        assert_eq!(
            validate_download(101, Some(100)),
            Err(DownloadRejection::DeviceLimit {
                payload: 101,
                max: 100
            })
        );
    }
    #[test]
    fn values() {
        assert_eq!(parse_boolean(Some(" YES ")), Some(true));
        assert_eq!(parse_boolean(Some("0")), Some(false));
        assert_eq!(parse_boolean(Some("unknown")), None);
        assert_eq!(parse_snapshot_state(Some("cancelled")), SnapshotState::None);
        assert_eq!(
            parse_snapshot_state(Some("MERGING")),
            SnapshotState::Merging
        );
        assert_eq!(
            parse_snapshot_state(Some("snapshotted")),
            SnapshotState::Snapshotted
        );
        assert_eq!(parse_snapshot_state(None), SnapshotState::Unknown);
    }
}
