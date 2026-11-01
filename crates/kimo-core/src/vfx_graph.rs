//! KIMO 8.5: the VFX graph engine. `vfx_graph` is one effect kind whose `strings["graph"]` holds a small JSON node
//! DAG; interpreting it produces the frame this effect contributes, blended into the clip by the normal `mix` param
//! exactly like droste_zoom/portal_mirror/melt_drip already do. The `effect` node type is the main reuse lever: it
//! dispatches straight into `crate::effects::apply`, so the entire existing effect catalog (twirl, bulge, kaleidoscope,
//! turbulent_displace, droste_zoom, portal_mirror, melt_drip, bloom, glow, halation, chromatic_aberration, hsl,
//! color_wheels, grade, displace, convolution, pixel_expr, coherent_noise, field_operator, posterize, grain, wave,
//! ripple, zoom_blur, light_rays, edge_glow, ...) is already a graph node, no reimplementation. Genuinely new code
//! here is bounded to: the graph plumbing itself (parse/validate/interpret, strict errors), three generators that
//! make a buffer from nothing (solid/gradient/shape, since every existing effect transforms a buffer rather than
//! creates one), and composite (blend modes, mask_apply) to combine named buffers -- nothing here duplicates an
//! existing effect's pixel math.
//!
//! `mask_clip`: the one node that reads another clip's render at the SAME instant (the externally driven aux clip,
//! wired through `layerfx::source_key`/`driven`/`apply` exactly like `subject_clone` already does -- see
//! layerfx.rs). Confirmed by reading every render_clip_alone call site in render.rs: there is no time-OFFSET fetch
//! anywhere in the engine today, so this and every other "read another clip" hook (including this one) only ever
//! sees that clip at the current frame's own time. The graph's "time" input is therefore `t` itself (and whatever a
//! wired Prop's own `expr` ops -- sin/wiggle/noise -- do with it), not true frame-offset sampling; vfx_graph_describe
//! states this plainly rather than pretending otherwise.
use crate::effects::{Frame, Params};
use crate::keyframe::Prop;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SOURCE_FRAME: &str = "__frame__";
pub const MASK_CLIP: &str = "__mask_clip__";

#[derive(Clone)]
pub struct GNode {
    pub id: String,
    pub kind: String,
    pub inputs: BTreeMap<String, String>,
    pub params: Value,
}
#[derive(Clone)]
pub struct Graph {
    pub nodes: Vec<GNode>,
    pub output: String,
}

/// name, one-line purpose, required inputs (for vfx_graph_describe and validation)
pub const NODE_CATALOG: &[(&str, &str, &[&str])] = &[
    ("effect", "Wraps any existing Kimo effect kind by name: params.effect_kind plus that effect's own params/strings. The whole catalog is available this way (twirl, bulge, kaleidoscope, turbulent_displace, droste_zoom, portal_mirror, melt_drip, bloom, glow, halation, chromatic_aberration, hsl, color_wheels, grade, displace, convolution, pixel_expr, coherent_noise, field_operator, posterize, grain, wave, ripple, zoom_blur, light_rays, edge_glow, saber, light_sweep, tilt_shift, channel_blur, ...). Use list_effects/effect_describe for the full list and each one's own params.", &["source"]),
    ("solid", "Flat colour generator (no input). params.color [r,g,b,a] 0..1, each entry a number or a keyframe Prop.", &[]),
    ("gradient", "Linear gradient generator (no input). params.angle_deg, params.stops [[pos 0..1,[r,g,b]], ...] (2..6 stops, each value number or Prop).", &[]),
    ("shape", "Soft circle/rect generator (no input): RGB=params.color, alpha=coverage. params.shape 'circle'|'rect', cx, cy, radius (circle, 0..1 of short side) or w/h (rect, 0..1), feather (0..1), color [r,g,b].", &[]),
    ("blend", "Composites two node outputs. inputs a, b. params.mode 'normal'|'add'|'screen'|'multiply'|'overlay'|'difference', params.opacity 0..1.", &["a", "b"]),
    ("mask_apply", "Multiplies source's alpha by mask's luma (falls back to mask's own alpha if its RGB is flat zero). inputs source, mask. params.invert 0/1.", &["source", "mask"]),
    ("mask_clip", "The externally driven mask/depth clip set via strings.mask_clip_id on the owning add_vfx_graph call, rendered at the SAME instant as this frame (no time-offset -- see the module note). No inputs; read-only source.", &[]),
];

fn known_kind(k: &str) -> bool { NODE_CATALOG.iter().any(|(n, _, _)| *n == k) }
fn required_inputs(k: &str) -> &'static [&'static str] { NODE_CATALOG.iter().find(|(n, _, _)| *n == k).map(|(_, _, r)| *r).unwrap_or(&[]) }

pub fn parse_graph(v: &Value) -> Result<Graph, String> {
    let nodes_v = v.get("nodes").and_then(|n| n.as_array()).ok_or("graph needs a 'nodes' array")?;
    if nodes_v.is_empty() { return Err("graph 'nodes' is empty".into()); }
    let mut nodes = Vec::with_capacity(nodes_v.len());
    let mut seen = HashSet::new();
    for (i, nv) in nodes_v.iter().enumerate() {
        let id = nv.get("id").and_then(|x| x.as_str()).ok_or_else(|| format!("nodes[{i}]: needs a string 'id'"))?.to_string();
        if !seen.insert(id.clone()) { return Err(format!("duplicate node id '{id}'")); }
        let kind = nv.get("type").and_then(|x| x.as_str()).ok_or_else(|| format!("node '{id}': needs a string 'type'"))?.to_string();
        let mut inputs = BTreeMap::new();
        if let Some(obj) = nv.get("inputs").and_then(|x| x.as_object()) {
            for (k, val) in obj {
                let s = val.as_str().ok_or_else(|| format!("node '{id}': inputs.{k} must be a node id string"))?;
                inputs.insert(k.clone(), s.to_string());
            }
        }
        let params = nv.get("params").cloned().unwrap_or_else(|| Value::Object(Default::default()));
        nodes.push(GNode { id, kind, inputs, params });
    }
    let output = v.get("output").and_then(|x| x.as_str()).ok_or("graph needs a string 'output' (a node id)")?.to_string();
    Ok(Graph { nodes, output })
}

/// Strict validation with small-model-fixable errors. Returns a topological node order on success.
pub fn validate(g: &Graph) -> Result<Vec<String>, String> {
    let by_id: HashMap<&str, &GNode> = g.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let all_kinds: Vec<&str> = NODE_CATALOG.iter().map(|(k, _, _)| *k).collect();
    for n in &g.nodes {
        if !known_kind(&n.kind) {
            return Err(format!("node '{}': unknown type '{}', one of [{}]", n.id, n.kind, all_kinds.join(", ")));
        }
        if n.kind == "effect" && n.params.get("effect_kind").and_then(|x| x.as_str()).is_none() {
            return Err(format!("node '{}' (type effect): needs params.effect_kind (the existing effect name to wrap)", n.id));
        }
        for r in required_inputs(&n.kind) {
            let Some(target) = n.inputs.get(*r) else {
                return Err(format!("node '{}' (type {}): needs input '{}' (wire it from another node's id, or \"{}\" for the untouched frame)", n.id, n.kind, r, SOURCE_FRAME));
            };
            if target != SOURCE_FRAME && target != MASK_CLIP && !by_id.contains_key(target.as_str()) {
                return Err(format!("node '{}': input '{}' points at unknown node '{}'", n.id, r, target));
            }
        }
        // non-required inputs still must point somewhere real if given at all
        for (k, target) in &n.inputs {
            if required_inputs(&n.kind).contains(&k.as_str()) { continue; }
            if target != SOURCE_FRAME && target != MASK_CLIP && !by_id.contains_key(target.as_str()) {
                return Err(format!("node '{}': input '{}' points at unknown node '{}'", n.id, k, target));
            }
        }
    }
    if !by_id.contains_key(g.output.as_str()) {
        return Err(format!("output '{}' is not a node id in this graph", g.output));
    }
    // Kahn's topological sort over real node->node edges (SOURCE_FRAME/MASK_CLIP are not graph edges)
    let mut indeg: HashMap<&str, usize> = g.nodes.iter().map(|n| (n.id.as_str(), 0usize)).collect();
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &g.nodes {
        for dep in n.inputs.values() {
            if dep != SOURCE_FRAME && dep != MASK_CLIP {
                *indeg.get_mut(n.id.as_str()).unwrap() += 1;
                edges.entry(dep.as_str()).or_default().push(n.id.as_str());
            }
        }
    }
    let mut queue: Vec<&str> = indeg.iter().filter(|(_, &d)| d == 0).map(|(k, _)| *k).collect();
    queue.sort_unstable();
    let mut order = Vec::new();
    let mut idx = 0;
    while idx < queue.len() {
        let cur = queue[idx];
        idx += 1;
        order.push(cur.to_string());
        if let Some(next) = edges.get(cur) {
            let mut next = next.clone();
            next.sort_unstable();
            for nx in next {
                let d = indeg.get_mut(nx).unwrap();
                *d -= 1;
                if *d == 0 { queue.push(nx); }
            }
        }
    }
    if order.len() != g.nodes.len() {
        let mut stuck: Vec<&str> = indeg.iter().filter(|(_, &d)| d > 0).map(|(k, _)| *k).collect();
        stuck.sort_unstable();
        return Err(format!("cycle detected among nodes: {}", stuck.join(", ")));
    }
    Ok(order)
}

// ---------- scalar/number reading: a node param is a literal number OR a full keyframe Prop (base/keys/expr JSON) ----------
pub fn value_to_prop(v: &Value) -> Result<Prop, String> {
    if let Some(n) = v.as_f64() {
        if !n.is_finite() { return Err("numeric param must be finite".into()); }
        return Ok(Prop::new(n));
    }
    if v.is_object() {
        return serde_json::from_value::<Prop>(v.clone()).map_err(|e| format!("param is not a number or a keyframe Prop object: {e}"));
    }
    Err("param must be a number or a keyframe Prop object ({\"base\":...,\"keys\":[...],\"expr\":[...]})".into())
}
fn num(params_obj: &Value, key: &str, default: f64, t: f64) -> Result<f64, String> {
    match params_obj.get(key) {
        None => Ok(default),
        Some(v) if v.is_null() => Ok(default),
        Some(v) => Ok(value_to_prop(v)?.at(t)),
    }
}
fn arr3(params_obj: &Value, key: &str, default: [f64; 3], t: f64) -> Result<[f32; 3], String> {
    match params_obj.get(key).and_then(|v| v.as_array()) {
        None => Ok([default[0] as f32, default[1] as f32, default[2] as f32]),
        Some(a) => {
            let mut out = [default[0] as f32, default[1] as f32, default[2] as f32];
            for (i, slot) in out.iter_mut().enumerate() {
                if let Some(v) = a.get(i) { *slot = value_to_prop(v)?.at(t) as f32; }
            }
            Ok(out)
        }
    }
}
fn str_param<'a>(params_obj: &'a Value, key: &str, default: &'a str) -> &'a str {
    params_obj.get(key).and_then(|v| v.as_str()).unwrap_or(default)
}

// ---------- tiny shared pixel helpers (kept local to this module; effects2.rs's own are private to it) ----------
/// Bilinear sample used by the particle renderer's soft sprite stamp (particle_tools.rs).
pub(crate) fn bilinear(src: &[f32], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    if w == 0 || h == 0 { return [0.0; 4]; }
    let (x, y) = (x.clamp(0.0, w as f32 - 1.001), y.clamp(0.0, h as f32 - 1.001));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: usize, yy: usize| -> [f32; 4] { let i = (yy * w + xx) * 4; [src[i], src[i + 1], src[i + 2], src[i + 3]] };
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut o = [0f32; 4];
    for j in 0..4 { o[j] = a[j] * (1.0 - fx) * (1.0 - fy) + b[j] * fx * (1.0 - fy) + c[j] * (1.0 - fx) * fy + d[j] * fx * fy; }
    o
}
pub(crate) fn luma(c: &[f32]) -> f32 { 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] }
fn clone_frame(f: &Frame) -> Frame { Frame { w: f.w, h: f.h, d: f.d.clone() } }
fn blend_frame(mix: f64, orig: &Frame, out: &mut Frame) {
    let k = mix.clamp(0.0, 1.0) as f32;
    if k >= 0.999 { return; }
    for i in 0..orig.d.len() { out.d[i] = orig.d[i] + (out.d[i] - orig.d[i]) * k; }
}

// ---------- generators ----------
fn gen_solid(w: usize, h: usize, params: &Value, t: f64) -> Result<Frame, String> {
    let c = arr3(params, "color", [1.0, 1.0, 1.0], t)?;
    let alpha = match params.get("color").and_then(|v| v.as_array()).and_then(|a| a.get(3)) {
        Some(v) => value_to_prop(v)?.at(t) as f32,
        None => 1.0,
    };
    let mut f = Frame::new(w, h);
    for i in 0..w * h { f.d[i * 4] = c[0]; f.d[i * 4 + 1] = c[1]; f.d[i * 4 + 2] = c[2]; f.d[i * 4 + 3] = alpha; }
    Ok(f)
}
fn gen_gradient(w: usize, h: usize, params: &Value, t: f64) -> Result<Frame, String> {
    let angle = num(params, "angle_deg", 0.0, t)?.to_radians() as f32;
    let stops_v = params.get("stops").and_then(|v| v.as_array()).cloned().unwrap_or_else(|| vec![serde_json::json!([0.0, [0.0, 0.0, 0.0]]), serde_json::json!([1.0, [1.0, 1.0, 1.0]])]);
    if stops_v.len() < 2 { return Err("gradient needs params.stops with at least 2 entries".into()); }
    if stops_v.len() > 6 { return Err("gradient takes at most 6 stops".into()); }
    let mut stops: Vec<(f32, [f32; 3])> = Vec::with_capacity(stops_v.len());
    for s in &stops_v {
        let arr = s.as_array().ok_or("each gradient stop must be [pos, [r,g,b]]")?;
        let pos = arr.get(0).and_then(|v| v.as_f64()).ok_or("gradient stop needs a numeric position")? as f32;
        let col = arr.get(1).and_then(|v| v.as_array()).ok_or("gradient stop needs a [r,g,b] colour")?;
        let mut rgb = [0f32; 3];
        for (i, slot) in rgb.iter_mut().enumerate() { *slot = col.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32; }
        stops.push((pos, rgb));
    }
    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let (ux, uy) = (angle.cos(), angle.sin());
    let (wf, hf) = (w as f32, h as f32);
    let mut f = Frame::new(w, h);
    for y in 0..h { for x in 0..w {
        let u = ((x as f32 / wf.max(1.0) - 0.5) * ux + (y as f32 / hf.max(1.0) - 0.5) * uy) + 0.5;
        let u = u.clamp(0.0, 1.0);
        let mut col = stops[0].1;
        for w2 in stops.windows(2) {
            if u >= w2[0].0 && u <= w2[1].0 {
                let span = (w2[1].0 - w2[0].0).max(1e-6);
                let k = ((u - w2[0].0) / span).clamp(0.0, 1.0);
                for j in 0..3 { col[j] = w2[0].1[j] + (w2[1].1[j] - w2[0].1[j]) * k; }
            }
        }
        if u <= stops[0].0 { col = stops[0].1; }
        if u >= stops[stops.len() - 1].0 { col = stops[stops.len() - 1].1; }
        let i = (y * w + x) * 4;
        f.d[i] = col[0]; f.d[i + 1] = col[1]; f.d[i + 2] = col[2]; f.d[i + 3] = 1.0;
    } }
    Ok(f)
}
fn gen_shape(w: usize, h: usize, params: &Value, t: f64) -> Result<Frame, String> {
    let shape = str_param(params, "shape", "circle").to_string();
    let (cx, cy) = (num(params, "cx", 0.5, t)? as f32 * w as f32, num(params, "cy", 0.5, t)? as f32 * h as f32);
    let md = w.min(h) as f32;
    let feather = (num(params, "feather", 0.05, t)? as f32 * md).max(0.01);
    let col = arr3(params, "color", [1.0, 1.0, 1.0], t)?;
    let mut f = Frame::new(w, h);
    match shape.as_str() {
        "circle" => {
            let r = num(params, "radius", 0.25, t)? as f32 * md;
            for y in 0..h { for x in 0..w {
                let d = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() - r) / feather;
                let a = (0.5 - d * 0.5).clamp(0.0, 1.0);
                let i = (y * w + x) * 4;
                f.d[i] = col[0]; f.d[i + 1] = col[1]; f.d[i + 2] = col[2]; f.d[i + 3] = a;
            } }
        }
        "rect" => {
            let (hw, hh) = (num(params, "w", 0.5, t)? as f32 * w as f32 * 0.5, num(params, "h", 0.5, t)? as f32 * h as f32 * 0.5);
            for y in 0..h { for x in 0..w {
                let dx = (x as f32 - cx).abs() - hw; let dy = (y as f32 - cy).abs() - hh;
                let d = dx.max(dy) / feather;
                let a = (0.5 - d * 0.5).clamp(0.0, 1.0);
                let i = (y * w + x) * 4;
                f.d[i] = col[0]; f.d[i + 1] = col[1]; f.d[i + 2] = col[2]; f.d[i + 3] = a;
            } }
        }
        other => return Err(format!("shape: 'shape' must be 'circle' or 'rect', got '{other}'")),
    }
    Ok(f)
}

// ---------- composite ----------
fn blend(a: &Frame, b: &Frame, params: &Value, t: f64) -> Result<Frame, String> {
    if a.w != b.w || a.h != b.h { return Err(format!("blend: size mismatch {}x{} vs {}x{}", a.w, a.h, b.w, b.h)); }
    let mode = str_param(params, "mode", "normal").to_string();
    let opacity = num(params, "opacity", 1.0, t)?.clamp(0.0, 1.0) as f32;
    let f = |x: f32, y: f32| -> f32 { match mode.as_str() {
        "add" => (x + y).min(1.0),
        "screen" => 1.0 - (1.0 - x) * (1.0 - y),
        "multiply" => x * y,
        "overlay" => if x < 0.5 { 2.0 * x * y } else { 1.0 - 2.0 * (1.0 - x) * (1.0 - y) },
        "difference" => (x - y).abs(),
        _ => y,
    } };
    let mut out = clone_frame(a);
    for i in 0..a.w * a.h {
        let bo = b.d[i * 4 + 3];
        for j in 0..3 {
            let composed = f(out.d[i * 4 + j], b.d[i * 4 + j]);
            out.d[i * 4 + j] = out.d[i * 4 + j] + (composed - out.d[i * 4 + j]) * opacity * bo;
        }
        out.d[i * 4 + 3] = (out.d[i * 4 + 3] + bo * opacity).min(1.0);
    }
    Ok(out)
}
fn mask_apply(source: &Frame, mask: &Frame, params: &Value, _t: f64) -> Result<Frame, String> {
    if source.w != mask.w || source.h != mask.h { return Err(format!("mask_apply: size mismatch {}x{} vs {}x{}", source.w, source.h, mask.w, mask.h)); }
    let invert = params.get("invert").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.5;
    let mut out = clone_frame(source);
    for i in 0..source.w * source.h {
        let m3 = [mask.d[i * 4], mask.d[i * 4 + 1], mask.d[i * 4 + 2]];
        let m = if m3.iter().all(|v| *v == 0.0) { mask.d[i * 4 + 3] } else { luma(&m3) * mask.d[i * 4 + 3] };
        let m = if invert { 1.0 - m } else { m };
        out.d[i * 4 + 3] *= m.clamp(0.0, 1.0);
    }
    Ok(out)
}

// ---------- node params -> (Params, strings) for the "effect" node ----------
fn effect_args(params_obj: &Value) -> Result<(Params, BTreeMap<String, String>), String> {
    let mut props: Params = BTreeMap::new();
    let mut strings: BTreeMap<String, String> = BTreeMap::new();
    if let Some(obj) = params_obj.as_object() {
        for (k, v) in obj {
            if k == "effect_kind" { continue; }
            match v {
                Value::String(s) => { strings.insert(k.clone(), s.clone()); }
                Value::Number(_) => { props.insert(k.clone(), value_to_prop(v)?); }
                Value::Object(_) => { props.insert(k.clone(), value_to_prop(v)?); }
                Value::Bool(b) => { props.insert(k.clone(), Prop::new(if *b { 1.0 } else { 0.0 })); }
                _ => return Err(format!("effect node param '{k}' must be a string, number, bool, or keyframe Prop object")),
            }
        }
    }
    Ok((props, strings))
}

/// Interpret an already-validated graph in the given topological order.
pub fn interpret(g: &Graph, order: &[String], source: &Frame, mask_clip: Option<&Frame>, t: f64, seed: u64) -> Result<Frame, String> {
    let by_id: HashMap<&str, &GNode> = g.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut bufs: HashMap<String, Frame> = HashMap::new();
    for id in order {
        let n = *by_id.get(id.as_str()).ok_or_else(|| format!("internal: node '{id}' missing"))?;
        let get_input = |bufs: &HashMap<String, Frame>, key: &str| -> Result<Frame, String> {
            let target = n.inputs.get(key).ok_or_else(|| format!("node '{}': missing input '{}'", n.id, key))?;
            if target == SOURCE_FRAME { Ok(clone_frame(source)) }
            else if target == MASK_CLIP {
                mask_clip.map(clone_frame).ok_or_else(|| format!("node '{}': input '{}' is \"{}\" but no strings.mask_clip_id is set on this add_vfx_graph call", n.id, key, MASK_CLIP))
            } else {
                bufs.get(target).map(clone_frame).ok_or_else(|| format!("node '{}': input '{}' -> '{}' not computed yet", n.id, key, target))
            }
        };
        let out = match n.kind.as_str() {
            "effect" => {
                let mut fr = get_input(&bufs, "source")?;
                let kind = n.params.get("effect_kind").and_then(|x| x.as_str()).unwrap_or("");
                let (props, strings) = effect_args(&n.params)?;
                crate::effects::apply(kind, &props, &strings, &mut fr, t, seed).map_err(|e| format!("node '{}' (effect {kind}): {e}", n.id))?;
                fr
            }
            "solid" => gen_solid(source.w, source.h, &n.params, t)?,
            "gradient" => gen_gradient(source.w, source.h, &n.params, t)?,
            "shape" => gen_shape(source.w, source.h, &n.params, t)?,
            "blend" => { let a = get_input(&bufs, "a")?; let b = get_input(&bufs, "b")?; blend(&a, &b, &n.params, t)? }
            "mask_apply" => { let s = get_input(&bufs, "source")?; let m = get_input(&bufs, "mask")?; mask_apply(&s, &m, &n.params, t)? }
            "mask_clip" => mask_clip.map(clone_frame).ok_or_else(|| format!("node '{}' (mask_clip): no strings.mask_clip_id set on this add_vfx_graph call", n.id))?,
            other => return Err(format!("node '{}': unknown type '{}'", n.id, other)),
        };
        bufs.insert(n.id.clone(), out);
    }
    bufs.remove(&g.output).ok_or_else(|| format!("output node '{}' did not run", g.output))
}

/// Parse + validate + interpret, in one call (the normal, non-driven entry point).
pub fn run_graph(graph_json: &str, source: &Frame, mask_clip: Option<&Frame>, t: f64, seed: u64) -> Result<Frame, String> {
    let v: Value = serde_json::from_str(graph_json).map_err(|e| format!("graph is not valid JSON: {e}"))?;
    let g = parse_graph(&v)?;
    let order = validate(&g)?;
    interpret(&g, &order, source, mask_clip, t, seed)
}

/// The `vfx_graph` effect kind's normal (non-driven, no mask_clip) entry point -- called from effects::apply.
pub fn apply_effect(params: &Params, strings: &BTreeMap<String, String>, f: &mut Frame, t: f64, seed: u64) -> Result<(), String> {
    let graph_json = strings.get("graph").ok_or("vfx_graph: strings.graph (the node graph JSON) is required")?;
    let mix = params.get("mix").map(|p| p.at(t)).unwrap_or(1.0);
    if mix <= 0.0 { return Ok(()); }
    let orig = clone_frame(f);
    let mut out = run_graph(graph_json, &orig, None, t, seed)?;
    blend_frame(mix, &orig, &mut out);
    f.d = out.d;
    Ok(())
}

/// The `vfx_graph` effect kind's driven entry point (strings.mask_clip_id set) -- called from layerfx::apply.
pub fn apply_driven(e: &crate::model::Effect, f: &mut Frame, mask_src: &Frame, t: f64, seed: u64) {
    let Some(graph_json) = e.strings.get("graph") else { return };
    let mix = e.params.get("mix").map(|p| p.at(t)).unwrap_or(1.0);
    if mix <= 0.0 { return; }
    let orig = clone_frame(f);
    match run_graph(graph_json, &orig, Some(mask_src), t, seed) {
        Ok(mut out) => { blend_frame(mix, &orig, &mut out); f.d = out.d; }
        Err(_) => { /* leave the frame untouched; add_vfx_graph's own validation (done at add time) is where this is surfaced */ }
    }
}

/// Compact node catalog text for vfx_graph_describe (kept out of director_profile's own tool-description budget --
/// this is a separate tool's payload, not a tool description).
pub fn describe_catalog() -> String {
    let mut s = String::new();
    for (name, desc, req) in NODE_CATALOG {
        s.push_str(&format!("{name} (inputs: {}): {desc}\n", if req.is_empty() { "none".to_string() } else { req.join(", ") }));
    }
    s.push_str("\nEvery numeric node param accepts a literal number OR a full keyframe Prop object ({\"base\":n,\"keys\":[{\"t\":s,\"v\":n,\"ease\":\"...\"}],\"expr\":[...]})\n");
    s.push_str("-- the same keyframe system every other Kimo effect param uses, including its expr ops (sin/wiggle/noise_impulse/spring/...) for non-keyframe-driven motion.\n");
    s.push_str("Time: only the current instant t (plus whatever a Prop's own expr ops compute from it) is available. There is no time-OFFSET clip read anywhere in the engine (confirmed in render.rs): a node cannot sample another frame of the SAME clip at a different time, only another clip's render at the SAME instant (mask_clip). Audio level is not currently exposed as a node param input.\n");
    s
}
