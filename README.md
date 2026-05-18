# Capture Bypass

A Windows utility that removes `WDA_EXCLUDEFROMCAPTURE` display-affinity protection from application windows, allowing them to be recorded normally by OBS, the Snipping Tool, and any other screen-capture software.

> **Legal notice:** Read [DISCLAIMER.md](DISCLAIMER.md) before use. Only use this tool on windows and processes you own or have explicit permission to capture.

![Release](https://img.shields.io/github/v/release/Londopy/capture-bypass) 
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/Londopy/HideDesktopApps/blob/main/LICENSE)
![Files](https://img.shields.io/github/directory-file-count/Londopy/capture-bypass) 
![Issues](https://img.shields.io/github/issues/Londopy/capture-bypass) 
![Size](https://img.shields.io/github/languages/code-size/Londopy/capture-bypass) 

---

## Download

Pre-built releases are on the [Releases](../../releases/latest) page — no Rust or build tools required.

| Download | What's inside | Best for |
|---|---|---|
| `capture-bypass-setup-*.exe` | Windows installer with prompts for shortcuts, startup, and install path | Most users |
| `capture-bypass-gui-*.zip` | Portable zip — `capture_bypass_gui.exe` + DLLs, just unzip and run | No-install preference |

The installer optionally adds a desktop shortcut, Start Menu entry, and a **Launch at Windows startup** entry (a UAC prompt will appear at each login since the app requires Administrator rights).

> **Note:** Windows Defender or other AV software may flag the payload DLLs due to the DLL injection technique. This is a false positive — see [DISCLAIMER.md](DISCLAIMER.md).

---

## How it works

Windows exposes [`SetWindowDisplayAffinity`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) which lets a process mark its own windows as protected from capture. Protected windows appear black/blank in screenshots and screen recordings.

Because the API only allows a process to modify *its own* windows, bypassing it requires running code inside the target process. This tool does that via **DLL injection**:

1. `OpenProcess` — open a handle to the target with VM + thread rights.
2. `VirtualAllocEx` / `WriteProcessMemory` — write the payload DLL path into the target's address space.
3. `CreateRemoteThread(LoadLibraryA)` — start a thread inside the target that loads the DLL.
4. The DLL's `DllMain` calls `SetWindowDisplayAffinity(hwnd, WDA_NONE)` on every window owned by that process, clearing the protection flag.

---

## Project structure

```
capture-bypass/
├── core/                       Shared Rust library — inject_dll() / inject_dll_stealth()
├── cli/                        Rust CLI binary
├── gui/                        Rust/egui GUI frontend
├── payload_dll/                One-shot payload DLL (strips once and exits)
├── payload_dll_persistent/     Persistent payload DLL (re-strips every 500 ms)
├── stress_tester/              Rust stress-test utility
└── installer/
    └── capture-bypass.iss      Inno Setup 6 installer script
```

---

## Requirements

| Dependency | Notes |
|---|---|
| [Rust + Cargo](https://rustup.rs) | Stable toolchain, `x86_64-pc-windows-msvc` target |
| Windows 10 2004+ | `WDA_EXCLUDEFROMCAPTURE` requires build 19041+ |
| Administrator privileges | Required by `OpenProcess` on other processes |

---

## Build

### x64 (required)

```powershell
git clone https://github.com/Londopy/capture-bypass.git
cd capture-bypass

# Build all crates — GUI, CLI, stress tester, and both payload DLLs
cargo build --release -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester
```

### x86 (optional — needed to inject into 32-bit processes)

The GUI auto-detects 32-bit target processes (shown with an orange **32** badge) and routes injection through the x86 binaries when present. If you skip this step, 32-bit targets fail gracefully with a status-bar error.

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
```

The x86 binaries land in `target/i686-pc-windows-msvc/release/`. The GUI picks them up automatically — no configuration needed.

---

## Usage

```powershell
target\release\capture_bypass_gui.exe
```

1. The app lists all visible, titled windows with their PID, process name, and live protection status (refreshed every 500 ms).
2. Use the **Filter** bar to search by process name, window title, or PID.
3. Tick **Protected only** to hide unprotected windows.
4. Click **Strip Protection** on any row, or **⚡ Strip All Protected** to batch-clear everything at once.
5. Toggle **Mode** between *One-shot* (strips once, fast) and *Persistent* (re-strips every 500 ms — for apps that fight back on a timer).
6. Enable **🤖 Auto-inject** to silently strip any newly protected window in the background; close to tray so it keeps running.
7. Enable **🚀 Start with Windows** to write a startup registry entry so the app launches automatically at login.
8. Click **📖 Help** in the header to open the built-in documentation.

### Browsers

Injecting Chrome, Edge, Firefox, Brave, Opera, Vivaldi, or Thorium automatically enumerates and injects all child processes, ensuring renderer processes that own the DRM video windows are covered.

---

## Testing

Use the included **Stress Tester** to verify injection is working correctly. Launch it from the **🔨 Stress Test** button in the GUI header, or run it directly:

```powershell
target\release\stress_tester.exe
```

The stress tester marks itself as `WDA_EXCLUDEFROMCAPTURE` on launch and polls `GetWindowDisplayAffinity` every 100 ms to show the live protection state. Use it to:

- **Verify one-shot injection** — click *Strip Protection* in the main app; the window should flip to OK.
- **Stress-test persistent mode** — enable *Fight Mode* in the tester (adjustable 50–2000 ms re-apply rate) and inject the persistent DLL. The strip counter should climb while the fight counter stays ahead.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

MIT — see [LICENSE](LICENSE).

## Disclaimer

See [DISCLAIMER.md](DISCLAIMER.md).
