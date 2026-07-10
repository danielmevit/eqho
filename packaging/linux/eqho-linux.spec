# PyInstaller onedir build for Linux.
# Run via packaging/linux/build.sh. Models download on first run (not bundled).

import re
from pathlib import Path

ROOT = Path(SPECPATH).resolve().parents[1]
VERSION = re.search(
    r'__version__ = "([^"]+)"',
    (ROOT / "src" / "version.py").read_text(encoding="utf-8"),
).group(1)

a = Analysis(
    [str(ROOT / "run.py")],
    pathex=[str(ROOT)],
    binaries=[],
    datas=[(str(ROOT / "assets"), "assets")],
    hiddenimports=[
        "pystray._xorg",
        "pystray._appindicator",
        "pystray._gtk",
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
