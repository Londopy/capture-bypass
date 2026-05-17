//! capture-bypass GUI — Rust/egui frontend
//!
//! Feature-parity with frontend/app.py (Python/customtkinter).
//! Requires Administrator privileges (enforced by embedded UAC manifest via build.rs).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use winreg::{enums::*, RegKey};

// Windows API
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, TRUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            IsWow64Process, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowDisplayAffinity, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    },
};

// ── Constants ─────────────────────────────────────────────────────────────────

const WDA_NONE: u32 = 0x00000000;
const WDA_MONITOR: u32 = 0x00000001;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x00000011;

const BROWSER_NAMES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "thorium.exe",
];

// ── Help content (mirrors Python _WIKI) ──────────────────────────────────────

const HELP_SECTIONS: &[(&str, &str)] = &[
    ("Overview", "\
What is Capture Bypass?
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Capture Bypass removes the WDA_EXCLUDEFROMCAPTURE screen-capture protection \
from Windows application windows, letting OBS, the Snipping Tool, and any \
other screen-capture software record them normally.

How does it work?
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Windows provides SetWindowDisplayAffinity(), which lets a process protect its \
own windows from capture.  Because the API only works on a process's own \
windows, bypassing it requires running code INSIDE the target process.

Capture Bypass does this via classic LoadLibrary DLL injection:

  1. OpenProcess          — open a handle to the target process
  2. VirtualAllocEx       — allocate memory inside the target
  3. WriteProcessMemory   — write the payload DLL path into that memory
  4. CreateRemoteThread   — start a thread inside the target that calls
                            LoadLibraryA, loading the payload DLL
  5. The DLL's DllMain    — calls SetWindowDisplayAffinity(hwnd, WDA_NONE)
                            for every window owned by that process

Legal notice
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Only use this tool on windows and processes you own or have explicit \
permission to capture.  See DISCLAIMER.md in the repository.
"),
    ("Requirements & Build", "\
Requirements
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  • Windows 10 build 19041+ (WDA_EXCLUDEFROMCAPTURE requires 2004+)
  • Administrator privileges  (OpenProcess on other processes requires admin)
  • Rust + Cargo  (https://rustup.rs)

Build — x64 (required)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo build --release -p payload_dll -p payload_dll_persistent -p gui

  Binaries land in:  target\\release\\

Build — x86 (optional, for 32-bit targets)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  rustup target add i686-pc-windows-msvc
  cargo build --release --target i686-pc-windows-msvc \\
        -p payload_dll -p payload_dll_persistent

  Binaries land in:  target\\i686-pc-windows-msvc\\release\\

  32-bit processes are shown with an orange \"32\" badge.
"),
    ("Usage Guide", "\
Window list
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
The list shows every visible, titled window with:

  PID        Process ID
  Process    Executable name  (orange \"32\" badge = 32-bit process)
  Title      Window title
  Status     Live protection state, refreshed every 500 ms:
               PROTECTED  — WDA_EXCLUDEFROMCAPTURE
               MONITOR    — WDA_MONITOR
               OK         — WDA_NONE (capturable)
  Action     \"Strip Protection\" button for that row

Header buttons
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ⟳ Refresh              Re-enumerate all windows and update status badges.

  ⚡ Strip All Protected  One click injects into every currently-protected PID.
                          Deduplicates so each process is only injected once.

  Mode toggle            Switch between injection modes (see Injection Modes).

  📖 Help                Opens this panel.

Filter bar
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Live search by window title, process name, or PID.  Click ✕ to clear.
  \"Protected only\" checkbox hides unprotected windows.

  🤖 Auto-inject         Background thread strips newly-protected windows
                          automatically.  Runs while app is minimised to tray.

Status bar
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Shows last action result and timestamp.
  Green = success   Red = error   Gray = neutral
"),
    ("Injection Modes", "\
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

Re-injection
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Windows caches loaded DLLs by file path — if the same DLL path is already
loaded in a process, LoadLibraryA silently no-ops.  Inject again if the
status badge shows PROTECTED after a previous strip.
"),
    ("Browser Injection", "\
Why browsers need special handling
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Chrome, Edge, Firefox, Brave, Opera, Vivaldi, and Thorium use a multi-process
architecture.  DRM-protected video is rendered in a separate child (renderer)
process that owns its own windows.  Injecting only into the main PID won't
strip the video window.

What Capture Bypass does
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
When you click \"Strip Protection\" on a browser row, the app automatically:

  1. Injects the payload into the main (browser) PID
  2. Enumerates all child processes via CreateToolhelp32Snapshot
  3. Injects into every child PID as well

Tip
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
If a browser re-applies protection after navigating to a new video, hit
⚡ Strip All Protected again, or enable 🤖 Auto-inject.
"),
    ("System Tray & Auto-inject", "\
System tray
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Clicking the window's ✕ close button hides the app to the system tray
instead of quitting — the icon remains in the notification area.

Tray icon right-click menu:
  Open    — restore the main window
  Quit    — fully exit the application

Auto-inject
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Enable the \"🤖 Auto-inject\" toggle in the toolbar.

A background thread polls GetWindowDisplayAffinity every 500 ms.
Any window that becomes protected and hasn't been seen before is
automatically injected.

Designed for streamers: enable auto-inject, minimise to tray, and any
app that tries to block capture is handled silently.
"),
    ("Troubleshooting", "\
DLLs not found
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
The Rust payload DLLs haven't been built yet.  Run:
  cargo build --release -p payload_dll -p payload_dll_persistent

\"Strip failed — try Administrator\"
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
OpenProcess requires SeDebugPrivilege for processes not owned by your
user session.  Right-click your terminal → Run as administrator.

Injection succeeds but window is still black in OBS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  1. Wait for the next 500 ms refresh — if status is now OK, OBS may
     need to refresh its capture source (remove and re-add it).
  2. For browsers, click Strip Protection again — it injects children too.
  3. Some apps re-apply protection on a timer.  Switch to 🔁 Persistent
     mode and inject again.

Antivirus flags the DLL
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
DLL injection is used by both legitimate tools and malware, so heuristic
scanners may flag payload_dll.dll.  Inspect payload_dll/src/lib.rs —
it only calls SetWindowDisplayAffinity.  Add an exclusion for target/ in
your AV settings.

x86 injection fails even with x86 binaries present
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
A 64-bit process cannot inject into a 32-bit process and vice-versa.
Make sure you built the x86 target:
  rustup target add i686-pc-windows-msvc
  cargo build --release --target i686-pc-windows-msvc -p payload_dll
"),
];

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct WindowEntry {
    pid: u32,
    process_name: String,
    title: String,
    affinity: u32,
    is_protected: bool,
    is_32bit: bool,
}

// ── Injection result message ──────────────────────────────────────────────────

struct InjResult {
    msg: String,
    ok: bool,
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    // Shared window list (background refresh thread writes, UI reads)
    shared_windows: Arc<Mutex<Vec<WindowEntry>>>,

    // DLL base dir (next to the exe)
    exe_dir: PathBuf,

    // Injection mode
    persistent_mode: bool,

    // Auto-inject
    auto_inject_enabled: bool,
    auto_inject_running: Arc<AtomicBool>,
    auto_inject_seen: Arc<Mutex<HashSet<u32>>>,

    // UI state
    filter: String,
    protected_only: bool,
    show_help: bool,
    help_section: usize,

    // Status bar
    status_msg: String,
    status_color: Color32,
    status_time: Option<Instant>,

    // Launch at Windows startup
    startup_enabled: bool,

    // Tray (must stay alive for the duration of the app)
    _tray_icon: Option<tray_icon::TrayIcon>,
    tray_open_id: Option<tray_icon::menu::MenuId>,
    tray_quit_id: Option<tray_icon::menu::MenuId>,
    quit_requested: bool,

    // Channel: background injection threads → UI
    inject_tx: Sender<InjResult>,
    inject_rx: Receiver<InjResult>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));

        let shared_windows: Arc<Mutex<Vec<WindowEntry>>> = Arc::new(Mutex::new(Vec::new()));

        // Kick off background refresh thread (500 ms interval)
        {
            let shared = Arc::clone(&shared_windows);
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                let windows = enumerate_windows();
                *shared.lock().unwrap() = windows;
                ctx.request_repaint();
                std::thread::sleep(Duration::from_millis(500));
            });
        }

        let (inject_tx, inject_rx) = mpsc::channel();

        // Set up system tray
        let (tray_icon, tray_open_id, tray_quit_id) = build_tray();

        App {
            shared_windows,
            exe_dir,
            persistent_mode: false,
            auto_inject_enabled: false,
            auto_inject_running: Arc::new(AtomicBool::new(false)),
            auto_inject_seen: Arc::new(Mutex::new(HashSet::new())),
            filter: String::new(),
            protected_only: false,
            show_help: false,
            help_section: 0,
            status_msg: String::from("Ready."),
            status_color: Color32::GRAY,
            status_time: None,
            startup_enabled: read_startup_reg(),
            _tray_icon: tray_icon,
            tray_open_id,
            tray_quit_id,
            quit_requested: false,
            inject_tx,
            inject_rx,
        }
    }

    // ── DLL path resolution ────────────────────────────────────────────────────

    fn dll_path(&self, is_32bit: bool) -> PathBuf {
        let name = if self.persistent_mode {
            "payload_dll_persistent.dll"
        } else {
            "payload_dll.dll"
        };
        if is_32bit {
            // Prefer {exe_dir}/x86/ — used by the installer and the GUI zip bundle.
            let installed = self.exe_dir.join("x86").join(name);
            if installed.exists() {
                return installed;
            }
            // Fall back to the Cargo build-output layout for local development.
            self.exe_dir
                .join("..")
                .join("i686-pc-windows-msvc")
                .join("release")
                .join(name)
        } else {
            self.exe_dir.join(name)
        }
    }

    // ── Injection ──────────────────────────────────────────────────────────────

    fn inject_pid_async(&self, pid: u32, process_name: String, is_32bit: bool) {
        let dll_path = self.dll_path(is_32bit);
        let tx = self.inject_tx.clone();
        std::thread::spawn(move || {
            if !dll_path.exists() {
                let arch = if is_32bit { "x86" } else { "x64" };
                let _ = tx.send(InjResult {
                    msg: format!(
                        "✗  DLL not found ({arch}): {}  — build with cargo build --release",
                        dll_path.display()
                    ),
                    ok: false,
                });
                return;
            }
            match injector_core::inject_dll(pid, &dll_path) {
                Ok(()) => {
                    let _ = tx.send(InjResult {
                        msg: format!(
                            "✓  Stripped PID {pid} ({process_name})"
                        ),
                        ok: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(InjResult {
                        msg: format!(
                            "✗  PID {pid} ({process_name}): {}",
                            e.message()
                        ),
                        ok: false,
                    });
                }
            }
        });
    }

    fn strip_window(&self, entry: &WindowEntry) {
        let is_browser = BROWSER_NAMES
            .iter()
            .any(|b| entry.process_name.eq_ignore_ascii_case(b));

        let mut targets: Vec<(u32, String, bool)> =
            vec![(entry.pid, entry.process_name.clone(), entry.is_32bit)];

        if is_browser {
            for child_pid in get_child_pids(entry.pid) {
                let is_32 = is_process_32bit(child_pid);
                targets.push((child_pid, format!("{} (child)", entry.process_name), is_32));
            }
        }

        for (pid, name, is_32) in targets {
            self.inject_pid_async(pid, name, is_32);
        }
    }

    fn strip_all_protected(&self, windows: &[WindowEntry]) {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut count = 0usize;

        for w in windows.iter().filter(|w| w.is_protected) {
            if seen.insert(w.pid) {
                self.strip_window(w);
                count += 1;
            }
        }

        if count == 0 {
            // will show in status from the inject results; handled by caller
        }
        let _ = self.inject_tx.send(InjResult {
            msg: format!("⚡ Stripping {count} protected process(es)…"),
            ok: true,
        });
    }

    fn set_status(&mut self, msg: impl Into<String>, ok: bool) {
        self.status_msg = msg.into();
        self.status_color = if ok {
            Color32::from_rgb(100, 220, 100)
        } else {
            Color32::from_rgb(220, 90, 90)
        };
        self.status_time = Some(Instant::now());
    }

    fn set_status_neutral(&mut self, msg: impl Into<String>) {
        self.status_msg = msg.into();
        self.status_color = Color32::GRAY;
        self.status_time = Some(Instant::now());
    }

    // ── Auto-inject ────────────────────────────────────────────────────────────

    fn start_auto_inject(&mut self) {
        self.auto_inject_running.store(true, Ordering::Relaxed);
        self.auto_inject_seen.lock().unwrap().clear();
        let running = Arc::clone(&self.auto_inject_running);
        let seen = Arc::clone(&self.auto_inject_seen);
        let exe_dir = self.exe_dir.clone();
        let persistent = self.persistent_mode;
        let tx = self.inject_tx.clone();

        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let windows = enumerate_windows();
                for w in windows.iter().filter(|w| w.is_protected) {
                    let already_seen = {
                        let mut guard = seen.lock().unwrap();
                        !guard.insert(w.pid)
                    };
                    if already_seen {
                        continue;
                    }
                    // Build DLL path inline (mirrors dll_path())
                    let dll_name = if persistent {
                        "payload_dll_persistent.dll"
                    } else {
                        "payload_dll.dll"
                    };
                    let dll = if w.is_32bit {
                        exe_dir.join("..").join("i686-pc-windows-msvc").join("release").join(dll_name)
                    } else {
                        exe_dir.join(dll_name)
                    };

                    let mut pids: Vec<(u32, String, bool)> =
                        vec![(w.pid, w.process_name.clone(), w.is_32bit)];
                    if BROWSER_NAMES.iter().any(|b| w.process_name.eq_ignore_ascii_case(b)) {
                        for child in get_child_pids(w.pid) {
                            pids.push((child, w.process_name.clone(), is_process_32bit(child)));
                        }
                    }

                    for (pid, name, is32) in pids {
                        let dll_path = if is32 {
                            exe_dir.join("..").join("i686-pc-windows-msvc").join("release").join(dll_name)
                        } else {
                            dll.clone()
                        };
                        if !dll_path.exists() {
                            continue;
                        }
                        match injector_core::inject_dll(pid, &dll_path) {
                            Ok(()) => {
                                let _ = tx.send(InjResult {
                                    msg: format!("🤖 Auto-stripped {name} (PID {pid})"),
                                    ok: true,
                                });
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        });
    }

    fn stop_auto_inject(&mut self) {
        self.auto_inject_running.store(false, Ordering::Relaxed);
    }
}

// ── egui rendering ─────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Poll injection results ───────────────────────────────────────────
        while let Ok(result) = self.inject_rx.try_recv() {
            self.set_status(result.msg, result.ok);
        }

        // ── Poll tray menu events ────────────────────────────────────────────
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if Some(&event.id) == self.tray_open_id.as_ref() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if Some(&event.id) == self.tray_quit_id.as_ref() {
                self.quit_requested = true;
                self.stop_auto_inject();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // ── Handle window close → minimize to tray ───────────────────────────
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        // Snapshot current window list
        let all_windows: Vec<WindowEntry> = self.shared_windows.lock().unwrap().clone();

        // Apply filter
        let filter_lc = self.filter.to_lowercase();
        let filtered: Vec<&WindowEntry> = all_windows
            .iter()
            .filter(|w| {
                if self.protected_only && !w.is_protected {
                    return false;
                }
                if filter_lc.is_empty() {
                    return true;
                }
                w.title.to_lowercase().contains(&filter_lc)
                    || w.process_name.to_lowercase().contains(&filter_lc)
                    || w.pid.to_string().contains(&filter_lc)
            })
            .collect();

        // ── Help side panel ──────────────────────────────────────────────────
        if self.show_help {
            egui::SidePanel::right("help_panel")
                .min_width(340.0)
                .max_width(480.0)
                .show(ctx, |ui| {
                    render_help_panel(ui, &mut self.show_help, &mut self.help_section);
                });
        }

        // ── Top bar ──────────────────────────────────────────────────────────
        // Collect actions here to avoid borrow conflicts
        let mut do_strip_all = false;
        let mut toggle_mode = false;
        let mut toggle_auto = false;
        let mut toggle_startup = false;
        let mut toggle_help = false;
        let mut manual_refresh = false;

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("🔓  capture-bypass");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📖 Help").clicked() {
                        toggle_help = true;
                    }
                    ui.add_space(4.0);

                    let mode_label = if self.persistent_mode {
                        "🔁 Persistent"
                    } else {
                        "⚡ One-shot"
                    };
                    let mode_btn = egui::Button::new(mode_label)
                        .fill(if self.persistent_mode {
                            Color32::from_rgb(30, 100, 50)
                        } else {
                            Color32::from_rgb(26, 70, 100)
                        });
                    if ui.add(mode_btn).clicked() {
                        toggle_mode = true;
                    }
                    ui.label("Mode:");
                    ui.add_space(8.0);

                    let strip_all_btn =
                        egui::Button::new("⚡ Strip All Protected").fill(Color32::from_rgb(160, 30, 30));
                    if ui.add(strip_all_btn).clicked() {
                        do_strip_all = true;
                    }
                    ui.add_space(4.0);

                    if ui.button("⟳  Refresh").clicked() {
                        manual_refresh = true;
                    }
                });
            });
            ui.add_space(4.0);

            // Filter bar + checkboxes
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(220.0)
                        .hint_text("process, title, or PID"),
                );
                if ui.small_button("✕").clicked() {
                    self.filter.clear();
                }
                ui.add_space(12.0);
                ui.checkbox(&mut self.protected_only, "Protected only");
                ui.add_space(12.0);

                let auto_label = if self.auto_inject_enabled {
                    "🤖 Auto-inject ON"
                } else {
                    "🤖 Auto-inject OFF"
                };
                let auto_btn = egui::Button::new(auto_label).fill(if self.auto_inject_enabled {
                    Color32::from_rgb(20, 80, 140)
                } else {
                    Color32::from_rgb(50, 50, 50)
                });
                if ui.add(auto_btn).clicked() {
                    toggle_auto = true;
                }

                ui.add_space(12.0);

                let startup_label = if self.startup_enabled {
                    "🚀 Start with Windows ON"
                } else {
                    "🚀 Start with Windows"
                };
                let startup_btn = egui::Button::new(startup_label)
                    .fill(if self.startup_enabled {
                        Color32::from_rgb(60, 100, 30)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    });
                let startup_resp = ui.add(startup_btn)
                    .on_hover_text("Adds this app to HKCU\\Run so it launches on login.\nWindows will show a UAC prompt each time because the app requires Administrator rights.");
                if startup_resp.clicked() {
                    toggle_startup = true;
                }
            });
            ui.add_space(4.0);
        });

        // Apply deferred actions
        if toggle_help {
            self.show_help = !self.show_help;
        }
        if toggle_mode {
            self.persistent_mode = !self.persistent_mode;
            let mode = if self.persistent_mode { "Persistent" } else { "One-shot" };
            self.set_status_neutral(format!("Mode: {mode}"));
        }
        if toggle_auto {
            self.auto_inject_enabled = !self.auto_inject_enabled;
            if self.auto_inject_enabled {
                self.start_auto_inject();
                self.set_status("🤖 Auto-inject active — minimize to tray.", true);
            } else {
                self.stop_auto_inject();
                self.set_status_neutral("Auto-inject disabled.");
            }
        }
        if toggle_startup {
            let desired = !self.startup_enabled;
            if write_startup_reg(desired) {
                self.startup_enabled = desired;
                if desired {
                    self.set_status("🚀 Added to Windows startup.", true);
                } else {
                    self.set_status_neutral("Removed from Windows startup.");
                }
            } else {
                self.set_status("✗ Could not write startup registry key.", false);
            }
        }
        if manual_refresh {
            // Background thread handles refresh; just show feedback
            self.set_status_neutral("Refreshing…");
        }
        if do_strip_all {
            if all_windows.iter().any(|w| w.is_protected) {
                self.strip_all_protected(&all_windows);
            } else {
                self.set_status_neutral("No protected windows found — hit Refresh first.");
            }
        }

        // ── Status bar ───────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let ts = self
                    .status_time
                    .map(|t| {
                        let secs = t.elapsed().as_secs();
                        if secs < 60 {
                            format!(" [{secs}s ago]")
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!("{}{ts}", self.status_msg))
                        .color(self.status_color),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n_prot = all_windows.iter().filter(|w| w.is_protected).count();
                    let n_32 = all_windows.iter().filter(|w| w.is_32bit).count();
                    ui.label(
                        RichText::new(format!(
                            "{} windows  •  {} protected  •  {} 32-bit",
                            all_windows.len(),
                            n_prot,
                            n_32
                        ))
                        .weak()
                        .small(),
                    );
                });
            });
            ui.add_space(4.0);
        });

        // ── Main table ───────────────────────────────────────────────────────
        // Collect any row-level injection requests
        let mut inject_target: Option<WindowEntry> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            if filtered.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("No matching windows.").weak().italics());
                });
                return;
            }

            TableBuilder::new(ui)
                .striped(true)
                .resizable(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(70.0))   // PID
                .column(Column::exact(160.0))  // Process
                .column(Column::exact(40.0))   // Arch
                .column(Column::remainder())   // Title
                .column(Column::exact(115.0))  // Status
                .column(Column::exact(150.0))  // Action
                .header(22.0, |mut header| {
                    header.col(|ui| { ui.strong("PID"); });
                    header.col(|ui| { ui.strong("Process"); });
                    header.col(|ui| { ui.strong("Arch"); });
                    header.col(|ui| { ui.strong("Window Title"); });
                    header.col(|ui| { ui.strong("Status"); });
                    header.col(|ui| { ui.strong("Action"); });
                })
                .body(|mut body| {
                    for entry in &filtered {
                        let row_h = 26.0;
                        body.row(row_h, |mut row| {
                            // PID
                            row.col(|ui| {
                                ui.monospace(entry.pid.to_string());
                            });
                            // Process name
                            row.col(|ui| {
                                ui.colored_label(
                                    Color32::from_rgb(130, 180, 255),
                                    truncate(&entry.process_name, 24),
                                );
                            });
                            // Arch badge
                            row.col(|ui| {
                                if entry.is_32bit {
                                    ui.label(
                                        RichText::new("32")
                                            .color(Color32::from_rgb(255, 170, 68))
                                            .strong()
                                            .small(),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("64")
                                            .color(Color32::GRAY)
                                            .small(),
                                    );
                                }
                            });
                            // Title
                            row.col(|ui| {
                                ui.label(truncate(&entry.title, 72));
                            });
                            // Status badge
                            row.col(|ui| {
                                render_status_badge(ui, entry.affinity);
                            });
                            // Action button
                            row.col(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new("Strip Protection")
                                            .min_size([140.0, 20.0].into()),
                                    )
                                    .clicked()
                                {
                                    inject_target = Some((*entry).clone());
                                }
                            });
                        });
                    }
                });
        });

        if let Some(target) = inject_target {
            self.strip_window(&target);
        }
    }
}

// ── Help panel renderer ───────────────────────────────────────────────────────

fn render_help_panel(ui: &mut Ui, show: &mut bool, section: &mut usize) {
    ui.horizontal(|ui| {
        ui.heading("📖 Help");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕").clicked() {
                *show = false;
            }
        });
    });
    ui.separator();

    ui.horizontal_top(|ui| {
        // Sidebar navigation
        ui.vertical(|ui| {
            ui.set_min_width(130.0);
            for (i, (title, _)) in HELP_SECTIONS.iter().enumerate() {
                let selected = *section == i;
                let btn = egui::SelectableLabel::new(selected, *title);
                if ui.add(btn).clicked() {
                    *section = i;
                }
            }
        });

        ui.separator();

        // Content
        ui.vertical(|ui| {
            if let Some((title, content)) = HELP_SECTIONS.get(*section) {
                ui.strong(*title);
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.label(*content);
                    });
            }
        });
    });
}

// ── Status badge ──────────────────────────────────────────────────────────────

fn render_status_badge(ui: &mut Ui, affinity: u32) {
    match affinity {
        WDA_EXCLUDEFROMCAPTURE => {
            ui.label(RichText::new("● PROTECTED").color(Color32::from_rgb(255, 80, 80)).strong());
        }
        WDA_MONITOR => {
            ui.label(RichText::new("● MONITOR").color(Color32::from_rgb(255, 170, 68)).strong());
        }
        WDA_NONE => {
            ui.label(RichText::new("● OK").color(Color32::from_rgb(100, 220, 100)).strong());
        }
        _ => {
            ui.label(RichText::new("● ?").color(Color32::GRAY));
        }
    }
}

// ── Tray setup ────────────────────────────────────────────────────────────────

fn build_tray() -> (
    Option<tray_icon::TrayIcon>,
    Option<tray_icon::menu::MenuId>,
    Option<tray_icon::menu::MenuId>,
) {
    use tray_icon::{
        menu::{Menu, MenuItem, PredefinedMenuItem},
        Icon, TrayIconBuilder,
    };

    // Generate a 32×32 solid blue square as the tray icon
    let size: u32 = 32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for i in 0..(size * size) as usize {
        rgba[i * 4] = 0x44; // R
        rgba[i * 4 + 1] = 0x88; // G
        rgba[i * 4 + 2] = 0xFF; // B
        rgba[i * 4 + 3] = 0xFF; // A
    }

    let icon = match Icon::from_rgba(rgba, size, size) {
        Ok(i) => i,
        Err(_) => return (None, None, None),
    };

    let open_item = MenuItem::new("Open", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let open_id = open_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let sep = PredefinedMenuItem::separator();
    if menu
        .append_items(&[
            &open_item as &dyn tray_icon::menu::IsMenuItem,
            &sep,
            &quit_item,
        ])
        .is_err()
    {
        return (None, None, None);
    }

    match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("capture-bypass")
        .build()
    {
        Ok(tray) => (Some(tray), Some(open_id), Some(quit_id)),
        Err(_) => (None, None, None),
    }
}

// ── Window enumeration ────────────────────────────────────────────────────────

/// Callback state threaded through EnumWindows via lparam.
struct EnumState {
    entries: Vec<WindowEntry>,
}

unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut EnumState);

    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf);
    if len == 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&title_buf[..len as usize]);

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    let process_name = get_process_name(pid).unwrap_or_else(|| "<unknown>".into());

    let mut affinity: u32 = 0;
    let _ = GetWindowDisplayAffinity(hwnd, &mut affinity);
    let is_protected = affinity == WDA_EXCLUDEFROMCAPTURE || affinity == WDA_MONITOR;
    let is_32bit = is_process_32bit(pid);

    state.entries.push(WindowEntry {
        pid,
        process_name,
        title,
        affinity,
        is_protected,
        is_32bit,
    });

    TRUE
}

fn enumerate_windows() -> Vec<WindowEntry> {
    let mut state = EnumState {
        entries: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_cb),
            LPARAM(&mut state as *mut EnumState as isize),
        );
    }
    state.entries
}

// ── Windows helpers ───────────────────────────────────────────────────────────

fn get_process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .ok()?;
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }
}

fn is_process_32bit(pid: u32) -> bool {
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let mut wow64 = BOOL(0);
        let _ = IsWow64Process(handle, &mut wow64);
        wow64.as_bool()
    }
}

fn get_child_pids(parent_pid: u32) -> Vec<u32> {
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut children = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ParentProcessID == parent_pid {
                    children.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        children
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max_chars {
        format!("{}…", chars[..max_chars].iter().collect::<String>())
    } else {
        s.to_string()
    }
}

// ── Windows startup registry helpers ─────────────────────────────────────────

const STARTUP_RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "capture-bypass";

/// Returns true if the startup registry entry currently exists.
fn read_startup_reg() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run) = hkcu.open_subkey(STARTUP_RUN_KEY) {
        run.get_value::<String, _>(STARTUP_VALUE_NAME).is_ok()
    } else {
        false
    }
}

/// Writes (enable=true) or deletes (enable=false) the startup entry.
/// Returns true on success.
fn write_startup_reg(enable: bool) -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(STARTUP_RUN_KEY, KEY_WRITE) else {
        return false;
    };
    if enable {
        // Quote the path in case it contains spaces (e.g. Program Files)
        let exe = std::env::current_exe().unwrap_or_default();
        let value = format!("\"{}\"", exe.display());
        run.set_value(STARTUP_VALUE_NAME, &value).is_ok()
    } else {
        match run.delete_value(STARTUP_VALUE_NAME) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true, // already gone
            Err(_) => false,
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("capture-bypass")
            .with_inner_size([1100.0, 650.0])
            .with_min_inner_size([900.0, 550.0]),
        ..Default::default()
    };

    eframe::run_native(
        "capture-bypass",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
