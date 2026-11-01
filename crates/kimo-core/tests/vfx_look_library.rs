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

#[test]
fn apply_vfx_preset_rejects_an_unknown_preset_with_the_full_list() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let err = dispatch(&mut ss, "apply_vfx_preset", &json!({"clip_id": cid, "preset": "nope"})).unwrap_err();
    assert!(err.contains("dreamy") && err.contains("ember_drift"), "{err}");
}
