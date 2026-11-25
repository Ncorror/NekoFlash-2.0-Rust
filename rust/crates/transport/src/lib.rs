//! Host I/O for claimed USB interfaces and explicitly selected TCP peers.
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
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

pub struct Tcp(TcpStream);

impl Tcp {
    pub fn connect(peer: SocketAddr, deadline: Instant) -> io::Result<Self> {
        let socket = TcpStream::connect_timeout(&peer, remaining(deadline)?)?;
        socket.set_nodelay(true)?;
        Ok(Self(socket))
    }

    pub fn cancellation_handle(&self) -> io::Result<TcpCancel> {
        self.0.try_clone().map(TcpCancel)
    }

    pub fn read_exact(&mut self, mut buf: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !buf.is_empty() {
            self.0.set_read_timeout(Some(remaining(deadline)?))?;
            match self.0.read(buf) {
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
            match self.0.write(buf) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

pub struct TcpCancel(TcpStream);

impl TcpCancel {
    pub fn cancel(&self) -> io::Result<()> {
        self.0.shutdown(Shutdown::Both)
    }
}
