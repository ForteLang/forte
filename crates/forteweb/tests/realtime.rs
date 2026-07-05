//! The AudioWorklet call sequence, natively: new → src → compile → play →
//! process. The browser editor went silent while the transport advanced, and
//! the E2E only asserted "position moves" — this pins "…and it makes sound".

use forteweb::*;

fn peak_after(seconds: f32) -> (f32, f64) {
    let ctx = fw_new(48_000.0);
    unsafe {
        let src = std::fs::read("../../packages/essentials_0.6.0/songs/first-light.forte").unwrap();
        let warm = std::fs::read_to_string("../../packages/essentials_0.6.0/songs/devices/warm.forte").unwrap();
        let modules = serde_json::json!({ "devices/warm.forte": warm }).to_string();

        let mp = fw_modules_prepare(ctx, modules.len());
        std::ptr::copy_nonoverlapping(modules.as_ptr(), mp, modules.len());
        assert_eq!(fw_modules_commit(ctx), 1, "one module staged");

        let sp = fw_src_prepare(ctx, src.len());
        std::ptr::copy_nonoverlapping(src.as_ptr(), sp, src.len());
        assert_eq!(fw_compile(ctx), 0, "first-light must compile in the worklet path");

        fw_play(ctx);
        let mut peak = 0.0f32;
        let blocks = (seconds * 48_000.0 / 128.0) as usize;
        for _ in 0..blocks {
            fw_process(ctx, 128);
            let l = std::slice::from_raw_parts(fw_out_l(ctx), 128);
            for s in l {
                peak = peak.max(s.abs());
            }
        }
        let pos = fw_position(ctx);
        (peak, pos)
    }
}

#[test]
fn the_worklet_path_actually_makes_sound() {
    let (peak, pos) = peak_after(3.0);
    assert!(pos > 1.0, "transport must advance (got {pos} beats)");
    assert!(
        peak > 0.05,
        "the realtime path must produce audio, not just advance (peak {peak})"
    );
}
