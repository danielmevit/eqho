# Build Eqho for Windows: onedir bundle + portable zip + installer (if Inno Setup is installed).
# Usage (from anywhere):
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Python = Join-Path $Root "venv\Scripts\python.exe"
if (-not (Test-Path $Python)) { $Python = "python" }

$Version = (Select-String -Path (Join-Path $Root "src\version.py") -Pattern '__version__ = "([^"]+)"').Matches[0].Groups[1].Value
Write-Host "=== Building Eqho v$Version ===" -ForegroundColor Cyan

& $Python -m pip install pyinstaller --quiet --disable-pip-version-check
& $Python -m PyInstaller (Join-Path $PSScriptRoot "eqho-win.spec") --noconfirm `
    --distpath (Join-Path $Root "dist") --workpath (Join-Path $Root "build")

$Zip = Join-Path $Root "dist\Eqho-portable-$Version.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path (Join-Path $Root "dist\Eqho\*") -DestinationPath $Zip
Write-Host "Portable zip: $Zip" -ForegroundColor Green

$Iscc = Get-Command "iscc.exe" -ErrorAction SilentlyContinue
if (-not $Iscc) {
    foreach ($p in @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
                     "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
                     "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")) {
        if ($p -and (Test-Path $p)) { $Iscc = $p; break }
    }
}
if ($Iscc) {
    & $Iscc "/DAppVersion=$Version" "/DDistDir=$(Join-Path $Root 'dist\Eqho')" (Join-Path $PSScriptRoot "installer.iss") | Select-Object -Last 3
    Write-Host "Installer: dist\Eqho-Setup-$Version.exe" -ForegroundColor Green
} else {
    Write-Host "Inno Setup not found - installer skipped. Install with: winget install JRSoftware.InnoSetup" -ForegroundColor Yellow
}
