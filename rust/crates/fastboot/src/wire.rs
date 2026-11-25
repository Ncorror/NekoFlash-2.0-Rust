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
    let decoded: String = bytes.iter().filter(|&&b| b != 0).map(|&b| {
        if b.is_ascii() { char::from(b) } else { '\u{fffd}' }
    }).collect();
    let raw = decoded.trim().to_owned();
    let mut chars = raw.chars();
    let prefix: String = chars.by_ref().take(4).collect();
    if prefix.chars().count() < 4 {
        return Reply { kind: "UNKNOWN".to_owned(), payload: raw.clone(), raw };
    }
    let payload = chars.as_str().trim().to_owned();
    Reply { kind: prefix, payload, raw }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_replies() {
        for (raw, kind, payload) in [("OKAY", "OKAY", ""), ("FAILbad", "FAIL", "bad"), ("INFO hello", "INFO", "hello"), ("TEXT hello", "TEXT", "hello"), ("DATA00000001", "DATA", "00000001")] {
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
    fn unknown_response_is_not_promoted_to_success() {
        let r = decode_reply(b"WHATignored");
        assert_eq!(r.kind, "WHAT");
        assert_eq!(r.payload, "ignored");
        assert_eq!(decode_reply(b"INFO\xff").payload, "\u{fffd}");
    }
}
