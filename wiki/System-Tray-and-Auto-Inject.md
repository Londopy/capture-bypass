# System Tray & Auto-inject

## System tray

Clicking **✕** hides the app to the system tray instead of closing it. The capture-bypass icon stays in the notification area (bottom-right of the taskbar).

This lets the app keep running in the background, which is the main point — you want auto-inject active while you're playing a game or watching video full-screen.

### Tray icon menu

Right-click the tray icon:

| Item | Action |
|---|---|
| **Open** | Brings the main window back |
| **Quit** | Fully exits the app, stops all background threads |

To actually exit, use **Quit** from the tray. Clicking ✕ only hides the window.

---

## 🤖 Auto-inject

The **Auto-inject** toggle starts a background thread that watches for protected windows and handles them automatically. You can enable it and basically forget about it.

### How it works

Every **500 ms**, the thread scans all visible windows and checks their protection state. For each protected process it finds, it runs through a quick state machine:

1. **First time seeing this PID** — inject with whatever mode is currently selected (one-shot or persistent).
2. **Already injected once, protection came back** — escalate to persistent mode automatically. Logs "Escalated→persistent" in the log panel.
3. **Already on persistent, or gave up** — skip it. No point hammering a process that isn't cooperating.
4. **OS mitigation policy blocks injection** — log the reason once and skip the process permanently. Nothing user-mode can do about these.
5. **Process exited** — remove it from the state table so if it restarts, it gets treated as new again.

Browser processes (Chrome, Edge, Firefox, etc.) get their child processes injected too, same as manual injection.

### Resetting

Toggling auto-inject off and back on clears the internal state table. Useful if you want to force a re-injection on a process that was already handled.

### Recommended setup for streamers

1. Set mode to **🔁 Persistent** (so injected processes stay clear).
2. Turn on **🤖 Auto-inject**.
3. Turn on **🚀 Start with Windows** so the app is ready on every boot.
4. Click **✕** to minimize to tray.

That's it. Any app that tries to block capture gets handled silently. You don't have to touch the app again.

### Limitations

- There's up to a **500 ms gap** between when a window becomes protected and when the thread catches it. For most use cases this is barely noticeable.
- Processes with Windows mitigation policies (e.g. Microsoft-signed-only DLL requirement) can't be injected from user-mode regardless of what this tool does. Auto-inject will log the reason and leave them alone.

---

## 🚀 Start with Windows

The **Start with Windows** toggle writes or removes a registry entry so capture-bypass launches at login.

### What it does

Clicking it writes:

```
HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run
  "capture-bypass" = "C:\...\capture_bypass_gui.exe"
```

Clicking it again removes that entry. The button always shows the real registry state — if you edit the entry externally, the button will be accurate the next time you open the app.

### UAC note

Because the app needs **Administrator** rights to inject into other processes, Windows will show a **UAC prompt** every time it auto-launches at login. There's no way around this — the app's manifest requires elevation. Just click **Yes**, or leave startup off if you find it annoying.

The startup option is also available as a checkbox during installer setup.

---

## ⚙ Settings — Injection log file

In **Settings**, you can turn on an optional **injection log file** that appends a timestamped entry every time a process is stripped — including which mode was used and whether it succeeded.

The log is saved to `%APPDATA%\capture-bypass\injection.log`. Click **Open log file** in Settings to jump straight to it in Explorer.

This is off by default. Useful if you want a record of what got stripped and when.
