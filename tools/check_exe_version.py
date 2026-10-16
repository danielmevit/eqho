"""Check the Windows version resource of Eqho.exe against src/version.py.

    python tools/check_exe_version.py dist/Eqho/Eqho.exe   # Windows: read the exe
    python tools/check_exe_version.py --spec-only          # anywhere: check the tuple

The exe check reads the resource with ctypes (GetFileVersionInfoW /
VerQueryValueW), prints the fields, and exits 1 if FileVersion doesn't match
src/version.py. --spec-only imports nothing from PyInstaller: it derives the
same 4-number tuple packaging/windows/eqho-win.spec builds and checks it.
"""

import ctypes
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIELDS = ("ProductName", "FileDescription", "FileVersion", "ProductVersion",
          "CompanyName", "LegalCopyright", "OriginalFilename")


def version_tuple():
    """Same derivation as eqho-win.spec: first 4 numbers, zero-padded."""
    text = (ROOT / "src" / "version.py").read_text(encoding="utf-8")
    version = re.search(r'__version__ = "([^"]+)"', text).group(1)
    nums = [int(n) for n in re.findall(r"\d+", version)[:4]]
    return version, tuple(nums + [0] * (4 - len(nums)))


def read_fields(exe):
    ver = ctypes.WinDLL("version")
    size = ver.GetFileVersionInfoSizeW(str(exe), None)
    if not size:
        return {}
    buf = ctypes.create_string_buffer(size)
    if not ver.GetFileVersionInfoW(str(exe), 0, size, buf):
        return {}
    ptr, length = ctypes.c_void_p(), ctypes.c_uint()
    ver.VerQueryValueW(buf, r"\VarFileInfo\Translation", ctypes.byref(ptr), ctypes.byref(length))
    lang, cp = (0x0409, 1200)
    if length.value >= 4:
        lang, cp = ctypes.cast(ptr, ctypes.POINTER(ctypes.c_ushort * 2)).contents
    fields = {}
    for name in FIELDS:
        sub = rf"\StringFileInfo\{lang:04X}{cp:04X}\{name}"
        if ver.VerQueryValueW(buf, sub, ctypes.byref(ptr), ctypes.byref(length)) and length.value:
            fields[name] = ctypes.wstring_at(ptr, length.value).rstrip("\0")
    return fields


def main(argv):
    version, vt = version_tuple()
    expected = ".".join(map(str, vt))
    if len(vt) != 4 or not all(0 <= n <= 0xFFFF for n in vt) or not any(vt):
        print(f"FAIL: src/version.py {version!r} -> invalid version tuple {vt}")
        return 1
    print(f"src/version.py {version} -> {vt}")
    if argv == ["--spec-only"]:
        print("OK: spec version tuple is valid")
        return 0
    if len(argv) != 1:
        print(__doc__)
        return 2
    if sys.platform != "win32":
        print("FAIL: reading an exe's version resource needs Windows (use --spec-only)")
        return 2
    fields = read_fields(Path(argv[0]))
    for name in FIELDS:
        print(f"  {name}: {fields.get(name, '<missing>')}")
    if fields.get("FileVersion") != expected:
        print(f"FAIL: FileVersion {fields.get('FileVersion')!r} != {expected!r}")
        return 1
    print("OK: FileVersion matches src/version.py")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
