; Eqho Windows installer (Inno Setup 6).
; Built by packaging/windows/build.ps1, which passes the version and dist dir:
;   ISCC /DAppVersion=x.y.z /DDistDir=<repo>\dist\Eqho installer.iss
; Per-user install (no UAC prompt). User settings (%APPDATA%\Eqho) and
; downloaded models are never touched by the uninstaller.

#define AppName "Eqho"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef DistDir
  #define DistDir "..\..\dist\Eqho"
#endif

[Setup]
AppId={{7C21F6E3-9B4A-4D2E-8F0C-5A6B3D91C4E7}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=damt.xyz
AppPublisherURL=https://damt.xyz
AppSupportURL=https://github.com/danielmevit/eqho/issues
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=Eqho-Setup-{#AppVersion}-win-x64
SetupIconFile=..\..\assets\eqho.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\Eqho.exe
UninstallDisplayName={#AppName}
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "autostart"; Description: "Start {#AppName} automatically when you sign in"; GroupDescription: "Startup:"

[Files]
Source: "{#DistDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\Eqho.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\Eqho.exe"; Tasks: desktopicon

[Registry]
; Same Run-key value name the in-app "Start with Windows" toggle writes,
; so the installer checkbox and the app setting stay in sync.
Root: HKCU; Subkey: "SOFTWARE\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\Eqho.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\Eqho.exe"; Description: "Launch {#AppName} now"; Flags: nowait postinstall skipifsilent
