"""Check that every tools/probe_*.py leaves the real Eqho config folder untouched.

Records size + mtime of eqho.log, settings.json and history.jsonl in the real
config folder, runs each probe in a subprocess (with EQHO_CONFIG_DIR unset, the
way a person would run it), then fails if any of those files changed.
"""
import os
import subprocess
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent
WATCHED = ("eqho.log", "settings.json", "history.jsonl")


def _real_config_dir() -> Path:
    # Same rule as src/settings.py, without importing src.
    import platformdirs
    return Path(platformdirs.user_config_dir("Eqho", appauthor=False, roaming=True))


def _snapshot(folder: Path) -> dict:
    snap = {}
    for name in WATCHED:
        p = folder / name
        if p.exists():
            st = p.stat()
            snap[name] = (st.st_size, st.st_mtime_ns)
        else:
            snap[name] = None
    return snap


def _run(probe: Path, env: dict):
    try:
        return subprocess.run([sys.executable, str(probe)], cwd=ROOT, env=env,
                              capture_output=True, text=True, timeout=600).returncode
    except subprocess.TimeoutExpired:
        return "timeout"


def main() -> int:
    real = _real_config_dir()
    print(f"real config folder: {real}")
    env = {k: v for k, v in os.environ.items() if k != "EQHO_CONFIG_DIR"}
    failed = False
    for probe in sorted(TOOLS.glob("probe_*.py")):
        # Something else (the running app, another checkout's probes) can write
        # the real log at the same moment; retry once before blaming the probe.
        for attempt in (1, 2):
            before = _snapshot(real)
            rc = _run(probe, env)
            after = _snapshot(real)
            changed = [n for n in WATCHED if before[n] != after[n]]
            if not changed:
                break
        status = "FAIL" if changed else "ok"
        failed |= bool(changed)
        extra = f" changed: {', '.join(changed)}" if changed else ""
        retried = " (retried)" if attempt == 2 else ""
        print(f"{status:4} {probe.name} (exit {rc}){retried}{extra}")
    print("FAIL: real config touched" if failed else "PASS: real config untouched")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
