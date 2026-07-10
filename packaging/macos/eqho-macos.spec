# PyInstaller .app bundle for macOS.
# Run via packaging/macos/build.sh. Unsigned: first launch needs
# right-click -> Open (Gatekeeper). Models download on first run.

import re
from pathlib import Path

ROOT = Path(SPECPATH).resolve().parents[1]
VERSION = re.search(
    r'__version__ = "([^"]+)"',
    (ROOT / "src" / "version.py").read_text(encoding="utf-8"),
).group(1)

ICNS = ROOT / "assets" / "eqho.icns"

a = Analysis(
    [str(ROOT / "run.py")],
    pathex=[str(ROOT)],
    binaries=[],
    datas=[(str(ROOT / "assets"), "assets")],
    hiddenimports=[
        "pystray._darwin",
        "PIL._tkinter_finder",
        "faster_whisper",
        "ctranslate2",
        "platformdirs",
    ],
    hookspath=[],
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    exclude_binaries=True,
    name="Eqho",
    debug=False,
    strip=False,
    upx=False,
    console=False,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name="Eqho",
)

app = BUNDLE(
    coll,
    name="Eqho.app",
    icon=str(ICNS) if ICNS.exists() else None,
    bundle_identifier="com.danielmevit.eqho",
    version=VERSION,
    info_plist={
        "NSMicrophoneUsageDescription": "Eqho transcribes your speech locally — the microphone never leaves this Mac.",
        "LSUIElement": True,  # menu-bar app, no Dock icon
        "NSHighResolutionCapable": True,
    },
)
