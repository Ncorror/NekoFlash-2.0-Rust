use nekoflash_core::{adb, fastboot};
#[test]
fn facade_exposes_both_protocols() {
    assert_eq!(adb::checksum(&[255, 1]), 256);
    assert!(fastboot::validate_download(4096, Some(4096)).is_ok());
}
