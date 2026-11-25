package ru.forum.adbfastboottool

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RustCoreAndroidTest {
    @Test fun installedRustLibraryComputesAdbChecksum() {
        assertTrue("Rust JNI library must load from the installed APK", RustCore.available)
        for (size in listOf(0, 1, 512, 65_536, 1_048_576)) {
            val payload = ByteArray(size) { (it * 11).toByte() }
            val expected = payload.sumOf { it.toInt() and 0xff }
            assertEquals(expected, RustCore.checksum(payload))
            assertEquals(expected, AdbPacketChecksum.compute(payload))
        }
    }
}
