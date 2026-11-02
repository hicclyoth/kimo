//! V8.8.0 TDD: quality-audit #7 refinement to the item-5 layer-kind rule in render.rs::wants_linear_blend
//! (Solid/Gradient scrims/dims blend gamma-space like Text/Shape, UNLESS the clip is actively
//! transitioning -- a crossfade between two Solid/Media clips stays a genuine linear-light mix, see
//! quality_v870.rs's crossfade_midpoint_is_188_with_the_option_on). The x264-preset argv tests live inline
//! in render.rs (legacy_encoder_command is pub(crate), not reachable from an integration test file).
use kimo_core::{commands::{dispatch, Session}, render};
use serde_json::json;

#[test]
fn x264_preset_rejects_an_unknown_value() {
    let mut s = Session::new();
    let err = dispatch(&mut s, "project_set", &json!({"x264_preset": "bogus"})).unwrap_err();
    assert!(err.contains("x264_preset"), "error should name the field: {err}");
}

/// quality audit #7: a 60%-opacity black Solid clip (standing in for a plain-color scrim/dim, the same
/// class of thing as the Shape caption plate in quality_v870.rs's graphic_plate test) over mid-grey
/// footage must blend to the SAME pixel whether linear_blend is on or off, because it is not actively
/// transitioning.
#[test]
fn static_solid_scrim_over_footage_blends_the_same_linear_on_or_off() {
    let run = |linear_on: bool| -> Vec<u8> {
        let mut s = Session::new();
        dispatch(&mut s, "project_set", &json!({"width": 16, "height": 16, "fps": 30, "linear_blend": linear_on})).unwrap();
        let bg = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
        dispatch(&mut s, "add_clip", &json!({"track_id": bg, "source_type": "solid", "color": [0.5, 0.5, 0.5], "start": 0.0, "duration": 1.0})).unwrap();
        let scrim_track = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
        let scrim = dispatch(&mut s, "add_clip", &json!({"track_id": scrim_track, "source_type": "solid", "color": [0.0, 0.0, 0.0], "start": 0.0, "duration": 1.0})).unwrap()["clip_id"].as_str().unwrap().to_string();
        dispatch(&mut s, "set_property", &json!({"clip_id": scrim, "property": "opacity", "value": 0.6})).unwrap();
        let cache = render::Cache::new();
        render::export_frame_rgba8(&s.project, 0.5, &cache).unwrap()
    };
    let on = run(true);
    let off = run(false);
    assert_eq!(on, off, "a static Solid scrim over footage must blend identically in gamma space regardless of linear_blend");
}

/// The crossfade test (quality_v870.rs) still measures 188/255 at the midpoint with linear_blend on: a
/// Solid clip actively transitioning (add_transition) is a genuine linear-light mix, not a static scrim,
/// so it must NOT be swept into the same-pixel-either-way rule above. This is the same assertion as
/// quality_v870's crossfade_midpoint_is_188_with_the_option_on, re-stated here next to the scrim test so
/// the two intentionally-different outcomes for "a Solid clip, linear_blend on" sit side by side.
#[test]
fn transitioning_solid_still_honors_linear_blend_unlike_a_static_scrim() {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 16, "height": 16, "fps": 30, "linear_blend": true})).unwrap();
    let bottom = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_clip", &json!({"track_id": bottom, "source_type": "solid", "color": [0.0, 0.0, 0.0], "start": 0.0, "duration": 2.0})).unwrap();
    let top = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let white = dispatch(&mut s, "add_clip", &json!({"track_id": top, "source_type": "solid", "color": [1.0, 1.0, 1.0], "start": 0.0, "duration": 1.0})).unwrap()["clip_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_transition", &json!({"clip_id": white, "kind": "crossfade", "duration": 1.0, "ease": "linear"})).unwrap();
    let cache = render::Cache::new();
    let bytes = render::export_frame_rgba8(&s.project, 0.5, &cache).unwrap();
    assert!((bytes[0] as i32 - 188).abs() <= 3, "a transitioning Solid should still get the physically-correct linear midpoint (~188), got {}", bytes[0]);
}

#[test]
fn auto_edit_turns_on_encoder_quality_and_color_tag_sanity_by_default() {
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("kimo_q880_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("th.mp4");
    if !p.exists() {
        let a = "aevalsrc='(between(t,0,3))*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d=3";
        let st = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=3",
            "-f", "lavfi", "-i", a, "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest", p.to_str().unwrap()]).stdin(Stdio::null()).status().unwrap();
        assert!(st.success());
    }
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 640, "height": 360, "fps": 30})).unwrap();
    let a = dispatch(&mut s, "import_media", &json!({"path": p.to_str().unwrap()})).unwrap()["asset_id"].as_str().unwrap().to_string();
    let r = dispatch(&mut s, "auto_edit", &json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false, "push_in": false, "end_screen": false})).unwrap();
    assert_eq!(r.get("encoder_quality"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(r.get("color_tag_sanity"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(s.project.x264_preset.as_deref(), Some("slow"));
    assert!(s.project.color_tag_sanity);
}
