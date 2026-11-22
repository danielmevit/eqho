# Build Eqho 0.9.x (Rust) for Windows x64:
#   - Compiles Eqho.exe (--release, static CRT + embedded icon/manifest/VERSIONINFO)
#   - Stages whisper.cpp 1.8.4 DLLs + LICENSE + THIRD_PARTY_NOTICES.txt
#   - Runs Eqho.exe --smoke gate
#   - Produces portable zip, Inno Setup 6 installer (if ISCC installed), and SHA256SUMS.txt
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build-rust.ps1 [-DllDir <path>] [-DevIsolated]
param(
    [string]$DllDir = "",
    [switch]$DevIsolated,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$CargoToml = Join-Path $Root "rust\Cargo.toml"
$VersionMatch = Select-String -Path $CargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $VersionMatch) {
    throw "Could not parse workspace version from $CargoToml"
}
$Version = $VersionMatch.Matches[0].Groups[1].Value
Write-Host "=== Building Eqho Rust v$Version (win-x64) ===" -ForegroundColor Cyan

if (-not $SkipBuild) {
    $env:CARGO_INCREMENTAL = "0"
    & cargo build --manifest-path $CargoToml -p eqho-app --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
}

$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "rust\target" }
$ExeSrc = Join-Path $TargetDir "release\Eqho.exe"
if (-not (Test-Path $ExeSrc)) {
    $ExeSrc = Join-Path $TargetDir "x86_64-pc-windows-msvc\release\Eqho.exe"
}
if (-not (Test-Path $ExeSrc)) {
    throw "Built Eqho.exe not found under $TargetDir"
}

$DistRoot = Join-Path $Root "dist"
$BundleDir = Join-Path $DistRoot "Eqho-$Version"
New-Item -ItemType Directory -Force -Path $BundleDir | Out-Null
Copy-Item -Force $ExeSrc (Join-Path $BundleDir "Eqho.exe")
Copy-Item -Force (Join-Path $Root "THIRD_PARTY_NOTICES.txt") (Join-Path $BundleDir "THIRD_PARTY_NOTICES.txt")
Copy-Item -Force (Join-Path $Root "LICENSE") (Join-Path $BundleDir "LICENSE")

if ($DllDir -ne "") {
    if (-not (Test-Path $DllDir)) { throw "Specified -DllDir does not exist: $DllDir" }
    Copy-Item -Force (Join-Path $DllDir "*.dll") $BundleDir
}

foreach ($required in @("whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-vulkan.dll")) {
    if (-not (Test-Path (Join-Path $BundleDir $required))) {
        throw "Required whisper.cpp runtime library missing from ${BundleDir}: $required (pass -DllDir <path>)"
    }
}

Write-Host "Running Eqho.exe --smoke gate..." -ForegroundColor Cyan
& (Join-Path $BundleDir "Eqho.exe") --smoke
if ($LASTEXITCODE -ne 0) { throw "Eqho.exe --smoke failed with exit code $LASTEXITCODE" }

$ZipPath = Join-Path $DistRoot "Eqho-portable-$Version-win-x64.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path (Join-Path $BundleDir "*") -DestinationPath $ZipPath -CompressionLevel Optimal
Write-Host "Portable zip: $ZipPath" -ForegroundColor Green

$Iscc = Get-Command "iscc.exe" -ErrorAction SilentlyContinue
if ($Iscc) { $Iscc = $Iscc.Source }
if (-not $Iscc) {
    foreach ($p in @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )) {
        if ($p -and (Test-Path $p)) { $Iscc = $p; break }
    }
}

$Artifacts = @($ZipPath)
if ($Iscc) {
    $IsccArgs = @(
        "/DAppVersion=$Version",
        "/DDistDir=$BundleDir",
        "/DOutputDir=$DistRoot"
    )
    if ($DevIsolated) { $IsccArgs += "/DDevIsolated=1" }
    $IsccArgs += (Join-Path $PSScriptRoot "installer-rust.iss")
    & $Iscc @IsccArgs
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
    $SetupExe = Join-Path $DistRoot "Eqho-Setup-$Version-win-x64.exe"
    Write-Host "Installer: $SetupExe" -ForegroundColor Green
    $Artifacts += $SetupExe
} else {
    Write-Host "Inno Setup 6 not found - installer skipped." -ForegroundColor Yellow
}

$SumsPath = Join-Path $DistRoot "SHA256SUMS-$Version-win-x64.txt"
$SumLines = foreach ($f in $Artifacts) {
    $hash = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
    "$hash  $(Split-Path -Leaf $f)"
}
$SumLines | Set-Content -Encoding Ascii $SumsPath
Write-Host "Checksums: $SumsPath" -ForegroundColor Green
