package ru.forum.adbfastboottool

internal object RustCore {
    val available = try {
        System.loadLibrary("nekoflash_core")
        true
    } catch (_: UnsatisfiedLinkError) {
        false
    }

    private external fun queryVariable(fd: Int, epIn: Int, epOut: Int, peer: String?, name: String): ByteArray

    fun getvar(name: String, peer: String? = null, fd: Int = -1, epIn: Int = 0, epOut: Int = 0): String {
        check(available) { "Rust JNI library is unavailable" }
        require(name != "all" && name.length in 1..57 && name.matches(Regex("[A-Za-z0-9_.:-]+"))) {
            "Expected one Fastboot variable name"
        }
        val reply = queryVariable(fd, epIn, epOut, peer, name)
        val value = String(reply, 1, reply.size - 1, Charsets.UTF_8)
        return when (reply[0].toInt()) {
            0 -> value
            1 -> throw IllegalStateException("Fastboot FAIL: $value")
            2 -> throw IllegalArgumentException(value)
            else -> throw java.io.IOException(value)
        }
    }

    external fun wirelessHandshake(
        read: java.util.function.Consumer<ByteArray>, write: java.util.function.Consumer<ByteArray>,
        upgrade: Runnable, peer: IntArray
    ): ByteArray

    external fun tokenize(text: String, sink: java.util.function.BiConsumer<String, String>)

    external fun ext4Geometry(blocks: Long, reserved: Long, inodes: Long, inodeSize: Int): LongArray?

    external fun sparseInfo(fd: Int, checkCancelled: Runnable): LongArray

    external fun checksum(payload: ByteArray): Int
}
