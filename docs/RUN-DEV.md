# Running the dev version (no install)

The quick reference for launching Eqho straight from the repo — the version on
the `dev` branch with all unreleased changes — without touching your installed
copy.

## TL;DR (your machine)

Open **Command Prompt** or **PowerShell**:

```bat
cd /d "D:\Vibe Coding\Eqho"
venv\Scripts\python.exe run.py
```

That's it. Eqho starts in the **system tray** (no window pops up) — press
**Alt+Q** in any app to dictate, right-click the tray icon → **Dashboard** for
settings, → **Quit** to stop it.

> ⚠️ Quit any installed/running Eqho first (tray → Quit). Two instances fight
> over the hotkey and the mic.

## Handy variations

| What | Command |
|------|---------|
| Open the dashboard immediately | `venv\Scripts\python.exe run.py --open-dashboard` |
| Health check, no GUI (settings→audio→model→transcribe) | `venv\Scripts\python.exe run.py --smoke` |
| Make sure you're on the dev branch first | `git checkout dev` then `git pull` |

## First time on a fresh machine

Only needed when `venv\` doesn't exist yet:

```bat
cd /d "D:\Vibe Coding\Eqho"
python -m venv venv
venv\Scripts\python.exe -m pip install -r requirements.txt
venv\Scripts\python.exe run.py
```

Python 3.10+ required. The first dictation downloads the active speech model
(size depends on the model you pick) — after that everything is offline.

## Good to know

- **Dev vs installed:** running from the repo uses the same settings file
  (`%APPDATA%\Eqho\settings.json`) and model cache as the installed app — you
  are testing the real thing, just with the newest code.
- **From WSL:** the UI needs Windows Python (WSL python has no tkinter here).
  One-liner: `cmd.exe /c "cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py"`
- **Logs:** `%APPDATA%\Eqho\eqho.log` — first stop when something misbehaves.
- **Installers** are built by CI on every version tag (`.github/workflows/release.yml`);
  you never build them locally for testing.
- **Dependencies changed?** (`requirements.txt` differs after a pull):
  `venv\Scripts\python.exe -m pip install -r requirements.txt` again.
