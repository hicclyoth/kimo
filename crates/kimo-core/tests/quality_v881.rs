//! V8.8.1 TDD: colour-tag sanity check for mistagged/untagged HD source footage (quality audit #2).
//! A synthetic HD clip is encoded once (pixel-identical YUV bytes throughout), then remuxed with its H.264
//! VUI colour tags rewritten via the `h264_metadata` bitstream filter (no re-encode -- the exact same
//! encoded bytes, only the container's colour-tag metadata changes) to simulate a real mistagged/untagged
//! HD source: an SD matrix (bt470bg) with no primaries/transfer tag at all, the audit's own measured shape
//! (talkinghead_real_final.mp4: color_space=bt470bg, color_transfer/color_primaries both absent).
use kimo_core::{commands::{dispatch, Session}, render};
use serde_json::json;
use std::process::{Command, Stdio};

fn fixture_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kimo_q881_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Builds (truly-tagged bt709 file, mistagged-but-byte-identical-video file). Both decode to the exact
/// same H.264 bitstream/pixel data; only the VUI colour tags in the mistagged one are rewritten (matrix
/// bt470bg=5, primaries/transfer unspecified=2 -- a remux via `-c:v copy` plus `h264_metadata`, no
/// re-encode, so any decoded RGB difference between them is caused ONLY by the tag Kimo reads, never by a
/// different encode).
fn fixtures() -> (String, String) {
    let dir = fixture_dir();
    let truth = dir.join("truth.mp4");
    let mistagged = dir.join("mistagged.mp4");
    if !truth.exists() {
        let st = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=s=1280x720:d=0.2:r=25",
            "-vf", "scale=out_color_matrix=bt709:out_range=tv", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709",
            truth.to_str().unwrap()]).stdin(Stdio::null()).status().unwrap();
        assert!(st.success(), "truth fixture encode failed");
    }
    if !mistagged.exists() {
        let st = Command::new("ffmpeg").args(["-v", "error", "-y", "-i", truth.to_str().unwrap(),
            "-c:v", "copy", "-bsf:v", "h264_metadata=colour_primaries=2:transfer_characteristics=2:matrix_coefficients=5",
            mistagged.to_str().unwrap()]).stdin(Stdio::null()).status().unwrap();
        assert!(st.success(), "mistagged remux failed");
    }
    (truth.to_str().unwrap().into(), mistagged.to_str().unwrap().into())
}

/// ffprobe sanity: the two fixture files really do carry different tags on identical pixel data (a
/// precondition of the test below, not the thing under test).
#[test]
fn fixture_files_carry_different_tags_on_identical_frames() {
    let (truth, mistagged) = fixtures();
    let probe = |p: &str, field: &str| -> String {
        let out = Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", &format!("stream={field}"), "-of", "default=nw=1:nk=1", p]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(probe(&truth, "color_space"), "bt709");
    assert_eq!(probe(&mistagged, "color_space"), "bt470bg");
    assert_eq!(probe(&mistagged, "color_transfer"), "unknown");
    assert_eq!(probe(&mistagged, "color_primaries"), "unknown");
}

fn decode_frame(path: &str, color_tag_sanity: bool) -> Vec<u8> {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 1280, "height": 720, "fps": 25, "color_tag_sanity": color_tag_sanity})).unwrap();
    let t = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let a = dispatch(&mut s, "import_media", &json!({"path": path})).unwrap()["asset_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_clip", &json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 0.16, "in_point": 0.0, "fit": "stretch"})).unwrap();
    let mut cache = render::Cache::new();
    render::prepare_at(&s.project, &mut cache, Some(0.08)).unwrap();
    render::export_frame_rgba8(&s.project, 0.08, &cache).unwrap()
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    let sum: f64 = (0..n).step_by(4).flat_map(|i| (0..3).map(move |j| i + j)).map(|i| (a[i] as f64 - b[i] as f64).abs()).sum();
    let count = (n / 4 * 3) as f64;
    sum / count
}

/// The core measurement: decoding the mistagged file with color_tag_sanity OFF (trusting the incomplete
/// bt470bg tag, today's/every-existing-project's behaviour) measurably shifts colour away from the truth
/// (the same file's pixels, correctly tagged); color_tag_sanity ON brings it back within rounding noise.
#[test]
fn color_tag_sanity_corrects_the_measured_shift() {
    let (truth, mistagged) = fixtures();
    let truth_rgb = decode_frame(&truth, false);
    let before = decode_frame(&mistagged, false); // trusts the bad tag -- today's behaviour
    let after = decode_frame(&mistagged, true); // color_tag_sanity on -- forces bt709

    let shift_before = mean_abs_diff(&truth_rgb, &before);
    let shift_after = mean_abs_diff(&truth_rgb, &after);
    eprintln!("color_tag_sanity: mean abs RGB shift before={shift_before:.2}/255, after={shift_after:.2}/255");

    assert!(shift_before > 2.0, "sanity: the mistagged decode should actually show a measurable shift (bt470bg vs bt709 matrix), got {shift_before:.2}/255");
    assert!(shift_after < shift_before / 2.0, "color_tag_sanity should substantially reduce the shift: before={shift_before:.2} after={shift_after:.2}");
    assert!(shift_after <= 2.0, "color_tag_sanity on should match the truly-tagged decode closely (8-bit rounding only), got {shift_after:.2}/255");
}

/// A correctly-tagged bt709 source is untouched by color_tag_sanity either way (nothing to "fix").
#[test]
fn color_tag_sanity_does_not_touch_a_correctly_tagged_source() {
    let (truth, _mistagged) = fixtures();
    let off = decode_frame(&truth, false);
    let on = decode_frame(&truth, true);
    assert_eq!(off, on, "a correctly-tagged bt709 source must decode identically whether color_tag_sanity is on or off");
}

/// auto_edit turns color_tag_sanity on by default (see quality_v880.rs's
/// auto_edit_turns_on_encoder_quality_and_color_tag_sanity_by_default for the full-pipeline version); a
/// hand-built project (project_set never called with color_tag_sanity) stays false, so golden_frames.txt
/// and golden_project.txt are untouched.
#[test]
fn color_tag_sanity_defaults_to_false_and_is_omitted_from_saved_projects() {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 320, "height": 180, "fps": 30})).unwrap();
    assert!(!s.project.color_tag_sanity);
    let saved = serde_json::to_string(&s.project).unwrap();
    assert!(!saved.contains("color_tag_sanity"), "false color_tag_sanity must be omitted from the saved project (skip_serializing_if)");
}
