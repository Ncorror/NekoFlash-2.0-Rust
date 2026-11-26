//! Resolve explicit targets from a completed inventory. A slot suffix or a
//! filename is insufficient evidence that a partition actually exists.
use crate::inventory::{Partition, Snapshot};

#[derive(Debug, Clone, Copy)]
pub enum Slot {
    Current,
    Named(char),
    Other,
    All,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolutionError {
    IncompleteInventory,
    InvalidName,
    ConflictingEvidence,
    MissingPartition,
    UnknownTopology,
    InvalidSlot,
}

impl Snapshot {
    fn unambiguous(&self, key: &str) -> Result<Option<&str>, ResolutionError> {
        if self
            .duplicate_variables
            .iter()
            .any(|d| d.conflicting && d.name == key)
        {
            return Err(ResolutionError::ConflictingEvidence);
        }
        Ok(self.value(key))
    }

    fn observed(&self, name: &str) -> Result<&Partition, ResolutionError> {
        for family in ["partition-size", "partition-type", "is-logical"] {
            self.unambiguous(&format!("{family}:{name}"))?;
        }
        self.partition(name)
            .filter(|p| {
                p.size_bytes.is_some_and(|n| n > 0)
                    || p.partition_type.is_some()
                    || p.logical.is_some()
            })
            .ok_or(ResolutionError::MissingPartition)
    }

    pub fn targets(&self, name: &str, slot: Slot) -> Result<Vec<&Partition>, ResolutionError> {
        if !self.complete || self.final_status != "OKAY" {
            return Err(ResolutionError::IncompleteInventory);
        }
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        {
            return Err(ResolutionError::InvalidName);
        }
        let name = name.to_ascii_lowercase();
        let has_slot = self.unambiguous(&format!("has-slot:{name}"))?;
        match crate::parse_boolean(has_slot) {
            Some(false) => return self.observed(&name).map(|p| vec![p]),
            None if has_slot.is_none() && matches!(slot, Slot::Current) => {
                return self.observed(&name).map(|p| vec![p])
            }
            None => return Err(ResolutionError::UnknownTopology),
            Some(true) => {}
        }
        let count = self
            .unambiguous("slot-count")?
            .and_then(|s| s.parse::<u8>().ok())
            .filter(|c| (1..=26).contains(c))
            .ok_or(ResolutionError::UnknownTopology)?;
        let lookup = |n| self.observed(&format!("{name}_{}", char::from(b'a' + n)));
        if matches!(slot, Slot::All) {
            return (0..count).map(lookup).collect();
        }
        let selected = match slot {
            Slot::Named(c) if c.is_ascii_lowercase() => c as u8 - b'a',
            Slot::Current | Slot::Other => {
                let current = self
                    .unambiguous("current-slot")?
                    .ok_or(ResolutionError::UnknownTopology)?;
                let current = current.strip_prefix('_').unwrap_or(current).as_bytes();
                let [c] = *current else {
                    return Err(ResolutionError::InvalidSlot);
                };
                if !c.is_ascii_lowercase() || c - b'a' >= count {
                    return Err(ResolutionError::InvalidSlot);
                }
                if matches!(slot, Slot::Other) {
                    if count < 2 {
                        return Err(ResolutionError::InvalidSlot);
                    }
                    (c - b'a' + 1) % count
                } else {
                    c - b'a'
                }
            }
            _ => return Err(ResolutionError::InvalidSlot),
        };
        if selected >= count {
            return Err(ResolutionError::InvalidSlot);
        }
        lookup(selected).map(|p| vec![p])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::parse;
    #[test]
    fn legacy_and_ab_require_observed_targets() {
        let legacy = parse(
            ["has-slot:recovery: no", "partition-type:recovery: raw"],
            true,
            "OKAY",
            None,
        );
        assert_eq!(
            legacy.targets("recovery", Slot::All).unwrap()[0].name,
            "recovery"
        );
        let input = [
            "has-slot:boot:yes",
            "current-slot:_b",
            "slot-count:2",
            "partition-size:boot_b:0x100",
        ];
        let ab = parse(input, true, "OKAY", None);
        assert_eq!(ab.targets("boot", Slot::Current).unwrap()[0].name, "boot_b");
        assert_eq!(
            ab.targets("boot", Slot::All),
            Err(ResolutionError::MissingPartition)
        );
        let complete = parse(
            input.into_iter().chain(["partition-type:boot_a:raw"]),
            true,
            "OKAY",
            None,
        );
        assert_eq!(
            complete.targets("boot", Slot::Other).unwrap()[0].name,
            "boot_a"
        );
        assert_eq!(complete.targets("boot", Slot::All).unwrap().len(), 2);
        assert_eq!(
            complete.targets("boot", Slot::Named('c')),
            Err(ResolutionError::InvalidSlot)
        );
    }
    #[test]
    fn failed_or_conflicting_reports_cannot_select_a_partition() {
        let partial = parse(["partition-size:boot:4096"], false, "FAIL", None);
        assert_eq!(
            partial.targets("boot", Slot::Current),
            Err(ResolutionError::IncompleteInventory)
        );
        let bad = parse(
            ["partition-size:boot:4096", "partition-size:boot:8192"],
            true,
            "OKAY",
            None,
        );
        assert_eq!(
            bad.targets("boot", Slot::Current),
            Err(ResolutionError::ConflictingEvidence)
        );
    }
}
