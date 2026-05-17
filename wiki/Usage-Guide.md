# Usage Guide

## Starting the app

Right-click `capture_bypass_gui.exe` → **Run as administrator**. Administrator privileges are required for `OpenProcess` to open handles to other processes.

The app requests UAC elevation automatically via its embedded manifest — you will see a UAC prompt on launch.

---

## Window list

The main panel shows every visible, titled window on the system, refreshed every **500 ms** in the background.

| Column | Description |
|---|---|
| PID | Process ID |
| Process | Executable filename. An orange **32** badge means the process is 32-bit. |
| Arch | **32** (orange) or **64** (gray) |
| Window Title | Title bar text, truncated if long |
| Status | Live protection state (see below) |
| Action | **Strip Protection** button for that row |

### Status badges

| Badge | Meaning |
|---|---|
| 🔴 **PROTECTED** | `WDA_EXCLUDEFROMCAPTURE` — window appears black in OBS |
| 🟡 **MONITOR** | `WDA_MONITOR` — protected from screen capture to secondary monitors |
| 🟢 **OK** | `WDA_NONE` — window is fully capturable |

---

## Header toolbar

### ⟳ Refresh
Forces an immediate re-enumeration of all windows. The background thread refreshes automatically every 500 ms, so this is only needed if you want an instant update.

### ⚡ Strip All Protected
One click injects into every currently-protected PID. Deduplicates so each process is injected only once, even if it owns multiple protected windows. Browser processes automatically have their child processes injected too.

### Mode toggle
Switches between injection modes. See [Injection Modes](Injection-Modes) for details.

- **⚡ One-shot** (default) — strips once and exits
- **🔁 Persistent** — re-strips every 500 ms for the process lifetime

### 📖 Help
Opens the built-in help panel — a sidebar with 7 sections covering all features. The wiki you're reading now mirrors and expands on that content.

---

## Filter bar

- **Filter text box** — live search by window title, process name, or PID. Filtering is case-insensitive.
- **✕ button** — clears the filter.
- **Protected only** checkbox — hides all rows where Status is OK. Useful when many windows are open.

---

## 🤖 Auto-inject

Toggles a background thread that watches for newly protected windows and injects them automatically. See [System Tray & Auto-inject](System-Tray-and-Auto-Inject) for full details.

---

## Status bar

The bottom bar shows the result of the last action with a timestamp:

- **Green** — injection succeeded
- **Red** — injection failed (error message included)
- **Gray** — neutral info (mode change, refresh, etc.)

The right side shows a live count: `N windows • N protected • N 32-bit`.

---

## Typical workflow

1. Launch the app as Administrator.
2. Open the application you want to capture (e.g. a DRM video player, a game).
3. The protected window appears with a 🔴 **PROTECTED** badge.
4. Click **Strip Protection** on that row. The badge should flip to 🟢 **OK** within one 500 ms cycle.
5. Start your screen capture in OBS — the window is now visible.

If the target re-applies protection after a few seconds, switch to **🔁 Persistent** mode and inject again. If you want completely hands-free operation, enable **🤖 Auto-inject** and minimize to tray.
