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
