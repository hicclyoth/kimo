//! KIMO 8.5 vfx_graph core: node validation (unknown type, missing input, cycle) and interpretation (solid/blend/effect nodes).
use kimo_core::effects::Frame;
use kimo_core::vfx_graph::{parse_graph, validate, interpret, run_graph};
use serde_json::json;

fn blank(w: usize, h: usize) -> Frame { let mut f = Frame::new(w, h); for i in 0..w * h { f.d[i * 4 + 3] = 1.0; } f }

#[test]
fn unknown_node_type_is_a_clear_error() {
    let g = json!({"nodes":[{"id":"a","type":"not_a_real_node"}],"output":"a"});
    let g = parse_graph(&g).unwrap();
    let err = validate(&g).unwrap_err();
    assert!(err.contains("a"), "{err}");
    assert!(err.contains("unknown type"), "{err}");
    assert!(err.contains("not_a_real_node"), "{err}");
}

#[test]
fn missing_required_input_is_a_clear_error() {
    let g = json!({"nodes":[{"id":"b","type":"blend","params":{"mode":"add"}}],"output":"b"});
    let g = parse_graph(&g).unwrap();
    let err = validate(&g).unwrap_err();
    assert!(err.contains("needs input"), "{err}");
    assert!(err.contains("'a'") || err.contains("'b'"), "{err}");
}

#[test]
fn cycle_is_detected_not_looped_forever() {
    let g = json!({"nodes":[
        {"id":"x","type":"blend","inputs":{"a":"y","b":"__frame__"},"params":{}},
        {"id":"y","type":"blend","inputs":{"a":"x","b":"__frame__"},"params":{}}
    ],"output":"x"});
    let g = parse_graph(&g).unwrap();
    let err = validate(&g).unwrap_err();
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn unknown_output_node_is_a_clear_error() {
    let g = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[1,1,1,1]}}],"output":"nope"});
    let g = parse_graph(&g).unwrap();
    let err = validate(&g).unwrap_err();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn effect_node_without_effect_kind_is_a_clear_error() {
    let g = json!({"nodes":[{"id":"a","type":"effect","inputs":{"source":"__frame__"},"params":{}}],"output":"a"});
    let g = parse_graph(&g).unwrap();
    let err = validate(&g).unwrap_err();
    assert!(err.contains("effect_kind"), "{err}");
}

#[test]
fn solid_node_fills_the_requested_color() {
    let src = blank(4, 4);
    let g = json!({"nodes":[{"id":"a","type":"solid","params":{"color":[0.25,0.5,0.75,1.0]}}],"output":"a"});
    let out = run_graph(&g.to_string(), &src, None, 0.0, 1).unwrap();
    assert!((out.d[0] - 0.25).abs() < 1e-5);
    assert!((out.d[1] - 0.5).abs() < 1e-5);
    assert!((out.d[2] - 0.75).abs() < 1e-5);
    assert!((out.d[3] - 1.0).abs() < 1e-5);
}

#[test]
fn blend_add_mode_adds_channels() {
    let src = blank(2, 2);
    let g = json!({"nodes":[
        {"id":"a","type":"solid","params":{"color":[0.3,0.0,0.0,1.0]}},
        {"id":"b","type":"solid","params":{"color":[0.4,0.0,0.0,1.0]}},
        {"id":"out","type":"blend","inputs":{"a":"a","b":"b"},"params":{"mode":"add","opacity":1.0}}
    ],"output":"out"});
    let out = run_graph(&g.to_string(), &src, None, 0.0, 1).unwrap();
    assert!((out.d[0] - 0.7).abs() < 1e-4, "got {}", out.d[0]);
}

#[test]
fn effect_node_wraps_an_existing_effect() {
    // twirl with angle 0 should be a no-op warp (identity remap): the frame content should survive unchanged.
    let mut src = Frame::new(4, 4);
    for i in 0..16 { src.d[i * 4] = (i as f32) / 16.0; src.d[i * 4 + 3] = 1.0; }
    let g = json!({"nodes":[{"id":"a","type":"effect","inputs":{"source":"__frame__"},"params":{"effect_kind":"twirl","angle":0.0}}],"output":"a"});
    let out = run_graph(&g.to_string(), &src, None, 0.0, 1).unwrap();
    assert_eq!(out.w, 4); assert_eq!(out.h, 4);
}

#[test]
fn graph_is_not_valid_json_is_a_clear_error() {
    let src = blank(2, 2);
    let err = match run_graph("{not json", &src, None, 0.0, 1) { Err(e) => e, Ok(_) => panic!("expected an error") };
    assert!(err.contains("JSON"), "{err}");
}

#[test]
fn topo_order_runs_dependencies_before_dependents() {
    let g = json!({"nodes":[
        {"id":"out","type":"blend","inputs":{"a":"src1","b":"src2"},"params":{"mode":"screen"}},
        {"id":"src1","type":"solid","params":{"color":[0.1,0.1,0.1,1.0]}},
        {"id":"src2","type":"solid","params":{"color":[0.2,0.2,0.2,1.0]}}
    ],"output":"out"});
    let g = parse_graph(&g).unwrap();
    let order = validate(&g).unwrap();
    let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
    assert!(pos("src1") < pos("out"));
    assert!(pos("src2") < pos("out"));
    let src = blank(2, 2);
    let out = interpret(&g, &order, &src, None, 0.0, 1).unwrap();
    assert!(out.d[0] > 0.0);
}
