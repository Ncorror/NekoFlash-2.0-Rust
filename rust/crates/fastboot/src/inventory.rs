//! Port of FastbootGetVarAllParser.kt at upstream f82484dd64fe (Apache-2.0).
//! Last observed value wins; conflicting duplicates remain visible.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MetadataField {
    Size,
    Type,
    Logical,
    HasSlot,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    pub size_bytes: Option<u64>,
    pub partition_type: Option<String>,
    pub logical: Option<bool>,
    pub has_slot: Option<bool>,
    pub metadata_fields: BTreeSet<MetadataField>,
}
impl Partition {
    pub fn has_concrete_evidence(&self) -> bool {
        self.metadata_fields.iter().any(|f| {
            matches!(
                f,
                MetadataField::Size | MetadataField::Type | MetadataField::Logical
            )
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct DuplicateVariable {
    pub name: String,
    pub values: Vec<String>,
    pub conflicting: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub variables: BTreeMap<String, String>,
    pub partitions: Vec<Partition>,
    pub complete: bool,
    pub final_status: String,
    pub final_message: Option<String>,
    pub ignored_lines: Vec<String>,
    pub duplicate_variables: Vec<DuplicateVariable>,
}
impl Snapshot {
    pub fn value(&self, name: &str) -> Option<&str> {
        self.variables
            .get(&name.trim().to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn partition(&self, name: &str) -> Option<&Partition> {
        self.partitions
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }
}

fn normalize(raw: &str) -> String {
    let mut value = raw.trim().replace('\0', "");
    loop {
        let lower = value.to_ascii_lowercase();
        let len = if lower.starts_with("info") || lower.starts_with("text") {
            4
        } else if lower.starts_with("(bootloader)") {
            "(bootloader)".len()
        } else {
            break;
        };
        value = value[len..].trim().to_owned();
    }
    value
}

fn split_variable(line: &str) -> Option<(String, String)> {
    let (first, tail) = line.split_once(':')?;
    let family = first.to_ascii_lowercase();
    if matches!(
        family.as_str(),
        "partition-size"
            | "partition-type"
            | "is-logical"
            | "has-slot"
            | "slot-successful"
            | "slot-unbootable"
            | "slot-retry-count"
    ) {
        if let Some((target, value)) = tail.split_once(':') {
            // Match upstream scoped regex only when the raw target is nonempty.
            if !target.is_empty() {
                let target = target.trim().to_ascii_lowercase();
                let value = value.trim();
                return if target.is_empty() || value.is_empty() {
                    None
                } else {
                    Some((format!("{family}:{target}"), value.to_owned()))
                };
            }
        }
    }
    let name = first.trim().to_ascii_lowercase();
    let value = tail.trim();
    if name.is_empty() || value.is_empty() {
        None
    } else {
        Some((name, value.to_owned()))
    }
}

pub fn parse_size(raw: Option<&str>) -> Option<u64> {
    let clean = raw?.trim().to_ascii_lowercase();
    let value = if let Some(hex) = clean.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else {
        clean.parse::<i64>().ok()?
    };
    u64::try_from(value).ok()
}

pub fn parse<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    complete: bool,
    final_status: &str,
    final_message: Option<&str>,
) -> Snapshot {
    let mut variables = BTreeMap::new();
    let mut all_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order = Vec::new();
    let mut ignored_lines = Vec::new();
    for line in lines.into_iter().flat_map(str::lines) {
        let clean = normalize(line);
        if clean.trim().is_empty() {
            continue;
        }
        let Some((name, value)) = split_variable(&clean) else {
            ignored_lines.push(clean);
            continue;
        };
        if name == "all" && value.eq_ignore_ascii_case("done!") {
            continue;
        }
        if !all_values.contains_key(&name) {
            order.push(name.clone());
        }
        all_values
            .entry(name.clone())
            .or_default()
            .push(value.clone());
        variables.insert(name, value);
    }
    let mut by_partition: BTreeMap<String, Partition> = BTreeMap::new();
    for (name, value) in &variables {
        let Some((family, target)) = name.split_once(':') else {
            continue;
        };
        let field = match family {
            "partition-size" => MetadataField::Size,
            "partition-type" => MetadataField::Type,
            "is-logical" => MetadataField::Logical,
            "has-slot" => MetadataField::HasSlot,
            _ => continue,
        };
        let target = target.trim().to_ascii_lowercase();
        if target.is_empty() {
            continue;
        }
        let partition = by_partition
            .entry(target.clone())
            .or_insert_with(|| Partition {
                name: target,
                ..Partition::default()
            });
        partition.metadata_fields.insert(field);
        match field {
            MetadataField::Size => partition.size_bytes = parse_size(Some(value)),
            MetadataField::Type => partition.partition_type = Some(value.clone()),
            MetadataField::Logical => partition.logical = super::parse_boolean(Some(value)),
            MetadataField::HasSlot => partition.has_slot = super::parse_boolean(Some(value)),
        }
    }
    let duplicate_variables = order
        .into_iter()
        .filter_map(|name| {
            let values = all_values.remove(&name)?;
            if values.len() < 2 {
                return None;
            }
            let conflicting = values
                .iter()
                .map(|v| v.trim().to_ascii_lowercase())
                .collect::<BTreeSet<_>>()
                .len()
                > 1;
            Some(DuplicateVariable {
                name,
                values,
                conflicting,
            })
        })
        .collect();
    Snapshot {
        variables,
        partitions: by_partition.into_values().collect(),
        complete,
        final_status: final_status.to_owned(),
        final_message: final_message
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned),
        ignored_lines,
        duplicate_variables,
    }
}
