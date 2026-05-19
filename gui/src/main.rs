//! capture-bypass GUI — Rust/egui frontend
//!
//! Requires Administrator privileges (enforced by embedded UAC manifest via build.rs).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
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
    Graphics::Gdi::{
        GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    },
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
    UI::{
        Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON},
        Input::KeyboardAndMouse::{
            HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
        },
        WindowsAndMessaging::{
            EnumWindows, GetIconInfo, GetWindowDisplayAffinity, GetWindowTextW,
            GetWindowThreadProcessId, ICONINFO, IsWindowVisible, MSG, PeekMessageW,
            PM_REMOVE, WM_HOTKEY,
        },
    },
};

// Constants

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

// Persistent config

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    persistent_mode: bool,
    #[serde(default)]
    auto_inject: bool,
    #[serde(default)]
    protected_only: bool,
    #[serde(default)]
    toast_enabled: bool,
    #[serde(default)]
    hotkey_enabled: bool,
    #[serde(default)]
    show_log: bool,
    #[serde(default)]
    watch_names: Vec<String>,
    #[serde(default = "default_sort_col")]
    sort_col: u8, // 0=pid,1=process,2=title,3=status
    #[serde(default)]
    sort_asc: bool,
    #[serde(default = "default_minimize_to_tray")]
    minimize_to_tray: bool,
    #[serde(default)]
    logging_enabled: bool,
    #[serde(default)]
    discord_rpc_enabled: bool,
}

fn default_sort_col() -> u8 { 0 }
fn default_minimize_to_tray() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Config {
            persistent_mode: false,
            auto_inject: false,
            protected_only: false,
            toast_enabled: false,
            hotkey_enabled: false,
            show_log: false,
            watch_names: Vec::new(),
            sort_col: 0,
            sort_asc: true,
            minimize_to_tray: true,
            logging_enabled: false,
            discord_rpc_enabled: false,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("capture-bypass").join("config.toml"))
}

fn load_config() -> Config {
    let path = match config_path() {
        Some(p) => p,
        None => return Config::default(),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Config::default(),
    };
    toml::from_str(&text).unwrap_or_default()
}

fn save_config(cfg: &Config) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

// Help content
//
// Structure: &[( tab_label, &[( sub_heading, body_text )] )]
// render_help_window() turns each sub_heading into a bold label + separator,
// so no raw ━━━ dividers needed.

const HELP_SECTIONS: &[(&str, &[(&str, &str)])] = &[
    ("Overview", &[
        ("What is Capture Bypass?",
            "Capture Bypass removes the WDA_EXCLUDEFROMCAPTURE screen-capture \
             protection from Windows application windows, letting OBS, the \
             Snipping Tool, and any other screen-capture software record them \
             normally."),
        ("How does it work?",
            "Windows provides SetWindowDisplayAffinity(), which lets a process \
             protect its own windows from capture.  Because the API only works \
             on a process's own windows, bypassing it requires running code \
             INSIDE the target process.\n\
             \n\
             Capture Bypass does this via DLL injection:\n\
             \n\
             1. OpenProcess        — open a handle to the target process\n\
             2. VirtualAllocEx     — allocate memory inside the target\n\
             3. WriteProcessMemory — write the payload DLL path into that memory\n\
             4. CreateRemoteThread — start a thread inside the target that calls\n\
                                     LoadLibraryA, loading the payload DLL\n\
             5. The DLL's DllMain  — calls SetWindowDisplayAffinity(hwnd, WDA_NONE)\n\
                                     for every window owned by that process"),
        ("Legal notice",
            "Only use this tool on windows and processes you own or have \
             explicit permission to capture.  See DISCLAIMER.md in the \
             repository for the full legal disclaimer."),
    ]),
    ("Requirements & Build", &[
        ("Requirements",
            "• Windows 10 build 19041+  (WDA_EXCLUDEFROMCAPTURE requires 2004+)\n\
             • Administrator privileges  (OpenProcess on other processes requires admin)\n\
             • Rust + Cargo  (https://rustup.rs)"),
        ("Build — x64 (required)",
            "cargo build --release -p payload_dll -p payload_dll_persistent -p gui\n\
             \n\
             Binaries land in:  target\\release\\"),
        ("Build — x86 (optional, for 32-bit targets)",
            "rustup target add i686-pc-windows-msvc\n\
             cargo build --release --target i686-pc-windows-msvc \\\n\
                   -p payload_dll -p payload_dll_persistent\n\
             \n\
             Binaries land in:  target\\i686-pc-windows-msvc\\release\\\n\
             \n\
             32-bit processes are shown with an orange \"32\" badge in the table."),
    ]),
    ("Usage Guide", &[
        ("Window list",
            "The table shows every visible, titled window with:\n\
             \n\
             PID      — Process ID\n\
             Process  — Executable name  (orange \"32\" badge = 32-bit process)\n\
             Title    — Window title\n\
             Status   — Live protection state, refreshed every 500 ms:\n\
                          PROTECTED = WDA_EXCLUDEFROMCAPTURE\n\
                          MONITOR   = WDA_MONITOR\n\
                          OK        = WDA_NONE (capturable)\n\
             Action   — \"Strip Protection\" button for that row"),
        ("Header buttons",
            "⟳ Refresh             Re-enumerate all windows immediately.\n\
             \n\
             ⚡ Strip All Protected Inject into every currently-protected PID at once.\n\
                                    Each process is injected only once even if it owns\n\
                                    multiple protected windows.\n\
             \n\
             Mode                  Toggle between One-shot and Persistent injection.\n\
             \n\
             🔨 Stress Test        Launch stress_tester.exe — a self-protecting window.\n\
                                    Includes Fight Mode, Scenario A (process scan),\n\
                                    and Scenario B (module ejection).\n\
             \n\
             ⚙ Settings            Opens the Settings window (startup, notifications,\n\
                                    and global hotkey).\n\
             \n\
             📋 Log                Toggle the injection history panel.\n\
             \n\
             📖 Help               Opens this window."),
        ("Filter bar",
            "Type to search live by window title, process name, or PID.  Click ✕ to clear.\n\
             \n\
             \"Protected only\" checkbox hides all unprotected windows.\n\
             \n\
             🤖 Auto-inject — background thread strips newly-protected windows \
             automatically.  Continues running while the app is minimised to tray."),
        ("Status bar",
            "Shows the last action result and how long ago it occurred.\n\
             Green = success     Red = error     Gray = neutral / informational"),
    ]),
    ("Injection Modes", &[
        ("⚡ One-shot mode (default)",
            "The payload DLL strips WDA protection once and exits.  \
             Fast and lightweight.\n\
             \n\
             Use when: the target app sets protection only once at startup \
             and never re-applies it."),
        ("🔁 Persistent mode",
            "The payload DLL stays alive inside the target process and \
             re-applies WDA_NONE every 500 ms for the entire lifetime of \
             the process.\n\
             \n\
             Use when: the target app calls SetWindowDisplayAffinity on a \
             timer to fight back against one-shot injection (e.g. DRM video \
             players)."),
        ("Re-injection & re-protection",
            "Windows caches loaded DLLs by file path — if the same DLL path \
             is already loaded in a process, LoadLibraryA silently no-ops.\n\
             \n\
             If a one-shot strip appears to succeed but the status badge \
             returns to PROTECTED shortly after, the app is re-applying \
             protection on a timer.  Switch to 🔁 Persistent mode — a popup \
             will also appear automatically when this is detected."),
    ]),
    ("Browser Injection", &[
        ("Why browsers need special handling",
            "Chrome, Edge, Firefox, Brave, Opera, Vivaldi, and Thorium use a \
             multi-process architecture.  DRM-protected video is rendered in a \
             separate child (renderer) process that owns its own windows.  \
             Injecting only into the main PID won't strip the video window."),
        ("What Capture Bypass does",
            "When you click \"Strip Protection\" on a browser row, the app \
             automatically:\n\
             \n\
             1. Injects the payload into the main (browser) PID\n\
             2. Enumerates all child processes via CreateToolhelp32Snapshot\n\
             3. Injects into every child PID as well"),
        ("Tip",
            "If a browser re-applies protection after navigating to a new \
             video, click ⚡ Strip All Protected again, or enable \
             🤖 Auto-inject so it's handled automatically."),
    ]),
    ("System Tray & Auto-inject", &[
        ("System tray",
            "Clicking the window's ✕ close button hides the app to the system \
             tray instead of quitting — the icon remains in the notification area.\n\
             \n\
             Tray icon right-click menu:\n\
               Open  — restore the main window\n\
               Quit  — fully exit the application"),
        ("Auto-inject",
            "Enable the 🤖 Auto-inject toggle in the toolbar.\n\
             \n\
             A background thread polls GetWindowDisplayAffinity every 500 ms. \
             Any window that becomes protected and hasn't been seen before is \
             automatically stripped.\n\
             \n\
             Designed for streamers: enable auto-inject, minimise to tray, and \
             any app that tries to block capture is handled silently in the \
             background."),
    ]),
    ("Troubleshooting", &[
        ("DLLs not found",
            "The payload DLLs haven't been built yet.  Run:\n\
             \n\
             cargo build --release -p payload_dll -p payload_dll_persistent"),
        ("\"Strip failed\" / access denied",
            "OpenProcess requires elevated privileges for processes not owned \
             by your session.  Make sure capture_bypass_gui.exe is running as \
             Administrator (the UAC prompt appears on launch)."),
        ("Injection succeeds but window is still black in OBS",
            "1. Wait for the next 500 ms refresh — if the status badge shows OK, \
             OBS may need its capture source refreshed (remove and re-add it).\n\
             2. For browsers, click Strip Protection again — it re-injects \
             child processes too.\n\
             3. If the badge keeps flipping back to PROTECTED, the app is \
             fighting back on a timer.  Switch to 🔁 Persistent mode."),
        ("Antivirus flags the DLL",
            "DLL injection is used by both legitimate tools and malware, so \
             heuristic scanners may flag the payload.  Inspect \
             payload_dll/src/lib.rs — it only calls SetWindowDisplayAffinity.\n\
             \n\
             Add an exclusion for the target\\ directory in your AV settings."),
        ("x86 injection fails even with x86 binaries present",
            "A 64-bit process cannot inject into a 32-bit process and \
             vice-versa.  Verify you built the x86 target:\n\
             \n\
             rustup target add i686-pc-windows-msvc\n\
             cargo build --release --target i686-pc-windows-msvc -p payload_dll"),
    ]),
];

// Data model

#[derive(Clone, Debug)]
struct WindowEntry {
    pid: u32,
    process_name: String,
    title: String,
    affinity: u32,
    is_protected: bool,
    is_32bit: bool,
}

// Injection result message

struct InjResult {
    msg: String,
    ok: bool,
}

// Injection log

struct LogEntry {
    time: Instant,
    msg: String,
    ok: bool,
}

// Column sort

#[derive(Clone, Copy, PartialEq)]
enum SortCol { Pid, Process, Title, Status }

impl SortCol {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => SortCol::Process,
            2 => SortCol::Title,
            3 => SortCol::Status,
            _ => SortCol::Pid,
        }
    }
    fn to_u8(self) -> u8 {
        match self {
            SortCol::Pid => 0,
            SortCol::Process => 1,
            SortCol::Title => 2,
            SortCol::Status => 3,
        }
    }
}

// Update check state

enum UpdateState {
    Checking,
    Available(String), // newer tag name
    UpToDate,
    Failed,
}

// Discord RPC -- sent from the main thread to the RPC background thread
// whenever something worth showing changes.
struct RpcState {
    auto_inject: bool,
    persistent:  bool,
    strip_count: u32,
}

const DISCORD_APP_ID: &str = "1506230832283648000";

// App

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
    show_settings: bool,

    // Status bar
    status_msg: String,
    status_color: Color32,
    status_time: Option<Instant>,

    // Launch at Windows startup
    startup_enabled: bool,

    // One-shot re-injection tracking
    one_shot_stripped: HashMap<u32, String>,
    reapply_alert: Vec<(u32, String)>,

    // Injection log
    log_entries: Vec<LogEntry>,
    show_log: bool,

    // Column sorting
    sort_col: SortCol,
    sort_asc: bool,

    // Watch mode — inject automatically whenever a process with a watched name appears
    watch_names: Vec<String>,
    watch_input: String,
    watch_seen: HashSet<u32>, // PIDs already handled by watch mode this session

    // Global hotkey (Ctrl+Shift+B → Strip All Protected)
    hotkey_enabled: bool,
    hotkey_id: i32,

    // Auto-update
    update_state: Option<UpdateState>,
    update_rx: Option<Receiver<UpdateState>>,

    // Toast notifications (auto-inject strips show a desktop notification)
    toast_enabled: bool,

    // Process icon cache: process_name → egui TextureHandle
    icon_cache: HashMap<String, Option<egui::TextureHandle>>,

    // Tray (must stay alive for the duration of the app)
    _tray_icon: Option<tray_icon::TrayIcon>,
    tray_open_id: Option<tray_icon::menu::MenuId>,
    tray_quit_id: Option<tray_icon::menu::MenuId>,
    // When true, X hides to tray instead of closing the process.
    minimize_to_tray: bool,
    // When true, each injection is appended to %APPDATA%\capture-bypass\injection.log
    logging_enabled: bool,

    // Discord Rich Presence
    discord_rpc_enabled: bool,
    discord_rpc_running: Arc<AtomicBool>,
    discord_rpc_tx: Option<std::sync::mpsc::SyncSender<RpcState>>,
    // How many successful strips this session (shown in Discord status)
    session_strip_count: Arc<std::sync::atomic::AtomicU32>,

    // Set by the off-thread tray watcher when the user clicks Open.
    tray_show: Arc<AtomicBool>,

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

        let cfg = load_config();

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

        // Kick off auto-update check in background
        let (update_tx, update_rx) = mpsc::channel::<UpdateState>();
        std::thread::spawn(move || {
            let _ = update_tx.send(UpdateState::Checking);
            match check_for_update() {
                Ok(Some(tag)) => { let _ = update_tx.send(UpdateState::Available(tag)); }
                Ok(None)      => { let _ = update_tx.send(UpdateState::UpToDate); }
                Err(_)        => { let _ = update_tx.send(UpdateState::Failed); }
            }
        });

        // Register global hotkey if saved as enabled
        let hotkey_id = 1_i32;
        if cfg.hotkey_enabled {
            register_hotkey(hotkey_id);
        }

        // Set up system tray
        let (tray_icon, tray_open_id, tray_quit_id) = build_tray();

        // Tray event watcher thread
        // We cannot poll tray events inside update() because winit stops
        // dispatching repaints to hidden viewports, so update() never runs
        // while the window is in the tray.  A dedicated blocking thread solves
        // this: it receives events regardless of window visibility.
        // Quit  → exit(0) immediately (OS cleans up the tray icon on process death)
        // Open  → set tray_show flag + request_repaint() so update() restores the window
        let tray_show = Arc::new(AtomicBool::new(false));
        {
            let quit_id    = tray_quit_id.clone();
            let open_id    = tray_open_id.clone();
            let show_flag  = Arc::clone(&tray_show);
            let repaint_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                loop {
                    if let Ok(event) = tray_icon::menu::MenuEvent::receiver().recv() {
                        let is_quit = quit_id.as_ref().map(|id| id == &event.id).unwrap_or(false);
                        let is_open = open_id.as_ref().map(|id| id == &event.id).unwrap_or(false);
                        if is_quit {
                            std::process::exit(0);
                        } else if is_open {
                            show_flag.store(true, Ordering::Relaxed);
                            repaint_ctx.request_repaint();
                        }
                    }
                }
            });
        }

        let mut app = App {
            shared_windows,
            exe_dir,
            persistent_mode: cfg.persistent_mode,
            auto_inject_enabled: false, // started below if saved
            auto_inject_running: Arc::new(AtomicBool::new(false)),
            auto_inject_seen: Arc::new(Mutex::new(HashSet::new())),
            filter: String::new(),
            protected_only: cfg.protected_only,
            show_help: false,
            help_section: 0,
            show_settings: false,
            status_msg: String::from("Ready."),
            status_color: Color32::GRAY,
            status_time: None,
            startup_enabled: read_startup_reg(),
            one_shot_stripped: HashMap::new(),
            reapply_alert: Vec::new(),
            log_entries: Vec::new(),
            show_log: cfg.show_log,
            sort_col: SortCol::from_u8(cfg.sort_col),
            sort_asc: cfg.sort_asc,
            watch_names: cfg.watch_names.clone(),
            watch_input: String::new(),
            watch_seen: HashSet::new(),
            hotkey_enabled: cfg.hotkey_enabled,
            hotkey_id,
            update_state: None,
            update_rx: Some(update_rx),
            toast_enabled: cfg.toast_enabled,
            icon_cache: HashMap::new(),
            _tray_icon: tray_icon,
            tray_open_id,
            tray_quit_id,
            minimize_to_tray: cfg.minimize_to_tray,
            logging_enabled: cfg.logging_enabled,
            discord_rpc_enabled: cfg.discord_rpc_enabled,
            discord_rpc_running: Arc::new(AtomicBool::new(false)),
            discord_rpc_tx: None,
            session_strip_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            tray_show,
            inject_tx,
            inject_rx,
        };

        // Restore auto-inject if it was running when the app last closed
        if cfg.auto_inject {
            app.auto_inject_enabled = true;
            app.start_auto_inject();
        }

        // Restore Discord RPC if it was enabled
        if cfg.discord_rpc_enabled {
            app.start_discord_rpc();
        }

        app
    }

    // DLL path resolution

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

    // Injection

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
            match injector_core::inject_dll_stealth(pid, &dll_path) {
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

    // Injection log file helpers

    /// Returns `%APPDATA%\capture-bypass\injection.log`
    fn log_path() -> Option<std::path::PathBuf> {
        dirs_next::config_dir().map(|d| d.join("capture-bypass").join("injection.log"))
    }

    /// Computes a UTC timestamp string (YYYY-MM-DD HH:MM:SS UTC) from SystemTime
    /// without pulling in the chrono crate.
    fn utc_timestamp() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Gregorian calendar math
        let s   = secs % 60;
        let m   = (secs / 60) % 60;
        let h   = (secs / 3_600) % 24;
        let days = secs / 86_400; // days since 1970-01-01
        // Shift epoch to 2000-03-01 (makes leap-year math simpler)
        let days400 = days + 10_957 + 31 + 28; // offset to 2000-03-01
        let (era, doe) = {
            let era = days400 / 146_097;
            (era, days400 - era * 146_097)
        };
        let yoe = (doe - doe/1_460 + doe/36_524 - doe/146_096) / 365;
        let y   = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp  = (5 * doy + 2) / 153;
        let d   = doy - (153 * mp + 2) / 5 + 1;
        let mo  = if mp < 10 { mp + 3 } else { mp - 9 };
        let yr  = if mo <= 2 { y + 1 } else { y };
        format!("{yr:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
    }

    /// Appends a single log line to the injection log file.
    /// Creates the directory and file if they don't exist.
    fn append_log_entry(&self, line: &str) {
        if let Some(path) = Self::log_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true).append(true).open(&path)
            {
                let ts = Self::utc_timestamp();
                let _ = writeln!(f, "[{ts}] {line}");
            }
        }
    }

    // Persist config

    fn persist(&self) {
        save_config(&Config {
            persistent_mode: self.persistent_mode,
            auto_inject: self.auto_inject_enabled,
            protected_only: self.protected_only,
            toast_enabled: self.toast_enabled,
            hotkey_enabled: self.hotkey_enabled,
            show_log: self.show_log,
            watch_names: self.watch_names.clone(),
            sort_col: self.sort_col.to_u8(),
            sort_asc: self.sort_asc,
            minimize_to_tray: self.minimize_to_tray,
            logging_enabled: self.logging_enabled,
            discord_rpc_enabled: self.discord_rpc_enabled,
        });
    }

    // Auto-inject

    fn start_auto_inject(&mut self) {
        self.auto_inject_running.store(true, Ordering::Relaxed);
        let running   = Arc::clone(&self.auto_inject_running);
        let exe_dir   = self.exe_dir.clone();
        let persistent = self.persistent_mode;
        let tx        = self.inject_tx.clone();
        let toast     = self.toast_enabled;

        std::thread::spawn(move || {
            // Per-PID state, entirely local to this thread — no Arc needed.
            //
            // (process_name, used_persistent, gave_up)
            //
            //  gave_up = true  → OS-level block (MitigationPolicy); don't retry.
            //  used_persistent = true → persistent DLL is active; protection
            //      shouldn't return, but skip quietly if it does (transient).
            //  Neither → one-shot injection succeeded; if protection comes back
            //      for the same PID we escalate to persistent automatically.
            let mut state: HashMap<u32, (String, bool, bool)> = HashMap::new();

            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(800));

                let windows = enumerate_windows();

                // Remove entries for processes that have exited
                // When a process restarts it gets a fresh PID and is treated as
                // a new arrival, which is exactly what we want.
                let live_pids: HashSet<u32> = windows.iter().map(|w| w.pid).collect();
                state.retain(|pid, _| live_pids.contains(pid));

                for w in windows.iter().filter(|w| w.is_protected) {
                    // Decide what action (if any) to take
                    let (use_persistent, is_escalation) =
                        match state.get(&w.pid) {
                            // Never seen this PID → use the user's global setting.
                            None => (persistent, false),

                            // Persistent DLL already active — protection coming
                            // back is transient (hook races); skip quietly.
                            Some((_, true, _)) => continue,

                            // OS policy blocked us — nothing more we can do.
                            Some((_, _, true)) => continue,

                            // One-shot injection succeeded earlier but protection
                            // is back → the app re-applied it.  Escalate to the
                            // persistent DLL, which hooks SetWindowDisplayAffinity
                            // so the app can no longer reapply the flag.
                            Some((_, false, false)) => (true, true),
                        };

                    let dll_name = if use_persistent {
                        "payload_dll_persistent.dll"
                    } else {
                        "payload_dll.dll"
                    };

                    let resolve_dll = |is32: bool| -> PathBuf {
                        if is32 {
                            let p = exe_dir.join("x86").join(dll_name);
                            if p.exists() { return p; }
                            exe_dir.join("..").join("i686-pc-windows-msvc")
                                   .join("release").join(dll_name)
                        } else {
                            exe_dir.join(dll_name)
                        }
                    };

                    // Expand browsers to child processes (renderer isolation).
                    let mut pids: Vec<(u32, String, bool)> =
                        vec![(w.pid, w.process_name.clone(), w.is_32bit)];
                    if BROWSER_NAMES.iter().any(|b| w.process_name.eq_ignore_ascii_case(b)) {
                        for child in get_child_pids(w.pid) {
                            pids.push((child, w.process_name.clone(),
                                       is_process_32bit(child)));
                        }
                    }

                    for (pid, name, is32) in pids {
                        let dll_path = resolve_dll(is32);
                        if !dll_path.exists() { continue; }

                        match injector_core::inject_checked(pid, &dll_path) {
                            Ok(()) => {
                                let verb = if is_escalation {
                                    "🔄 Escalated→persistent"
                                } else {
                                    "🤖 Auto-stripped"
                                };
                                let msg = format!("{verb} {name} (PID {pid})");
                                if toast { send_toast("capture-bypass", &msg); }
                                let _ = tx.send(InjResult { msg, ok: true });
                                state.insert(pid, (name, use_persistent, false));
                            }

                            // OS-enforced block — nothing we can do in user-mode.
                            // Log once and give up on this PID.
                            Err(injector_core::InjectError::MitigationPolicy(reason)) => {
                                let msg = format!(
                                    "⛔ {name} (PID {pid}) blocked by OS policy: {reason}"
                                );
                                let _ = tx.send(InjResult { msg, ok: false });
                                state.insert(pid, (name, false, true));
                            }

                            // Other errors (privilege race, timing) — log only
                            // if we haven't already logged for this PID.
                            Err(e) => {
                                if !state.contains_key(&pid) {
                                    let msg = format!("⚠️ {name} (PID {pid}): {e}");
                                    let _ = tx.send(InjResult { msg, ok: false });
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    fn stop_auto_inject(&mut self) {
        self.auto_inject_running.store(false, Ordering::Relaxed);
    }

    // Discord Rich Presence

    fn start_discord_rpc(&mut self) {
        use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
        use std::sync::mpsc;

        if self.discord_rpc_running.load(Ordering::Relaxed) {
            return;
        }
        self.discord_rpc_running.store(true, Ordering::Relaxed);

        let (tx, rx) = mpsc::sync_channel::<RpcState>(8);
        self.discord_rpc_tx = Some(tx);

        let running = Arc::clone(&self.discord_rpc_running);
        let strip_count = Arc::clone(&self.session_strip_count);

        std::thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || {
                // new() is infallible in discord-rich-presence 1.1 — connect() can fail
                let mut client = DiscordIpcClient::new(DISCORD_APP_ID);
                if client.connect().is_err() {
                    return;
                }

                // Push the initial presence right away
                let count = strip_count.load(Ordering::Relaxed);
                let _ = client.set_activity(
                    activity::Activity::new()
                        .details("capture-bypass")
                        .state(&format!("Strips this session: {count}"))
                        .buttons(vec![
                            activity::Button::new(
                                "Get capture-bypass",
                                "https://github.com/Londopy/capture-bypass/releases/latest",
                            ),
                            activity::Button::new(
                                "★ GitHub",
                                "https://github.com/Londopy/capture-bypass",
                            ),
                        ]),
                );

                // Keep updating whenever the main thread sends a new state
                while running.load(Ordering::Relaxed) {
                    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                        Ok(state) => {
                            let mode_str = if state.persistent { "persistent" } else { "one-shot" };
                            let auto_str = if state.auto_inject { "auto-inject on" } else { "manual" };
                            let status = format!("{auto_str} · {mode_str} · {strips} strips",
                                strips = state.strip_count);
                            let _ = client.set_activity(
                                activity::Activity::new()
                                    .details("capture-bypass")
                                    .state(&status)
                                    .buttons(vec![
                                        activity::Button::new(
                                            "Get capture-bypass",
                                            "https://github.com/Londopy/capture-bypass/releases/latest",
                                        ),
                                        activity::Button::new(
                                            "★ GitHub",
                                            "https://github.com/Londopy/capture-bypass",
                                        ),
                                    ]),
                            );
                        }
                        // Timeout just means no update — stay connected and loop
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        // Channel closed — main thread dropped the sender
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }

                let _ = client.clear_activity();
                let _ = client.close();
            })
            .ok();
    }

    fn stop_discord_rpc(&mut self) {
        self.discord_rpc_running.store(false, Ordering::Relaxed);
        // Dropping the sender unblocks the RPC thread's recv_timeout immediately
        self.discord_rpc_tx = None;
    }

    // Send the current state to the RPC thread (no-op if RPC is off)
    fn push_rpc_state(&self) {
        if let Some(tx) = &self.discord_rpc_tx {
            let _ = tx.try_send(RpcState {
                auto_inject: self.auto_inject_enabled,
                persistent:  self.persistent_mode,
                strip_count: self.session_strip_count.load(Ordering::Relaxed),
            });
        }
    }
}

// egui rendering

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll injection results
        let mut rpc_needs_update = false;
        while let Ok(result) = self.inject_rx.try_recv() {
            if self.logging_enabled {
                self.append_log_entry(&result.msg);
            }
            if result.ok {
                self.session_strip_count.fetch_add(1, Ordering::Relaxed);
                rpc_needs_update = true;
            }
            self.log_entries.push(LogEntry {
                time: Instant::now(),
                msg: result.msg.clone(),
                ok: result.ok,
            });
            if self.log_entries.len() > 500 {
                self.log_entries.remove(0);
            }
            self.set_status(result.msg, result.ok);
        }
        if rpc_needs_update {
            self.push_rpc_state();
        }

        // Poll auto-update result
        if let Some(rx) = &self.update_rx {
            while let Ok(state) = rx.try_recv() {
                self.update_state = Some(state);
            }
        }

        // Poll global hotkey messages
        if self.hotkey_enabled {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                if PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                    // WM_HOTKEY — Strip All Protected
                    let all = self.shared_windows.lock().unwrap().clone();
                    if all.iter().any(|w| w.is_protected) {
                        self.strip_all_protected(&all);
                    }
                }
            }
        }

        // Tray "Open" action
        // Quit is handled by the off-thread tray watcher (process::exit).
        // Open sets this flag + requests a repaint so we catch it here.
        if self.tray_show.load(Ordering::Relaxed) {
            self.tray_show.store(false, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Handle window close (X button)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.minimize_to_tray {
                // Hide to tray — the tray watcher thread handles true quit.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            // else: let eframe handle the close normally → process exits.
        }

        // Snapshot current window list
        let all_windows: Vec<WindowEntry> = self.shared_windows.lock().unwrap().clone();

        // Detect re-applied protection on one-shot stripped processes
        // Only fires when NOT in persistent mode and NOT in auto-inject (which
        // would handle it silently).  Moves matching PIDs to reapply_alert so the
        // modal popup can prompt the user.
        if !self.persistent_mode && !self.auto_inject_enabled && !self.one_shot_stripped.is_empty() {
            for w in all_windows.iter().filter(|w| w.is_protected) {
                if let Some(name) = self.one_shot_stripped.remove(&w.pid) {
                    if !self.reapply_alert.iter().any(|(pid, _)| *pid == w.pid) {
                        self.reapply_alert.push((w.pid, name));
                    }
                }
            }
        }

        // Watch mode — inject when a watched process name appears
        if !self.watch_names.is_empty() && !self.auto_inject_enabled {
            for w in all_windows.iter() {
                let matched = self.watch_names.iter().any(|n| {
                    w.process_name.to_lowercase() == n.to_lowercase()
                });
                if matched && !self.watch_seen.contains(&w.pid) {
                    self.watch_seen.insert(w.pid);
                    self.inject_pid_async(w.pid, w.process_name.clone(), w.is_32bit);
                }
            }
            // Prune PIDs that are no longer alive
            let live_pids: HashSet<u32> = all_windows.iter().map(|w| w.pid).collect();
            self.watch_seen.retain(|pid| live_pids.contains(pid));
        }

        // Apply filter
        let filter_lc = self.filter.to_lowercase();
        let mut filtered: Vec<&WindowEntry> = all_windows
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

        // Apply column sort
        let sort_col = self.sort_col;
        let sort_asc = self.sort_asc;
        filtered.sort_by(|a, b| {
            let ord = match sort_col {
                SortCol::Pid     => a.pid.cmp(&b.pid),
                SortCol::Process => a.process_name.to_lowercase().cmp(&b.process_name.to_lowercase()),
                SortCol::Title   => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                SortCol::Status  => a.affinity.cmp(&b.affinity),
            };
            if sort_asc { ord } else { ord.reverse() }
        });

        // Help window
        render_help_window(ctx, &mut self.show_help, &mut self.help_section);

        // Settings window
        let mut toggle_startup_from_settings          = false;
        let mut toggle_toast_from_settings            = false;
        let mut toggle_hotkey_from_settings           = false;
        let mut toggle_minimize_to_tray_from_settings = false;
        let mut toggle_logging_from_settings          = false;
        let mut open_log_file_from_settings           = false;
        let mut toggle_discord_rpc_from_settings      = false;
        render_settings_window(
            ctx,
            &mut self.show_settings,
            self.startup_enabled,
            self.toast_enabled,
            self.hotkey_enabled,
            self.minimize_to_tray,
            self.logging_enabled,
            self.discord_rpc_enabled,
            &mut toggle_startup_from_settings,
            &mut toggle_toast_from_settings,
            &mut toggle_hotkey_from_settings,
            &mut toggle_minimize_to_tray_from_settings,
            &mut toggle_logging_from_settings,
            &mut open_log_file_from_settings,
            &mut toggle_discord_rpc_from_settings,
        );

        // Top bar
        // Collect actions here to avoid borrow conflicts
        let mut do_strip_all = false;
        let mut toggle_mode = false;
        let mut toggle_auto = false;
        let mut toggle_help = false;
        let mut toggle_settings = false;
        let mut manual_refresh = false;
        let mut do_stress_test = false;
        let mut toggle_log = false;
        let mut new_sort: Option<(SortCol, bool)> = None;
        let mut remove_watch: Option<usize> = None;
        let mut add_watch = false;
        // Modal reapply actions — collected during modal rendering below.
        let mut dismiss_reapply = false;
        let mut switch_and_restrip = false;

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("🔓  capture-bypass");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📖 Help").clicked() {
                        toggle_help = true;
                    }
                    ui.add_space(4.0);

                    // Update available notification
                    if let Some(UpdateState::Available(tag)) = &self.update_state {
                        let tag = tag.clone();
                        if ui
                            .add(egui::Button::new(format!("🆕 v{tag} available"))
                                .fill(Color32::from_rgb(80, 50, 10)))
                            .on_hover_text("Click to open the releases page")
                            .clicked()
                        {
                            // open::that() silently fails when running as Administrator because
                            // ShellExecute can't hand a URL off to a non-elevated browser process.
                            // Routing through explorer.exe works because explorer runs at normal
                            // user privilege and accepts URL arguments from elevated callers.
                            let _ = std::process::Command::new("explorer.exe")
                                .arg("https://github.com/Londopy/capture-bypass/releases/latest")
                                .spawn();
                        }
                        ui.add_space(4.0);
                    }

                    // Settings
                    if ui.add(egui::Button::new("⚙ Settings")
                        .fill(Color32::from_rgb(50, 50, 50)))
                        .on_hover_text("Startup, notifications, hotkey")
                        .clicked()
                    {
                        toggle_settings = true;
                    }
                    ui.add_space(4.0);

                    // Log toggle
                    let log_label = if self.show_log { "📋 Log ON" } else { "📋 Log" };
                    if ui.add(egui::Button::new(log_label)
                        .fill(if self.show_log { Color32::from_rgb(30, 60, 40) } else { Color32::from_rgb(50, 50, 50) }))
                        .on_hover_text("Toggle the injection log panel")
                        .clicked()
                    {
                        toggle_log = true;
                    }
                    ui.add_space(4.0);

                    if ui
                        .button("🔨 Stress Test")
                        .on_hover_text(
                            "Launch stress_tester.exe — a self-protecting window.\n\
                             Also tests Scenario A (process scan) and Scenario B\n\
                             (module ejection) to verify stealth defences work.",
                        )
                        .clicked()
                    {
                        do_stress_test = true;
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
            });

            // Watch mode row
            ui.horizontal(|ui| {
                ui.label("Watch:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.watch_input)
                        .desired_width(150.0)
                        .hint_text("process.exe"),
                );
                if ui.small_button("➕ Add").clicked() {
                    add_watch = true;
                }
                ui.add_space(8.0);
                let names: Vec<(usize, String)> = self.watch_names.iter().cloned().enumerate().collect();
                for (i, name) in names {
                    let resp = ui.add(
                        egui::Button::new(format!("👁 {name}  ✕"))
                            .fill(Color32::from_rgb(40, 60, 80)),
                    );
                    if resp.clicked() {
                        remove_watch = Some(i);
                    }
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
            if self.persistent_mode {
                self.one_shot_stripped.clear();
                self.reapply_alert.clear();
            }
            self.persist();
            self.push_rpc_state();
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
            self.persist();
            self.push_rpc_state();
        }
        if manual_refresh {
            // Background thread handles refresh; just show feedback
            self.set_status_neutral("Refreshing…");
        }
        if do_stress_test {
            let stress_path = self.exe_dir.join("stress_tester.exe");
            if stress_path.exists() {
                match std::process::Command::new(&stress_path).spawn() {
                    Ok(_) => self.set_status_neutral("Launched stress_tester.exe."),
                    Err(e) => self.set_status(format!("✗ Could not launch stress_tester: {e}"), false),
                }
            } else {
                self.set_status(
                    "✗ stress_tester.exe not found — build with: cargo build --release -p stress_tester",
                    false,
                );
            }
        }
        if toggle_log {
            self.show_log = !self.show_log;
            self.persist();
        }
        if toggle_settings {
            self.show_settings = !self.show_settings;
        }
        // Actions forwarded from the settings window
        if toggle_startup_from_settings {
            let desired = !self.startup_enabled;
            if write_startup_reg(desired) {
                self.startup_enabled = desired;
                let s = if desired { "🚀 Added to Windows startup." } else { "Removed from Windows startup." };
                self.set_status(s, desired);
            } else {
                self.set_status("✗ Could not write startup registry key.", false);
            }
        }
        if toggle_toast_from_settings {
            self.toast_enabled = !self.toast_enabled;
            let s = if self.toast_enabled { "🔔 Toast notifications ON." } else { "🔕 Toast notifications OFF." };
            self.set_status_neutral(s);
            self.persist();
        }
        if toggle_hotkey_from_settings {
            self.hotkey_enabled = !self.hotkey_enabled;
            if self.hotkey_enabled {
                register_hotkey(self.hotkey_id);
                self.set_status_neutral("⌨ Hotkey registered: Ctrl+Shift+B");
            } else {
                unregister_hotkey(self.hotkey_id);
                self.set_status_neutral("⌨ Hotkey unregistered.");
            }
            self.persist();
        }
        if toggle_minimize_to_tray_from_settings {
            self.minimize_to_tray = !self.minimize_to_tray;
            let s = if self.minimize_to_tray {
                "✕ closes to tray."
            } else {
                "✕ exits the app."
            };
            self.set_status_neutral(s);
            self.persist();
        }
        if toggle_logging_from_settings {
            self.logging_enabled = !self.logging_enabled;
            let s = if self.logging_enabled {
                "📋 Injection log file ON."
            } else {
                "📋 Injection log file OFF."
            };
            self.set_status_neutral(s);
            self.persist();
        }
        if open_log_file_from_settings {
            if let Some(path) = App::log_path() {
                // Create the file if it doesn't exist yet so Explorer can open it.
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::File::create(&path);
                }
                let _ = std::process::Command::new("explorer").arg(&path).spawn();
            }
        }
        if toggle_discord_rpc_from_settings {
            self.discord_rpc_enabled = !self.discord_rpc_enabled;
            if self.discord_rpc_enabled {
                self.start_discord_rpc();
                self.set_status_neutral("🎮 Discord Rich Presence ON.");
            } else {
                self.stop_discord_rpc();
                self.set_status_neutral("🎮 Discord Rich Presence OFF.");
            }
            self.persist();
        }
        if add_watch {
            let name = self.watch_input.trim().to_string();
            if !name.is_empty() && !self.watch_names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                self.watch_names.push(name);
                self.watch_input.clear();
                self.persist();
            }
        }
        if let Some(idx) = remove_watch {
            if idx < self.watch_names.len() {
                self.watch_names.remove(idx);
                self.persist();
            }
        }
        if let Some((col, asc)) = new_sort {
            self.sort_col = col;
            self.sort_asc = asc;
            self.persist();
        }
        if do_strip_all {
            if all_windows.iter().any(|w| w.is_protected) {
                // Track stripped PIDs for re-protection detection (one-shot mode only).
                if !self.persistent_mode {
                    let mut seen: HashSet<u32> = HashSet::new();
                    for w in all_windows.iter().filter(|w| w.is_protected) {
                        if seen.insert(w.pid) {
                            self.one_shot_stripped.insert(w.pid, w.process_name.clone());
                            if BROWSER_NAMES.iter().any(|b| w.process_name.eq_ignore_ascii_case(b)) {
                                for child_pid in get_child_pids(w.pid) {
                                    self.one_shot_stripped.insert(
                                        child_pid,
                                        format!("{} (child)", w.process_name),
                                    );
                                }
                            }
                        }
                    }
                }
                self.strip_all_protected(&all_windows);
            } else {
                self.set_status_neutral("No protected windows found — hit Refresh first.");
            }
        }

        // Status bar
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

        // Injection log panel
        if self.show_log {
            egui::TopBottomPanel::bottom("log_panel")
                .resizable(true)
                .min_height(80.0)
                .default_height(150.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Injection Log");
                        ui.add_space(8.0);
                        if ui.small_button("Clear").clicked() {
                            self.log_entries.clear();
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in &self.log_entries {
                                let elapsed = entry.time.elapsed().as_secs();
                                let color = if entry.ok {
                                    Color32::from_rgb(100, 220, 100)
                                } else {
                                    Color32::from_rgb(220, 90, 90)
                                };
                                ui.label(
                                    RichText::new(format!("[{elapsed}s ago]  {}", entry.msg))
                                        .color(color)
                                        .small()
                                        .monospace(),
                                );
                            }
                        });
                });
        }

        // Main table
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
                .column(Column::exact(22.0))   // Icon
                .column(Column::exact(70.0))   // PID
                .column(Column::exact(160.0))  // Process
                .column(Column::exact(40.0))   // Arch
                .column(Column::remainder())   // Title
                .column(Column::exact(115.0))  // Status
                .column(Column::exact(150.0))  // Action
                .header(22.0, |mut header| {
                    header.col(|ui| { ui.strong(""); }); // Icon (no label)
                    let sc = sort_col;
                    let sa = sort_asc;
                    let arrow = |col: SortCol| -> &'static str {
                        if sc == col { if sa { " ▲" } else { " ▼" } } else { "" }
                    };
                    header.col(|ui| {
                        if ui.button(format!("PID{}", arrow(SortCol::Pid))).clicked() {
                            new_sort = Some((SortCol::Pid, if sc == SortCol::Pid { !sa } else { true }));
                        }
                    });
                    header.col(|ui| {
                        if ui.button(format!("Process{}", arrow(SortCol::Process))).clicked() {
                            new_sort = Some((SortCol::Process, if sc == SortCol::Process { !sa } else { true }));
                        }
                    });
                    header.col(|ui| { ui.strong("Arch"); });
                    header.col(|ui| {
                        if ui.button(format!("Window Title{}", arrow(SortCol::Title))).clicked() {
                            new_sort = Some((SortCol::Title, if sc == SortCol::Title { !sa } else { true }));
                        }
                    });
                    header.col(|ui| {
                        if ui.button(format!("Status{}", arrow(SortCol::Status))).clicked() {
                            new_sort = Some((SortCol::Status, if sc == SortCol::Status { !sa } else { true }));
                        }
                    });
                    header.col(|ui| { ui.strong("Action"); });
                })
                .body(|mut body| {
                    for entry in &filtered {
                        let row_h = 26.0;
                        body.row(row_h, |mut row| {
                            // Process icon
                            row.col(|ui| {
                                if let Some(tex) = get_process_icon(
                                    &entry.process_name,
                                    &mut self.icon_cache,
                                    ctx,
                                    &self.exe_dir,
                                ) {
                                    ui.add(egui::Image::new(&*tex).max_size([16.0, 16.0].into()).fit_to_exact_size([16.0, 16.0].into()));
                                }
                            });
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
            // Track in one-shot mode so re-protection can be detected.
            if !self.persistent_mode {
                self.one_shot_stripped.insert(target.pid, target.process_name.clone());
                if BROWSER_NAMES.iter().any(|b| target.process_name.eq_ignore_ascii_case(b)) {
                    for child_pid in get_child_pids(target.pid) {
                        self.one_shot_stripped
                            .insert(child_pid, format!("{} (child)", target.process_name));
                    }
                }
            }
            self.strip_window(&target);
        }

        // Re-protection modal
        // Shown when a one-shot-stripped process has re-applied capture protection.
        if !self.reapply_alert.is_empty() {
            egui::Window::new("⚠️  Protection Re-applied")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        "The following app(s) re-applied capture protection after being stripped:",
                    );
                    ui.add_space(4.0);
                    for (pid, name) in &self.reapply_alert {
                        ui.label(
                            RichText::new(format!("  • {name}  (PID {pid})"))
                                .color(Color32::from_rgb(255, 100, 100))
                                .strong(),
                        );
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(
                        "⚡ One-shot mode injects once and exits — Windows caches the DLL path,\n\
                         so re-injecting with the same file is a no-op.",
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "✅  Fix: switch to 🔁 Persistent mode.\n\
                             The persistent DLL stays loaded and re-strips every 500 ms,\n\
                             fighting back automatically whenever protection is re-applied.",
                        )
                        .strong(),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let btn = egui::Button::new("🔁  Switch to Persistent & Re-strip")
                            .fill(Color32::from_rgb(30, 100, 50));
                        if ui.add(btn).clicked() {
                            switch_and_restrip = true;
                        }
                        ui.add_space(8.0);
                        if ui.button("Dismiss").clicked() {
                            dismiss_reapply = true;
                        }
                    });
                });
        }

        // Apply modal actions
        if dismiss_reapply {
            self.reapply_alert.clear();
        }
        if switch_and_restrip {
            let alerted = std::mem::take(&mut self.reapply_alert);
            self.persistent_mode = true;
            self.one_shot_stripped.clear();
            for (pid, name) in &alerted {
                let is_32bit = all_windows
                    .iter()
                    .find(|w| w.pid == *pid)
                    .map(|w| w.is_32bit)
                    .unwrap_or(false);
                self.inject_pid_async(*pid, name.clone(), is_32bit);
            }
            self.set_status(
                format!(
                    "🔁 Switched to Persistent — re-stripping {} process(es).",
                    alerted.len()
                ),
                true,
            );
        }
    }
}

// Help window

/// Render one body block line-by-line so egui doesn't collapse manual
/// whitespace/indentation into a single wrapped paragraph.
/// Lines that look like shell commands are shown in monospace green.
fn render_help_body(ui: &mut egui::Ui, body: &str) {
    for raw_line in body.split('\n') {
        let line = raw_line.trim_end();
        if line.is_empty() {
            ui.add_space(5.0);
        } else if line.trim_start().starts_with("cargo ")
            || line.trim_start().starts_with("rustup ")
            || line.trim_start().starts_with("target\\")
            || line.trim_start().starts_with("target/")
            || line.trim_start().starts_with("git ")
            || line.trim_start().starts_with("python ")
        {
            ui.label(
                RichText::new(line)
                    .font(egui::FontId::monospace(12.5))
                    .color(Color32::from_rgb(150, 220, 130)),
            );
        } else {
            ui.label(line);
        }
    }
}

fn render_help_window(ctx: &egui::Context, show: &mut bool, section: &mut usize) {
    if !*show {
        return;
    }

    egui::Window::new("📖  Help")
        .open(show)
        .resizable(true)
        .collapsible(false)
        .default_size([720.0, 540.0])
        .min_size([480.0, 340.0])
        .show(ctx, |ui| {
            // Tab bar
            // Wraps automatically on narrow windows — no sidebar, no
            // horizontal_top, so the window never expands sideways.
            ui.horizontal_wrapped(|ui| {
                ui.add_space(2.0);
                for (i, (title, _)) in HELP_SECTIONS.iter().enumerate() {
                    let selected = *section == i;
                    let btn = egui::Button::new(*title)
                        .fill(if selected {
                            Color32::from_rgb(40, 80, 130)
                        } else {
                            Color32::from_rgb(35, 35, 35)
                        });
                    if ui
                        .add(btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        *section = i;
                    }
                }
            });

            ui.separator();

            // Content area — full width, vertically scrollable
            egui::ScrollArea::vertical()
                .id_salt("help_content")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some((tab_title, sub_sections)) = HELP_SECTIONS.get(*section) {
                        ui.add_space(4.0);
                        ui.label(RichText::new(*tab_title).size(18.0).strong());
                        ui.add_space(8.0);

                        for (heading, body) in sub_sections.iter() {
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(*heading)
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::from_rgb(140, 190, 255)),
                            );
                            ui.add_space(6.0);
                            render_help_body(ui, body);
                            ui.add_space(10.0);
                        }
                    }
                });
        });
}

// Settings window

#[allow(clippy::too_many_arguments)]
fn render_settings_window(
    ctx: &egui::Context,
    show: &mut bool,
    startup_enabled: bool,
    toast_enabled: bool,
    hotkey_enabled: bool,
    minimize_to_tray: bool,
    logging_enabled: bool,
    discord_rpc_enabled: bool,
    toggle_startup: &mut bool,
    toggle_toast: &mut bool,
    toggle_hotkey: &mut bool,
    toggle_minimize_to_tray: &mut bool,
    toggle_logging: &mut bool,
    open_log_file: &mut bool,
    toggle_discord_rpc: &mut bool,
) {
    if !*show {
        return;
    }

    egui::Window::new("⚙  Settings")
        .open(show)
        .resizable(false)
        .collapsible(false)
        .default_size([340.0, 420.0])
        .show(ctx, |ui| {
            ui.add_space(4.0);

            // Startup
            ui.label(RichText::new("Startup").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let startup_label = if startup_enabled {
                    "🚀  Start with Windows  (ON)"
                } else {
                    "🚀  Start with Windows  (OFF)"
                };
                let btn = egui::Button::new(startup_label)
                    .fill(if startup_enabled {
                        Color32::from_rgb(50, 90, 25)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui.add(btn)
                    .on_hover_text(
                        "Adds capture-bypass to HKCU\\Run so it launches on login.\n\
                         Windows will show a UAC prompt each time because the app\n\
                         requires Administrator rights.",
                    )
                    .clicked()
                {
                    *toggle_startup = true;
                }
            });
            ui.add_space(12.0);

            // Notifications
            ui.label(RichText::new("Notifications").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let toast_label = if toast_enabled {
                    "🔔  Toast notifications  (ON)"
                } else {
                    "🔕  Toast notifications  (OFF)"
                };
                let btn = egui::Button::new(toast_label)
                    .fill(if toast_enabled {
                        Color32::from_rgb(70, 60, 10)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui.add(btn)
                    .on_hover_text(
                        "Show a Windows desktop notification whenever auto-inject\n\
                         silently strips a process in the background.",
                    )
                    .clicked()
                {
                    *toggle_toast = true;
                }
            });
            ui.add_space(12.0);

            // Hotkey
            ui.label(RichText::new("Hotkey").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let hk_label = if hotkey_enabled {
                    "⌨  Ctrl+Shift+B  (ON)"
                } else {
                    "⌨  Ctrl+Shift+B  (OFF)"
                };
                let btn = egui::Button::new(hk_label)
                    .fill(if hotkey_enabled {
                        Color32::from_rgb(35, 35, 90)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui.add(btn)
                    .on_hover_text(
                        "Register Ctrl+Shift+B as a global hotkey.\n\
                         Pressing it strips all protected windows, even\n\
                         when the app is minimised to the system tray.",
                    )
                    .clicked()
                {
                    *toggle_hotkey = true;
                }
            });
            ui.add_space(6.0);
            if hotkey_enabled {
                ui.label(
                    RichText::new("  Global shortcut active: Ctrl+Shift+B → Strip All Protected")
                        .size(11.0)
                        .color(Color32::from_rgb(130, 130, 200)),
                );
            }
            ui.add_space(12.0);

            // Window
            ui.label(RichText::new("Window").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let tray_label = if minimize_to_tray {
                    "🗕  Minimize to tray on close  (ON)"
                } else {
                    "🗕  Minimize to tray on close  (OFF)"
                };
                let btn = egui::Button::new(tray_label)
                    .fill(if minimize_to_tray {
                        Color32::from_rgb(40, 60, 80)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui
                    .add(btn)
                    .on_hover_text(
                        "ON  — clicking ✕ hides the app to the system tray.\n\
                         OFF — clicking ✕ exits the app completely.\n\
                         The tray icon's Quit option always exits regardless.",
                    )
                    .clicked()
                {
                    *toggle_minimize_to_tray = true;
                }
            });
            ui.add_space(12.0);

            // Logging
            ui.label(RichText::new("Logging").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let log_label = if logging_enabled {
                    "📋  Injection log file  (ON)"
                } else {
                    "📋  Injection log file  (OFF)"
                };
                let btn = egui::Button::new(log_label)
                    .fill(if logging_enabled {
                        Color32::from_rgb(30, 70, 50)
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui
                    .add(btn)
                    .on_hover_text(
                        "Append a timestamped entry to injection.log each time\n\
                         a process is stripped, including mode and result.\n\
                         Log is stored in %APPDATA%\\capture-bypass\\injection.log",
                    )
                    .clicked()
                {
                    *toggle_logging = true;
                }
            });
            if logging_enabled {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if ui
                        .button("📂  Open log file")
                        .on_hover_text("%APPDATA%\\capture-bypass\\injection.log")
                        .clicked()
                    {
                        *open_log_file = true;
                    }
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new("  %APPDATA%\\capture-bypass\\injection.log")
                        .size(10.5)
                        .color(Color32::from_rgb(110, 110, 150)),
                );
            }
            ui.add_space(8.0);

            // Discord Rich Presence
            ui.label(RichText::new("Discord").strong().color(Color32::from_rgb(180, 180, 220)));
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let rpc_label = if discord_rpc_enabled {
                    "🎮  Discord Rich Presence  (ON)"
                } else {
                    "🎮  Discord Rich Presence  (OFF)"
                };
                let btn = egui::Button::new(rpc_label)
                    .fill(if discord_rpc_enabled {
                        Color32::from_rgb(30, 45, 90)   // Discord blurple-ish
                    } else {
                        Color32::from_rgb(50, 50, 50)
                    })
                    .min_size([300.0, 28.0].into());
                if ui
                    .add(btn)
                    .on_hover_text(
                        "Show capture-bypass status in Discord.\n\
                         Displays mode, auto-inject state, and strips\n\
                         this session. Requires Discord to be running.",
                    )
                    .clicked()
                {
                    *toggle_discord_rpc = true;
                }
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new("  Shows mode, auto-inject state, and strip count in Discord.")
                    .size(10.5)
                    .color(Color32::from_rgb(110, 110, 150)),
            );
            ui.add_space(4.0);
        });
}

// Status badge

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

// Tray setup

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

// Window enumeration

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

// Windows helpers

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

// Utilities

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max_chars {
        format!("{}…", chars[..max_chars].iter().collect::<String>())
    } else {
        s.to_string()
    }
}

// Windows startup registry helpers

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

// Global hotkey helpers

fn register_hotkey(id: i32) {
    unsafe {
        let _ = RegisterHotKey(
            None,
            id,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0),
            u32::from('B'),
        );
    }
}

fn unregister_hotkey(id: i32) {
    unsafe {
        let _ = UnregisterHotKey(None, id);
    }
}

// Toast notification helper
// Uses a simple PowerShell one-liner so we don't need the WinRT COM machinery.

fn send_toast(title: &str, body: &str) {
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, \
         ContentType = WindowsRuntime] | Out-Null; \
         $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(\
           [Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $template.GetElementsByTagName('text')[0].AppendChild($template.CreateTextNode('{title}')) | Out-Null; \
         $template.GetElementsByTagName('text')[1].AppendChild($template.CreateTextNode('{body}')) | Out-Null; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('capture-bypass')\
           .Show([Windows.UI.Notifications.ToastNotification]::new($template))",
        title = title.replace('\'', ""),
        body  = body.replace('\'', ""),
    );
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .spawn();
}

// Auto-update check

/// Parse "X.Y.Z" (with or without a leading 'v') into a comparable tuple.
fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.splitn(4, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()
        // strip any pre-release suffix like "-alpha.1"
        .map(|p| p.split('-').next().unwrap_or(p))
        .and_then(|p| p.parse().ok())?;
    Some((major, minor, patch))
}

fn check_for_update() -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let current = env!("CARGO_PKG_VERSION");
    let resp: serde_json::Value = ureq::get(
        "https://api.github.com/repos/Londopy/capture-bypass/releases/latest",
    )
    .set("User-Agent", &format!("capture-bypass/{current}"))
    .call()?
    .into_json()?;

    let raw_tag = resp["tag_name"].as_str().unwrap_or("").trim().to_string();
    if raw_tag.is_empty() {
        return Ok(None);
    }

    // Only show the banner when the remote version is strictly newer.
    // Semver comparison avoids false positives when Cargo.toml and the
    // GitHub tag differ in format (e.g. "1.0.0" vs "v1.0.0").
    let remote_ver = parse_semver(&raw_tag);
    let current_ver = parse_semver(current);
    match (current_ver, remote_ver) {
        (Some(c), Some(r)) if r > c => Ok(Some(raw_tag.trim_start_matches('v').to_string())),
        _ => Ok(None),
    }
}

// Process icon helpers

/// Resolve the full executable path from process name by scanning
/// the window list process names — used to locate the exe for SHGetFileInfoW.
fn find_exe_path(process_name: &str) -> Option<PathBuf> {
    // Walk all running processes and return the first full path that matches the name
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry: windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W =
            std::mem::zeroed();
        entry.dwSize =
            std::mem::size_of::<windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W>()
                as u32;
        if windows::Win32::System::Diagnostics::ToolHelp::Process32FirstW(snap, &mut entry).is_err() {
            return None;
        }
        loop {
            let name_raw: Vec<u16> = entry.szExeFile.iter().copied().take_while(|&c| c != 0).collect();
            let name = String::from_utf16_lossy(&name_raw);
            if name.eq_ignore_ascii_case(process_name) {
                let pid = entry.th32ProcessID;
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut buf = [0u16; 1024];
                    let mut sz = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buf.as_mut_ptr()), &mut sz).is_ok()
                    {
                        return Some(PathBuf::from(String::from_utf16_lossy(&buf[..sz as usize]).as_str()));
                    }
                }
            }
            if windows::Win32::System::Diagnostics::ToolHelp::Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
    }
    None
}

fn load_icon_for_process(
    process_name: &str,
    ctx: &egui::Context,
) -> Option<egui::TextureHandle> {
    let exe_path = find_exe_path(process_name)?;
    let path_wide: Vec<u16> = exe_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut sfi: SHFILEINFOW = std::mem::zeroed();
        let flags = SHGFI_ICON | SHGFI_SMALLICON;
        let res = SHGetFileInfoW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if res == 0 {
            return None;
        }
        let hicon = sfi.hIcon;

        let mut icon_info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return None;
        }

        let hbmp = icon_info.hbmColor;
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;

        let hdc = GetDC(None);
        // First call: populate width/height
        GetDIBits(hdc, hbmp, 0, 0, None, &mut bmi, DIB_RGB_COLORS);
        let w = bmi.bmiHeader.biWidth.unsigned_abs() as usize;
        let h = bmi.bmiHeader.biHeight.unsigned_abs() as usize;

        if w == 0 || h == 0 {
            ReleaseDC(None, hdc);
            return None;
        }

        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;
        bmi.bmiHeader.biHeight = -(h as i32); // top-down DIB

        let mut pixels = vec![0u8; w * h * 4];
        GetDIBits(hdc, hbmp, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);

        // Windows returns BGRA — swap B and R to get RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(hicon);

        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
        Some(ctx.load_texture(
            format!("icon_{process_name}"),
            image,
            egui::TextureOptions::LINEAR,
        ))
    }
}

/// Returns cached icon, loading it on first access.
fn get_process_icon<'a>(
    process_name: &str,
    cache: &'a mut HashMap<String, Option<egui::TextureHandle>>,
    ctx: &egui::Context,
    _exe_dir: &Path,
) -> Option<&'a egui::TextureHandle> {
    cache
        .entry(process_name.to_string())
        .or_insert_with(|| load_icon_for_process(process_name, ctx))
        .as_ref()
}

// Entry point

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
