#!/usr/bin/env bash
# Build Eqho for macOS: .app bundle -> compressed .dmg (unsigned).
# First launch: right-click -> Open to pass Gatekeeper; then grant
# Accessibility + Input Monitoring in Privacy & Security for hotkeys/typing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(grep -o '__version__ = "[^"]*"' "$ROOT/src/version.py" | cut -d'"' -f2)"
PY="$ROOT/venv/bin/python"
[ -x "$PY" ] || PY=python3

echo "=== Building Eqho v$VERSION (macOS) ==="
"$PY" -m pip install pyinstaller --quiet --disable-pip-version-check
"$PY" -m PyInstaller "$ROOT/packaging/macos/eqho-macos.spec" --noconfirm \
    --distpath "$ROOT/dist" --workpath "$ROOT/build"

DMG="$ROOT/dist/Eqho-macos-$VERSION.dmg"
rm -f "$DMG"
hdiutil create -volname "Eqho" -srcfolder "$ROOT/dist/Eqho.app" -ov -format UDZO "$DMG"
echo "dmg: $DMG"
