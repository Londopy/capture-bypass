//! capture_bypass — GUI tool for stripping Windows WDA capture protection.
//!
//! Shows every visible, titled top-level window with its PID, process name,
//! and title.  Clicking "Strip Protection" injects payload_dll.dll into that
//! process; the DLL calls SetWindowDisplayAffinity(hwnd, WDA_NONE) for every
//! window owned by the process, making them capturable by OBS, Snipping Tool,
//! etc.
//!
//! Requires Administrator privileges (enforced by the embedded UAC manifest
//! compiled in via build.rs / winres).

// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, ScrollArea, Ui};
use std::path::PathBuf;

// Windows API imports used at module level (needed by the module-level callback).
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, TRUE},
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    },
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Capture Bypass")
            .with_inner_size([880.0, 560.0])
            .with_min_inner_size([600.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Capture Bypass",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct WindowEntry {
    hwnd: isize,
    pid: u32,
    process_name: String,
    title: String,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    windows: Vec<WindowEntry>,
    dll_path: PathBuf,
    status: Status,
    filter: String,
}

#[derive(Default)]
enum Status {
    #[default]
    Idle,
    Ok(String),
    Err(String),
}

impl App {
    fn new() -> Self {
        // payload_dll.dll lives next to the executable.
        let dll_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("payload_dll.dll")))
            .unwrap_or_else(|| PathBuf::from("payload_dll.dll"));

        let mut app = Self {
            windows: Vec::new(),
            dll_path,
            status: Status::Idle,
            filter: String::new(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.windows = enumerate_windows();
        self.status = Status::Ok(format!("Found {} visible windows.", self.windows.len()));
    }

    fn inject(&mut self, entry: &WindowEntry) {
        if !self.dll_path.exists() {
            self.status = Status::Err(format!(
                "payload_dll.dll not found at: {}",
                self.dll_path.display()
            ));
            return;
        }

        match injector_core::inject_dll(entry.pid, &self.dll_path) {
            Ok(()) => {
                self.status = Status::Ok(format!(
                    "✓  Injected into PID {} ({}) — protection stripped.",
                    entry.pid, entry.process_name
                ));
            }
            Err(e) => {
                self.status = Status::Err(format!(
                    "✗  PID {} ({}): {}",
                    entry.pid,
                    entry.process_name,
                    e.message()
                ));
            }
        }
    }
}

// ── egui rendering ─────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("🔓  Capture Bypass");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⟳  Refresh").clicked() {
                        self.refresh();
                    }
                });
            });
            ui.label(
                RichText::new(
                    "Click \"Strip Protection\" on any row to remove WDA capture protection \
                     from that process.",
                )
                .weak(),
            );
            ui.add_space(4.0);
        });

        // ── Status bar ───────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            match &self.status {
                Status::Idle => {
                    ui.label(RichText::new("Ready.").weak());
                }
                Status::Ok(msg) => {
                    ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN));
                }
                Status::Err(msg) => {
                    ui.label(RichText::new(msg).color(Color32::LIGHT_RED));
                }
            }
            let dll_ok = self.dll_path.exists();
            ui.label(
                RichText::new(format!(
                    "DLL: {}  {}",
                    self.dll_path.display(),
                    if dll_ok { "✓" } else { "✗ NOT FOUND" }
                ))
                .weak()
                .color(if dll_ok { Color32::GRAY } else { Color32::RED }),
            );
            ui.add_space(4.0);
        });

        // ── Main panel ───────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // Filter bar
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.text_edit_singleline(&mut self.filter);
                if ui.small_button("✕").clicked() {
                    self.filter.clear();
                }
            });
            ui.separator();

            // Column headers
            ui.horizontal(|ui| {
                ui.add_sized([60.0, 18.0], egui::Label::new(RichText::new("PID").strong().monospace()));
                ui.add_sized([160.0, 18.0], egui::Label::new(RichText::new("Process").strong()));
                ui.add_sized([340.0, 18.0], egui::Label::new(RichText::new("Window Title").strong()));
                ui.label(RichText::new("Action").strong());
            });
            ui.separator();

            let filter_lc = self.filter.to_lowercase();
            let entries: Vec<WindowEntry> = self
                .windows
                .iter()
                .filter(|w| {
                    filter_lc.is_empty()
                        || w.title.to_lowercase().contains(&filter_lc)
                        || w.process_name.to_lowercase().contains(&filter_lc)
                        || w.pid.to_string().contains(&filter_lc)
                })
                .cloned()
                .collect();

            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut inject_target: Option<WindowEntry> = None;

                for entry in &entries {
                    ui.horizontal(|ui| {
                        render_row(ui, entry, &mut inject_target);
                    });
                    ui.separator();
                }

                if let Some(target) = inject_target {
                    self.inject(&target);
                }

                if entries.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("No matching windows.").weak().italics());
                    });
                }
            });
        });
    }
}

fn render_row(ui: &mut Ui, entry: &WindowEntry, inject_target: &mut Option<WindowEntry>) {
    ui.add_sized(
        [60.0, 18.0],
        egui::Label::new(RichText::new(entry.pid.to_string()).monospace()),
    );
    ui.add_sized(
        [160.0, 18.0],
        egui::Label::new(RichText::new(&entry.process_name).color(Color32::from_rgb(130, 180, 255))),
    );
    let title_display = if entry.title.chars().count() > 58 {
        format!("{}…", entry.title.chars().take(58).collect::<String>())
    } else {
        entry.title.clone()
    };
    ui.add_sized([340.0, 18.0], egui::Label::new(&title_display));

    if ui
        .add(egui::Button::new("Strip Protection").min_size([140.0, 22.0].into()))
        .on_hover_text(format!(
            "Inject payload_dll.dll into PID {} and call SetWindowDisplayAffinity(WDA_NONE)",
            entry.pid
        ))
        .clicked()
    {
        *inject_target = Some(entry.clone());
    }
}

// ── Window enumeration (module-level callback) ────────────────────────────────

/// Callback for EnumWindows.  Collects visible, titled windows into the Vec
/// whose pointer is passed as lparam.
///
/// SAFETY: lparam must be a valid `*mut Vec<WindowEntry>` for the duration of
/// the EnumWindows call.
unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let entries = &mut *(lparam.0 as *mut Vec<WindowEntry>);

    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf);
    if len == 0 {
        return TRUE; // skip untitled windows
    }
    let title = String::from_utf16_lossy(&title_buf[..len as usize]);

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    let process_name = get_process_name(pid).unwrap_or_else(|| "<unknown>".to_string());

    entries.push(WindowEntry {
        hwnd: hwnd.0,
        pid,
        process_name,
        title,
    });

    TRUE
}

fn enumerate_windows() -> Vec<WindowEntry> {
    let mut entries: Vec<WindowEntry> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_cb),
            LPARAM(&mut entries as *mut Vec<WindowEntry> as isize),
        );
    }
    entries
}

/// Returns the bare filename (e.g. "notepad.exe") for the given PID, or None.
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
        let full_path = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&full_path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }
}
