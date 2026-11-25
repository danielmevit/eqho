; Eqho 0.9.x (Rust) Windows Installer (Inno Setup 6).
; Usage:
;   ISCC /DAppVersion=0.9.0 /DDistDir=<path>\Eqho /DOutputDir=<path> installer-rust.iss
;   ISCC /DAppVersion=0.9.0-dev /DDevIsolated=1 /DDistDir=<path>\Eqho-0.9.0-dev /DOutputDir=<path> installer-rust.iss
;
; Per-user install (no UAC prompt). User settings (%APPDATA%\Eqho) and
; downloaded GGML speech models are never deleted on uninstall.

#ifndef AppVersion
  #define AppVersion "0.9.2"
#endif
#ifndef DistDir
  #define DistDir "..\..\dist\Eqho"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#ifdef DevIsolated
  #define AppName "Eqho (Rust 0.9 Dev)"
  #define InstallDirName "Eqho-Rust-0.9"
  #define RunKeyName "Eqho-Rust-0.9"
  #define AppGuid "{{A4F91C22-8E11-4F7D-B2C0-90E1B6D4A909}"
#else
  #define AppName "Eqho"
  #define InstallDirName "Eqho"
  #define RunKeyName "Eqho"
  #define AppGuid "{{7C21F6E3-9B4A-4D2E-8F0C-5A6B3D91C4E7}"
#endif

[Setup]
AppId={#AppGuid}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=damt.xyz
AppPublisherURL=https://damt.xyz
AppSupportURL=https://github.com/danielmevit/eqho/issues
DefaultDirName={autopf}\{#InstallDirName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=Eqho-Setup-{#AppVersion}-win-x64
SetupIconFile=..\..\assets\eqho.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\Eqho.exe
UninstallDisplayName={#AppName}
CloseApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "autostart"; Description: "Start {#AppName} automatically when you sign in"; GroupDescription: "Startup:"

[Files]
Source: "{#DistDir}\Eqho.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\Eqho.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\Eqho.exe"; Tasks: desktopicon

[Registry]
; Matches the in-app "Start with Windows" HKCU Run value name so installer and dashboard stay in sync.
Root: HKCU; Subkey: "SOFTWARE\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#RunKeyName}"; ValueData: """{app}\Eqho.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\Eqho.exe"; Description: "Launch {#AppName} now"; Flags: nowait postinstall skipifsilent

[Code]
// Eqho runs primarily as a tray app (dashboard window hidden), so Restart Manager cannot
// always close it automatically during an in-place upgrade. Gracefully terminate any running
// Eqho.exe (and its --model-host child) right before extracting updated files.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /T /IM Eqho.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(250);
  Result := '';
end;
