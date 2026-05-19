# Contributing

Contributions are welcome. Read this before opening a pull request.

## Ground rules

- Everything must comply with [DISCLAIMER.md](DISCLAIMER.md) and [TERMS_OF_SERVICE.md](TERMS_OF_SERVICE.md).
- Don't submit code whose main purpose is to help with copyright infringement or unauthorized system access.
- Be cool in issues and PRs.

## Getting started

```powershell
git clone https://github.com/Londopy/capture-bypass.git
cd capture-bypass

# Build everything
cargo build --release -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester

# Run the GUI (needs Administrator)
target\release\capture_bypass_gui.exe
```

For 32-bit target support, build the x86 payload DLLs too:

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
```

For ARM64:

```powershell
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli -p gui -p stress_tester
```

See [Installation & Build](wiki/Installation-and-Build.md) for full setup details.

## Project layout

| Path | Language | Purpose |
|---|---|---|
| `core/` | Rust | Shared injection library (`inject_dll`, `inject_dll_stealth`, `InjectError`) |
| `cli/` | Rust | CLI binary |
| `gui/` | Rust / egui | GUI with auto-inject, watch mode, hotkey, toasts, log panel, settings |
| `payload_dll/` | Rust | One-shot payload (strips once and exits) |
| `payload_dll_persistent/` | Rust | Persistent payload (hooks `SetWindowDisplayAffinity`, keeps re-stripping) |
| `stress_tester/` | Rust | Self-protecting test window with Fight Mode, Scenario A, Scenario B |
| `installer/` | Inno Setup | Installer script (`capture-bypass.iss`) |

## Submitting changes

1. Fork the repo and make a branch: `git checkout -b my-feature`.
2. Make your changes. Run `cargo clippy` and fix any warnings.
3. Update `CHANGELOG.md` under `[Unreleased]` (Keep a Changelog format).
4. Open a PR with a clear description of what changed and why.

## Reporting bugs

Open a GitHub Issue and include:
- Your Windows version (`winver`)
- Target process name and whether it's 32-bit or 64-bit
- The full error from the status bar, injection log panel, or stderr

## License note

By contributing, you agree your code will be released under the same MIT license as the rest of the project. If a company wants to use this commercially, they need to reach out separately — see [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md).
