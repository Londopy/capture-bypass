# Contributing

Contributions are welcome. Please read this document before opening a pull request.

## Ground rules

- All contributions must comply with the conditions in [DISCLAIMER.md](DISCLAIMER.md).
- Do not submit code whose primary purpose is to facilitate copyright infringement
  or unauthorised access to systems.
- Be respectful in issues and pull requests.

## Getting started

```powershell
git clone https://github.com/Londopy/windows_capture_bypass.git
cd windows_capture_bypass
cargo build                 # debug build, all crates
pip install customtkinter
python frontend/app.py
```

## Project layout

| Path | Language | Purpose |
|---|---|---|
| `core/` | Rust | Shared injection library |
| `cli/` | Rust | Subprocess-friendly CLI binary |
| `gui/` | Rust | Standalone egui GUI |
| `payload_dll/` | Rust | DLL injected into target processes |
| `frontend/` | Python | customtkinter GUI frontend |
| `test_protection.py` | Python | End-to-end test harness |

## Submitting changes

1. Fork the repository and create a branch: `git checkout -b my-feature`.
2. Make your changes. Run `cargo clippy` and fix any warnings.
3. Update `CHANGELOG.md` under `[Unreleased]` using the Keep a Changelog format.
4. Open a pull request with a clear description of what changed and why.

## Reporting bugs

Open a GitHub Issue and include:
- Your Windows version (`winver`)
- The target process name and whether it's 32-bit or 64-bit
- The full error message from the status bar or stderr
