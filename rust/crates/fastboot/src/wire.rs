//! Response decoding from FastbootProtocol.readPacket, upstream f82484dd64fe.
//! A transport must preserve message boundaries; this is not a TCP framer.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub kind: String,
    pub payload: String,
    pub raw: String,
}

pub fn decode_reply(bytes: &[u8]) -> Reply {
    // Java US_ASCII decodes non-ASCII bytes to U+FFFD.
    let decoded: String = bytes
        .iter()
        .map(|&b| {
            if b.is_ascii() {
                char::from(b)
            } else {
                '\u{fffd}'
            }
        })
        .collect();
    let raw = decoded.replace('\0', "").trim().to_owned();
    let mut chars = raw.chars();
    let prefix: String = chars.by_ref().take(4).collect();
    if prefix.chars().count() < 4 {
        return Reply {
            kind: "UNKNOWN".to_owned(),
            payload: raw.clone(),
            raw,
        };
    }
    // The formatted text is for diagnostics. Only literal wire bytes can carry a status.
    let kind = if bytes.starts_with(prefix.as_bytes()) {
        prefix
    } else {
        "UNKNOWN".to_owned()
    };
    let payload = if kind == "DATA" {
        decoded[4..].trim_end_matches('\0').to_owned()
    } else {
        chars.as_str().trim().to_owned()
    };
    Reply { kind, payload, raw }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_replies() {
        for (raw, kind, payload) in [
            ("OKAY", "OKAY", ""),
            ("FAILbad", "FAIL", "bad"),
            ("INFO hello", "INFO", "hello"),
            ("TEXT hello", "TEXT", "hello"),
            ("DATA00000001", "DATA", "00000001"),
        ] {
            let r = decode_reply(raw.as_bytes());
            assert_eq!((r.kind.as_str(), r.payload.as_str()), (kind, payload));
        }
    }
    #[test]
    fn null_padding_and_short_response() {
        assert_eq!(decode_reply(b"  OKAY a\0\0 ").payload, "a");
        assert_eq!(decode_reply(b"OK").kind, "UNKNOWN");
    }
    #[test]
    fn status_and_data_are_never_reconstructed_from_formatted_text() {
        for packet in [&b"O\0KAY"[..], b" OKAY", b"D\0ATA00000020"] {
            assert_eq!(decode_reply(packet).kind, "UNKNOWN");
        }
        for data in [&b"DATA0000\x000020"[..], b"DATA 00000020", b"DATA00000020 "] {
            let reply = decode_reply(data);
            assert_eq!(reply.kind, "DATA");
            assert_eq!(crate::parse_data_size(reply.payload.as_bytes()), None);
        }
        assert_eq!(decode_reply(b"DATA00000020\0\0").payload, "00000020");
    }
    #[test]
    fn unknown_response_is_not_promoted_to_success() {
        let r = decode_reply(b"WHATignored");
        assert_eq!(r.kind, "WHAT");
        assert_eq!(r.payload, "ignored");
        assert_eq!(decode_reply(b"INFO\xff").payload, "\u{fffd}");
    }
}
