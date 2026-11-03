//! quality_v893.rs: V8.9.3 frame-blend conform for mismatched source frame rates (quality audit #5).
use kimo_core::commands::{dispatch, Session};
use kimo_core::render::{render_frame, Cache};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_q893_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }

/// A 24fps synthetic source whose content changes every native frame (testsrc2 draws a moving pattern and
/// a frame counter), so an exact duplicate between two decoded project frames can only come from the
/// fps= conform step repeating a native frame, never from the source holding still on its own.
fn clip_24fps(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=320x180:r=24:d={dur}"), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// A 30fps project playing a 24fps source 1:1 (speed 1.0, no retime) duplicates exactly the frames the
/// audit measured (roughly a fifth of them, since 24/30 = 0.8): count consecutive-frame exact-duplicate
/// pairs with frame_blend_conform off (today's plain `fps=` behaviour) and assert it is a real, nonzero
/// fraction before testing the fix.
fn setup(name: &str, dur: f64, frame_blend_conform: bool) -> (Session, String) {
    let mut ss = Session::new();
    call(&mut ss, "project_set", json!({"width": 320, "height": 180, "fps": 30, "frame_blend_conform": frame_blend_conform}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": clip_24fps(name, dur)})), "asset_id");
    let tr = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_clip", json!({"track_id": tr, "source_type": "media", "asset_id": a, "start": 0, "duration": dur}));
    (ss, a)
}

fn frame_hashes(ss: &Session, dur: f64, fps: f64) -> Vec<u64> {
    use std::hash::{Hash, Hasher};
    let mut cache = Cache::new();
    kimo_core::render::prepare_export_range(&ss.project, &mut cache, 0.0, dur).unwrap();
    let n = (dur * fps).floor() as usize;
    (0..n).map(|i| {
        let f = render_frame(&ss.project, i as f64 / fps, &cache).unwrap();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in &f.d { v.to_bits().hash(&mut h); }
        h.finish()
    }).collect()
}

fn consecutive_duplicate_count(hashes: &[u64]) -> usize { hashes.windows(2).filter(|w| w[0] == w[1]).count() }

/// TDD, the exact test the audit itself proposed: measure the duplicate-frame count drop, and confirm the
/// blended frames are genuinely new pixels, not duplicate frames relabelled under a different hash by luck.
#[test]
fn frame_blend_conform_drops_the_duplicate_frame_count_and_blends_not_relabels() {
    let dur = 2.0;
    let (off, _) = setup("q893_off.mp4", dur, false);
    let (on, _) = setup("q893_on.mp4", dur, true);
    let hashes_off = frame_hashes(&off, dur, 30.0);
    let hashes_on = frame_hashes(&on, dur, 30.0);
    let dup_off = consecutive_duplicate_count(&hashes_off);
    let dup_on = consecutive_duplicate_count(&hashes_on);
    assert!(dup_off > 5, "a plain 24fps-in-30fps conform should duplicate a real, measurable share of frames (got {dup_off} of {} pairs)", hashes_off.len());
    assert!(dup_on < dup_off, "frame_blend_conform should reduce exact-duplicate consecutive frames: off={dup_off} on={dup_on}");
    assert!(dup_on * 3 <= dup_off, "mi_mode=blend should leave only a small residual of exact duplicates (stream-boundary frames with no neighbour to blend with), not the same share as plain duplication: off={dup_off} on={dup_on}");

    // Not simply duplicate frames relabelled: the blended frames must actually differ, pixel for pixel,
    // from what the option-off (plain duplicate) render produced at the very same project times.
    let differs_from_off = hashes_on.iter().zip(&hashes_off).filter(|(a, b)| a != b).count();
    assert!(differs_from_off > dup_off, "turning the option on should change more frames than it merely de-duplicates (changed {differs_from_off}, had {dup_off} duplicate pairs)");
}

/// option off (the default) must render byte-identical to how every project rendered before this version --
/// the new field defaults to false and is additive-only.
#[test]
fn frame_blend_conform_defaults_to_false_and_is_bit_identical_when_off() {
    let dur = 1.0;
    let mut ss = Session::new();
    call(&mut ss, "project_set", json!({"width": 320, "height": 180, "fps": 30}));
    assert_eq!(ss.project.frame_blend_conform, false, "frame_blend_conform must default to false");
    let a = sid(&call(&mut ss, "import_media", json!({"path": clip_24fps("q893_default.mp4", dur)})), "asset_id");
    let tr = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_clip", json!({"track_id": tr, "source_type": "media", "asset_id": a, "start": 0, "duration": dur}));
    let (ss_explicit_off, _) = setup("q893_explicit_off.mp4", dur, false);
    let h1 = frame_hashes(&ss, dur, 30.0);
    let h2 = frame_hashes(&ss_explicit_off, dur, 30.0);
    assert_eq!(h1, h2, "leaving frame_blend_conform unset must render identically to setting it explicitly false");
}

/// A clip with a speed ramp (retimed) is left alone -- frame_blend_conform only ever touches plain,
/// unretimed 1x playback (see conform_fps_filter's own eligibility check in render.rs).
#[test]
fn frame_blend_conform_does_not_touch_a_retimed_clip() {
    let dur = 1.0;
    let mut ss_a = Session::new();
    call(&mut ss_a, "project_set", json!({"width": 320, "height": 180, "fps": 30, "frame_blend_conform": false}));
    let a = sid(&call(&mut ss_a, "import_media", json!({"path": clip_24fps("q893_ramp.mp4", dur)})), "asset_id");
    let tr = sid(&call(&mut ss_a, "add_track", json!({})), "track_id");
    call(&mut ss_a, "add_clip", json!({"track_id": tr, "source_type": "media", "asset_id": a, "start": 0, "duration": dur, "speed": 0.5}));

    let mut ss_b = Session::new();
    call(&mut ss_b, "project_set", json!({"width": 320, "height": 180, "fps": 30, "frame_blend_conform": true}));
    let b = sid(&call(&mut ss_b, "import_media", json!({"path": clip_24fps("q893_ramp.mp4", dur)})), "asset_id");
    let tr2 = sid(&call(&mut ss_b, "add_track", json!({})), "track_id");
    call(&mut ss_b, "add_clip", json!({"track_id": tr2, "source_type": "media", "asset_id": b, "start": 0, "duration": dur, "speed": 0.5}));

    let h_a = frame_hashes(&ss_a, dur, 30.0);
    let h_b = frame_hashes(&ss_b, dur, 30.0);
    assert_eq!(h_a, h_b, "a clip with speed != 1.0 must render identically regardless of frame_blend_conform");
}

/// Speed cost: minterpolate is more expensive than a plain fps= duplicate filter. Measured, not asserted
/// against a hard budget (this is a decode-filter cost, highly box-dependent) -- printed for the report.
#[test]
fn frame_blend_conform_speed_cost_is_measured() {
    let dur = 3.0;
    let (off, _) = setup("q893_speed_off.mp4", dur, false);
    let (on, _) = setup("q893_speed_on.mp4", dur, true);
    let t0 = Instant::now();
    frame_hashes(&off, dur, 30.0);
    let off_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    frame_hashes(&on, dur, 30.0);
    let on_ms = t1.elapsed().as_millis();
    eprintln!("frame_blend_conform speed: off={off_ms}ms on={on_ms}ms ({}x) over {} project frames", on_ms as f64 / off_ms.max(1) as f64, (dur * 30.0) as usize);
    assert!(on_ms < off_ms * 20 + 500, "frame_blend_conform should not be wildly more expensive than plain duplication: off={off_ms}ms on={on_ms}ms");
}
