//! Host I/O for claimed USB interfaces and explicitly selected TCP peers.
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "android", target_os = "linux"))]
pub mod usb;

pub fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "operation deadline expired"))
}

/// A write is one protocol message. A short USB write is ambiguous and must fail.
pub trait Packets {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()>;
    fn receive(&mut self, buffer: &mut [u8], deadline: Instant) -> io::Result<usize>;
}

pub struct Tcp(Arc<TcpStream>);

impl Tcp {
    pub fn connect(peer: SocketAddr, deadline: Instant) -> io::Result<Self> {
        let socket = TcpStream::connect_timeout(&peer, remaining(deadline)?)?;
        socket.set_nodelay(true)?;
        Ok(Self(Arc::new(socket)))
    }

    pub fn cancellation_handle(&self) -> io::Result<TcpCancel> {
        Ok(TcpCancel(Arc::downgrade(&self.0)))
    }

    /// Transfer the connected socket to a platform TLS implementation. The TLS
    /// owner must arrange its own cancellation once it takes over the socket.
    pub fn into_stream(self) -> io::Result<TcpStream> {
        Arc::try_unwrap(self.0).or_else(|socket| socket.try_clone())
    }

    pub fn read_exact(&mut self, mut buf: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !buf.is_empty() {
            self.0.set_read_timeout(Some(remaining(deadline)?))?;
            match self.0.as_ref().read(buf) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(n) => buf = &mut buf[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn write_all(&mut self, mut buf: &[u8], deadline: Instant) -> io::Result<()> {
        while !buf.is_empty() {
            self.0.set_write_timeout(Some(remaining(deadline)?))?;
            match self.0.as_ref().write(buf) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

pub struct TcpCancel(Weak<TcpStream>);

impl TcpCancel {
    pub fn cancel(&self) -> io::Result<()> {
        self.0
            .upgrade()
            .map_or(Ok(()), |socket| socket.shutdown(Shutdown::Both))
    }
}
