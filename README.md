# Capture Bypass

> **Legal notice:** By downloading, installing, cloning, or running this project in any form, you agree to the [Terms of Service](TERMS_OF_SERVICE.md) and [Disclaimer](DISCLAIMER.md). Only use this tool on windows and processes you own or have explicit permission to capture. If you do not agree, do not use this software.

[![CI](https://github.com/Londopy/capture-bypass/actions/workflows/release.yml/badge.svg)](https://github.com/Londopy/capture-bypass/actions/workflows/release.yml)
![Release](https://img.shields.io/github/v/release/Londopy/capture-bypass) 
[![License: MIT](https://img.shields.io/badge/License-MIT%20%28personal%29-yellow.svg)](https://github.com/Londopy/capture-bypass/blob/main/LICENSE)
![Files](https://img.shields.io/github/directory-file-count/Londopy/capture-bypass) 
![Issues](https://img.shields.io/github/issues/Londopy/capture-bypass) 
![Size](https://img.shields.io/github/languages/code-size/Londopy/capture-bypass)
[![Rust](https://img.shields.io/badge/rust-1.78%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078d4?logo=windows&logoColor=white)](https://github.com/Londopy/capture-bypass/releases/latest)
![GitHub Downloads](https://img.shields.io/github/downloads/Londopy/capture-bypass/total?)

---

## What is this?

Some apps on Windows intentionally block screen recording. You open OBS, try to capture a window, and it just shows up as a black box — even though the app is right there on your screen. This isn't OBS being broken. The app is actively telling Windows *"don't let anyone record me."*

**Capture Bypass removes that block.**

Once you run it on a window, OBS, the Snipping Tool, ShareX, and any other capture software can see it normally again.

**Common reasons people use this:**

- **Streaming or recording** — you want to share your screen but an app (a media player, a game launcher, a video player) goes black in OBS
- **Taking a screenshot** — Windows + Shift + S just captures a black rectangle where the app should be
- **Screen sharing** — you're in a meeting and need to show someone a window that refuses to appear on their end
- **Recording a tutorial** — you're making a guide for something but the app keeps disappearing from the recording

It works on basically any app. Media players, certain games, some messaging apps, video software — if it's showing up black in OBS, this is probably why, and this fixes it.

**This is a tool, not a hack.** It works entirely through a documented Windows API. No sketchy stuff, no kernel exploits. The only reason it needs Administrator is because that's what Windows requires to interact with another process.

---

## Download

Pre-built binaries are on the [Releases](../../releases/latest) page — no Rust or build tools needed.

| Download | What's inside |
|---|---|
| `capture-bypass-setup-*.exe` | Windows installer (x64) — picks install path, shortcuts, optional startup entry |
| `capture-bypass-*-portable-x64.zip` | Portable build for x64 PCs |
| `capture-bypass-*-portable-arm64.zip` | Portable build for ARM64 (Snapdragon X, Surface) |

The installer adds a desktop shortcut, Start Menu entry, and optionally a **Launch at Windows startup** entry (UAC prompt will show on each login since the app needs admin rights).

Want a portable layout or a custom build? See the [Build](#build) section.

> **Note:** Windows Defender or other AV software may flag the payload DLLs because of the DLL injection technique. This is a false positive — see [DISCLAIMER.md](DISCLAIMER.md).

---

## How it works

Windows exposes [`SetWindowDisplayAffinity`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity), which lets a process mark its own windows as protected from screen capture. Protected windows show up as black/blank in OBS and screenshots.

Since the API only lets a process touch *its own* windows, bypassing it means running code inside the target. This tool does that with **DLL injection**:

1. `OpenProcess` — get a handle to the target with VM + thread access.
2. `VirtualAllocEx` / `WriteProcessMemory` — write the payload DLL path into the target's memory.
3. `CreateRemoteThread(LoadLibraryA)` — spin up a thread in the target that loads the DLL.
4. The DLL's `DllMain` calls `SetWindowDisplayAffinity(hwnd, WDA_NONE)` on every window that process owns.

---

## Project structure

```
capture-bypass/
├── core/                       Shared injection library (inject_dll, inject_dll_stealth, InjectError)
├── cli/                        CLI binary
├── gui/                        egui GUI
├── payload_dll/                One-shot payload DLL (strips once and exits)
├── payload_dll_persistent/     Persistent payload DLL (hooks SetWindowDisplayAffinity, re-strips every 500 ms)
├── stress_tester/              Stress-test utility with Fight Mode
└── installer/
    └── capture-bypass.iss      Inno Setup 6 script
```

---

## Requirements

| Dependency | Notes |
|---|---|
| [Rust + Cargo](https://rustup.rs) | Stable toolchain, `x86_64-pc-windows-msvc` target |
| Windows 10 2004+ | `WDA_EXCLUDEFROMCAPTURE` needs build 19041+ |
| Administrator rights | `OpenProcess` on other processes requires it |

---

## Build

### x64 (main)

```powershell
git clone https://github.com/Londopy/capture-bypass.git
cd capture-bypass

# Build everything — GUI, CLI, stress tester, both payload DLLs
cargo build --release -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester
```

### x86 (optional — needed to inject into 32-bit processes)

The GUI auto-detects 32-bit targets (orange **32** badge) and uses the x86 binaries when they're present. Skip this if you don't need 32-bit support.

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
```

x86 binaries land in `target/i686-pc-windows-msvc/release/`. The GUI picks them up automatically.

### ARM64 (native build for Snapdragon X / Surface)

```powershell
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester
```

---

## Usage

```powershell
target\release\capture_bypass_gui.exe
```

1. The app lists all visible, titled windows with their PID, process icon, name, and live protection status (refreshed every 500 ms). Click any column header to sort.
2. Use the **Filter** bar to search by process name, window title, or PID.
3. Tick **Protected only** to hide unprotected windows.
4. Click **Strip Protection** on any row, or **⚡ Strip All Protected** to clear everything at once.
5. Toggle **Mode** between *One-shot* (strips once, fast) and *Persistent* (re-strips every 500 ms — for apps that keep re-applying protection). If an app re-applies after a one-shot, a popup offers to escalate to persistent automatically.
6. Enable **🤖 Auto-inject** to run in the background — it scans every 500 ms, strips newly protected windows automatically, escalates to persistent mode if a process fights back, and skips anything the OS has locked down with mitigation policies. You can just leave it on and forget about it.
7. Enable **🚀 Start with Windows** to add a startup registry entry so the app launches at login.
8. Click **📖 Help** to open the built-in docs.
9. Use the **Watch** bar to pin specific process names — they get stripped automatically whenever they appear, independent of auto-inject.
10. Click **⌨ Hotkey** to register **Ctrl+Shift+B** as a global "Strip All Protected" shortcut, works even when minimised to tray.
11. Click **🔔 Toasts** to get Windows desktop notifications whenever auto-inject strips something.
12. Click **📋 Log** to open the scrollable injection log panel.
13. Open **⚙ Settings** to configure tray behavior, hotkey, toasts, and an optional **injection log file** (`%APPDATA%\capture-bypass\injection.log`) that records every strip with a timestamp and which mode was used.
14. When a newer release is detected, a **🆕 v{x.y.z} available** button appears in the header.

All settings are saved automatically to `%APPDATA%\capture-bypass\config.toml`.

### Browsers

Injecting Chrome, Edge, Firefox, Brave, Opera, Vivaldi, or Thorium automatically covers all child processes too, since the window's renderer lives in a child process rather than the main browser PID.

---

## Testing

Use the included **Stress Tester** to verify injection works. Launch it from the **🔨 Stress Test** button in the header, or directly:

```powershell
target\release\stress_tester.exe
```

The stress tester marks itself as `WDA_EXCLUDEFROMCAPTURE` on launch and polls `GetWindowDisplayAffinity` every 100 ms to show live protection state. Use it to:

- **Verify one-shot injection** — click *Strip Protection*; the badge should flip to OK.
- **Stress-test persistent mode** — enable *Fight Mode* (adjustable 50–2000 ms re-apply rate) and inject the persistent DLL. The strip counter should keep climbing while the fight counter stays ahead.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

**Personal / open-source use:** MIT — see [LICENSE](LICENSE).

**Commercial use (companies, paid products, enterprise):** a separate license is required. Email **Londopy@protonmail.com** or see [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md).

## Disclaimer

See [DISCLAIMER.md](DISCLAIMER.md).
