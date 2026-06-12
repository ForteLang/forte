//! Render the demo project's arrangement to a WAV file.
//! Usage: cargo run --release --example bounce -- out.wav

use std::path::PathBuf;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo.wav".to_string());
    let path = PathBuf::from(&out);
    let project = dawcore::model::Project::demo();
    match dawcore::bounce::render_wav(&project, &path, 8.0) {
        Ok(secs) => println!("Rendered {secs:.1}s of audio -> {out}"),
        Err(e) => {
            eprintln!("bounce failed: {e}");
            std::process::exit(1);
        }
    }
}
