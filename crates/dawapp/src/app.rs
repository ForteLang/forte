//! The Bitwig-style desktop front-end. Holds the project model and pushes edits
//! to the audio engine through the lock-free command channel.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke, Vec2};

use std::collections::HashSet;

use dawcore::command::Command;
use dawcore::engine::{build_automation, build_clip, build_device, build_mods, build_track};
use dawcore::model::{
    note_name, AutomationPoint, Clip, Device, DeviceKind, GridConn, GridModule, GridModuleKind,
    ModKind, ModRoute, Modulator, Note, Project, Scale, Track, TrackKind, MAX_TRACKS, NOTE_NAMES,
    TRACK_COLORS,
};

use crate::audio::{self, Audio};
use crate::theme;
use crate::widgets;

#[derive(PartialEq, Eq, Clone, Copy)]
enum View {
    Arrange,
    Launcher,
    Mix,
}

#[derive(Clone, Copy)]
enum ArrDragMode {
    Move,
    Resize,
}

struct ArrDrag {
    track: usize,
    index: usize,
    mode: ArrDragMode,
    grab: f64,  // beat offset within clip where grabbed
    audio: bool, // targets an audio clip rather than a MIDI clip
}

#[derive(Clone, Copy)]
enum DragMode {
    Move,
    Resize,
}

struct NoteDrag {
    idx: usize,
    mode: DragMode,
}

pub struct DawApp {
    audio: Option<Audio>,
    sample_rate: f32,
    audio_error: Option<String>,

    project: Project,
    view: View,
    selected_track: usize,
    selected_device: usize,
    editing: Option<(usize, usize)>, // (track display index, scene)
    note_drag: Option<NoteDrag>,
    master_volume: f32,

    /// Active modulation-routing target: (track idx, device idx, modulator idx).
    assign_mod: Option<(usize, usize, usize)>,
    arr_drag: Option<ArrDrag>,
    loop_anchor: Option<f64>,
    beats_per_px: f32, // arranger zoom (beats per pixel)

    /// Tracks whose automation sub-lane is expanded in the Arranger.
    auto_open: HashSet<usize>,
    /// Dragged automation point: (track idx, point idx).
    auto_drag: Option<(usize, usize)>,

    // The Grid editor
    grid_edit: Option<(usize, usize)>, // (track, device) of the open Poly Grid
    grid_node_drag: Option<usize>,     // node being moved
    grid_wire_drag: Option<(usize, usize)>, // dragging a wire from (node, out port)
    grid_pan: egui::Vec2,

    // file & history
    metronome: bool,
    file_win: bool,
    file_path: String,
    status: Option<String>,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,

    /// Commands accumulated during a frame, flushed to the engine at the end.
    cmds: Vec<Command>,
}

// computer-keyboard → MIDI map (A row = white keys from C4)
fn key_to_pitch(key: egui::Key) -> Option<u8> {
    use egui::Key::*;
    Some(match key {
        A => 60,
        W => 61,
        S => 62,
        E => 63,
        D => 64,
        F => 65,
        T => 66,
        G => 67,
        Y => 68,
        H => 69,
        U => 70,
        J => 71,
        K => 72,
        _ => return None,
    })
}

impl DawApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);

        // Always returns a working backend (silent fallback if no hardware).
        let a = audio::start();
        let sample_rate = a.sample_rate;
        let audio_error = if a.silent { Some(a.device_name.clone()) } else { None };
        let audio = Some(a);

        let project = Project::demo();

        // Optional startup hooks (handy for testing / launching into a view).
        let view = match std::env::var("BITWIG_VIEW").as_deref() {
            Ok("mix") => View::Mix,
            _ => View::Arrange,
        };
        let editing = std::env::var("BITWIG_EDIT").ok().map(|_| (2usize, 0usize));
        let mut auto_open_init = HashSet::new();
        if std::env::var("BITWIG_AUTO").is_ok() {
            auto_open_init.insert(3); // Lead's volume lane (has demo automation)
        }
        // Open the Grid editor on the Bass (Poly Grid) track for headless capture.
        let (grid_edit, sel_track) = if std::env::var("BITWIG_GRID").is_ok() {
            let bi = project.tracks.iter().position(|t| t.name == "Bass").unwrap_or(0);
            (Some((bi, 0usize)), bi)
        } else {
            (None, 0usize)
        };

        let mut app = DawApp {
            audio,
            sample_rate,
            audio_error,
            project,
            view,
            selected_track: sel_track,
            selected_device: 0,
            editing,
            note_drag: None,
            master_volume: 0.9,
            assign_mod: None,
            arr_drag: None,
            loop_anchor: None,
            beats_per_px: 1.0 / 24.0, // 24 px per beat
            auto_open: auto_open_init,
            auto_drag: None,
            grid_edit,
            grid_node_drag: None,
            grid_wire_drag: None,
            grid_pan: egui::Vec2::ZERO,
            metronome: false,
            file_win: false,
            file_path: "project.bitwig.json".into(),
            status: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            cmds: Vec::new(),
        };
        app.initial_sync();
        app
    }

    fn initial_sync(&mut self) {
        let sr = self.sample_rate;
        self.cmds.push(Command::SetTempo(self.project.tempo));
        self.cmds.push(Command::SetLaunchQuant(self.project.launch_quant));
        self.cmds.push(Command::SetLoop {
            enabled: self.project.loop_enabled,
            start: self.project.loop_start,
            end: self.project.loop_end,
        });
        for t in &self.project.tracks {
            self.cmds.push(Command::AddTrack { slot: t.id, track: build_track(t, sr) });
        }
        // Optional: auto-start arrangement playback (headless capture / demos).
        if std::env::var("BITWIG_PLAY").is_ok() {
            self.cmds.push(Command::Play);
        }
        self.flush();
    }

    /// Rebuild a whole track in the engine after an arrangement/device edit.
    fn sync_track(&mut self, ti: usize) {
        let sr = self.sample_rate;
        if let Some(t) = self.project.tracks.get(ti) {
            self.cmds.push(Command::AddTrack { slot: t.id, track: build_track(t, sr) });
        }
    }

    fn push_loop(&mut self) {
        self.cmds.push(Command::SetLoop {
            enabled: self.project.loop_enabled,
            start: self.project.loop_start,
            end: self.project.loop_end,
        });
    }

    fn sync_automation(&mut self, ti: usize) {
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.volume_automation
                .sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
            let id = t.id;
            let pts = build_automation(t);
            self.cmds.push(Command::SetAutomation { track: id, points: Box::new(pts) });
        }
    }

    fn sync_sends(&mut self, ti: usize) {
        if let Some(t) = self.project.tracks.get(ti) {
            self.cmds.push(Command::SetSends { track: t.id, sends: Box::new(t.sends.clone()) });
        }
    }

    // ---- undo / redo -------------------------------------------------------

    fn push_undo(&mut self) {
        self.undo_stack.push(self.project.clone());
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.project.clone());
            self.apply_project(prev);
            self.status = Some("Undo".into());
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.project.clone());
            self.apply_project(next);
            self.status = Some("Redo".into());
        }
    }

    /// Replace the project and rebuild the whole engine graph.
    fn apply_project(&mut self, p: Project) {
        self.project = p;
        self.editing = None;
        self.assign_mod = None;
        if self.selected_track >= self.project.tracks.len() {
            self.selected_track = self.project.tracks.len().saturating_sub(1);
        }
        self.selected_device = 0;
        self.resync_all();
    }

    fn resync_all(&mut self) {
        for slot in 0..MAX_TRACKS {
            self.cmds.push(Command::RemoveTrack { slot });
        }
        let sr = self.sample_rate;
        self.cmds.push(Command::SetTempo(self.project.tempo));
        self.cmds.push(Command::SetLaunchQuant(self.project.launch_quant));
        self.cmds.push(Command::SetMetronome(self.metronome));
        self.push_loop();
        for t in &self.project.tracks {
            self.cmds.push(Command::AddTrack { slot: t.id, track: build_track(t, sr) });
        }
    }

    // ---- file operations ---------------------------------------------------

    fn save_project(&mut self) {
        let json = self.project.to_json();
        match std::fs::write(&self.file_path, json) {
            Ok(()) => self.status = Some(format!("Saved {}", self.file_path)),
            Err(e) => self.status = Some(format!("Save failed: {e}")),
        }
    }

    fn open_project(&mut self) {
        match std::fs::read_to_string(&self.file_path) {
            Ok(s) => match Project::from_json(&s) {
                Ok(p) => {
                    self.push_undo();
                    self.apply_project(p);
                    self.status = Some(format!("Opened {}", self.file_path));
                }
                Err(e) => self.status = Some(format!("Parse failed: {e}")),
            },
            Err(e) => self.status = Some(format!("Open failed: {e}")),
        }
    }

    fn export_wav(&mut self) {
        let path = std::path::Path::new(&self.file_path).with_extension("wav");
        match dawcore::bounce::render_wav(&self.project, &path, 8.0) {
            Ok(secs) => self.status = Some(format!("Exported {} ({secs:.1}s)", path.display())),
            Err(e) => self.status = Some(format!("Export failed: {e}")),
        }
    }

    fn flush(&mut self) {
        if let Some(a) = &mut self.audio {
            for c in self.cmds.drain(..) {
                a.handle.send(c);
            }
            a.handle.collect_garbage();
        } else {
            self.cmds.clear();
        }
    }

    fn playing(&self) -> bool {
        self.audio
            .as_ref()
            .map(|a| a.handle.shared.playing.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    fn position_beats(&self) -> f64 {
        self.audio.as_ref().map(|a| a.handle.shared.position_beats()).unwrap_or(0.0)
    }

    fn track_peak(&self, slot: usize) -> f32 {
        self.audio.as_ref().map(|a| a.handle.shared.track_peak(slot)).unwrap_or(0.0)
    }

    fn master_peak(&self) -> f32 {
        self.audio.as_ref().map(|a| a.handle.shared.master_peak()).unwrap_or(0.0)
    }

    fn active_scene(&self, slot: usize) -> i32 {
        self.audio.as_ref().map(|a| a.handle.shared.active_scene(slot)).unwrap_or(-1)
    }

    fn toggle_play(&mut self) {
        if self.playing() {
            self.cmds.push(Command::Stop);
        } else {
            self.cmds.push(Command::Play);
            self.cmds.push(Command::LaunchScene(0));
        }
    }
}

impl eframe::App for DawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint(); // keep meters and the playhead live

        self.handle_input(ctx);

        egui::TopBottomPanel::top("transport")
            .exact_height(46.0)
            .show(ctx, |ui| self.transport_bar(ui));

        egui::SidePanel::left("inspector")
            .exact_width(200.0)
            .resizable(false)
            .show(ctx, |ui| self.inspector(ui));

        egui::SidePanel::right("browser")
            .exact_width(200.0)
            .resizable(false)
            .show(ctx, |ui| self.browser(ui));

        let editing = self.editing.is_some();
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(if editing { 280.0 } else { 256.0 })
            .resizable(true)
            .show(ctx, |ui| {
                if editing {
                    self.piano_roll(ui);
                } else {
                    self.device_panel(ui);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Arrange => self.arranger(ui),
            View::Launcher => self.clip_launcher(ui),
            View::Mix => self.mixer(ui),
        });

        if self.file_win {
            self.file_window(ctx);
        }
        if self.grid_edit.is_some() {
            self.grid_window(ctx);
        }

        self.flush();
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

impl DawApp {
    fn handle_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let track_id = self.project.tracks.get(self.selected_track).map(|t| t.id);
        for ev in events {
            if let egui::Event::Key { key, pressed, repeat, modifiers, .. } = ev {
                // global shortcuts
                if modifiers.command || modifiers.ctrl {
                    if pressed && !repeat {
                        match key {
                            egui::Key::Z if modifiers.shift => self.redo(),
                            egui::Key::Z => self.undo(),
                            egui::Key::Y => self.redo(),
                            egui::Key::S => self.save_project(),
                            _ => {}
                        }
                    }
                    continue;
                }
                if key == egui::Key::Space && pressed && !repeat {
                    // avoid stealing space from text fields
                    if !ctx.wants_keyboard_input() {
                        self.toggle_play();
                    }
                    continue;
                }
                if repeat {
                    continue;
                }
                if let (Some(pitch), Some(tid)) = (key_to_pitch(key), track_id) {
                    if ctx.wants_keyboard_input() {
                        continue;
                    }
                    if pressed {
                        self.cmds.push(Command::NoteOn { track: tid, note: pitch, velocity: 0.85 });
                    } else {
                        self.cmds.push(Command::NoteOff { track: tid, note: pitch });
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The Grid editor
// ---------------------------------------------------------------------------

const GN_W: f32 = 116.0;
const GN_TITLE_H: f32 = 20.0;
const GN_PORT_DY: f32 = 16.0;

impl DawApp {
    /// Geometry for an output/input port: returns its centre on the canvas.
    fn grid_port_pos(origin: Pos2, m: &GridModule, port: usize, is_out: bool) -> Pos2 {
        let n = if is_out { m.kind.outputs().len() } else { m.kind.inputs().len() };
        let body_h = GN_TITLE_H + n.max(1) as f32 * GN_PORT_DY + 6.0;
        let _ = body_h;
        let x = if is_out { origin.x + m.pos.0 + GN_W } else { origin.x + m.pos.0 };
        let y = origin.y + m.pos.1 + GN_TITLE_H + port as f32 * GN_PORT_DY + GN_PORT_DY * 0.5;
        Pos2::new(x, y)
    }

    fn grid_window(&mut self, ctx: &egui::Context) {
        let Some((ti, di)) = self.grid_edit else { return };
        if ti >= self.project.tracks.len()
            || di >= self.project.tracks[ti].devices.len()
            || self.project.tracks[ti].devices[di].grid.is_none()
        {
            self.grid_edit = None;
            return;
        }

        let mut open = true;
        let track_name = self.project.tracks[ti].name.clone();
        egui::Window::new(format!("The Grid — {track_name}"))
            .open(&mut open)
            .default_size([720.0, 420.0])
            .default_pos([180.0, 90.0])
            .show(ctx, |ui| {
                // palette
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Add:").size(10.0).color(theme::TEXT_FAINT));
                    for k in GridModuleKind::PALETTE {
                        if ui.button(k.label()).clicked() {
                            self.grid_add_module(ti, di, k);
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("drag node = move · drag out→in = wire · right-click = delete").size(9.0).color(theme::TEXT_FAINT));
                });
                ui.separator();
                self.grid_canvas(ui, ti, di);
            });
        if !open {
            self.grid_edit = None;
        }
    }

    fn grid_add_module(&mut self, ti: usize, di: usize, kind: GridModuleKind) {
        self.push_undo();
        if let Some(g) = self.project.tracks[ti].devices[di].grid.as_mut() {
            g.modules.push(GridModule { kind, pos: (40.0, 220.0), params: kind.defaults() });
        }
        self.sync_track(ti);
    }

    fn grid_canvas(&mut self, ui: &mut egui::Ui, ti: usize, di: usize) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, Rounding::same(4.0), Color32::from_gray(0x14));
        // dot grid
        let mut gx = rect.left();
        while gx < rect.right() {
            let mut gy = rect.top();
            while gy < rect.bottom() {
                painter.circle_filled(Pos2::new(gx, gy), 0.7, Color32::from_gray(0x24));
                gy += 24.0;
            }
            gx += 24.0;
        }
        let origin = rect.left_top() + self.grid_pan + egui::vec2(8.0, 8.0);

        let modules = self.project.tracks[ti].devices[di].grid.as_ref().unwrap().modules.clone();
        let conns = self.project.tracks[ti].devices[di].grid.as_ref().unwrap().conns.clone();

        // wires
        for c in &conns {
            if c.from.0 >= modules.len() || c.to.0 >= modules.len() {
                continue;
            }
            let a = Self::grid_port_pos(origin, &modules[c.from.0], c.from.1, true);
            let b = Self::grid_port_pos(origin, &modules[c.to.0], c.to.1, false);
            grid_wire(&painter, a, b, theme::ACCENT);
        }
        // wire being dragged
        if let Some((nf, pf)) = self.grid_wire_drag {
            if nf < modules.len() {
                let a = Self::grid_port_pos(origin, &modules[nf], pf, true);
                if let Some(p) = resp.interact_pointer_pos() {
                    grid_wire(&painter, a, p, theme::PLAY);
                }
            }
        }

        let pointer = resp.interact_pointer_pos();

        // draw nodes + collect port hits
        let mut hover_in: Option<(usize, usize)> = None;
        for (ni, m) in modules.iter().enumerate() {
            let n_in = m.kind.inputs().len();
            let n_out = m.kind.outputs().len();
            let rows = n_in.max(n_out).max(1);
            let body_h = GN_TITLE_H + rows as f32 * GN_PORT_DY + 6.0;
            let node_rect = Rect::from_min_size(
                origin + egui::vec2(m.pos.0, m.pos.1),
                Vec2::new(GN_W, body_h),
            );
            let is_instrument_io = matches!(m.kind, GridModuleKind::NoteIn | GridModuleKind::Out);
            let title_col = if is_instrument_io { theme::ACCENT_DIM } else { theme::PANEL_RAISED };
            painter.rect_filled(node_rect, Rounding::same(5.0), theme::PANEL_ALT);
            painter.rect_filled(
                Rect::from_min_size(node_rect.left_top(), Vec2::new(GN_W, GN_TITLE_H)),
                Rounding::same(5.0),
                title_col,
            );
            painter.rect_stroke(node_rect, Rounding::same(5.0), Stroke::new(1.0, theme::BORDER));
            painter.text(node_rect.left_top() + egui::vec2(7.0, 10.0), Align2::LEFT_CENTER, m.kind.label(), FontId::proportional(11.0), theme::TEXT);

            // input ports (left) + labels
            for p in 0..n_in {
                let pp = Self::grid_port_pos(origin, m, p, false);
                painter.circle_filled(pp, 4.0, theme::TEXT_DIM);
                painter.text(pp + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, m.kind.inputs()[p], FontId::proportional(8.0), theme::TEXT_FAINT);
                if let Some(ptr) = pointer {
                    if pp.distance(ptr) < 9.0 {
                        hover_in = Some((ni, p));
                    }
                }
            }
            // output ports (right) + labels
            for p in 0..n_out {
                let pp = Self::grid_port_pos(origin, m, p, true);
                painter.circle_filled(pp, 4.0, theme::ACCENT);
                painter.text(pp - egui::vec2(8.0, 0.0), Align2::RIGHT_CENTER, m.kind.outputs()[p], FontId::proportional(8.0), theme::TEXT_FAINT);
            }

        }

        // node param mini-knobs drawn as a compact row beneath the title
        for (ni, m) in modules.iter().enumerate() {
            let params = m.kind.params();
            if params.is_empty() {
                continue;
            }
            let base = origin + egui::vec2(m.pos.0, m.pos.1);
            let rows = m.kind.inputs().len().max(m.kind.outputs().len()).max(1);
            let py = base.y + GN_TITLE_H + rows as f32 * GN_PORT_DY + 2.0;
            for (pi, plabel) in params.iter().enumerate() {
                let cx = base.x + 14.0 + pi as f32 * 26.0;
                let knob_rect = Rect::from_center_size(Pos2::new(cx, py + 10.0), Vec2::splat(20.0));
                let mut v = m.params.get(pi).copied().unwrap_or(0.0);
                let kresp = ui.interact(knob_rect, ui.id().with(("gk", ti, di, ni, pi)), Sense::drag());
                if kresp.dragged() {
                    v = (v - kresp.drag_delta().y * 0.01).clamp(0.0, 1.0);
                    self.project.tracks[ti].devices[di].grid.as_mut().unwrap().modules[ni].params[pi] = v;
                    self.cmds.push(Command::SetGridParam { track: self.project.tracks[ti].id, device: di, node: ni, param: pi, value: v });
                }
                painter.circle_filled(knob_rect.center(), 9.0, Color32::from_gray(0x2c));
                let ang = (-135.0 + v * 270.0).to_radians();
                let dir = Vec2::new(ang.sin(), -ang.cos());
                painter.line_segment([knob_rect.center(), knob_rect.center() + dir * 8.0], Stroke::new(1.5, theme::ACCENT));
                painter.text(Pos2::new(cx, py + 22.0), Align2::CENTER_CENTER, *plabel, FontId::proportional(7.0), theme::TEXT_FAINT);
            }
        }

        // ---- interactions ----
        if resp.drag_started() {
            if let Some(ptr) = pointer {
                // start a wire from an output port?
                let mut started = false;
                for (ni, m) in modules.iter().enumerate() {
                    for p in 0..m.kind.outputs().len() {
                        if Self::grid_port_pos(origin, m, p, true).distance(ptr) < 9.0 {
                            self.grid_wire_drag = Some((ni, p));
                            started = true;
                        }
                    }
                }
                // else pick a node body to move
                if !started {
                    for (ni, m) in modules.iter().enumerate() {
                        let rows = m.kind.inputs().len().max(m.kind.outputs().len()).max(1);
                        let body_h = GN_TITLE_H + rows as f32 * GN_PORT_DY + 6.0;
                        let nr = Rect::from_min_size(origin + egui::vec2(m.pos.0, m.pos.1), Vec2::new(GN_W, body_h));
                        if nr.contains(ptr) {
                            self.grid_node_drag = Some(ni);
                            self.push_undo();
                        }
                    }
                }
            }
        }
        if resp.dragged() {
            if let Some(ni) = self.grid_node_drag {
                let d = resp.drag_delta();
                let g = self.project.tracks[ti].devices[di].grid.as_mut().unwrap();
                g.modules[ni].pos.0 = (g.modules[ni].pos.0 + d.x).max(0.0);
                g.modules[ni].pos.1 = (g.modules[ni].pos.1 + d.y).max(0.0);
            }
        }
        if resp.drag_stopped() {
            if let Some((nf, pf)) = self.grid_wire_drag.take() {
                if let Some((nt, pt)) = hover_in {
                    if nt != nf {
                        self.grid_connect(ti, di, (nf, pf), (nt, pt));
                    }
                }
            }
            if self.grid_node_drag.take().is_some() {
                self.sync_track(ti); // positions are cosmetic, but keep engine in step
            }
        }
        // right-click delete: node or wire
        if resp.secondary_clicked() {
            if let Some(ptr) = pointer {
                // delete a node under the pointer
                let mut deleted = false;
                for (ni, m) in modules.iter().enumerate() {
                    if matches!(m.kind, GridModuleKind::NoteIn | GridModuleKind::Out) {
                        continue; // keep the fixed I/O nodes
                    }
                    let rows = m.kind.inputs().len().max(m.kind.outputs().len()).max(1);
                    let body_h = GN_TITLE_H + rows as f32 * GN_PORT_DY + 6.0;
                    let nr = Rect::from_min_size(origin + egui::vec2(m.pos.0, m.pos.1), Vec2::new(GN_W, body_h));
                    if nr.contains(ptr) {
                        self.grid_remove_module(ti, di, ni);
                        deleted = true;
                        break;
                    }
                }
                // else delete a wire near the pointer
                if !deleted {
                    for (ci, c) in conns.iter().enumerate() {
                        if c.from.0 >= modules.len() || c.to.0 >= modules.len() {
                            continue;
                        }
                        let a = Self::grid_port_pos(origin, &modules[c.from.0], c.from.1, true);
                        let b = Self::grid_port_pos(origin, &modules[c.to.0], c.to.1, false);
                        if dist_to_segment(ptr, a, b) < 6.0 {
                            self.push_undo();
                            self.project.tracks[ti].devices[di].grid.as_mut().unwrap().conns.remove(ci);
                            self.sync_track(ti);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn grid_connect(&mut self, ti: usize, di: usize, from: (usize, usize), to: (usize, usize)) {
        self.push_undo();
        let g = self.project.tracks[ti].devices[di].grid.as_mut().unwrap();
        // one source per input port: replace any existing wire into that port
        g.conns.retain(|c| c.to != to);
        g.conns.push(GridConn { from, to });
        self.sync_track(ti);
    }

    fn grid_remove_module(&mut self, ti: usize, di: usize, node: usize) {
        self.push_undo();
        let g = self.project.tracks[ti].devices[di].grid.as_mut().unwrap();
        g.modules.remove(node);
        // drop wires touching it and renumber higher indices
        g.conns.retain(|c| c.from.0 != node && c.to.0 != node);
        for c in &mut g.conns {
            if c.from.0 > node { c.from.0 -= 1; }
            if c.to.0 > node { c.to.0 -= 1; }
        }
        self.sync_track(ti);
    }
}

fn grid_wire(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    // simple cubic bezier with horizontal tangents
    let dx = (b.x - a.x).abs().max(30.0) * 0.5;
    let c1 = Pos2::new(a.x + dx, a.y);
    let c2 = Pos2::new(b.x - dx, b.y);
    let mut prev = a;
    let steps = 18;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let mt = 1.0 - t;
        let p = Pos2::new(
            mt * mt * mt * a.x + 3.0 * mt * mt * t * c1.x + 3.0 * mt * t * t * c2.x + t * t * t * b.x,
            mt * mt * mt * a.y + 3.0 * mt * mt * t * c1.y + 3.0 * mt * t * t * c2.y + t * t * t * b.y,
        );
        painter.line_segment([prev, p], Stroke::new(2.0, color));
        prev = p;
    }
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() < 1e-6 {
        0.0
    } else {
        (((p - a).dot(ab)) / ab.length_sq()).clamp(0.0, 1.0)
    };
    let proj = a + ab * t;
    p.distance(proj)
}

// ---------------------------------------------------------------------------
// File window
// ---------------------------------------------------------------------------

impl DawApp {
    fn file_window(&mut self, ctx: &egui::Context) {
        let mut open = self.file_win;
        egui::Window::new("Project File")
            .open(&mut open)
            .resizable(false)
            .default_pos([260.0, 60.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Path");
                    ui.add(egui::TextEdit::singleline(&mut self.file_path).desired_width(280.0));
                });
                ui.horizontal(|ui| {
                    if ui.button("💾 Save (Ctrl+S)").clicked() {
                        self.save_project();
                    }
                    if ui.button("📂 Open").clicked() {
                        self.open_project();
                    }
                    if ui.button("🔊 Export WAV").clicked() {
                        self.status = Some("Rendering…".into());
                        self.export_wav();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("↶ Undo (Ctrl+Z)").clicked() {
                        self.undo();
                    }
                    if ui.button("↷ Redo (Ctrl+Y)").clicked() {
                        self.redo();
                    }
                    ui.label(
                        egui::RichText::new(format!("history: {}", self.undo_stack.len()))
                            .size(10.0)
                            .color(theme::TEXT_FAINT),
                    );
                });
                if let Some(s) = &self.status {
                    ui.separator();
                    ui.label(egui::RichText::new(s).size(11.0).color(theme::ACCENT));
                }
            });
        self.file_win = open;
    }
}

// ---------------------------------------------------------------------------
// Transport bar
// ---------------------------------------------------------------------------

impl DawApp {
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            // logo
            let (r, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
            ui.painter().rect_filled(r, Rounding::same(3.0), theme::ACCENT);
            ui.label(egui::RichText::new("BITWIG").strong().color(theme::ACCENT));
            ui.label(egui::RichText::new("Studio 6").color(theme::TEXT_DIM));
            if ui.selectable_label(self.file_win, "File").clicked() {
                self.file_win = !self.file_win;
            }
            ui.separator();

            let playing = self.playing();
            if ui.add(transport_btn("●", theme::RECORD, false)).clicked() {}
            if ui.add(transport_btn(if playing { "⏸" } else { "▶" }, theme::PLAY, playing)).clicked() {
                self.toggle_play();
            }
            if ui.add(transport_btn("■", theme::TEXT, false)).clicked() {
                self.cmds.push(Command::Stop);
            }
            // arranger loop toggle
            if ui.add(transport_btn("⟲", theme::ACCENT, self.project.loop_enabled)).clicked() {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.push_loop();
            }
            // metronome click
            if ui.add(transport_btn("♪", theme::PLAY, self.metronome)).clicked() {
                self.metronome = !self.metronome;
                self.cmds.push(Command::SetMetronome(self.metronome));
            }
            ui.separator();

            // position
            let pos = self.position_beats();
            let (num, _) = self.project.time_sig;
            let bar = (pos / num as f64).floor() as i64 + 1;
            let beat = (pos % num as f64).floor() as i64 + 1;
            let six = ((pos % 1.0) * 4.0).floor() as i64 + 1;
            ui.label(
                egui::RichText::new(format!("{bar}.{beat}.{six}"))
                    .monospace()
                    .size(16.0)
                    .strong(),
            );
            ui.separator();

            // tempo
            ui.label(egui::RichText::new("TEMPO").size(9.0).color(theme::TEXT_FAINT));
            let resp = ui.add(
                egui::DragValue::new(&mut self.project.tempo)
                    .range(20.0..=300.0)
                    .speed(0.2)
                    .fixed_decimals(1),
            );
            if resp.changed() {
                self.cmds.push(Command::SetTempo(self.project.tempo));
            }

            let (n, d) = self.project.time_sig;
            ui.label(egui::RichText::new(format!("{n}/{d}")).color(theme::TEXT_DIM));
            ui.separator();

            // project key signature (Bitwig 6)
            ui.label(egui::RichText::new("KEY").size(9.0).color(theme::TEXT_FAINT));
            let mut root = self.project.key.root as usize;
            egui::ComboBox::from_id_salt("key_root")
                .width(46.0)
                .selected_text(NOTE_NAMES[root])
                .show_ui(ui, |ui| {
                    for (i, name) in NOTE_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut root, i, *name);
                    }
                });
            self.project.key.root = root as u8;

            let mut scale = self.project.key.scale;
            egui::ComboBox::from_id_salt("key_scale")
                .width(110.0)
                .selected_text(scale.name())
                .show_ui(ui, |ui| {
                    for s in Scale::ALL {
                        ui.selectable_value(&mut scale, s, s.name());
                    }
                });
            self.project.key.scale = scale;
            ui.separator();

            // launch quantization (Bitwig default: 1 bar)
            ui.label(egui::RichText::new("Q").size(9.0).color(theme::TEXT_FAINT));
            let quant_label = |q: f64| match q {
                x if x <= 0.0 => "Off",
                x if (x - 1.0).abs() < 0.01 => "1/4",
                x if (x - 2.0).abs() < 0.01 => "1/2",
                x if (x - 4.0).abs() < 0.01 => "1 Bar",
                x if (x - 8.0).abs() < 0.01 => "2 Bars",
                _ => "1 Bar",
            };
            let mut q = self.project.launch_quant;
            egui::ComboBox::from_id_salt("launch_quant")
                .width(64.0)
                .selected_text(quant_label(q))
                .show_ui(ui, |ui| {
                    for opt in [0.0, 1.0, 2.0, 4.0, 8.0] {
                        ui.selectable_value(&mut q, opt, quant_label(opt));
                    }
                });
            if q != self.project.launch_quant {
                self.project.launch_quant = q;
                self.cmds.push(Command::SetLaunchQuant(q));
            }

            // view toggle on the far right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(self.view == View::Mix, "Mix").clicked() {
                    self.view = View::Mix;
                }
                if ui.selectable_label(self.view == View::Launcher, "Launcher").clicked() {
                    self.view = View::Launcher;
                }
                if ui.selectable_label(self.view == View::Arrange, "Arrange").clicked() {
                    self.view = View::Arrange;
                }
                if self.audio_error.is_some() {
                    ui.label(egui::RichText::new("◌ silent").color(theme::TEXT_FAINT).size(10.0))
                        .on_hover_text("No audio device — transport and sequencer run, but there is no sound output.");
                }
            });
        });
    }
}

fn transport_btn(glyph: &str, color: Color32, active: bool) -> egui::Button<'static> {
    let mut b = egui::Button::new(egui::RichText::new(glyph).color(if active { Color32::BLACK } else { color }))
        .min_size(Vec2::new(30.0, 26.0));
    if active {
        b = b.fill(color);
    }
    b
}

// ---------------------------------------------------------------------------
// Inspector (left)
// ---------------------------------------------------------------------------

impl DawApp {
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("INSPECTOR").size(10.0).color(theme::TEXT_FAINT));
        ui.separator();

        ui.label(egui::RichText::new("Project").strong());
        ui.horizontal(|ui| {
            ui.label("Tempo");
            ui.label(format!("{:.1} BPM", self.project.tempo));
        });
        ui.horizontal(|ui| {
            ui.label("Key");
            let k = self.project.key;
            ui.colored_label(theme::ACCENT, format!("{} {}", NOTE_NAMES[k.root as usize], k.scale.name()));
        });
        ui.horizontal(|ui| {
            ui.label("Tracks");
            ui.label(format!("{}", self.project.tracks.len()));
        });
        if let Some(a) = &self.audio {
            ui.horizontal(|ui| {
                ui.label("Out");
                ui.label(egui::RichText::new(&a.device_name).size(10.0).color(theme::TEXT_DIM));
            });
            ui.horizontal(|ui| {
                ui.label("SR");
                ui.label(format!("{:.0} Hz", a.sample_rate));
            });
        }
        ui.separator();

        if let Some(track) = self.project.tracks.get_mut(self.selected_track) {
            ui.label(egui::RichText::new("Track").strong());
            ui.add(egui::TextEdit::singleline(&mut track.name).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.label("Type");
                ui.label(format!("{:?}", track.kind));
            });
            ui.horizontal(|ui| {
                ui.label("Color");
                let (r, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                ui.painter().rect_filled(r, Rounding::same(3.0), theme::track_color(track.color));
            });
            ui.horizontal(|ui| {
                ui.label("Devices");
                ui.label(format!("{}", track.devices.len()));
            });
            let id = track.id;
            if ui.button("Delete Track").clicked() {
                self.push_undo();
                self.cmds.push(Command::RemoveTrack { slot: id });
                self.project.tracks.retain(|t| t.id != id);
                if self.selected_track >= self.project.tracks.len() && self.selected_track > 0 {
                    self.selected_track -= 1;
                }
            }
        }

        ui.separator();
        ui.label(egui::RichText::new("Keyboard").strong());
        ui.label(
            egui::RichText::new("Play the selected instrument with A–K (white) and W E T Y U (black). Space toggles play.")
                .size(10.0)
                .color(theme::TEXT_FAINT),
        );
    }
}

// ---------------------------------------------------------------------------
// Browser (right)
// ---------------------------------------------------------------------------

impl DawApp {
    fn browser(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("BROWSER").size(10.0).color(theme::TEXT_FAINT));
        ui.separator();

        ui.label(egui::RichText::new("ADD TRACK").size(9.0).color(theme::TEXT_FAINT));
        for (kind, label) in [
            (TrackKind::Instrument, "Instrument Track"),
            (TrackKind::Audio, "Audio Track"),
            (TrackKind::Effect, "Effect Track"),
        ] {
            if ui.add(browser_item(label, theme::ACCENT)).clicked() {
                self.add_track(kind);
            }
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("INSTRUMENTS").size(9.0).color(theme::TEXT_FAINT));
        if ui.add(browser_item("Polymer", Color32::from_rgb(0x9a, 0x6f, 0xd0))).clicked() {
            self.add_device(DeviceKind::Polymer);
        }
        if ui.add(browser_item("Sampler", Color32::from_rgb(0xe0, 0x8a, 0x3c))).clicked() {
            self.add_sampler(dawcore::model::SampleSource::None);
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("SAMPLES").size(9.0).color(theme::TEXT_FAINT));
        for name in ["Kick", "Snare", "Hat"] {
            if ui.add(browser_item(name, theme::PLAY)).clicked() {
                // add a Sampler preloaded with this built-in onto the selected track
                self.add_sampler(dawcore::model::SampleSource::Builtin(name.into()));
            }
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("AUDIO FX").size(9.0).color(theme::TEXT_FAINT));
        for kind in [DeviceKind::Filter, DeviceKind::Eq, DeviceKind::Drive, DeviceKind::Delay, DeviceKind::Reverb] {
            if ui.add(browser_item(kind.label(), Color32::from_rgb(0x4f, 0xb6, 0xc8))).clicked() {
                self.add_device(kind);
            }
        }
    }

    fn add_sampler(&mut self, source: dawcore::model::SampleSource) {
        self.push_undo();
        let sr = self.sample_rate;
        if let Some(track) = self.project.tracks.get_mut(self.selected_track) {
            let dev = Device::sampler(source);
            let cmd = Command::AddDevice { track: track.id, device: build_device(&dev, sr) };
            track.devices.push(dev);
            self.selected_device = track.devices.len() - 1;
            self.cmds.push(cmd);
        }
    }

    fn add_track(&mut self, kind: TrackKind) {
        self.push_undo();
        let id = self.project.alloc_id();
        let color = TRACK_COLORS[self.project.tracks.len() % TRACK_COLORS.len()];
        let name = format!("Track {}", self.project.tracks.len() + 1);
        let track = Track::new(id, name, kind, color);
        self.cmds.push(Command::AddTrack { slot: id, track: build_track(&track, self.sample_rate) });
        self.project.tracks.push(track);
        self.selected_track = self.project.tracks.len() - 1;
        self.selected_device = 0;
    }

    fn add_device(&mut self, kind: DeviceKind) {
        self.push_undo();
        let sr = self.sample_rate;
        if let Some(track) = self.project.tracks.get_mut(self.selected_track) {
            let dev = Device::new(kind);
            let cmd = Command::AddDevice { track: track.id, device: build_device(&dev, sr) };
            track.devices.push(dev);
            self.cmds.push(cmd);
        }
    }
}

fn browser_item(label: &str, dot: Color32) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, Rounding::same(4.0), theme::PANEL_RAISED);
        }
        let p = ui.painter_at(rect);
        let dot_c = Pos2::new(rect.left() + 10.0, rect.center().y);
        p.rect_filled(Rect::from_center_size(dot_c, Vec2::splat(8.0)), Rounding::same(2.0), dot);
        p.text(
            Pos2::new(rect.left() + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            theme::TEXT,
        );
        resp
    }
}

// ---------------------------------------------------------------------------
// Arranger timeline
// ---------------------------------------------------------------------------

const ARR_HEADER_W: f32 = 140.0;
const ARR_RULER_H: f32 = 26.0;
const ARR_LANE_H: f32 = 58.0;
const ARR_AUTO_H: f32 = 46.0;

impl DawApp {
    fn arranger(&mut self, ui: &mut egui::Ui) {
        let ppb = 1.0 / self.beats_per_px; // pixels per beat
        let (num, _) = self.project.time_sig;
        let bar_beats = num as f64;

        // total timeline length
        let mut max_beat = self.project.loop_end.max(64.0);
        for t in &self.project.tracks {
            for a in &t.arranger {
                max_beat = max_beat.max(a.start + a.duration);
            }
        }
        max_beat = (max_beat / bar_beats).ceil() * bar_beats + bar_beats * 4.0;
        let timeline_w = max_beat as f32 * ppb;
        let n_tracks = self.project.tracks.len();

        // variable lane heights: expanded tracks get an automation sub-lane
        let lane_hs: Vec<f32> = (0..n_tracks)
            .map(|ti| ARR_LANE_H + if self.auto_open.contains(&ti) { ARR_AUTO_H } else { 0.0 })
            .collect();
        let mut lane_tops = Vec::with_capacity(n_tracks);
        let mut acc = 0.0f32;
        for h in &lane_hs {
            lane_tops.push(acc);
            acc += h;
        }
        let lanes_h = ARR_RULER_H + acc;

        ui.horizontal_top(|ui| {
            // ---- track header column ----
            ui.vertical(|ui| {
                ui.allocate_exact_size(Vec2::new(ARR_HEADER_W, ARR_RULER_H), Sense::hover());
                for ti in 0..n_tracks {
                    self.arr_track_header(ui, ti, lane_hs[ti]);
                }
            });

            // ---- timeline ----
            egui::ScrollArea::horizontal().show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(timeline_w, lanes_h), Sense::click_and_drag());
                let p = ui.painter_at(rect);
                let left = rect.left();
                let ruler_bottom = rect.top() + ARR_RULER_H;

                // ruler background
                p.rect_filled(Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), ruler_bottom)), Rounding::ZERO, theme::HEADER);

                // bar lines + numbers
                let mut b = 0.0;
                let mut bar = 1;
                while b <= max_beat {
                    let x = left + b as f32 * ppb;
                    p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, theme::GRID));
                    p.text(Pos2::new(x + 3.0, rect.top() + 8.0), Align2::LEFT_CENTER, format!("{bar}"), FontId::proportional(9.0), theme::TEXT_FAINT);
                    b += bar_beats;
                    bar += 1;
                }

                // loop region
                if self.project.loop_enabled {
                    let lr = Rect::from_min_max(
                        Pos2::new(left + self.project.loop_start as f32 * ppb, rect.top()),
                        Pos2::new(left + self.project.loop_end as f32 * ppb, ruler_bottom),
                    );
                    p.rect_filled(lr, Rounding::ZERO, Color32::from_rgba_unmultiplied(0xff, 0x8a, 0x00, 40));
                    p.rect_stroke(lr, Rounding::ZERO, Stroke::new(1.0, theme::ACCENT));
                }

                // cue markers
                for cue in &self.project.cue_markers {
                    let x = left + cue.position as f32 * ppb;
                    p.text(Pos2::new(x + 2.0, ruler_bottom - 6.0), Align2::LEFT_BOTTOM, &cue.name, FontId::proportional(9.0), theme::TEXT_DIM);
                    p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, ruler_bottom)], Stroke::new(1.0, theme::TEXT_FAINT));
                }

                // lanes + clips
                for ti in 0..n_tracks {
                    let lane_top = ruler_bottom + lane_tops[ti];
                    let lane = Rect::from_min_size(Pos2::new(left, lane_top), Vec2::new(timeline_w, lane_hs[ti]));
                    if ti % 2 == 1 {
                        p.rect_filled(lane, Rounding::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
                    }
                    p.line_segment([Pos2::new(left, lane.bottom()), Pos2::new(rect.right(), lane.bottom())], Stroke::new(1.0, theme::BORDER));

                    let color = theme::track_color(self.project.tracks[ti].color);
                    for a in &self.project.tracks[ti].arranger {
                        let cx = left + a.start as f32 * ppb;
                        let cw = (a.duration as f32 * ppb).max(4.0);
                        let cr = Rect::from_min_size(Pos2::new(cx, lane_top + 3.0), Vec2::new(cw, ARR_LANE_H - 6.0));
                        p.rect_filled(cr, Rounding::same(3.0), color);
                        p.rect_stroke(cr, Rounding::same(3.0), Stroke::new(1.0, Color32::from_black_alpha(120)));
                        p.text(Pos2::new(cr.left() + 5.0, cr.top() + 9.0), Align2::LEFT_CENTER, &a.clip.name, FontId::proportional(10.0), Color32::from_black_alpha(200));
                        // mini notes
                        if !a.clip.notes.is_empty() && a.clip.length > 0.0 {
                            let lo = a.clip.notes.iter().map(|n| n.pitch).min().unwrap() as f32;
                            let hi = a.clip.notes.iter().map(|n| n.pitch).max().unwrap() as f32 + 1.0;
                            let range = (hi - lo).max(1.0);
                            let reps = (a.duration / a.clip.length).ceil() as i32;
                            for r in 0..reps {
                                for n in &a.clip.notes {
                                    let nb = r as f64 * a.clip.length + n.start;
                                    if nb >= a.duration { break; }
                                    let nx = cr.left() + (nb / a.duration) as f32 * cr.width();
                                    let nw = ((n.length / a.duration) as f32 * cr.width()).max(1.5);
                                    let ny = cr.bottom() - 4.0 - (n.pitch as f32 - lo) / range * (cr.height() - 16.0);
                                    p.rect_filled(Rect::from_min_size(Pos2::new(nx, ny), Vec2::new(nw, 2.0)), Rounding::ZERO, Color32::from_black_alpha(110));
                                }
                            }
                        }
                    }

                    // audio clips (waveform-style)
                    for a in &self.project.tracks[ti].audio_clips {
                        let cx = left + a.start as f32 * ppb;
                        let cw = (a.duration as f32 * ppb).max(4.0);
                        let cr = Rect::from_min_size(Pos2::new(cx, lane_top + 3.0), Vec2::new(cw, ARR_LANE_H - 6.0));
                        let col = theme::track_color(a.color);
                        p.rect_filled(cr, Rounding::same(3.0), col.gamma_multiply(0.7));
                        p.rect_stroke(cr, Rounding::same(3.0), Stroke::new(1.0, col));
                        // stylised waveform bars
                        let mid = cr.center().y;
                        let bars = (cr.width() / 3.0) as i32;
                        for k in 0..bars {
                            let fx = cr.left() + 2.0 + k as f32 * 3.0;
                            let ph = (k as f32 * 0.7).sin().abs() * 0.5 + (k as f32 * 0.27).cos().abs() * 0.4;
                            let h = ph * (cr.height() * 0.4);
                            p.line_segment([Pos2::new(fx, mid - h), Pos2::new(fx, mid + h)], Stroke::new(1.0, Color32::from_black_alpha(120)));
                        }
                        p.text(Pos2::new(cr.left() + 4.0, cr.top() + 8.0), Align2::LEFT_CENTER, &format!("🔊 {}", a.name), FontId::proportional(9.0), Color32::from_black_alpha(220));
                    }

                    // ---- automation sub-lane (volume) ----
                    if self.auto_open.contains(&ti) {
                        let auto_top = lane_top + ARR_LANE_H;
                        let ar = Rect::from_min_size(Pos2::new(left, auto_top), Vec2::new(timeline_w, ARR_AUTO_H));
                        p.rect_filled(ar, Rounding::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 60));
                        p.line_segment([ar.left_top(), ar.right_top()], Stroke::new(1.0, theme::BORDER));
                        let val_y = |v: f32| auto_top + (1.0 - v) * (ARR_AUTO_H - 8.0) + 4.0;
                        let pts = &self.project.tracks[ti].volume_automation;
                        let curve = Stroke::new(1.5, theme::ACCENT);
                        if pts.is_empty() {
                            // flat line at the fader value
                            let y = val_y(self.project.tracks[ti].volume);
                            p.line_segment([Pos2::new(left, y), Pos2::new(rect.right(), y)], Stroke::new(1.0, theme::TEXT_FAINT));
                        } else {
                            // segment before the first point
                            let first = &pts[0];
                            let fy = val_y(first.value);
                            p.line_segment([Pos2::new(left, fy), Pos2::new(left + first.beat as f32 * ppb, fy)], curve);
                            for w in pts.windows(2) {
                                let (a, b) = (&w[0], &w[1]);
                                let (ax, bx) = (left + a.beat as f32 * ppb, left + b.beat as f32 * ppb);
                                let (ay, by) = (val_y(a.value), val_y(b.value));
                                if a.hold {
                                    // v6 hold: flat until the next point, then step
                                    p.line_segment([Pos2::new(ax, ay), Pos2::new(bx, ay)], curve);
                                    p.line_segment([Pos2::new(bx, ay), Pos2::new(bx, by)], curve);
                                } else {
                                    p.line_segment([Pos2::new(ax, ay), Pos2::new(bx, by)], curve);
                                }
                            }
                            // segment after the last point
                            let last = pts.last().unwrap();
                            let ly = val_y(last.value);
                            p.line_segment([Pos2::new(left + last.beat as f32 * ppb, ly), Pos2::new(rect.right(), ly)], curve);
                            // point handles: square = hold, circle = ramp
                            for pt in pts {
                                let c = Pos2::new(left + pt.beat as f32 * ppb, val_y(pt.value));
                                if pt.hold {
                                    p.rect_filled(Rect::from_center_size(c, Vec2::splat(7.0)), Rounding::ZERO, theme::ACCENT);
                                } else {
                                    p.circle_filled(c, 3.5, theme::ACCENT);
                                }
                            }
                        }
                    }
                }

                // playhead
                let pos = self.position_beats();
                let phx = left + pos as f32 * ppb;
                p.line_segment([Pos2::new(phx, rect.top()), Pos2::new(phx, rect.bottom())], Stroke::new(2.0, theme::ACCENT));

                self.arr_interact(rect, &resp, ppb, max_beat, &lane_tops, &lane_hs);
            });
        });
    }

    fn arr_track_header(&mut self, ui: &mut egui::Ui, ti: usize, height: f32) {
        let (color, name, mute, solo, is_fx) = {
            let t = &self.project.tracks[ti];
            (t.color, t.name.clone(), t.mute, t.solo, t.kind == TrackKind::Effect)
        };
        let selected = ti == self.selected_track;
        let auto_on = self.auto_open.contains(&ti);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ARR_HEADER_W, height), Sense::click());
        ui.painter().rect_filled(rect, Rounding::ZERO, if selected { theme::PANEL_ALT } else { theme::PANEL });
        ui.painter().rect_filled(Rect::from_min_size(rect.left_top(), Vec2::new(3.0, height)), Rounding::ZERO, theme::track_color(color));
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::BORDER));
        ui.painter().text(Pos2::new(rect.left() + 10.0, rect.top() + 14.0), Align2::LEFT_CENTER, &name, FontId::proportional(12.0), theme::TEXT);
        if is_fx {
            ui.painter().text(Pos2::new(rect.right() - 8.0, rect.top() + 14.0), Align2::RIGHT_CENTER, "FX", FontId::proportional(9.0), theme::TEXT_FAINT);
        }
        if resp.clicked() {
            self.selected_track = ti;
            self.selected_device = 0;
        }
        // chips anchored to the clip row, not the (taller) automation lane
        let chip_y = rect.top() + ARR_LANE_H - 22.0;
        let mb = Rect::from_min_size(Pos2::new(rect.left() + 10.0, chip_y), Vec2::new(18.0, 14.0));
        let sb = Rect::from_min_size(Pos2::new(rect.left() + 32.0, chip_y), Vec2::new(18.0, 14.0));
        let ab = Rect::from_min_size(Pos2::new(rect.left() + 54.0, chip_y), Vec2::new(18.0, 14.0));
        if self.toggle_chip(ui, mb, "M", mute, Color32::from_rgb(0xd0, 0xa0, 0x40)) {
            self.set_mute(ti, !mute);
        }
        if self.toggle_chip(ui, sb, "S", solo, theme::ACCENT) {
            self.set_solo(ti, !solo);
        }
        if self.toggle_chip(ui, ab, "A", auto_on, theme::PLAY) {
            if auto_on {
                self.auto_open.remove(&ti);
            } else {
                self.auto_open.insert(ti);
            }
        }
        if auto_on {
            ui.painter().text(
                Pos2::new(rect.left() + 10.0, rect.top() + ARR_LANE_H + 12.0),
                Align2::LEFT_CENTER,
                "Volume",
                FontId::proportional(9.0),
                theme::TEXT_FAINT,
            );
        }
    }

    /// Locate which track lane (and whether the automation sub-lane) contains `y`.
    fn lane_at(lane_tops: &[f32], lane_hs: &[f32], local_y: f32) -> Option<(usize, bool)> {
        for ti in 0..lane_tops.len() {
            let top = lane_tops[ti];
            if local_y >= top && local_y < top + lane_hs[ti] {
                return Some((ti, local_y - top > ARR_LANE_H));
            }
        }
        None
    }

    /// Nearest automation point within grab range, if any.
    fn auto_hit(&self, ti: usize, beat: f64, value: f32, ppb: f32) -> Option<usize> {
        let beat_tol = (8.0 / ppb) as f64;
        self.project.tracks[ti]
            .volume_automation
            .iter()
            .enumerate()
            .filter(|(_, p)| (p.beat - beat).abs() < beat_tol && (p.value - value).abs() < 0.18)
            .min_by(|a, b| {
                let da = (a.1.beat - beat).abs();
                let db = (b.1.beat - beat).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }

    fn arr_interact(
        &mut self,
        rect: Rect,
        resp: &egui::Response,
        ppb: f32,
        max_beat: f64,
        lane_tops: &[f32],
        lane_hs: &[f32],
    ) {
        let snap = |beat: f64| (beat).max(0.0).round();
        let snap_q = |beat: f64| ((beat / 0.25).round() * 0.25).max(0.0); // 1/16 for automation
        let pointer = resp.interact_pointer_pos();
        let ruler_bottom = rect.top() + ARR_RULER_H;
        let auto_val = |local_y_in_lane: f32| -> f32 {
            let y = local_y_in_lane - ARR_LANE_H; // within the automation sub-lane
            (1.0 - (y - 4.0) / (ARR_AUTO_H - 8.0)).clamp(0.0, 1.0)
        };

        if resp.drag_started() || resp.clicked() || resp.secondary_clicked() || resp.double_clicked() {
            if let Some(pos) = pointer {
                let beat = ((pos.x - rect.left()) / ppb) as f64;
                if pos.y < ruler_bottom {
                    // ruler: start setting the loop region by dragging
                    if resp.drag_started() {
                        self.loop_anchor = Some(snap(beat));
                    }
                } else if let Some((lane, in_auto)) = Self::lane_at(lane_tops, lane_hs, pos.y - ruler_bottom) {
                    let local_y = pos.y - ruler_bottom - lane_tops[lane];
                    if in_auto {
                        // ---- automation sub-lane ----
                        let value = auto_val(local_y);
                        let hit = self.auto_hit(lane, beat, value, ppb);
                        if resp.double_clicked() {
                            if let Some(idx) = hit {
                                self.push_undo();
                                let p = &mut self.project.tracks[lane].volume_automation[idx];
                                p.hold = !p.hold;
                                self.sync_automation(lane);
                            }
                        } else if resp.secondary_clicked() {
                            if let Some(idx) = hit {
                                self.push_undo();
                                self.project.tracks[lane].volume_automation.remove(idx);
                                self.sync_automation(lane);
                            }
                        } else if resp.drag_started() {
                            if let Some(idx) = hit {
                                self.push_undo();
                                self.auto_drag = Some((lane, idx));
                            }
                        } else if resp.clicked() && hit.is_none() {
                            self.push_undo();
                            self.project.tracks[lane].volume_automation.push(AutomationPoint {
                                beat: snap_q(beat),
                                value,
                                hold: false,
                            });
                            self.sync_automation(lane);
                        }
                    } else {
                        // ---- clip row: hit-test MIDI then audio clips ----
                        let mut hit = None;
                        for (i, a) in self.project.tracks[lane].arranger.iter().enumerate() {
                            if beat >= a.start && beat <= a.start + a.duration {
                                let near_end = beat > a.start + a.duration - (8.0 / ppb) as f64;
                                hit = Some((i, near_end, beat - a.start, false));
                                break;
                            }
                        }
                        if hit.is_none() {
                            for (i, a) in self.project.tracks[lane].audio_clips.iter().enumerate() {
                                if beat >= a.start && beat <= a.start + a.duration {
                                    let near_end = beat > a.start + a.duration - (8.0 / ppb) as f64;
                                    hit = Some((i, near_end, beat - a.start, true));
                                    break;
                                }
                            }
                        }
                        if let Some((idx, near_end, grab, audio)) = hit {
                            if resp.secondary_clicked() {
                                self.push_undo();
                                if audio {
                                    self.project.tracks[lane].audio_clips.remove(idx);
                                } else {
                                    self.project.tracks[lane].arranger.remove(idx);
                                }
                                self.sync_track(lane);
                            } else if resp.drag_started() {
                                self.push_undo();
                                self.selected_track = lane;
                                self.arr_drag = Some(ArrDrag {
                                    track: lane,
                                    index: idx,
                                    mode: if near_end { ArrDragMode::Resize } else { ArrDragMode::Move },
                                    grab,
                                    audio,
                                });
                            }
                        }
                    }
                }
            }
        }

        if resp.dragged() {
            if let Some(pos) = pointer {
                let beat = ((pos.x - rect.left()) / ppb) as f64;
                if let Some(anchor) = self.loop_anchor {
                    let b = snap(beat);
                    self.project.loop_start = anchor.min(b);
                    self.project.loop_end = (anchor.max(b)).max(anchor.min(b) + 1.0);
                } else if let Some((tk, idx)) = self.auto_drag {
                    if idx < self.project.tracks[tk].volume_automation.len() {
                        let local_y = pos.y - ruler_bottom - lane_tops[tk];
                        let p = &mut self.project.tracks[tk].volume_automation[idx];
                        p.beat = snap_q(beat).min(max_beat);
                        p.value = auto_val(local_y);
                    }
                } else if let Some(d) = self.arr_drag.as_ref().map(|d| (d.track, d.index, d.mode, d.grab, d.audio)) {
                    let (tk, idx, mode, grab, audio) = d;
                    // (start, duration) accessors for whichever clip kind is dragged
                    let len = if audio {
                        self.project.tracks[tk].audio_clips.len()
                    } else {
                        self.project.tracks[tk].arranger.len()
                    };
                    if idx < len {
                        let (start_ref, dur_ref): (&mut f64, &mut f64) = if audio {
                            let c = &mut self.project.tracks[tk].audio_clips[idx];
                            (&mut c.start, &mut c.duration)
                        } else {
                            let c = &mut self.project.tracks[tk].arranger[idx];
                            (&mut c.start, &mut c.duration)
                        };
                        match mode {
                            ArrDragMode::Move => {
                                *start_ref = snap(beat - grab).clamp(0.0, max_beat);
                            }
                            ArrDragMode::Resize => {
                                *dur_ref = snap(beat - *start_ref).max(1.0);
                            }
                        }
                    }
                }
            }
        }

        if resp.drag_stopped() {
            if self.loop_anchor.take().is_some() {
                self.push_loop();
            }
            if let Some((tk, _)) = self.auto_drag.take() {
                self.sync_automation(tk);
            }
            if let Some(d) = self.arr_drag.take() {
                self.sync_track(d.track);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clip launcher
// ---------------------------------------------------------------------------

const TRACK_W: f32 = 124.0;
const HEAD_H: f32 = 46.0;
const CELL_H: f32 = 52.0;
const SCENE_W: f32 = 30.0;

impl DawApp {
    fn clip_launcher(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // scene launch column
                ui.vertical(|ui| {
                    ui.allocate_exact_size(Vec2::new(SCENE_W, HEAD_H), Sense::hover());
                    for s in 0..self.project.scenes.len() {
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(SCENE_W, CELL_H), Sense::click());
                        let hot = resp.hovered();
                        ui.painter().rect_filled(rect, Rounding::ZERO, if hot { theme::PANEL } else { theme::HEADER });
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            format!("▶{}", s + 1),
                            FontId::proportional(11.0),
                            if hot { theme::ACCENT } else { theme::TEXT_FAINT },
                        );
                        if resp.clicked() {
                            self.cmds.push(Command::Play);
                            self.cmds.push(Command::LaunchScene(s));
                        }
                    }
                });

                // track columns
                let track_count = self.project.tracks.len();
                for ti in 0..track_count {
                    self.track_column(ui, ti);
                }

                // add-track button
                ui.vertical(|ui| {
                    ui.add_space(HEAD_H + 4.0);
                    if ui.button("+").clicked() {
                        self.add_track(TrackKind::Instrument);
                    }
                });
            });
        });
    }

    fn track_column(&mut self, ui: &mut egui::Ui, ti: usize) {
        ui.vertical(|ui| {
            let (tid, color, name, mute, solo) = {
                let t = &self.project.tracks[ti];
                (t.id, t.color, t.name.clone(), t.mute, t.solo)
            };
            let selected = ti == self.selected_track;

            // header
            let (hrect, hresp) = ui.allocate_exact_size(Vec2::new(TRACK_W, HEAD_H), Sense::click());
            ui.painter().rect_filled(hrect, Rounding::ZERO, if selected { theme::PANEL_ALT } else { theme::PANEL });
            ui.painter().rect_filled(
                Rect::from_min_size(hrect.left_top(), Vec2::new(3.0, HEAD_H)),
                Rounding::ZERO,
                theme::track_color(color),
            );
            ui.painter().text(
                Pos2::new(hrect.left() + 10.0, hrect.top() + 12.0),
                Align2::LEFT_CENTER,
                &name,
                FontId::proportional(12.0),
                theme::TEXT,
            );
            if hresp.clicked() {
                self.selected_track = ti;
                self.selected_device = 0;
            }
            // M / S mini buttons inside the header
            let mb = Rect::from_min_size(Pos2::new(hrect.left() + 8.0, hrect.bottom() - 18.0), Vec2::new(18.0, 14.0));
            let sb = Rect::from_min_size(Pos2::new(hrect.left() + 30.0, hrect.bottom() - 18.0), Vec2::new(18.0, 14.0));
            if self.toggle_chip(ui, mb, "M", mute, Color32::from_rgb(0xd0, 0xa0, 0x40)) {
                self.set_mute(ti, !mute);
            }
            if self.toggle_chip(ui, sb, "S", solo, theme::ACCENT) {
                self.set_solo(ti, !solo);
            }

            // clip cells
            for scene in 0..self.project.scenes.len() {
                self.clip_cell(ui, ti, tid, scene);
            }
        });
    }

    fn toggle_chip(&self, ui: &mut egui::Ui, rect: Rect, label: &str, on: bool, color: Color32) -> bool {
        let resp = ui.interact(rect, ui.id().with(("chip", rect.left() as i32, rect.top() as i32, label)), Sense::click());
        let bg = if on { color } else { theme::PANEL_RAISED };
        ui.painter().rect_filled(rect, Rounding::same(2.0), bg);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.0),
            if on { Color32::BLACK } else { theme::TEXT_DIM },
        );
        resp.clicked()
    }

    fn clip_cell(&mut self, ui: &mut egui::Ui, ti: usize, tid: usize, scene: usize) {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(TRACK_W, CELL_H), Sense::click());
        let inner = rect.shrink(4.0);
        let has_clip = self.project.tracks[ti].clips[scene].is_some();

        if !has_clip {
            ui.painter().rect(inner, Rounding::same(4.0), theme::SLOT_EMPTY, Stroke::new(1.0, theme::PANEL_ALT));
            if resp.hovered() {
                ui.painter().text(inner.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), theme::TEXT_FAINT);
            }
            if resp.clicked() {
                self.selected_track = ti;
                self.create_clip(ti, scene);
            }
            return;
        }

        let (clip_color, clip_name, notes_summary, clip_len) = {
            let clip = self.project.tracks[ti].clips[scene].as_ref().unwrap();
            (clip.color, clip.name.clone(), summarize(&clip.notes, clip.length), clip.length)
        };
        let _ = clip_len;
        let active = self.active_scene(tid) == scene as i32 && self.playing();

        ui.painter().rect_filled(inner, Rounding::same(4.0), theme::track_color(clip_color));
        if active {
            ui.painter().rect_stroke(inner, Rounding::same(4.0), Stroke::new(2.0, theme::PLAY));
        }
        ui.painter().text(
            Pos2::new(inner.left() + 6.0, inner.top() + 9.0),
            Align2::LEFT_CENTER,
            &clip_name,
            FontId::proportional(11.0),
            Color32::from_rgba_premultiplied(0, 0, 0, 200),
        );
        ui.painter().text(
            Pos2::new(inner.left() + 6.0, inner.bottom() - 8.0),
            Align2::LEFT_CENTER,
            if active { "▶ playing" } else { "▷" },
            FontId::proportional(9.0),
            Color32::from_rgba_premultiplied(0, 0, 0, 160),
        );
        // mini note preview
        for (x, y, w) in &notes_summary {
            let nr = Rect::from_min_size(
                Pos2::new(inner.left() + 4.0 + x * (inner.width() - 8.0), inner.top() + 16.0 + y * (inner.height() - 22.0)),
                Vec2::new((w * (inner.width() - 8.0)).max(2.0), 2.0),
            );
            ui.painter().rect_filled(nr, Rounding::ZERO, Color32::from_rgba_premultiplied(0, 0, 0, 120));
        }

        if resp.clicked() {
            self.selected_track = ti;
            self.cmds.push(Command::Play);
            self.cmds.push(Command::LaunchClip { track: tid, scene });
        }
        if resp.double_clicked() {
            self.editing = Some((ti, scene));
        }
        if resp.secondary_clicked() {
            self.push_undo();
            self.cmds.push(Command::SetClip { track: tid, scene, clip: None });
            self.project.tracks[ti].clips[scene] = None;
        }
    }

    fn create_clip(&mut self, ti: usize, scene: usize) {
        self.push_undo();
        let (tid, color) = {
            let t = &self.project.tracks[ti];
            (t.id, t.color)
        };
        let clip = Clip::new("Clip", color);
        self.cmds.push(Command::SetClip { track: tid, scene, clip: Some(build_clip(&clip)) });
        self.project.tracks[ti].clips[scene] = Some(clip);
        self.editing = Some((ti, scene));
    }

    fn set_mute(&mut self, ti: usize, value: bool) {
        let tid = self.project.tracks[ti].id;
        self.project.tracks[ti].mute = value;
        self.cmds.push(Command::SetTrackMute { track: tid, value });
    }
    fn set_solo(&mut self, ti: usize, value: bool) {
        let tid = self.project.tracks[ti].id;
        self.project.tracks[ti].solo = value;
        self.cmds.push(Command::SetTrackSolo { track: tid, value });
    }
}

fn summarize(notes: &[Note], len: f64) -> Vec<(f32, f32, f32)> {
    if notes.is_empty() {
        return Vec::new();
    }
    let lo = notes.iter().map(|n| n.pitch).min().unwrap() as f32;
    let hi = notes.iter().map(|n| n.pitch).max().unwrap() as f32 + 1.0;
    let range = (hi - lo).max(1.0);
    notes
        .iter()
        .map(|n| {
            (
                (n.start / len) as f32,
                1.0 - (n.pitch as f32 - lo) / range,
                (n.length / len) as f32,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Mixer
// ---------------------------------------------------------------------------

impl DawApp {
    fn mixer(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let n = self.project.tracks.len();
                for ti in 0..n {
                    self.channel_strip(ui, ti);
                }
                self.master_strip(ui);
            });
        });
    }

    fn channel_strip(&mut self, ui: &mut egui::Ui, ti: usize) {
        let (tid, color, name, mut volume, mut pan, mute, solo, is_fx) = {
            let t = &self.project.tracks[ti];
            (t.id, t.color, t.name.clone(), t.volume, t.pan, t.mute, t.solo, t.kind == TrackKind::Effect)
        };
        let selected = ti == self.selected_track;

        // effect (return) tracks reachable from this strip's sends
        let fx_targets: Vec<(usize, String)> = if is_fx {
            Vec::new()
        } else {
            self.project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Effect)
                .map(|t| (t.id, t.name.clone()))
                .collect()
        };
        let n_sends = fx_targets.len();
        let frame_h = 264.0 + n_sends as f32 * 16.0;

        ui.vertical(|ui| {
            let (frame, _) = ui.allocate_exact_size(Vec2::new(80.0, frame_h), Sense::hover());
            let border = if is_fx { theme::PLAY } else if selected { theme::ACCENT_DIM } else { theme::BORDER };
            ui.painter().rect(frame, Rounding::same(4.0), theme::PANEL_ALT, Stroke::new(1.0, border));
            ui.painter().rect_filled(Rect::from_min_size(frame.left_top(), Vec2::new(80.0, 3.0)), Rounding::ZERO, theme::track_color(color));
            ui.painter().text(Pos2::new(frame.center().x, frame.top() + 14.0), Align2::CENTER_CENTER, &name, FontId::proportional(11.0), theme::TEXT);

            // pan slider
            let pan_rect = Rect::from_min_size(Pos2::new(frame.left() + 8.0, frame.top() + 26.0), Vec2::new(64.0, 14.0));
            if ui.put(pan_rect, egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false)).changed() {
                self.project.tracks[ti].pan = pan;
                self.cmds.push(Command::SetTrackPan { track: tid, value: pan });
            }

            // post-fader sends to effect tracks
            for (k, (dest, _dest_name)) in fx_targets.iter().enumerate() {
                let y = frame.top() + 44.0 + k as f32 * 16.0;
                let label_rect = Rect::from_min_size(Pos2::new(frame.left() + 6.0, y), Vec2::new(18.0, 14.0));
                ui.painter().text(label_rect.left_center(), Align2::LEFT_CENTER, format!("S{}", k + 1), FontId::proportional(9.0), theme::PLAY);
                let send_rect = Rect::from_min_size(Pos2::new(frame.left() + 24.0, y), Vec2::new(50.0, 14.0));
                let mut level = self.project.tracks[ti]
                    .sends
                    .iter()
                    .find(|(d, _)| d == dest)
                    .map(|(_, l)| *l)
                    .unwrap_or(0.0);
                if ui
                    .put(send_rect, egui::Slider::new(&mut level, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    let sends = &mut self.project.tracks[ti].sends;
                    if let Some(s) = sends.iter_mut().find(|(d, _)| d == dest) {
                        s.1 = level;
                    } else {
                        sends.push((*dest, level));
                    }
                    self.sync_sends(ti);
                }
            }

            // fader + meter
            let fader_top = frame.top() + 48.0 + n_sends as f32 * 16.0;
            let fader_h = frame.bottom() - 30.0 - fader_top;
            let fader_rect = Rect::from_min_size(Pos2::new(frame.left() + 18.0, fader_top), Vec2::new(20.0, fader_h));
            if widgets::fader(ui, fader_rect, &mut volume) {
                self.project.tracks[ti].volume = volume;
                self.cmds.push(Command::SetTrackGain { track: tid, value: volume });
            }
            let meter_rect = Rect::from_min_size(Pos2::new(frame.left() + 46.0, fader_top), Vec2::new(10.0, fader_h));
            widgets::meter(ui, meter_rect, self.track_peak(tid));

            // M / S
            let mb = Rect::from_min_size(Pos2::new(frame.left() + 10.0, frame.bottom() - 24.0), Vec2::new(28.0, 18.0));
            let sb = Rect::from_min_size(Pos2::new(frame.left() + 42.0, frame.bottom() - 24.0), Vec2::new(28.0, 18.0));
            if self.toggle_chip(ui, mb, "M", mute, Color32::from_rgb(0xd0, 0xa0, 0x40)) {
                self.set_mute(ti, !mute);
            }
            if self.toggle_chip(ui, sb, "S", solo, theme::ACCENT) {
                self.set_solo(ti, !solo);
            }
        });
    }

    fn master_strip(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            let (frame, _) = ui.allocate_exact_size(Vec2::new(80.0, 260.0), Sense::hover());
            ui.painter().rect(frame, Rounding::same(4.0), theme::PANEL_ALT, Stroke::new(1.0, theme::ACCENT_DIM));
            ui.painter().rect_filled(Rect::from_min_size(frame.left_top(), Vec2::new(80.0, 3.0)), Rounding::ZERO, theme::ACCENT);
            ui.painter().text(Pos2::new(frame.center().x, frame.top() + 14.0), Align2::CENTER_CENTER, "Master", FontId::proportional(11.0), theme::TEXT);

            let fader_rect = Rect::from_min_size(Pos2::new(frame.left() + 18.0, frame.top() + 48.0), Vec2::new(20.0, 160.0));
            let mut v = self.master_volume;
            if widgets::fader(ui, fader_rect, &mut v) {
                self.master_volume = v;
                // master gain handled by track sum + limiter; expose later if needed
            }
            let meter_rect = Rect::from_min_size(Pos2::new(frame.left() + 46.0, frame.top() + 48.0), Vec2::new(10.0, 160.0));
            widgets::meter(ui, meter_rect, self.master_peak());
        });
    }
}

// ---------------------------------------------------------------------------
// Device panel
// ---------------------------------------------------------------------------

impl DawApp {
    fn device_panel(&mut self, ui: &mut egui::Ui) {
        let track_name = self.project.tracks.get(self.selected_track).map(|t| t.name.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("DEVICE CHAIN").size(10.0).color(theme::TEXT_FAINT));
            ui.label(egui::RichText::new(format!("— {track_name}")).size(10.0).color(theme::TEXT_DIM));
            if self.playing() {
                let v = self.audio.as_ref().map(|a| a.handle.shared.active_voices.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0);
                ui.label(egui::RichText::new(format!("voices: {v}")).size(10.0).color(theme::TEXT_FAINT));
            }
        });
        ui.separator();

        let device_count = self.project.tracks.get(self.selected_track).map(|t| t.devices.len()).unwrap_or(0);
        if device_count == 0 {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No devices — add one from the Browser ▸").color(theme::TEXT_FAINT));
            });
            return;
        }

        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for di in 0..device_count {
                    self.device_card(ui, di);
                }
            });
        });
    }

    fn device_card(&mut self, ui: &mut egui::Ui, di: usize) {
        let ti = self.selected_track;
        let (tid, kind, enabled, params, modulators, sample) = {
            let t = &self.project.tracks[ti];
            let d = &t.devices[di];
            (t.id, d.kind, d.enabled, d.params.clone(), d.modulators.clone(), d.sample.clone())
        };

        // ring amount/colour for a given parameter (from this device's modulators)
        let ring_for = |pi: usize| -> Option<(f32, Color32)> {
            let mut total = 0.0;
            let mut color = None;
            for m in &modulators {
                for r in &m.routes {
                    if r.param == pi {
                        total += r.amount;
                        if color.is_none() {
                            let c = m.kind.color();
                            color = Some(Color32::from_rgb(c[0], c[1], c[2]));
                        }
                    }
                }
            }
            color.map(|c| (total, c))
        };
        // which modulator (if any) is in routing/assign mode on this device
        let assigning: Option<usize> = match self.assign_mod {
            Some((at, ad, am)) if at == ti && ad == di => Some(am),
            _ => None,
        };

        let labels = kind.params();
        // card is sized to hold one or two rows of knobs
        let knob_w = 54.0;
        let cols = labels.len().min(6).max(1);
        let rows = labels.len().div_ceil(6);
        let card_w = (cols as f32 * knob_w + 24.0).max(160.0);
        let card_h = 34.0 + rows as f32 * 62.0 + 56.0; // + modulator row

        egui::Frame::none()
            .fill(theme::PANEL_RAISED)
            .stroke(Stroke::new(1.0, if di == self.selected_device { theme::ACCENT_DIM } else { theme::BORDER }))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                // The strip ui is horizontal; force the card's content to stack.
                ui.vertical(|ui| {
                ui.set_width(card_w);
                ui.set_min_height(card_h);

                // header
                ui.horizontal(|ui| {
                    let (led, lresp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::click());
                    ui.painter().circle_filled(led.center(), 5.0, if enabled { theme::PLAY } else { Color32::from_gray(60) });
                    if lresp.clicked() {
                        self.project.tracks[ti].devices[di].enabled = !enabled;
                        self.cmds.push(Command::SetDeviceEnabled { track: tid, device: di, value: !enabled });
                    }
                    ui.label(egui::RichText::new(kind.label()).strong());
                    if ui.add(egui::Button::new("✕").small()).clicked() {
                        self.cmds.push(Command::RemoveDevice { track: tid, device: di });
                        self.project.tracks[ti].devices.remove(di);
                        if self.selected_device >= di && self.selected_device > 0 {
                            self.selected_device -= 1;
                        }
                    }
                });
                ui.separator();

                if di >= self.project.tracks[ti].devices.len() {
                    return; // was just removed this frame
                }

                // sample picker for the Sampler instrument
                if kind == DeviceKind::Sampler {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Sample").size(9.0).color(theme::TEXT_FAINT));
                        let cur = sample.label();
                        egui::ComboBox::from_id_salt(("samp", ti, di))
                            .width(96.0)
                            .selected_text(cur)
                            .show_ui(ui, |ui| {
                                for name in ["Kick", "Snare", "Hat"] {
                                    if ui.selectable_label(false, name).clicked() {
                                        self.push_undo();
                                        self.project.tracks[ti].devices[di].sample =
                                            dawcore::model::SampleSource::Builtin(name.into());
                                        self.sync_track(ti);
                                    }
                                }
                            });
                        if ui.button("Load WAV…").clicked() {
                            // load from the File window's path field
                            let path = self.file_path.clone();
                            self.push_undo();
                            self.project.tracks[ti].devices[di].sample =
                                dawcore::model::SampleSource::File(path);
                            self.sync_track(ti);
                            self.status = Some("Loaded sample from File path".into());
                        }
                    });
                }

                // Poly Grid: open the modular graph editor
                if kind == DeviceKind::PolyGrid {
                    let n_nodes = self.project.tracks[ti].devices[di]
                        .grid.as_ref().map(|g| g.modules.len()).unwrap_or(0);
                    ui.horizontal(|ui| {
                        if ui.button("✎ Open Grid Editor").clicked() {
                            self.grid_edit = Some((ti, di));
                        }
                        ui.label(egui::RichText::new(format!("{n_nodes} modules")).size(10.0).color(theme::TEXT_FAINT));
                    });
                }

                // parameter knobs / dropdowns, laid out in rows of up to six
                for chunk_start in (0..labels.len()).step_by(6) {
                    ui.horizontal(|ui| {
                        for pi in chunk_start..(chunk_start + 6).min(labels.len()) {
                            if pi >= params.len() {
                                break;
                            }
                            let label = labels[pi];
                            if let Some(options) = kind.options(pi) {
                                let mut idx = params[pi] as usize;
                                ui.allocate_ui(Vec2::new(knob_w, 56.0), |ui| {
                                    ui.vertical_centered(|ui| {
                                        egui::ComboBox::from_id_salt(("opt", ti, di, pi))
                                            .width(knob_w)
                                            .selected_text(options.get(idx).copied().unwrap_or(""))
                                            .show_ui(ui, |ui| {
                                                for (oi, o) in options.iter().enumerate() {
                                                    ui.selectable_value(&mut idx, oi, *o);
                                                }
                                            });
                                        ui.label(egui::RichText::new(label).size(9.0).color(theme::TEXT_DIM));
                                    });
                                });
                                if idx as f32 != params[pi] {
                                    self.project.tracks[ti].devices[di].params[pi] = idx as f32;
                                    self.cmds.push(Command::SetParam { track: tid, device: di, param: pi, value: idx as f32 });
                                }
                            } else if let Some(mi) = assigning {
                                // routing mode: drag adjusts this modulator's amount on the param
                                let mut amount = modulators
                                    .get(mi)
                                    .and_then(|m| m.routes.iter().find(|r| r.param == pi))
                                    .map(|r| r.amount)
                                    .unwrap_or(0.0);
                                let c = modulators[mi].kind.color();
                                let color = Color32::from_rgb(c[0], c[1], c[2]);
                                if widgets::knob_assign(ui, params[pi], &mut amount, label, color) {
                                    self.set_route(ti, di, mi, pi, amount);
                                }
                            } else {
                                let mut v = params[pi];
                                if widgets::knob(ui, &mut v, label, ring_for(pi)) {
                                    self.project.tracks[ti].devices[di].params[pi] = v;
                                    self.cmds.push(Command::SetParam { track: tid, device: di, param: pi, value: v });
                                }
                            }
                        }
                    });
                }

                // ---- modulator slots (Bitwig's unified modulation system) ----
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MOD").size(9.0).color(theme::TEXT_FAINT));
                    for (mi, m) in modulators.iter().enumerate() {
                        let c = m.kind.color();
                        let col = Color32::from_rgb(c[0], c[1], c[2]);
                        let active = assigning == Some(mi);
                        let label = if active { format!("◉ {}", m.kind.label()) } else { m.kind.label().to_string() };
                        let chip = egui::Button::new(egui::RichText::new(label).size(10.0).color(if active { Color32::BLACK } else { col }))
                            .fill(if active { col } else { theme::PANEL_ALT })
                            .stroke(Stroke::new(1.0, col));
                        if ui.add(chip).clicked() {
                            self.assign_mod = if active { None } else { Some((ti, di, mi)) };
                        }
                    }
                    // add-modulator menu
                    ui.menu_button("+", |ui| {
                        for k in ModKind::ALL {
                            if ui.button(k.label()).clicked() {
                                self.project.tracks[ti].devices[di].modulators.push(Modulator::new(k));
                                self.sync_mods(ti);
                                ui.close_menu();
                            }
                        }
                    });
                });

                // selected-modulator controls (rate / value)
                if let Some(mi) = assigning {
                    if let Some(m) = modulators.get(mi) {
                        ui.horizontal(|ui| {
                            match m.kind {
                                ModKind::Lfo | ModKind::Random | ModKind::Steps => {
                                    let mut rate = m.rate;
                                    ui.label(egui::RichText::new("Rate").size(9.0).color(theme::TEXT_DIM));
                                    if ui.add(egui::Slider::new(&mut rate, 0.0..=1.0).show_value(false)).changed() {
                                        self.project.tracks[ti].devices[di].modulators[mi].rate = rate;
                                        self.sync_mods(ti);
                                    }
                                }
                                ModKind::Macro => {
                                    let mut val = m.value;
                                    ui.label(egui::RichText::new("Macro").size(9.0).color(theme::TEXT_DIM));
                                    if ui.add(egui::Slider::new(&mut val, 0.0..=1.0).show_value(false)).changed() {
                                        self.project.tracks[ti].devices[di].modulators[mi].value = val;
                                        self.sync_mods(ti);
                                    }
                                }
                            }
                            ui.label(egui::RichText::new("drag a knob above to route").size(9.0).color(theme::TEXT_FAINT));
                        });
                    }
                }
                }); // end vertical
            });
    }

    fn set_route(&mut self, ti: usize, di: usize, mi: usize, param: usize, amount: f32) {
        let routes = &mut self.project.tracks[ti].devices[di].modulators[mi].routes;
        if amount.abs() < 0.005 {
            routes.retain(|r| r.param != param);
        } else if let Some(r) = routes.iter_mut().find(|r| r.param == param) {
            r.amount = amount;
        } else {
            routes.push(ModRoute { param, amount });
        }
        self.sync_mods(ti);
    }

    fn sync_mods(&mut self, ti: usize) {
        if let Some(t) = self.project.tracks.get(ti) {
            self.cmds.push(Command::SetModRoutes { track: t.id, modulators: build_mods(t) });
        }
    }
}

// ---------------------------------------------------------------------------
// Piano roll
// ---------------------------------------------------------------------------

const PR_LOW: u8 = 36;
const PR_HIGH: u8 = 84;
const PR_ROW_H: f32 = 12.0;
const PR_BEAT_W: f32 = 52.0;
const PR_GRID: f64 = 0.25;
const PR_KEYS_W: f32 = 44.0;

impl DawApp {
    fn piano_roll(&mut self, ui: &mut egui::Ui) {
        let Some((ti, scene)) = self.editing else { return };
        if ti >= self.project.tracks.len() || self.project.tracks[ti].clips.get(scene).map(|c| c.is_none()).unwrap_or(true) {
            self.editing = None;
            return;
        }

        let (tid, color, clip_name, mut clip_len) = {
            let clip = self.project.tracks[ti].clips[scene].as_ref().unwrap();
            (self.project.tracks[ti].id, self.project.tracks[ti].color, clip.name.clone(), clip.length)
        };

        // toolbar
        let mut close = false;
        let mut len_changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(clip_name).strong());
            ui.label(egui::RichText::new(&self.project.tracks[ti].name).color(theme::TEXT_DIM));
            ui.separator();
            ui.label("Length");
            egui::ComboBox::from_id_salt("clip_len")
                .selected_text(format!("{} beats", clip_len as i64))
                .show_ui(ui, |ui| {
                    for l in [1.0, 2.0, 4.0, 8.0, 16.0] {
                        if ui.selectable_value(&mut clip_len, l, format!("{} beats", l as i64)).changed() {
                            len_changed = true;
                        }
                    }
                });
            ui.label(egui::RichText::new("click to add · drag to move · right-click to delete").size(10.0).color(theme::TEXT_FAINT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close ✕").clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.editing = None;
            return;
        }
        if len_changed {
            self.project.tracks[ti].clips[scene].as_mut().unwrap().length = clip_len;
            self.sync_clip(ti, scene);
        }

        let pitches: Vec<u8> = (PR_LOW..=PR_HIGH).rev().collect();
        let grid_h = pitches.len() as f32 * PR_ROW_H;
        let grid_w = (clip_len as f32) * PR_BEAT_W;
        let key = self.project.key;

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // key labels
                let (keys_rect, _) = ui.allocate_exact_size(Vec2::new(PR_KEYS_W, grid_h), Sense::hover());
                let kp = ui.painter_at(keys_rect);
                for (row, &pitch) in pitches.iter().enumerate() {
                    let y = keys_rect.top() + row as f32 * PR_ROW_H;
                    let black = NOTE_NAMES[(pitch % 12) as usize].contains('#');
                    let rr = Rect::from_min_size(Pos2::new(keys_rect.left(), y), Vec2::new(PR_KEYS_W, PR_ROW_H));
                    kp.rect_filled(rr, Rounding::ZERO, if black { Color32::from_gray(0x1c) } else { Color32::from_gray(0x26) });
                    if key.scale.contains(pitch, key.root) {
                        kp.rect_filled(Rect::from_min_size(rr.left_top(), Vec2::new(3.0, PR_ROW_H)), Rounding::ZERO, theme::ACCENT_DIM);
                    }
                    if pitch % 12 == 0 {
                        kp.text(Pos2::new(keys_rect.left() + 4.0, y + PR_ROW_H * 0.5), Align2::LEFT_CENTER, note_name(pitch), FontId::proportional(8.0), theme::TEXT_FAINT);
                    }
                }

                // note grid
                let (grect, gresp) = ui.allocate_exact_size(Vec2::new(grid_w, grid_h), Sense::click_and_drag());
                self.draw_grid(ui, grect, &pitches, clip_len, key);
                self.handle_piano_interaction(ui, grect, gresp, ti, scene, tid, color, &pitches, clip_len);

                // playhead
                if self.active_scene(tid) == scene as i32 && self.playing() {
                    let ph = (self.position_beats().rem_euclid(clip_len)) as f32 * PR_BEAT_W;
                    ui.painter().line_segment(
                        [Pos2::new(grect.left() + ph, grect.top()), Pos2::new(grect.left() + ph, grect.bottom())],
                        Stroke::new(2.0, theme::PLAY),
                    );
                }
            });
        });
    }

    fn draw_grid(&self, ui: &egui::Ui, rect: Rect, pitches: &[u8], clip_len: f64, key: dawcore::model::KeySignature) {
        let p = ui.painter_at(rect);
        p.rect_filled(rect, Rounding::ZERO, theme::SLOT_EMPTY);
        for (row, &pitch) in pitches.iter().enumerate() {
            let y = rect.top() + row as f32 * PR_ROW_H;
            let black = NOTE_NAMES[(pitch % 12) as usize].contains('#');
            if black {
                p.rect_filled(Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), PR_ROW_H)), Rounding::ZERO, Color32::from_rgba_premultiplied(0, 0, 0, 50));
            }
            if key.scale.contains(pitch, key.root) {
                p.rect_filled(Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), PR_ROW_H)), Rounding::ZERO, Color32::from_rgba_unmultiplied(0xff, 0x8a, 0x00, 14));
            }
            p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(0.5, theme::GRID));
        }
        let mut b = 0.0;
        while b <= clip_len {
            let x = rect.left() + b as f32 * PR_BEAT_W;
            let bar = (b % 1.0).abs() < 1e-6;
            p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(if bar { 1.0 } else { 0.5 }, theme::GRID));
            b += PR_GRID;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_piano_interaction(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        resp: egui::Response,
        ti: usize,
        scene: usize,
        tid: usize,
        color: [u8; 3],
        pitches: &[u8],
        clip_len: f64,
    ) {
        // draw existing notes and capture hit-testing
        let notes_len = self.project.tracks[ti].clips[scene].as_ref().unwrap().notes.len();
        let painter = ui.painter_at(rect);
        for ni in 0..notes_len {
            let n = &self.project.tracks[ti].clips[scene].as_ref().unwrap().notes[ni];
            let row = (PR_HIGH - n.pitch) as f32;
            let nr = Rect::from_min_size(
                Pos2::new(rect.left() + n.start as f32 * PR_BEAT_W, rect.top() + row * PR_ROW_H + 1.0),
                Vec2::new((n.length as f32 * PR_BEAT_W - 1.0).max(3.0), PR_ROW_H - 2.0),
            );
            painter.rect(nr, Rounding::same(2.0), theme::track_color(color), Stroke::new(1.0, Color32::from_gray(20)));
        }

        let pointer = resp.interact_pointer_pos();

        // begin drag / add
        if resp.drag_started() || resp.clicked() {
            if let Some(pos) = pointer {
                let local_x = pos.x - rect.left();
                let local_y = pos.y - rect.top();
                let row = (local_y / PR_ROW_H).floor() as usize;
                let pitch = *pitches.get(row.min(pitches.len() - 1)).unwrap();
                let beat = (local_x / PR_BEAT_W) as f64;

                // hit existing note?
                let mut hit = None;
                let notes = &self.project.tracks[ti].clips[scene].as_ref().unwrap().notes;
                for (ni, n) in notes.iter().enumerate() {
                    if n.pitch == pitch && beat >= n.start && beat <= n.start + n.length {
                        let near_end = beat > n.start + n.length - 0.15;
                        hit = Some((ni, near_end));
                        break;
                    }
                }
                match hit {
                    Some((ni, near_end)) => {
                        self.push_undo();
                        self.note_drag = Some(NoteDrag { idx: ni, mode: if near_end { DragMode::Resize } else { DragMode::Move } });
                    }
                    None if resp.clicked() => {
                        self.push_undo();
                        let snapped = (beat / PR_GRID).round() * PR_GRID;
                        let start = snapped.clamp(0.0, clip_len - PR_GRID);
                        let note = Note { pitch, start, length: 1.0, velocity: 100 };
                        self.project.tracks[ti].clips[scene].as_mut().unwrap().notes.push(note);
                        self.cmds.push(Command::NoteOn { track: tid, note: pitch, velocity: 0.9 });
                        self.cmds.push(Command::NoteOff { track: tid, note: pitch });
                        self.sync_clip(ti, scene);
                    }
                    None => {}
                }
            }
        }

        // continue drag
        if resp.dragged() {
            if let (Some(drag), Some(pos)) = (self.note_drag.as_ref().map(|d| (d.idx, d.mode)), pointer) {
                let (idx, mode) = drag;
                let local_x = (pos.x - rect.left()).max(0.0);
                let local_y = pos.y - rect.top();
                let clip = self.project.tracks[ti].clips[scene].as_mut().unwrap();
                if idx < clip.notes.len() {
                    match mode {
                        DragMode::Move => {
                            let row = (local_y / PR_ROW_H).floor().clamp(0.0, (pitches.len() - 1) as f32) as usize;
                            let pitch = pitches[row];
                            let beat = ((local_x / PR_BEAT_W) as f64 / PR_GRID).round() * PR_GRID;
                            clip.notes[idx].pitch = pitch;
                            clip.notes[idx].start = beat.clamp(0.0, clip_len - clip.notes[idx].length);
                        }
                        DragMode::Resize => {
                            let end_beat = ((local_x / PR_BEAT_W) as f64 / PR_GRID).round() * PR_GRID;
                            let len = (end_beat - clip.notes[idx].start).max(PR_GRID);
                            clip.notes[idx].length = len.min(clip_len - clip.notes[idx].start);
                        }
                    }
                }
            }
        }

        if resp.drag_stopped() {
            if self.note_drag.is_some() {
                self.note_drag = None;
                self.sync_clip(ti, scene);
            }
        }

        // delete with right-click
        if resp.secondary_clicked() {
            if let Some(pos) = pointer {
                let local_x = pos.x - rect.left();
                let local_y = pos.y - rect.top();
                let row = (local_y / PR_ROW_H).floor() as usize;
                let pitch = *pitches.get(row.min(pitches.len() - 1)).unwrap();
                let beat = (local_x / PR_BEAT_W) as f64;
                let hit = self.project.tracks[ti].clips[scene].as_ref().unwrap().notes.iter()
                    .position(|n| n.pitch == pitch && beat >= n.start && beat <= n.start + n.length);
                if let Some(ni) = hit {
                    self.push_undo();
                    self.project.tracks[ti].clips[scene].as_mut().unwrap().notes.remove(ni);
                    self.sync_clip(ti, scene);
                }
            }
        }
    }

    fn sync_clip(&mut self, ti: usize, scene: usize) {
        let tid = self.project.tracks[ti].id;
        let clip = self.project.tracks[ti].clips[scene].as_ref().map(build_clip);
        self.cmds.push(Command::SetClip { track: tid, scene, clip });
    }
}
