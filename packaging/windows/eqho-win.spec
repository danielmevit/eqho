# PyInstaller onedir build for Windows.
# Run via packaging/windows/build.ps1, or directly from the repo root:
#   pyinstaller packaging/windows/eqho-win.spec --noconfirm
# Onedir (not onefile): faster startup, and the Inno Setup installer packages
# the folder anyway. Whisper models are NOT bundled — downloaded on first run.

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
        "pystray._win32",
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
    upx=True,
    console=False,
    icon=str(ROOT / "assets" / "eqho.ico"),
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    name="Eqho",
)
