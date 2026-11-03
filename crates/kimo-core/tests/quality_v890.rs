//! quality_v890.rs: V8.9.0 motion blur + handheld stabilization, wired into auto_edit (quality audit #3).
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_q890_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }
fn proj() -> Session { let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 640, "height": 360, "fps": 30})); ss }

/// Same talking-head stand-in as auto_edit.rs's own tests: a voice-like tone in the given speech windows.
fn talk(name: &str, speech: &[(f64, f64)]) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let dur = speech.iter().map(|(_, e)| *e).fold(0.0_f64, f64::max);
    let gate: Vec<String> = speech.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
    let a = format!("aevalsrc='({})*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=640x360:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A locked-off (tripod) b-roll stand-in: a static crop of a moving test pattern, same as auto_edit.rs's own `broll()`.
fn broll_locked(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=640x360:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A handheld b-roll stand-in: the SAME test pattern, rendered from a larger canvas through a crop window
/// that wobbles sinusoidally (simulated hand-shake) -- the only difference from `broll_locked` is the
/// jitter, so any stabilization measured between the two is caused by the shake, not the content.
fn broll_handheld(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let vf = "crop=640:360:x='80+30*sin(2*PI*5*t)':y='100+30*cos(2*PI*6.3*t)'";
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=800x560:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(), "-vf", vf, "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// V8.9.0: a push-in's eased Ken Burns ramp actually moves x/y/scale continuously within the clip, so it
/// gets a real, nonzero motion_blur by default -- proportional to how fast it moves, never forced to a flat
/// constant. A hard-cut punch-in (ease: 0, no ramp -- see auto_edit.rs's own comment) stays at 0 regardless,
/// because nothing ever moves within either side of that cut for motion blur to blur.
#[test] fn push_in_ramp_gets_motion_blur_proportional_to_its_speed_punch_ins_stay_at_zero() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("mb_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll_locked("mb_broll.mp4", 3.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [b], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");

    let pushed = r["stages"]["push_in"]["clips"].as_array().unwrap_or_else(|| panic!("no push_in stage: {r}"));
    assert!(!pushed.is_empty(), "expected a pushed b-roll clip: {r}");
    let push_cid = sid(&pushed[0], "clip_id");
    let push_clip = ss.project.find_clip(&push_cid).unwrap();
    assert!(push_clip.motion_blur > 0.0, "a real eased push-in ramp should get nonzero motion_blur by default, got {}", push_clip.motion_blur);
    assert!(push_clip.motion_blur <= 180.0, "motion_blur should never exceed a full 180 deg shutter, got {}", push_clip.motion_blur);

    let pin = r["stages"]["punch_ins"].clone();
    assert!(!pin.is_null(), "expected a punch_ins stage on a 2-sentence talking head: {r}");
    for tr in &ss.project.tracks { for c in &tr.clips {
        if matches!(c.source, kimo_core::model::Source::Media { .. }) && c.id != push_cid {
            assert_eq!(c.motion_blur, 0.0, "clip {} (not the push-in) should keep motion_blur at 0 -- punch-ins are hard, instant cuts with nothing moving within a clip for motion blur to catch: {c:?}", c.id);
        }
    }}
}

/// motion_blur: false must leave every clip (including the push-in) at the ordinary default of 0 -- the
/// option actually gates the behaviour, it is not just reported back unused.
#[test] fn motion_blur_false_leaves_the_push_in_at_zero() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("mb_off_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll_locked("mb_off_broll.mp4", 3.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [b], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["motion_blur"], json!(false));
    let pushed = r["stages"]["push_in"]["clips"].as_array().unwrap_or_else(|| panic!("no push_in stage: {r}"));
    let push_cid = sid(&pushed[0], "clip_id");
    assert_eq!(ss.project.find_clip(&push_cid).unwrap().motion_blur, 0.0, "motion_blur:false should leave the push-in's motion_blur untouched at 0");
}

/// V8.9.0: stabilize_handheld defaults to false (measured cost -- see CHANGELOG.md v8.9.0), so an ordinary
/// auto_edit call writes no stabilize stage and touches no clip's x/y/scale/rotation beyond push-in's own.
#[test] fn stabilize_handheld_defaults_to_false() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("stab_off_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll_handheld("stab_off_broll.mp4", 1.5)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [b], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "push_in": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["stabilize_handheld"], json!(false));
    assert!(r["stages"]["stabilize"].is_null(), "stabilize_handheld defaults to false: no stage expected: {r}");
}

/// The core measurement: given one genuinely handheld (simulated shake) b-roll clip and one genuinely
/// locked-off clip, stabilize_handheld:true must correct the shaky one (real x/y keys written, nonzero
/// measured shake over the threshold) and leave the locked-off one exactly as decoded (no stage entry
/// marked stabilized, no keys written) -- the "so locked-off shots are untouched" half of the ask.
#[test] fn stabilize_handheld_corrects_the_shaky_clip_and_leaves_the_locked_one_alone() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("stab_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let shaky = sid(&call(&mut ss, "import_media", json!({"path": broll_handheld("stab_shaky.mp4", 1.5)})), "asset_id");
    let locked = sid(&call(&mut ss, "import_media", json!({"path": broll_locked("stab_locked.mp4", 1.5)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [shaky, locked], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "push_in": false, "stabilize_handheld": true}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["stabilize_handheld"], json!(true));
    let clips = r["stages"]["stabilize"]["clips"].as_array().unwrap_or_else(|| panic!("no stabilize stage with stabilize_handheld:true and 2 b-roll clips: {r}"));
    assert_eq!(clips.len(), 2, "both b-roll clips should be scanned: {r}");

    let by_shake: Vec<(bool, f64)> = clips.iter().map(|c| (c["stabilized"].as_bool().unwrap(), c["shake_px"].as_f64().unwrap())).collect();
    let n_stabilized = by_shake.iter().filter(|(s, _)| *s).count();
    assert_eq!(n_stabilized, 1, "exactly the shaky clip should have been stabilized, got {by_shake:?}: {r}");
    let (shaky_entry, locked_entry): (&Value, &Value) = (
        clips.iter().find(|c| c["stabilized"].as_bool() == Some(true)).unwrap(),
        clips.iter().find(|c| c["stabilized"].as_bool() == Some(false)).unwrap(),
    );
    let shaky_px = shaky_entry["shake_px"].as_f64().unwrap();
    let locked_px = locked_entry["shake_px"].as_f64().unwrap();
    assert!(shaky_px > locked_px * 3.0, "the handheld clip's measured shake ({shaky_px:.3}px) should clearly exceed the locked-off clip's ({locked_px:.3}px): {r}");

    // The locked-off clip must come out of this with no x/y/scale/rotation keys at all -- scanned, not written.
    let locked_cid = sid(locked_entry, "clip_id");
    let lc = ss.project.find_clip(&locked_cid).unwrap();
    for k in ["x", "y", "scale", "rotation"] {
        if let Some(q) = lc.props.get(k) { assert!(q.keys.is_empty(), "locked-off clip should have no {k} keys written, got {:?}", q.keys); }
    }
    // The shaky clip DID get real correction keys written (method position_scale_rotation writes x/y/scale/rotation).
    let shaky_cid = sid(shaky_entry, "clip_id");
    let sc = ss.project.find_clip(&shaky_cid).unwrap();
    let has_keys = ["x", "y", "scale", "rotation"].iter().any(|k| sc.props.get(*k).is_some_and(|q| !q.keys.is_empty()));
    assert!(has_keys, "the stabilized clip should have real x/y/scale/rotation correction keys written: {sc:?}");
}
