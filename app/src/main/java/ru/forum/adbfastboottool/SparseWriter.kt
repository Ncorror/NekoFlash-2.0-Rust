package ru.forum.adbfastboottool

import java.io.ByteArrayOutputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Участок образа, который собирается на хосте: с блока [block]. */
internal sealed interface SparseChunk {
    val block: Long

    /** [blocks] блоков, заполненных 32-битным словом [value]. */
    data class Fill(override val block: Long, val blocks: Long, val value: Int = 0) : SparseChunk

    /** Данные, дополненные нулями до целого блока. */
    class Raw(override val block: Long, val bytes: ByteArray) : SparseChunk
}

/**
 * `sparse_file_write` из AOSP `libsparse` без сжатия и суммы: участки по
 * возрастанию блока, промежутки между ними и хвост до [totalBlocks] —
 * `DONT_CARE`. Так пишут образы `mke2fs -E android_sparse` и `ImageBuilder`.
 */
internal object SparseWriter {
    fun write(blockSize: Int, totalBlocks: Long, chunks: List<SparseChunk>): ByteArray =
        ByteArrayOutputStream().also { writeTo(it, SparsePlan(blockSize, totalBlocks, chunks)) {} }.toByteArray()

    fun writeTo(output: OutputStream, plan: SparsePlan, checkCancelled: () -> Unit) {
        val (blockSize, totalBlocks, chunks) = plan
        require(blockSize > 0 && totalBlocks in 0..UINT)
        fun blocks(chunk: SparseChunk): Long = when (chunk) {
            is SparseChunk.Fill -> chunk.blocks
            is SparseChunk.Raw -> (chunk.bytes.size.toLong() + blockSize - 1) / blockSize
        }
        var count = 0L
        var at = 0L
        for (chunk in chunks) {
            checkCancelled()
            val size = blocks(chunk)
            require(chunk.block in at..totalBlocks && size in 0..(totalBlocks - chunk.block))
            if (chunk is SparseChunk.Raw) require(size * blockSize <= UINT - CHUNK_HEADER_LEN)
            count += if (chunk.block > at) 2 else 1
            at = chunk.block + size
        }
        if (at < totalBlocks) count++
        require(count <= UINT)
        val header = ByteBuffer.allocate(HEADER_LEN).order(ByteOrder.LITTLE_ENDIAN)
        header.putInt(MAGIC).putShort(MAJOR.toShort()).putShort(0)
            .putShort(HEADER_LEN.toShort()).putShort(CHUNK_HEADER_LEN.toShort())
            .putInt(blockSize).putInt(totalBlocks.toInt()).putInt(count.toInt()).putInt(0)
        output.write(header.array())

        fun chunkHeader(type: Int, size: Long, payload: Long) {
            header.clear()
            header.putShort(type.toShort()).putShort(0).putInt(size.toInt()).putInt((CHUNK_HEADER_LEN + payload).toInt())
            output.write(header.array(), 0, CHUNK_HEADER_LEN)
        }
        at = 0
        for (chunk in chunks) {
            checkCancelled()
            if (chunk.block > at) chunkHeader(CHUNK_DONT_CARE, chunk.block - at, 0)
            val size = blocks(chunk)
            when (chunk) {
                is SparseChunk.Fill -> {
                    chunkHeader(CHUNK_FILL, size, FILL_BYTES.toLong())
                    header.putInt(0, chunk.value)
                    output.write(header.array(), 0, FILL_BYTES)
                }
                is SparseChunk.Raw -> {
                    chunkHeader(CHUNK_RAW, size, size * blockSize)
                    var offset = 0
                    while (offset < chunk.bytes.size) {
                        checkCancelled()
                        val n = minOf(64 * 1024, chunk.bytes.size - offset)
                        output.write(chunk.bytes, offset, n)
                        offset += n
                    }
                    val padding = (size * blockSize - chunk.bytes.size).toInt()
                    if (padding > 0) output.write(ByteArray(padding))
                }
            }
            at = chunk.block + size
        }
        if (at < totalBlocks) chunkHeader(CHUNK_DONT_CARE, totalBlocks - at, 0)
    }

    private const val MAGIC = 0xed26ff3a.toInt()
    private const val HEADER_LEN = 28
    private const val CHUNK_HEADER_LEN = 12
    private const val CHUNK_RAW = 0xcac1
    private const val CHUNK_FILL = 0xcac2
    private const val CHUNK_DONT_CARE = 0xcac3
    private const val FILL_BYTES = 4
    private const val MAJOR = 1
    private const val UINT = 0xFFFF_FFFFL
}

internal data class SparsePlan(val blockSize: Int, val totalBlocks: Long, val chunks: List<SparseChunk>)
