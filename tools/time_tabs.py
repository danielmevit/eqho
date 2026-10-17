"""Time how long each dashboard tab takes to build (first visit) and to show again.

Copies the owner's settings into a temporary EQHO_CONFIG_DIR (nothing is written to the real folder), opens the
dashboard (a window appears for ~15 s) and prints seconds per tab. Optional 2nd argument: a tab key to cProfile.
Waits for the dashboard's background tab prewarm first; --cold turns the prewarm off (tabs build on first click).

    venv/Scripts/python.exe tools/time_tabs.py . [models] [--cold]
"""
import cProfile, io, json, os, pstats, shutil, sys, tempfile, time
from pathlib import Path

REPO = Path(sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] != "--cold" else Path(__file__).resolve().parents[1])
sys.path.insert(0, str(REPO))
tmp = Path(tempfile.mkdtemp(prefix="eqho_tabtime_"))
real = Path(os.environ["APPDATA"]) / "Eqho" / "settings.json"
data = json.loads(real.read_text(encoding="utf-8-sig")) if real.exists() else {}
data["onboarding_done"] = True
(tmp / "settings.json").write_text(json.dumps(data), encoding="utf-8")
os.environ["EQHO_CONFIG_DIR"] = str(tmp)   # real settings copied; nothing written to the real folder

import customtkinter as ctk
from src.fonts import load_fonts
from src.settings import Settings
from src.ui.dashboard import Dashboard

def pump(root, n=20):
    for _ in range(n):
        root.update()
        time.sleep(0.01)

cold = "--cold" in sys.argv          # --cold: no background prewarm, tabs build on their first click
argv = [a for a in sys.argv if a != "--cold"]
if cold:
    Dashboard._schedule_prewarm = lambda self: None
load_fonts()
root = ctk.CTk(); root.withdraw()
settings = Settings.load()
t0 = time.perf_counter()
dash = Dashboard(settings=settings, on_settings_changed=lambda *a, **k: None, parent=root, initial_tab="general")
root.update()
print(f"open dashboard + General: {time.perf_counter() - t0:.2f}s")
pump(root)
if not cold:                         # a user takes a few seconds before the first click
    t0 = time.perf_counter()
    def warming():
        cols = dash._get_col_count()
        return dash._warming or getattr(dash, "_resize_job", None) or any(
            dash._tab_built_cols.get(k) != cols for k in ("models", "history", "settings"))
    while warming() and time.perf_counter() - t0 < 20:
        root.update(); time.sleep(0.005)
    print(f"background prewarm done after {time.perf_counter() - t0:.2f}s")
prof_target = argv[2] if len(argv) > 2 else ""
for key in ("models", "history", "settings", "general", "models", "history", "settings"):
    pr = cProfile.Profile() if key == prof_target and key not in dash._tab_frames else None
    t0 = time.perf_counter()
    if pr: pr.enable()
    dash._show_tab(key); root.update()
    if pr:
        pr.disable()
        s = io.StringIO(); pstats.Stats(pr, stream=s).sort_stats("cumulative").print_stats(18); print(s.getvalue()[:3500])
    print(f"show {key:9s}: {time.perf_counter() - t0:.2f}s")
    pump(root, 5)
dash.destroy(); root.destroy()
shutil.rmtree(tmp, ignore_errors=True)
