# NekoFlash 2.0 Rust — architecture and migration journal

Baseline: [Ncorror/NekoFlash-2.0 @ f82484dd64fe](https://github.com/Ncorror/NekoFlash-2.0/tree/f82484dd64fe980137585ae613b6de0a0cc540ed).
Independent repository: `Ncorror/NekoFlash-2.0-Rust`. Preserve Git history, authorship, Apache-2.0 license, third-party notices, and existing hardware behavior. Upstream push is disabled in the experimental clone.

## Current layout (implemented)

- `app/`: existing Android UI, services, JNI/C++ and tests; no bulk relocation or rewrite.
- `rust/crates/adb/`: ADB checksum and version policy.
- `rust/crates/fastboot/`: DATA reply, download bounds, value parsers and getvar inventory.
- `rust/crates/core/`: facade exposing both protocol libraries to future platform adapters.
- `.github/workflows/rust-core.yml`: independent Rust checks.
- `docs/RUST_PORT.md`: single Rust migration journal.

Dependency direction: Android adapter -> Rust core facade -> independent protocol crates. Protocol crates have no Android dependencies and no unsafe code. No third-party Rust dependencies yet. All existing Android functionality remains the reference implementation; the Rust path is not wired into it yet.

## Planned modules (not empty placeholder crates)

| Layer | Responsibility | Dependency rule |
|---|---|---|
| Transport | USB and TCP byte IO, timeouts, disconnect, cancel | Never depends on ADB or Fastboot |
| ADB | Framing, auth, dispatch, shell, SYNC, sideload, Wi-Fi pairing | Depends on transport contracts |
| Fastboot | Wire state, inventory, current slot, flash, fetch | Depends on transport contracts |
| Images | Sparse, boot/vbmeta, ext4/f2fs, archive streaming | Independent of transport and UI |
| Operations | Flash queues, script engine, cancellation, progress, recovery | Uses protocols and images |
| Android bridge | JNI, descriptor lifecycle, event mapping | Uses core; unsafe only at necessary FFI boundaries |
| Android app | UI, SAF, Android USB permission, lifecycle, services, localization | Calls bridge; no protocol duplication after cutover |

Retain Android-owned USB permissions and SAF; do not assume direct access to /dev/bus/usb or filesystem paths. Define ownership and cancellation for transferred file descriptors; never close the Android-owned descriptor from Rust. Keep secrets out of logs. Preserve current-slot discovery and non-A/B handling: never synthesize partitions. Keep the original Kotlin engines available until equivalent Rust tests and hardware verification pass. Preserve Mi account/unlock flows, shell semantics and all current professional operations. Avoid premature generic abstractions and dependencies.

## Status (starter v3)

- Four initial policy ports and Fastboot getvar inventory retained. ADB 24-byte framing and Fastboot reply decoding added, separate from transports.
- 15 prior tests + 5 ADB header tests + 3 Fastboot response tests (23 total, written, not executed in preparation environment).
- Rust compiler unavailable in preparation environment. Cargo.lock constructed for the dependency-free workspace; confirm using Cargo in CI/Termux.
- Android source layout and namespace deliberately preserved; publication script assigns independent applicationId `.rust`, separate label and version, so debug `.rust.dev` does not replace original `.dev`. Existing release and manual signing jobs are guarded to the upstream repository. Shared external workspace paths and application-ID-dependent behavior still need review before hardware use. Android build and app ID-dependent tests still require verification.
- Rust JNI integration and APK are not yet implemented. No claim of Rust-powered APK or hardware parity.
- Repository has not been created by this package until publish.sh successfully runs.

## Next milestone

1. Run workspace tests and clippy; compare inventory against original Kotlin behavior, including ordering/Unicode/numeric corner cases. Current Rust map is sorted rather than Kotlin insertion-ordered, and case normalization is ASCII; no UI consumer yet.
2. Extend initial Fastboot wire and ADB framing into command/stream state machines; add recorded protocol fixtures, fragmented IO and cancellation tests.
3. Implement transport abstraction and Android file descriptor adapter; keep legacy backend for A/B tests.
4. Integrate a small JNI vertical slice (`getvar` and device discovery) into Android; then transfer, shell, SYNC and sideload.
5. Move images, script engine, operation orchestration and diagnostics in tested slices. Keep platform-specific account/UI logic where appropriate; evaluate further Rust ports after feature parity.
6. Compare USB/ADB-Wi-Fi hardware behavior, throughput, CPU, memory, APK size; validate non-A/B and A/B devices, format/wipe and recovery flows before releases.

## Publication recovery

The publisher stages a fresh clone in a temporary directory, preserves it on failure, and only moves a committed prepared checkout to the final path. A retry resumes a clean, recognized local starter commit and verifies the exact remote main. It never force-pushes or overwrites an unrelated repository. If repository creation succeeded but local origin setup failed, inspect the target before attaching it manually.

## Preparation verification

Local package verification passed: Bash syntax, Python syntax, TOML parsing, workspace/lock membership, CI YAML parsing, test declaration count, variant transformation and mocked GitHub publication with a local bare repository, repeated invocation, remote divergence rejection and unchanged upstream. The local publication simulation uses a small synthetic upstream fixture and a mocked `gh`; it is not a real GitHub publication or Android build. No Rust compiler was installed, and rustup download was unavailable (DNS failure). No Rust tests, Clippy, Android build or hardware tests are claimed to have passed. The original app continues using its Kotlin/C++ implementation until JNI migration. Review original external storage paths, Android permissions, app-ID-specific tests, and Rust-vs-Kotlin parsing corner cases before parallel hardware testing.

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
