//! Custom-painted controls matching Bitwig's look: rotary knobs, faders and
//! signal meters. Each returns whether the value changed so the caller can push
//! a command to the engine only on real edits.

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke, Ui, Vec2};

use crate::theme;

/// Vertical-drag rotary knob with a 270° sweep. Returns true if `value` changed.
pub fn knob(ui: &mut Ui, value: &mut f32, label: &str, modulated: bool) -> bool {
    let desired = Vec2::new(52.0, 56.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click_and_drag());
    let mut changed = false;

    if resp.dragged() {
        let speed = if ui.input(|i| i.modifiers.shift) { 0.001 } else { 0.005 };
        let delta = -resp.drag_delta().y * speed;
        if delta != 0.0 {
            *value = (*value + delta).clamp(0.0, 1.0);
            changed = true;
            resp.mark_changed();
        }
    }
    if resp.double_clicked() {
        *value = 0.5;
        changed = true;
    }

    let painter = ui.painter_at(rect);
    let center = Pos2::new(rect.center().x, rect.top() + 20.0);
    let radius = 17.0;
    painter.circle_filled(center, radius, Color32::from_gray(0x16));
    painter.circle_filled(center, radius - 3.0, Color32::from_gray(0x2c));
    painter.circle_stroke(center, radius, Stroke::new(1.0, theme::BORDER));

    // value arc tick
    let angle = (-135.0 + *value * 270.0).to_radians();
    let dir = Vec2::new(angle.sin(), -angle.cos());
    let ind = if modulated { theme::PLAY } else { theme::ACCENT };
    painter.line_segment(
        [center + dir * (radius * 0.35), center + dir * (radius * 0.95)],
        Stroke::new(2.5, ind),
    );

    painter.text(
        Pos2::new(rect.center().x, rect.top() + 40.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.0),
        theme::TEXT_DIM,
    );
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 50.0),
        Align2::CENTER_CENTER,
        &format!("{}", (*value * 100.0).round() as i32),
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );

    changed
}

/// A vertical meter filling `rect` from the bottom, green→amber→red.
pub fn meter(ui: &Ui, rect: Rect, level: f32) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, Rounding::same(2.0), theme::SLOT_EMPTY);
    let level = level.clamp(0.0, 1.0);
    if level <= 0.0 {
        return;
    }
    let h = rect.height() * level;
    let fill = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - h), rect.right_bottom());
    let color = if level > 0.9 {
        theme::RECORD
    } else if level > 0.7 {
        Color32::from_rgb(0xd0, 0xc0, 0x40)
    } else {
        theme::PLAY
    };
    painter.rect_filled(fill, Rounding::same(2.0), color);
}

/// Vertical fader. Returns true when `value` (0..1) changes.
pub fn fader(ui: &mut Ui, rect: Rect, value: &mut f32) -> bool {
    let resp = ui.interact(rect, ui.id().with(("fader", rect.left() as i32, rect.top() as i32)), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            if (v - *value).abs() > f32::EPSILON {
                *value = v;
                changed = true;
            }
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, Rounding::same(3.0), theme::SLOT_EMPTY);
    let h = rect.height() * *value;
    let fill = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - h), rect.right_bottom());
    painter.rect_filled(fill, Rounding::same(3.0), theme::ACCENT_DIM);
    let handle_y = rect.bottom() - h;
    let handle = Rect::from_min_max(
        Pos2::new(rect.left() - 2.0, handle_y - 4.0),
        Pos2::new(rect.right() + 2.0, handle_y + 4.0),
    );
    painter.rect_filled(handle, Rounding::same(2.0), theme::TEXT);
    changed
}
