//! C ABI consumed by the Android JNI adapter.
use nekoflash_core::adb;

/// # Safety
/// For nonzero length, data must identify readable contiguous bytes for the
/// duration of the call. JNI supplies a borrowed array and releases it on return.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_adb_checksum(data: *const u8, length: usize) -> u32 {
    if length == 0 {
        return 0;
    }
    // SAFETY: the caller satisfies the function contract.
    adb::checksum(unsafe { std::slice::from_raw_parts(data, length) })
}

/// # Safety
/// `bytes` must point to `length` readable bytes in the borrowed JNI string.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_fastboot_data_size(bytes: *const u8, length: usize) -> i64 {
    if length != 8 || bytes.is_null() {
        return -1;
    }
    // SAFETY: JNI holds the string until this synchronous call returns.
    let digits = unsafe { std::slice::from_raw_parts(bytes, length) };
    nekoflash_core::fastboot::parse_data_size(digits).map_or(-1, i64::from)
}

#[cfg(unix)]
#[repr(C)]
pub struct SparseInfo {
    block_size: u32,
    blocks: u32,
    chunks: u32,
}

/// # Safety
/// fd belongs to a live, exclusively positioned file for this call; result
/// points to writable storage. The callback may use context only synchronously.
/// Returns 0 on success, 1 on invalid input, 2 on I/O error, 3 on cancellation.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nekoflash_sparse_info(
    fd: i32,
    context: *mut std::ffi::c_void,
    cancelled: extern "C" fn(*mut std::ffi::c_void) -> bool,
    result: *mut SparseInfo,
) -> i32 {
    use std::{fs::File, io, os::fd::BorrowedFd};
    if fd < 0 || result.is_null() {
        return 1;
    }
    // SAFETY: JNI retains the ParcelFileDescriptor on the stack. Rust closes only its duplicate.
    let fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let Ok(owned) = fd.try_clone_to_owned() else {
        return 2;
    };
    let mut file = File::from(owned);
    let inspected = nekoflash_core::images::inspect(&mut file, || {
        if cancelled(context) {
            Err(io::ErrorKind::Interrupted.into())
        } else {
            Ok(())
        }
    });
    match inspected {
        Ok(info) => {
            // SAFETY: the C caller supplied storage with the same repr(C) layout.
            unsafe {
                result.write(SparseInfo {
                    block_size: info.block_size,
                    blocks: info.blocks,
                    chunks: info.chunks,
                })
            };
            0
        }
        Err(e) => match e.kind() {
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => 1,
            io::ErrorKind::Interrupted => 3,
            _ => 2,
        },
    }
}

/// # Safety
/// text points to len UTF-16 code units. Each callback borrows the two token
/// slices synchronously. It returns false if Java raised an exception.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_tokenize(
    text: *const u16,
    len: usize,
    context: *mut std::ffi::c_void,
    emit: extern "C" fn(*mut std::ffi::c_void, *const u16, usize, *const u16, usize) -> bool,
) -> bool {
    let input = if len == 0 {
        &[]
    } else {
        // SAFETY: supplied by GetStringChars until all callbacks return.
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    let Ok(tokens) = nekoflash_core::fastboot::command_line::tokenize(input) else {
        return false;
    };
    tokens.iter().all(|t| {
        emit(
            context,
            t.value.as_ptr(),
            t.value.len(),
            t.literal.as_ptr(),
            t.literal.len(),
        )
    })
}

#[cfg(any(target_os = "android", target_os = "linux"))]
mod getvar;

mod wireless;

/// # Safety
/// On success, writes the seven geometry fields into caller-owned storage.
#[no_mangle]
pub unsafe extern "C" fn nekoflash_ext4_geometry(
    blocks: u64,
    reserved: u64,
    inodes: u64,
    inode_size: u32,
    output: *mut [i64; 7],
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    let Ok(g) = nekoflash_core::images::ext4::geometry(blocks, reserved, inodes, inode_size) else {
        return false;
    };
    *output = [
        g.blocks,
        g.reserved,
        g.blocks_per_group,
        g.groups,
        g.descriptor_blocks,
        g.inodes_per_group,
        g.inode_blocks_per_group,
    ]
    .map(|n| n as i64);
    true
}
