# Contributing

Contributions are welcome. Please read this document before opening a pull request.

## Ground rules

- All contributions must comply with the conditions in [DISCLAIMER.md](DISCLAIMER.md) and [TERMS_OF_SERVICE.md](TERMS_OF_SERVICE.md).
- Do not submit code whose primary purpose is to facilitate copyright infringement
  or unauthorised access to systems.
- Be respectful in issues and pull requests.

## Getting started

```powershell
git clone https://github.com/Londopy/capture-bypass.git
cd capture-bypass

# Build all Rust crates — GUI, CLI, stress tester, and both payload DLLs
cargo build --release -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester

# Run the GUI (requires Administrator)
target\release\capture_bypass_gui.exe
```

For 32-bit target support, also build the x86 payload DLLs:

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
```

See [Installation & Build](wiki/Installation-and-Build.md) for full setup details.

## Project layout

| Path | Language | Purpose |
|---|---|---|
| `core/` | Rust | Shared injection library (`inject_dll`, `inject_dll_stealth`, `InjectError`) |
| `cli/` | Rust | Subprocess-friendly CLI binary |
| `gui/` | Rust / egui | Full-featured GUI with auto-inject, watch mode, hotkey, toasts, and log panel |
| `payload_dll/` | Rust | One-shot payload DLL (strips once and exits) |
| `payload_dll_persistent/` | Rust | Persistent payload DLL (re-strips every 500 ms) |
| `stress_tester/` | Rust | Self-protecting stress-test window with Fight Mode, Scenario A, and Scenario B |
| `installer/` | Inno Setup | Installer script (`capture-bypass.iss`) |

## Submitting changes

1. Fork the repository and create a branch: `git checkout -b my-feature`.
2. Make your changes. Run `cargo clippy` and fix any warnings before submitting.
3. Update `CHANGELOG.md` under `[Unreleased]` using the Keep a Changelog format.
4. Open a pull request with a clear description of what changed and why.

## Reporting bugs

Open a GitHub Issue and include:
- Your Windows version (`winver`)
- The target process name and whether it's 32-bit or 64-bit
- The full error message from the status bar, injection log panel, or stderr
