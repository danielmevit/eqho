# Build Eqho into a standalone .exe (version lives in src/version.py)
# Run from the Eqho directory:
#   powershell -ExecutionPolicy Bypass -File build.ps1

Write-Host "=== Building Eqho ===" -ForegroundColor Cyan

pip install pyinstaller --quiet

pyinstaller Eqho.spec --noconfirm

Write-Host ""
Write-Host "Build complete! Executable is at:" -ForegroundColor Green
Write-Host "  dist\Eqho.exe" -ForegroundColor Yellow
Write-Host ""
Write-Host "To add to Windows startup:" -ForegroundColor Cyan
Write-Host '  Copy dist\Eqho.exe to %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup' -ForegroundColor Yellow
