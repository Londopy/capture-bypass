//! theme.rs — capture-bypass visual theme
//!
//! Modern dark "instrument" look: near-black surfaces, a single cyan signal
//! accent, flat/ghost controls, rounded corners, IBM Plex Mono for data.
//!
//! Entry point: call `theme::install(&cc.egui_ctx)` once in `App::new`.
//! Everything below is additive — it does not change any app logic, only how
//! widgets are painted. Reusable helpers (buttons, toggle, segmented control,
//! status badge, icons) live at the bottom.
//!
//! ── Built against egui 0.31 ──────────────────────────────────────────────
//! A few lines use API that changed across egui minor versions. If a build
//! error points here, these are the usual suspects (0.31 shown / older noted):
//!   • `font_data.insert(.., Arc::new(FontData::..))`  (0.29: no `Arc`)
//!   • `CornerRadius` + `window_corner_radius`/`corner_radius`  (0.29: `Rounding` + `*_rounding`)
//!   • `Shadow { offset: [i8;2], blur: u8, spread: u8, .. }`  (0.29: `Vec2` offset)
//!   • `Painter::rect_stroke(rect, cr, stroke, StrokeKind::Inside)`  (0.29: no `StrokeKind`)

#![allow(dead_code)]

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin,
    Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2,
};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────
// Palette
// ─────────────────────────────────────────────────────────────────────────

pub const BG: Color32 = Color32::from_rgb(0x0D, 0x0E, 0x12);
pub const BG_ELEV: Color32 = Color32::from_rgb(0x13, 0x15, 0x19);
pub const BG_ELEV_2: Color32 = Color32::from_rgb(0x18, 0x1A, 0x20);
pub const ROW: Color32 = Color32::from_rgb(0x10, 0x12, 0x16);
pub const ROW_ALT: Color32 = Color32::from_rgb(0x13, 0x15, 0x19);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1D, 0x23);
pub const LINE: Color32 = Color32::from_rgb(0x23, 0x26, 0x2E);
pub const LINE_SOFT: Color32 = Color32::from_rgb(0x1B, 0x1E, 0x24);

pub const ACCENT: Color32 = Color32::from_rgb(0x3D, 0xD6, 0xC4);
pub const ACCENT_2: Color32 = Color32::from_rgb(0x37, 0xA6, 0xC9);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x6C);
pub const SUCCESS: Color32 = Color32::from_rgb(0x54, 0xD9, 0x8C);
pub const AMBER: Color32 = Color32::from_rgb(0xE9, 0xA2, 0x3B);

pub const TEXT: Color32 = Color32::from_rgb(0xDD, 0xE1, 0xE8);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x7C, 0x82, 0x8F);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x4C, 0x52, 0x5E);
pub const PROCESS: Color32 = Color32::from_rgb(0x8F, 0xB8, 0xE8);

/// Cyan accent at `a`/255 opacity — for tint fills (blends over the surface).
pub fn accent_a(a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (0x3D * a as u32 / 255) as u8,
        (0xD6 * a as u32 / 255) as u8,
        (0xC4 * a as u32 / 255) as u8,
        a,
    )
}
/// Danger red at `a`/255 opacity.
pub fn danger_a(a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (0xFF * a as u32 / 255) as u8,
        (0x5C * a as u32 / 255) as u8,
        (0x6C * a as u32 / 255) as u8,
        a,
    )
}
/// Success green at `a`/255 opacity.
pub fn success_a(a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (0x54 * a as u32 / 255) as u8,
        (0xD9 * a as u32 / 255) as u8,
        (0x8C * a as u32 / 255) as u8,
        a,
    )
}

pub const RADIUS: u8 = 5;

// ─────────────────────────────────────────────────────────────────────────
// Install
// ─────────────────────────────────────────────────────────────────────────

/// Apply the full theme (fonts + colors + spacing + text styles). Call once.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    style.visuals = build_visuals();

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(11.0, 6.0);
    s.interact_size.y = 28.0;
    s.window_margin = Margin::same(0);
    s.menu_margin = Margin::same(6);
    s.indent = 18.0;

    use TextStyle::*;
    style.text_styles = [
        (Heading, FontId::new(17.0, FontFamily::Proportional)),
        (Body, FontId::new(13.5, FontFamily::Proportional)),
        (Button, FontId::new(13.0, FontFamily::Proportional)),
        (Small, FontId::new(11.0, FontFamily::Proportional)),
        (Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();

    ctx.set_style(style);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // ── IBM Plex Mono — Monospace family (PIDs, stats, badges, labels) ──────
    fonts.font_data.insert(
        "plex_mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/IBMPlexMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "plex_mono_bold".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/IBMPlexMono-Bold.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex_mono".to_owned());

    // ── IBM Plex Sans — Proportional family (all UI text) ───────────────────
    fonts.font_data.insert(
        "plex_sans".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/IBMPlexSans-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "plex_sans_medium".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/IBMPlexSans-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "plex_sans_semibold".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/IBMPlexSans-SemiBold.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex_sans".to_owned());

    ctx.set_fonts(fonts);
}

fn build_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);

    v.panel_fill = BG;
    v.window_fill = BG_ELEV;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = BG_ELEV; // text-edit background
    v.faint_bg_color = ROW_ALT; // striped table rows
    v.hyperlink_color = ACCENT;

    v.selection.bg_fill = accent_a(40);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 16],
        blur: 48,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };

    v.widgets = build_widgets();
    v
}

fn build_widgets() -> egui::style::Widgets {
    let cr = CornerRadius::same(RADIUS);
    let mut w = egui::style::Widgets::default();

    // Non-interactive: labels, separators, panel backgrounds.
    w.noninteractive.bg_fill = BG;
    w.noninteractive.weak_bg_fill = BG;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE_SOFT);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    w.noninteractive.corner_radius = cr;

    // Inactive (default): egui buttons use `weak_bg_fill` — transparent gives
    // the flat "ghost" look; `bg_fill` (used by checkboxes/combos) is elevated.
    w.inactive.bg_fill = BG_ELEV_2;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    w.inactive.corner_radius = cr;
    w.inactive.expansion = 0.0;

    // Hovered.
    w.hovered.bg_fill = BG_ELEV_2;
    w.hovered.weak_bg_fill = BG_ELEV_2;
    w.hovered.bg_stroke = Stroke::new(1.0, LINE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.corner_radius = cr;
    w.hovered.expansion = 1.0;

    // Active / pressed.
    w.active.bg_fill = accent_a(30);
    w.active.weak_bg_fill = accent_a(30);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.corner_radius = cr;
    w.active.expansion = 1.0;

    // Open (combo boxes, menus).
    w.open = w.active;
    w
}

// ─────────────────────────────────────────────────────────────────────────
// Text helpers
// ─────────────────────────────────────────────────────────────────────────

pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace().color(TEXT_DIM)
}
pub fn mono_faint(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace().size(11.0).color(TEXT_FAINT)
}
/// Small uppercase mono caption, e.g. section labels ("MODE", "WATCH").
pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .monospace()
        .size(10.0)
        .color(TEXT_FAINT)
}

// ─────────────────────────────────────────────────────────────────────────
// Buttons
// ─────────────────────────────────────────────────────────────────────────

/// Flat ghost button (default toolbar style). Transparent until hovered.
pub fn ghost(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(label).color(TEXT_DIM)))
}

/// The single accent-filled primary action for a view.
pub fn primary(ui: &mut Ui, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::from_rgb(0x0B, 0x15, 0x12)).strong())
            .fill(ACCENT),
    )
}

/// Destructive action — restrained danger outline, fills on hover.
pub fn danger(ui: &mut Ui, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(DANGER))
            .fill(danger_a(28))
            .stroke(Stroke::new(1.0, danger_a(90))),
    )
}

/// Toggle-styled button: accent tint when `on`, ghost when off.
pub fn toggle_button(ui: &mut Ui, on: bool, label: &str) -> Response {
    if on {
        ui.add(
            egui::Button::new(RichText::new(label).color(ACCENT))
                .fill(accent_a(30))
                .stroke(Stroke::new(1.0, accent_a(90))),
        )
    } else {
        ghost(ui, label)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Toggle switch (animated knob)
// ─────────────────────────────────────────────────────────────────────────

/// Linear interpolate between two opaque colors, `t` in 0..1.
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgb(
        egui::lerp(a.r() as f32..=b.r() as f32, t) as u8,
        egui::lerp(a.g() as f32..=b.g() as f32, t) as u8,
        egui::lerp(a.b() as f32..=b.b() as f32, t) as u8,
    )
}

/// Paint an animated switch at state `on` inside `rect`.
fn paint_switch(ui: &Ui, rect: Rect, id: egui::Id, on: bool) {
    let t = ui.ctx().animate_bool(id, on); // 0..1, eased by egui
    let radius = (rect.height() / 2.0) as u8;
    let track_col = lerp_color(
        Color32::from_rgb(0x1E, 0x21, 0x28),
        Color32::from_rgb(0x18, 0x35, 0x34),
        t,
    );
    let knob_col = lerp_color(Color32::from_rgb(0x5B, 0x61, 0x6D), ACCENT, t);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(radius), track_col);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, if on { accent_a(120) } else { LINE }),
        StrokeKind::Inside,
    );
    let r = rect.height() / 2.0 - 3.0;
    let cx = egui::lerp((rect.left() + r + 2.0)..=(rect.right() - r - 2.0), t);
    painter.circle_filled(Pos2::new(cx, rect.center().y), r, knob_col);
}

/// A sliding on/off switch that owns its state — flips `*on` on click.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(32.0, 18.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    paint_switch(ui, rect, resp.id, *on);
    resp
}

/// Display-only animated switch at a fixed state; returns the click response
/// WITHOUT flipping (the caller owns the state — useful for the flag pattern).
pub fn switch_at(ui: &mut Ui, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 18.0), Sense::click());
    paint_switch(ui, rect, resp.id, on);
    resp
}

/// A settings row: name + description on the left, animated switch on the right.
/// Returns true if the switch (or row) was clicked. Caller owns the state.
pub fn setting_row(ui: &mut Ui, on: bool, name: &str, desc: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(name).color(TEXT).size(13.0));
            if !desc.is_empty() {
                ui.label(RichText::new(desc).color(TEXT_DIM).size(11.5));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if switch_at(ui, on).clicked() {
                clicked = true;
            }
        });
    });
    ui.add_space(4.0);
    clicked
}

// ─────────────────────────────────────────────────────────────────────────
// Segmented control (sliding highlight)
// ─────────────────────────────────────────────────────────────────────────

/// Two-option segmented control. Returns the newly-selected index if clicked.
/// `selected` is the current index (0 or 1).
pub fn segmented2(ui: &mut Ui, selected: usize, opts: [&str; 2]) -> Option<usize> {
    let font = FontId::new(12.0, FontFamily::Proportional);
    let pad = 11.0;
    // Measure each option width.
    let widths: Vec<f32> = opts
        .iter()
        .map(|s| {
            ui.fonts(|f| {
                f.layout_no_wrap(s.to_string(), font.clone(), TEXT)
                    .size()
                    .x
            }) + pad * 2.0
        })
        .collect();
    let total_w = widths.iter().sum::<f32>() + 4.0;
    let height = 26.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(total_w, height), Sense::click());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), BG_ELEV_2);
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );

    // Animate the highlight position toward the selected segment.
    let target_x = 2.0 + widths[..selected].iter().sum::<f32>();
    let anim_x = ui
        .ctx()
        .animate_value_with_time(resp.id.with("seg_x"), target_x, 0.18);
    let anim_w = ui
        .ctx()
        .animate_value_with_time(resp.id.with("seg_w"), widths[selected], 0.18);
    let hl = Rect::from_min_size(
        Pos2::new(rect.left() + anim_x, rect.top() + 2.0),
        Vec2::new(anim_w, height - 4.0),
    );
    painter.rect_filled(hl, CornerRadius::same(3), BG);
    painter.rect_stroke(
        hl,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgb(0x2A, 0x2E, 0x36)),
        StrokeKind::Inside,
    );

    // Labels + hit testing.
    let mut clicked = None;
    let mut x = rect.left() + 2.0;
    for (i, s) in opts.iter().enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(widths[i], height - 4.0));
        let col = if i == selected {
            if i == 1 { ACCENT } else { TEXT }
        } else {
            TEXT_DIM
        };
        painter.text(seg.center(), Align2::CENTER_CENTER, *s, font.clone(), col);
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if seg.contains(pos) {
                    clicked = Some(i);
                }
            }
        }
        x += widths[i];
    }
    clicked.filter(|&i| i != selected)
}

// ─────────────────────────────────────────────────────────────────────────
// Status badge (protected / clear pill)
// ─────────────────────────────────────────────────────────────────────────

/// Render a rounded status pill. `protected` picks red PROTECTED vs green CLEAR.
pub fn status_badge(ui: &mut Ui, protected: bool) -> Response {
    let (label, col, bg, border) = if protected {
        ("PROTECTED", DANGER, danger_a(28), danger_a(90))
    } else {
        ("CLEAR", SUCCESS, success_a(24), success_a(80))
    };
    let font = FontId::new(10.5, FontFamily::Monospace);
    let text_w = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font.clone(), col).size().x);
    let w = text_w + 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, border), StrokeKind::Inside);
    // dot
    let dot_c = Pos2::new(rect.left() + 9.0, rect.center().y);
    painter.circle_filled(dot_c, 2.5, col);
    // label
    painter.text(
        Pos2::new(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        col,
    );
    resp
}

// ─────────────────────────────────────────────────────────────────────────
// A thin hairline separator that matches the palette.
// ─────────────────────────────────────────────────────────────────────────

pub fn hairline(ui: &mut Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, LINE_SOFT);
}
