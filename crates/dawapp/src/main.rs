//! Native desktop entry point. Spins up the egui window; the audio engine is
//! started inside [`app::DawApp::new`] and driven by cpal on its own thread.

mod app;
mod audio;
mod theme;
mod widgets;

fn main() -> eframe::Result<()> {
    // BITWIG_WINDOW=WxH overrides the initial size (used by the visual tests).
    let (w, h) = std::env::var("BITWIG_WINDOW")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1280.0, 800.0));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_min_inner_size([720.0, 480.0])
            .with_resizable(true)
            .with_title("Bitwig Studio 6 — Clone"),
        ..Default::default()
    };

    eframe::run_native(
        "Bitwig Studio 6 — Clone",
        options,
        Box::new(|cc| Ok(Box::new(app::DawApp::new(cc)))),
    )
}
