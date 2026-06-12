//! Custom-painted controls matching Bitwig's look: rotary knobs, faders and
//! signal meters. Each returns whether the value changed so the caller can push
//! a command to the engine only on real edits.

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke, Ui, Vec2};

use crate::theme;

fn knob_face(painter: &egui::Painter, center: Pos2, radius: f32) {
    painter.circle_filled(center, radius, Color32::from_gray(0x16));
    painter.circle_filled(center, radius - 3.0, Color32::from_gray(0x2c));
    painter.circle_stroke(center, radius, Stroke::new(1.0, theme::BORDER));
}

/// Draw a coloured arc on the knob's outer ring spanning `amount` (bipolar) of
/// the sweep starting from `value` — Bitwig's modulation ring.
fn mod_ring(painter: &egui::Painter, center: Pos2, radius: f32, value: f32, amount: f32, color: Color32) {
    if amount.abs() < 0.005 {
        return;
    }
    let r = radius + 2.5;
    let a0 = -135.0 + value * 270.0;
    let a1 = (a0 + amount * 270.0).clamp(-150.0, 150.0);
    let (lo, hi) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let steps = ((hi - lo).abs() / 8.0).ceil().max(1.0) as i32;
    let mut prev: Option<Pos2> = None;
    for i in 0..=steps {
        let a = (lo + (hi - lo) * i as f32 / steps as f32).to_radians();
        let p = center + Vec2::new(a.sin(), -a.cos()) * r;
        if let Some(pp) = prev {
            painter.line_segment([pp, p], Stroke::new(2.5, color));
        }
        prev = Some(p);
    }
}

/// Vertical-drag rotary knob with a 270° sweep. `ring` optionally renders a
/// modulation arc. Returns true if `value` changed.
pub fn knob(ui: &mut Ui, value: &mut f32, label: &str, ring: Option<(f32, Color32)>) -> bool {
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
    knob_face(&painter, center, radius);

    let angle = (-135.0 + *value * 270.0).to_radians();
    let dir = Vec2::new(angle.sin(), -angle.cos());
    painter.line_segment(
        [center + dir * (radius * 0.35), center + dir * (radius * 0.95)],
        Stroke::new(2.5, theme::ACCENT),
    );
    if let Some((amount, color)) = ring {
        mod_ring(&painter, center, radius, *value, amount, color);
    }

    painter.text(Pos2::new(rect.center().x, rect.top() + 40.0), Align2::CENTER_CENTER, label, FontId::proportional(9.0), theme::TEXT_DIM);
    painter.text(Pos2::new(rect.center().x, rect.top() + 50.0), Align2::CENTER_CENTER, &format!("{}", (*value * 100.0).round() as i32), FontId::proportional(9.0), theme::TEXT_FAINT);

    changed
}

/// Modulation-assign knob: dragging edits the bipolar route `amount` (-1..1)
/// instead of the base value, and the ring shows the depth. Returns true if
/// `amount` changed.
pub fn knob_assign(ui: &mut Ui, base: f32, amount: &mut f32, label: &str, color: Color32) -> bool {
    let desired = Vec2::new(52.0, 56.0);
    let (rect, _resp) = ui.allocate_exact_size(desired, Sense::hover());
    let resp = ui.interact(rect, ui.id().with(("kassign", rect.left() as i32, rect.top() as i32, label)), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() {
        let speed = if ui.input(|i| i.modifiers.shift) { 0.0005 } else { 0.004 };
        let d = -resp.drag_delta().y * speed;
        if d != 0.0 {
            *amount = (*amount + d).clamp(-1.0, 1.0);
            changed = true;
        }
    }
    if resp.double_clicked() {
        *amount = 0.0;
        changed = true;
    }

    let painter = ui.painter_at(rect);
    let center = Pos2::new(rect.center().x, rect.top() + 20.0);
    let radius = 17.0;
    knob_face(&painter, center, radius);
    // dim base indicator
    let angle = (-135.0 + base * 270.0).to_radians();
    let dir = Vec2::new(angle.sin(), -angle.cos());
    painter.line_segment([center + dir * (radius * 0.35), center + dir * (radius * 0.95)], Stroke::new(2.0, theme::TEXT_FAINT));
    mod_ring(&painter, center, radius, base, *amount, color);
    // highlight border to signal assign mode
    painter.circle_stroke(center, radius + 4.0, Stroke::new(1.0, color));

    painter.text(Pos2::new(rect.center().x, rect.top() + 40.0), Align2::CENTER_CENTER, label, FontId::proportional(9.0), color);
    painter.text(Pos2::new(rect.center().x, rect.top() + 50.0), Align2::CENTER_CENTER, &format!("{:+.0}", *amount * 100.0), FontId::proportional(9.0), theme::TEXT_FAINT);
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
