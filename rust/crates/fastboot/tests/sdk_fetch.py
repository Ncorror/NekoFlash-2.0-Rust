"""Compare the SDK and Rust clients against the same read-only Fastboot TCP peer."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile

PARTITION = "boot_a"
IMAGE = bytes((index * 13 + 7) % 251 for index in range(70_001))


def send_frame(conn, payload):
    conn.sendall(struct.pack(">Q", len(payload)))
    for start in range(0, len(payload), 1401):
        conn.sendall(payload[start:start + 1401])


def serve(listener, final, before_final):
    conn, _ = listener.accept()
    requests = 0
    with conn, conn.makefile("rb") as stream:
        conn.settimeout(10)
        assert stream.read(4) == b"FB01"
        conn.sendall(b"FB01")
        while header := stream.read(8):
            length, = struct.unpack(">Q", header)
            assert 0 < length <= 64
            command = stream.read(length).decode("ascii")
            if command.startswith("getvar:"):
                values = {"has-slot:boot_a": "no", "partition-size:boot_a": hex(len(IMAGE)),
                          "max-fetch-size": "0x20000", "is-userspace": "yes"}
                value = values.get(command[len("getvar:"):])
                send_frame(conn, b"FAILunknown variable" if value is None else b"OKAY" + value.encode())
                continue
            verb, name, *fields = command.split(":")
            assert (verb, name) == ("fetch", PARTITION), command
            assert len(fields) <= 2
            assert not fields or int(fields[0], 0) == 0
            assert len(fields) < 2 or int(fields[1], 0) == len(IMAGE)
            requests += 1
            send_frame(conn, b"INFOreading")
            send_frame(conn, f"DATA{len(IMAGE):08x}".encode())
            send_frame(conn, IMAGE[:3])
            send_frame(conn, IMAGE[3:])
            before_final()
            if final is None:
                break
            send_frame(conn, final)
    assert requests == 1


def transfer(command_for_peer, final=b"OKAY", before_final=lambda: None):
    with socket.socket() as listener, ThreadPoolExecutor(max_workers=1) as worker:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(10)
        peer = f"127.0.0.1:{listener.getsockname()[1]}"
        pending = worker.submit(serve, listener, final, before_final)
        result = subprocess.run(command_for_peer(peer), capture_output=True, text=True, timeout=15)
        pending.result()
        return result


def main(sdk, rust):
    with tempfile.TemporaryDirectory() as folder:
        files = Path(folder)
        sdk_out = files / "sdk.img"
        rust_out = files / "rust.img"

        def native(peer):
            return [rust, peer, PARTITION, rust_out, "0", hex(len(IMAGE))]

        result = transfer(lambda peer: [sdk, "-s", "tcp:" + peer, "fetch", PARTITION, sdk_out])
        assert result.returncode == 0, result.stderr
        result = transfer(native)
        assert result.returncode == 0, result.stderr
        assert sdk_out.read_bytes() == rust_out.read_bytes() == IMAGE
        print("SDK and Rust fetched", len(IMAGE), "identical bytes. SHA-256", hashlib.sha256(IMAGE).hexdigest())
        rust_out.unlink()

        result = transfer(native, final=b"FAILread failed")
        assert result.returncode != 0 and "Fastboot FAIL: read failed" in result.stderr, result.stderr
        assert set(files.iterdir()) == {sdk_out}

        result = transfer(native, final=None)
        assert result.returncode != 0, result.stdout
        assert set(files.iterdir()) == {sdk_out}

        result = transfer(native, before_final=lambda: rust_out.write_bytes(b"existing result"))
        assert result.returncode != 0, result.stdout
        assert rust_out.read_bytes() == b"existing result"
        assert set(files.iterdir()) == {sdk_out, rust_out}
        print("Verified final FAIL, dropped acknowledgment and concurrent destination creation")


if __name__ == "__main__":
    main(*sys.argv[1:])
