# Testing

capture-bypass ships a dedicated stress-test window for verifying injection end-to-end without needing a real DRM application.

---

## Running the stress tester

Two versions are available — they are functionally identical:

| Version | How to run | Requires |
|---|---|---|
| **Rust** (recommended) | `target\release\stress_tester.exe` | Just build the crate (included in release zip) |
| **Python** (original) | `python test_protection.py` | Python 3.10+ |

```powershell
# Rust version (included in the release zip — no build needed)
target\release\stress_tester.exe

# Or build it yourself
cargo build --release -p stress_tester
target\release\stress_tester.exe

# Python version
python test_protection.py
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

- Set the fight interval **below 500 ms** (e.g. 100 ms) to simulate an aggressive app. The persistent DLL re-strips every 500 ms, so it will lose the race. This is expected — it shows the limits of the 500 ms persistent interval.
- Set the fight interval **above 500 ms** (e.g. 1000 ms) to simulate a typical DRM app. Persistent mode wins comfortably.
- Click **↺ Reset Counters** at any time to zero both counters for a fresh test.

---

## Manual protection buttons

The stress tester also has two buttons for manual control:

- **🔴 Apply Protection** — manually re-applies `WDA_EXCLUDEFROMCAPTURE`
- **✅ Remove Protection** — manually clears to `WDA_NONE`

These are useful for quick before/after screenshots or OBS source tests.
