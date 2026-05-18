//! stress_tester — self-protecting test window for capture-bypass.
//!
//! A standalone egui app for verifying capture-bypass injection.
//!
//! Features
//! ─────────
//! • Applies WDA_EXCLUDEFROMCAPTURE to itself on launch.
//! • Background thread polls GetWindowDisplayAffinity every 100 ms and
//!   updates the display without any external interaction.
//!
//! Fight mode
//! ──────────
//! Re-applies WDA_EXCLUDEFROMCAPTURE at a configurable interval (50–2000 ms),
//! simulating apps that resist one-shot injection.
//!
//! Scenario A — Process enumeration (TH32CS_SNAPPROCESS)
//! ────────────────────────────────────────────────────────
//! Polls the process list for a configurable injector exe name.
//! When the injector is found running, re-applies WDA_EXCLUDEFROMCAPTURE.
//! Tests whether stealth renaming the injector (Scenario A defence) works.
//!
//! Scenario B — Module ejection (TH32CS_SNAPMODULE)
//! ────────────────────────────────────────────────────
//! Polls the process's own loaded module list every 250 ms.
//! Any module whose name contains the configured pattern (default "payload_dll")
//! or, optionally, any anonymous .tmp module, gets FreeLibrary'd immediately
//! and protection is re-applied.
//! Tests whether the stealth temp-copy approach defeats name-based DLL scanning.
//!
//! No Administrator rights required (SetWindowDisplayAffinity works on your
//! own windows without elevation).

// No console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, Frame, RichText};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use windows::Win32::{
    Foundation::{BOOL, HMODULE, HWND, LPARAM, TRUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW,
            Process32NextW, MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE,
            TH32CS_SNAPPROCESS,
        },
        LibraryLoader::FreeLibrary,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowDisplayAffinity, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    },
};

/// HWND is a raw pointer — wrap it so it's Send across threads.
/// SAFETY: only used on our own window from a single Win32 call per thread.
#[derive(Clone, Copy)]
struct SendHwnd(isize);
unsafe impl Send for SendHwnd {}

// ── WDA constants ─────────────────────────────────────────────────────────────

const WDA_NONE: u32 = 0x00000000;
const WDA_MONITOR: u32 = 0x00000001;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x00000011;
const AFF_UNKNOWN: u32 = 0xFFFF_FFFF;

// ── Windows helpers ───────────────────────────────────────────────────────────

unsafe fn set_affinity(hwnd: HWND, value: u32) -> bool {
    SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(value)).is_ok()
}

unsafe fn get_affinity(hwnd: HWND) -> u32 {
    let mut aff: u32 = 0;
    if GetWindowDisplayAffinity(hwnd, &mut aff).is_ok() { aff } else { AFF_UNKNOWN }
}

fn find_hwnd_for_pid(target_pid: u32) -> Option<HWND> {
    struct State { target_pid: u32, found: Option<HWND> }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut State);
        if !IsWindowVisible(hwnd).as_bool() { return TRUE; }
        let mut buf = [0u16; 256];
        if GetWindowTextW(hwnd, &mut buf) == 0 { return TRUE; }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.target_pid && state.found.is_none() {
            state.found = Some(hwnd);
        }
        TRUE
    }

    let mut state = State { target_pid, found: None };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut state as *mut State as isize));
    }
    state.found
}

/// Returns true if a process with `name` (case-insensitive) is currently running.
fn process_is_running(name: &str) -> bool {
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry).is_err() {
            return false;
        }
        loop {
            let exe: Vec<u16> = entry.szExeFile.iter().copied().take_while(|&c| c != 0).collect();
            if String::from_utf16_lossy(&exe).eq_ignore_ascii_case(name) {
                return true;
            }
            if Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
        false
    }
}

/// Scans the module list of `pid` and calls FreeLibrary on every module whose
/// name matches `pattern` (case-insensitive substring) or, if `also_tmp` is
/// true, on every anonymous `.tmp` module.
/// Returns the number of modules ejected.
fn eject_matching_modules(pid: u32, pattern: &str, also_tmp: bool) -> u32 {
    let mut ejected = 0u32;
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let mut entry: MODULEENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(snap, &mut entry).is_err() {
            return 0;
        }
        loop {
            let mod_name: Vec<u16> = entry.szModule.iter().copied().take_while(|&c| c != 0).collect();
            let name = String::from_utf16_lossy(&mod_name).to_lowercase();
            let hit = (!pattern.is_empty() && name.contains(&pattern.to_lowercase()))
                || (also_tmp && name.ends_with(".tmp"));
            if hit {
                let hmod = HMODULE(entry.hModule as *mut _);
                if FreeLibrary(hmod).is_ok() {
                    ejected += 1;
                }
            }
            if Module32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
    }
    ejected
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    /// Our own HWND, discovered on the first frame.
    hwnd: Option<SendHwnd>,

    /// Current display affinity (written by monitor thread, read by UI).
    current_aff: Arc<AtomicU32>,

    /// How many times fight mode re-applied protection.
    reapply_count: Arc<AtomicU32>,
    /// How many times an external tool cleared us.
    strip_count: Arc<AtomicU32>,

    // ── Fight mode ────────────────────────────────────────────────────────────
    fight_active: bool,
    fight_running: Arc<AtomicBool>,
    fight_interval_ms: Arc<AtomicU32>,
    slider_value: u32,

    // ── Scenario A — process enumeration ─────────────────────────────────────
    /// Whether the Scenario A detection thread is running.
    scenario_a_active: bool,
    scenario_a_running: Arc<AtomicBool>,
    /// How many times the injector process was detected.
    scenario_a_detections: Arc<AtomicU32>,
    /// Editable injector exe name shown in the UI.
    scenario_a_input: String,
    /// Live copy shared with the background thread.
    scenario_a_name: Arc<Mutex<String>>,

    // ── Scenario B — module ejection ─────────────────────────────────────────
    /// Whether the Scenario B scan thread is running.
    scenario_b_active: bool,
    scenario_b_running: Arc<AtomicBool>,
    /// How many modules have been FreeLibrary'd.
    scenario_b_ejections: Arc<AtomicU32>,
    /// DLL name substring to match (default "payload_dll").
    scenario_b_pattern: String,
    scenario_b_pattern_shared: Arc<Mutex<String>>,
    /// Also eject any .tmp module (catches stealth-renamed DLLs).
    scenario_b_scan_tmp: bool,
    scenario_b_scan_tmp_shared: Arc<AtomicBool>,

    /// egui context for background threads.
    ctx: egui::Context,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let default_pattern = "payload_dll".to_string();
        App {
            hwnd: None,
            current_aff: Arc::new(AtomicU32::new(AFF_UNKNOWN)),
            reapply_count: Arc::new(AtomicU32::new(0)),
            strip_count: Arc::new(AtomicU32::new(0)),
            fight_active: false,
            fight_running: Arc::new(AtomicBool::new(false)),
            fight_interval_ms: Arc::new(AtomicU32::new(500)),
            slider_value: 500,
            scenario_a_active: false,
            scenario_a_running: Arc::new(AtomicBool::new(false)),
            scenario_a_detections: Arc::new(AtomicU32::new(0)),
            scenario_a_input: "capture_bypass_gui.exe".to_string(),
            scenario_a_name: Arc::new(Mutex::new("capture_bypass_gui.exe".to_string())),
            scenario_b_active: false,
            scenario_b_running: Arc::new(AtomicBool::new(false)),
            scenario_b_ejections: Arc::new(AtomicU32::new(0)),
            scenario_b_pattern: default_pattern.clone(),
            scenario_b_pattern_shared: Arc::new(Mutex::new(default_pattern)),
            scenario_b_scan_tmp: false,
            scenario_b_scan_tmp_shared: Arc::new(AtomicBool::new(false)),
            ctx: cc.egui_ctx.clone(),
        }
    }

    // ── One-time init ──────────────────────────────────────────────────────────

    fn initialize(&mut self) {
        let our_pid = std::process::id();
        if let Some(hwnd) = find_hwnd_for_pid(our_pid) {
            let sh = SendHwnd(hwnd.0 as isize);
            self.hwnd = Some(sh);
            unsafe { set_affinity(hwnd, WDA_EXCLUDEFROMCAPTURE); }
            self.current_aff.store(WDA_EXCLUDEFROMCAPTURE, Ordering::Relaxed);
            self.start_monitor_thread(sh);
        }
    }

    fn start_monitor_thread(&self, sh: SendHwnd) {
        let aff_shared = Arc::clone(&self.current_aff);
        let strip_count = Arc::clone(&self.strip_count);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let hwnd = HWND(sh.0 as *mut _);
            let mut last_aff = WDA_EXCLUDEFROMCAPTURE;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let aff = unsafe { get_affinity(hwnd) };
                aff_shared.store(aff, Ordering::Relaxed);
                if (last_aff == WDA_EXCLUDEFROMCAPTURE || last_aff == WDA_MONITOR)
                    && aff == WDA_NONE
                {
                    strip_count.fetch_add(1, Ordering::Relaxed);
                }
                last_aff = aff;
                ctx.request_repaint();
            }
        });
    }

    // ── Fight mode ────────────────────────────────────────────────────────────

    fn start_fight(&mut self) {
        let Some(sh) = self.hwnd else { return };
        self.fight_active = true;
        self.fight_running.store(true, Ordering::Relaxed);
        let running = Arc::clone(&self.fight_running);
        let interval = Arc::clone(&self.fight_interval_ms);
        let reapply_count = Arc::clone(&self.reapply_count);
        std::thread::spawn(move || {
            let hwnd = HWND(sh.0 as *mut _);
            while running.load(Ordering::Relaxed) {
                let ms = interval.load(Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(ms as u64));
                if !running.load(Ordering::Relaxed) { break; }
                let aff = unsafe { get_affinity(hwnd) };
                if aff != WDA_EXCLUDEFROMCAPTURE {
                    unsafe { set_affinity(hwnd, WDA_EXCLUDEFROMCAPTURE); }
                    reapply_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }

    fn stop_fight(&mut self) {
        self.fight_active = false;
        self.fight_running.store(false, Ordering::Relaxed);
    }

    // ── Scenario A — process scan ─────────────────────────────────────────────

    fn start_scenario_a(&mut self) {
        let Some(sh) = self.hwnd else { return };
        self.scenario_a_active = true;
        self.scenario_a_running.store(true, Ordering::Relaxed);
        // Push the current UI input into the shared name
        *self.scenario_a_name.lock().unwrap() = self.scenario_a_input.clone();

        let running = Arc::clone(&self.scenario_a_running);
        let name = Arc::clone(&self.scenario_a_name);
        let detections = Arc::clone(&self.scenario_a_detections);
        let ctx = self.ctx.clone();

        std::thread::spawn(move || {
            let hwnd = HWND(sh.0 as *mut _);
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if !running.load(Ordering::Relaxed) { break; }
                let n = name.lock().unwrap().clone();
                if process_is_running(&n) {
                    unsafe { set_affinity(hwnd, WDA_EXCLUDEFROMCAPTURE); }
                    detections.fetch_add(1, Ordering::Relaxed);
                }
                ctx.request_repaint();
            }
        });
    }

    fn stop_scenario_a(&mut self) {
        self.scenario_a_active = false;
        self.scenario_a_running.store(false, Ordering::Relaxed);
    }

    // ── Scenario B — module ejection ─────────────────────────────────────────

    fn start_scenario_b(&mut self) {
        let Some(sh) = self.hwnd else { return };
        self.scenario_b_active = true;
        self.scenario_b_running.store(true, Ordering::Relaxed);
        *self.scenario_b_pattern_shared.lock().unwrap() = self.scenario_b_pattern.clone();
        self.scenario_b_scan_tmp_shared.store(self.scenario_b_scan_tmp, Ordering::Relaxed);

        let running = Arc::clone(&self.scenario_b_running);
        let pattern = Arc::clone(&self.scenario_b_pattern_shared);
        let scan_tmp = Arc::clone(&self.scenario_b_scan_tmp_shared);
        let ejections = Arc::clone(&self.scenario_b_ejections);
        let ctx = self.ctx.clone();
        let own_pid = std::process::id();

        std::thread::spawn(move || {
            let hwnd = HWND(sh.0 as *mut _);
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if !running.load(Ordering::Relaxed) { break; }
                let pat = pattern.lock().unwrap().clone();
                let tmp = scan_tmp.load(Ordering::Relaxed);
                let n = eject_matching_modules(own_pid, &pat, tmp);
                if n > 0 {
                    unsafe { set_affinity(hwnd, WDA_EXCLUDEFROMCAPTURE); }
                    ejections.fetch_add(n, Ordering::Relaxed);
                }
                ctx.request_repaint();
            }
        });
    }

    fn stop_scenario_b(&mut self) {
        self.scenario_b_active = false;
        self.scenario_b_running.store(false, Ordering::Relaxed);
    }
}

// ── egui rendering ─────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // One-time init once the window handle is available
        if self.hwnd.is_none() {
            self.initialize();
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        let aff = self.current_aff.load(Ordering::Relaxed);
        let protected = aff == WDA_EXCLUDEFROMCAPTURE || aff == WDA_MONITOR;
        let reapply = self.reapply_count.load(Ordering::Relaxed);
        let strips = self.strip_count.load(Ordering::Relaxed);

        // Window title updates with state (visible in OBS window capture)
        let title = match aff {
            WDA_NONE               => "✅ NOT PROTECTED — Capture Bypass Stress Tester",
            WDA_MONITOR            => "🟡 MONITOR-ONLY — Capture Bypass Stress Tester",
            WDA_EXCLUDEFROMCAPTURE => "🔴 PROTECTED — Capture Bypass Stress Tester",
            _                      => "⚪ UNKNOWN — Capture Bypass Stress Tester",
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));

        let bg_color = if protected {
            Color32::from_rgb(61, 16, 16)  // deep red
        } else {
            Color32::from_rgb(13, 45, 13)  // deep green
        };
        ctx.set_visuals({
            let mut v = egui::Visuals::dark();
            v.panel_fill = bg_color;
            v.window_fill = bg_color;
            v
        });

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).fill(bg_color))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {

                // ── Status ───────────────────────────────────────────────────
                ui.add_space(16.0);
                let (status_text, status_color) = match aff {
                    WDA_NONE               => ("✅  NOT PROTECTED  (WDA_NONE)",              Color32::from_rgb(102, 221, 102)),
                    WDA_MONITOR            => ("🟡  MONITOR-ONLY  (WDA_MONITOR)",            Color32::from_rgb(255, 200, 50)),
                    WDA_EXCLUDEFROMCAPTURE => ("🔴  PROTECTED  (WDA_EXCLUDEFROMCAPTURE)",    Color32::from_rgb(255, 90, 90)),
                    _                      => ("⚪  UNKNOWN",                                Color32::GRAY),
                };
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(status_text).size(22.0).strong().color(status_color));
                    ui.add_space(4.0);
                    let hwnd_str = self.hwnd
                        .map(|sh| format!("HWND: 0x{:08X}", sh.0 as usize))
                        .unwrap_or_else(|| "HWND: (discovering…)".into());
                    let fight_str = if self.fight_active {
                        format!("  |  Fight: ON @ {} ms", self.slider_value)
                    } else {
                        "  |  Fight: OFF".into()
                    };
                    ui.label(RichText::new(format!("{hwnd_str}{fight_str}")).size(13.0).color(Color32::from_rgb(170, 170, 170)));
                });

                ui.add_space(10.0);

                // ── Counters ─────────────────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 180.0);
                        ui.label(RichText::new(format!("Fight re-applies: {reapply}")).size(13.0).color(Color32::from_rgb(255, 136, 136)));
                        ui.add_space(40.0);
                        ui.label(RichText::new(format!("External strips: {strips}")).size(13.0).color(Color32::from_rgb(136, 255, 136)));
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(10.0);

                // ── Manual buttons ───────────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 215.0);
                        let apply_btn = egui::Button::new(RichText::new("🔴  Apply Protection").size(14.0))
                            .fill(Color32::from_rgb(139, 0, 0)).min_size([200.0, 34.0].into());
                        if ui.add(apply_btn).clicked() {
                            if let Some(sh) = self.hwnd {
                                unsafe { set_affinity(HWND(sh.0 as *mut _), WDA_EXCLUDEFROMCAPTURE); }
                            }
                        }
                        ui.add_space(16.0);
                        let remove_btn = egui::Button::new(RichText::new("✅  Remove Protection").size(14.0))
                            .fill(Color32::from_rgb(20, 90, 20)).min_size([200.0, 34.0].into());
                        if ui.add(remove_btn).clicked() {
                            if let Some(sh) = self.hwnd {
                                unsafe { set_affinity(HWND(sh.0 as *mut _), WDA_NONE); }
                            }
                        }
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // ── Fight mode ───────────────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("⚔  Fight Mode").size(15.0).strong());
                    ui.add_space(4.0);
                    ui.label(RichText::new("Re-applies WDA_EXCLUDEFROMCAPTURE at the chosen interval.\nSimulates apps that resist one-shot injection.")
                        .size(12.0).color(Color32::from_rgb(170, 170, 170)));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 200.0);
                        ui.label(RichText::new("Re-apply every:").size(13.0));
                        ui.add_space(8.0);
                        let slider = egui::Slider::new(&mut self.slider_value, 50..=2000)
                            .step_by(50.0).suffix(" ms").integer();
                        if ui.add(slider).changed() {
                            self.fight_interval_ms.store(self.slider_value, Ordering::Relaxed);
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 190.0);
                        let (label, color) = if self.fight_active {
                            ("⏹  Stop Fight Mode",  Color32::from_rgb(176, 58, 46))
                        } else {
                            ("▶  Start Fight Mode", Color32::from_rgb(125, 60, 152))
                        };
                        let btn = egui::Button::new(RichText::new(label).size(14.0))
                            .fill(color).min_size([200.0, 34.0].into());
                        if ui.add(btn).clicked() {
                            if self.fight_active { self.stop_fight(); } else { self.start_fight(); }
                        }
                        ui.add_space(16.0);
                        let reset_btn = egui::Button::new(RichText::new("↺  Reset Counters").size(14.0))
                            .fill(Color32::from_rgb(80, 80, 80)).min_size([160.0, 34.0].into());
                        if ui.add(reset_btn).clicked() {
                            self.reapply_count.store(0, Ordering::Relaxed);
                            self.strip_count.store(0, Ordering::Relaxed);
                        }
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // ── Scenario A — process enumeration ─────────────────────────
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("🔍  Scenario A — Process Enumeration  (TH32CS_SNAPPROCESS)").size(15.0).strong());
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Polls the running-process list every 250 ms.\n\
                         When the injector exe is found, re-applies WDA_EXCLUDEFROMCAPTURE.\n\
                         \n\
                         What this tests:  The real injector renames itself to a random exe to evade this \
                         (Scenario A defence).  Leave the default name and run capture_bypass_gui.exe — \
                         this tester will detect it.  Then test whether renaming the injector breaks detection."
                    ).size(12.0).color(Color32::from_rgb(200, 200, 160))
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Injector name:");
                    ui.add_space(4.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.scenario_a_input)
                            .desired_width(220.0)
                            .hint_text("capture_bypass_gui.exe"),
                    );
                    if resp.changed() && self.scenario_a_active {
                        *self.scenario_a_name.lock().unwrap() = self.scenario_a_input.clone();
                    }
                    ui.add_space(12.0);
                    let detections = self.scenario_a_detections.load(Ordering::Relaxed);
                    ui.label(
                        RichText::new(format!("Detections: {detections}"))
                            .size(13.0)
                            .color(if detections > 0 { Color32::from_rgb(255, 100, 100) } else { Color32::GRAY }),
                    );
                    ui.add_space(12.0);
                    if ui.small_button("↺").on_hover_text("Reset counter").clicked() {
                        self.scenario_a_detections.store(0, Ordering::Relaxed);
                    }
                });

                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 130.0);
                        let (label, color) = if self.scenario_a_active {
                            ("⏹  Stop Scenario A", Color32::from_rgb(176, 58, 46))
                        } else {
                            ("▶  Start Scenario A", Color32::from_rgb(30, 90, 140))
                        };
                        let btn = egui::Button::new(RichText::new(label).size(14.0))
                            .fill(color).min_size([220.0, 32.0].into());
                        if ui.add(btn).clicked() {
                            if self.scenario_a_active { self.stop_scenario_a(); } else { self.start_scenario_a(); }
                        }
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // ── Scenario B — module ejection ─────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("💉  Scenario B — Module Ejection  (TH32CS_SNAPMODULE)").size(15.0).strong());
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Polls this process's own loaded module list every 250 ms.\n\
                         Any module whose name matches the pattern gets FreeLibrary'd immediately\n\
                         and WDA_EXCLUDEFROMCAPTURE is re-applied.\n\
                         \n\
                         What this tests:  Non-stealth injection loads payload_dll.dll — easily caught by name.\n\
                         Stealth injection loads a random .tmp — tick 'Also eject .tmp modules' to test whether \
                         the tester can evict the stealth copy too."
                    ).size(12.0).color(Color32::from_rgb(200, 200, 160))
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("DLL name pattern:");
                    ui.add_space(4.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.scenario_b_pattern)
                            .desired_width(200.0)
                            .hint_text("payload_dll"),
                    );
                    if resp.changed() && self.scenario_b_active {
                        *self.scenario_b_pattern_shared.lock().unwrap() = self.scenario_b_pattern.clone();
                    }
                    ui.add_space(16.0);
                    let tmp_resp = ui.checkbox(&mut self.scenario_b_scan_tmp, "Also eject .tmp modules")
                        .on_hover_text(
                            "Calls FreeLibrary on every .tmp module loaded in this process.\n\
                             Stealth injection uses random .tmp names — this tests whether\n\
                             a paranoid app can still evict the DLL despite the name change."
                        );
                    if tmp_resp.changed() && self.scenario_b_active {
                        self.scenario_b_scan_tmp_shared.store(self.scenario_b_scan_tmp, Ordering::Relaxed);
                    }
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ejections = self.scenario_b_ejections.load(Ordering::Relaxed);
                    ui.label(
                        RichText::new(format!("Modules ejected: {ejections}"))
                            .size(13.0)
                            .color(if ejections > 0 { Color32::from_rgb(255, 100, 100) } else { Color32::GRAY }),
                    );
                    ui.add_space(12.0);
                    if ui.small_button("↺").on_hover_text("Reset counter").clicked() {
                        self.scenario_b_ejections.store(0, Ordering::Relaxed);
                    }
                });

                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 130.0);
                        let (label, color) = if self.scenario_b_active {
                            ("⏹  Stop Scenario B", Color32::from_rgb(176, 58, 46))
                        } else {
                            ("▶  Start Scenario B", Color32::from_rgb(30, 90, 140))
                        };
                        let btn = egui::Button::new(RichText::new(label).size(14.0))
                            .fill(color).min_size([220.0, 32.0].into());
                        if ui.add(btn).clicked() {
                            if self.scenario_b_active { self.stop_scenario_b(); } else { self.start_scenario_b(); }
                        }
                    });
                });

                ui.add_space(16.0);

                // ── Tip box ───────────────────────────────────────────────────
                egui::Frame::none()
                    .fill(Color32::from_rgb(30, 30, 50))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("💡  Quick test guide").strong());
                        ui.add_space(4.0);
                        ui.label(RichText::new(
                            "1. Start Scenario A with the default name → launch capture_bypass_gui.exe → \
                               detections should climb while protection stays on.\n\
                               Then test if stealth injection (gui with renamed exe) bypasses it — \
                               detections should stay at 0.\n\
                             \n\
                             2. Start Scenario B (pattern: payload_dll) → inject the non-stealth DLL → \
                               ejections should climb immediately.\n\
                               Then re-inject using stealth mode → ejections should stay at 0 (name is now a .tmp).\n\
                               Tick 'Also eject .tmp modules' and re-inject stealth → ejections climb again, \
                               confirming the tester can still catch .tmp DLLs if it tries hard enough."
                        ).size(11.0).color(Color32::from_rgb(180, 180, 210)));
                    });

                ui.add_space(12.0);

                }); // ScrollArea
            });
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("🔴 PROTECTED — Capture Bypass Stress Tester")
            .with_inner_size([680.0, 780.0])
            .with_min_inner_size([580.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Capture Bypass Stress Tester",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
