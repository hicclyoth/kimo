//! An open path (no Z) was closed with a real edge, so its stroke grew a chord and trim's arc length counted it.
//! SVG closes an open path for the fill only. Written red against kimo 8.12.1.
use kimo_core::commands::{dispatch, Session};
use kimo_core::paths::Path;
use serde_json::{json, Value};

fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
const OPEN: &str = "M0 0 L100 0 L100 100";

#[test] fn open_path_length_has_no_closing_edge() { let p = Path::parse(OPEN, false).unwrap(); assert!((p.total - 200.0).abs() < 1e-6, "length {}", p.total); }

#[test] fn point_on_the_chord_is_not_on_an_open_path() { let p = Path::parse(OPEN, false).unwrap(); let (d, _) = p.nearest(30.0, 30.0); assert!((d - 30.0).abs() < 1e-6, "distance {d}"); }

#[test] fn arc_position_ignores_the_chord() { let p = Path::parse(OPEN, false).unwrap(); let (_, u) = p.nearest(100.0, 50.0); assert!((u - 0.75).abs() < 1e-6, "u {u}"); }

/// Fills must not change: the region is still closed.
#[test] fn open_path_still_fills_as_closed() { let p = Path::parse(OPEN, false).unwrap(); assert!(p.sd(80.0, 20.0) < 0.0); assert!(p.sd(20.0, 80.0) > 0.0); assert!((p.sd(30.0, 31.0).abs() - 0.7071).abs() < 0.01, "fill edge stays on the chord"); }

#[test] fn closed_path_is_unchanged() { let p = Path::parse("M0 0 L100 0 L100 100 Z", false).unwrap(); assert!((p.total - (200.0 + 20000f64.sqrt())).abs() < 1e-6); assert!(p.nearest(30.0, 30.0).0 < 1e-6); }

fn alpha_at(ss: &mut Session, name: &str, x: usize, y: usize) -> u8 {
    let f = std::env::temp_dir().join(format!("kimo_open_{}_{name}.png", std::process::id()));
    call(ss, "render_frame", json!({"time": 0.1, "path": f.to_str().unwrap(), "inline": "never"}));
    let o = std::process::Command::new("ffmpeg").args(["-v", "error", "-i", f.to_str().unwrap(), "-f", "rawvideo", "-pix_fmt", "gray", "-"]).output().unwrap();
    let _ = std::fs::remove_file(&f); o.stdout[y * 320 + x]
}

/// The drawn stroke follows the two real edges and leaves the chord empty.
#[test] fn stroke_of_an_open_path_draws_no_chord() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 320, "height": 320, "fps": 25}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_path_shape", json!({"track_id": t, "start": 0, "duration": 1, "d": "M-100 -100 L100 -100 L100 100", "fill": [-1, 0, 0], "stroke_width": 8, "stroke_color": [1, 1, 1]}));
    assert!(alpha_at(&mut ss, "edge", 160, 60) > 200, "the real edge must draw");
    assert!(alpha_at(&mut ss, "chord", 160, 160) < 30, "the chord must stay empty");
}

/// The far end of an open path is not its start: a trim that has only drawn the first part leaves the end empty.
#[test] fn trim_leaves_the_far_end_of_an_open_path_empty() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 320, "height": 320, "fps": 25}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    let c = call(&mut ss, "add_path_shape", json!({"track_id": t, "start": 0, "duration": 1, "d": "M-100 -100 L100 -100 L100 100", "fill": [-1, 0, 0], "stroke_width": 8, "stroke_color": [1, 1, 1]}))["clip_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_shape_modifier", json!({"clip_id": c, "kind": "trim", "params": {"start": 0, "end": 0.3, "offset": 0}}));
    assert!(alpha_at(&mut ss, "drawn", 100, 60) > 200, "the drawn part must show");
    assert!(alpha_at(&mut ss, "end", 260, 262) < 30, "the far end must stay empty");
}
