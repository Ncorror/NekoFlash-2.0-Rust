//! Platform-independent facade for the future Android bridge.
#![forbid(unsafe_code)]

pub use nekoflash_adb as adb;
pub use nekoflash_fastboot as fastboot;
