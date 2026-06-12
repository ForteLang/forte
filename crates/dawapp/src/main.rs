//! Native desktop entry point. Spins up the egui window; the audio engine is
//! started inside [`app::DawApp::new`] and driven by cpal on its own thread.

mod app;
mod audio;
mod theme;
mod widgets;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("Bitwig Studio 6 — Clone"),
        ..Default::default()
    };

    eframe::run_native(
        "Bitwig Studio 6 — Clone",
        options,
        Box::new(|cc| Ok(Box::new(app::DawApp::new(cc)))),
    )
}
