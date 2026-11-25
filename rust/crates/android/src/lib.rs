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
