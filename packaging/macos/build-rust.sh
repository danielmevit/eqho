#!/usr/bin/env bash
# Build & package Eqho 0.9.x (Rust + Slint + whisper.cpp 1.8.4 Metal) for macOS (arm64 / x64):
#   - Compiles Eqho (--release --locked, fat LTO, stripped)
#   - Assembles Eqho.app with Info.plist, Eqho.icns, and Frameworks/libwhisper*.dylib + libggml*.dylib (Metal + Accelerate + CPU)
#   - Ad-hoc codesigns Eqho.app and runs Eqho --version / --smoke gate
#   - Packages drag-to-Applications Eqho-<version>-macos-<arch>.dmg + SHA256SUMS
#
# Usage:
#   bash packaging/macos/build-rust.sh --lib-dir <path> [--target aarch64-apple-darwin|x86_64-apple-darwin] [--out-dir <path>] [--skip-build]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_TOML="$ROOT/rust/Cargo.toml"
LIB_DIR=""
OUT_DIR="$ROOT/dist"
RUST_TARGET=""
SKIP_BUILD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lib-dir) LIB_DIR="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --target) RUST_TARGET="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

VERSION="$(grep -E '^\s*version\s*=\s*"' "$CARGO_TOML" | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')"
HOST_ARCH="$(uname -m)"
if [[ -z "$RUST_TARGET" ]]; then
  if [[ "$HOST_ARCH" == "arm64" ]]; then
    RUST_TARGET="aarch64-apple-darwin"
  else
    RUST_TARGET="x86_64-apple-darwin"
  fi
fi
ARCH_LABEL="$([[ "$RUST_TARGET" == aarch64-* ]] && echo "arm64" || echo "x64")"
echo "=== Building Eqho Rust v${VERSION} (macos-${ARCH_LABEL}, target=${RUST_TARGET}) ==="

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  CARGO_INCREMENTAL=0 cargo build --manifest-path "$CARGO_TOML" -p eqho-app --release --locked --target "$RUST_TARGET"
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
BIN_SRC="$TARGET_DIR/$RUST_TARGET/release/Eqho"
if [[ ! -x "$BIN_SRC" ]]; then
  BIN_SRC="$TARGET_DIR/release/Eqho"
fi
if [[ ! -x "$BIN_SRC" ]]; then
  echo "error: built Eqho binary not found under $TARGET_DIR" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
APP_BUNDLE="$OUT_DIR/Eqho.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Frameworks" "$APP_BUNDLE/Contents/Resources"

cp "$BIN_SRC" "$APP_BUNDLE/Contents/MacOS/Eqho"
chmod 755 "$APP_BUNDLE/Contents/MacOS/Eqho"
cp "$ROOT/assets/eqho.icns" "$APP_BUNDLE/Contents/Resources/Eqho.icns"
cp "$ROOT/LICENSE" "$APP_BUNDLE/Contents/Resources/LICENSE.txt"
cp "$ROOT/THIRD_PARTY_NOTICES.txt" "$APP_BUNDLE/Contents/Resources/THIRD_PARTY_NOTICES.txt"

if [[ -n "$LIB_DIR" && -d "$LIB_DIR" ]]; then
  find "$LIB_DIR" -maxdepth 2 \( -name 'libwhisper*.dylib' -o -name 'libggml*.dylib' \) -exec cp -a {} "$APP_BUNDLE/Contents/Frameworks/" \;
  echo "[ok] Bundled $(ls "$APP_BUNDLE/Contents/Frameworks"/*.dylib | wc -l | tr -d ' ') whisper.cpp/ggml dylibs"
else
  echo "error: --lib-dir with compiled libwhisper.dylib + libggml*.dylib (Metal) is required" >&2
  exit 1
fi

# Add @executable_path/../Frameworks rpath if not already present.
install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP_BUNDLE/Contents/MacOS/Eqho" 2>/dev/null || true

cat > "$APP_BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Eqho</string>
  <key>CFBundleDisplayName</key>
  <string>Eqho</string>
  <key>CFBundleIdentifier</key>
  <string>xyz.damt.eqho</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleExecutable</key>
  <string>Eqho</string>
  <key>CFBundleIconFile</key>
  <string>Eqho.icns</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>Eqho needs microphone access to transcribe your speech locally on your Mac. Audio never leaves your device.</string>
</dict>
</plist>
PLIST

# Ad-hoc sign so Apple Silicon executes cleanly without Gatekeeper invalid-signature kills.
codesign --force --deep --sign - "$APP_BUNDLE"

ISO_CFG="$(mktemp -d)"
trap 'rm -rf "$ISO_CFG"' EXIT
VER_OUT="$(EQHO_CONFIG_DIR="$ISO_CFG" "$APP_BUNDLE/Contents/MacOS/Eqho" --version)"
echo "[version] $VER_OUT"
EQHO_CONFIG_DIR="$ISO_CFG" "$APP_BUNDLE/Contents/MacOS/Eqho" --smoke --dll-dir "$APP_BUNDLE/Contents/Frameworks"

DMG_STAGE="$OUT_DIR/_dmg_stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
cp -R "$APP_BUNDLE" "$DMG_STAGE/Eqho.app"
ln -s /Applications "$DMG_STAGE/Applications"

DMG_NAME="Eqho-${VERSION}-macos-${ARCH_LABEL}.dmg"
DMG_PATH="$OUT_DIR/$DMG_NAME"
rm -f "$DMG_PATH"
hdiutil create -volname "Eqho ${VERSION}" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH"
rm -rf "$DMG_STAGE"
echo "[ok] DMG: $DMG_PATH"

SUMS_PATH="$OUT_DIR/SHA256SUMS-${VERSION}-macos-${ARCH_LABEL}.txt"
(cd "$OUT_DIR" && shasum -a 256 "$DMG_NAME") > "$SUMS_PATH"
echo "[ok] Checksums: $SUMS_PATH"
cat "$SUMS_PATH"
