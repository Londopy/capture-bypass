# Testing

capture-bypass ships a dedicated stress-test window for verifying injection end-to-end without needing a real DRM application.

---

## Running the stress tester

The stress tester is included in the installer. Launch it from the **🔨 Stress Test** button in the GUI header, or run it directly:

```powershell
# If installed:
"C:\Program Files\capture-bypass\stress_tester.exe"

# If built from source:
target\release\stress_tester.exe
```

No Administrator rights are required — `SetWindowDisplayAffinity` works on your own windows without elevation.

---

## What the stress tester does

On launch, the window:

1. Applies `WDA_EXCLUDEFROMCAPTURE` to itself — the window appears black in OBS immediately.
2. Starts a **monitor thread** that polls `GetWindowDisplayAffinity` every **100 ms** and updates the display.
3. Changes the **window background** and **title bar text** based on protection state, so the result is visible in OBS as well as in the window itself.

| State | Background | Title |
|---|---|---|
| Protected (`WDA_EXCLUDEFROMCAPTURE`) | Deep red | 🔴 PROTECTED — Capture Bypass Stress Tester |
| Clear (`WDA_NONE`) | Deep green | ✅ NOT PROTECTED — Capture Bypass Stress Tester |
| Monitor-only (`WDA_MONITOR`) | Deep red | 🟡 MONITOR-ONLY — Capture Bypass Stress Tester |

---

## Verifying one-shot injection

1. Launch `stress_tester.exe` — it opens with a red background (protected).
2. Launch `capture_bypass_gui.exe` as Administrator.
3. Find the stress tester row in the window list (filter for "stress" if needed).
4. Click **Strip Protection**.
5. The stress tester window should turn **green** within one 100 ms poll cycle.
6. The **External strips** counter increments by 1.

---

## Stress-testing persistent mode (Fight Mode)

Fight Mode simulates an application that actively resists injection by re-applying `WDA_EXCLUDEFROMCAPTURE` on a timer.

### Enabling Fight Mode

1. In the stress tester, adjust the **Re-apply every** slider (50–2000 ms, default 500 ms).
2. Click **▶ Start Fight Mode**.
3. The **Fight re-applies** counter starts climbing — each tick re-protects the window.

### Beating Fight Mode with persistent injection

1. In the main app, switch to **🔁 Persistent** mode.
2. Click **Strip Protection** on the stress tester row.
3. Watch the counters:
   - **Fight re-applies** keeps climbing (the stress tester is fighting back)
   - **External strips** also increments — the persistent DLL is re-stripping every 500 ms
4. The window should stay **green** once the persistent DLL is installed, because it re-strips at the same or faster rate than the fight interval.

### Fight Mode tips

- Set the fight interval **below 500 ms** (e.g. 100 ms) to simulate an aggressive app. The persistent DLL re-strips every 500 ms, so it will lose the race — this is expected and shows the limits of the persistent interval.
- Set the fight interval **above 500 ms** (e.g. 1000 ms) to simulate a typical DRM app. Persistent mode wins comfortably.
- Click **↺ Reset Counters** at any time to zero both counters for a fresh test.

---

## Scenario A — Process enumeration  (`TH32CS_SNAPPROCESS`)

Some applications defend themselves by scanning the running-process list and re-applying protection whenever they see the injector's exe name.

The stress tester can simulate this: when **Scenario A** is active, it polls `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` every 250 ms. If it finds a process whose name matches the configured **Injector name** (default `capture_bypass_gui.exe`), it immediately re-applies `WDA_EXCLUDEFROMCAPTURE` and increments the **Detections** counter.

### How to use it

1. Start Scenario A with the default injector name.
2. Launch `capture_bypass_gui.exe` — detections climb and the window stays protected.
3. Now test the **defence**: capture-bypass renames its injected DLL to a random `.tmp` and uses `inject_dll_stealth`. If you also renamed the injector exe itself, Scenario A finds nothing and detections stay at 0.

The **Injector name** field is live — you can change it while the thread runs.

---

## Scenario B — Module ejection  (`TH32CS_SNAPMODULE`)

A more aggressive defence: the target app periodically scans its **own loaded modules** and calls `FreeLibrary` on anything that looks like an injected payload, then re-applies protection.

The stress tester simulates this by polling `CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, self_pid)` every 250 ms. Any module whose name contains the **DLL name pattern** (default `payload_dll`) gets `FreeLibrary`'d immediately, protection is re-applied, and the **Modules ejected** counter increments.

### Also eject .tmp modules

The **stealth injection** in capture-bypass copies the payload to `%TEMP%\<random>.tmp` before loading it, so the module name visible in the snapshot is an opaque hex string rather than `payload_dll.dll`. Tick **Also eject .tmp modules** to test whether a paranoid app can still catch the stealth copy by ejecting all `.tmp` modules regardless of name.

| Scenario B config | Non-stealth injection | Stealth injection |
|---|---|---|
| Pattern: `payload_dll`, no .tmp | ❌ Ejected immediately | ✅ Survives (name is a `.tmp`) |
| Pattern: `payload_dll`, .tmp ON | ❌ Ejected | ❌ Ejected (even the `.tmp` is caught) |

### How to use it

1. Start Scenario B (pattern: `payload_dll`, .tmp unchecked).
2. Inject the non-stealth DLL (`payload_dll.dll`) — ejections climb.
3. Switch to stealth injection — ejections stay at 0, confirming the rename defence works.
4. Tick **Also eject .tmp modules** and re-inject stealth — ejections climb again, showing a sufficiently paranoid app can still evict it.

---

## Manual protection buttons

The stress tester has two buttons for manual control:

- **🔴 Apply Protection** — manually re-applies `WDA_EXCLUDEFROMCAPTURE`
- **✅ Remove Protection** — manually clears to `WDA_NONE`

These are useful for quick before/after screenshots or OBS source tests without using the main app.
