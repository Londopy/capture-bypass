; ─────────────────────────────────────────────────────────────────────────────
; capture-bypass  —  Inno Setup 6 installer script
;
; Build manually (from the repo root, after `cargo build --release`):
;   iscc installer\capture-bypass.iss
;
; Override the version string on the command line:
;   iscc /DMyAppVersion=3.4.1 installer\capture-bypass.iss
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

; Use the app's own icon for the installer exe
SetupIconFile=..\target\release\capture_bypass_gui.exe

; Output
OutputDir=..\dist
OutputBaseFilename=capture-bypass-setup-{#MyAppVersion}-x64

; Compression
Compression=lzma2/ultra64
SolidCompression=yes

; Appearance
WizardStyle=modern

; Require admin so the app (which itself needs admin) can be installed system-wide
PrivilegesRequired=admin

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

; Desktop shortcut
Name: "{autodesktop}\{#MyAppName}"; \
  Filename: "{app}\{#MyAppExeName}"; \
  Tasks: desktopicon

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
; Offer to launch the app after installation finishes
Filename: "{app}\{#MyAppExeName}"; \
  Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; \
  Flags: nowait postinstall skipifsilent
