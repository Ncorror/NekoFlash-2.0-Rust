//! One ADB connection with one active service at a time. The Android platform
//! owns the host identity and TLS pairing material. Session errors require a
//! new handshake; a partially sent service is never retried.
use crate::{
    packet_io::{Link, PacketIo, MAX_PAYLOAD},
    wire::Header,
    VERSION_SKIP_CHECKSUM, VERSION_WITH_CHECKSUM,
};
use std::io::{self, Read, Write};
use std::time::Instant;

const CNXN: u32 = u32::from_le_bytes(*b"CNXN");
const AUTH: u32 = u32::from_le_bytes(*b"AUTH");
const STLS: u32 = u32::from_le_bytes(*b"STLS");
const OPEN: u32 = u32::from_le_bytes(*b"OPEN");
const WRTE: u32 = u32::from_le_bytes(*b"WRTE");
const OKAY: u32 = u32::from_le_bytes(*b"OKAY");
const CLSE: u32 = u32::from_le_bytes(*b"CLSE");
const BANNER: &[u8] = b"host::features=shell_v2\0";
fn bad(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

pub trait HostIdentity {
    fn sign_token(&mut self, token: &[u8; 20]) -> io::Result<Vec<u8>>;
    /// Android's base64 ADB public key and comment, terminated by NUL.
    fn public_key_payload(&mut self) -> io::Result<Vec<u8>>;
}

pub struct Connection<T> {
    wire: PacketIo<T>,
    local_version: u32,
    peer_version: u32,
    max: u32,
    next_id: u32,
    healthy: bool,
    banner: Vec<u8>,
}

impl<T: Link> Connection<T> {
    fn established(
        wire: PacketIo<T>,
        local_version: u32,
        h: Header,
        banner: Vec<u8>,
    ) -> io::Result<Self> {
        if h.command != CNXN || h.arg0 < VERSION_WITH_CHECKSUM || h.arg1 == 0 {
            return Err(bad("invalid ADB connection confirmation"));
        }
        Ok(Self {
            wire,
            local_version,
            peer_version: h.arg0,
            max: h.arg1.min(MAX_PAYLOAD),
            next_id: 1,
            healthy: true,
            banner,
        })
    }
    pub fn peer_version(&self) -> u32 {
        self.peer_version
    }
    pub fn max_payload(&self) -> u32 {
        self.max
    }
    pub fn banner(&self) -> &[u8] {
        &self.banner
    }

    pub fn usb(link: T, identity: &mut impl HostIdentity, deadline: Instant) -> io::Result<Self> {
        let v = VERSION_WITH_CHECKSUM;
        let mut wire = PacketIo::new(link);
        wire.write(CNXN, v, MAX_PAYLOAD, BANNER, 4096, deadline)?;
        let mut attempts = 0;
        loop {
            let (h, data) = wire.read(MAX_PAYLOAD, v, v, deadline)?;
            if h.command == CNXN {
                return Self::established(wire, v, h, data);
            }
            if h.command != AUTH || h.arg0 != 1 || h.arg1 != 0 || data.len() != 20 {
                return Err(bad("expected ADB authentication challenge"));
            }
            let token = data
                .as_slice()
                .try_into()
                .map_err(|_| bad("invalid AUTH token"))?;
            let (kind, payload) = match attempts {
                0 => (2, identity.sign_token(token)?),
                1 => (3, identity.public_key_payload()?),
                _ => return Err(io::ErrorKind::PermissionDenied.into()),
            };
            wire.write(AUTH, kind, 0, &payload, 4096, deadline)?;
            attempts += 1;
        }
    }

    /// The upgrade callback consumes the socket and must complete the TLS
    /// handshake using the paired host identity or return an error.
    pub fn wireless<L: Link>(
        link: L,
        upgrade: impl FnOnce(L, Instant) -> io::Result<T>,
        deadline: Instant,
    ) -> io::Result<Self> {
        let v = VERSION_SKIP_CHECKSUM;
        let mut plain = PacketIo::new(link);
        plain.write(CNXN, v, MAX_PAYLOAD, BANNER, 4096, deadline)?;
        let (h, payload) = plain.read(4096, v, v, deadline)?;
        if h.command != STLS
            || h.arg0 != VERSION_WITH_CHECKSUM
            || h.arg1 != 0
            || !payload.is_empty()
        {
            return Err(bad("Wireless ADB requires STLS"));
        }
        plain.write(STLS, VERSION_WITH_CHECKSUM, 0, &[], 4096, deadline)?;
        let mut wire = PacketIo::new(upgrade(plain.into_link()?, deadline)?);
        let (h, data) = wire.read(MAX_PAYLOAD, v, v, deadline)?;
        h.validate_payload(&data, v, h.arg0)
            .map_err(|_| bad("invalid ADB connection banner checksum"))?;
        Self::established(wire, v, h, data)
    }

    pub fn open(&mut self, service: &str, deadline: Instant) -> io::Result<Service<'_, T>> {
        if !self.healthy {
            return Err(io::ErrorKind::NotConnected.into());
        }
        if service.is_empty()
            || service.as_bytes().contains(&0)
            || service.len() >= self.max as usize
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| bad("ADB stream IDs exhausted"))?;
        let mut name = Vec::from(service);
        name.push(0);
        self.healthy = false;
        self.wire.write(OPEN, id, 0, &name, self.max, deadline)?;
        let (h, body) =
            self.wire
                .read(MAX_PAYLOAD, self.local_version, self.peer_version, deadline)?;
        if h.arg1 != id || !body.is_empty() {
            return Err(bad("invalid OPEN reply"));
        }
        if h.command == CLSE {
            self.healthy = true;
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "ADB service rejected",
            ));
        }
        if h.command != OKAY || h.arg0 == 0 {
            return Err(bad("missing OPEN acknowledgement"));
        }
        Ok(Service {
            conn: self,
            local: id,
            remote: h.arg0,
            deadline,
            pending: Vec::new(),
            offset: 0,
            closed: false,
            close_stdin_ack: false,
            failed: false,
        })
    }
}

pub struct Service<'a, T: Link> {
    conn: &'a mut Connection<T>,
    local: u32,
    remote: u32,
    deadline: Instant,
    pending: Vec<u8>,
    offset: usize,
    closed: bool,
    close_stdin_ack: bool,
    failed: bool,
}

impl<T: Link> Service<'_, T> {
    fn send(&mut self, cmd: u32, payload: &[u8]) -> io::Result<()> {
        if self.failed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        self.failed = true;
        self.conn.wire.write(
            cmd,
            self.local,
            self.remote,
            payload,
            self.conn.max,
            self.deadline,
        )?;
        self.failed = false;
        Ok(())
    }
    fn event(&mut self) -> io::Result<u32> {
        if self.failed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        self.failed = true;
        let (h, bytes) = self.conn.wire.read(
            MAX_PAYLOAD,
            self.conn.local_version,
            self.conn.peer_version,
            self.deadline,
        )?;
        if h.arg0 != self.remote || h.arg1 != self.local {
            return Err(bad("ADB stream ID mismatch"));
        }
        match h.command {
            WRTE if self.pending.is_empty() => {
                self.pending = bytes;
                self.offset = 0;
            }
            OKAY | CLSE if bytes.is_empty() => {}
            _ => return Err(bad("unexpected ADB stream message")),
        }
        self.failed = false;
        if h.command == WRTE && self.pending.is_empty() {
            self.send(OKAY, &[])?;
        }
        if h.command == CLSE {
            self.send(CLSE, &[])?;
            self.closed = true;
            self.conn.healthy = true;
        }
        Ok(h.command)
    }
    fn write_packet(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.send(WRTE, buf)?;
        loop {
            match self.event()? {
                OKAY => return Ok(()),
                CLSE => return Err(io::ErrorKind::BrokenPipe.into()),
                _ => {}
            }
        }
    }
    /// Recovery's legacy -1 request expects a zero-byte WRTE, then DONEDONE.
    pub fn write_empty_packet(&mut self) -> io::Result<()> {
        self.write_packet(&[])
    }
}
impl<T: Link> Read for Service<'_, T> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.pending.is_empty() {
            if self.closed {
                return Ok(0);
            }
            if self.event()? == OKAY && !std::mem::take(&mut self.close_stdin_ack) {
                self.failed = true;
                return Err(bad("unexpected ADB write acknowledgement"));
            }
        }
        let n = out.len().min(self.pending.len() - self.offset);
        out[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        if self.offset == self.pending.len() {
            self.pending.clear();
            if !self.closed {
                self.send(OKAY, &[])?;
            }
        }
        Ok(n)
    }
}
impl<T: Link> Write for Service<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(self.conn.max as usize);
        self.write_packet(&buf[..n])?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T: Link> Service<'_, T> {
    /// Release a completed service. Do not reuse stream IDs while the peer may
    /// still have a packet in flight. A failed close leaves the link unusable.
    pub fn finish(mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        self.send(CLSE, &[])?;
        loop {
            let (h, data) = self.conn.wire.read(
                MAX_PAYLOAD,
                self.conn.local_version,
                self.conn.peer_version,
                self.deadline,
            )?;
            if h.arg0 != self.remote || h.arg1 != self.local {
                return Err(bad("ADB close ID mismatch"));
            }
            match h.command {
                CLSE if data.is_empty() => {
                    self.conn.healthy = true;
                    return Ok(());
                }
                OKAY if data.is_empty() && std::mem::take(&mut self.close_stdin_ack) => {}
                WRTE => {} // In flight when the host closed. Never acknowledge new data.
                _ => return Err(bad("invalid ADB close response")),
            }
        }
    }
    /// Drain stdout while the peer processes CLOSE_STDIN. A daemon can send
    /// several WRTE packets before acknowledging our input or closing the service.
    pub(crate) fn finish_shell_input(&mut self) -> io::Result<()> {
        const FRAME: [u8; 5] = [4, 0, 0, 0, 0];
        let mut parts = FRAME.chunks(self.conn.max as usize).peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                self.send(WRTE, part)?;
                self.close_stdin_ack = true;
            } else {
                self.write_packet(part)?;
            }
        }
        Ok(())
    }
}
