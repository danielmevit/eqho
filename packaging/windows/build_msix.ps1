<#
    Builds the Windows MSIX package for Eqho into dist\store\ (or -OutDir).

    KEEP THIS FILE PURE ASCII. Windows PowerShell 5.1 reads a BOM-less .ps1 as ANSI
    (cp1252), so non-ASCII punctuation in strings can fail to parse.

    Usage:
        powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 -DllDir C:\path\to\whisper-dlls
        powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 -Store -DllDir C:\path\to\whisper-dlls
        powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 -SignForSideload C:\path\to\test.pfx -DllDir C:\path\to\whisper-dlls
#>
param(
    [string]$ExePath,
    [string]$DllDir,
    [string]$SignForSideload,
    [string]$CertPassword,
    [switch]$Store,
    [switch]$SkipBuild,
    [string]$OutDir = "dist\store"
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$resolvedOutDir = if ([System.IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $root $OutDir }
$stageDir = Join-Path $resolvedOutDir "_stage"
Push-Location $root

try {
    if ($Store -and $SignForSideload) {
        Write-Error "Cannot combine -Store and -SignForSideload: Microsoft Store packages must be uploaded unsigned."
    }

    # -- 0. Validate Store environment variables up front when -Store is set --
    if ($Store) {
        foreach ($required in @(
            "EQHO_STORE_ID",
            "EQHO_STORE_IDENTITY_NAME",
            "EQHO_STORE_PUBLISHER",
            "EQHO_STORE_PUBLISHER_DISPLAY"
        )) {
            if (-not (Get-Item "env:$required" -ErrorAction SilentlyContinue).Value) {
                Write-Error "-Store requires $required (see Partner Center -> Product management -> Product identity)"
            }
        }
        if ($env:EQHO_STORE_ID.Trim().ToUpperInvariant() -eq "9NBLGGH4XXXX") {
            Write-Error "-Store requires EQHO_STORE_ID to be set to Eqho's real Partner Center Store ID, not placeholder 9NBLGGH4XXXX."
        }
    }

    # -- 1. Version from rust/Cargo.toml --
    $cargoTomlPath = Join-Path $root "rust\Cargo.toml"
    if (-not (Test-Path $cargoTomlPath)) {
        Write-Error "Missing $cargoTomlPath"
    }
    $cargoToml = Get-Content $cargoTomlPath -Raw
    $wsMatch = [regex]::Match($cargoToml, '(?ms)^\[workspace\.package\](.*?)(?=^\[|\z)')
    if (-not $wsMatch.Success) {
        Write-Error "Could not find [workspace.package] in rust\Cargo.toml"
    }
    $verMatch = [regex]::Match($wsMatch.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if (-not $verMatch.Success) {
        Write-Error "Could not find workspace.package.version in rust\Cargo.toml"
    }
    $rawVersion = $verMatch.Groups[1].Value.Trim()
    $baseVersion = ($rawVersion -split '[-+]', 2)[0]
    if ($baseVersion -notmatch '^\d+\.\d+\.\d+$') {
        Write-Error "Expected three-part SemVer base in rust\Cargo.toml (got '$rawVersion')"
    }
    $msixVersion = "$baseVersion.0"
    Write-Host "=== Eqho $rawVersion (MSIX $msixVersion) ===" -ForegroundColor Cyan

    # -- 2. Locate or build Eqho.exe (with --features store) --
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "rust\target" }
    if ($ExePath) {
        if (-not (Test-Path $ExePath)) {
            Write-Error "Specified -ExePath not found: $ExePath"
        }
    } else {
        if (-not $SkipBuild) {
            Write-Host "`n--- Building eqho-app (store, release) ---" -ForegroundColor Cyan
            $env:CARGO_INCREMENTAL = "0"
            Push-Location (Join-Path $root "rust")
            try {
                & cargo build -p eqho-app --features store --release --locked
                if ($LASTEXITCODE -ne 0) {
                    Write-Error "cargo build -p eqho-app --features store --release --locked failed"
                }
            } finally {
                Pop-Location
            }
        }
        $candidates = @(
            (Join-Path $targetDir "release\Eqho.exe"),
            (Join-Path $targetDir "x86_64-pc-windows-msvc\release\Eqho.exe")
        )
        $ExePath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
        if (-not $ExePath) {
            Write-Error "Eqho.exe was not found under $targetDir (checked release\Eqho.exe and x86_64-pc-windows-msvc\release\Eqho.exe)"
        }
    }
    Write-Host "[ok] Executable: $ExePath" -ForegroundColor Green

    # -- 3. Find Windows 10/11 SDK (makeappx / signtool / makepri) --
    $kitsRoot = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    $sdk = Get-ChildItem $kitsRoot -Directory -ErrorAction SilentlyContinue |
        Where-Object { Test-Path "$($_.FullName)\x64\makeappx.exe" } |
        Sort-Object {
            $parsed = $null
            if ([version]::TryParse($_.Name, [ref]$parsed)) { $parsed } else { [version]"0.0.0.0" }
        } -Descending |
        Select-Object -First 1
    if (-not $sdk) {
        Write-Error "Windows 10/11 SDK makeappx.exe not found under $kitsRoot"
    }
    $sdkBin = Join-Path $sdk.FullName "x64"
    $makeappx = Join-Path $sdkBin "makeappx.exe"
    $signtool = Join-Path $sdkBin "signtool.exe"
    $makepri = Join-Path $sdkBin "makepri.exe"
    Write-Host "[ok] Windows SDK: $($sdk.Name)" -ForegroundColor Green

    # -- 4. Stage files --
    if (Test-Path $stageDir) {
        Remove-Item $stageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $resolvedOutDir -Force | Out-Null
    New-Item -ItemType Directory -Path $stageDir -Force | Out-Null

    Copy-Item $ExePath (Join-Path $stageDir "Eqho.exe")

    # Stage whisper.cpp and ggml DLLs
    $effectiveDllDir = $DllDir
    if (-not $effectiveDllDir) {
        $exeDir = Split-Path -Parent $ExePath
        if ($exeDir -and (Test-Path (Join-Path $exeDir "whisper.dll"))) {
            $effectiveDllDir = $exeDir
        }
    }
    if (-not $effectiveDllDir) {
        Write-Error "No whisper.cpp DLL directory provided. Pass -DllDir <path> containing whisper.dll and ggml*.dll."
    }
    if (-not (Test-Path $effectiveDllDir)) {
        Write-Error "Specified -DllDir does not exist: $effectiveDllDir"
    }
    $dllFiles = Get-ChildItem -Path $effectiveDllDir -Filter "*.dll" -File
    if (-not $dllFiles -or $dllFiles.Count -eq 0) {
        Write-Error "No *.dll files found in $effectiveDllDir"
    }
    foreach ($dll in $dllFiles) {
        Copy-Item $dll.FullName (Join-Path $stageDir $dll.Name)
    }

    foreach ($requiredDll in @("whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-vulkan.dll")) {
        if (-not (Test-Path (Join-Path $stageDir $requiredDll))) {
            Write-Error "Required runtime library missing after staging: $requiredDll (from $effectiveDllDir)"
        }
    }
    $cpuDlls = Get-ChildItem -Path $stageDir -Filter "ggml-cpu*.dll" -File
    if (-not $cpuDlls -or $cpuDlls.Count -eq 0) {
        Write-Error "Required CPU backend missing after staging: expected at least one ggml-cpu*.dll in $effectiveDllDir"
    }
    Write-Host "[ok] Staged $($dllFiles.Count) runtime DLL(s) from $effectiveDllDir" -ForegroundColor Green

    $licenseSrc = Join-Path $root "LICENSE"
    if (-not (Test-Path $licenseSrc)) {
        Write-Error "Missing LICENSE at $licenseSrc"
    }
    Copy-Item $licenseSrc (Join-Path $stageDir "LICENSE.txt")

    $noticesSrc = Join-Path $root "THIRD_PARTY_NOTICES.txt"
    if (-not (Test-Path $noticesSrc)) {
        Write-Error "Missing THIRD_PARTY_NOTICES.txt at $noticesSrc"
    }
    Copy-Item $noticesSrc (Join-Path $stageDir "THIRD-PARTY-NOTICES.txt")

    $assetsSrc = Join-Path $PSScriptRoot "assets"
    $assetsDst = Join-Path $stageDir "assets"
    if (-not (Test-Path $assetsSrc)) {
        Write-Error "Missing MSIX logo assets at $assetsSrc (run: python3 packaging/windows/generate_assets.py)"
    }
    New-Item -ItemType Directory -Path $assetsDst -Force | Out-Null
    Copy-Item (Join-Path $assetsSrc "*") $assetsDst

    # -- 5. Resolve manifest placeholders and stamp AppxManifest.xml --
    $manifestPath = Join-Path $PSScriptRoot "AppxManifest.xml"
    $manifest = Get-Content $manifestPath -Raw

    if ($Store) {
        $identityName = $env:EQHO_STORE_IDENTITY_NAME
        $publisher = $env:EQHO_STORE_PUBLISHER
        $publisherDisplay = $env:EQHO_STORE_PUBLISHER_DISPLAY
        $displayName = if ($env:EQHO_STORE_DISPLAY_NAME) { $env:EQHO_STORE_DISPLAY_NAME } else { "Eqho" }
    } elseif ($SignForSideload) {
        if (-not (Test-Path $SignForSideload)) {
            Write-Error "Sideload certificate not found at $SignForSideload"
        }
        $pw = if ($CertPassword) { $CertPassword } elseif ($env:EQHO_CERT_PASSWORD) { $env:EQHO_CERT_PASSWORD } else { "" }
        $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($SignForSideload, $pw)
        $identityName = if ($env:EQHO_STORE_IDENTITY_NAME) { $env:EQHO_STORE_IDENTITY_NAME } else { "Eqho.SideloadTest" }
        $publisher = if ($env:EQHO_STORE_PUBLISHER) { $env:EQHO_STORE_PUBLISHER } else { $cert.Subject }
        $publisherDisplay = if ($env:EQHO_STORE_PUBLISHER_DISPLAY) { $env:EQHO_STORE_PUBLISHER_DISPLAY } else { "Eqho Local Test" }
        $displayName = if ($env:EQHO_STORE_DISPLAY_NAME) { $env:EQHO_STORE_DISPLAY_NAME } else { "Eqho" }
    } else {
        $identityName = if ($env:EQHO_STORE_IDENTITY_NAME) { $env:EQHO_STORE_IDENTITY_NAME } else { "Eqho.LocalTest" }
        $publisher = if ($env:EQHO_STORE_PUBLISHER) { $env:EQHO_STORE_PUBLISHER } else { "CN=EqhoLocalTest" }
        $publisherDisplay = if ($env:EQHO_STORE_PUBLISHER_DISPLAY) { $env:EQHO_STORE_PUBLISHER_DISPLAY } else { "Eqho Local Test" }
        $displayName = if ($env:EQHO_STORE_DISPLAY_NAME) { $env:EQHO_STORE_DISPLAY_NAME } else { "Eqho" }
        Write-Host "[warn] Using placeholder local identity ($identityName). Pass -Store with Partner Center env vars for Store upload." -ForegroundColor Yellow
    }

    $replacements = @{
        "__IDENTITY_NAME__"          = [System.Security.SecurityElement]::Escape($identityName)
        "__PUBLISHER__"              = [System.Security.SecurityElement]::Escape($publisher)
        "__PUBLISHER_DISPLAY_NAME__" = [System.Security.SecurityElement]::Escape($publisherDisplay)
        "__DISPLAY_NAME__"           = [System.Security.SecurityElement]::Escape($displayName)
        "__VERSION__"                = $msixVersion
    }
    foreach ($entry in $replacements.GetEnumerator()) {
        $manifest = $manifest.Replace($entry.Key, $entry.Value)
    }
    if ($manifest -match '__[A-Z0-9_]+__') {
        Write-Error "Unreplaced manifest placeholder remains in AppxManifest.xml"
    }

    $stagedManifest = Join-Path $stageDir "AppxManifest.xml"
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($stagedManifest, $manifest, $utf8NoBom)

    # -- 6. Generate resources.pri when makepri.exe is available --
    if (Test-Path $makepri) {
        Write-Host "`n--- Generating resources.pri ---" -ForegroundColor Cyan
        $priConfig = Join-Path $stageDir "priconfig.xml"
        $priOut = Join-Path $stageDir "resources.pri"
        & $makepri createconfig /cf $priConfig /dq en-US /o
        if ($LASTEXITCODE -ne 0) { Write-Error "makepri createconfig failed" }
        & $makepri new /pr $stageDir /cf $priConfig /mn $stagedManifest /of $priOut /o
        if ($LASTEXITCODE -ne 0) { Write-Error "makepri new failed" }
        Remove-Item $priConfig -Force -ErrorAction SilentlyContinue
    }

    # -- 7. Pack MSIX --
    $variant = if ($Store) { "store" } elseif ($SignForSideload) { "sideload" } else { "unsigned" }
    $msixPath = Join-Path $resolvedOutDir "Eqho-$rawVersion-$variant.msix"
    if (Test-Path $msixPath) {
        Remove-Item $msixPath -Force
    }
    Write-Host "`n--- Packing $msixPath ---" -ForegroundColor Cyan
    & $makeappx pack /d $stageDir /p $msixPath /o
    if ($LASTEXITCODE -ne 0) {
        Write-Error "makeappx pack failed"
    }

    # -- 8. Optional local sideload signing --
    if ($SignForSideload) {
        if (-not (Test-Path $signtool)) {
            Write-Error "signtool.exe not found at $signtool"
        }
        $pw = if ($CertPassword) { $CertPassword } elseif ($env:EQHO_CERT_PASSWORD) { $env:EQHO_CERT_PASSWORD } else { $null }
        Write-Host "`n--- Signing for local sideload testing ---" -ForegroundColor Cyan
        if ($pw) {
            & $signtool sign /fd SHA256 /f $SignForSideload /p $pw $msixPath
        } else {
            & $signtool sign /fd SHA256 /f $SignForSideload $msixPath
        }
        if ($LASTEXITCODE -ne 0) {
            Write-Error "signtool sign failed"
        }
        Write-Host "[ok] Signed sideload MSIX: $msixPath" -ForegroundColor Green
    } elseif ($Store) {
        Write-Host "[ok] Store MSIX (unsigned, ready for Partner Center): $msixPath" -ForegroundColor Green
    } else {
        Write-Host "[ok] Unsigned test MSIX: $msixPath" -ForegroundColor Green
    }
} finally {
    if (Test-Path $stageDir) {
        Remove-Item $stageDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    Pop-Location
}
