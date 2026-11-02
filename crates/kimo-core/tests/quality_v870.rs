//! V8.7.0 TDD: linear-light compositing, lanczos fit and TPDF dither for auto_edit's picture-quality
//! option (project.linear_blend, the same switch across all three -- a fast compositing-only option, see
//! render.rs::blend_region_opt and ext/auto_edit.rs; NOT the heavier set_color_pipeline, which measured
//! ~3.4x slower end to end). Written before the wiring landed, per the brief's "write a failing test first".
use kimo_core::{commands::{Session,dispatch},render,effects::Frame};
use serde_json::json;

/// Black solid (bottom track) crossfading to white (top track) over exactly 1s, linear ease, so the
/// midpoint (t=0.5) has op=0.5 exactly -- the same synthetic crossfade shape the audit used to measure
/// 128/255 (gamma-space naive average) vs 188/255 (linear_blend on, the correct linear-light blend).
/// 16x16 is the smallest canvas project_set accepts (16..16384).
fn crossfade_project(linear_on: bool) -> Session {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 16, "height": 16, "fps": 30, "linear_blend": linear_on})).unwrap();
    let bottom = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let top = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_clip", &json!({"track_id": bottom, "source_type": "solid", "color": [0.0, 0.0, 0.0], "start": 0.0, "duration": 2.0})).unwrap();
    let white = dispatch(&mut s, "add_clip", &json!({"track_id": top, "source_type": "solid", "color": [1.0, 1.0, 1.0], "start": 0.0, "duration": 1.0})).unwrap()["clip_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_transition", &json!({"clip_id": white, "kind": "crossfade", "duration": 1.0, "ease": "linear"})).unwrap();
    s
}

fn midpoint_r(linear_on: bool) -> u8 {
    let s = crossfade_project(linear_on);
    let cache = render::Cache::new();
    let bytes = render::export_frame_rgba8(&s.project, 0.5, &cache).unwrap();
    bytes[0]
}

#[test]
fn crossfade_midpoint_is_188_with_the_option_on() {
    let v = midpoint_r(true);
    assert!((v as i32 - 188).abs() <= 3, "expected ~188/255 (linear-light blend) with linear_blend on, got {v}");
}

#[test]
fn crossfade_midpoint_is_128_with_the_option_off() {
    let v = midpoint_r(false);
    assert!((v as i32 - 128).abs() <= 1, "expected ~128/255 (naive gamma-space average) by default, got {v}");
}

#[test]
fn lanczos_is_selected_in_the_fit_filter_string_when_the_option_is_on() {
    let on = render::clip_fit_filter(640, 360, 0.0, true);
    let off = render::clip_fit_filter(640, 360, 0.0, false);
    assert!(on.contains("flags=lanczos"), "expected flags=lanczos in the fit filter when on: {on}");
    assert!(!off.contains("flags=lanczos"), "expected no lanczos flag by default: {off}");
    // every fit mode (stretch/cover/contain) carries the flag when on, and none of them do when off
    for fit in [0.0, 0.7, 1.7] {
        assert!(render::clip_fit_filter(640, 360, fit, true).contains("flags=lanczos"));
        assert!(!render::clip_fit_filter(640, 360, fit, false).contains("flags=lanczos"));
    }
}

#[test]
fn option_off_is_bit_identical_to_before() {
    // The default fit filter string (no flags=) is unchanged from the pre-8.7.0 shape.
    assert_eq!(render::clip_fit_filter(1280, 720, 1.7, false), "scale=1280:720");
    assert_eq!(render::clip_fit_filter(1280, 720, 0.7, false), "scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720");
    assert_eq!(render::clip_fit_filter(1280, 720, 0.0, false), "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2:color=black@0");
    // And the crossfade's own default-off output is exactly the old naive gamma-space average (128, +/-1
    // for 8-bit rounding of 127.5), matching what the engine produced before this commit.
    let v = midpoint_r(false);
    assert!((v as i32 - 128).abs() <= 1);
    // Plain to_rgba8 (no dither) is untouched: deterministic exact rounding, no option threaded through it.
    let f = Frame { w: 2, h: 1, d: vec![0.5, 0.5, 0.5, 1.0, 0.25, 0.25, 0.25, 1.0] };
    assert_eq!(render::to_rgba8(&f), vec![128, 128, 128, 255, 64, 64, 64, 255]);
}

/// TPDF dither over a realistic banding shape: a SHALLOW gradient (0.10..0.30, the kind of soft sky/
/// blurred-backdrop slope the audit flagged) spread across a WIDE frame, so without dither each 8-bit code
/// repeats for many consecutive pixels (long flat terraces -- the textbook banding defect this exists to
/// kill). With dither: no long terraces, level coverage does not shrink, and the per-pixel error against
/// the true analog value is small, unbiased, zero-mean noise (not a systematic shift).
#[test]
fn dither_produces_no_banding_steps_over_a_smooth_gradient() {
    let w = 2000usize;
    let h = 4usize;
    let (lo, hi): (f32, f32) = (0.10, 0.30);
    let mut f = Frame { w, h, d: vec![0.0; w * h * 4] };
    for y in 0..h { for x in 0..w {
        let v = lo + (hi - lo) * x as f32 / (w - 1) as f32;
        let i = (y * w + x) * 4;
        f.d[i] = v; f.d[i + 1] = v; f.d[i + 2] = v; f.d[i + 3] = 1.0;
    }}
    let plain = render::to_rgba8(&f);
    let dithered = render::to_rgba8_tpdf(&f);

    let row = |bytes: &[u8]| -> Vec<u8> { (0..w).map(|x| bytes[x * 4]).collect() };
    let plain_row = row(&plain);
    let dith_row = row(&dithered);

    let unique = |r: &[u8]| -> usize { let mut v = r.to_vec(); v.sort_unstable(); v.dedup(); v.len() };
    let max_run = |r: &[u8]| -> usize { let mut best = 1usize; let mut cur = 1usize; for i in 1..r.len() { if r[i] == r[i - 1] { cur += 1; best = best.max(cur); } else { cur = 1; } } best };

    let plain_levels = unique(&plain_row);
    let dith_levels = unique(&dith_row);
    let plain_run = max_run(&plain_row);
    let dith_run = max_run(&dith_row);

    assert!(plain_run > 10, "sanity: the undithered ramp should actually band (long flat runs), got max run {plain_run}");
    assert!(dith_run < plain_run, "dither should break up banding terraces: plain max run {plain_run}, dithered max run {dith_run}");
    assert!(dith_levels >= plain_levels, "dither should not reduce level coverage: plain={plain_levels} dithered={dith_levels}");

    let errs: Vec<f64> = (0..w).map(|x| { let ideal = (lo + (hi - lo) * x as f32 / (w - 1) as f32) as f64 * 255.0; dith_row[x] as f64 - ideal }).collect();
    let mean: f64 = errs.iter().sum::<f64>() / errs.len() as f64;
    let var: f64 = errs.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / errs.len() as f64;
    let std = var.sqrt();
    assert!(mean.abs() < 0.2, "dither should not bias the ramp, mean error {mean:.3}");
    assert!((0.15..0.9).contains(&std), "dither error std should show real triangular-PDF noise (~0.3-0.5 LSB), got {std:.3}");
}

#[test]
fn auto_edit_turns_on_color_pipeline_as_its_last_step() {
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("kimo_q870_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("th.mp4");
    if !p.exists() {
        // voice-like tone (see tests/auto_edit.rs's own `talk()` helper): plain sine/testsrc has "no
        // speech found" rejected by auto_rough_cut's VAD.
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
    assert_eq!(r.get("linear_compositing"), Some(&serde_json::Value::Bool(true)), "auto_edit's own result should say linear_compositing: true by default");
    assert!(s.project.linear_blend, "auto_edit should have turned linear_blend on as its last step");
    assert!(!s.project.color_pipeline.enabled, "auto_edit's fast linear_blend option must NOT also flip the heavier color_pipeline");
}
