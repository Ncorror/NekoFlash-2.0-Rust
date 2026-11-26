#!/usr/bin/env bash
# Run on the build host against the emulator selected by the workflow.
set -euo pipefail
project="$(cd "$(dirname "$0")/.." && pwd)"
api="$(adb shell getprop ro.build.version.sdk | tr -d '\r')"
[[ "$api" == "$1" ]]
evidence="$project/build/android-evidence/api$api"
mkdir -p "$evidence"
cd "$evidence"
trap 'adb logcat -d > android-instrumentation-logcat.txt 2>&1 || true' EXIT
adb install "$project/app/build/outputs/apk/debug/app-debug.apk"
adb install "$project/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
adb logcat -c || true
adb shell am instrument -w -r ru.forum.adbfastboottool.rust.dev.test/ru.forum.adbfastboottool.NekoFlashAndroidTestRunner | tee android-mkfs-tests.txt
adb logcat -d > android-instrumentation-logcat.txt 2>&1 || true
mkdir -p android-source-ui
adb exec-out run-as ru.forum.adbfastboottool.rust.dev cat files/source-test-output/quick-restored.png > android-source-ui/quick-restored.png
adb exec-out run-as ru.forum.adbfastboottool.rust.dev cat files/source-test-output/quick-unavailable.png > android-source-ui/quick-unavailable.png
adb exec-out run-as ru.forum.adbfastboottool.rust.dev cat files/source-test-output/quick-partition-menu.png > android-source-ui/quick-partition-menu.png
adb exec-out run-as ru.forum.adbfastboottool.rust.dev cat files/source-test-output/mass-saf-queue.png > android-source-ui/mass-saf-queue.png
python3 -c "from pathlib import Path; p=Path('android-source-ui'); assert all((p/n).stat().st_size > 0 for n in ('quick-restored.png', 'quick-unavailable.png', 'quick-partition-menu.png', 'mass-saf-queue.png'))"
python3 "$project/tests/collect_android_ui.py" --output android-interface-ui
python3 -c "from pathlib import Path; s=Path('android-mkfs-tests.txt').read_text(); assert 'OK (34 tests)' in s and 'FAILURES' not in s, s"
python3 "$project/tests/verify_android_filesystems.py" --output android-mkfs-images
