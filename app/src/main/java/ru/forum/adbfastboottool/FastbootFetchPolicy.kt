package ru.forum.adbfastboottool

internal object FastbootFetchPolicy {
    fun dataSize(reply: String, expected: Long?): Long? {
        val size = FastbootDataReplyPolicy.sizeOrNull(reply) ?: return null
        return size.takeIf { it > 0L && (expected == null || it == expected) }
    }
}
