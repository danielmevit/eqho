#!/usr/bin/env bash
# Build & package Eqho 0.9.x (Rust + Slint + whisper.cpp 1.8.4) for Linux x64:
#   - Compiles Eqho (--release --locked, fat LTO, stripped)
#   - Stages libwhisper.so* + libggml*.so* (Vulkan + All CPU ISA) + LICENSE + THIRD_PARTY_NOTICES.txt
#   - Runs Eqho --version and Eqho --smoke gate in an isolated config dir
#   - Produces dist/Eqho-<version>-linux-x64.tar.gz (+ .AppImage when appimagetool is present)
#     and dist/SHA256SUMS-<version>-linux-x64.txt
#
# Usage:
#   bash packaging/linux/build-rust.sh [--lib-dir <path>] [--out-dir <path>] [--skip-build]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_TOML="$ROOT/rust/Cargo.toml"
LIB_DIR=""
OUT_DIR="$ROOT/dist"
SKIP_BUILD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lib-dir) LIB_DIR="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

VERSION="$(grep -E '^\s*version\s*=\s*"' "$CARGO_TOML" | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
ARCH_LABEL="$([[ "$ARCH" == "x86_64" ]] && echo "x64" || echo "$ARCH")"
echo "=== Building Eqho Rust v${VERSION} (linux-${ARCH_LABEL}) ==="

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  CARGO_INCREMENTAL=0 cargo build --manifest-path "$CARGO_TOML" -p eqho-app --release --locked
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
BIN_SRC="$TARGET_DIR/release/Eqho"
if [[ ! -x "$BIN_SRC" ]]; then
  BIN_SRC="$TARGET_DIR/${ARCH}-unknown-linux-gnu/release/Eqho"
fi
if [[ ! -x "$BIN_SRC" ]]; then
  echo "error: built Eqho binary not found under $TARGET_DIR" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
BUNDLE_NAME="Eqho-${VERSION}-linux-${ARCH_LABEL}"
BUNDLE_DIR="$OUT_DIR/$BUNDLE_NAME"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/lib" "$BUNDLE_DIR/share/icons"

cp "$BIN_SRC" "$BUNDLE_DIR/Eqho"
chmod 755 "$BUNDLE_DIR/Eqho"
cp "$ROOT/LICENSE" "$BUNDLE_DIR/LICENSE"
cp "$ROOT/THIRD_PARTY_NOTICES.txt" "$BUNDLE_DIR/THIRD_PARTY_NOTICES.txt"
cp "$ROOT/assets/logo_64_dark.png" "$BUNDLE_DIR/share/icons/eqho.png"
cp "$ROOT/packaging/linux/eqho.desktop" "$BUNDLE_DIR/eqho.desktop"

HAS_WHISPER_LIBS=0
if [[ -n "$LIB_DIR" ]]; then
  if [[ ! -d "$LIB_DIR" ]]; then
    echo "error: --lib-dir does not exist: $LIB_DIR" >&2
    exit 1
  fi
  find "$LIB_DIR" -maxdepth 2 \( -name 'libwhisper.so*' -o -name 'libggml*.so*' \) -exec cp -a {} "$BUNDLE_DIR/lib/" \;
  if ls "$BUNDLE_DIR/lib"/libwhisper.so* >/dev/null 2>&1; then
    HAS_WHISPER_LIBS=1
    echo "[ok] Bundled $(ls "$BUNDLE_DIR/lib"/*.so* | wc -l) whisper.cpp/ggml shared libraries"
  fi
fi

# Smoke gate in isolated temp config dir
ISO_CFG="$(mktemp -d)"
trap 'rm -rf "$ISO_CFG"' EXIT
VER_OUT="$(EQHO_CONFIG_DIR="$ISO_CFG" "$BUNDLE_DIR/Eqho" --version)"
echo "[version] $VER_OUT"
if [[ "$HAS_WHISPER_LIBS" -eq 1 ]]; then
  EQHO_CONFIG_DIR="$ISO_CFG" "$BUNDLE_DIR/Eqho" --smoke --dll-dir "$BUNDLE_DIR/lib"
else
  echo "[warn] No --lib-dir supplied; skipping live libwhisper.so inference check in --smoke"
  EQHO_CONFIG_DIR="$ISO_CFG" "$BUNDLE_DIR/Eqho" --smoke || true
fi

# Portable tar.gz archive
TAR_PATH="$OUT_DIR/${BUNDLE_NAME}.tar.gz"
rm -f "$TAR_PATH"
tar -C "$OUT_DIR" -czf "$TAR_PATH" "$BUNDLE_NAME"
echo "[ok] Portable archive: $TAR_PATH"
ARTIFACTS=("$TAR_PATH")

# Optional AppImage when appimagetool is available
if command -v appimagetool >/dev/null 2>&1; then
  APPDIR="$OUT_DIR/Eqho.AppDir"
  rm -rf "$APPDIR"
  mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib"
  cp "$BUNDLE_DIR/Eqho" "$APPDIR/usr/bin/Eqho"
  if [[ "$HAS_WHISPER_LIBS" -eq 1 ]]; then
    cp -a "$BUNDLE_DIR/lib"/* "$APPDIR/usr/lib/"
  fi
  cp "$ROOT/assets/logo_64_dark.png" "$APPDIR/eqho.png"
  cp "$ROOT/packaging/linux/eqho.desktop" "$APPDIR/eqho.desktop"
  cat > "$APPDIR/AppRun" <<'APPRUN'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "$0")")"
export EQHO_WHISPER_DLL_DIR="$HERE/usr/lib"
export LD_LIBRARY_PATH="$HERE/usr/lib:${LD_LIBRARY_PATH:-}"
exec "$HERE/usr/bin/Eqho" "$@"
APPRUN
  chmod 755 "$APPDIR/AppRun"
  APPIMAGE_PATH="$OUT_DIR/${BUNDLE_NAME}.AppImage"
  rm -f "$APPIMAGE_PATH"
  APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" appimagetool "$APPDIR" "$APPIMAGE_PATH"
  rm -rf "$APPDIR"
  echo "[ok] AppImage: $APPIMAGE_PATH"
  ARTIFACTS+=("$APPIMAGE_PATH")
else
  echo "[info] appimagetool not found — skipping AppImage (tar.gz built cleanly)"
fi

SUMS_PATH="$OUT_DIR/SHA256SUMS-${VERSION}-linux-${ARCH_LABEL}.txt"
(cd "$OUT_DIR" && sha256sum "${ARTIFACTS[@]##*/}") > "$SUMS_PATH"
echo "[ok] Checksums: $SUMS_PATH"
cat "$SUMS_PATH"
