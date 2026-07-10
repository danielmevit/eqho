#!/usr/bin/env bash
# Build Eqho for Linux: onedir bundle -> tar.gz, plus an AppImage when
# appimagetool is available (best-effort; the tar.gz always ships).
# Runtime deps documented in README: libportaudio2, xclip,
# gir1.2-ayatanaappindicator3 (tray), and an X11 session for hotkeys/injection.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(grep -o '__version__ = "[^"]*"' "$ROOT/src/version.py" | cut -d'"' -f2)"
PY="$ROOT/venv/bin/python"
[ -x "$PY" ] || PY=python3

echo "=== Building Eqho v$VERSION (linux) ==="
"$PY" -m pip install pyinstaller --quiet --disable-pip-version-check
"$PY" -m PyInstaller "$ROOT/packaging/linux/eqho-linux.spec" --noconfirm \
    --distpath "$ROOT/dist" --workpath "$ROOT/build"

tar -czf "$ROOT/dist/Eqho-linux-$VERSION.tar.gz" -C "$ROOT/dist" Eqho
echo "tar.gz: dist/Eqho-linux-$VERSION.tar.gz"

# -- AppImage (best-effort) ----------------------------------------------------
APPDIR="$ROOT/build/Eqho.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr"
cp -r "$ROOT/dist/Eqho" "$APPDIR/usr/eqho"
cp "$ROOT/packaging/linux/eqho.desktop" "$APPDIR/eqho.desktop"
cp "$ROOT/assets/logo_62_dark.png" "$APPDIR/eqho.png"
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/eqho/Eqho" "$@"
EOF
chmod +x "$APPDIR/AppRun"

TOOL="$(command -v appimagetool || true)"
if [ -n "$TOOL" ]; then
    ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" "$APPDIR" \
        "$ROOT/dist/Eqho-$VERSION-x86_64.AppImage" \
        && echo "AppImage: dist/Eqho-$VERSION-x86_64.AppImage" \
        || echo "appimagetool failed — tar.gz still available"
else
    echo "appimagetool not found — skipping AppImage (tar.gz still available)"
fi
