use kimo_core::{commands::{Session,dispatch},render};
use serde_json::json;
use std::process::Command;

/// V8.6.0 overlap pipeline: decode of the next export batch now runs on a prefetch thread against a
/// cache snapshot while the current batch composes, and the encoder runs on its own thread. The real
/// proof of "bit-identical to the old sequential path" is the framemd5-vs-baseline-binary comparison in
/// kimo_core_bench.md (two different binaries, can't be a unit test); what a unit test CAN prove is that
/// this concurrent pipeline is deterministic and loses no frames across many batches of real decoded
/// media -- a race in the snapshot/prefetch handoff would show up as non-determinism or a frame count
/// drift between runs, not as a crash.
fn media_project(w: u32, h: u32, fps: f64, dur: f64) -> Session {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": w, "height": h, "fps": fps})).unwrap();
    let source = "/home/user/tab/work/v7q_real/talkinghead_real_final.mp4";
    let asset = dispatch(&mut s, "import_media", &json!({"path": source})).unwrap();
    let tr = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].clone();
    dispatch(&mut s, "add_clip", &json!({"track_id": tr, "source_type": "media", "asset_id": asset["asset_id"], "start": 0, "duration": dur})).unwrap();
    s
}

fn framemd5(path: &str) -> Vec<u8> {
    let out = Command::new("ffmpeg").args(["-v", "error", "-i", path, "-f", "framemd5", "-"]).output().unwrap();
    assert!(out.status.success(), "ffprobe framemd5 failed: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// At 320x180/24fps with nt>=1, a 5s clip is 120 frames -- several export batches (batch = nt*4), so the
/// prefetch-decode-while-compose handoff runs several times, not just once.
#[test]
fn overlap_export_is_deterministic_across_many_batches() {
    let s = media_project(320, 180, 24.0, 5.0);
    let n = (5.0 * 24.0f64).round() as usize;
    assert_eq!(s.project.tracks[0].clips.len(), 1);

    let p1 = std::env::temp_dir().join(format!("kimo-overlap-det-a-{}.mp4", std::process::id()));
    let p2 = std::env::temp_dir().join(format!("kimo-overlap-det-b-{}.mp4", std::process::id()));
    let dur1 = render::export(&s.project, p1.to_str().unwrap(), 20).unwrap();
    let dur2 = render::export(&s.project, p2.to_str().unwrap(), 20).unwrap();

    // Same clip, same project, two independent exports through the concurrent pipeline (each gets its
    // own fresh Cache and its own prefetch/compose/encoder threads): the frame count and every frame's
    // bytes must match exactly, or the snapshot/channel handoff is racing.
    assert_eq!(dur1, dur2);
    let md5_1 = framemd5(p1.to_str().unwrap());
    let md5_2 = framemd5(p2.to_str().unwrap());
    assert_eq!(md5_1, md5_2, "two exports of the same project through the overlap pipeline produced different frame bytes");

    // Frame count: no frame dropped or duplicated across the batch boundaries the prefetch thread hands off at.
    let probe = Command::new("ffprobe").args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0", p1.to_str().unwrap()]).output().unwrap();
    let counted: usize = String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap();
    assert_eq!(counted, n, "overlap pipeline dropped or duplicated frames across batch handoffs");

    std::fs::remove_file(p1).unwrap();
    std::fs::remove_file(p2).unwrap();
}

/// A single-batch export (fewer frames than one batch, so the prefetch path never fires: `end == n` on
/// the very first `decode_batch` call) must still work -- the `carried: None` branch of the new loop.
#[test]
fn overlap_export_single_batch_still_works() {
    let s = media_project(160, 90, 24.0, 0.2); // 5 frames: well under any realistic batch size
    let n = (0.2 * 24.0f64).round() as usize;
    let path = std::env::temp_dir().join(format!("kimo-overlap-single-{}.mp4", std::process::id()));
    render::export(&s.project, path.to_str().unwrap(), 20).unwrap();
    let probe = Command::new("ffprobe").args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0", path.to_str().unwrap()]).output().unwrap();
    let counted: usize = String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap();
    assert_eq!(counted, n);
    std::fs::remove_file(path).unwrap();
}

/// `Cache::snapshot()` shares frame data by Arc (no pixel copy): after decoding into the snapshot, the
/// original cache's own entry for the same clip must be untouched (no aliasing between the two).
#[test]
fn cache_snapshot_does_not_alias_the_original() {
    let s = media_project(160, 90, 24.0, 1.0);
    let mut cache = render::Cache::new_export();
    render::prepare_export_range(&s.project, &mut cache, 0.0, 0.2).unwrap();
    let before = cache.frames_of(&s.project.tracks[0].clips[0].id).map(|f| f.len());
    assert!(before.is_some() && before.unwrap() > 0);

    let mut snap = cache.snapshot();
    render::prepare_export_range(&s.project, &mut snap, 0.5, 0.9).unwrap();

    // The original `cache` must report the same window it had before the snapshot was mutated.
    let after = cache.frames_of(&s.project.tracks[0].clips[0].id).map(|f| f.len());
    assert_eq!(before, after, "decoding into a snapshot mutated the original cache it was cloned from");
}
