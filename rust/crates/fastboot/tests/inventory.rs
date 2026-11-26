use nekoflash_fastboot::inventory::{parse, parse_size};

#[test]
fn current_slot_and_real_partitions() {
    let snap = parse(
        [
            "INFO(bootloader) current-slot: a",
            "TEXTpartition-size:boot_a: 0xC000000",
            "partition-type:boot_a: raw",
            "has-slot:boot: yes",
            "all: done!",
        ],
        true,
        "OKAY",
        None,
    );
    assert_eq!(snap.value("CURRENT-SLOT"), Some("a"));
    assert_eq!(
        snap.partition("boot_a").unwrap().size_bytes,
        Some(201326592)
    );
    assert!(snap.partition("boot_a").unwrap().has_concrete_evidence());
    assert!(!snap.partition("boot").unwrap().has_concrete_evidence());
    assert!(snap.partition("boot_b").is_none());
}
#[test]
fn non_ab_inventory_is_not_extended() {
    let snap = parse(
        ["partition-size:boot: 123", "has-slot:boot: no"],
        true,
        "OKAY",
        None,
    );
    assert_eq!(snap.partition("boot").unwrap().has_slot, Some(false));
    assert!(snap.partition("boot_a").is_none());
    assert!(snap.partition("boot_b").is_none());
    assert_eq!(snap.value("current-slot"), None);
}
#[test]
fn duplicate_values_are_visible() {
    let snap = parse(
        [
            "current-slot: A",
            "current-slot: a",
            "current-slot: b",
            "product: marble",
            "product: MARBLE",
        ],
        true,
        "OKAY",
        None,
    );
    assert_eq!(snap.value("current-slot"), Some("b"));
    assert_eq!(snap.duplicate_variables.len(), 2);
    assert!(snap.duplicate_variables[0].conflicting);
    assert!(!snap.duplicate_variables[1].conflicting);
}
#[test]
fn failed_and_partial_output_preserved() {
    let snap = parse(
        ["(bootloader) unknown message\nINFOis-logical:system_a: yes\0"],
        false,
        "FAIL",
        Some("remote error"),
    );
    assert!(!snap.complete);
    assert_eq!(snap.final_status, "FAIL");
    assert_eq!(snap.final_message.as_deref(), Some("remote error"));
    assert_eq!(snap.ignored_lines, vec!["unknown message"]);
    assert_eq!(snap.partition("system_a").unwrap().logical, Some(true));
}
#[test]
fn sizes_match_signed_upstream_bounds() {
    assert_eq!(parse_size(Some("0XFF")), Some(255));
    assert_eq!(parse_size(Some("-1")), None);
    assert_eq!(parse_size(Some("0x8000000000000000")), None);
    assert_eq!(
        parse_size(Some("9223372036854775807")),
        Some(i64::MAX as u64)
    );
    assert_eq!(parse_size(Some("not a number")), None);
}

#[test]
fn preserves_first_seen_order_and_unicode_variable_names() {
    let s = parse(
        ["z:last\rÉTAT:État\r\na:first\nétat:ÉTAT"],
        true,
        "OKAY",
        None,
    );
    assert_eq!(
        s.variables_in_order().collect::<Vec<_>>(),
        [("z", "last"), ("état", "ÉTAT"), ("a", "first")]
    );
    assert!(!s.duplicate_variables[0].conflicting);
    assert_eq!(s.value("ÉTAT"), Some("ÉTAT"));
}
