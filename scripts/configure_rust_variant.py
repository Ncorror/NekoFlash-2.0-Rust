#!/usr/bin/env python3
"""One-time variant configuration, checked against the pinned upstream tree."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]

def replace_once(path, old, new):
    text = path.read_text(encoding='utf-8')
    if text.count(old) != 1:
        raise SystemExit(f'Unexpected baseline: {path}: {old!r}')
    path.write_text(text.replace(old, new), encoding='utf-8')

app = ROOT / 'app/build.gradle'
replace_once(app, 'applicationId = "ru.forum.adbfastboottool"', 'applicationId = "ru.forum.adbfastboottool.rust"')
# Keep versionName and BUILD_ID synchronized; original debug suffix is retained.
replace_once(app, 'versionName = "6.0.0-alpha15"', 'versionName = "6.0.0-alpha15-rust.1"')
replace_once(app, '6.0.0-alpha15+${buildCommit.take(12)}', '6.0.0-alpha15-rust.1+${buildCommit.take(12)}')
for path in sorted((ROOT / 'app/src/main/res').glob('values*/strings.xml')):
    text = path.read_text(encoding='utf-8')
    changed, count = re.subn(r'(<string\s+name="app_name"[^>]*>)[^<]*(</string>)', r'\g<1>NekoFlash 2.0 Rust\g<2>', text)
    if count > 1:
        raise SystemExit(f'Duplicate app_name in {path}')
    if count:
        path.write_text(changed, encoding='utf-8')
# Preserve original audited signing code and tests; skip signing jobs in this experiment.
build = ROOT / '.github/workflows/build.yml'
replace_once(build, '  build:\n    runs-on:', "  build:\n    if: ${{ github.repository == 'Ncorror/NekoFlash-2.0' || github.event_name != 'workflow_dispatch' || !inputs.sign_release }}\n    runs-on:")
release = ROOT / '.github/workflows/release.yml'
replace_once(release, '  tag-provenance:\n', "  tag-provenance:\n    if: ${{ github.repository == 'Ncorror/NekoFlash-2.0' }}\n")
print('PASS: configured independent application ID, label, version and upstream-only signing jobs')
