"""
test_protection.py — Capture Bypass stress tester

This window applies WDA_EXCLUDEFROMCAPTURE to itself, then optionally fights
back on a configurable timer.  Use it to verify that the injector (both
one-shot and persistent modes) can strip and hold off re-application.

Features
--------
• Live affinity polling   — reads GetWindowDisplayAffinity every 100 ms and
                            updates the indicator without any injection.
• Fight mode              — a background thread re-applies WDA_EXCLUDEFROMCAPTURE
                            at a rate you choose (50 – 2000 ms).  Simulates apps
                            that resist one-shot injection.
• Strip counters          — tracks how many times an external tool cleared the
                            affinity vs. how many times fight mode re-applied it.
• OBS-friendly            — window background and title change with protection
                            state so the result is obvious in any screen capture.

Requirements
------------
    pip install customtkinter
    (no pywin32 — pure ctypes)
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wintypes
import threading
import time
import tkinter as tk

import customtkinter as ctk

# ── Win32 constants & helpers ─────────────────────────────────────────────────

WDA_NONE               = 0x00000000
WDA_MONITOR            = 0x00000001
WDA_EXCLUDEFROMCAPTURE = 0x00000011

_user32 = ctypes.windll.user32


def _set_affinity(hwnd: int, value: int) -> bool:
    return bool(_user32.SetWindowDisplayAffinity(hwnd, value))


def _get_affinity(hwnd: int) -> int:
    aff = wintypes.DWORD(0)
    return aff.value if _user32.GetWindowDisplayAffinity(hwnd, ctypes.byref(aff)) else -1


def _affinity_label(aff: int) -> str:
    return {
        WDA_NONE:               "✅  NOT PROTECTED  (WDA_NONE)",
        WDA_MONITOR:            "🟡  MONITOR-ONLY  (WDA_MONITOR)",
        WDA_EXCLUDEFROMCAPTURE: "🔴  PROTECTED  (WDA_EXCLUDEFROMCAPTURE)",
    }.get(aff, f"⚪  UNKNOWN  (0x{aff:08X})")


# ── Main window ───────────────────────────────────────────────────────────────

ctk.set_appearance_mode("dark")
ctk.set_default_color_theme("blue")


class StressTestApp(ctk.CTk):
    # Colours used to repaint the window bg when state changes (OBS-visible)
    _BG_PROTECTED   = "#3d1010"   # deep red
    _BG_UNPROTECTED = "#0d2d0d"   # deep green

    def __init__(self) -> None:
        super().__init__()
        self.title("🔴 PROTECTED — Capture Bypass Stress Tester")
        self.geometry("620x520")
        self.minsize(540, 460)

        self._hwnd:          int  = 0
        self._fight_active:  bool = False
        self._fight_stop:    threading.Event = threading.Event()

        # Counters
        self._reapply_count: int = 0   # fight mode re-applications
        self._strip_count:   int = 0   # times external tool cleared us

        # Last known affinity (for edge-detection in the monitor thread)
        self._last_aff: int = -1

        self._build_ui()

        # Grab the HWND after the window is fully mapped
        self.after(200, self._on_ready)

    # ── UI ────────────────────────────────────────────────────────────────────

    def _build_ui(self) -> None:
        # Big status indicator
        self._status_lbl = ctk.CTkLabel(
            self,
            text="Initialising…",
            font=ctk.CTkFont(size=22, weight="bold"),
            text_color="white",
        )
        self._status_lbl.pack(pady=(28, 8))

        self._sub_lbl = ctk.CTkLabel(
            self,
            text="",
            font=ctk.CTkFont(size=13),
            text_color="#AAAAAA",
        )
        self._sub_lbl.pack()

        # Counter row
        cnt_frame = ctk.CTkFrame(self, fg_color="transparent")
        cnt_frame.pack(pady=(14, 4))

        self._reapply_lbl = ctk.CTkLabel(
            cnt_frame, text="Fight re-applies: 0",
            font=ctk.CTkFont(size=13), text_color="#FF8888",
        )
        self._reapply_lbl.pack(side="left", padx=20)

        self._strip_lbl = ctk.CTkLabel(
            cnt_frame, text="External strips: 0",
            font=ctk.CTkFont(size=13), text_color="#88FF88",
        )
        self._strip_lbl.pack(side="left", padx=20)

        ctk.CTkFrame(self, height=1, fg_color="#444444").pack(fill="x", padx=30, pady=12)

        # Manual protection buttons
        btn_row = ctk.CTkFrame(self, fg_color="transparent")
        btn_row.pack()

        ctk.CTkButton(
            btn_row, text="🔴  Apply Protection", width=200,
            fg_color="#8b0000", hover_color="#5a0000",
            command=self._apply_protection,
        ).pack(side="left", padx=8)

        ctk.CTkButton(
            btn_row, text="✅  Remove Protection", width=200,
            fg_color="#145a14", hover_color="#0d3d0d",
            command=self._remove_protection,
        ).pack(side="left", padx=8)

        ctk.CTkFrame(self, height=1, fg_color="#444444").pack(fill="x", padx=30, pady=18)

        # Fight mode controls
        ctk.CTkLabel(
            self, text="⚔️  Fight Mode",
            font=ctk.CTkFont(size=15, weight="bold"),
        ).pack()

        ctk.CTkLabel(
            self,
            text="Fight mode re-applies WDA_EXCLUDEFROMCAPTURE at the chosen interval,\n"
                 "simulating apps that resist one-shot injection.",
            font=ctk.CTkFont(size=12),
            text_color="#AAAAAA",
        ).pack(pady=(4, 10))

        # Interval slider
        slider_row = ctk.CTkFrame(self, fg_color="transparent")
        slider_row.pack()

        ctk.CTkLabel(slider_row, text="Re-apply every:", width=120, anchor="e").pack(side="left")

        self._interval_var = tk.IntVar(value=500)
        self._interval_slider = ctk.CTkSlider(
            slider_row,
            from_=50, to=2000, number_of_steps=39,
            variable=self._interval_var,
            width=220,
            command=self._on_slider,
        )
        self._interval_slider.pack(side="left", padx=10)

        self._interval_lbl = ctk.CTkLabel(slider_row, text="500 ms", width=70, anchor="w")
        self._interval_lbl.pack(side="left")

        # Fight toggle + counter reset
        fight_btn_row = ctk.CTkFrame(self, fg_color="transparent")
        fight_btn_row.pack(pady=12)

        self._fight_btn = ctk.CTkButton(
            fight_btn_row, text="▶  Start Fight Mode", width=200,
            fg_color="#7d3c98", hover_color="#5b2c6f",
            command=self._toggle_fight,
        )
        self._fight_btn.pack(side="left", padx=8)

        ctk.CTkButton(
            fight_btn_row, text="↺  Reset Counters", width=160,
            fg_color="#555555", hover_color="#333333",
            command=self._reset_counters,
        ).pack(side="left", padx=8)

    # ── Initialisation ────────────────────────────────────────────────────────

    def _on_ready(self) -> None:
        raw = self.wm_frame()
        # wm_frame() returns a hex string on Windows (e.g. "0x00031234"); convert to int
        try:
            self._hwnd = int(raw, 16) if isinstance(raw, str) else int(raw)
        except (ValueError, TypeError):
            self._hwnd = self.winfo_id()
        # Apply protection immediately so the window is blocked on launch
        self._apply_protection()
        # Start the monitor thread
        threading.Thread(target=self._monitor_loop, daemon=True).start()

    # ── Protection helpers ────────────────────────────────────────────────────

    def _apply_protection(self) -> None:
        if self._hwnd:
            _set_affinity(self._hwnd, WDA_EXCLUDEFROMCAPTURE)

    def _remove_protection(self) -> None:
        if self._hwnd:
            _set_affinity(self._hwnd, WDA_NONE)

    # ── Monitor loop (100 ms polling) ─────────────────────────────────────────

    def _monitor_loop(self) -> None:
        """Reads GetWindowDisplayAffinity every 100 ms and updates the UI."""
        while True:
            time.sleep(0.1)
            try:
                if not self._hwnd:
                    continue
                aff = _get_affinity(self._hwnd)
                if aff != self._last_aff:
                    prev = self._last_aff
                    self._last_aff = aff
                    # Count an external strip when we go protected → clear
                    if prev in (WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR) and aff == WDA_NONE:
                        self._strip_count += 1
                self.after(0, lambda a=aff: self._update_display(a))
            except Exception:
                pass

    def _update_display(self, aff: int) -> None:
        label_text = _affinity_label(aff)
        protected  = aff != WDA_NONE

        self._status_lbl.configure(text=label_text)

        fight_info = (
            f" | Fight: {'ON' if self._fight_active else 'OFF'}"
            f" @ {self._interval_var.get()} ms"
        )
        self._sub_lbl.configure(text=f"HWND: 0x{self._hwnd:08X}{fight_info}")

        # Update window title and background — highly visible in OBS
        if protected:
            self.title("🔴 PROTECTED — Capture Bypass Stress Tester")
            self.configure(fg_color=self._BG_PROTECTED)
            self._status_lbl.configure(text_color="#FF6060")
        else:
            self.title("✅ NOT PROTECTED — Capture Bypass Stress Tester")
            self.configure(fg_color=self._BG_UNPROTECTED)
            self._status_lbl.configure(text_color="#66DD66")

        # Update counters
        self._reapply_lbl.configure(text=f"Fight re-applies: {self._reapply_count}")
        self._strip_lbl.configure(text=f"External strips: {self._strip_count}")

    # ── Fight mode ────────────────────────────────────────────────────────────

    def _toggle_fight(self) -> None:
        if self._fight_active:
            self._stop_fight()
        else:
            self._start_fight()

    def _start_fight(self) -> None:
        self._fight_active = True
        self._fight_stop.clear()
        self._fight_btn.configure(
            text="⏹  Stop Fight Mode",
            fg_color="#b03a2e", hover_color="#7b241c",
        )
        threading.Thread(target=self._fight_loop, daemon=True).start()

    def _stop_fight(self) -> None:
        self._fight_active = False
        self._fight_stop.set()
        self._fight_btn.configure(
            text="▶  Start Fight Mode",
            fg_color="#7d3c98", hover_color="#5b2c6f",
        )

    def _fight_loop(self) -> None:
        """Re-applies WDA_EXCLUDEFROMCAPTURE at the chosen interval until stopped."""
        while not self._fight_stop.is_set():
            interval_ms = self._interval_var.get()
            if self._fight_stop.wait(timeout=interval_ms / 1000):
                break
            if not self._fight_active:
                break
            aff = _get_affinity(self._hwnd)
            if aff != WDA_EXCLUDEFROMCAPTURE:
                _set_affinity(self._hwnd, WDA_EXCLUDEFROMCAPTURE)
                self._reapply_count += 1

    def _on_slider(self, _value: float) -> None:
        ms = self._interval_var.get()
        self._interval_lbl.configure(text=f"{ms} ms")

    # ── Counter reset ─────────────────────────────────────────────────────────

    def _reset_counters(self) -> None:
        self._reapply_count = 0
        self._strip_count   = 0
        self._reapply_lbl.configure(text="Fight re-applies: 0")
        self._strip_lbl.configure(text="External strips: 0")


# ── Entry point ───────────────────────────────────────────────────────────────

if __name__ == "__main__":
    StressTestApp().mainloop()
