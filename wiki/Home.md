# capture-bypass Wiki

**capture-bypass** removes `WDA_EXCLUDEFROMCAPTURE` screen-capture protection from Windows application windows, letting OBS, the Snipping Tool, and any other capture software record them normally.

---

## Pages

| Page | What's covered |
|---|---|
| [Installation & Build](Installation-and-Build) | Requirements, release downloads, building from source (x64 + x86 + ARM64) |
| [Usage Guide](Usage-Guide) | UI walkthrough — window list, filter, all toolbar buttons |
| [Injection Modes](Injection-Modes) | One-shot vs Persistent — when to use each |
| [Browser Injection](Browser-Injection) | Why browsers need multi-process injection and how it works |
| [System Tray & Auto-inject](System-Tray-and-Auto-Inject) | Close-to-tray, auto-inject background thread |
| [Testing](Testing) | stress_tester — fight mode, Scenario A (process scan), Scenario B (module ejection) |
| [Troubleshooting](Troubleshooting) | DLLs not found, AV flags, injection fails, OBS still black |

---

## Quick start

```powershell
# 1. Download from the Releases page:
#    - capture-bypass-setup-X.Y.Z-x64.exe  (installer, x64)
#    - capture-bypass-X.Y.Z-portable-x64.zip  (portable, x64)
#    - capture-bypass-X.Y.Z-portable-arm64.zip  (portable, ARM64 — Snapdragon X / Surface)
#
# 2. Run the installer or extract the zip.
#    Launch from the desktop shortcut or Start Menu (runs as Administrator automatically).
```

The app lists every visible window. Click **Strip Protection** on any protected row, or **⚡ Strip All Protected** to clear everything at once.

---

## How it works

Windows exposes [`SetWindowDisplayAffinity`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity), which lets a process protect its own windows from capture. Because the API only works on a process's *own* windows, bypassing it requires running code **inside** the target process.

capture-bypass does this via classic **LoadLibrary DLL injection**:

1. `OpenProcess` — open a handle to the target with VM + thread rights
2. `VirtualAllocEx` / `WriteProcessMemory` — write the payload DLL path into the target's address space
3. `CreateRemoteThread(LoadLibraryA)` — start a thread inside the target that loads the DLL
4. The DLL's `DllMain` calls `SetWindowDisplayAffinity(hwnd, WDA_NONE)` on every window owned by that process

---

## Legal notice

Only use this tool on windows and processes **you own or have explicit permission to capture**.  
See [DISCLAIMER.md](https://github.com/Londopy/capture-bypass/blob/main/DISCLAIMER.md) in the repository.
