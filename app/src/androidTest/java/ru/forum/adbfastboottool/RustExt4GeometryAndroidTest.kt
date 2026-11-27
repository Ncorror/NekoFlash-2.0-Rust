package ru.forum.adbfastboottool

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RustExt4GeometryAndroidTest {
    @Test fun nativeGroupGeometryMatchesBothInodeProfilesAtSizeBoundaries() {
        val limits = listOf(1L, 64, 128, 768, 8192, 8200, 32768, 131072, 1L shl 30, 0xffff_ffffL)
        val sizes = limits.flatMap { listOf(it - 1, it, it + 1) }
        for (blocks in sizes.filter { it in 1..0xffff_ffffL }) {
            for (projectIds in listOf(false, true)) {
                val profile = Ext4Profile.of(blocks, projectIds)
                val inodes = blocks * Ext4Geometry.BLOCK_SIZE / profile.inodeRatio
                val reserved = (profile.reservedRatio * blocks / 100.0).toLong()
                val native = RustCore.ext4Geometry(blocks, reserved, inodes, profile.inodeSize)
                val expected = runCatching { Ext4Geometry.of(blocks, profile, useRust = false) }
                val layout = expected.getOrNull()
                val detail = "blocks=$blocks, projectIds=$projectIds"
                if (layout == null) {
                    check(expected.exceptionOrNull() is IllegalArgumentException)
                    assertNull(detail, native)
                } else {
                    assertArrayEquals(detail, longArrayOf(
                        layout.blocks, layout.reservedBlocks, layout.blocksPerGroup,
                        layout.groups.toLong(), layout.descBlocks.toLong(), layout.inodesPerGroup.toLong(),
                        layout.inodeBlocksPerGroup.toLong()
                    ), native)
                }
            }
        }
    }
}
