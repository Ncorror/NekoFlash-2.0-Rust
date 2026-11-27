"""Exercise the Android Fastboot range client on the emulator selected by adb."""
from pathlib import Path
import re
import subprocess
import sys

from sdk_fetch import IMAGE, PARTITION, transfer


def main(client):
    root = subprocess.check_output(
        ["adb", "shell", "mktemp", "-d", "/data/local/tmp/nekoflash-fetch.XXXXXX"], text=True
    ).strip()
    if not re.fullmatch(r"/data/local/tmp/nekoflash-fetch\.[A-Za-z0-9]{6}", root):
        raise ValueError(f"Unexpected temporary directory: {root!r}")
    executable = f"{root}/fetch_range"
    output = f"{root}/boot-prefix.img"

    def native(peer):
        # Android Emulator routes 10.0.2.2 to the host's loopback interface.
        _, port = peer.rsplit(":", 1)
        return ["adb", "shell", executable, f"10.0.2.2:{port}",
                PARTITION, output, "0", hex(len(IMAGE))]

    try:
        subprocess.run(["adb", "push", str(client), executable], check=True)
        subprocess.run(["adb", "shell", "chmod", "700", executable], check=True)

        result = transfer(native)
        assert result.returncode == 0, result.stderr + result.stdout
        assert subprocess.check_output(["adb", "exec-out", "cat", output]) == IMAGE
        subprocess.run(["adb", "shell", "rm", output], check=True)

        for final in (b"FAILread failed", None):
            result = transfer(native, final=final)
            assert result.returncode != 0, result.stdout
            assert subprocess.check_output(
                ["adb", "shell", "ls", "-A", root], text=True
            ).splitlines() == ["fetch_range"], "Unconfirmed data left in the output directory"
        print(f"PASS Android Fastboot fetch: {len(IMAGE)} bytes matched, FAIL and lost OKAY left no output")
    finally:
        subprocess.run(["adb", "shell", "rm", "-rf", root], check=True)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve(strict=True))
