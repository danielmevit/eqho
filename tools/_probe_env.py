"""Isolate probes from the owner's real config folder (EQ-PROBEENV).

Import this before any ``src`` import. If EQHO_CONFIG_DIR is not set, it points
it at a fresh temporary folder (removed at exit), so probes never write to the
real %APPDATA%\\Eqho (eqho.log, settings.json, history.jsonl).
"""
import atexit
import os
import shutil
import tempfile

if not os.environ.get("EQHO_CONFIG_DIR"):
    _TMP = tempfile.mkdtemp(prefix="eqho_probe_")
    os.environ["EQHO_CONFIG_DIR"] = _TMP
    atexit.register(shutil.rmtree, _TMP, ignore_errors=True)
