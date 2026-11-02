//! KIMO 8.5 vfx look library: every named look (vfx_look_library::NAMES) builds a valid graph, is reachable
//! through apply_vfx_preset alongside V8.4's own 4 presets, and actually changes the pixels.
use kimo_core::commands::{dispatch, Session};
use kimo_core::vfx_graph::{parse_graph, run_graph, validate};
use kimo_core::vfx_look_library;
use kimo_core::effects::Frame;
use serde_json::json;

fn make_clip(ss: &mut Session) -> String {
    dispatch(ss, "project_set", &json!({"width": 32, "height": 32, "fps": 10.0})).unwrap();
    let tr = dispatch(ss, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    dispatch(ss, "add_shape", &json!({"track_id": tr, "shape": "rect", "width": 32, "height": 32, "color": [0.4, 0.3, 0.5], "start": 0, "duration": 2})).unwrap()["clip_id"].as_str().unwrap().to_string()
}

#[test]
fn every_named_look_builds_a_valid_graph_at_every_intensity() {
    for name in vfx_look_library::NAMES {
        for i in [0.0, 0.4, 1.0] {
            let g = vfx_look_library::graph_for(name, i).unwrap_or_else(|e| panic!("{name}@{i}: {e}"));
            let parsed = parse_graph(&g).unwrap_or_else(|e| panic!("{name}@{i}: graph is not valid vfx_graph JSON: {e}"));
            validate(&parsed).unwrap_or_else(|e| panic!("{name}@{i}: {e}"));
        }
        assert!(vfx_look_library::describe(name).is_some(), "{name} has no describe() text");
    }
}

#[test]
fn an_unknown_look_name_is_a_clear_error() {
    let err = vfx_look_library::graph_for("not_a_real_look", 0.5).unwrap_err();
    assert!(err.contains("unknown vfx look"), "{err}");
}

#[test]
fn every_look_actually_changes_the_frame_when_rendered() {
    // a flat colour is a degenerate case for several of these (chromatic_aberration/sharpen on a flat field is a
    // no-op, bloom needs something above its threshold) -- a bright soft rect on a darker field gives every look
    // real edges and real highlights to work with, the way a real shot would.
    let src = { let mut f = Frame::new(32, 32); for y in 0..32 { for x in 0..32 {
        let i = y * 32 + x; let bright = x >= 10 && x < 22 && y >= 10 && y < 22;
        let (r, g, b) = if bright { (0.95, 0.9, 0.8) } else { (0.08 + 0.1 * (x as f32 / 32.0), 0.06, 0.12 + 0.1 * (y as f32 / 32.0)) };
        f.d[i * 4] = r; f.d[i * 4 + 1] = g; f.d[i * 4 + 2] = b; f.d[i * 4 + 3] = 1.0;
    } } f };
    for name in vfx_look_library::NAMES {
        let g = vfx_look_library::graph_for(name, 0.8).unwrap();
        let out = run_graph(&g.to_string(), &src, None, 0.5, 1).unwrap_or_else(|e| panic!("{name}: render failed: {e}"));
        assert_ne!(out.d, src.d, "{name} at intensity 0.8 should visibly change a flat test frame");
    }
}

/// 8.5.4: the montage review's #3 finding. chrome_bloom's "cool" grade push used the `temperature`
/// effect, whose amount is scaled 0.2x internally (the same trap frost_veil hit and was fixed for in
/// 8.5.2 -- see that look's own comment) -- a flat +-0.07ish additive RGB shift at the montage's own
/// default intensity (0.45), easily swamped by a busy, warmly-lit real shot. It must read clearly at
/// that same default intensity, on a representative warm/skin-toned frame, without clipping highlights.
fn warm_real_footage_like_frame() -> Frame {
    // warm midtone "skin" field with per-pixel texture (not flat -- chromatic_aberration/sharpen are
    // near no-ops on a flat field) plus a bright warm highlight patch, roughly like a warmly-lit
    // talking-head shot: nothing already near 1.0 before the look runs.
    let mut f = Frame::new(48, 48);
    for y in 0..48 { for x in 0..48 {
        let i = y * 48 + x;
        let tex = ((x * 7 + y * 13) % 11) as f32 / 11.0 * 0.06;
        let highlight = x >= 30 && x < 42 && y >= 4 && y < 14;
        let (r, g, b) = if highlight { (0.88, 0.74, 0.55) } else { (0.62 + tex, 0.46 + tex * 0.8, 0.38 + tex * 0.6) };
        f.d[i * 4] = r; f.d[i * 4 + 1] = g; f.d[i * 4 + 2] = b; f.d[i * 4 + 3] = 1.0;
    } }
    f
}
fn mean_rgb_and_clipped_pct(f: &Frame) -> ([f64; 3], f64) {
    let n = (f.w * f.h) as usize;
    let mut sum = [0f64; 3]; let mut clipped = 0usize;
    for i in 0..n {
        for c in 0..3 { sum[c] += f.d[i * 4 + c] as f64; }
        if f.d[i * 4..i * 4 + 3].iter().any(|v| *v >= 0.98) { clipped += 1; }
    }
    ([sum[0] / n as f64, sum[1] / n as f64, sum[2] / n as f64], clipped as f64 * 100.0 / n as f64)
}
#[test]
fn chrome_bloom_cool_push_reads_clearly_on_warm_real_footage_without_clipping() {
    let src = warm_real_footage_like_frame();
    let (src_rgb, _) = mean_rgb_and_clipped_pct(&src);
    let g = vfx_look_library::graph_for("chrome_bloom", 0.45).unwrap(); // auto_edit's own default intensity
    let out = run_graph(&g.to_string(), &src, None, 0.5, 1).unwrap();
    let (out_rgb, clipped_pct) = mean_rgb_and_clipped_pct(&out);
    // "cool" means blue rises and red falls relative to the source -- the net red->blue swing is the
    // one-number proxy for "reads as a deliberate cool/chrome grade" rather than "a nice grade".
    let cool_swing = (out_rgb[2] - src_rgb[2]) - (out_rgb[0] - src_rgb[0]);
    assert!(cool_swing > 0.15, "chrome_bloom@0.45 on warm footage should swing clearly blue-over-red, got {cool_swing:.4} (src {src_rgb:?} -> out {out_rgb:?}) -- temperature's internal 0.2x scale is the known trap (see frost_veil's own 8.5.2 fix)");
    assert!(clipped_pct < 25.0, "chrome_bloom@0.45 should not wash out highlights: {clipped_pct:.1}% of the frame clipped");

    // V8.5.5: the montage review's #2 finding -- the OLD push swung an R-B delta of about +123 (0..255
    // scale) vs the source at full intensity, reading as a near-total warm->cool inversion (a white-balance
    // glitch) rather than a grade. This ceilings the swing at full intensity so chrome_bloom stays "clearly
    // cooler and shinier" without crossing into that inversion read; 80 sits comfortably above the new
    // push's own ~66 and well under the old push's ~123, so a regression back toward the old strength trips
    // it.
    let g_full = vfx_look_library::graph_for("chrome_bloom", 1.0).unwrap();
    let out_full = run_graph(&g_full.to_string(), &src, None, 0.5, 1).unwrap();
    let (out_full_rgb, clipped_full_pct) = mean_rgb_and_clipped_pct(&out_full);
    let full_swing_255 = ((out_full_rgb[2] - src_rgb[2]) - (out_full_rgb[0] - src_rgb[0])) * 255.0;
    assert!(full_swing_255 <= 80.0, "chrome_bloom@1.0's red->blue swing vs source should stay well short of a full color inversion (ceiling ~80 on a 0..255 scale); got {full_swing_255:.1}");
    assert!(clipped_full_pct < 25.0, "chrome_bloom@1.0 should not wash out highlights: {clipped_full_pct:.1}% of the frame clipped");
}

#[test]
fn apply_vfx_preset_reaches_every_v85_look_and_keeps_v84_working() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    for name in vfx_look_library::NAMES {
        let r = dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": name, "intensity": 0.6})).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r["preset"].as_str(), Some(name));
        let eid = { let c = ss.project.find_clip(&cid).unwrap(); let e = c.effects.iter().find(|e| e.kind == "vfx_graph").unwrap(); e.id.clone() };
        // clean up so the next look's assertion only sees its own vfx_graph effect
        dispatch(&mut ss, "add_vfx_graph", &json!({"clip_id": cid, "effect_id": eid, "remove": true})).unwrap();
    }
    // a V8.4 preset on the same clip still works unchanged
    let r = dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": "dreamy", "intensity": 0.5})).unwrap();
    assert_eq!(r["sub_effects"].as_array().unwrap().len(), 4);
}


/// 8.5.4: the montage review's #1 finding. On a short real cutaway (well under glitch_surge's two
/// stacked glitch_blocks layers settling fully), fineblocks and rgb_split used a nonzero "low" baseline
/// between punches (1.5 + 1.5*i and 0.4 + 0.4*i) -- deliberate extra texture on a long demo clip, but
/// it reads as a sustained wash with no clean recovery on a short one. bigblocks already rested at a
/// true 0; fineblocks and rgb_split must now match it so a clip never sits at a visible nonzero amount.
#[test]
fn glitch_surge_amount_keys_truly_rest_at_zero_between_punches() {
    for i in [0.0, 0.4, 1.0] {
        let g = vfx_look_library::graph_for("glitch_surge", i).unwrap();
        let nodes = g["nodes"].as_array().unwrap();
        for id in ["fineblocks", "split"] {
            let node = nodes.iter().find(|n| n["id"] == json!(id)).unwrap_or_else(|| panic!("no {id} node in glitch_surge: {g}"));
            let amount = &node["params"]["amount"];
            assert_eq!(amount["base"].as_f64().unwrap(), 0.0, "{id}@{i} should rest at a true 0 between punches: {amount}");
            let keys = amount["keys"].as_array().unwrap_or_else(|| panic!("{id}@{i} has no pulse keys: {amount}"));
            assert_eq!(keys[0]["v"].as_f64().unwrap(), 0.0, "{id}@{i} low key[0] should be 0, not a nonzero hum: {amount}");
            assert_eq!(keys[3]["v"].as_f64().unwrap(), 0.0, "{id}@{i} low key[3] (end of the period) should be 0: {amount}");
        }
    }
}

/// 8.5.4: the montage review's #1 finding, part two. apply_vfx_preset's generic top-level mix envelope
/// fades in/out over 15% of the clip-local window the look runs across -- on a short real cutaway
/// (here 1.17s, the montage's actual city b-roll cut-in) that is ~0.18s, long enough to still be
/// mid-ramp through glitch_surge's own first punch (attack 0.045s + hold 0.1s = done by 0.145s), muting
/// it before it ever reads as a hit. glitch_surge's own ~0.6s pulse period now caps how long that
/// fade-in may take, so the first punch lands at full strength instead of half-faded.
#[test]
fn glitch_surge_mix_ramp_reaches_full_strength_before_the_first_punch_decays_on_a_short_clip() {
    let mut ss = Session::new();
    dispatch(&mut ss, "project_set", &json!({"width": 32, "height": 32, "fps": 30.0})).unwrap();
    let tr = dispatch(&mut ss, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let cid = dispatch(&mut ss, "add_shape", &json!({"track_id": tr, "shape": "rect", "width": 32, "height": 32, "color": [0.4, 0.3, 0.5], "start": 0, "duration": 1.17})).unwrap()["clip_id"].as_str().unwrap().to_string();
    dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": "glitch_surge", "intensity": 0.6})).unwrap();
    let c = ss.project.find_clip(&cid).unwrap();
    let e = c.effects.iter().find(|e| e.kind == "vfx_graph").unwrap();
    let mix = e.params.get("mix").unwrap_or_else(|| panic!("no mix param: {e:?}"));
    // ramp_prop lays down exactly 4 keys: start(0), start+fade(peak), end-fade(peak), end(0)
    assert_eq!(mix.keys.len(), 4, "{:?}", mix.keys);
    let fade = mix.keys[1].t - mix.keys[0].t;
    assert!(fade <= 0.15, "mix fade-in on a short glitch_surge clip must be capped near the pulse's own attack+hold window (~0.145s), got {fade}s -- the default 15%-of-window (0.1755s here) still ramps through the first punch");
}

#[test]
fn apply_vfx_preset_rejects_an_unknown_preset_with_the_full_list() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let err = dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": "nope"})).unwrap_err();
    assert!(err.contains("dreamy") && err.contains("ember_drift"), "{err}");
}


/// V8.5.5: the montage review's #2 finding, part two. chrome_bloom's own color_wheels push (see
/// vfx_look_library.rs) was pulled back, but on a short real cutaway the generic top-level mix fade (15%
/// of the clip-local window -- here 1.83s, the montage's own chrome beat clip, giving ~0.27s) still landed
/// close to the "about 0.18s" snap the review flagged. chrome_bloom's own min_fade floor lengthens this to
/// about 0.4-0.5s on a clip this short, without shortening an already-longer fade on a longer clip.
#[test]
fn chrome_bloom_mix_fade_in_is_lengthened_on_a_short_clip_but_not_shortened_on_a_long_one() {
    let mut ss = Session::new();
    dispatch(&mut ss, "project_set", &json!({"width": 32, "height": 32, "fps": 30.0})).unwrap();
    let tr = dispatch(&mut ss, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let cid = dispatch(&mut ss, "add_shape", &json!({"track_id": tr, "shape": "rect", "width": 32, "height": 32, "color": [0.4, 0.3, 0.5], "start": 0, "duration": 1.83})).unwrap()["clip_id"].as_str().unwrap().to_string();
    dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": "chrome_bloom", "intensity": 0.45})).unwrap();
    let c = ss.project.find_clip(&cid).unwrap();
    let e = c.effects.iter().find(|e| e.kind == "vfx_graph").unwrap();
    let mix = e.params.get("mix").unwrap_or_else(|| panic!("no mix param: {e:?}"));
    assert_eq!(mix.keys.len(), 4, "{:?}", mix.keys);
    let fade = mix.keys[1].t - mix.keys[0].t;
    let generic_default = 1.83 * 0.15;
    assert!(fade >= 0.4 && fade <= 0.5, "chrome_bloom's mix fade-in should be lengthened to about 0.4-0.5s on a clip this short (generic default would be {generic_default:.3}s), got {fade}s");

    // a long clip's already-slower default fade must not be shortened by the floor.
    let mut ss2 = Session::new();
    dispatch(&mut ss2, "project_set", &json!({"width": 32, "height": 32, "fps": 30.0})).unwrap();
    let tr2 = dispatch(&mut ss2, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let cid2 = dispatch(&mut ss2, "add_shape", &json!({"track_id": tr2, "shape": "rect", "width": 32, "height": 32, "color": [0.4, 0.3, 0.5], "start": 0, "duration": 10.0})).unwrap()["clip_id"].as_str().unwrap().to_string();
    dispatch(&mut ss2, "apply_vfx_preset", &json!({"clip_id": cid2, "preset": "chrome_bloom", "intensity": 0.45})).unwrap();
    let c2 = ss2.project.find_clip(&cid2).unwrap();
    let e2 = c2.effects.iter().find(|e| e.kind == "vfx_graph").unwrap();
    let mix2 = e2.params.get("mix").unwrap();
    let fade2 = mix2.keys[1].t - mix2.keys[0].t;
    assert!((fade2 - 1.5).abs() < 0.01, "a 10s clip's default 15%-of-window fade (1.5s) should not be shortened by chrome_bloom's 0.45s floor, got {fade2}s");
}
