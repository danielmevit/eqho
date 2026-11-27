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

# Native .deb package (Ubuntu / Debian / Linux Mint) when dpkg-deb is available
if command -v dpkg-deb >/dev/null 2>&1; then
  DEB_ARCH="$([[ "$ARCH" == "x86_64" ]] && echo "amd64" || echo "$ARCH")"
  DEB_STAGE="$OUT_DIR/_deb_stage"
  rm -rf "$DEB_STAGE"
  mkdir -p \
    "$DEB_STAGE/DEBIAN" \
    "$DEB_STAGE/usr/bin" \
    "$DEB_STAGE/usr/lib/eqho/lib" \
    "$DEB_STAGE/usr/share/applications" \
    "$DEB_STAGE/usr/share/icons/hicolor/64x64/apps" \
    "$DEB_STAGE/usr/share/doc/eqho"

  cp "$BUNDLE_DIR/Eqho" "$DEB_STAGE/usr/lib/eqho/Eqho"
  chmod 755 "$DEB_STAGE/usr/lib/eqho/Eqho"
  if [[ "$HAS_WHISPER_LIBS" -eq 1 ]]; then
    cp -a "$BUNDLE_DIR/lib"/* "$DEB_STAGE/usr/lib/eqho/lib/"
  fi
  cat > "$DEB_STAGE/usr/bin/eqho" <<'WRAPPER'
#!/usr/bin/env bash
export EQHO_WHISPER_DLL_DIR="/usr/lib/eqho/lib"
export LD_LIBRARY_PATH="/usr/lib/eqho/lib:${LD_LIBRARY_PATH:-}"
exec /usr/lib/eqho/Eqho "$@"
WRAPPER
  chmod 755 "$DEB_STAGE/usr/bin/eqho"

  sed 's|^Exec=Eqho|Exec=/usr/bin/eqho|' "$ROOT/packaging/linux/eqho.desktop" \
    > "$DEB_STAGE/usr/share/applications/eqho.desktop"
  # Pad the 62x62 logo_64_dark.png to exact 64x64 for strict hicolor/64x64 icon compliance (fallback to cp).
  ICON_DST="$DEB_STAGE/usr/share/icons/hicolor/64x64/apps/eqho.png"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$ROOT/assets/logo_64_dark.png" "$ICON_DST" <<'PY' || cp "$ROOT/assets/logo_64_dark.png" "$ICON_DST"
import struct, sys, zlib
src, dst = sys.argv[1], sys.argv[2]
data = open(src, "rb").read()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    raise RuntimeError("not png")
pos = 8
w = h = bd = ct = 0
idat = bytearray()
while pos < len(data):
    ln = struct.unpack(">I", data[pos:pos+4])[0]
    tag = data[pos+4:pos+8]
    body = data[pos+8:pos+8+ln]
    pos += 12 + ln
    if tag == b"IHDR":
        w, h, bd, ct, _, _, _ = struct.unpack(">IIBBBBB", body)
    elif tag == b"IDAT":
        idat.extend(body)
if (w, h) == (64, 64) or bd != 8 or ct != 6 or w > 64 or h > 64:
    open(dst, "wb").write(data)
    sys.exit(0)
raw = zlib.decompress(bytes(idat))
bpp = 4
stride = w * bpp
rows = []
prev = bytearray(stride)
i = 0
for _ in range(h):
    ft = raw[i]; i += 1
    row = bytearray(raw[i:i+stride]); i += stride
    if ft == 1:
        for x in range(bpp, stride): row[x] = (row[x] + row[x-bpp]) & 0xFF
    elif ft == 2:
        for x in range(stride): row[x] = (row[x] + prev[x]) & 0xFF
    elif ft == 3:
        for x in range(stride):
            a = row[x-bpp] if x >= bpp else 0
            row[x] = (row[x] + ((a + prev[x]) >> 1)) & 0xFF
    elif ft == 4:
        for x in range(stride):
            a = row[x-bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x-bpp] if x >= bpp else 0
            p = a + b - c
            pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
            pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
            row[x] = (row[x] + pr) & 0xFF
    rows.append(bytes(row)); prev = row
ox = (64 - w) // 2
oy = (64 - h) // 2
out = bytearray()
blank = bytes(64 * 4)
for y in range(64):
    out.append(0)
    if oy <= y < oy + h:
        out.extend(bytes(ox * 4))
        out.extend(rows[y - oy])
        out.extend(bytes((64 - ox - w) * 4))
    else:
        out.extend(blank)
def chunk(tag, body):
    return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)
ihdr = struct.pack(">IIBBBBB", 64, 64, 8, 6, 0, 0, 0)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(out), 9)) + chunk(b"IEND", b"")
open(dst, "wb").write(png)
PY
  else
    cp "$ROOT/assets/logo_64_dark.png" "$ICON_DST"
  fi
  cp "$ROOT/LICENSE" "$DEB_STAGE/usr/share/doc/eqho/LICENSE"
  cp "$ROOT/THIRD_PARTY_NOTICES.txt" "$DEB_STAGE/usr/share/doc/eqho/THIRD_PARTY_NOTICES.txt"

  INSTALLED_KB="$(du -sk "$DEB_STAGE/usr" | awk '{print $1}')"
  cat > "$DEB_STAGE/DEBIAN/control" <<CONTROL
Package: eqho
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${DEB_ARCH}
Installed-Size: ${INSTALLED_KB}
Maintainer: Daniel Mitev <support@damt.xyz>
Homepage: https://eqho.damt.xyz/
Depends: libc6, libstdc++6, curl, libfontconfig1, libfreetype6, libpulse0, libxkbcommon0, libxkbcommon-x11-0
Recommends: libvulkan1, libdbus-1-3, libcairo2, libpangocairo-1.0-0, xclip, wl-clipboard, wtype
Description: Calm, 100% offline local voice-to-text dictation
 Press a hotkey, speak, and your words appear in any focused app.
 Runs locally via whisper.cpp with Vulkan GPU and multi-ISA CPU support.
CONTROL

  DEB_PATH="$OUT_DIR/${BUNDLE_NAME}.deb"
  rm -f "$DEB_PATH"
  dpkg-deb --root-owner-group --build "$DEB_STAGE" "$DEB_PATH"
  rm -rf "$DEB_STAGE"
  echo "[ok] Debian package: $DEB_PATH"
  ARTIFACTS+=("$DEB_PATH")
else
  echo "[info] dpkg-deb not found — skipping .deb package"
fi

SUMS_PATH="$OUT_DIR/SHA256SUMS-${VERSION}-linux-${ARCH_LABEL}.txt"
(cd "$OUT_DIR" && sha256sum "${ARTIFACTS[@]##*/}") > "$SUMS_PATH"
echo "[ok] Checksums: $SUMS_PATH"
cat "$SUMS_PATH"
