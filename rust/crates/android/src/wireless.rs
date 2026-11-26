//! The Android socket and enrolled identity stay with the platform. Rust
//! negotiates CNXN/STLS and hands the authenticated stream back before OPEN.
use nekoflash_core::{
    adb::{connection::Connection, packet_io::Link},
    transport::remaining,
};
use std::{
    ffi::c_void,
    io,
    time::{Duration, Instant},
};

#[repr(C)]
pub struct Callbacks {
    context: *mut c_void,
    read: extern "C" fn(*mut c_void, *mut u8, usize) -> bool,
    write: extern "C" fn(*mut c_void, *const u8, usize) -> bool,
    upgrade: extern "C" fn(*mut c_void) -> bool,
    connected: extern "C" fn(*mut c_void, u32, u32, *const u8, usize),
    failed: extern "C" fn(*mut c_void, *const u8, usize),
}

fn completed(ok: bool, deadline: Instant) -> io::Result<()> {
    if !ok {
        return Err(io::Error::other(
            "Android Wireless debugging transport failed",
        ));
    }
    remaining(deadline).map(|_| ())
}

impl Link for &Callbacks {
    fn read_exact(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<()> {
        remaining(deadline)?;
        if buf.is_empty() {
            return Ok(());
        }
        completed(
            (self.read)(self.context, buf.as_mut_ptr(), buf.len()),
            deadline,
        )
    }
    fn write_transfer(&mut self, buf: &[u8], deadline: Instant) -> io::Result<()> {
        remaining(deadline)?;
        completed(
            (self.write)(self.context, buf.as_ptr(), buf.len()),
            deadline,
        )
    }
}

/// # Safety
/// The caller supplies live synchronous callbacks on the owning JNI thread.
/// read initializes its whole destination on success. Buffers are never retained.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_wireless_handshake(io: &Callbacks) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let result = Connection::wireless(
        io,
        |link, deadline| {
            remaining(deadline)?;
            completed((link.upgrade)(link.context), deadline)?;
            Ok(link)
        },
        deadline,
    );
    match result {
        Ok(conn) => (io.connected)(
            io.context,
            conn.peer_version(),
            conn.max_payload(),
            conn.banner().as_ptr(),
            conn.banner().len(),
        ),
        Err(e) => {
            let message = e.to_string();
            (io.failed)(io.context, message.as_ptr(), message.len());
        }
    }
}
