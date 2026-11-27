//! Fastboot v1 transactions. One owner sends one command at a time. A broken
//! transaction consumes the transport, including on cancellation or a short file.
use crate::{parse_data_size, validate_data_reply, validate_download, wire, DataReply};
use nekoflash_transport::{remaining, Packets, Tcp};
use std::io::{self, Read, Write};
use std::time::Instant;

fn invalid(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Wire format specified in AOSP fastboot/README.md, TCP Protocol v1.
pub struct TcpPackets {
    tcp: Tcp,
    data_left_in_frame: u64,
}

impl TcpPackets {
    pub fn handshake(mut tcp: Tcp, deadline: Instant) -> io::Result<Self> {
        tcp.write_all(b"FB01", deadline)?;
        let mut peer = [0; 4];
        tcp.read_exact(&mut peer, deadline)?;
        if &peer[..2] != b"FB" || !peer[2..].iter().all(u8::is_ascii_digit) || &peer[2..] == b"00" {
            return Err(invalid("unsupported Fastboot TCP handshake"));
        }
        Ok(Self {
            tcp,
            data_left_in_frame: 0,
        })
    }

    fn frame_length(&mut self, max: u64, deadline: Instant) -> io::Result<u64> {
        let mut length = [0; 8];
        self.tcp.read_exact(&mut length, deadline)?;
        let n = u64::from_be_bytes(length);
        if n == 0 || n > max {
            return Err(invalid("invalid Fastboot TCP frame length"));
        }
        Ok(n)
    }
}
impl Packets for TcpPackets {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        if self.data_left_in_frame != 0 {
            return Err(invalid("unconsumed Fastboot TCP data"));
        }
        self.tcp
            .write_all(&(bytes.len() as u64).to_be_bytes(), deadline)?;
        self.tcp.write_all(bytes, deadline)
    }
    fn receive(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize> {
        if self.data_left_in_frame != 0 {
            return Err(invalid("unconsumed Fastboot TCP data"));
        }
        let n = self.frame_length(buf.len() as u64, deadline)? as usize;
        self.tcp.read_exact(&mut buf[..n], deadline)?;
        Ok(n)
    }

    fn receive_data(
        &mut self,
        buf: &mut [u8],
        remaining_bytes: u64,
        deadline: Instant,
    ) -> io::Result<usize> {
        if buf.is_empty() || remaining_bytes == 0 {
            return Ok(0);
        }
        if self.data_left_in_frame == 0 {
            self.data_left_in_frame = self.frame_length(remaining_bytes, deadline)?;
        }
        if self.data_left_in_frame > remaining_bytes {
            return Err(invalid("invalid Fastboot TCP data read"));
        }
        let n = self.data_left_in_frame.min(buf.len() as u64) as usize;
        self.tcp.read_exact(&mut buf[..n], deadline)?;
        self.data_left_in_frame -= n as u64;
        Ok(n)
    }
}

pub struct Session<T>(Option<T>);

fn check_name(name: &str, colon: bool) -> io::Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c) || (colon && c == b':'))
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}

fn command<T: Packets>(wire: &mut T, text: &str, deadline: Instant) -> io::Result<()> {
    if text.len() > 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Fastboot command exceeds 64 bytes",
        ));
    }
    wire.send(text.as_bytes(), deadline)
}

#[derive(Debug)]
pub enum Error {
    Transport(io::Error),
    Device(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "{error}"),
            Self::Device(reason) => write!(f, "Fastboot FAIL: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Device(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Transport(e)
    }
}

fn reply<T: Packets>(
    wire: &mut T,
    deadline: Instant,
    mut info: impl FnMut(&str),
) -> Result<wire::Reply, Error> {
    let mut buffer = [0; 1024];
    loop {
        remaining(deadline)?;
        let n = wire.receive(&mut buffer, deadline)?;
        let bytes = buffer
            .get(..n)
            .ok_or_else(|| invalid("invalid packet length"))?;
        if !matches!(
            bytes.get(..4),
            Some(b"INFO" | b"TEXT" | b"FAIL" | b"DATA" | b"OKAY")
        ) {
            return Err(invalid("invalid status prefix").into());
        }
        let r = wire::decode_reply(bytes);
        match r.kind.as_str() {
            "INFO" | "TEXT" => info(&r.payload),
            "FAIL" => return Err(Error::Device(r.payload)),
            "OKAY" | "DATA" => return Ok(r),
            _ => return Err(invalid("unrecognized Fastboot response").into()),
        }
    }
}

impl<T: Packets> Session<T> {
    pub fn new(wire: T) -> Self {
        Self(Some(wire))
    }

    fn transaction<R>(&mut self, f: impl FnOnce(&mut T) -> Result<R, Error>) -> Result<R, Error> {
        let mut wire = self
            .0
            .take()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        let result = f(&mut wire);
        if result.is_ok() || matches!(result, Err(Error::Device(_))) {
            self.0 = Some(wire);
        }
        result
    }

    /// Emits INFO and TEXT in arrival order, including getvar:all inventory.
    pub fn getvar(
        &mut self,
        name: &str,
        deadline: Instant,
        mut info: impl FnMut(&str),
    ) -> Result<String, Error> {
        check_name(name, true)?;
        if name.len() > 57 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        let cmd = format!("getvar:{name}");
        self.transaction(|wire| {
            command(wire, &cmd, deadline)?;
            let mut named_info = None;
            let mut plain_info = None;
            let r = reply(wire, deadline, |s| {
                info(s);
                let line = s.strip_prefix("(bootloader)").unwrap_or(s).trim();
                if let Some(v) = labelled_value(name, line) {
                    if !v.is_empty() {
                        named_info = Some(v.to_owned());
                    }
                } else if !line.is_empty() && !line.contains(':') && named_info.is_none() {
                    plain_info = Some(line.to_owned());
                }
            })?;
            if r.kind != "OKAY" {
                return Err(invalid("unexpected DATA in getvar").into());
            }
            // A direct OKAY may be a bare scalar containing colons, e.g. a MAC.
            Ok(if r.payload.is_empty() {
                named_info.or(plain_info).unwrap_or_default()
            } else {
                labelled_value(name, &r.payload)
                    .unwrap_or(&r.payload)
                    .to_owned()
            })
        })
    }

    /// Fetch an exact range from an explicitly resolved partition. Use a staging
    /// writer: bytes already written must be discarded if the final status fails.
    pub fn fetch_range(
        &mut self,
        partition: &str,
        offset: u64,
        size: u32,
        output: &mut impl Write,
        deadline: Instant,
        mut progress: impl FnMut(u32) -> io::Result<()>,
    ) -> Result<(), Error> {
        check_name(partition, false)?;
        if size == 0 || offset > i64::MAX as u64 - u64::from(size) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        let cmd = format!("fetch:{partition}:0x{offset:08x}:0x{size:08x}");
        if cmd.len() > 64 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        progress(0)?;
        self.transaction(|wire| {
            command(wire, &cmd, deadline)?;
            let header = reply(wire, deadline, |_| {})?;
            if header.kind != "DATA" || parse_data_size(header.payload.as_bytes()) != Some(size) {
                return Err(invalid("device did not announce the requested fetch range").into());
            }
            let mut buf = vec![0u8; (size as usize).min(16 * 1024)];
            let mut received = 0;
            while received < size {
                remaining(deadline)?;
                let left = size - received;
                let n = wire.receive_data(&mut buf, u64::from(left), deadline)?;
                if n == 0 || n > buf.len() || n > left as usize {
                    return Err(invalid("invalid Fastboot data read length").into());
                }
                output.write_all(&buf[..n])?;
                received += n as u32;
                progress(received)?;
            }
            if reply(wire, deadline, |_| {})?.kind != "OKAY" {
                return Err(invalid("fetch not confirmed").into());
            }
            Ok(())
        })
    }

    /// Accepts an explicitly resolved partition. No inferred slot or retry. The
    /// caller must perform the app's existing partition and operation review.
    pub fn flash(
        &mut self,
        partition: &str,
        reader: &mut impl Read,
        length: i64,
        device_max: Option<i64>,
        deadline: Instant,
        mut progress: impl FnMut(u64) -> io::Result<()>,
    ) -> Result<(), Error> {
        check_name(partition, false)?;
        let flash = format!("flash:{partition}");
        if flash.len() > 64 || validate_download(length, device_max).is_err() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        let length = length as u64;
        self.transaction(|wire| {
            progress(0)?;
            command(wire, &format!("download:{length:08x}"), deadline)?;
            let mut response = [0; 1024];
            // Require the exact DATAxxxxxxxx bytes before consuming the image.
            loop {
                remaining(deadline)?;
                let n = wire.receive(&mut response, deadline)?;
                let packet = response
                    .get(..n)
                    .ok_or_else(|| invalid("invalid packet length"))?;
                if packet.starts_with(b"INFO") || packet.starts_with(b"TEXT") {
                    continue;
                }
                if let Some(reason) = packet.strip_prefix(b"FAIL") {
                    return Err(Error::Device(String::from_utf8_lossy(reason).into_owned()));
                }
                let digits = packet
                    .strip_prefix(b"DATA")
                    .and_then(|b| std::str::from_utf8(b).ok());
                if !matches!(
                    digits.map(|s| validate_data_reply(s, length)),
                    Some(DataReply::Accepted(_))
                ) {
                    return Err(invalid("device did not accept the exact download length").into());
                }
                break;
            }
            let mut sent = 0;
            let mut buf = vec![0; 64 * 1024];
            while sent < length {
                remaining(deadline)?;
                let n = (length - sent).min(buf.len() as u64) as usize;
                reader.read_exact(&mut buf[..n])?;
                wire.send(&buf[..n], deadline)?;
                sent += n as u64;
                progress(sent)?;
            }
            if reply(wire, deadline, |_| {})?.kind != "OKAY" {
                return Err(invalid("download not confirmed").into());
            }
            command(wire, &flash, deadline)?;
            if reply(wire, deadline, |_| {})?.kind != "OKAY" {
                return Err(invalid("flash not confirmed").into());
            }
            Ok(())
        })
    }
}

fn labelled_value<'a>(name: &str, line: &'a str) -> Option<&'a str> {
    let prefix = line.get(..name.len())?;
    let matches = prefix
        .bytes()
        .zip(name.bytes())
        .all(|(a, b)| a.eq_ignore_ascii_case(&b) || (a == b'_' && b == b'-'));
    if !matches {
        return None;
    }
    line[name.len()..].strip_prefix(':').map(str::trim)
}
