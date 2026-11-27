package ru.forum.adbfastboottool

internal data class FastbootPacket(val type: String, val payload: String, val raw: String) {
    companion object {
        fun decode(buffer: ByteArray, count: Int): FastbootPacket {
            val wire = String(buffer, 0, count, Charsets.US_ASCII)
            val raw = wire.replace("\u0000", "").trim()
            if (wire.length < 4) return FastbootPacket("UNKNOWN", raw, raw)

            // Formatting the diagnostic text must never manufacture a status or a DATA size.
            val type = wire.substring(0, 4)
            if (!raw.startsWith(type)) return FastbootPacket("UNKNOWN", raw, raw)
            val payload = if (type == "DATA") wire.substring(4).trimEnd('\u0000') else raw.substring(4).trim()
            return FastbootPacket(type, payload, raw)
        }
    }
}
