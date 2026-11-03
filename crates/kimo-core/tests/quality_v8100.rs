//! quality_v8100.rs: V8.10.0 content-adaptive denoise + sharpen (quality audit #6).
use kimo_core::commands::{dispatch, Session};
use kimo_core::model::Source;
use kimo_core::noise_est::laplacian_noise_sigma;
use kimo_core::render::{render_frame, Cache};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_q8100_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }
fn proj() -> Session { let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 640, "height": 360, "fps": 30})); ss }

/// Same talking-head stand-in as auto_edit.rs's own tests and quality_v890.rs: a voice-like tone in the
/// given speech windows, over a clean (unnoised) test pattern -- the dialogue track is never the clip
/// under test here, just a vehicle auto_edit needs to run at all.
fn talk(name: &str, speech: &[(f64, f64)]) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let dur = speech.iter().map(|(_, e)| *e).fold(0.0_f64, f64::max);
    let gate: Vec<String> = speech.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
    let a = format!("aevalsrc='({})*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=640x360:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A clean b-roll stand-in: a flat colour field (unlike `mandelbrot`, auto_edit.rs's/quality_v890.rs's own
/// b-roll stand-in used there for its MOTION, not its noise level -- a fractal pattern carries real fine
/// detail even in its nominally smooth regions and measures noisier than real camera footage on this
/// estimator; `gradients` was tried too and measured unstable, 0.0002 on one sampled frame and 0.014 on
/// another of the SAME undenoised clip, apparently its own dithering interacting with x264 -- a flat colour
/// field has no such surprise: zero spatial variation to dither in the first place). -crf 12 (near-lossless)
/// so compression itself adds no banding on top.
fn broll_clean(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=c=0x606060:s=640x360:r=30:d={dur}"), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "12", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A noisy b-roll stand-in: the SAME smooth `gradients` base as `broll_clean`, with ffmpeg's own
/// deterministic `noise` filter (fixed seed) added before encoding -- simulating real sensor noise on a
/// mostly-smooth background, the audit's own motivating case (a low-light phone clip: a smoothish image
/// with grain on top), and the shape this estimator is actually built to catch. NOT built on a busy
/// pattern like mandelbrot: a fractal's own fine detail can leave NO locally-flat pixel anywhere in a
/// frame as it zooms in, which starves the flat-region gate of anything to measure and reads back as
/// (wrongly) "clean" -- confirmed while writing this test. `strength` is ffmpeg's own 0..100 noise scale.
fn broll_noisy(name: &str, dur: f64, strength: u32) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let vf = format!("noise=alls={strength}:allf=t+u:all_seed=42");
    // Same flat colour base as `broll_clean`, plus ffmpeg's own deterministic noise filter -- the ONLY
    // variable between the two is the injected per-pixel noise. -crf 12 (near-lossless), NOT ultrafast's
    // own lossier default: measured directly while writing this test, on a smooth `gradients` base, at
    // the default (lossier) crf x264's rate control crushed the injected noise inconsistently frame to
    // frame (one sampled frame measuring the real injected noise, the very next measuring as clean as the
    // undenoised source, on the SAME clip); crf 12 measured rock solid across every sampled frame on both
    // bases, so it is used here too rather than re-risking the same instability.
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=c=0x606060:s=640x360:r=30:d={dur}"), "-vf", &vf, "-an", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "12", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// Grayscale (0..=1) of a rendered RGBA frame buffer, Rec709 luma -- the same convention `tracking::gray`
/// uses, reimplemented here so the test doesn't need a non-pub import.
fn render_gray(f: &kimo_core::effects::Frame) -> Vec<f32> {
    f.d.chunks_exact(4).map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).collect()
}

/// Mean absolute Sobel-ish gradient magnitude over a whole grayscale frame -- a simple, whole-frame
/// "how much real detail/edge energy is left" proxy, used to check the compensating sharpen keeps edge
/// sharpness from collapsing under the denoise pass.
fn mean_gradient_energy(gray: &[f32], w: usize, h: usize) -> f64 {
    let at = |x: usize, y: usize| gray[y * w + x] as f64;
    let mut sum = 0.0f64; let mut n = 0u64;
    for y in 1..h - 1 { for x in 1..w - 1 {
        let gx = at(x + 1, y) - at(x - 1, y);
        let gy = at(x, y + 1) - at(x, y - 1);
        sum += (gx * gx + gy * gy).sqrt(); n += 1;
    } }
    sum / n as f64
}

fn find_clip_effect<'a>(ss: &'a Session, cid: &str, kind: &str) -> Option<&'a kimo_core::model::Effect> {
    ss.project.find_clip(cid).and_then(|c| c.effects.iter().find(|e| e.kind == kind))
}

/// Pure unit test of the estimator itself (quality audit #6's own TDD list, item 2): a synthetic noisy
/// frame must measure noisier than the same base frame with no noise added. Deterministic hand-rolled
/// noise (same small LCG shape as color/denoise.rs's own test module), not ffmpeg, so this test needs no
/// decode at all.
#[test]
fn laplacian_noise_sigma_measures_a_noisy_frame_noisier_than_a_clean_one() {
    const W: usize = 64; const H: usize = 48;
    struct Lcg(u64);
    impl Lcg {
        fn u(&mut self) -> f32 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((self.0 >> 40) as f32) / (1u64 << 24) as f32 }
        fn g(&mut self, s: f32) -> f32 { ((self.u() + self.u() + self.u() + self.u()) - 2.0) * s * 1.732 }
    }
    // a smooth gradient base (no hard edges), identical for both -- isolates the noise as the only variable
    let base: Vec<f32> = (0..H).flat_map(|y| (0..W).map(move |x| 0.3 + 0.4 * (x as f32 / W as f32) + 0.1 * (y as f32 / H as f32))).collect();
    let clean = base.clone();
    let mut r = Lcg(42);
    let noisy: Vec<f32> = base.iter().map(|&v| (v + r.g(0.05)).clamp(0.0, 1.0)).collect();

    let sigma_clean = laplacian_noise_sigma(&clean, W, H, 0.02);
    let sigma_noisy = laplacian_noise_sigma(&noisy, W, H, 0.02);
    assert!(sigma_clean < 0.001, "a perfectly smooth gradient should measure near-zero noise, got {sigma_clean}");
    assert!(sigma_noisy > sigma_clean * 5.0, "injected noise should measure clearly noisier: clean={sigma_clean} noisy={sigma_noisy}");
}

/// Degenerate-input safety: a too-small buffer or a fully-textured frame (every pixel flagged as an edge,
/// so there are no flat pixels to measure) must return 0.0, never panic or divide by zero.
#[test]
fn laplacian_noise_sigma_handles_degenerate_input() {
    assert_eq!(laplacian_noise_sigma(&[0.5, 0.5, 0.5, 0.5], 2, 2, 0.02), 0.0, "2x2 has no interior pixel");
    // A steep monotonic ramp: every adjacent pixel differs by well over the flat threshold, so no pixel
    // anywhere in the frame counts as flat (a period-2 checkerboard would NOT prove this: x-1/x+1 are the
    // same parity and a skip-one gradient cancels exactly for it -- the adjacent-neighbour gate this
    // estimator actually uses is what makes a steep ramp, and a period-2 pattern alike, correctly non-flat).
    let steep: Vec<f32> = (0..25).map(|i| (i % 5) as f32 * 0.5).collect();
    assert_eq!(laplacian_noise_sigma(&steep, 5, 5, 0.001), 0.0, "a steep ramp with a tight flat threshold has no flat pixels anywhere");
    let checker: Vec<f32> = (0..25).map(|i| if (i % 5 + i / 5) % 2 == 0 { 0.0 } else { 1.0 }).collect();
    assert_eq!(laplacian_noise_sigma(&checker, 5, 5, 0.001), 0.0, "a true 2D checkerboard (period-2 in both axes) is caught by the adjacent-neighbour flat gate too");
}

/// quality audit #6's own TDD list, item 3: a clean media clip's noise measures below the floor and gets
/// no denoise/sharpen effect added at all -- in the SAME edit as a genuinely noisy clip that DOES get one,
/// proving the decision is per-clip (content-adaptive), not a blanket on/off.
#[test]
fn clean_clip_gets_no_filter_noisy_clip_in_the_same_edit_does() {
    let mut ss = proj();
    let th = sid(&call(&mut ss, "import_media", json!({"path": talk("q8100_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let clean = sid(&call(&mut ss, "import_media", json!({"path": broll_clean("q8100_clean.mp4", 2.0)})), "asset_id");
    let noisy = sid(&call(&mut ss, "import_media", json!({"path": broll_noisy("q8100_noisy.mp4", 2.0, 8)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [th], "broll_asset_ids": [clean, noisy], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["adaptive_denoise"], json!(true), "adaptive_denoise should default to on inside auto_edit: {r}");

    let clips = r["stages"]["adaptive_denoise"]["clips"].as_array().unwrap_or_else(|| panic!("no adaptive_denoise stage: {r}"));
    // The dialogue (talking-head) clip is measured too -- find the two b-roll entries specifically by asset
    // rather than assuming the stage array holds exactly the b-roll clips.
    let clean_cid0 = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &clean)).map(|c| c.id.clone()).unwrap();
    let noisy_cid0 = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &noisy)).map(|c| c.id.clone()).unwrap();
    let clean_entry = clips.iter().find(|c| sid(c, "clip_id") == clean_cid0).unwrap_or_else(|| panic!("no adaptive_denoise entry for the clean b-roll clip: {clips:?}"));
    let noisy_entry = clips.iter().find(|c| sid(c, "clip_id") == noisy_cid0).unwrap_or_else(|| panic!("no adaptive_denoise entry for the noisy b-roll clip: {clips:?}"));
    assert!(clean_entry["noise_sigma"].as_f64().unwrap() < noisy_entry["noise_sigma"].as_f64().unwrap(), "the clean clip should measure less noisy than the noisy one: {clips:?}");
    assert_eq!(clean_entry["denoise_amount"].as_f64().unwrap(), 0.0, "the clean clip should get no denoise amount: {clips:?}");
    assert!(noisy_entry["denoise_amount"].as_f64().unwrap() > 0.0, "the noisy clip should get a nonzero denoise amount: {clips:?}");

    let clean_cid = sid(clean_entry, "clip_id");
    let noisy_cid = sid(noisy_entry, "clip_id");
    assert!(find_clip_effect(&ss, &clean_cid, "denoise").is_none(), "clean clip must have no denoise effect added");
    assert!(find_clip_effect(&ss, &clean_cid, "sharpen").is_none(), "clean clip must have no sharpen effect added");
    assert!(find_clip_effect(&ss, &noisy_cid, "denoise").is_some(), "noisy clip must have a denoise effect added");
    assert!(find_clip_effect(&ss, &noisy_cid, "sharpen").is_some(), "noisy clip must have a compensating sharpen effect added");

    // Linear-light rule: regardless of what the dialogue (talking-head) clip itself measures, no
    // Text/Solid/Gradient clip is ever a candidate -- estimate_clip_noise only accepts Source::Media, so
    // there is nothing further to assert here beyond "only media clips ever appear in the stage".
    for c in clips { let cid = sid(c, "clip_id"); let clip = ss.project.find_clip(&cid).unwrap(); assert!(matches!(clip.source, Source::Media { .. }), "only media clips should ever be scanned"); }
}

/// quality audit #6's own TDD list, item 1: adaptive_denoise:false must add no denoise/sharpen effect to
/// any clip, however noisy the source -- the gate actually gates the behaviour, it is not just reported
/// back unused. This is the "off is bit-identical" guarantee: with no effect ever added, decode + composite
/// for every clip is byte-for-byte what it was before this version.
#[test]
fn adaptive_denoise_false_adds_no_effect_to_a_genuinely_noisy_clip() {
    let mut ss = proj();
    let th = sid(&call(&mut ss, "import_media", json!({"path": talk("q8100_off_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy = sid(&call(&mut ss, "import_media", json!({"path": broll_noisy("q8100_off_noisy.mp4", 2.0, 8)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [th], "broll_asset_ids": [noisy], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false, "adaptive_denoise": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["adaptive_denoise"], json!(false));
    assert!(r["stages"]["adaptive_denoise"].is_null(), "adaptive_denoise:false should write no stage: {r}");
    assert_eq!(ss.project.adaptive_denoise, false, "the project marker field must stay false when the gate is off");
    for tr in &ss.project.tracks { for c in &tr.clips {
        assert!(!c.effects.iter().any(|e| e.kind == "denoise" || e.kind == "sharpen"), "clip {} got a denoise/sharpen effect with adaptive_denoise:false: {:?}", c.id, c.effects);
    } }
}

/// Leaving adaptive_denoise unset must behave exactly like explicitly true (both default to on inside
/// auto_edit, the same unwrap_or(true) convention as color_tag_sanity/motion_blur/frame_blend_conform).
#[test]
fn adaptive_denoise_unset_defaults_to_true() {
    let mut ss = proj();
    let th = sid(&call(&mut ss, "import_media", json!({"path": talk("q8100_unset_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy = sid(&call(&mut ss, "import_media", json!({"path": broll_noisy("q8100_unset_noisy.mp4", 2.0, 8)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [th], "broll_asset_ids": [noisy], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert_eq!(r["adaptive_denoise"], json!(true));
    assert!(!r["stages"]["adaptive_denoise"]["clips"].as_array().unwrap().is_empty());
}

/// quality audit #6's own TDD list, item 4: the noisy clip's RENDERED output must measure lower noise than
/// the same clip rendered with the gate off -- proving the effect isn't just added to the project but
/// actually reduces measured noise on the pixels that get encoded.
#[test]
fn noisy_clip_rendered_output_has_lower_noise_variance() {
    let dur = 2.0;
    let mut ss_on = proj();
    let th_on = sid(&call(&mut ss_on, "import_media", json!({"path": talk("q8100_var_th_on.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy_on = sid(&call(&mut ss_on, "import_media", json!({"path": broll_noisy("q8100_var_noisy.mp4", dur, 8)})), "asset_id");
    let r_on = call(&mut ss_on, "auto_edit", json!({"talking_head_asset_ids": [th_on], "broll_asset_ids": [noisy_on], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false}));
    assert!(r_on["ok"] == json!(true), "{r_on}");
    // Both the dialogue (talking-head) clip and the noisy b-roll clip get measured; find the b-roll one
    // specifically (the one whose asset is noisy_on) rather than assuming the stage has exactly one entry.
    let on_cid = ss_on.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &noisy_on)).map(|c| c.id.clone()).unwrap_or_else(|| panic!("no b-roll clip found in the on session"));
    let on_clips = r_on["stages"]["adaptive_denoise"]["clips"].as_array().unwrap();
    let on_entry = on_clips.iter().find(|c| sid(c, "clip_id") == on_cid).unwrap_or_else(|| panic!("no adaptive_denoise entry for the b-roll clip: {on_clips:?}"));
    assert!(on_entry["denoise_amount"].as_f64().unwrap() > 0.0, "the noisy clip should have gotten a denoise amount: {on_clips:?}");
    let cid_on = on_cid;
    let clip_on = ss_on.project.find_clip(&cid_on).unwrap().clone();

    let mut ss_off = proj();
    let th_off = sid(&call(&mut ss_off, "import_media", json!({"path": talk("q8100_var_th_off.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy_off = sid(&call(&mut ss_off, "import_media", json!({"path": broll_noisy("q8100_var_noisy.mp4", dur, 8)})), "asset_id");
    let r_off = call(&mut ss_off, "auto_edit", json!({"talking_head_asset_ids": [th_off], "broll_asset_ids": [noisy_off], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false, "adaptive_denoise": false}));
    assert!(r_off["ok"] == json!(true), "{r_off}");
    // Find the matching b-roll clip in the OFF session (same source asset) by scanning tracks for a media
    // clip whose asset is noisy_off and whose timeline start matches the ON clip's (place_broll is
    // deterministic given identical inputs, so both sessions place it at the same point).
    let off_cid = ss_off.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &noisy_off)).map(|c| c.id.clone()).unwrap_or_else(|| panic!("no b-roll clip found in the off session"));
    let clip_off2 = ss_off.project.find_clip(&off_cid).unwrap().clone();
    assert!(!clip_off2.effects.iter().any(|e| e.kind == "denoise"), "sanity: off clip must carry no denoise effect");

    let mut cache_on = Cache::new();
    kimo_core::render::prepare_range(&ss_on.project, &mut cache_on, clip_on.start + 0.1, clip_on.start + 0.1).unwrap();
    let f_on = render_frame(&ss_on.project, clip_on.start + 0.1, &cache_on).unwrap();

    let mut cache_off = Cache::new();
    kimo_core::render::prepare_range(&ss_off.project, &mut cache_off, clip_off2.start + 0.1, clip_off2.start + 0.1).unwrap();
    let f_off = render_frame(&ss_off.project, clip_off2.start + 0.1, &cache_off).unwrap();

    let gray_on = render_gray(&f_on);
    let gray_off = render_gray(&f_off);
    let sigma_on = laplacian_noise_sigma(&gray_on, f_on.w, f_on.h, 0.02);
    let sigma_off = laplacian_noise_sigma(&gray_off, f_off.w, f_off.h, 0.02);
    assert!(sigma_on < sigma_off, "denoised render should measure lower noise than undenoised: on={sigma_on} off={sigma_off}");
    assert!(sigma_on < sigma_off * 0.85, "the drop should be a real, meaningful reduction, not noise in the measurement itself: on={sigma_on} off={sigma_off}");
}

/// quality audit #6's own TDD list, item 5: edge sharpness doesn't drop. The compensating sharpen pass
/// should keep whole-frame edge/detail energy from collapsing under the denoise pass -- allow some give
/// (denoise's own softening isn't fully, perfectly undone by a cheaper sharpen) but it must not fall off a
/// cliff, and the denoised+sharpened render should clearly out-sharpen a denoise-only render at the same
/// amount (proving the sharpen pass is doing real work, not a no-op).
#[test]
fn edge_sharpness_does_not_collapse_under_denoise() {
    let dur = 2.0;
    let mut ss = proj();
    let th = sid(&call(&mut ss, "import_media", json!({"path": talk("q8100_edge_th.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy = sid(&call(&mut ss, "import_media", json!({"path": broll_noisy("q8100_edge_noisy.mp4", dur, 8)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [th], "broll_asset_ids": [noisy], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let clips = r["stages"]["adaptive_denoise"]["clips"].as_array().unwrap();
    // The dialogue clip is measured too and may land at any index; find the noisy b-roll entry by asset.
    let cid = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &noisy)).map(|c| c.id.clone()).unwrap();
    let denoise_amount = clips.iter().find(|c| sid(c, "clip_id") == cid).unwrap()["denoise_amount"].as_f64().unwrap();
    assert!(denoise_amount > 0.0);
    let clip = ss.project.find_clip(&cid).unwrap().clone();

    // Render as-is (denoise + compensating sharpen).
    let mut cache = Cache::new();
    kimo_core::render::prepare_range(&ss.project, &mut cache, clip.start + 0.1, clip.start + 0.1).unwrap();
    let f_both = render_frame(&ss.project, clip.start + 0.1, &cache).unwrap();
    let energy_both = mean_gradient_energy(&render_gray(&f_both), f_both.w, f_both.h);

    // Remove just the sharpen effect and re-render (denoise only, same amount) to isolate its contribution.
    let sharpen_eid = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "sharpen").unwrap().id.clone();
    call(&mut ss, "remove_effect", json!({"clip_id": cid, "effect_id": sharpen_eid}));
    let mut cache2 = Cache::new();
    kimo_core::render::prepare_range(&ss.project, &mut cache2, clip.start + 0.1, clip.start + 0.1).unwrap();
    let f_denoise_only = render_frame(&ss.project, clip.start + 0.1, &cache2).unwrap();
    let energy_denoise_only = mean_gradient_energy(&render_gray(&f_denoise_only), f_denoise_only.w, f_denoise_only.h);

    // Undenoised baseline (off session) for the floor this is being compared against.
    let mut ss_off = proj();
    let th_off = sid(&call(&mut ss_off, "import_media", json!({"path": talk("q8100_edge_th_off.mp4", &[(0.0, 4.0), (4.5, 8.0)])})), "asset_id");
    let noisy_off = sid(&call(&mut ss_off, "import_media", json!({"path": broll_noisy("q8100_edge_noisy.mp4", dur, 8)})), "asset_id");
    let r_off = call(&mut ss_off, "auto_edit", json!({"talking_head_asset_ids": [th_off], "broll_asset_ids": [noisy_off], "captions": false, "grade": false, "broll_color_match": false, "end_screen": false, "motion_blur": false, "push_in": false, "punch_ins": false, "stabilize_handheld": false, "adaptive_denoise": false}));
    assert!(r_off["ok"] == json!(true));
    let off_cid = ss_off.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| matches!(&c.source, Source::Media { asset } if asset == &noisy_off)).map(|c| c.id.clone()).unwrap();
    let clip_off = ss_off.project.find_clip(&off_cid).unwrap().clone();
    let mut cache3 = Cache::new();
    kimo_core::render::prepare_range(&ss_off.project, &mut cache3, clip_off.start + 0.1, clip_off.start + 0.1).unwrap();
    let f_off = render_frame(&ss_off.project, clip_off.start + 0.1, &cache3).unwrap();
    let energy_off = mean_gradient_energy(&render_gray(&f_off), f_off.w, f_off.h);

    assert!(energy_both > energy_denoise_only, "the compensating sharpen should measurably raise edge energy back up versus denoise alone: with_sharpen={energy_both} denoise_only={energy_denoise_only}");
    assert!(energy_both > energy_off * 0.75, "edge sharpness should not collapse: denoised+sharpened={energy_both} undenoised baseline={energy_off}");
}
