; ─────────────────────────────────────────────────────────────────────────────
; capture-bypass  —  Inno Setup 6 installer script
;
; !! IMPORTANT — use the SAME version in both steps below !!
;
; Step 1 — build the exe with the version baked in (from the repo root):
;   set APP_VERSION=3.4.11 && cargo build --release -p gui -p stress_tester -p payload_dll -p payload_dll_persistent
;
;   APP_VERSION is read by gui\build.rs and overrides CARGO_PKG_VERSION inside
;   the exe, so the built-in update check always reports the correct version.
;
; Step 2 — compile the installer (same version string):
;   iscc /DMyAppVersion=3.4.11 installer\capture-bypass.iss
;
; Output lands in:  dist\capture-bypass-setup-<version>-x64.exe
; ─────────────────────────────────────────────────────────────────────────────

; ── Defines ──────────────────────────────────────────────────────────────────

#ifndef MyAppVersion
  #define MyAppVersion "dev"
#endif

#define MyAppName      "capture-bypass"
#define MyAppPublisher "Londopy"
#define MyAppURL       "https://github.com/Londopy/capture-bypass"
#define MyAppExeName   "capture_bypass_gui.exe"

; ── [Setup] ───────────────────────────────────────────────────────────────────

[Setup]
; Unique GUID — do NOT reuse this for a different app.
AppId={{A3F7B2C1-4D5E-6F7A-8B9C-0D1E2F3A4B5C}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases

; Install to C:\Program Files\capture-bypass by default
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes

; License page shown during install
LicenseFile=license.txt

; Use the app icon for the installer exe
SetupIconFile=..\gui\icon.ico

; Output
OutputDir=..\dist
OutputBaseFilename=capture-bypass-setup-{#MyAppVersion}-x64

; Compression
Compression=lzma2/ultra64
SolidCompression=yes

; Detect running instances and prompt the user to close them before
; overwriting files.  Because the app runs elevated and the installer
; does not, Inno Setup cannot forcibly terminate it — it will show a
; "please close the application" dialog instead.
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no

; Appearance
WizardStyle=modern

; Per-user install — no UAC prompt during setup.
; The {auto*} constants below resolve to per-user locations automatically.
; The app itself still requests elevation at launch (required for OpenProcess).
PrivilegesRequired=lowest

; Only install on x64 Windows (the GUI exe is x64-only)
ArchitecturesInstallIn64BitMode=x64compatible

; ── [Languages] ───────────────────────────────────────────────────────────────

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

; ── [Tasks] ───────────────────────────────────────────────────────────────────

[Tasks]
; Desktop shortcut — checked by default
Name: "desktopicon"; \
  Description: "Create a &desktop shortcut"; \
  GroupDescription: "Additional shortcuts:"

; Start Menu shortcut — checked by default (AllowNoIcons lets users opt out)
Name: "startmenuicon"; \
  Description: "Create a &Start Menu shortcut"; \
  GroupDescription: "Additional shortcuts:"

; Windows startup entry — unchecked by default (UAC prompt caveat shown)
Name: "startup"; \
  Description: "Launch capture-bypass at &Windows startup  (a UAC elevation prompt will appear each login)"; \
  GroupDescription: "Startup:"; \
  Flags: unchecked

; Stress-tester desktop shortcut — unchecked by default (power-user tool)
Name: "stressdesktopicon"; \
  Description: "Create a desktop shortcut for the &Stress Tester utility"; \
  GroupDescription: "Additional shortcuts:"; \
  Flags: unchecked

; ── [Files] ───────────────────────────────────────────────────────────────────

[Files]
; Main GUI executable
Source: "..\target\release\capture_bypass_gui.exe"; \
  DestDir: "{app}"; Flags: ignoreversion

; Stress-test utility
Source: "..\target\release\stress_tester.exe"; \
  DestDir: "{app}"; Flags: ignoreversion

; x64 payload DLLs
Source: "..\target\release\payload_dll.dll"; \
  DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\payload_dll_persistent.dll"; \
  DestDir: "{app}"; Flags: ignoreversion

; x86 payload DLLs — optional, only present if the i686 target was built.
; The GUI looks for these in {app}\x86\ automatically.
Source: "..\target\i686-pc-windows-msvc\release\payload_dll.dll"; \
  DestDir: "{app}\x86"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\target\i686-pc-windows-msvc\release\payload_dll_persistent.dll"; \
  DestDir: "{app}\x86"; Flags: ignoreversion skipifsourcedoesntexist

; ── [Icons] ───────────────────────────────────────────────────────────────────

[Icons]
; Start Menu entries (created when startmenuicon task is selected)
Name: "{group}\{#MyAppName}"; \
  Filename: "{app}\{#MyAppExeName}"; \
  Tasks: startmenuicon
Name: "{group}\Stress Tester"; \
  Filename: "{app}\stress_tester.exe"; \
  Tasks: startmenuicon
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; \
  Filename: "{uninstallexe}"; \
  Tasks: startmenuicon

; Desktop shortcut (main app)
Name: "{autodesktop}\{#MyAppName}"; \
  Filename: "{app}\{#MyAppExeName}"; \
  Tasks: desktopicon

; Desktop shortcut (stress tester — optional, unchecked by default)
Name: "{autodesktop}\{#MyAppName} Stress Tester"; \
  Filename: "{app}\stress_tester.exe"; \
  Tasks: stressdesktopicon

; ── [Registry] ────────────────────────────────────────────────────────────────

[Registry]
; HKCU Run entry — written only when the "startup" task is checked.
; Automatically removed on uninstall (uninsdeletevalue).
Root: HKCU; \
  Subkey: "SOFTWARE\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; \
  ValueName: "{#MyAppName}"; \
  ValueData: """{app}\{#MyAppExeName}"""; \
  Flags: uninsdeletevalue; \
  Tasks: startup

; ── [Run] ─────────────────────────────────────────────────────────────────────

[Run]
; Offer to launch the app after installation finishes.
; shellexec is required because the installer runs without elevation but the
; app's manifest requests requireAdministrator — ShellExecuteEx handles this
; correctly (triggers the UAC prompt), while CreateProcess would return error 740.
Filename: "{app}\{#MyAppExeName}"; \
  Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; \
  Flags: nowait postinstall skipifsilent shellexec

; ── [Code] ────────────────────────────────────────────────────────────────────

[Code]
// Check whether capture_bypass_gui.exe is currently running.
// Called by Inno Setup before the install begins (PrepareToInstall).
// Because the app runs elevated and the installer does not, we cannot
// terminate it programmatically — we block the install and tell the user
// to quit from the system tray first.
function IsAppRunning(const ExeName: String): Boolean;
var
  WbemLocator  : Variant;
  WbemService  : Variant;
  WbemObjectSet: Variant;
begin
  Result := False;
  try
    WbemLocator   := CreateOleObject('WbemScripting.SWbemLocator');
    WbemService   := WbemLocator.ConnectServer('', 'root\cimv2', '', '');
    WbemObjectSet := WbemService.ExecQuery(
      'SELECT Name FROM Win32_Process WHERE Name="' + ExeName + '"');
    Result := (WbemObjectSet.Count > 0);
  except
    // WMI unavailable — fall through and allow install
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if IsAppRunning('{#MyAppExeName}') then
    Result :=
      'capture-bypass is currently running.' + #13#10 + #13#10 +
      'Please quit it before upgrading:' + #13#10 +
      '  1. Right-click the capture-bypass icon in the system tray.' + #13#10 +
      '  2. Click Quit.' + #13#10 +
      '  3. Run the installer again.';
end;
