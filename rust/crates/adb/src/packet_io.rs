//! ADB messages over a byte stream or claimed bulk endpoints. Authentication
//! and STLS must finish before opening device services.
use crate::wire::{Header, HEADER_SIZE};
use nekoflash_transport::{remaining, Packets, Tcp};
use std::io;
use std::num::NonZeroU16;
use std::time::Instant;

pub const MAX_PAYLOAD: u32 = 1024 * 1024;

pub trait Link {
    fn write_transfer(&mut self, buf: &[u8], deadline: Instant) -> io::Result<()>;
    fn read_exact(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<()>;
}

impl Link for Tcp {
    fn write_transfer(&mut self, buf: &[u8], deadline: Instant) -> io::Result<()> {
        self.write_all(buf, deadline)
    }
    fn read_exact(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<()> {
        Tcp::read_exact(self, buf, deadline)
    }
}

pub struct UsbLink<T> {
    bulk: T,
    packet_size: NonZeroU16,
}

impl<T: Packets> UsbLink<T> {
    pub fn new(bulk: T, out_max_packet_size: NonZeroU16) -> io::Result<Self> {
        if !matches!(out_max_packet_size.get(), 8 | 16 | 32 | 64 | 512 | 1024) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(Self {
            bulk,
            packet_size: out_max_packet_size,
        })
    }
}

impl<T: Packets> Link for UsbLink<T> {
    fn write_transfer(&mut self, buf: &[u8], deadline: Instant) -> io::Result<()> {
        // 16 KiB is accepted on the API 26 USB path and aligned to all bulk packet sizes.
        for part in buf.chunks(16384) {
            self.bulk.send(part, deadline)?;
        }
        if !buf.is_empty() && buf.len().is_multiple_of(self.packet_size.get() as usize) {
            self.bulk.send(&[], deadline)?;
        }
        Ok(())
    }
    fn read_exact(&mut self, mut buf: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !buf.is_empty() {
            remaining(deadline)?;
            let max = buf.len().min(16384);
            let count = self.bulk.receive(&mut buf[..max], deadline)?;
            if count > max {
                return Err(io::ErrorKind::InvalidData.into());
            }
            buf = &mut buf[count..];
        }
        Ok(())
    }
}

/// Takes the link out of service after a partial packet, invalid magic or checksum.
pub struct PacketIo<T> {
    link: Option<T>,
}

impl<T: Link> PacketIo<T> {
    pub fn new(link: T) -> Self {
        Self { link: Some(link) }
    }

    pub fn write(
        &mut self,
        command: u32,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
        peer_max: u32,
        deadline: Instant,
    ) -> io::Result<()> {
        let header = Header::for_payload(command, arg0, arg1, payload, peer_max.min(MAX_PAYLOAD))
            .map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut link = self.link.take().ok_or(io::ErrorKind::NotConnected)?;
        link.write_transfer(&header.encode(), deadline)?;
        if !payload.is_empty() {
            link.write_transfer(payload, deadline)?;
        }
        self.link = Some(link);
        Ok(())
    }

    pub fn read(
        &mut self,
        max: u32,
        local_version: u32,
        peer_version: u32,
        deadline: Instant,
    ) -> io::Result<(Header, Vec<u8>)> {
        let mut link = self.link.take().ok_or(io::ErrorKind::NotConnected)?;
        let mut header = [0; HEADER_SIZE];
        link.read_exact(&mut header, deadline)?;
        let header = Header::decode(&header, max.min(MAX_PAYLOAD))
            .map_err(|_| io::ErrorKind::InvalidData)?;
        let mut data = vec![0; header.data_length as usize];
        link.read_exact(&mut data, deadline)?;
        header
            .validate_payload(&data, local_version, peer_version)
            .map_err(|_| io::ErrorKind::InvalidData)?;
        self.link = Some(link);
        Ok((header, data))
    }
}
