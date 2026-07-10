"""Top-level launcher script for Eqho.

Run from the Eqho directory:
    python run.py            # start the app
    python run.py --smoke    # headless self-check (no GUI, tray, or hotkeys)
"""

import sys

if __name__ == "__main__":
    if "--smoke" in sys.argv:
        from src.smoke import run_smoke
        sys.exit(run_smoke())
    if "--diagnose" in sys.argv:
        from src.diagnose import run_diagnose
        sys.exit(run_diagnose(sys.argv))
    from src.main import main
    main()
