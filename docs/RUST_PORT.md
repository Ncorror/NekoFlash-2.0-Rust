# NekoFlash 2.0 Rust — architecture and migration journal

Baseline: [Ncorror/NekoFlash-2.0 @ f82484dd64fe](https://github.com/Ncorror/NekoFlash-2.0/tree/f82484dd64fe980137585ae613b6de0a0cc540ed).
Independent repository: `Ncorror/NekoFlash-2.0-Rust`. Preserve Git history, authorship, Apache-2.0 license, third-party notices, and existing hardware behavior. Upstream push is disabled in the experimental clone.

## Current architecture

| Component | Implementation in the Rust fork | Installed Android path |
|---|---|---|
| Android application | Kotlin UI, service ownership, permission and SAF lifecycle | Separate application ID and public workspace. |
| `transport` | TCP deadlines and shutdown, synchronous bulk I/O on duplicated USB descriptors | Rust diagnostic queries. Kotlin and the existing C++ URB queue own the established production transports. |
| `fastboot` | FB01/TCP framing, getvar, inventory, observed slot targets and bounded download/flash transactions | Opt-in scalar getvar via OTG/TCP. The terminal tokenizer uses Rust. The established backend handles flashing, logical partitions and recovery. |
| `adb` | CNXN/AUTH/STLS, single-service streams, shell, SYNC and sideload | Rust checksums and opt-in Wireless ADB negotiation. Android supplies the paired TLS socket, keys, dispatcher, file operations and sideload verdicts. |
| `images` | Sparse v1 inspection, a bounded RAW/FILL writer and ext4 group geometry | Production sparse inspection via JNI. Rust computes ext4 group and inode table sizes. Kotlin builds the metadata and streams ext4/F2FS output to the staging file. |
| `core` and `android` | Protocol facade and a static archive linked by NDK CMake into `libnekoflash_core.so` | Four Android ABIs. JNI borrows arrays synchronously and duplicates borrowed descriptors for native I/O. |
| Operations | Kotlin serial binding, foreground admission, plan composition, script execution, confirmation, progress and recovery journal | Existing device and account operations stay available while native alternatives undergo parity testing. |

The original checkout is kept separately. In the fork, relative file operations, the shell link, logs and scoped shares use `Download/NekoFlash-Rust`. The Kotlin namespace remains `ru.forum.adbfastboottool` and the debug application ID is `ru.forum.adbfastboottool.rust.dev`.

## Building and exercising the port

Run the host checks from `rust/`:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

For Android, use the pinned SDK/NDK in `app/build.gradle`, JDK 17 and the project's filesystem-tool prerequisites. Install the Rust targets and build from the checkout root:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:testDebugUnitTest :app:lintDebug
python3 tests/check_filesystem_apk.py app/build/outputs/apk/debug/app-debug.apk
```

The Android terminal supports a read-only Rust query for a connected OTG Fastboot device and for an explicitly selected Fastboot TCP peer. Supply a numeric IPv4 address or bracketed IPv6 address and the actual port.

```text
fastboot --rust getvar product
fastboot --rust -s tcp:192.0.2.10:5554 getvar current-slot
```

After pairing in the Wi-Fi dialog, the following terminal command uses Rust to negotiate STLS on the displayed **connection** port. Android performs the TLS handshake using the enrolled host key. Subsequent shell and transfer commands use the same authenticated socket and dispatcher.

```text
adb --rust connect 192.0.2.10:37001
```

## Qualification still required

- Compare Rust OTG reads on real bootloaders, read-only inventory on A/B and non-A/B devices, and disconnect during transfers. Each USB connection requires Android's permission and exclusive interface ownership.
- Qualify the opt-in Rust STLS handshake on paired devices, connect the USB AUTH signer, and integrate Rust service streams with Android. Compare binary shell records, multi-file SYNC and recovery sideload against the installed Kotlin paths.
- Qualify native DATA/flash, fetch, logical-partition operations, filesystem generation, script execution and cancellation against the existing Android operation review and recovery behavior before switching their backends.
- Record device model, topology, transfer throughput, CPU, peak memory and outcomes for each hardware test. Test images, disposable partitions and explicit device authorization are prerequisites for destructive validation.

## Historical checkpoints

The starter preserved the upstream history, license, notices and Android engines while introducing three dependency-free Rust crates. Publication-tool and mock-repository verification from the preparation phase is recorded in the [starter journal](https://github.com/Ncorror/NekoFlash-2.0-Rust/blob/7741dc03da6d745749cae2bcae4d56cfe20181ac/docs/RUST_PORT.md).

## 2026-11-25 — initial publication and CI corrections

- Published main `7741dc03da6d745749cae2bcae4d56cfe20181ac` to the private Rust repository. Termux compiled the workspace and passed 23/23 tests (8 ADB, 9 Fastboot, 5 inventory, 1 facade).
- First GitHub CI failed: Android revision-14 and i18n guards expected the original application ID; Rust Clippy rejected two constant-sized chunk iterators. Updated the guards to require the independent `.rust` ID, and changed the ADB header codec to fixed-size array chunk iteration without relaxing validation or lint rules.
- CI rerun, Android APK, external workspace separation, JNI integration and hardware parity remain unverified.

## 2026-11-25 — fork isolation and build audit

- At 3d2a085 the Rust workflow passed. The Android job passed the repository checks, JVM tests and lint; its emulator commands and evidence collectors still addressed the upstream debug application. The fork now runs instrumentation, fixture providers and run-as collection under ru.forum.adbfastboottool.rust.dev. The Kotlin package and runner class remain compatible.
- The app, terminal link, imports, diagnostics, scoped file sharing and instrumentation use Download/NekoFlash-Rust. Existing public files stay in place. Private fallbacks remain per application ID. The fork's Termux checkout, publication defaults and maintenance scripts target the Rust repository. Keep the upstream checkout at f82484d for comparison with its push URL disabled.
- In the Linux checkout, all 23 Rust tests, locked all-target Clippy, rustfmt, REV27 and its predecessor guards, localization, documentation, tester-kit, diagnostic ZIP, branch-cleanup and repository-security checks passed. Rust CI now enforces formatting.
- Audit: Android already serializes Fastboot transactions and owns interface permissions, USB detach, SAF, and the cancellable USBFS URB queue. ADB has a single packet dispatcher and existing pairing/TLS and identity lifetimes. Sparse and filesystem tools have independent installed-APK checks. Preserve those boundaries and keep the existing transfer engines available during device qualification.

## 2026-11-25 — Rust transport, Fastboot transactions, sparse and JNI

- Added deadline-aware TCP, a socket shutdown handle, and synchronous USBDEVFS_BULK on a duplicate of an already claimed Android descriptor. Dropping the adapter closes its duplicate. The legacy queued USBFS sender still owns production Fastboot DATA transfers and cancellation.
- ADB packet I/O handles fragmented reads, negotiated limits and checksums, USB transfer boundaries and explicit ZLPs. Fastboot v1 TCP handles the FB handshake and big-endian frame lengths. The session supports INFO/TEXT, getvar and bounded download/flash to an explicit partition; it requires two terminal acknowledgments and drops ambiguous sessions. These new session APIs are library paths pending physical-device qualification.
- Added constant-memory sparse structure inspection and a bounded RAW/FILL writer for aligned raw input. Android's verified mkfs generators remain separately executable. Rust computes ADB payload checksums from Kotlin via a small JNI library; the Kotlin fallback remains available. CMake builds the Rust archive for each selected ABI. Install the standard Rust targets with `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android` before running Gradle locally.
- Host verification: 28 Rust tests passed including a real fragmented TCP loopback, DATA mismatch, rejected flash after malformed acknowledgements, USB descriptor ownership and sparse truncation. Formatting, locked all-target Clippy and static archive builds for all four Android targets passed.
- Baseline Android run 45684640711 built both APKs. Upload returned “Artifact storage quota has been hit”. Emulator tests now execute against the APK on the build runner before uploads on both API 26 and 34. Publication still requires artifact quota. The installed-app suite now explicitly checks that the Rust JNI library loads and matches byte-wise checksum results. Signatures, release isolation and the existing filesystem and UI checks are retained.

## 2026-11-25 — ADB services and Android sparse validation

- Added USB CNXN/AUTH and Wireless CNXN/STLS connection state machines. Host signatures, public keys and TLS sockets are supplied by the platform. The wireless path rejects a plaintext CNXN on the TLS port. Service streams validate local/remote IDs, track WRTE/OKAY and CLSE, enforce deadlines and prevent reuse of an abandoned operation.
- Added Shell v2 with binary stdout/stderr and explicit EXIT, legacy shell, streaming SYNC SEND/RECV, and recovery-driven Sideload. Sideload bounds block indices, accounts for retries once, and reports transfer completion separately from installation. Kotlin still supplies the installed app's authenticated device session, pairing UI, keystore, dispatcher, interactive shell, directory walks and recovery verdict.
- JNI now inspects sparse files from a dedicated Android descriptor. Rust duplicates the descriptor, checks bounds before seeking over chunk payloads, and propagates Kotlin cancellation. Installed filesystem tests compare native inspection against the retained Kotlin implementation on ext4 and f2fs images.
- 34 locked Rust tests and all-target Clippy passed. New traces include USB ZLP, early remote close with buffered EXIT, wrong stream ID, mandatory STLS, binary SYNC and duplicate/out-of-range sideload blocks. Built a four-ABI debug APK locally with NDK 27.2.12479018. Local mkfs tools/assets and their corresponding sources matched every digest from the upstream f82484d build, checked by the existing pinned builder.

## 2026-11-26 — sparse output, tokenization and emulator capacity

- The ext4 and F2FS formatters now stream their verified sparse chunk plans into the staging file. This removes the separate full-image body and result buffers from the Android format path. The public byte-array APIs and golden image tests use the same encoder. Sparse holes retain DONT_CARE semantics, fill runs retain their explicit values and cancellation is checked between raw writes.
- Android Fastboot command tokenization now uses Rust with UTF-16 literals and unescaped argument values. Quoted empty arguments, OEM spelling, escaped spaces and unpaired surrogates are covered. The Kotlin parser remains the fallback.
- Run 45688082255 at 6670d00 passed all 566 JVM tests, lint and the APK build. API 26 could not allocate its default 7.2 GiB userdata partition on the build runner; API 34 then ran out of disk during installation. The workflow now requests 2 GiB userdata, frees disposable build intermediates and removes the first AVD and system image before installing the second. Both emulator jobs still run the complete installed suite. The separate artifact-storage quota error was also present in this run.

## 2026-11-26 — Android Fastboot queries and slot evidence

- The Android terminal can issue opt-in scalar getvar queries over either claimed OTG bulk endpoints or Fastboot TCP. USB shares the existing transaction lock and operation admission. TCP has a single active diagnostic query and a five-second deadline across connect, handshake and reply. A transport error after a USB request invalidates the session. An explicit device FAIL completes the request without resending it.
- Inventory iteration preserves the order of first observation, including CR-separated lines and Unicode names. Slot resolution uses the completed snapshot and concrete partition entries; missing or conflicting evidence prevents selection. Legacy unslotted and A/B fixtures are covered.
- Shell v2 now drains stdout/stderr while CLOSE_STDIN is being acknowledged. Closing a broken TCP connection releases the socket even if an unused cancellation handle survives.
- Local verification passed for Rust formatting, locked Clippy and 40 Rust tests, including fragmented TCP getvar and early-close shell. A JVM loaded the C++/Rust JNI library and exercised unsigned checksum, UTF-16 tokenization, sparse descriptor ownership and cancellation, and scoped getvar against a real loopback peer. Android built the debug and instrumentation APKs. All 566 JVM tests passed without skips and lint reported zero findings. The APK check verifies ELF ABI, 16 KiB alignment and exported Rust bridge entry points for every ABI. Physical device verification remains pending.
- Run [45774333756](https://github.com/Ncorror/NekoFlash-2.0-Rust/actions/runs/45774333756) showed that both emulator images still require 7372.80 MiB free for userdata despite the requested size. Only 5392 MiB and 4899 MiB were available. CI now reclaims unused preinstalled SDKs on the disposable hosted runner before building. The artifact service also reported an account storage quota error.

## 2026-11-26 — Wireless ADB through the Android TLS adapter

- Added an explicit terminal connection path through the Rust CNXN/STLS engine. Handshake callbacks run synchronously on the owning Android I/O thread. The dispatcher starts only after TLS completes and Rust validates the device banner. Protocol or TLS failures close the session. Connection admission refuses to interrupt an active device operation.
- Shared the existing four wireless scenarios between connection paths and now run them against the Rust bridge in installed Android tests. The suite covers pairing, shell, binary and empty-file SYNC pushes, bounded metadata probing, and two reboot disconnect orderings.
- Host checks passed for formatting, Clippy and 41 Rust tests. A Linux JNI build passed all four wireless scenarios over real loopback TLS with Android's pairing library. The standard host JVM suite passed all 566 tests with zero skips, and lint reported zero findings. Android-specific file descriptor tests run on the installed app.

## 2026-11-26 — ext4 group geometry

- Ported 4 KiB ext4 group sizing, descriptor and inode tables, sparse-super backups and short-tail trimming. The formatter uses the Rust result via JNI and keeps the Kotlin implementation for fallback and detailed errors. Sparse serialization still stages the complete validated image before any device transfer.
- Auditing the pinned e2fsprogs 1.47.0 source revealed an inode alignment error in the Kotlin implementation. Its mask cleared bit 3, while ext2fs_initialize clears the low three bits. The Rust implementation and the fork's fallback now agree with mke2fs. For 8200 blocks and 512-byte inodes, an independent dumpe2fs reported 8200 inodes per group and 1025 inode-table blocks.
- Added exact-value regressions and an installed JNI comparison across tiny layouts, group boundaries, inode profiles, and the 32-bit block limit.
- This revision passed rustfmt, locked all-target Clippy, 44 Rust tests and 567 Android JVM tests (zero failures or skips). Android lint reported zero findings, and both APKs built. The generated 8200-block ext4 image with project IDs and casefold passed all five e2fsck phases.
- On the previous Android run both API 26 and API 34 passed all 35 installed tests and the filesystem evidence checks after disk reclamation. The remaining failed steps reported the GitHub Actions artifact-storage quota. [Run 45820898152](https://github.com/Ncorror/NekoFlash-2.0-Rust/actions/runs/45820898152)

## 2026-11-26 — Android native library size

- Enabled section garbage collection for the Rust JNI shared library. The stripped library shrank from 1,363,640 to 558,464 bytes on arm64, 921,108 to 362,584 on armv7, 1,376,776 to 570,680 on x86 and 1,379,400 to 576,328 on x86_64. All four APK libraries retained their six JNI functions, 16 KiB ELF load alignment and system-library dependencies. Both debug APKs built, JVM tests and lint passed.
- A local Linux x86_64/JDK 17 microbenchmark exercised the same Android ADB checksum entry point with the JNI library enabled and with the Kotlin fallback. With warmed 64 KiB arrays it measured 8.75 and 1.21 GiB/s respectively (median of seven 128 MiB runs). The resulting sums matched. Device transport throughput still requires hardware measurements.
