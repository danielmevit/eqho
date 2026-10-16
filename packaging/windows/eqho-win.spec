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

# --- Windows version resource (Properties -> Details) ------------------------
# Generated from VERSION so there is no separate version file to keep in sync.
# tools/check_exe_version.py verifies the built exe (CI) and this tuple (--spec-only).
from PyInstaller.utils.win32.versioninfo import (
    FixedFileInfo, StringFileInfo, StringStruct, StringTable,
    VarFileInfo, VarStruct, VSVersionInfo,
)

_vnums = [int(n) for n in re.findall(r"\d+", VERSION)[:4]]
VERSION_TUPLE = tuple(_vnums + [0] * (4 - len(_vnums)))
_vstr = ".".join(str(n) for n in VERSION_TUPLE)


def _version_info(original_filename):
    return VSVersionInfo(
        ffi=FixedFileInfo(filevers=VERSION_TUPLE, prodvers=VERSION_TUPLE),
        kids=[
            StringFileInfo([StringTable("040904B0", [
                StringStruct("CompanyName", "Daniel Mevit"),
                StringStruct("FileDescription", "Eqho \u2014 offline voice to text"),
                StringStruct("FileVersion", _vstr),
                StringStruct("InternalName", "Eqho"),
                StringStruct("LegalCopyright", "\u00a9 2026 Daniel Mevit \u2014 AGPL-3.0"),
                StringStruct("OriginalFilename", original_filename),
                StringStruct("ProductName", "Eqho"),
                StringStruct("ProductVersion", _vstr),
            ])]),
            VarFileInfo([VarStruct("Translation", [0x0409, 1200])]),
        ],
    )

# --- pywhispercpp (whisper.cpp / Vulkan backend) ------------------------------
# _pywhispercpp is a TOP-LEVEL extension module; its whisper/ggml DLLs sit beside
# it in site-packages (self-contained wheel built by tools/mkwheel.py). Bundle
# ALL FIVE explicitly: PyInstaller's static PE analysis misses ggml-vulkan.dll
# and ggml-cpu.dll because ggml.dll LoadLibrary's the backend DLLs at runtime.
# If pywhispercpp isn't installed the build still succeeds — the app falls back
# to faster-whisper (engine_backend "auto").
_pwcpp_binaries = []
_pwcpp_hiddenimports = []
try:
    import importlib.util as _ilu
    _pw_spec = _ilu.find_spec("_pywhispercpp")
    if _pw_spec and _pw_spec.origin:
        _pw_dir = Path(_pw_spec.origin).parent
        for _dll in ("whisper.dll", "ggml.dll", "ggml-base.dll",
                     "ggml-cpu.dll", "ggml-vulkan.dll"):
            _p = _pw_dir / _dll
            if _p.exists():
                _pwcpp_binaries.append((str(_p), "."))
        _pwcpp_hiddenimports = [
            "pywhispercpp", "pywhispercpp.model", "pywhispercpp.constants",
            "pywhispercpp.utils", "_pywhispercpp", "requests", "tqdm",
        ]
        print(f"[eqho-spec] pywhispercpp bundled ({len(_pwcpp_binaries)} DLLs)")
    else:
        print("[eqho-spec] pywhispercpp not installed — whisper.cpp not bundled")
except Exception as _e:
    print(f"[eqho-spec] pywhispercpp bundling skipped: {_e}")

a = Analysis(
    [str(ROOT / "run.py")],
    pathex=[str(ROOT)],
    binaries=_pwcpp_binaries,
    datas=[(str(ROOT / "assets"), "assets")],
    hiddenimports=[
        "pystray._win32",
        "PIL._tkinter_finder",
        "faster_whisper",
        "ctranslate2",
        "platformdirs",
        "src.model_host",
        *_pwcpp_hiddenimports,
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
    version=_version_info("Eqho.exe"),
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    name="Eqho",
)
