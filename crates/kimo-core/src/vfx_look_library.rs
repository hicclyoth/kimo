//! KIMO 8.5 VFX look library: 9 named looks built entirely on vfx_graph (node chains of existing effects,
//! generators and the particles node), each a self-contained graph JSON parameterized by one 0..1 `intensity`.
//! Exposed through apply_vfx_preset (surreal_vfx_tools.rs) alongside V8.4's dreamy/glitch/vertigo/echo, which
//! stay exactly as they were -- these are additional preset names in the same tool, same intensity dial.
use serde_json::{json, Value};

fn eff(id: &str, src: &str, kind: &str, params: Value) -> Value {
    let mut p = params; if let Value::Object(m) = &mut p { m.insert("effect_kind".into(), json!(kind)); }
    json!({"id": id, "type": "effect", "inputs": {"source": src}, "params": p})
}
fn blend(id: &str, a: &str, b: &str, mode: &str, opacity: f64) -> Value {
    json!({"id": id, "type": "blend", "inputs": {"a": a, "b": b}, "params": {"mode": mode, "opacity": opacity}})
}
fn particles(id: &str, preset: &str, overrides: Value) -> Value {
    let mut p = overrides; if let Value::Object(m) = &mut p { m.insert("preset".into(), json!(preset)); }
    json!({"id": id, "type": "particles", "params": p})
}
const SRC: &str = "__frame__";

pub const NAMES: [&str; 9] = ["ember_drift", "chrome_bloom", "glitch_surge", "dream_haze", "frost_veil", "magic_sparkle", "vhs_decay", "aurora_wash", "solar_flare"];

pub fn describe(name: &str) -> Option<&'static str> {
    Some(match name {
        "ember_drift" => "warm embers drifting up off the frame, screen-blended over a gentle teal/orange bloom grade",
        "chrome_bloom" => "sleek chrome look: RGB fringe, a touch of sharpen, soft highlight bloom",
        "glitch_surge" => "a digital surge: block glitches, RGB split and scanline flicker",
        "dream_haze" => "soft dreamy haze: blur, bloom, a warm light-rays source and a gentle vignette",
        "frost_veil" => "cool frosted-glass read: blue edge-glow, soft blur, cool grade, vignette",
        "magic_sparkle" => "golden/violet magic dust screen-blended over a soft bloom base",
        "vhs_decay" => "worn VHS tape: grain, scanlines, chroma fringe, desaturated, vignette",
        "aurora_wash" => "a teal-to-violet aurora colour wash overlaid on the frame with drifting fireflies",
        "solar_flare" => "a blown-out solar flare: strong light rays, bloom, warm grade and a lift in exposure",
        _ => return None,
    })
}

/// Builds the full vfx_graph JSON for a named look, scaled by intensity (0..1, matching apply_vfx_preset's existing dial).
pub fn graph_for(name: &str, i: f64) -> Result<Value, String> {
    let i = i.clamp(0.0, 1.0);
    let g = match name {
        "ember_drift" => json!({"nodes": [
            eff("grade", SRC, "teal_orange", json!({"amount": 0.2 + 0.3 * i})),
            eff("glow", "grade", "bloom", json!({"amount": 0.25 + 0.45 * i, "threshold": 0.6})),
            particles("sparks", "sparks", json!({"seed": 11, "rate": 70.0 + 140.0 * i, "size0": 2.5, "size1": 0.8})),
            blend("out", "glow", "sparks", "screen", 1.0),
        ], "output": "out"}),
        "chrome_bloom" => json!({"nodes": [
            eff("split", SRC, "chromatic_aberration", json!({"amount": 1.5 + 3.5 * i})),
            eff("sharp", "split", "sharpen", json!({"amount": 0.2 + 0.3 * i})),
            eff("out", "sharp", "bloom", json!({"amount": 0.2 + 0.4 * i, "threshold": 0.65})),
        ], "output": "out"}),
        "glitch_surge" => json!({"nodes": [
            eff("blocks", SRC, "glitch_blocks", json!({"amount": 8.0 + 45.0 * i, "slices": 14.0, "rate": 12.0})),
            eff("split", "blocks", "rgb_split", json!({"amount": 2.0 + 6.0 * i})),
            eff("out", "split", "scanlines", json!({"amount": 0.12 + 0.22 * i, "spacing": 3.0})),
        ], "output": "out"}),
        "dream_haze" => json!({"nodes": [
            eff("soft", SRC, "blur", json!({"radius": 1.0 + 2.5 * i})),
            eff("glow", "soft", "bloom", json!({"amount": 0.25 + 0.4 * i, "threshold": 0.6})),
            eff("rays", "glow", "light_rays", json!({"amount": 0.2 + 0.4 * i, "threshold": 0.65, "cx": 0.5, "cy": 0.15})),
            eff("warm", "rays", "temperature", json!({"amount": 0.15 + 0.2 * i})),
            eff("out", "warm", "vignette", json!({"amount": 0.15 + 0.25 * i})),
        ], "output": "out"}),
        "frost_veil" => json!({"nodes": [
            eff("edge", SRC, "edge_glow", json!({"amount": 0.4 + 0.5 * i, "r": 0.6, "g": 0.85, "b": 1.0})),
            eff("soft", "edge", "blur", json!({"radius": 0.8 + 1.4 * i})),
            eff("cool", "soft", "temperature", json!({"amount": -(0.12 + 0.18 * i)})),
            eff("out", "cool", "vignette", json!({"amount": 0.2 + 0.2 * i})),
        ], "output": "out"}),
        "magic_sparkle" => json!({"nodes": [
            eff("glow", SRC, "bloom", json!({"amount": 0.2 + 0.3 * i, "threshold": 0.62})),
            particles("dust", "magic_dust", json!({"seed": 23, "rate": 60.0 + 120.0 * i})),
            blend("out", "glow", "dust", "screen", 1.0),
        ], "output": "out"}),
        "vhs_decay" => json!({"nodes": [
            eff("grain", SRC, "grain", json!({"amount": 0.08 + 0.18 * i})),
            eff("scan", "grain", "scanlines", json!({"amount": 0.15 + 0.25 * i, "spacing": 3.0})),
            eff("split", "scan", "chromatic_aberration", json!({"amount": 1.0 + 3.0 * i})),
            eff("desat", "split", "saturation", json!({"amount": (1.0 - 0.35 * i).max(0.1)})),
            eff("out", "desat", "vignette", json!({"amount": 0.2 + 0.2 * i})),
        ], "output": "out"}),
        "aurora_wash" => json!({"nodes": [
            eff("glow", SRC, "bloom", json!({"amount": 0.15 + 0.2 * i, "threshold": 0.7})),
            {"id": "wash", "type": "gradient", "params": {"angle_deg": 35.0, "stops": [[0.0, [0.1, 0.85, 0.85]], [1.0, [0.65, 0.2, 0.9]]]}},
            blend("washed", "glow", "wash", "overlay", 0.2 + 0.35 * i),
            particles("fireflies", "fireflies", json!({"seed": 21, "rate": 5.0 + 10.0 * i})),
            blend("out", "washed", "fireflies", "screen", 1.0),
        ], "output": "out"}),
        "solar_flare" => json!({"nodes": [
            eff("rays", SRC, "light_rays", json!({"amount": 0.4 + 0.6 * i, "threshold": 0.55, "cx": 0.7, "cy": 0.2})),
            eff("glow", "rays", "bloom", json!({"amount": 0.3 + 0.5 * i, "threshold": 0.55})),
            eff("grade", "glow", "teal_orange", json!({"amount": 0.25 + 0.3 * i})),
            eff("out", "grade", "exposure", json!({"stops": 0.1 + 0.25 * i})),
        ], "output": "out"}),
        other => return Err(format!("unknown vfx look '{other}'; looks: {}", NAMES.join(", "))),
    };
    Ok(g)
}
