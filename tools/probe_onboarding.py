"""Probe: first-time setup (EQ-ONBOARD) — settings rules and first launch.

1. Fresh install (no settings.json) -> setup needed, volume_duck "off".
2. Existing settings.json without `onboarding_done` -> treated as done; a
   saved "mute" stays "mute"; a file without volume_duck keeps the old "mute".
3. recommend_model (graphics card faked): "en" -> distil-large-v3; "de" ->
   large-v3-turbo; no
   "detect automatically" language choice (planned for 0.8.11).
3b. Building the language page saves nothing (a chosen model is kept);
   picking a language saves it + the recommended model.
3d. Setup on a fresh install saves the model recommended for this PC;
   "Run setup again" leaves the model alone (EQ-PICK).
3c. Closing the window (X) during setup = "Skip setup": marked done.
4. App.run() on first launch opens setup and does NOT preload/download; with
   setup done it preloads.
5. Window fit (EQ-FIT): the dashboard's starting size fits the work area
   (above the taskbar) at common screen/scaling pairs, never below the
   minimum, centred in the work area (fake work-area numbers, no window). Fakes for overlay/tray/hotkeys/transcriber/UI —
   no window, no mic, no downloads.

One line per check. Exit 0 on success, 1 on failure:
    venv/Scripts/python.exe tools/probe_onboarding.py
"""

import json
import os
import sys
import tempfile
import time
from pathlib import Path

_TMP = tempfile.mkdtemp(prefix="eqho-onboard-")
os.environ["EQHO_CONFIG_DIR"] = _TMP  # before importing src: never the real config
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.settings as settings_mod
from src.settings import Settings
import src.modelpick as modelpick
from src.modelpick import recommend_model

modelpick._hw_class = "gpu"  # fake hardware: the checks below must not depend on this PC

failures = []


def check(ok: bool, label: str) -> None:
    print(f"[{'PASS' if ok else 'FAIL'}] {label}")
    if not ok:
        failures.append(label)


def use_config(path: Path) -> None:
    settings_mod.CONFIG_FILE = path
    settings_mod.LEGACY_MODEL_DIR = path.parent / "no-legacy-model-cache"


def test_settings() -> None:
    with tempfile.TemporaryDirectory() as td:
        cfg = Path(td) / "settings.json"
        use_config(cfg)
        s = Settings.load()
        check(not s.onboarding_done, "fresh install -> setup needed")
        check(s.volume_duck == "off", "fresh install -> volume_duck default 'off'")

        cfg.write_text(json.dumps({"language": "de", "volume_duck": "mute"}), encoding="utf-8")
        s = Settings.load()
        check(s.onboarding_done, "existing settings.json without the field -> setup done")
        check(s.volume_duck == "mute", "saved volume_duck 'mute' stays 'mute'")

        cfg.write_text(json.dumps({"language": "en"}), encoding="utf-8")
        check(Settings.load().volume_duck == "mute",
              "existing file without volume_duck keeps the old default 'mute'")

        cfg.write_text(json.dumps({"onboarding_done": False}), encoding="utf-8")
        check(not Settings.load().onboarding_done, "saved onboarding_done False is kept")

        s = Settings.load()
        s.onboarding_done = True
        s.save()
        check(Settings.load().onboarding_done, "onboarding_done True round-trips")


def test_recommend() -> None:
    check(recommend_model("en") == "distil-large-v3", 'recommend_model("en") -> distil-large-v3')
    check(recommend_model("de") == "large-v3-turbo", 'recommend_model("de") -> large-v3-turbo')
    from src.ui.onboarding import language_choices
    check(all(c != "auto" for _, c in language_choices()), "no automatic-language choice")


def test_language_page() -> None:
    from types import SimpleNamespace
    import src.ui.onboarding as onb
    from src.ui.onboarding import OnboardingScreen
    onb.font = lambda *a, **kw: None  # no Tk root in the probe
    with tempfile.TemporaryDirectory() as td:
        use_config(Path(td) / "settings.json")
        Settings(language="en", model_size="distil-medium.en").save()
        scr = OnboardingScreen.__new__(OnboardingScreen)
        scr.ctx = SimpleNamespace(settings=Settings.load())
        scr._body = None
        scr._title = lambda *a, **kw: None
        scr._string_var = lambda value="": SimpleNamespace(get=lambda: value)
        made = {}
        def dropdown(parent, **kw):
            made.update(kw)
            return SimpleNamespace(pack=lambda *a, **k: None)
        scr._dropdown = dropdown
        scr._build_language()
        check(Settings.load().model_size == "distil-medium.en",
              "language page shown: saved model_size unchanged")
        made["command"]("German")
        s = Settings.load()
        check(s.language == "de" and s.model_size == "large-v3-turbo",
              "language picked: language + recommended model saved")


def test_fresh_install_model() -> None:
    from types import SimpleNamespace
    import src.ui.onboarding as onb
    from src.ui.onboarding import OnboardingScreen
    names = ("recommend_model", "ctk", "ghost_button", "primary_button", "secondary_button")
    saved = {n: getattr(onb, n) for n in names}
    saved_show = OnboardingScreen.show_page
    onb.recommend_model = lambda lang: f"pick-{lang}"
    try:
        for done, label in ((False, "fresh install"), (True, "Run setup again")):
            with tempfile.TemporaryDirectory() as td:
                use_config(Path(td) / "settings.json")
                Settings(language="de", model_size="medium", onboarding_done=done).save()
                ctx = SimpleNamespace(settings=Settings.load(), colors=SimpleNamespace(
                    bg_primary="#000", fg_secondary="#fff",
                    bg_tertiary="#111", bg_hover="#222"))
                onb.ctk = SimpleNamespace(CTkFrame=_Fake, CTkLabel=_Fake,
                                          CTkScrollableFrame=_Fake)
                for name in ("ghost_button", "primary_button", "secondary_button"):
                    setattr(onb, name, _Fake)
                OnboardingScreen.show_page = lambda self, i: None
                try:
                    OnboardingScreen(ctx, None, on_done=lambda *a: None)
                except Exception as e:
                    check(False, f"{label}: setup screen built ({e})")
                    continue
                want = "pick-de" if not done else "medium"
                got = Settings.load().model_size
                check(got == want, f"{label}: model_size {got!r} (want {want!r})")
    finally:
        for n, v in saved.items():
            setattr(onb, n, v)
        OnboardingScreen.show_page = saved_show


def test_close_during_setup() -> None:
    from types import SimpleNamespace
    from src.ui.dashboard import Dashboard
    with tempfile.TemporaryDirectory() as td:
        use_config(Path(td) / "settings.json")
        Settings(onboarding_done=False).save()
        dash = SimpleNamespace(_settings=Settings.load(), _tabs={}, _own_root=None,
                               destroy=lambda: destroyed.append(1))
        destroyed = []
        dash._on_close = lambda: Dashboard._on_close(dash)
        dash._end_onboarding = lambda sk, mc: Dashboard._end_onboarding(dash, sk, mc)
        dash._onboarding = SimpleNamespace(
            skip=lambda: dash._end_onboarding(True, False))
        dash._on_close()
        check(Settings.load().onboarding_done, "X during setup: setup marked done (= Skip)")
        check(dash._onboarding is None and destroyed, "X during setup: window closes")


class _Fake:
    def __init__(self, *a, **kw):
        pass

    def __getattr__(self, name):
        return lambda *a, **kw: None


class FakeTranscriber(_Fake):
    def is_model_ready(self):
        return False


def run_app(onboarding_done: bool):
    """Build the real App with fakes, run it, return (preloads, setups)."""
    import src.main as main_mod
    import src.ui as ui_pkg
    import src.watchdog as watchdog_mod
    with tempfile.TemporaryDirectory() as td:
        cfg = Path(td) / "settings.json"
        use_config(cfg)
        if onboarding_done:
            Settings(onboarding_done=True).save()
        main_mod.TranscriptionOverlay = _Fake
        main_mod.TrayApp = _Fake
        main_mod.HotkeyManager = _Fake
        main_mod.VoiceTranscriber = FakeTranscriber
        main_mod.load_fonts = lambda: None
        watchdog_mod.start = lambda *a, **kw: None
        setups = []
        ui_pkg.open_dashboard = lambda *a, **kw: setups.append(kw)
        preloads = []
        main_mod.App._preload_model = lambda self: preloads.append(1)
        app = main_mod.App()
        saved_argv = sys.argv
        sys.argv = [saved_argv[0]]
        try:
            app.run()
        finally:
            sys.argv = saved_argv
        time.sleep(0.3)  # let a preload thread (if any) run
        return preloads, setups


def test_first_launch() -> None:
    preloads, setups = run_app(onboarding_done=False)
    check(not preloads, "first launch: App.run does not preload/download the model")
    check(any(kw.get("onboarding") for kw in setups), "first launch: setup screen opened")
    preloads, setups = run_app(onboarding_done=True)
    check(bool(preloads), "setup done: App.run preloads the model as before")
    check(not any(kw.get("onboarding") for kw in setups), "setup done: no setup screen")


def test_window_fit() -> None:
    from src.ui.dashboard import MIN_H, MIN_W, WIN_H, WIN_W
    from src.ui.layout import fit_window
    zoom = 1.25
    # (label, screen w, h, Windows scaling, taskbar px, monitor left offset)
    cases = [("1920x1080 at 150 %", 1920, 1080, 1.5, 72, 0),
             ("1366x768 at 100 %", 1366, 768, 1.0, 48, 0),
             ("2560x1440 at 125 %", 2560, 1440, 1.25, 60, 0),
             ("3840x2160 at 200 %", 3840, 2160, 2.0, 96, 0),
             ("1920x1080 at 150 %, second monitor", 1920, 1080, 1.5, 72, 2560)]
    for label, sw, sh, dpi, bar, off in cases:
        scale = dpi * zoom
        area = (off, 0, off + sw, sh - bar)
        cw, ch = round(16 * dpi), round(39 * dpi)
        w, h, x, y = fit_window(WIN_W, WIN_H, MIN_W, MIN_H, scale, area, cw, ch)
        outer = (x, y, x + round(w * scale) + cw, y + round(h * scale) + ch)
        inside = (outer[0] >= area[0] and outer[1] >= area[1]
                  and outer[2] <= area[2] and outer[3] <= area[3])
        check(inside and MIN_W <= w <= WIN_W and MIN_H <= h <= WIN_H,
              f"window fit {label}: {w}x{h} logical, outer {outer} inside {area}")
        centred = (abs((outer[0] - area[0]) - (area[2] - outer[2])) <= 1
                   and abs((outer[1] - area[1]) - (area[3] - outer[3])) <= 1)
        check(centred, f"window fit {label}: centred in the work area")
    w, h, x, y = fit_window(WIN_W, WIN_H, MIN_W, MIN_H, 1.875, (0, 0, 1920, 1008), 24, 59)
    check(h < WIN_H, f"laptop case shrinks the height ({h} < {WIN_H})")
    w, h, x, y = fit_window(WIN_W, WIN_H, MIN_W, MIN_H, 3.0, (0, 0, 1024, 700), 0, 0)
    check((w, h, x, y) == (MIN_W, MIN_H, 0, 0),
          "work area smaller than the minimum: minimum size at the top-left")
    w, h, x, y = fit_window(WIN_W, WIN_H, MIN_W, MIN_H, 1.0, (0, 0, 3000, 2000), 0, 0)
    check((w, h) == (WIN_W, WIN_H), "big screen: never larger than the default size")


def test_col_count() -> None:
    from types import SimpleNamespace
    from src.ui.dashboard import Dashboard, col_count
    for logical, want in ((559, 1), (560, 2), (600, 2), (899, 2), (900, 3), (1200, 3)):
        check(col_count(logical) == want, f"col_count({logical}) == {want}")
    for dpi in (1.0, 1.25, 1.5, 2.0):
        for zoom in (1.0, 1.25, 1.5):
            scale = dpi * zoom
            got = {}
            for logical in (559, 600, 899, 900):
                fake = SimpleNamespace(
                    _content=SimpleNamespace(winfo_width=lambda w=round(logical * scale): w),
                    _get_widget_scaling=lambda s=scale: s, _ui_scale=zoom)
                got[logical] = Dashboard._get_col_count(fake)
            check(got == {559: 1, 600: 2, 899: 2, 900: 3},
                  f"columns at DPI {dpi} x zoom {zoom}: {got}")


def main() -> int:
    for test in (test_settings, test_recommend, test_language_page,
                 test_fresh_install_model,
                 test_close_during_setup, test_first_launch,
                 test_window_fit, test_col_count):
        try:
            test()
        except Exception as e:
            import traceback
            traceback.print_exc()
            check(False, f"{test.__name__} crashed: {e}")
    print("PASS" if not failures else f"FAIL ({len(failures)} checks)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
