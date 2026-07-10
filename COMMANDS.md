# Eqho Commands Reference

All commands run from PowerShell in `D:\Vibe Coding\Eqho`.

## Daily Use

```powershell
# Navigate to project
cd "D:\Vibe Coding\Eqho"

# Activate virtual environment
venv\Scripts\activate

# Run the app
python run.py
```

## First-Time Setup

```powershell
# Create virtual environment
python -m venv venv

# Activate it
venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt
```

## If venv breaks (e.g. after folder rename)

```powershell
# Delete and recreate
Remove-Item -Recurse -Force venv
python -m venv venv
venv\Scripts\activate
pip install -r requirements.txt
```

## Install / Update Dependencies

```powershell
venv\Scripts\activate
pip install -r requirements.txt
```

## Build Windows Distributables

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
# Output: dist\Eqho\ (onedir bundle)
#         dist\Eqho-portable-<version>.zip
#         dist\Eqho-Setup-<version>.exe   (only if Inno Setup is installed:
#                                          winget install JRSoftware.InnoSetup)
```

CI builds the same artifacts automatically on every `v*` tag (see `.github/workflows/release.yml`).

## Headless Self-Check

```powershell
python run.py --smoke
# JSON report; exit 0 = pass. Run after any engine change.
```

## Git — Two-Repo Workflow

```powershell
# You have two remotes:
#   origin  → github.com/DanielMevit/Eqho          (PUBLIC)
#   private → github.com/DanielMevit/Eqho-private   (private)
#
# Two branches:
#   dev  → daily work (default push goes to private)
#   main → public releases only

# Check status
git status

# Check which branch you're on
git branch

# --- Daily work (on dev branch) ---
git add .
git commit -m "description of changes"
git push                          # pushes dev → private repo

# --- Publish to public repo ---
git checkout main
git merge dev
git push origin main              # pushes main → public repo
git checkout dev                  # switch back to work

# --- Sync private repo's main (if needed) ---
git push private main
```

## CUDA (GPU acceleration)

```powershell
# Install CUDA Toolkit 12.9 (one-time)
winget install Nvidia.CUDA --version 12.9
# Restart terminal after install
```

## Verify Windows Startup Registry

```powershell
reg query "HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" /v Eqho
```

## Troubleshooting

```powershell
# Check Python version (need 3.10+)
python --version

# Check if CUDA is available
python -c "import torch; print(torch.cuda.is_available())"

# Check installed packages
pip list

# Reinstall a specific package
pip install --force-reinstall customtkinter
```
