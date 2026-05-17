# Capture Bypass

A Windows utility that removes `WDA_EXCLUDEFROMCAPTURE` display-affinity protection from application windows, allowing them to be recorded normally by OBS, the Snipping Tool, and any other screen-capture software.

> **Legal notice:** Read [DISCLAIMER.md](DISCLAIMER.md) before use. Only use this tool on windows and processes you own or have explicit permission to capture.

![Relese](https://img.shields.io/github/v/release/Londopy/capture-bypass) 
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/Londopy/HideDesktopApps/blob/main/LICENSE)
![Files](https://img.shields.io/github/directory-file-count/Londopy/capture-bypass) 
![Issues](https://img.shields.io/github/issues/Londopy/capture-bypass) 
![Size](https://img.shields.io/github/languages/code-size/Londopy/capture-bypass) 

---

## Download

Pre-built binaries are available on the [Releases](../../releases/latest) page — no Rust or build tools required.

| Download | What's inside |
|---|---|
| `capture-bypass-python-*.zip` | The Python frontend (`app.py`) + all binaries pre-arranged. Install Python deps below, then run `python app.py`. |
| `capture-bypass-gui-*.zip` | Standalone `capture_bypass_gui.exe` — no Python needed, same feature set as the Python frontend. |

> **Note:** Windows Defender or other AV software may flag the payload DLLs due to the DLL injection technique. This is a false positive — see [DISCLAIMER.md](DISCLAIMER.md).

---

## How it works

Windows exposes [`SetWindowDisplayAffinity`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) which lets a process mark its own windows as protected from capture. Protected windows appear black/blank in screenshots and screen recordings.

Because the API only allows a process to modify *its own* windows, bypassing it requires running code inside the target process. This tool does that via classic **LoadLibrary DLL injection**:

1. `OpenProcess` — open a handle to the target with VM + thread rights.
2. `VirtualAllocEx` / `WriteProcessMemory` — write the payload DLL path into the target's address space.
3. `CreateRemoteThread(LoadLibraryA)` — start a thread inside the target that loads the DLL.
4. The DLL's `DllMain` spawns a worker thread that calls `SetWindowDisplayAffinity(hwnd, WDA_NONE)` on every window owned by that process, clearing the protection flag.

---

## Project structure

```
windows_capture_bypass/
├── core/                       Shared Rust library — inject_dll()
├── cli/                        Rust CLI binary — called by the Python frontend
├── gui/                        Rust/egui GUI frontend (feature-parity with app.py)
├── payload_dll/                One-shot payload DLL (strips once and exits)
├── payload_dll_persistent/     Persistent payload DLL (loops every 500 ms)
├── stress_tester/              Rust stress-test window (port of test_protection.py)
├── frontend/
│   └── app.py                  Python/customtkinter GUI frontend
└── test_protection.py          Python stress-test window (original)
```

---

## Requirements

| Dependency | Notes |
|---|---|
| [Rust + Cargo](https://rustup.rs) | Stable toolchain, `x86_64-pc-windows-msvc` target |
| Python 3.10+ | For the frontend |
| [customtkinter](https://github.com/TomSchimansky/CustomTkinter) | `pip install customtkinter` |
| [pystray](https://github.com/moses-palmer/pystray) + [Pillow](https://python-pillow.org) | `pip install pystray pillow` (system tray support) |
| Windows 10 2004+ | `WDA_EXCLUDEFROMCAPTURE` requires build 19041+ |
| Administrator privileges | Required by `OpenProcess` on other processes |

---

## Build

### x64 (required)

```powershell
# Clone the repo
git clone https://github.com/Londopy/windows_capture_bypass.git
cd windows_capture_bypass

# Build all Rust crates for 64-bit
cargo build --release -p payload_dll -p payload_dll_persistent -p cli

# Install Python dependencies
pip install customtkinter pystray pillow
```

### x86 (optional — needed to inject into 32-bit processes)

The frontend auto-detects 32-bit target processes (shown with an orange **32** badge) and
routes injection through the x86 binaries when they are present. If you skip this step,
32-bit targets will fail gracefully with a status-bar error message.

```powershell
# Add the 32-bit Windows MSVC target
rustup target add i686-pc-windows-msvc

# Build all Rust crates for 32-bit
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
```

The x86 binaries land in `target/i686-pc-windows-msvc/release/`. The frontend
picks them up automatically — no configuration needed.

---

## Usage

```powershell
# Python frontend
python frontend/app.py

# Rust GUI (no Python required)
target\release\capture_bypass_gui.exe
```

1. The app lists all visible, titled windows with their PID, process name, and live protection status.
2. Use the **Filter** bar to search by process name or window title.
3. Tick **Protected only** to hide unprotected windows.
4. Click **Strip Protection** on any row, or **⚡ Strip All Protected** to batch-clear everything at once.
5. Toggle **Mode** between *One-shot* (strips once) and *Persistent* (re-strips every 500 ms — for apps that fight back).
6. Enable **🤖 Auto-inject** to have the app silently strip any newly protected window in the background; close to tray so it keeps running.
7. Click **📖 Help** in the header to open the built-in wiki — covers injection modes, browser handling, 32-bit support, troubleshooting, and more.

The `cli.exe` and `payload_dll*.dll` binaries must be present in `target/release/` (and optionally `target/i686-pc-windows-msvc/release/`) relative to the repo root.

> **Built-in docs:** click **📖 Help** in the app header for a full wiki covering injection modes, browser handling, 32-bit support, and troubleshooting — no browser needed.

### Browsers

Injecting Chrome, Edge, Firefox, Brave, Opera, Vivaldi, or Thorium automatically enumerates
and injects all child processes, ensuring renderer processes that own the DRM video windows
are covered.

---

## Testing

Two stress-test windows are available — pick whichever suits your setup:

| Option | How to run | Requires |
|---|---|---|
| Rust (recommended) | `target\release\stress_tester.exe` | Just build the crate |
| Python (original) | `python test_protection.py` | Python 3.10+ |

Both are functionally identical. The Rust binary is included in the `capture-bypass-gui-*.zip` release bundle, so you can test without any build step.

```powershell
# Build and run the Rust stress tester
cargo build --release -p stress_tester
target\release\stress_tester.exe

# Or run the Python original
python test_protection.py
```

The window marks itself as `WDA_EXCLUDEFROMCAPTURE` on launch and polls `GetWindowDisplayAffinity`
every 100 ms to show the live state. Use it to:

- **Verify one-shot injection** — click *Strip Protection* in the main app; the window should flip green.
- **Stress-test persistent mode** — enable *Fight Mode* in the test window (adjustable 50–2000 ms re-apply rate) and inject the persistent DLL. The strip counter should climb while the fight counter stays ahead — until persistent mode catches up.

---

## Frontends

capture-bypass ships two frontends. Both use the same injection core.
Pick whichever suits your workflow.

| Frontend | Language | How to run | Notes |
|---|---|---|---|
| Native GUI | Rust / egui | `target/release/capture_bypass_gui.exe` | No dependencies, single .exe |
| Python GUI | Python / customtkinter | `python frontend/app.py` | Requires Python 3.10+ and pip deps |

Both frontends have feature-parity: window table with live protection badges, filter bar, one-shot / persistent mode toggle, batch strip, auto-inject, system tray, and built-in help.

### Build the Rust GUI

```powershell
cargo build --release -p gui
# Produces: target/release/capture_bypass_gui.exe
# The payload DLLs must be in the same directory — build them first:
cargo build --release -p payload_dll -p payload_dll_persistent
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

MIT — see [LICENSE](LICENSE).

## Disclaimer

See [DISCLAIMER.md](DISCLAIMER.md).
