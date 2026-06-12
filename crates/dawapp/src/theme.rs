//! Bitwig Studio 6 visual language for egui: permanent dark theme, near-black
//! layered panels, rounded corners, and the signature warm orange accent.

use egui::{Color32, Context, Rounding, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const PANEL: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const PANEL_ALT: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const HEADER: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const SLOT_EMPTY: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const BORDER: Color32 = Color32::from_rgb(0x0d, 0x0d, 0x0d);
pub const GRID: Color32 = Color32::from_rgb(0x2e, 0x2e, 0x2e);
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x8a);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5c, 0x5c, 0x5c);
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0x8a, 0x00);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0xb3, 0x5f, 0x00);
pub const RECORD: Color32 = Color32::from_rgb(0xff, 0x3b, 0x30);
pub const PLAY: Color32 = Color32::from_rgb(0x5a, 0xc8, 0x5a);

pub fn track_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn install(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = PANEL;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = SLOT_EMPTY;
    visuals.faint_bg_color = PANEL_ALT;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.bg_fill = PANEL_RAISED;
    visuals.widgets.inactive.weak_bg_fill = PANEL_RAISED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(4.0);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    visuals.widgets.hovered.rounding = Rounding::same(4.0);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.rounding = Rounding::same(4.0);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_style(style);
}
