//! KIMO 8.5 particles graph node: wraps crate::particles' existing deterministic emitter. Determinism (seeded,
//! same params -> byte-identical output) and mask-driven ("layer" shape, emit_edge) emission from another node.
use kimo_core::effects::Frame;
use kimo_core::vfx_graph::run_graph;
use serde_json::json;

fn blank(w: usize, h: usize) -> Frame { let mut f = Frame::new(w, h); for i in 0..w * h { f.d[i * 4 + 3] = 1.0; } f }

#[test]
fn particles_node_is_deterministic_same_seed_same_params() {
    let src = blank(48, 48);
    let g = json!({"nodes":[{"id":"p","type":"particles","params":{"preset":"sparks","seed":7}}],"output":"p"});
    let a = run_graph(&g.to_string(), &src, None, 0.7, 1).unwrap();
    let b = run_graph(&g.to_string(), &src, None, 0.7, 1).unwrap();
    assert_eq!(a.d, b.d, "same seed + params + t must render byte-identical particle frames");
}

#[test]
fn particles_node_different_seed_changes_the_frame() {
    let src = blank(48, 48);
    let g1 = json!({"nodes":[{"id":"p","type":"particles","params":{"preset":"sparks","seed":7}}],"output":"p"});
    let g2 = json!({"nodes":[{"id":"p","type":"particles","params":{"preset":"sparks","seed":99}}],"output":"p"});
    let a = run_graph(&g1.to_string(), &src, None, 0.7, 1).unwrap();
    let b = run_graph(&g2.to_string(), &src, None, 0.7, 1).unwrap();
    assert_ne!(a.d, b.d, "a different seed should move the particles");
}

#[test]
fn particles_node_renders_something_visible_from_a_preset() {
    let src = blank(64, 64);
    let g = json!({"nodes":[{"id":"p","type":"particles","params":{"preset":"sparks","seed":5}}],"output":"p"});
    let out = run_graph(&g.to_string(), &src, None, 0.3, 1).unwrap();
    let any_alpha = out.d.chunks(4).any(|px| px[3] > 0.01);
    assert!(any_alpha, "a sparks preset at t=0.3 should have put down at least one visible particle");
}

#[test]
fn particles_node_rejects_an_unknown_preset_name() {
    let src = blank(16, 16);
    let g = json!({"nodes":[{"id":"p","type":"particles","params":{"preset":"not_a_real_preset"}}],"output":"p"});
    let err = match run_graph(&g.to_string(), &src, None, 0.0, 1) { Err(e) => e, Ok(_) => panic!("expected an error") };
    assert!(err.contains("unknown preset") || err.contains("particles"), "{err}");
}

#[test]
fn particles_node_can_emit_from_a_mask_input_shaped_layer() {
    // a bright circle 'mask' feeds shape=layer emission: spawn points come from the mask's alpha, not the default center.
    let src = blank(64, 64);
    let g = json!({"nodes":[
        {"id":"m","type":"shape","params":{"shape":"circle","cx":0.5,"cy":0.5,"radius":0.3,"feather":0.02,"color":[1,1,1]}},
        {"id":"p","type":"particles","inputs":{"mask":"m"},"params":{"preset":"dust","shape":"layer","rate":400,"seed":11}}
    ],"output":"p"});
    let out = run_graph(&g.to_string(), &src, None, 2.0, 1).unwrap();
    let any_alpha = out.d.chunks(4).any(|px| px[3] > 0.01);
    assert!(any_alpha, "layer-shape particles over a real mask should render something");
}
