//! stress_tester — Rust port of test_protection.py
//!
//! A self-protecting window for verifying capture-bypass injection.
//!
//! Features
//! ─────────
//! • Applies WDA_EXCLUDEFROMCAPTURE to itself on launch.
//! • Background thread polls GetWindowDisplayAffinity every 100 ms and
//!   updates the display without any external interaction.
//! • Fight mode: re-applies WDA_EXCLUDEFROMCAPTURE at a configurable
//!   interval (50 – 2000 ms), simulating apps that resist one-shot injection.
//! • Strip / re-apply counters so you can see the fight in real-time.
//! • Window background and title change with the protection state — obvious
//!   in OBS whether the injection succeeded.
//!
//! No Administrator rights required (SetWindowDisplayAffinity works on your
//! own windows without elevation).

// No console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, Frame, RichText};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, TRUE},
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowDisplayAffinity, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    },
};

/// HWND is a raw pointer (*mut c_void) and therefore not Send by default.
/// We store it as isize so the newtype itself is Send — Rust 2021 closure
/// capture precision would otherwise capture the inner HWND field directly,
/// bypassing the unsafe impl Send.
/// SAFETY: We only use the value to reconstruct an HWND for Win32 calls on
/// our own window, which is valid from any thread on Windows.
#[derive(Clone, Copy)]
struct SendHwnd(isize);
unsafe impl Send for SendHwnd {}

// ── WDA constants ─────────────────────────────────────────────────────────────

const WDA_NONE: u32 = 0x00000000;
const WDA_MONITOR: u32 = 0x00000001;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x00000011;

// Sentinel: "not yet read" stored in the AtomicU32 between first init and
// first monitor-thread read.
const AFF_UNKNOWN: u32 = 0xFFFF_FFFF;

// ── Windows helpers ───────────────────────────────────────────────────────────

unsafe fn set_affinity(hwnd: HWND, value: u32) -> bool {
    SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(value)).is_ok()
}

unsafe fn get_affinity(hwnd: HWND) -> u32 {
    let mut aff: u32 = 0;
    if GetWindowDisplayAffinity(hwnd, &mut aff).is_ok() {
        aff
    } else {
        AFF_UNKNOWN
    }
}

/// Find a visible, titled window whose process ID matches `target_pid`.
fn find_hwnd_for_pid(target_pid: u32) -> Option<HWND> {
    struct State {
        target_pid: u32,
        found: Option<HWND>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut State);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        let mut buf = [0u16; 256];
        if GetWindowTextW(hwnd, &mut buf) == 0 {
            return TRUE;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.target_pid && state.found.is_none() {
            state.found = Some(hwnd);
        }
        TRUE
    }

    let mut state = State {
        target_pid,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut state as *mut State as isize),
        );
    }
    state.found
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

    /// Whether fight mode is currently running.
    fight_active: bool,
    /// Stop signal for the fight thread.
    fight_running: Arc<AtomicBool>,
    /// Interval in ms, shared with the fight thread so it picks up slider changes.
    fight_interval_ms: Arc<AtomicU32>,
    /// Slider value shown in the UI (mirrors fight_interval_ms).
    slider_value: u32,

    /// egui context cloned for use in background threads.
    ctx: egui::Context,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        App {
            hwnd: None,
            current_aff: Arc::new(AtomicU32::new(AFF_UNKNOWN)),
            reapply_count: Arc::new(AtomicU32::new(0)),
            strip_count: Arc::new(AtomicU32::new(0)),
            fight_active: false,
            fight_running: Arc::new(AtomicBool::new(false)),
            fight_interval_ms: Arc::new(AtomicU32::new(500)),
            slider_value: 500,
            ctx: cc.egui_ctx.clone(),
        }
    }

    // ── HWND discovery + thread startup ───────────────────────────────────────

    /// Called once on the first frame once the window is visible.
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

                // Detect an external strip: protected → clear
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
                if !running.load(Ordering::Relaxed) {
                    break;
                }
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
}

// ── egui rendering ─────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── One-time initialization (wait for window to exist) ───────────────
        if self.hwnd.is_none() {
            self.initialize();
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        let aff = self.current_aff.load(Ordering::Relaxed);
        let protected = aff == WDA_EXCLUDEFROMCAPTURE || aff == WDA_MONITOR;
        let reapply = self.reapply_count.load(Ordering::Relaxed);
        let strips = self.strip_count.load(Ordering::Relaxed);

        // ── Window title (changes with state — visible in OBS) ───────────────
        let title = if aff == WDA_NONE {
            "✅ NOT PROTECTED — Capture Bypass Stress Tester".to_string()
        } else if aff == WDA_MONITOR {
            "🟡 MONITOR-ONLY — Capture Bypass Stress Tester".to_string()
        } else if aff == WDA_EXCLUDEFROMCAPTURE {
            "🔴 PROTECTED — Capture Bypass Stress Tester".to_string()
        } else {
            "⚪ UNKNOWN — Capture Bypass Stress Tester".to_string()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // ── Background colour: deep red when protected, deep green when clear ──
        let bg_color = if protected {
            Color32::from_rgb(61, 16, 16)   // deep red
        } else {
            Color32::from_rgb(13, 45, 13)   // deep green
        };
        ctx.set_visuals({
            let mut v = egui::Visuals::dark();
            v.panel_fill = bg_color;
            v.window_fill = bg_color;
            v
        });

        // ── UI ───────────────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).fill(bg_color))
            .show(ctx, |ui| {
                ui.add_space(24.0);

                // ── Big status label ─────────────────────────────────────────
                let (status_text, status_color) = match aff {
                    WDA_NONE => (
                        "✅  NOT PROTECTED  (WDA_NONE)",
                        Color32::from_rgb(102, 221, 102),
                    ),
                    WDA_MONITOR => (
                        "🟡  MONITOR-ONLY  (WDA_MONITOR)",
                        Color32::from_rgb(255, 200, 50),
                    ),
                    WDA_EXCLUDEFROMCAPTURE => (
                        "🔴  PROTECTED  (WDA_EXCLUDEFROMCAPTURE)",
                        Color32::from_rgb(255, 90, 90),
                    ),
                    _ => ("⚪  UNKNOWN", Color32::GRAY),
                };
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(status_text)
                            .size(22.0)
                            .strong()
                            .color(status_color),
                    );
                    ui.add_space(6.0);

                    // Sub-label: HWND + fight state
                    let hwnd_str = self
                        .hwnd
                        .map(|sh| format!("HWND: 0x{:08X}", sh.0 as usize))
                        .unwrap_or_else(|| "HWND: (discovering…)".into());
                    let fight_str = if self.fight_active {
                        format!("  |  Fight: ON @ {} ms", self.slider_value)
                    } else {
                        "  |  Fight: OFF".into()
                    };
                    ui.label(
                        RichText::new(format!("{hwnd_str}{fight_str}"))
                            .size(13.0)
                            .color(Color32::from_rgb(170, 170, 170)),
                    );
                });

                ui.add_space(14.0);

                // ── Counters ─────────────────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        // Centre the pair manually with spacing
                        ui.add_space(ui.available_width() / 2.0 - 180.0);
                        ui.label(
                            RichText::new(format!("Fight re-applies: {reapply}"))
                                .size(13.0)
                                .color(Color32::from_rgb(255, 136, 136)),
                        );
                        ui.add_space(40.0);
                        ui.label(
                            RichText::new(format!("External strips: {strips}"))
                                .size(13.0)
                                .color(Color32::from_rgb(136, 255, 136)),
                        );
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // ── Manual protection buttons ─────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 215.0);

                        let apply_btn = egui::Button::new(
                            RichText::new("🔴  Apply Protection").size(14.0),
                        )
                        .fill(Color32::from_rgb(139, 0, 0))
                        .min_size([200.0, 34.0].into());

                        if ui.add(apply_btn).clicked() {
                            if let Some(sh) = self.hwnd {
                                unsafe { set_affinity(HWND(sh.0 as *mut _), WDA_EXCLUDEFROMCAPTURE); }
                            }
                        }

                        ui.add_space(16.0);

                        let remove_btn = egui::Button::new(
                            RichText::new("✅  Remove Protection").size(14.0),
                        )
                        .fill(Color32::from_rgb(20, 90, 20))
                        .min_size([200.0, 34.0].into());

                        if ui.add(remove_btn).clicked() {
                            if let Some(sh) = self.hwnd {
                                unsafe { set_affinity(HWND(sh.0 as *mut _), WDA_NONE); }
                            }
                        }
                    });
                });

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(14.0);

                // ── Fight mode section ────────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("⚔  Fight Mode").size(15.0).strong());
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Re-applies WDA_EXCLUDEFROMCAPTURE at the chosen interval.\n\
                             Simulates apps that resist one-shot injection.",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(170, 170, 170)),
                    );
                    ui.add_space(12.0);

                    // Interval slider
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 200.0);
                        ui.label(RichText::new("Re-apply every:").size(13.0));
                        ui.add_space(8.0);

                        let slider = egui::Slider::new(&mut self.slider_value, 50..=2000)
                            .step_by(50.0)
                            .suffix(" ms")
                            .integer();
                        if ui.add(slider).changed() {
                            self.fight_interval_ms
                                .store(self.slider_value, Ordering::Relaxed);
                        }
                    });

                    ui.add_space(12.0);

                    // Fight toggle + reset
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() / 2.0 - 190.0);

                        let (fight_label, fight_color) = if self.fight_active {
                            (
                                "⏹  Stop Fight Mode",
                                Color32::from_rgb(176, 58, 46),
                            )
                        } else {
                            (
                                "▶  Start Fight Mode",
                                Color32::from_rgb(125, 60, 152),
                            )
                        };

                        let fight_btn = egui::Button::new(
                            RichText::new(fight_label).size(14.0),
                        )
                        .fill(fight_color)
                        .min_size([200.0, 34.0].into());

                        if ui.add(fight_btn).clicked() {
                            if self.fight_active {
                                self.stop_fight();
                            } else {
                                self.start_fight();
                            }
                        }

                        ui.add_space(16.0);

                        let reset_btn = egui::Button::new(
                            RichText::new("↺  Reset Counters").size(14.0),
                        )
                        .fill(Color32::from_rgb(80, 80, 80))
                        .min_size([160.0, 34.0].into());

                        if ui.add(reset_btn).clicked() {
                            self.reapply_count.store(0, Ordering::Relaxed);
                            self.strip_count.store(0, Ordering::Relaxed);
                        }
                    });
                });
            });
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("🔴 PROTECTED — Capture Bypass Stress Tester")
            .with_inner_size([620.0, 520.0])
            .with_min_inner_size([540.0, 460.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Capture Bypass Stress Tester",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
