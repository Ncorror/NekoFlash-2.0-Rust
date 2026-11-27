package ru.forum.adbfastboottool

import androidx.test.platform.app.InstrumentationRegistry

class RustWirelessAndroidTest : WirelessAdbContract(
    useRustHandshake = true,
    tempRoot = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir
)
