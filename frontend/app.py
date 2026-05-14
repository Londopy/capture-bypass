"""
Capture Bypass — Python frontend  (v3.0)

New in v3:
  • 32-bit process support — detects WOW64 processes via IsWow64Process and
    automatically routes injection through the x86 CLI + x86 DLL binaries.
    A "32" badge appears on each 32-bit row so the distinction is visible.

Requirements:
    pip install customtkinter pystray pillow

Build (x64 + x86):
    cargo build --release -p payload_dll -p payload_dll_persistent -p cli
    cargo build --release --target i686-pc-windows-msvc -p payload_dll -p payload_dll_persistent -p cli
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wintypes
import os
import shutil
import subprocess
import tempfile
import threading
import time
import uuid
from pathlib import Path

import customtkinter as ctk
from PIL import Image, ImageDraw
import pystray

# ── Paths ──────────────────────────────────────────────────────────────────────

_REPO      = Path(__file__).resolve().parent.parent
_X64       = _REPO / "target" / "release"
_X86       = _REPO / "target" / "i686-pc-windows-msvc" / "release"

CLI_X64             = _X64 / "cli.exe"
CLI_X86             = _X86 / "cli.exe"
DLL_ONESHOT_X64     = _X64 / "payload_dll.dll"
DLL_ONESHOT_X86     = _X86 / "payload_dll.dll"
DLL_PERSIST_X64     = _X64 / "payload_dll_persistent.dll"
DLL_PERSIST_X86     = _X86 / "payload_dll_persistent.dll"

# ── Constants ──────────────────────────────────────────────────────────────────

WDA_NONE               = 0x00000000
WDA_MONITOR            = 0x00000001
WDA_EXCLUDEFROMCAPTURE = 0x00000011

BROWSER_NAMES = {
    "chrome.exe", "msedge.exe", "firefox.exe",
    "brave.exe", "opera.exe", "vivaldi.exe", "thorium.exe",
}

# ── Windows API ────────────────────────────────────────────────────────────────

_user32   = ctypes.windll.user32
_kernel32 = ctypes.windll.kernel32

_WNDENUMPROC                = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
_PROCESS_QUERY_LIMITED_INFO = 0x1000
_TH32CS_SNAPPROCESS         = 0x00000002


class _PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize",              wintypes.DWORD),
        ("cntUsage",            wintypes.DWORD),
        ("th32ProcessID",       wintypes.DWORD),
        ("th32DefaultHeapID",   ctypes.c_size_t),
        ("th32ModuleID",        wintypes.DWORD),
        ("cntThreads",          wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase",      wintypes.LONG),
        ("dwFlags",             wintypes.DWORD),
        ("szExeFile",           ctypes.c_wchar * 260),
    ]


def _get_process_name(pid: int) -> str:
    h = _kernel32.OpenProcess(_PROCESS_QUERY_LIMITED_INFO, False, pid)
    if not h:
        return "<unknown>"
    try:
        buf, size = ctypes.create_unicode_buffer(1024), wintypes.DWORD(1024)
        _kernel32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size))
        return os.path.basename(buf.value) if buf.value else "<unknown>"
    finally:
        _kernel32.CloseHandle(h)


def is_process_32bit(pid: int) -> bool:
    """True if the process is 32-bit (running under WOW64 on this 64-bit OS)."""
    h = _kernel32.OpenProcess(_PROCESS_QUERY_LIMITED_INFO, False, pid)
    if not h:
        return False
    try:
        wow64 = wintypes.BOOL(False)
        _kernel32.IsWow64Process(h, ctypes.byref(wow64))
        return bool(wow64.value)
    finally:
        _kernel32.CloseHandle(h)


def _get_display_affinity(hwnd: int) -> int:
    aff = wintypes.DWORD(0)
    return aff.value if _user32.GetWindowDisplayAffinity(hwnd, ctypes.byref(aff)) else -1


def get_child_pids(parent_pid: int) -> list[int]:
    snap = _kernel32.CreateToolhelp32Snapshot(_TH32CS_SNAPPROCESS, 0)
    if snap == ctypes.c_void_p(-1).value:
        return []
    children: list[int] = []
    entry = _PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(_PROCESSENTRY32W)
    try:
        if _kernel32.Process32FirstW(snap, ctypes.byref(entry)):
            while True:
                if entry.th32ParentProcessID == parent_pid:
                    children.append(entry.th32ProcessID)
                if not _kernel32.Process32NextW(snap, ctypes.byref(entry)):
                    break
    finally:
        _kernel32.CloseHandle(snap)
    return children


def enumerate_windows() -> list[dict]:
    results: list[dict] = []

    def _cb(hwnd: int, _: int) -> bool:
        if not _user32.IsWindowVisible(hwnd):
            return True
        length = _user32.GetWindowTextLengthW(hwnd)
        if length == 0:
            return True
        buf = ctypes.create_unicode_buffer(length + 1)
        _user32.GetWindowTextW(hwnd, buf, length + 1)
        pid = wintypes.DWORD(0)
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        affinity    = _get_display_affinity(hwnd)
        is_prot     = affinity in (WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR)
        is_32       = is_process_32bit(pid.value)
        results.append({
            "hwnd":         hwnd,
            "pid":          pid.value,
            "process_name": _get_process_name(pid.value),
            "title":        buf.value,
            "affinity":     affinity,
            "is_protected": is_prot,
            "is_32bit":     is_32,
        })
        return True

    _user32.EnumWindows(_WNDENUMPROC(_cb), 0)
    return results


# ── Tray icon ──────────────────────────────────────────────────────────────────

def _make_tray_icon(active: bool = True) -> Image.Image:
    sz, img = 64, Image.new("RGBA", (64, 64), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    col = "#4488FF" if active else "#888888"
    d.arc([16, 8, 48, 36], start=180, end=0, fill=col, width=6)
    d.rounded_rectangle([10, 28, 54, 58], radius=5, fill=col)
    d.ellipse([27, 34, 37, 44], fill="white")
    d.rectangle([29, 40, 35, 50], fill="white")
    return img


# ── Wiki ───────────────────────────────────────────────────────────────────────

_WIKI: dict[str, str] = {
    "Overview": """\
What is Capture Bypass?
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Capture Bypass removes the WDA_EXCLUDEFROMCAPTURE screen-capture protection
from Windows application windows, letting OBS, the Snipping Tool, and any
other screen-capture software record them normally.

How does it work?
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Windows provides SetWindowDisplayAffinity(), which lets a process protect its
own windows from capture.  Because the API only works on a process's own
windows, bypassing it requires running code INSIDE the target process.

Capture Bypass does this via classic LoadLibrary DLL injection:

  1. OpenProcess          — open a handle to the target process
  2. VirtualAllocEx       — allocate memory inside the target
  3. WriteProcessMemory   — write the payload DLL path into that memory
  4. CreateRemoteThread   — start a thread inside the target that calls
                            LoadLibraryA, loading the payload DLL
  5. The DLL's DllMain    — spawns a worker thread which calls
                            SetWindowDisplayAffinity(hwnd, WDA_NONE) on every
                            window owned by that process

Legal notice
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Only use this tool on windows and processes you own or have explicit
permission to capture.  See DISCLAIMER.md in the repository.
""",

    "Requirements & Build": """\
Requirements
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  • Windows 10 build 19041+ (WDA_EXCLUDEFROMCAPTURE requires 2004+)
  • Administrator privileges  (OpenProcess on other processes requires admin)
  • Rust + Cargo  (https://rustup.rs)
  • Python 3.10+
  • pip packages:  customtkinter  pystray  pillow

Build — x64 (required)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo build --release -p payload_dll -p payload_dll_persistent -p cli

  Binaries land in:  target\\release\\

Build — x86 (optional, for 32-bit targets)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  rustup target add i686-pc-windows-msvc
  cargo build --release --target i686-pc-windows-msvc \\
        -p payload_dll -p payload_dll_persistent -p cli

  Binaries land in:  target\\i686-pc-windows-msvc\\release\\

  32-bit processes are shown with an orange "32" badge.  If the x86 binaries
  are missing the app will warn you but still work for 64-bit targets.

Python dependencies
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  pip install customtkinter pystray pillow
""",

    "Usage Guide": """\
Running the app
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Run as Administrator, then:
    python frontend/app.py

Window list
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  The list shows every visible, titled window with:

    PID        Process ID
    Process    Executable name  (orange "32" badge = 32-bit process)
    Title      Window title (truncated at 50 characters)
    Status     Live protection state, polled on each refresh:
                 🔴 Protected     — WDA_EXCLUDEFROMCAPTURE
                 🟡 Monitor-only  — WDA_MONITOR
                 ✅ Clear         — WDA_NONE (capturable)
                 ⚪ Unknown       — GetWindowDisplayAffinity failed
    Action     "Strip Protection" button for that row

Header buttons
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ⟳ Refresh             Re-enumerate all windows and update status badges.

  ⚡ Strip All Protected One click injects into every currently-protected PID.
                         Deduplicates so each process is only injected once.

  Mode toggle           Switch between injection modes (see Injection Modes).

  📖 Help               Opens this wiki.

Toolbar
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Filter bar            Live search by window title, process name, or PID.
                        Click ✕ to clear.

  Protected only        Hide unprotected windows — shows only what needs fixing.

  🤖 Auto-inject        Background thread polls every 3 seconds and
                         automatically strips any newly-protected window.
                         Designed for use while minimised to the system tray.

Status bar (bottom)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Left   — last action result / scanning state
  Right  — binary presence indicator, e.g.:
             x64 cli:✓ dll:✓  |  x86 cli:✓ dll:✗
           ✓ = found   ✗ = not built yet
""",

    "Injection Modes": """\
⚡ One-shot mode  (default)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  The payload DLL strips WDA protection once and exits.  Fast and lightweight.

  Use when:  the target app sets protection only once at startup and never
             re-applies it.

🔁 Persistent mode
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  The payload DLL stays alive inside the target process and re-applies
  WDA_NONE every 500 ms for the entire lifetime of the process.

  Use when:  the target app calls SetWindowDisplayAffinity on a timer to
             fight back against one-shot injection (e.g. DRM video players).
             This is what "fight mode" in the stress tester simulates.

  Toggle:    Click the Mode button in the header to switch between modes.
             The status bar shows which DLL is active.

Re-injection
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Windows caches loaded DLLs by file path — if the same DLL path is already
  loaded in a process, LoadLibraryA silently no-ops.  Capture Bypass works
  around this by copying the payload to a unique temp path before each
  injection, so every strip is guaranteed to fire DllMain.
""",

    "Browser Injection": """\
Why browsers need special handling
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Chrome, Edge, Firefox, Brave, Opera, Vivaldi, and Thorium use a multi-
  process architecture.  The main browser window lives in one process, but
  DRM-protected video is rendered in a separate child (renderer) process
  that owns its own windows.  Injecting only into the main PID won't strip
  the video window.

What Capture Bypass does
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  When you click "Strip Protection" on a browser row, the app automatically:

  1. Injects the payload into the main (browser) PID
  2. Enumerates all child processes via CreateToolhelp32Snapshot
  3. Injects into every child PID as well

  The result counter ("stripped N processes") reflects all injections.

Tip
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  If a browser re-applies protection after navigating to a new video, hit
  ⚡ Strip All Protected again, or enable 🤖 Auto-inject so it is handled
  automatically in the background.
""",

    "System Tray & Auto-inject": """\
System tray
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Clicking the window's ✕ close button hides the app to the system tray
  instead of quitting — the icon stays in your taskbar notification area.

  Tray icon right-click menu:
    Show Capture Bypass        Restore the main window
    Auto-Inject  (toggle)      Enable/disable background auto-inject
    Strip All Protected Now    Trigger a batch strip from the tray
    Quit                       Fully exit the application

  Double-click the tray icon to restore the window.

Auto-inject
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Enable the "🤖 Auto-inject (tray)" checkbox in the toolbar.

  A background thread polls GetWindowDisplayAffinity every 3 seconds.
  Any window that becomes protected and hasn't been seen before is
  automatically injected.  The tray icon tooltip updates to show what
  was stripped.

  Designed for streamers: start a stream, enable auto-inject, minimise
  to tray, and any app that tries to block capture is handled silently.
""",

    "Troubleshooting": """\
"No cli.exe found"
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  The Rust binaries haven't been built yet.  Run:
    cargo build --release -p payload_dll -p payload_dll_persistent -p cli

"Strip failed — try Administrator"
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  OpenProcess requires SeDebugPrivilege for processes not owned by your
  user session.  Right-click your terminal → Run as administrator, then
  launch the app again.

Injection succeeds but window is still black in OBS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  1. Hit ⟳ Refresh — if the status badge is now ✅ Clear, OBS may need
     to refresh its capture source.  Remove and re-add the Window Capture
     source in OBS.
  2. For browsers, the protected window may be in a child process.
     Click "Strip Protection" again — it will inject into children too.
  3. Some apps re-apply protection on a timer.  Switch to 🔁 Persistent
     mode and inject again.

Antivirus flags the DLL
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  DLL injection is a technique used by both legitimate tools and malware,
  so heuristic scanners may flag payload_dll.dll.  This is a false positive.
  You can inspect the source in payload_dll/src/lib.rs — it only calls
  SetWindowDisplayAffinity.  Add an exclusion for the target/ folder in
  your AV settings.

32-bit badge missing but target is 32-bit
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  IsWow64Process requires PROCESS_QUERY_LIMITED_INFO access.  If the app
  can't open the process handle (e.g. running without admin on a protected
  process), is_process_32bit() returns False as a safe default.  Running
  as Administrator resolves this.

x86 injection fails even with x86 binaries present
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  A 64-bit cli.exe cannot inject into a 32-bit process and vice-versa.
  Make sure you built the x86 target:
    rustup target add i686-pc-windows-msvc
    cargo build --release --target i686-pc-windows-msvc -p cli -p payload_dll
""",
}


class WikiWindow(ctk.CTkToplevel):
    """In-app documentation window."""

    _SIDEBAR_W = 200

    def __init__(self, parent: ctk.CTk) -> None:
        super().__init__(parent)
        self.title("Capture Bypass — Help & Documentation")
        self.geometry("900x620")
        self.minsize(700, 500)
        self.grab_set()          # modal-ish: keeps focus on this window

        self._sections = list(_WIKI.keys())
        self._build_ui()
        self._show_section(self._sections[0])

    def _build_ui(self) -> None:
        # ── Sidebar ──────────────────────────────────────────────────────
        sidebar = ctk.CTkFrame(self, width=self._SIDEBAR_W, corner_radius=0,
                               fg_color=("#DDDDDD", "#1E1E1E"))
        sidebar.pack(side="left", fill="y")
        sidebar.pack_propagate(False)

        ctk.CTkLabel(
            sidebar, text="📖  Contents",
            font=ctk.CTkFont(size=13, weight="bold"),
            anchor="w",
        ).pack(fill="x", padx=14, pady=(16, 8))

        self._nav_btns: dict[str, ctk.CTkButton] = {}
        for section in self._sections:
            btn = ctk.CTkButton(
                sidebar, text=section,
                anchor="w", height=34,
                fg_color="transparent",
                hover_color=("#CCCCCC", "#2E2E2E"),
                text_color=("#111111", "#DDDDDD"),
                corner_radius=6,
                command=lambda s=section: self._show_section(s),
            )
            btn.pack(fill="x", padx=8, pady=2)
            self._nav_btns[section] = btn

        # ── Content area ──────────────────────────────────────────────────
        content_frame = ctk.CTkFrame(self, corner_radius=0, fg_color="transparent")
        content_frame.pack(side="left", fill="both", expand=True)

        self._section_title = ctk.CTkLabel(
            content_frame, text="",
            font=ctk.CTkFont(size=15, weight="bold"),
            anchor="w",
        )
        self._section_title.pack(fill="x", padx=20, pady=(16, 6))

        self._textbox = ctk.CTkTextbox(
            content_frame,
            font=ctk.CTkFont(family="Segoe UI", size=13),
            wrap="word",
            state="disabled",
            corner_radius=8,
            border_width=1,
        )
        self._textbox.pack(fill="both", expand=True, padx=16, pady=(0, 16))

    def _show_section(self, section: str) -> None:
        # Highlight active nav button
        for name, btn in self._nav_btns.items():
            if name == section:
                btn.configure(fg_color=("#4488FF", "#1a5276"),
                              text_color="white")
            else:
                btn.configure(fg_color="transparent",
                              text_color=("#111111", "#DDDDDD"))

        self._section_title.configure(text=section)
        self._textbox.configure(state="normal")
        self._textbox.delete("1.0", "end")
        self._textbox.insert("1.0", _WIKI[section])
        self._textbox.configure(state="disabled")


# ── App ────────────────────────────────────────────────────────────────────────

ctk.set_appearance_mode("dark")
ctk.set_default_color_theme("blue")


class App(ctk.CTk):
    def __init__(self) -> None:
        super().__init__()
        self.title("Capture Bypass")
        self.geometry("1080x660")
        self.minsize(720, 420)

        self._all_windows:      list[dict]         = []
        self._persistent_mode:  bool               = False
        self._auto_inject:      bool               = False
        self._auto_inject_seen: set[int]           = set()
        self._tray:             pystray.Icon | None = None

        self.protocol("WM_DELETE_WINDOW", self._on_close)
        self._build_ui()
        self.after(100, self._refresh)

    # ── UI ─────────────────────────────────────────────────────────────────────

    def _build_ui(self) -> None:
        # Header
        hdr = ctk.CTkFrame(self, height=54, corner_radius=0)
        hdr.pack(fill="x")
        hdr.pack_propagate(False)

        ctk.CTkLabel(
            hdr, text="🔓  Capture Bypass",
            font=ctk.CTkFont(size=18, weight="bold"),
        ).pack(side="left", padx=16, pady=12)

        btn_bar = ctk.CTkFrame(hdr, fg_color="transparent")
        btn_bar.pack(side="right", padx=12, pady=8)

        ctk.CTkButton(btn_bar, text="⟳  Refresh", width=110,
                      command=self._refresh).pack(side="left", padx=4)

        ctk.CTkButton(btn_bar, text="📖 Help", width=80,
                      fg_color="#555555", hover_color="#333333",
                      command=self._open_wiki).pack(side="left", padx=4)

        ctk.CTkButton(
            btn_bar, text="⚡ Strip All Protected", width=165,
            fg_color="#c0392b", hover_color="#922b21",
            command=self._strip_all_protected,
        ).pack(side="left", padx=4)

        mode_f = ctk.CTkFrame(btn_bar, fg_color="transparent")
        mode_f.pack(side="left", padx=(12, 0))
        ctk.CTkLabel(mode_f, text="Mode:", font=ctk.CTkFont(size=12)).pack(side="left")
        self._mode_btn = ctk.CTkButton(
            mode_f, text="⚡ One-shot", width=130,
            fg_color="#1a5276", hover_color="#154360",
            command=self._toggle_mode,
        )
        self._mode_btn.pack(side="left", padx=6)

        # Toolbar
        tb = ctk.CTkFrame(self, fg_color="transparent")
        tb.pack(fill="x", padx=12, pady=(6, 0))

        ctk.CTkLabel(tb, text="Filter:").pack(side="left", padx=(0, 6))
        self._filter_var = ctk.StringVar()
        self._filter_var.trace_add("write", lambda *_: self._apply_filter())
        ctk.CTkEntry(tb, textvariable=self._filter_var, width=250).pack(side="left")
        ctk.CTkButton(tb, text="✕", width=30,
                      command=lambda: self._filter_var.set("")).pack(side="left", padx=(4, 16))

        self._prot_only_var = ctk.BooleanVar(value=False)
        ctk.CTkCheckBox(tb, text="Protected only",
                        variable=self._prot_only_var,
                        command=self._apply_filter).pack(side="left", padx=(0, 16))

        self._auto_var = ctk.BooleanVar(value=False)
        ctk.CTkCheckBox(tb, text="🤖 Auto-inject (tray)",
                        variable=self._auto_var,
                        command=self._on_auto_toggle).pack(side="left")

        # Column headers
        col_hdr = ctk.CTkFrame(self, corner_radius=0, fg_color=("#CCCCCC", "#2A2A2A"))
        col_hdr.pack(fill="x", padx=12, pady=(6, 0))
        for text, width in [("PID", 70), ("Process", 195), ("Window Title", 330), ("Status", 115)]:
            ctk.CTkLabel(col_hdr, text=text, width=width, anchor="w",
                         font=ctk.CTkFont(weight="bold")).pack(side="left", padx=8, pady=4)
        ctk.CTkLabel(col_hdr, text="Action", anchor="w",
                     font=ctk.CTkFont(weight="bold")).pack(side="left", pady=4)

        # Scrollable list
        self._scroll = ctk.CTkScrollableFrame(self)
        self._scroll.pack(fill="both", expand=True, padx=12, pady=6)

        # Status bar
        sbar = ctk.CTkFrame(self, height=34, corner_radius=0)
        sbar.pack(fill="x", side="bottom")
        sbar.pack_propagate(False)
        self._status_lbl = ctk.CTkLabel(sbar, text="Starting…", anchor="w")
        self._status_lbl.pack(side="left", padx=12)
        self._bin_lbl = ctk.CTkLabel(sbar, text="", anchor="e", font=ctk.CTkFont(size=11))
        self._bin_lbl.pack(side="right", padx=12)

    # ── Data ───────────────────────────────────────────────────────────────────

    def _refresh(self) -> None:
        self._set_status("Scanning…", "gray")
        threading.Thread(target=self._do_refresh, daemon=True).start()

    def _do_refresh(self) -> None:
        windows = enumerate_windows()
        self.after(0, lambda: self._on_refresh_done(windows))

    def _on_refresh_done(self, windows: list[dict]) -> None:
        self._all_windows = windows
        self._apply_filter()
        self._update_bin_lbl()
        n_prot = sum(1 for w in windows if w["is_protected"])
        n_32   = sum(1 for w in windows if w["is_32bit"])
        msg = f"Found {len(windows)} windows"
        msg += f" — {n_prot} protected" if n_prot else ""
        msg += f", {n_32} are 32-bit." if n_32 else "."
        self._set_status(msg, "#FF9944" if n_prot else "#88FF88")

    def _apply_filter(self) -> None:
        q    = self._filter_var.get().lower()
        prot = self._prot_only_var.get()
        rows = [
            w for w in self._all_windows
            if (not prot or w["is_protected"])
            and (not q or q in w["title"].lower()
                 or q in w["process_name"].lower()
                 or q in str(w["pid"]))
        ]
        self._render_rows(rows)

    # ── Rows ───────────────────────────────────────────────────────────────────

    def _render_rows(self, windows: list[dict]) -> None:
        for child in self._scroll.winfo_children():
            child.destroy()

        if not windows:
            ctk.CTkLabel(self._scroll, text="No matching windows.",
                         text_color="gray", font=ctk.CTkFont(slant="italic")).pack(pady=20)
            return

        for w in windows:
            row_col = ("#3d1a1a", "#2d1010") if w["is_protected"] else ("gray86", "#2b2b2b")
            row = ctk.CTkFrame(self._scroll, corner_radius=4, fg_color=row_col)
            row.pack(fill="x", pady=1)

            # PID
            ctk.CTkLabel(row, text=str(w["pid"]), width=70, anchor="w",
                         font=ctk.CTkFont(family="Courier New")).pack(side="left", padx=8, pady=3)

            # Process name + bitness badge
            proc_f = ctk.CTkFrame(row, width=195, fg_color="transparent")
            proc_f.pack(side="left", pady=3)
            proc_f.pack_propagate(False)
            ctk.CTkLabel(proc_f, text=w["process_name"], anchor="w",
                         text_color="#82B4FF").pack(side="left")
            if w["is_32bit"]:
                ctk.CTkLabel(
                    proc_f, text=" 32", anchor="w",
                    text_color="#FFAA44",
                    font=ctk.CTkFont(size=10, weight="bold"),
                ).pack(side="left")

            # Title
            title = w["title"][:50] + "…" if len(w["title"]) > 50 else w["title"]
            ctk.CTkLabel(row, text=title, width=330, anchor="w").pack(side="left", pady=3)

            # Status badge
            if w["affinity"] == WDA_EXCLUDEFROMCAPTURE:
                stxt, scol = "🔴 Protected",    "#FF6060"
            elif w["affinity"] == WDA_MONITOR:
                stxt, scol = "🟡 Monitor-only", "#FFAA44"
            elif w["affinity"] == WDA_NONE:
                stxt, scol = "✅ Clear",         "#66DD66"
            else:
                stxt, scol = "⚪ Unknown",       "gray"
            ctk.CTkLabel(row, text=stxt, width=115, anchor="w",
                         text_color=scol, font=ctk.CTkFont(size=12)).pack(side="left", pady=3)

            ctk.CTkButton(row, text="Strip Protection", width=145,
                          command=lambda win=w: self._strip(win)).pack(side="left", padx=8, pady=3)

    # ── Injection ──────────────────────────────────────────────────────────────

    def _resolve_binaries(self, is_32bit: bool) -> tuple[Path, Path]:
        """Return (cli_path, dll_path) for the given target architecture."""
        if is_32bit:
            cli = CLI_X86
            dll = DLL_PERSIST_X86 if self._persistent_mode else DLL_ONESHOT_X86
        else:
            cli = CLI_X64
            dll = DLL_PERSIST_X64 if self._persistent_mode else DLL_ONESHOT_X64
        return cli, dll

    def _inject_pid(self, pid: int, name: str, is_32bit: bool = False) -> bool:
        cli, dll = self._resolve_binaries(is_32bit)
        arch_tag = "x86" if is_32bit else "x64"

        if not cli.exists():
            self._set_status(
                f"✗  {cli.name} ({arch_tag}) not found — "
                f"cargo build --release --target i686-pc-windows-msvc -p cli",
                "#FF7070",
            )
            return False
        if not dll.exists():
            self._set_status(f"✗  {dll.name} ({arch_tag}) not found — build it first.", "#FF7070")
            return False

        # Copy the DLL to a unique temp path so that Windows treats each
        # injection as a new module.  LoadLibraryA caches by path; reusing the
        # same path on a process that already has the DLL loaded will silently
        # no-op (DllMain won't fire again).  A fresh path forces a new load.
        tmp_dll = Path(tempfile.gettempdir()) / f"cb_{uuid.uuid4().hex[:12]}{dll.suffix}"
        try:
            shutil.copy2(dll, tmp_dll)
        except OSError:
            tmp_dll = dll   # fallback — first injection still works

        try:
            r = subprocess.run([str(cli), str(pid), str(tmp_dll)],
                               capture_output=True, text=True, timeout=8)
            return r.returncode == 0
        except Exception:
            return False
        # Note: tmp_dll is intentionally not deleted here.  On Windows a DLL
        # mapped into a remote process cannot be unlinked while it is loaded,
        # and the persistent variant runs indefinitely.  %TEMP% is cleaned by
        # Windows on reboot / Disk Cleanup.

    def _strip(self, win: dict) -> None:
        if not CLI_X64.exists() and not CLI_X86.exists():
            self._set_status("✗  No cli.exe found — run cargo build --release first.", "#FF7070")
            return

        # Build list of (pid, name, is_32bit)
        targets: list[tuple[int, str, bool]] = [
            (win["pid"], win["process_name"], win.get("is_32bit", False))
        ]
        if win["process_name"].lower() in BROWSER_NAMES:
            for child_pid in get_child_pids(win["pid"]):
                targets.append((child_pid, win["process_name"] + " (child)",
                                 is_process_32bit(child_pid)))

        self._set_status(
            f"Injecting into {win['process_name']} "
            f"({len(targets)} process{'es' if len(targets) > 1 else ''})…", "gray"
        )

        def _run() -> None:
            ok = fail = 0
            for pid, name, bit32 in targets:
                if self._inject_pid(pid, name, bit32):
                    ok += 1
                else:
                    fail += 1

            if fail == 0:
                msg, col = (f"✓  {win['process_name']} — stripped ({ok} process{'es' if ok>1 else ''}).",
                            "#88FF88")
            else:
                msg, col = (f"⚠  {ok} ok, {fail} failed — try Administrator.", "#FFAA44")
            self.after(0, lambda: self._set_status(msg, col))
            self.after(500, self._refresh)

        threading.Thread(target=_run, daemon=True).start()

    def _strip_all_protected(self) -> None:
        protected = [w for w in self._all_windows if w["is_protected"]]
        if not protected:
            self._set_status("No protected windows found — hit Refresh first.", "#FFAA44")
            return

        seen: set[int] = set()
        unique: list[dict] = []
        for w in protected:
            if w["pid"] not in seen:
                seen.add(w["pid"])
                unique.append(w)

        self._set_status(f"Stripping {len(unique)} protected process(es)…", "gray")

        def _run() -> None:
            ok = fail = 0
            for w in unique:
                pids = [(w["pid"], w["process_name"], w.get("is_32bit", False))]
                if w["process_name"].lower() in BROWSER_NAMES:
                    pids += [(c, w["process_name"], is_process_32bit(c))
                             for c in get_child_pids(w["pid"])]
                for pid, name, bit32 in pids:
                    if self._inject_pid(pid, name, bit32):
                        ok += 1
                    else:
                        fail += 1
            msg = f"✓  Done — {ok} injected" + (f", {fail} failed." if fail else ".")
            col = "#88FF88" if fail == 0 else "#FFAA44"
            self.after(0, lambda: self._set_status(msg, col))
            self.after(500, self._refresh)

        threading.Thread(target=_run, daemon=True).start()

    # ── Mode ───────────────────────────────────────────────────────────────────

    def _toggle_mode(self) -> None:
        self._persistent_mode = not self._persistent_mode
        if self._persistent_mode:
            self._mode_btn.configure(text="🔁 Persistent",
                                     fg_color="#1e8449", hover_color="#196f3d")
        else:
            self._mode_btn.configure(text="⚡ One-shot",
                                     fg_color="#1a5276", hover_color="#154360")
        self._update_bin_lbl()

    # ── Auto-inject ────────────────────────────────────────────────────────────

    def _on_auto_toggle(self) -> None:
        self._auto_inject = self._auto_var.get()
        if self._auto_inject:
            self._auto_inject_seen.clear()
            threading.Thread(target=self._auto_loop, daemon=True).start()
            self._set_status("🤖 Auto-inject on — minimize to tray.", "#88AAFF")
        else:
            self._set_status("Auto-inject off.", "gray")

    def _auto_loop(self) -> None:
        while self._auto_inject:
            time.sleep(3)
            try:
                for w in enumerate_windows():
                    if w["is_protected"] and w["pid"] not in self._auto_inject_seen:
                        self._auto_inject_seen.add(w["pid"])
                        ok = self._inject_pid(w["pid"], w["process_name"], w["is_32bit"])
                        if w["process_name"].lower() in BROWSER_NAMES:
                            for c in get_child_pids(w["pid"]):
                                self._inject_pid(c, w["process_name"], is_process_32bit(c))
                        msg = (f"🤖 Auto-stripped {w['process_name']} (PID {w['pid']})"
                               if ok else
                               f"🤖 Auto-inject failed for {w['process_name']}")
                        self.after(0, lambda m=msg, ok=ok:
                                   self._set_status(m, "#88FF88" if ok else "#FFAA44"))
                        if self._tray:
                            self._tray.title = f"Capture Bypass — stripped {w['process_name']}"
            except Exception:
                pass

    # ── Tray ───────────────────────────────────────────────────────────────────

    def _setup_tray(self) -> None:
        menu = pystray.Menu(
            pystray.MenuItem("Show Capture Bypass", self._show_from_tray, default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Auto-Inject",
                             self._tray_toggle_auto,
                             checked=lambda _: self._auto_inject),
            pystray.MenuItem("Strip All Protected Now", self._tray_strip_all),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Quit", self._tray_quit),
        )
        self._tray = pystray.Icon(
            "capture_bypass", _make_tray_icon(self._auto_inject),
            "Capture Bypass", menu,
        )
        threading.Thread(target=self._tray.run, daemon=True).start()

    def _show_from_tray(self, *_) -> None:
        self.after(0, self.deiconify)
        self.after(0, self.lift)

    def _tray_toggle_auto(self, *_) -> None:
        self._auto_var.set(not self._auto_inject)
        self.after(0, self._on_auto_toggle)

    def _tray_strip_all(self, *_) -> None:
        self.after(0, self._strip_all_protected)

    def _tray_quit(self, *_) -> None:
        self._auto_inject = False
        if self._tray:
            self._tray.stop()
        self.after(0, self.destroy)

    def _on_close(self) -> None:
        self.withdraw()
        if self._tray is None:
            self._setup_tray()

    # ── Wiki ───────────────────────────────────────────────────────────────────

    def _open_wiki(self) -> None:
        if hasattr(self, "_wiki_win") and self._wiki_win.winfo_exists():
            self._wiki_win.focus()
            return
        self._wiki_win = WikiWindow(self)

    # ── Helpers ────────────────────────────────────────────────────────────────

    def _set_status(self, text: str, color: str = "white") -> None:
        self._status_lbl.configure(text=text, text_color=color)

    def _update_bin_lbl(self) -> None:
        dll64 = DLL_PERSIST_X64 if self._persistent_mode else DLL_ONESHOT_X64
        dll86 = DLL_PERSIST_X86 if self._persistent_mode else DLL_ONESHOT_X86

        def tick(p: Path) -> str:
            return "✓" if p.exists() else "✗"

        parts = [
            f"x64 cli:{tick(CLI_X64)} dll:{tick(dll64)}",
            f"x86 cli:{tick(CLI_X86)} dll:{tick(dll86)}",
        ]
        all_x64_ok = CLI_X64.exists() and dll64.exists()
        self._bin_lbl.configure(
            text="  |  ".join(parts),
            text_color="#88FF88" if all_x64_ok else "#FF7070",
        )


# ── Entry point ────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    App().mainloop()
