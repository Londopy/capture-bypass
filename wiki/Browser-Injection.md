# Browser Injection

## Why browsers need special handling

Chrome, Edge, Firefox, Brave, Opera, Vivaldi, and Thorium use a **multi-process architecture**. Each tab, extension, and renderer runs in its own child process for security isolation (sandboxing).

Some video streaming sites decode and render video inside a **renderer child process** — not the main browser process. That child process is the one that calls `SetWindowDisplayAffinity` to protect the video window. Injecting only into the main browser PID will not strip the video window, because the main process does not own it.

---

## What capture-bypass does automatically

When you click **Strip Protection** (or **⚡ Strip All Protected**) on any browser row, the app:

1. Injects the payload into the **main browser PID** shown in the table
2. Calls `CreateToolhelp32Snapshot` to enumerate all processes on the system
3. Finds every process whose **parent PID** matches the main browser PID
4. Injects the payload into **each child PID** as well

This covers the renderer that owns the protected video window regardless of which child it lands in.

### Supported browsers

| Browser | Executable |
|---|---|
| Google Chrome | `chrome.exe` |
| Microsoft Edge | `msedge.exe` |
| Mozilla Firefox | `firefox.exe` |
| Brave | `brave.exe` |
| Opera | `opera.exe` |
| Vivaldi | `vivaldi.exe` |
| Thorium | `thorium.exe` |

Detection is by executable name (case-insensitive). Any browser not on this list is treated as a regular single-process application.

---

## Tips

- **If the video goes black again after navigating to a new stream:** The browser may have spawned a new renderer process. Click **Strip Protection** again — or enable **🤖 Auto-inject** so it's handled automatically.
- **Multiple browser windows open:** Each window may have its own renderer. The batch injection covers all children, so all windows are handled in one click.
- **Persistent mode with browsers:** Recommended for sites that re-apply protection frequently. The persistent DLL running inside each renderer process keeps them clear continuously.
- **32-bit browsers:** Rare on modern systems, but if a browser row shows an orange **32** badge, the x86 payload DLL will be used automatically (if built — see [Installation & Build](Installation-and-Build)).
