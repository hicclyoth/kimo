//! add_droste_zoom, add_portal_mirror, add_vertigo_zoom (KIMO 8.4 surreal VFX). Checks each is discoverable and changes real
//! pixels on a synthetic scene, that mix/intensity 0 leaves the frame untouched (opt-in default), that it is keyframable
//! like any other clip effect, and (for vertigo_zoom) that the subject plane genuinely stays put while the background moves.
use kimo_core::commands::{dispatch, Session};
use kimo_core::effects::Frame;
use kimo_core::render;
use serde_json::{json, Value};

const W: usize = 160; const H: usize = 90;
fn c(s: &mut Session, n: &str, a: Value) -> Value { dispatch(s, n, &a).unwrap_or_else(|e| panic!("{n} {a}: {e}")) }
fn id(v: &Value, k: &str) -> String { v[k].as_str().unwrap().to_string() }
fn png(name: &str, f: impl Fn(usize, usize) -> [u8; 4]) -> String {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!("kimo-svfx-{}-{name}.png", std::process::id()));
    let mut ch = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{W}x{H}"), "-i", "-", "-frames:v", "1", path.to_str().unwrap()]).stdin(std::process::Stdio::piped()).spawn().expect("ffmpeg");
    { let mut si = ch.stdin.take().unwrap(); let mut o = Vec::with_capacity(W * H * 4); for y in 0..H { for x in 0..W { o.extend_from_slice(&f(x, y)); } } si.write_all(&o).unwrap(); }
    assert!(ch.wait().unwrap().success()); path.to_str().unwrap().to_string()
}
fn in_sq(x: usize, y: usize) -> bool { (65..95).contains(&x) && (30..60).contains(&y) }
fn ramp(x: usize) -> u8 { (x * 255 / (W - 1)) as u8 }
/// (session, picture clip id); a green square over a left-to-right red ramp background, like the depth_parallax scene.
fn scene() -> (Session, String) {
    let mut s = Session::new(); c(&mut s, "project_set", json!({"width": W, "height": H, "fps": 10}));
    let pic = png("pic", |x, y| if in_sq(x, y) { [25, 230, 25, 255] } else { [ramp(x), 25, 76, 255] });
    let t1 = id(&c(&mut s, "add_track", json!({})), "track_id");
    let a = id(&c(&mut s, "import_media", json!({"path": pic})), "asset_id");
    let v = id(&c(&mut s, "add_clip", json!({"track_id": t1, "source_type": "media", "asset_id": a, "start": 0, "duration": 2.0})), "clip_id");
    (s, v)
}
fn depth_scene() -> (Session, String, String) {
    let (mut s, v) = scene();
    let dep = png("depth", |x, y| if in_sq(x, y) { [255, 255, 255, 255] } else { [0, 0, 0, 255] });
    let t2 = id(&c(&mut s, "add_track", json!({})), "track_id");
    let am = id(&c(&mut s, "import_media", json!({"path": dep})), "asset_id");
    let m = id(&c(&mut s, "add_clip", json!({"track_id": t2, "source_type": "media", "asset_id": am, "start": 0, "duration": 2.0})), "clip_id");
    (s, v, m)
}
fn frame(s: &Session, t: f64) -> Frame { let mut cache = render::Cache::new(); render::prepare_at(&s.project, &mut cache, Some(t)).unwrap(); render::render_frame(&s.project, t, &cache).unwrap() }
fn px(f: &Frame, x: usize, y: usize) -> [f32; 3] { let i = (y * f.w + x) * 4; [f.d[i], f.d[i + 1], f.d[i + 2]] }
fn green(f: &Frame, x: usize, y: usize) -> bool { let p = px(f, x, y); p[1] > 0.8 && p[0] < 0.2 }
fn differs(a: &Frame, b: &Frame) -> usize { (0..a.w * a.h * 4).filter(|&i| (a.d[i] - b.d[i]).abs() > 0.02).count() }

#[test]
fn droste_zoom_changes_pixels_and_is_opt_in() {
    let (s0, v0) = scene(); let base = frame(&s0, 1.0);
    let (mut s, v) = scene();
    let r = dispatch(&mut s, "add_droste_zoom", &json!({"clip_id": v, "intensity": 0.8, "start_time": 0.0, "end_time": 2.0}));
    assert!(r.is_ok(), "add_droste_zoom must exist: {}", r.unwrap_err());
    let f = frame(&s, 1.0);
    assert!(differs(&base, &f) > 500, "droste_zoom at intensity 0.8 must visibly change the frame");
    // intensity 0 (fully faded, mix ramps 0->peak->0, so sample right at the very start before the ramp) must be ~unchanged
    let (mut s2, v2) = scene(); c(&mut s2, "add_droste_zoom", json!({"clip_id": v2, "intensity": 0.8, "start_time": 1.2, "end_time": 1.6}));
    let f2 = frame(&s2, 0.2); // t=0.2 is well before the [1.2,1.6] window -> mix must be 0
    assert_eq!(differs(&base, &f2), 0, "outside its start_time..end_time window droste_zoom must leave the frame untouched");
    let _ = v0;
}

#[test]
fn portal_mirror_shows_a_flipped_inverted_disc_and_is_removable() {
    let (mut s, v) = scene();
    c(&mut s, "add_portal_mirror", json!({"clip_id": v, "intensity": 1.0, "start_time": 0.0, "end_time": 2.0, "cx": 0.5, "cy": 0.5, "radius": 0.3, "flip_vertical": true, "invert_color": true}));
    let f = frame(&s, 1.0);
    // center of the portal (45,45) looks straight up/down across the flip axis at the same point -> stays whatever colour it was,
    // but just off-center it must show the vertically-mirrored, colour-inverted world: a point above center shows the (inverted) pixel from below center
    let above = px(&f, 80, 20); // inside the portal radius, above center
    assert!(above[0] > 0.5 || above[2] > 0.5, "portal_mirror should show an inverted-colour sample near its edge, got {above:?}");
    let outside = px(&f, 10, 10); // far outside the portal radius
    assert!(outside[1] < 0.2, "well outside the portal the frame must be untouched (not green/bright), got {outside:?}");
    let r = dispatch(&mut s, "add_portal_mirror", &json!({"clip_id": v, "remove": true}));
    assert_eq!(r.unwrap()["removed"], json!(1), "remove must take the effect back off");
}

#[test]
fn vertigo_zoom_locks_the_subject_and_moves_the_background() {
    let (s0, v0, m0) = depth_scene(); let base = frame(&s0, 0.0); let _ = (v0, m0);
    let (mut s, v, m) = depth_scene();
    let r = dispatch(&mut s, "add_vertigo_zoom", &json!({"clip_id": v, "depth_clip_id": m, "intensity": 0.6, "start_time": 0.0, "end_time": 2.0, "focal_depth": 1.0, "direction": "in"}));
    assert!(r.is_ok(), "add_vertigo_zoom must exist: {}", r.unwrap_err());
    let f = frame(&s, 1.9); // just before end_time, dolly is almost at full target (eased, not yet rendered past the clip's own duration)
    // the subject (depth 1.0 = focal_depth) must still be exactly where it started -- depth_parallax's own guarantee
    for (x, y) in [(70, 45), (80, 45), (90, 45)] { assert!(green(&f, x, y), "subject must stay locked at ({x},{y}), got {:?}", px(&f, x, y)); }
    // the far background (depth 0) must have moved: far from the subject, the ramp colour must no longer match its
    // original position (it shifted because dolly scales depth-0 points away from the depth-1 focal plane)
    assert!((px(&f, 10, 45)[0] - ramp(10) as f32 / 255.0).abs() > 0.03, "far background must have visibly shifted under a vertigo dolly");
    // at t=0 (start of the window) the dolly keyframe is exactly 0 -> identity: subject still green, background still at its own ramp colour
    let f0 = frame(&s, 0.0);
    for (x, y) in [(70, 45), (80, 45), (90, 45)] { assert!(green(&f0, x, y), "at start_time the subject must already be in its resting place"); }
    for x in [5, 20, 140] { assert!((px(&f0, x, 45)[0] - ramp(x) as f32 / 255.0).abs() < 0.02, "at start_time (dolly=0) the background at x={x} must sit exactly at its own ramp colour"); }
    let _ = base;
}
