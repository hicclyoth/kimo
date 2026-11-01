//! KIMO 8.5 vfx_graph MCP tools: add_vfx_graph commits the effect (validated eagerly, clear error on a bad graph),
//! vfx_graph_describe returns the node catalog, vfx_preview renders without committing.
use kimo_core::commands::{dispatch, Session};
use serde_json::json;

fn make_clip(ss: &mut Session) -> String {
    dispatch(ss, "project_set", &json!({"width": 16, "height": 16, "fps": 10.0})).unwrap();
    let tr = dispatch(ss, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    dispatch(ss, "add_shape", &json!({"track_id": tr, "shape": "rect", "width": 16, "height": 16, "color": [0.2, 0.3, 0.4], "start": 0, "duration": 2})).unwrap()["clip_id"].as_str().unwrap().to_string()
}

#[test]
fn add_vfx_graph_rejects_a_bad_graph_with_a_clear_error() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let bad = json!({"nodes":[{"id":"a","type":"nope"}],"output":"a"});
    let err = dispatch(&mut ss, "add_vfx_graph", &json!({"clip_id": cid, "graph": bad})).unwrap_err();
    assert!(err.contains("unknown type"), "{err}");
}

#[test]
fn add_vfx_graph_commits_a_good_graph_and_is_keyframable() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let good = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[1,0,0,1]}}],"output":"a"});
    let r = dispatch(&mut ss, "add_vfx_graph", &json!({"clip_id": cid, "graph": good, "mix": 1.0})).unwrap();
    assert!(r["effect_id"].as_str().is_some());
    let eid = r["effect_id"].as_str().unwrap();
    let c = ss.project.find_clip(&cid).unwrap();
    let e = c.effects.iter().find(|e| e.id == eid).unwrap();
    assert_eq!(e.kind, "vfx_graph");
    assert!(e.params.get("mix").is_some(), "mix must be a real keyframable Prop");
}

#[test]
fn add_vfx_graph_remove_takes_it_off() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let good = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[1,0,0,1]}}],"output":"a"});
    dispatch(&mut ss, "add_vfx_graph", &json!({"clip_id": cid, "graph": good})).unwrap();
    let r = dispatch(&mut ss, "add_vfx_graph", &json!({"clip_id": cid, "remove": true})).unwrap();
    assert_eq!(r["removed"].as_u64(), Some(1));
    assert!(ss.project.find_clip(&cid).unwrap().effects.iter().all(|e| e.kind != "vfx_graph"));
}

#[test]
fn vfx_graph_describe_lists_the_node_catalog() {
    let mut ss = Session::new();
    let r = dispatch(&mut ss, "vfx_graph_describe", &json!({})).unwrap();
    let cat = r["catalog"].as_str().unwrap();
    for name in ["effect", "solid", "gradient", "shape", "blend", "mask_apply", "mask_clip"] {
        assert!(cat.contains(name), "catalog missing '{name}': {cat}");
    }
}

#[test]
fn vfx_preview_renders_requested_times_with_metrics_and_does_not_commit() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let tmp = std::env::temp_dir().join(format!("vfx_preview_test_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let graph = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[0.9,0.1,0.1,1.0]}}],"output":"a"});
    let r = dispatch(&mut ss, "vfx_preview", &json!({"clip_id": cid, "graph": graph, "times": [0.0, 0.1], "out_dir": tmp.to_string_lossy()})).unwrap();
    let frames = r["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    for fr in frames {
        let p = fr["path"].as_str().unwrap();
        assert!(std::path::Path::new(p).exists(), "{p} should exist");
        assert!(fr["mean_luma"].as_f64().unwrap() > 0.0);
        assert!(fr["clipping_pct"].as_f64().is_some());
        assert!(fr["edge_energy"].as_f64().is_some());
        assert!(fr["diff_from_source"].as_f64().unwrap() > 0.0, "a red solid over a dark-blue shape should differ from source");
    }
    // not committed as a clip effect
    assert!(ss.project.find_clip(&cid).unwrap().effects.iter().all(|e| e.kind != "vfx_graph"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn vfx_preview_rejects_more_than_six_times() {
    let mut ss = Session::new();
    let cid = make_clip(&mut ss);
    let graph = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[1,1,1,1]}}],"output":"a"});
    let times: Vec<f64> = (0..7).map(|i| i as f64 * 0.1).collect();
    let err = dispatch(&mut ss, "vfx_preview", &json!({"clip_id": cid, "graph": graph, "times": times})).unwrap_err();
    assert!(err.to_lowercase().contains("1-6") || err.to_lowercase().contains("6"), "{err}");
}
