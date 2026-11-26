package ru.forum.adbfastboottool

class AdbWirelessClientTest : WirelessAdbContract(System.getenv("NEKOFLASH_TEST_RUST_HANDSHAKE") == "1")
