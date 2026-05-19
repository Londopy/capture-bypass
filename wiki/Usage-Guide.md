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
| Process | Executable filename with its extracted icon. An orange **32** badge means the process is 32-bit. |
| Window Title | Title bar text, truncated if long |
| Status | Live protection state (see below) |
| Action | **Strip Protection** button for that row |

Click any column header to sort the list ascending ▲; click again to reverse ▼. The current sort column and direction are saved to config and restored on next launch.

### Status badges

| Badge | Meaning |
|---|---|
| 🔴 **PROTECTED** | `WDA_EXCLUDEFROMCAPTURE` — window appears black in OBS |
| 🟡 **MONITOR** | `WDA_MONITOR` — protected from screen capture to secondary monitors |
| 🟢 **OK** | `WDA_NONE` — window is fully capturable |

### Process icons

Each row displays the 16×16 icon of the executable as Windows would show it in Explorer. Icons are extracted once per process name and cached for the session. If the icon cannot be extracted (e.g. no file found, system process), the column is left blank.

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

### 🤖 Auto-inject
Toggles a background thread that silently strips any newly protected window. See [System Tray & Auto-inject](System-Tray-and-Auto-Inject) for full details. When active, newly protected windows are stripped without any manual interaction; if **🔔 Toasts** is also enabled, a desktop notification fires for each auto-strip.

### 🔔 Toasts
Enables Windows desktop toast notifications whenever the auto-inject thread silently strips a process. Each toast shows the process name and window title. Toasts are only sent by the background thread — manual **Strip Protection** clicks do not trigger them.

### ⌨ Hotkey
Registers **Ctrl+Shift+B** as a global hotkey. Pressing it at any time — even when the app is minimised to tray — strips all currently protected windows, identical to clicking **⚡ Strip All Protected**. Click the button again to unregister the hotkey. The hotkey state is saved to config.

### 📖 Help
Opens the built-in help panel — a sidebar with 7 sections covering all features. The wiki you're reading now mirrors and expands on that content.

### 🔨 Stress Test
Launches `stress_tester.exe`, a dedicated test window for verifying injection end-to-end. See [Testing](Testing) for full details.

### 🆕 Update banner
When a newer GitHub release is detected on startup, a **🆕 v{x.y.z} available** button appears in the header. Clicking it opens the Releases page in your default browser. No automatic update is performed — it is a notification only. The check runs once per launch in a background thread.

---

## Filter bar

- **Filter text box** — live search by window title, process name, or PID. Filtering is case-insensitive.
- **✕ button** — clears the filter.
- **Protected only** checkbox — hides all rows where Status is OK. Useful when many windows are open.

---

## Watch bar

The **Watch** bar lets you pin one or more process names for targeted auto-injection, independently of the global auto-inject toggle.

- Type a process name (e.g. `vlc.exe`) in the text box and press **Enter** or click **+ Add**.
- Any time a process matching that name appears on the system, its windows are stripped automatically.
- Remove a name by clicking **✕** next to it in the watch list.
- Watch names are saved to config and restored on next launch.

**Note:** When global **🤖 Auto-inject** is enabled it covers all protected windows, making watch mode redundant. Watch mode is most useful when you only want to target specific known applications without running auto-inject globally.

---

## Injection log

Click **📋 Log** to open the injection log panel at the bottom of the window. The panel shows a timestamped, scrollable history of every strip attempt and its result:

- **Green** entries — injection succeeded
- **Red** entries — injection failed, with an error message

The log persists for the session. Closing and reopening the panel does not clear it. The panel's open/closed state is saved to config.

---

## 🚀 Start with Windows

Adds (or removes) a Windows startup entry so capture-bypass launches automatically at every login.

Clicking the button writes `capture-bypass` into `HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`. Clicking it again removes the entry. The button reflects the real registry state on every launch.

**UAC note:** because the app requires Administrator rights, Windows will show a UAC elevation prompt each time it auto-launches at startup. This is unavoidable — simply click **Yes** to allow it. The option can also be enabled during installer setup.

See [System Tray & Auto-inject](System-Tray-and-Auto-Inject) for more details.

---

## Status bar

The bottom bar shows the result of the last action with a timestamp:

- **Green** — injection succeeded
- **Red** — injection failed (error message included)
- **Gray** — neutral info (mode change, refresh, etc.)

The right side shows a live count: `N windows • N protected • N 32-bit`.

---

## ⚙ Settings

Click the **⚙ Settings** button in the header to open the Settings window. Everything here is saved to config and restored on next launch.

| Setting | What it does |
|---|---|
| 🚀 Start with Windows | Adds/removes a startup registry entry so the app launches at login |
| 🔔 Toast notifications | Enables Windows desktop notifications for auto-inject strips |
| ⌨ Ctrl+Shift+B | Registers/unregisters the global hotkey |
| 🗕 Minimize to tray on close | ON = ✕ hides to tray; OFF = ✕ exits the app |
| 📋 Injection log file | Appends a timestamped line to `injection.log` for every strip attempt |

### Injection log file

When enabled, every injection result (success or failure) is logged to `%APPDATA%\capture-bypass\injection.log` with a UTC timestamp and the mode that was used. The file is created automatically the first time something gets logged.

Click **Open log file** in the Settings window to open it directly in Explorer. Useful if you want a record of what got stripped and when.

---

## Persistent settings

All UI state is saved automatically to `%APPDATA%\capture-bypass\config.toml` after every change and restored on next launch. Settings saved include: injection mode, auto-inject, protected-only filter, toast notifications, hotkey, log panel visibility, watch names, sort column, tray behavior, and injection log file toggle.

---

## Typical workflow

1. Launch the app as Administrator.
2. Open the application you want to capture (e.g. a DRM video player, a game).
3. The protected window appears with a 🔴 **PROTECTED** badge.
4. Click **Strip Protection** on that row. The badge should flip to 🟢 **OK** within one 500 ms cycle.
5. Start your screen capture in OBS — the window is now visible.

If the target re-applies protection after a few seconds, switch to **🔁 Persistent** mode and inject again. If you want completely hands-free operation, enable **🤖 Auto-inject** and minimise to tray. Enable **🚀 Start with Windows** so the app is ready the next time you boot. Add the process name to the **Watch** bar if you want targeted injection without running auto-inject globally.
