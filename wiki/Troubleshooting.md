# Troubleshooting

---

## "DLL not found" error in the status bar

The payload DLLs haven't been built yet, or the executable is not next to the DLLs.

**Fix:**
```powershell
cargo build --release -p payload_dll -p payload_dll_persistent
```
Make sure `capture_bypass_gui.exe`, `payload_dll.dll`, and `payload_dll_persistent.dll` are all in the same folder. If you downloaded the release zip, they should already be together — don't move just the exe.

---

## "Strip failed — try Administrator" or access denied

`OpenProcess` requires `SeDebugPrivilege` (or process ownership) to open a handle to another process. Without it, the call returns `ERROR_ACCESS_DENIED`.

**Fix:** Right-click `capture_bypass_gui.exe` → **Run as administrator**. The app requests UAC elevation automatically via its manifest.

---

## Injection succeeds but the window is still black in OBS

**Step 1 — Wait for the next refresh cycle.**
The status badge updates every 500 ms. If it now shows 🟢 **OK**, the injection worked. OBS may need a moment to pick up the change — try removing and re-adding the Window Capture source.

**Step 2 — For browsers, inject again.**
If you're capturing a browser tab showing DRM video, the renderer process (not the main browser) owns the window. Click **Strip Protection** on the browser row — child process injection is automatic.

**Step 3 — The app is re-applying protection.**
Some apps call `SetWindowDisplayAffinity` on a timer. Switch to **🔁 Persistent** mode in the toolbar, then click **Strip Protection** again. The persistent DLL will continuously re-strip every 500 ms.

**Step 4 — OBS capture type.**
Make sure OBS is using **Window Capture** (not Display Capture). Window Capture respects the per-window affinity; Display Capture captures the entire desktop and may work even without injection on some setups.

---

## The app is flagged by antivirus

DLL injection is a technique used by both legitimate tools (debuggers, overlays, accessibility software) and malware. Heuristic AV scanners may flag the payload DLLs.

**The DLLs only call `SetWindowDisplayAffinity`.** You can inspect the source yourself:
- `payload_dll/src/lib.rs`
- `payload_dll_persistent/src/lib.rs`

**Fix:** Add an exclusion for the `target\release\` folder in your AV settings, or for the specific DLL files.

---

## 32-bit injection fails even with x86 binaries present

A 64-bit process cannot inject into a 32-bit process, and vice-versa. The app detects 32-bit target processes (orange **32** badge) and routes them through the x86 payload DLLs automatically — but only if they have been built.

**Fix:**
```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent
```

The x86 DLLs land in `target\i686-pc-windows-msvc\release\`. The app finds them automatically.

---

## Window disappears from the list immediately after appearing

Some applications create and destroy windows rapidly (splash screens, popups). The 500 ms refresh means transient windows may not appear long enough to inject manually.

**Fix:** Enable **🤖 Auto-inject** — it fires within 500 ms of any protected window appearing, without requiring manual interaction.

---

## App minimizes to tray and I can't get it back

Right-click the capture-bypass icon in the system tray (bottom-right corner, may be hidden under the ^ arrow) and click **Open**.

If the tray icon is not visible, check the "hidden icons" overflow area by clicking the ^ arrow in the system tray.

---

## The status badge doesn't update after injection

The background refresh thread runs every 500 ms. If the badge hasn't updated after ~1 second:

1. Click **⟳ Refresh** to force an immediate enumeration.
2. If the badge is still **PROTECTED**, the injection likely failed — check the status bar for an error message.

---

## Getting help

If none of the above resolves your issue, [open a GitHub Issue](https://github.com/Londopy/capture-bypass/issues) and include:

- Your Windows version (`winver`)
- The target process name and whether it's 32-bit or 64-bit (check the **Arch** column)
- The full error message from the status bar
- Whether you're running as Administrator
