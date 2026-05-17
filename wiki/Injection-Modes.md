# Injection Modes

capture-bypass ships two payload DLLs with different lifetimes. You switch between them with the **Mode** toggle in the toolbar.

---

## ⚡ One-shot mode (default)

**Payload:** `payload_dll.dll`

The DLL is injected, strips `WDA_EXCLUDEFROMCAPTURE` from every window owned by the target process, and immediately exits. The DLL is unloaded by the OS shortly after `DllMain` returns.

**Use when:** The target application sets capture protection once — typically at startup — and never re-applies it. This covers the majority of apps.

**Pros:** Clean, lightweight, leaves no persistent code in the target process.

**Cons:** If the target re-applies protection on a timer, the window will go black again after the next re-apply cycle.

---

## 🔁 Persistent mode

**Payload:** `payload_dll_persistent.dll`

The DLL stays alive inside the target process and loops every **500 ms**, repeatedly calling `SetWindowDisplayAffinity(hwnd, WDA_NONE)` on all windows owned by that process.

**Use when:** The target application actively fights back — it has its own timer that re-applies `WDA_EXCLUDEFROMCAPTURE` periodically (common in DRM video players and some games). Persistent mode wins the race by checking more frequently than most apps re-protect.

**Pros:** Wins against any re-apply rate slower than 500 ms.

**Cons:** Leaves a thread running inside the target process for as long as the process is alive. Harmless, but worth knowing.

---

## Re-injection

Windows caches loaded DLLs by file path. If the same DLL path is already mapped into a process, `LoadLibraryA` silently no-ops — the DLL does not run again.

**If the status badge shows PROTECTED after a previous injection:**
- For one-shot: just click **Strip Protection** again. The first injection already unloaded, so the path is free to load again.
- For persistent: the DLL is still running. If protection came back, the target process may be loading the DLL from a different path, or a second DRM layer is involved. Try re-injecting anyway — it will re-run if the first copy was somehow unloaded.

---

## Choosing the right mode

```
App goes black once at startup → One-shot
App goes black again after a few seconds → Persistent
App goes black immediately even with Persistent → try Auto-inject + Persistent
```
