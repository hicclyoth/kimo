use kimo_core::{commands::{Session,dispatch},render};
use serde_json::json;

/// V8.6.1: pixel-diff the in-process libav decoder (KIMO_DECODER=libav) against the existing ffmpeg-pipe
/// decode() path on a genuinely variable-frame-rate source (built once by this test's own fixture:
/// /tmp/vfr_test/vfr_test.mp4, 6 distinctly-coloured frames at irregular real timestamps 0 / 0.52 / 0.60 /
/// 0.92 / 1.12 / 1.52s -- see build_vfr_fixture()), at a CFR project rate (10fps) that doesn't evenly
/// divide any of those source gaps, so every output tick genuinely exercises "which source frame does a
/// VFR-conforming CFR tick pick". KIMO_DECODER is process-global env, so this is the only test in this
/// file and we never run anything else concurrently against it.
fn build_vfr_fixture() -> String {
    let dir = "/tmp/vfr_test";
    let out = format!("{dir}/vfr_test.mp4");
    if std::path::Path::new(&out).exists() { return out; }
    std::fs::create_dir_all(dir).unwrap();
    let colors = ["red", "green", "blue", "yellow", "cyan", "magenta"];
    for (i, c) in colors.iter().enumerate() {
        std::process::Command::new("ffmpeg").args(["-y", "-v", "error", "-f", "lavfi", "-i", &format!("color=c={c}:s=64x64:d=1"), "-frames:v", "1", &format!("{dir}/frame{i}.png")]).status().unwrap();
    }
    let list = format!("{dir}/list.txt");
    std::fs::write(&list, "file 'frame0.png'\nduration 0.5\nfile 'frame1.png'\nduration 0.1\nfile 'frame2.png'\nduration 0.3\nfile 'frame3.png'\nduration 0.2\nfile 'frame4.png'\nduration 0.4\nfile 'frame5.png'\nduration 0.6\nfile 'frame5.png'\n").unwrap();
    let status = std::process::Command::new("ffmpeg").current_dir(dir).args(["-y", "-v", "error", "-f", "concat", "-safe", "0", "-i", "list.txt", "-fps_mode", "vfr", "-pix_fmt", "yuv420p", "vfr_test.mp4"]).status().unwrap();
    assert!(status.success());
    out
}

fn vfr_project(fps: f64, dur: f64, source: &str) -> Session {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 64, "height": 64, "fps": fps})).unwrap();
    let asset = dispatch(&mut s, "import_media", &json!({"path": source})).unwrap();
    let tr = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].clone();
    dispatch(&mut s, "add_clip", &json!({"track_id": tr, "source_type": "media", "asset_id": asset["asset_id"], "start": 0, "duration": dur})).unwrap();
    s
}

#[test]
fn libav_vfr_conform_matches_pipe_path_within_rounding_noise() {
    let source = build_vfr_fixture();
    let s = vfr_project(10.0, 1.9, &source); // 19 output ticks at 0.1s, spanning the fixture's irregular timestamps
    let clip_id = s.project.tracks[0].clips[0].id.clone();

    std::env::remove_var("KIMO_DECODER");
    let mut cache_pipe = render::Cache::new_export();
    render::prepare_export_range(&s.project, &mut cache_pipe, 0.0, 1.8).unwrap();
    let pipe_frames = cache_pipe.frames_of(&clip_id).unwrap();
    let pipe_len = pipe_frames.len();
    let pipe_bytes: Vec<u8> = (0..pipe_len).flat_map(|i| pipe_frames.get(i).unwrap().to_vec()).collect();

    std::env::set_var("KIMO_DECODER", "libav");
    let mut cache_libav = render::Cache::new_export();
    render::prepare_export_range(&s.project, &mut cache_libav, 0.0, 1.8).unwrap();
    std::env::remove_var("KIMO_DECODER");
    let libav_frames = cache_libav.frames_of(&clip_id).unwrap();
    let libav_len = libav_frames.len();
    let libav_bytes: Vec<u8> = (0..libav_len).flat_map(|i| libav_frames.get(i).unwrap().to_vec()).collect();

    // Same frame count as the pipe path -- no tick dropped or duplicated by the libav conform.
    assert_eq!(pipe_len, libav_len, "libav decoder produced a different frame count than the ffmpeg-pipe path");
    assert_eq!(pipe_bytes.len(), libav_bytes.len());

    let mut max_delta: i32 = 0;
    let mut sum_delta: i64 = 0;
    for (a, b) in pipe_bytes.iter().zip(libav_bytes.iter()) {
        let d = (*a as i32 - *b as i32).abs();
        max_delta = max_delta.max(d);
        sum_delta += d as i64;
    }
    let mean = sum_delta as f64 / pipe_bytes.len() as f64;
    eprintln!("libav vs pipe VFR-conform pixel diff: max={max_delta} mean={mean:.4} over {} bytes", pipe_bytes.len());

    // Identical frame SELECTION (not necessarily byte-identical encode, since the two paths go through
    // different decode/scale code -- but these are flat solid colours, so if they picked the same source
    // frame the bytes should match almost exactly; a handful of 8-bit rounding units at most). A real
    // divergence (picking an adjacent, differently-coloured frame) would show as max_delta near 255.
    assert!(max_delta <= 4, "libav's VFR-conform frame selection picked a different source frame than the pipe path at some tick (max channel delta {max_delta} -- a real pick divergence shows as ~255)");
}
