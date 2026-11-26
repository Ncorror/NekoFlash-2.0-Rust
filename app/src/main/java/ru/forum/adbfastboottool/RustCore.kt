package ru.forum.adbfastboottool

internal object RustCore {
    val available = try {
        System.loadLibrary("nekoflash_core")
        true
    } catch (_: UnsatisfiedLinkError) {
        false
    }

    external fun tokenize(text: String, sink: java.util.function.BiConsumer<String, String>)

    external fun sparseInfo(fd: Int, checkCancelled: Runnable): LongArray

    external fun checksum(payload: ByteArray): Int
}
