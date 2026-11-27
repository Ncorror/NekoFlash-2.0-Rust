//! Group geometry used by the Android ext4 formatter with 4096-byte blocks,
//! 32-byte descriptors, sparse_super and the AOSP ext4_utils mkfs profile.
//! Port of the upstream application's Ext4Geometry.kt (Apache-2.0).
const BLOCK_SIZE: u64 = 4096;
const BITMAP_BITS: u64 = BLOCK_SIZE * 8;
const MAX_BLOCKS: u64 = u32::MAX as u64;

#[derive(Debug, PartialEq, Eq)]
pub struct Geometry {
    pub blocks: u64,
    pub reserved: u64,
    pub blocks_per_group: u64,
    pub groups: u64,
    pub descriptor_blocks: u64,
    pub inodes_per_group: u64,
    pub inode_blocks_per_group: u64,
}

fn backup_group(group: u64) -> bool {
    group <= 1
        || [3, 5, 7].iter().any(|base| {
            let mut n = group;
            while n.is_multiple_of(*base) {
                n /= base;
            }
            n == 1
        })
}

pub fn geometry(
    requested: u64,
    reserved: u64,
    inodes: u64,
    inode_size: u32,
) -> Result<Geometry, &'static str> {
    if !(1..=MAX_BLOCKS).contains(&requested)
        || reserved >= requested
        || inodes > MAX_BLOCKS
        || !matches!(inode_size, 128 | 256 | 512)
    {
        return Err("Invalid argument passed to ext2 library");
    }
    let inode_size = u64::from(inode_size);
    let mut blocks = requested;
    let mut reserved_blocks = reserved;
    let mut blocks_per_group = BITMAP_BITS;
    loop {
        let groups = blocks.div_ceil(blocks_per_group);
        if groups == 0 {
            return Err("Filesystem too small.");
        }
        let descriptors = groups.div_ceil(BLOCK_SIZE / 32);
        let mut ipg = inodes.max(12).div_ceil(groups);
        if ipg > BITMAP_BITS {
            if blocks_per_group < 256 {
                return Err("Too many inodes");
            }
            blocks_per_group -= 8;
            blocks = requested;
            continue;
        }
        let per_block = BLOCK_SIZE / inode_size;
        ipg = ipg.min(65536 - per_block);
        let (group_inodes, table_blocks) = loop {
            // ext2fs_initialize aligns down to a multiple of eight inodes.
            let count = (ipg.div_ceil(per_block) * per_block).max(8) & !7u64;
            if count * groups <= MAX_BLOCKS {
                break (count, count.div_ceil(per_block));
            }
            ipg -= 1;
        };
        if descriptors > blocks_per_group * 3 / 4 {
            return Err("meta_bg would be needed");
        }
        if 3 + table_blocks + descriptors > blocks_per_group {
            return Err("Too many inodes");
        }
        let remainder = blocks % blocks_per_group;
        let backup = if backup_group(groups - 1) {
            1 + descriptors
        } else {
            0
        };
        let overhead = 2 + table_blocks + backup;
        if groups == 1 && remainder != 0 && remainder < overhead {
            return Err("Filesystem too small.");
        }
        if remainder != 0 && remainder < overhead + 50 {
            blocks -= remainder;
            // Preserve the rounding order of the AOSP-profile Kotlin formatter.
            let ratio = 100.0 * reserved as f64 / requested as f64;
            reserved_blocks = (ratio * blocks as f64 / 100.0) as u64;
            continue;
        }
        return Ok(Geometry {
            blocks,
            reserved: reserved_blocks,
            blocks_per_group,
            groups,
            descriptor_blocks: descriptors,
            inodes_per_group: group_inodes,
            inode_blocks_per_group: table_blocks,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concrete_groups_and_sparse_super() {
        let g = geometry(8192, 81, 8192, 128).unwrap();
        assert_eq!(
            g,
            Geometry {
                blocks: 8192,
                reserved: 81,
                blocks_per_group: 32768,
                groups: 1,
                descriptor_blocks: 1,
                inodes_per_group: 8192,
                inode_blocks_per_group: 256
            }
        );
        for (n, yes) in [
            (0, true),
            (1, true),
            (2, false),
            (5, true),
            (9, true),
            (15, false),
            (49, true),
        ] {
            assert_eq!(backup_group(n), yes);
        }
    }
    #[test]
    fn project_inodes_match_mke2fs_at_an_odd_multiple_of_eight() {
        let g = geometry(8200, 82, 8200, 512).unwrap();
        assert_eq!((g.inodes_per_group, g.inode_blocks_per_group), (8200, 1025));
    }
    #[test]
    fn short_tail_and_large_volumes() {
        let g = geometry(32769, 327, 32769, 128).unwrap();
        assert!(g.blocks <= 32769);
        assert!(g.groups > 0);
        assert_eq!(geometry(32769, 327, 8192, 256).unwrap().blocks, 32768);
        let g = geometry(MAX_BLOCKS, MAX_BLOCKS / 100, MAX_BLOCKS / 16, 512).unwrap();
        assert!(g.groups > 65535);
        assert!(g.groups * g.inodes_per_group <= MAX_BLOCKS);
        assert!(geometry(32, 0, 32, 128).is_err());
        assert!(geometry(MAX_BLOCKS + 1, 0, 1, 256).is_err());
    }
}
