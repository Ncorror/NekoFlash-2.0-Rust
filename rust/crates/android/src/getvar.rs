//! One read-only Fastboot query on a claimed USB interface or an explicit
//! numeric TCP peer. The Android caller serializes use of the USB interface.
use nekoflash_core::{
    fastboot::session::{Error, Session, TcpPackets},
    transport::{usb::UsbBulk, Tcp},
};
use std::{
    ffi::{c_char, CStr},
    io,
    net::SocketAddr,
    os::fd::BorrowedFd,
    time::{Duration, Instant},
};

#[repr(C)]
pub struct Reply {
    length: u32,
    bytes: [u8; 4096],
}

fn store(output: &mut Reply, bytes: &[u8]) {
    let n = output.bytes.len().min(bytes.len());
    output.bytes[..n].copy_from_slice(&bytes[..n]);
    output.length = n as u32;
}

/// # Safety
/// output points to caller-owned storage. name and optional peer are NUL-terminated.
/// Without peer, fd is alive, claimed by Android and held exclusively until return.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_getvar(
    fd: i32,
    input: i32,
    out: i32,
    peer: *const c_char,
    name: *const c_char,
    output: *mut Reply,
) -> i32 {
    // SAFETY: C++ supplies storage that lives until it has copied the reply.
    let Some(output) = (unsafe { output.as_mut() }) else {
        return 2;
    };
    let query = || -> Result<String, Error> {
        if name.is_null() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        // SAFETY: strings are JNI-borrowed and released only after returning.
        let name = unsafe { CStr::from_ptr(name) }
            .to_str()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        if !peer.is_null() {
            let address = unsafe { CStr::from_ptr(peer) }
                .to_str()
                .ok()
                .and_then(|s| s.parse::<SocketAddr>().ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "expected IPv4:port or [IPv6]:port",
                    )
                })?;
            let wire = TcpPackets::handshake(Tcp::connect(address, deadline)?, deadline)?;
            return Session::new(wire).getvar(name, deadline, |_| {});
        }
        if fd < 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        // SAFETY: Kotlin retains the claimed connection and holds its transaction lock.
        let fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let ep = |n| u8::try_from(n).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput));
        let wire = UsbBulk::new(fd, ep(input)?, ep(out)?)?;
        Session::new(wire).getvar(name, deadline, |_| {})
    };
    match query() {
        Ok(s) => {
            store(output, s.as_bytes());
            0
        }
        Err(Error::Device(s)) => {
            store(output, s.as_bytes());
            1
        }
        Err(Error::Transport(e)) => {
            store(output, e.to_string().as_bytes());
            if e.kind() == io::ErrorKind::InvalidInput {
                2
            } else {
                3
            }
        }
    }
}
