//! Synchronous USBDEVFS_BULK on an interface claimed by Android. The caller
//! keeps the interface claimed and exclusively assigned until this handle drops.
//! UAPI layout from linux/usbdevice_fs.h, USBDEVFS_BULK.
use super::{remaining, Packets};
use std::ffi::c_void;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::time::Instant;

#[repr(C)]
struct BulkTransfer {
    endpoint: u32,
    length: u32,
    timeout_ms: u32,
    data: *mut c_void,
}

nix::ioctl_readwrite!(bulk_ioctl, b'U', 2, BulkTransfer);

pub struct UsbBulk {
    fd: OwnedFd,
    input: u8,
    output: u8,
}

impl UsbBulk {
    pub fn new(fd: BorrowedFd<'_>, input: u8, output: u8) -> io::Result<Self> {
        if !(0x81..=0x8f).contains(&input) || !(1..=15).contains(&output) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid bulk endpoint pair",
            ));
        }
        Ok(Self {
            fd: fd.try_clone_to_owned()?,
            input,
            output,
        })
    }

    fn transfer(
        &self,
        ep: u8,
        ptr: *mut c_void,
        len: usize,
        deadline: Instant,
    ) -> io::Result<usize> {
        let mut transfer = BulkTransfer {
            endpoint: ep.into(),
            length: u32::try_from(len).map_err(|_| io::ErrorKind::InvalidInput)?,
            // Zero means no timeout to usbfs. Round positive sub-millisecond budgets up.
            timeout_ms: remaining(deadline)?
                .as_nanos()
                .div_ceil(1_000_000)
                .min(u32::MAX as u128) as u32,
            data: ptr,
        };
        // SAFETY: send and receive supply a live slice of exactly len bytes.
        // OUT endpoints only read the slice; IN endpoints have exclusive access.
        // The synchronous ioctl finishes before the borrow ends.
        let count = unsafe { bulk_ioctl(self.fd.as_raw_fd(), &mut transfer) }?;
        let count = count as usize;
        if count > len {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(count)
    }
}

impl Packets for UsbBulk {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        let n = self.transfer(
            self.output,
            bytes.as_ptr().cast_mut().cast(),
            bytes.len(),
            deadline,
        )?;
        if n != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short USB bulk write",
            ));
        }
        Ok(())
    }

    fn receive(&mut self, bytes: &mut [u8], deadline: Instant) -> io::Result<usize> {
        self.transfer(self.input, bytes.as_mut_ptr().cast(), bytes.len(), deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::fd::AsFd;
    use std::time::Duration;

    #[test]
    fn rejects_control_endpoints_and_preserves_original_descriptor() {
        let fd = File::open("/dev/null").unwrap();
        assert!(UsbBulk::new(fd.as_fd(), 0x80, 1).is_err());
        assert!(UsbBulk::new(fd.as_fd(), 0x81, 0x82).is_err());
        let mut usb = UsbBulk::new(fd.as_fd(), 0x81, 1).unwrap();
        assert_eq!(
            usb.send(b"test", Instant::now()).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(usb
            .receive(&mut [0; 8], Instant::now() + Duration::from_secs(1))
            .is_err());
        drop(usb);
        assert!(fd.metadata().is_ok());
    }
}
