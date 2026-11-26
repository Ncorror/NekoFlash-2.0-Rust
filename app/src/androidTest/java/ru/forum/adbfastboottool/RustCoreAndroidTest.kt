package ru.forum.adbfastboottool

import androidx.test.platform.app.InstrumentationRegistry
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.File
import java.net.ServerSocket
import java.net.InetAddress
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.CancellationException
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

    @Test fun installedTokenizerPreservesOemLiteralsAndEmptyArguments() {
        val text = """oem "" 'путь с пробелом' a\ b"""
        val tokens = mutableListOf<FastbootCommandLine.Token>()
        RustCore.tokenize(text) { value, literal -> tokens.add(FastbootCommandLine.Token(value, literal)) }
        assertEquals(listOf("oem", "", "путь с пробелом", "a b"), tokens.map { it.value })
        assertEquals(listOf("oem", "\"\"", "'путь с пробелом'", "a\\ b"), tokens.map { it.literal })
        assertEquals(tokens, FastbootCommandLine.tokenize(text))
    }

    @Test fun nativeSparseInspectionPreservesCancellationAndRejectsTrailingBytes() {
        val directory = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir
        val image = File.createTempFile("rust-inspect-", ".sparse", directory)
        try {
            AndroidSparseImage.Writer(image, 4096, 1).use { writer ->
                writer.fill(0, 1)
                writer.finish()
            }
            val failure = runCatching {
                AndroidSparseImage.inspect(image) { throw CancellationException("cancel sparse scan") }
            }.exceptionOrNull()
            assertTrue(failure is CancellationException)
            assertEquals("cancel sparse scan", failure?.message)
            assertEquals(AndroidSparseImage.inspectKotlin(image), AndroidSparseImage.inspect(image))
            image.appendBytes(byteArrayOf(1))
            assertTrue(runCatching { AndroidSparseImage.inspect(image) }.exceptionOrNull() is IllegalArgumentException)
        } finally {
            image.delete()
        }
    }

    @Test fun nativeFastbootTcpQueriesAFragmentedPeer() {
        val executor = Executors.newSingleThreadExecutor()
        try {
            ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
                server.soTimeout = 5000
                val peer = executor.submit {
                    server.accept().use { socket ->
                        socket.soTimeout = 5000
                        val input = DataInputStream(socket.getInputStream())
                        val out = DataOutputStream(socket.getOutputStream())
                        fun read(n: Int) = ByteArray(n).also(input::readFully).toString(Charsets.US_ASCII)
                        assertEquals("FB01", read(4))
                        out.writeBytes("FB01")
                        val expected = "getvar:partition-size:boot_a"
                        assertEquals(expected.length.toLong(), input.readLong())
                        assertEquals(expected, read(expected.length))
                        for (reply in listOf("INFOpartition-size:boot_a: 0x4000", "OKAY")) {
                            out.writeLong(reply.length.toLong())
                            for (byte in reply.toByteArray(Charsets.US_ASCII)) out.writeByte(byte.toInt())
                        }
                    }
                }
                assertEquals("0x4000", RustCore.getvar("partition-size:boot_a", "127.0.0.1:${server.localPort}"))
                peer.get(5, TimeUnit.SECONDS)
            }
        } finally {
            executor.shutdownNow()
        }
    }
}
