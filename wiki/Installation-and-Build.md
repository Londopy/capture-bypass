# Installation & Build

## Option A — Download and run the installer (easiest)

Go to the [Releases page](https://github.com/Londopy/capture-bypass/releases/latest) and download:

```
capture-bypass-setup-X.Y.Z-x64.exe
```

Run it and follow the prompts. You can choose:

- **Install directory** (defaults to `C:\Program Files\capture-bypass`)
- **Desktop shortcut** — checked by default
- **Start Menu shortcut** — checked by default
- **Launch at Windows startup** — unchecked by default; adds the app to `HKCU\...\Run` so it starts with Windows. Because the app requires Administrator rights, Windows will show a UAC elevation prompt at each login.

At the end, click **"Launch capture-bypass"** to start it immediately.

---

## Option B — Download the portable zip

If you prefer no installer, download the zip instead:

| Bundle | Contents | Who it's for |
|---|---|---|
| `capture-bypass-gui-vX.Y.Z-x64.zip` | `capture_bypass_gui.exe`, `stress_tester.exe`, `payload_dll.dll`, `payload_dll_persistent.dll`, `x86\*.dll` | Anyone — no Python or Rust required |
| `capture-bypass-python-vX.Y.Z.zip` | `app.py` + all binaries pre-arranged | Python users who prefer the Python frontend |

Unzip anywhere, right-click `capture_bypass_gui.exe` → **Run as administrator**.

> **Antivirus note:** Windows Defender or other AV software may flag the payload DLLs due to the DLL injection technique. This is a false positive — the DLL only calls `SetWindowDisplayAffinity`. See [DISCLAIMER.md](https://github.com/Londopy/capture-bypass/blob/main/DISCLAIMER.md).

---

## Option B — Build from source

### Requirements

| Dependency | Notes |
|---|---|
| Windows 10 build 19041+ | `WDA_EXCLUDEFROMCAPTURE` requires the 2004 update |
| Administrator privileges | Required by `OpenProcess` on other processes |
| [Rust + Cargo](https://rustup.rs) | Stable toolchain, `x86_64-pc-windows-msvc` target |
| Python 3.10+ | Only needed for the Python frontend (`frontend/app.py`) |
| [customtkinter](https://github.com/TomSchimansky/CustomTkinter) + pystray + Pillow | Python frontend deps only |

### x64 build (required)

```powershell
git clone https://github.com/Londopy/capture-bypass.git
cd capture-bypass

# Build everything
cargo build --release -p payload_dll -p payload_dll_persistent -p gui -p stress_tester

# Rust GUI — no Python needed
target\release\capture_bypass_gui.exe

# Or install Python deps and use the Python frontend
pip install customtkinter pystray pillow
python frontend\app.py
```

Binaries land in `target\release\`.

### x86 build (optional — for 32-bit targets)

The app auto-detects 32-bit processes (shown with an orange **32** badge) and routes injection through the x86 payload DLLs when they are present. If you skip this step, 32-bit targets fail gracefully with a status-bar error.

```powershell
rustup target add i686-pc-windows-msvc

cargo build --release --target i686-pc-windows-msvc `
    -p payload_dll `
    -p payload_dll_persistent
```

The x86 binaries land in `target\i686-pc-windows-msvc\release\`. The app picks them up automatically — no configuration required.

---

## File layout after a full build

```
target\
├── release\
│   ├── capture_bypass_gui.exe       ← Rust GUI (run as Admin)
│   ├── stress_tester.exe            ← Rust stress-test window
│   ├── cli.exe                      ← CLI binary (used by Python frontend)
│   ├── payload_dll.dll              ← One-shot payload
│   └── payload_dll_persistent.dll  ← Persistent payload
└── i686-pc-windows-msvc\release\
    ├── cli.exe                      ← x86 CLI (optional)
    ├── payload_dll.dll              ← x86 one-shot payload (optional)
    └── payload_dll_persistent.dll  ← x86 persistent payload (optional)
```
