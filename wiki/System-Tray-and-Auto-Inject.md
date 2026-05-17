# System Tray & Auto-inject

## System tray

Clicking the window's **✕ close button** hides the app to the system tray instead of quitting. The capture-bypass icon remains in the notification area (bottom-right corner of the taskbar).

This lets the app keep running in the background — useful for streamers who want auto-inject active while playing a full-screen game or watching video.

### Tray icon menu

Right-click the tray icon to get:

| Item | Action |
|---|---|
| **Open** | Restores the main window and brings it to the foreground |
| **Quit** | Fully exits the application, stopping all background threads |

### Fully quitting

To exit completely, use **Quit** from the tray menu. Clicking ✕ only hides the window — the process continues running.

---

## 🤖 Auto-inject

The **Auto-inject** toggle (in the filter bar) starts a background thread that automatically strips any window that becomes protected.

### How it works

1. Every **500 ms**, the thread calls `GetWindowDisplayAffinity` on all visible windows.
2. Any window that is protected **and has not been seen before** in this session is injected immediately.
3. Once a PID has been injected, it is added to the "seen" set and skipped on future scans — preventing repeated injection of the same process.
4. Browser processes are handled the same way as manual injection: all child PIDs are injected alongside the main PID.

### Resetting the seen set

The seen set is cleared when you **toggle Auto-inject off and back on**. This is useful if a process was injected but then restarted — toggling resets the memory so it gets injected fresh.

### Recommended workflow for streamers

1. Enable **🔁 Persistent** mode (so any injected process stays clear indefinitely).
2. Enable **🤖 Auto-inject**.
3. Enable **🚀 Start with Windows** so the app is always ready when you boot.
4. Click **✕** to minimize to tray.
5. Stream normally — any app that tries to block capture is handled silently.

### Limitations

- Auto-inject only fires on windows that have *not been seen before*. If a process is already in the seen set and re-applies protection (e.g. it restarted), toggle Auto-inject off and back on to reset.
- The 500 ms polling interval means there may be up to a 500 ms window where the capture is black before injection fires. For most use cases this is imperceptible.

---

## 🚀 Start with Windows

The **Start with Windows** toggle (in the filter bar) writes or removes a registry entry so capture-bypass launches automatically at every login.

### How it works

Clicking the toggle writes this registry value:

```
HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run
  "capture-bypass" = "C:\...\capture_bypass_gui.exe"
```

Clicking it again deletes that value. The button always reflects the real registry state — if you modify the entry externally, the button will show the correct status next time you open the app.

### UAC note

Because the app requires **Administrator** privileges (it must inject into other processes), Windows will show a **UAC elevation prompt** each time it auto-launches at login. This is unavoidable — the app's manifest mandates elevation. Simply click **Yes** to allow it, or leave the startup option off if the prompt is inconvenient.

The startup option is also available as an optional checkbox during installer setup.
