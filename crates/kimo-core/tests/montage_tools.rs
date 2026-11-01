//! montage_tools (V8.3-M): score_windows / best_segments -- the non-dialogue sibling of auto_rough_cut. Each test here
//! checks a real editorial outcome (junk actually gets dropped, good footage actually survives), not just "it returns ok".
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_montage_tools_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }
fn proj() -> Session { let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 640, "height": 360, "fps": 30})); ss }

/// A clean, sharp, well-exposed, locked-off clip: a Mandelbrot zoom (lots of fine detail, no camera motion).
fn good(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=640x360:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// Heavily blurred: same source, gblur -- should score low on sharpness.
fn blurry(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=640x360:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(), "-vf", "gblur=sigma=20", "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// Near-black: should score low on exposure.
fn dark(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=640x360:r=30:d={dur}"), "-vf", "colorchannelmixer=rr=0.03:gg=0.03:bb=0.03", "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// Jittery hand-held stand-in: an oversized source jittered by a fast sin/cos crop pan -- should score high on shake.
fn shaky(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=900x600:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(),
        "-vf", "crop=640:360:x='40+35*sin(2*PI*9*t)':y='40+35*cos(2*PI*12*t)'", "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

#[test] fn a_clean_clip_scores_high_and_is_kept() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": good("clean.mp4", 3.0)})), "asset_id");
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 3.0, "in_point": 0.0})), "clip_id");
    let r = call(&mut ss, "score_windows", json!({"clip_ids": [c], "window_s": 1.0}));
    assert!(r["ok"] == json!(true) || r.get("windows").is_some(), "{r}");
    let wins = r["windows"].as_array().unwrap();
    assert!(wins.len() >= 2, "expected several windows: {r}");
    for w in wins { assert!(w["kept"] == json!(true), "clean clip window should be kept: {w}"); assert!(w["score"].as_f64().unwrap() > 0.45, "{w}"); }
}

#[test] fn blurry_dark_and_shaky_clips_are_scored_low_and_dropped() {
    let mut ss = proj();
    let ag = sid(&call(&mut ss, "import_media", json!({"path": good("g2.mp4", 2.0)})), "asset_id");
    let ab = sid(&call(&mut ss, "import_media", json!({"path": blurry("b2.mp4", 2.0)})), "asset_id");
    let ad = sid(&call(&mut ss, "import_media", json!({"path": dark("d2.mp4", 2.0)})), "asset_id");
    let ash = sid(&call(&mut ss, "import_media", json!({"path": shaky("s2.mp4", 2.0)})), "asset_id");
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let cg = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ag, "start": 0.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let cb = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ab, "start": 2.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let cd = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ad, "start": 4.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let csh = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ash, "start": 6.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let r = call(&mut ss, "score_windows", json!({"clip_ids": [cg.clone(), cb.clone(), cd.clone(), csh.clone()], "window_s": 2.0}));
    let wins = r["windows"].as_array().unwrap();
    let of = |cid: &str| -> Value { wins.iter().find(|w| w["clip_id"] == json!(cid)).unwrap().clone() };
    let (wg, wb, wd, wsh) = (of(&cg), of(&cb), of(&cd), of(&csh));
    assert!(wg["kept"] == json!(true), "good clip should be kept: {wg}");
    assert!(wb["score"].as_f64().unwrap() < wg["score"].as_f64().unwrap(), "blurry should score lower than good: {wb} vs {wg}");
    assert!(wb["kept"] == json!(false), "heavily blurred clip should be dropped: {wb}");
    assert!(wd["kept"] == json!(false), "near-black clip should be dropped: {wd}");
    assert!(wd["score_parts"]["exposure"].as_f64().unwrap() < 0.5, "{wd}");
    assert!(wsh["shake_px"].as_f64().unwrap() > wg["shake_px"].as_f64().unwrap(), "shaky clip should measure more shake than the locked-off good clip: {wsh} vs {wg}");
    assert!(wsh["kept"] == json!(false), "shaky clip should be dropped: {wsh}");
}

#[test] fn best_segments_actually_cuts_the_junk_and_keeps_the_rest() {
    let mut ss = proj();
    let ag = sid(&call(&mut ss, "import_media", json!({"path": good("g3.mp4", 2.0)})), "asset_id");
    let ab = sid(&call(&mut ss, "import_media", json!({"path": blurry("b3.mp4", 2.0)})), "asset_id");
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let cg = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ag, "start": 0.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let cb = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": ab, "start": 2.0, "duration": 2.0, "in_point": 0.0})), "clip_id");
    let r = call(&mut ss, "best_segments", json!({"clip_ids": [cg.clone(), cb.clone()], "window_s": 2.0}));
    assert_eq!(r["dropped_count"].as_u64().unwrap(), 1, "{r}");
    let kept: Vec<String> = r["kept_clips"].as_array().unwrap().iter().map(|c| c["clip_id"].as_str().unwrap().to_string()).collect();
    assert!(kept.contains(&cg), "the good clip should survive: {r}");
    assert!(!kept.contains(&cb), "the blurry clip should be gone: {r}");
    // the dropped clip is actually gone from the project, not just unlisted
    assert!(ss.project.find_clip(&cb).is_none(), "blurry clip should have been deleted from the timeline");
    assert!(ss.project.find_clip(&cg).is_some(), "good clip should still be on the timeline");
}

#[test] fn a_mixed_clip_is_split_so_only_its_good_half_survives() {
    // One clip, first half sharp, second half heavily blurred (built as two concatenated segments) -- best_segments
    // should split it and keep only the sharp half, not drop or keep the whole thing as one piece.
    let mut ss = proj();
    let p = dir().join("mixed.mp4");
    if !p.exists() {
        let half_g = good("mixed_g.mp4", 2.0); let half_b = blurry("mixed_b.mp4", 2.0);
        let list = dir().join("mixed_list.txt");
        std::fs::write(&list, format!("file '{half_g}'\nfile '{half_b}'\n")).unwrap();
        ffmpeg(&["-f", "concat", "-safe", "0", "-i", list.to_str().unwrap(), "-c", "copy", p.to_str().unwrap()]);
    }
    let a = sid(&call(&mut ss, "import_media", json!({"path": p.to_string_lossy()})), "asset_id");
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 4.0, "in_point": 0.0})), "clip_id");
    let r = call(&mut ss, "best_segments", json!({"clip_ids": [c.clone()], "window_s": 1.0}));
    let kept = r["kept_clips"].as_array().unwrap();
    assert!(!kept.is_empty(), "the sharp half should survive: {r}");
    let total_kept: f64 = kept.iter().map(|k| k["end"].as_f64().unwrap() - k["start"].as_f64().unwrap()).sum();
    assert!(total_kept < 3.0, "only the sharp half (~2s) should survive, not the whole 4s clip: {r}");
    assert!(total_kept > 0.5, "something real should survive: {r}");
}
