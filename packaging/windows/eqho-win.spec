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
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    name="Eqho",
)
