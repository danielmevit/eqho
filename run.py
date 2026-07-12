"""Top-level launcher script for Eqho.

Run from the Eqho directory:
    python run.py            # start the app
    python run.py --smoke    # headless self-check (no GUI, tray, or hotkeys)
"""

import multiprocessing
import sys

if __name__ == "__main__":
    # REQUIRED before any multiprocessing use — without it a frozen (PyInstaller)
    # build would relaunch the whole app as each model-host child process.
    multiprocessing.freeze_support()
    if "--smoke" in sys.argv:
        from src.smoke import run_smoke
        sys.exit(run_smoke())
    if "--diagnose" in sys.argv:
        from src.diagnose import run_diagnose
        sys.exit(run_diagnose(sys.argv))
    from src.main import main
    main()
