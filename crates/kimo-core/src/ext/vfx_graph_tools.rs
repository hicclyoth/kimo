//! KIMO 8.5 MCP tools: add_vfx_graph (commit a node graph as a clip effect), vfx_graph_describe (compact node
//! catalog for a small model), vfx_preview (render a graph at a few times without committing it, with cheap metrics
//! so a model can look and tweak before a full render -- WHAT TO BUILD item 3).
use super::kit::*;
use crate::effects::Frame;
use crate::keyframe::{Ease, Key, Prop};
use crate::model::*;
use crate::vfx_graph;

fn ramp_prop(start: f64, end: f64, peak: f64, fade: f64) -> Prop {
    let fade = fade.max(0.001).min(((end - start) / 2.0).max(0.001));
    let mut pr = Prop::new(0.0);
    pr.keys.push(Key { t: start, v: 0.0, ease: Ease::InOut, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: start + fade, v: peak, ease: Ease::Linear, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: (end - fade).max(start + fade), v: peak, ease: Ease::InOut, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: end, v: 0.0, ease: Ease::Linear, bez: None, out: None, inn: None });
    pr
}
fn num(a: &Value, k: &str, d: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match a.get(k) { None => Ok(d), Some(v) if v.is_null() => Ok(d),
        Some(v) => { let x = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{k} must be a number ({lo}..{hi})"))?; if !(lo..=hi).contains(&x) { Err(format!("{k} must be between {lo} and {hi}, got {x}")) } else { Ok(x) } } }
}

/// Clone the project down to one clip and render it at `t` -- the same restriction analysis::render applies for
/// source "clip:<id>", kept local so vfx_preview gets the Frame directly (RGBA f32) instead of analysis::render's
/// lossy Image round-trip (it drops alpha and downcasts to f64 for histogram work it doesn't need here).
fn render_clip_at(ss: &mut Session, clip_id: &str, t: f64) -> Result<Frame, String> {
    let mut p = ss.project.clone();
    let c = p.find_clip(clip_id).ok_or("no such clip")?.clone();
    if t < c.start || t >= c.start + c.duration { return Err(format!("time {t} is outside clip {clip_id}'s own span [{}, {})", c.start, c.start + c.duration)); }
    p.tracks.retain(|tr| tr.clips.iter().any(|x| x.id == clip_id));
    for tr in &mut p.tracks { tr.clips.retain(|x| x.id == clip_id); }
    let cache = &mut ss.cache;
    crate::render::prepare_at(&p, cache, Some(t))?;
    let mut f = crate::render::render_frame(&p, t, cache)?;
    crate::color::pipeline::output(&p, &mut f)?;
    Ok(f)
}

fn mean_luma(f: &Frame) -> f64 { let n = f.w * f.h; if n == 0 { return 0.0; } (0..n).map(|i| vfx_graph::luma(&f.d[i * 4..i * 4 + 3]) as f64).sum::<f64>() / n as f64 }
fn clipping_pct(f: &Frame) -> f64 { let n = f.w * f.h; if n == 0 { return 0.0; } let bad = (0..n).filter(|&i| (0..3).any(|j| f.d[i * 4 + j] >= 0.995 || f.d[i * 4 + j] <= 0.005)).count(); bad as f64 / n as f64 }
fn edge_energy(f: &Frame) -> f64 {
    let (w, h) = (f.w, f.h); if w < 3 || h < 3 { return 0.0; }
    let l = |x: usize, y: usize| vfx_graph::luma(&f.d[(y * w + x) * 4..(y * w + x) * 4 + 3]) as f64;
    let mut sum = 0.0; let mut n = 0usize;
    for y in 1..h - 1 { for x in 1..w - 1 {
        let gx = -l(x - 1, y - 1) - 2.0 * l(x - 1, y) - l(x - 1, y + 1) + l(x + 1, y - 1) + 2.0 * l(x + 1, y) + l(x + 1, y + 1);
        let gy = -l(x - 1, y - 1) - 2.0 * l(x, y - 1) - l(x + 1, y - 1) + l(x - 1, y + 1) + 2.0 * l(x, y + 1) + l(x + 1, y + 1);
        sum += (gx * gx + gy * gy).sqrt(); n += 1;
    } }
    if n == 0 { 0.0 } else { sum / n as f64 }
}
fn diff_from_source(a: &Frame, b: &Frame) -> f64 {
    if a.w != b.w || a.h != b.h { return 1.0; }
    let n = a.w * a.h; if n == 0 { return 0.0; }
    (0..n).map(|i| (vfx_graph::luma(&a.d[i * 4..i * 4 + 3]) - vfx_graph::luma(&b.d[i * 4..i * 4 + 3])).abs() as f64).sum::<f64>() / n as f64
}

pub fn commands() -> Vec<Command> { vec![
    Command { name: "add_vfx_graph", description: "KIMO 8.5 VFX graph: apply a node-graph effect (see vfx_graph_describe) to a clip, ramped in/out over a time window.", mutates: true,
        schema: obj(vec![
            ("clip_id", s("clip to apply it to")),
            ("graph", json!({"type":"object","description":"the node graph: {\"nodes\":[{\"id\":..,\"type\":..,\"inputs\":{..},\"params\":{..}}],\"output\":\"<node id>\"} -- see vfx_graph_describe for node types"})),
            ("mix", n("0..1, overall strength at full ramp, default 1.0")),
            ("start_time", n("clip-local seconds the effect ramps in from, default 0")),
            ("end_time", n("clip-local seconds the effect ramps out by, default the clip's own duration")),
            ("mask_clip_id", s("optional: another clip (e.g. a track_matte output) rendered at the SAME instant and exposed to the graph as the \"__mask_clip__\" input -- see vfx_graph_describe's time note")),
            ("effect_id", s("the effect_id returned by an earlier add_vfx_graph call, to update that same graph instead of adding a new one")),
            ("remove", json!({"type":"boolean","description":"true removes the clip's vfx_graph (or the one named by effect_id)"}))], &["clip_id"]),
        run: |ss, a| {
            let cid = str_arg(a, "clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            let eid_in = a.get("effect_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            if a.get("remove").and_then(|v| v.as_bool()).unwrap_or(false) {
                let c = ss.project.find_clip_mut(&cid).ok_or_else(|| format!("no such clip {cid}"))?;
                let n0 = c.effects.len();
                match &eid_in { Some(id) => c.effects.retain(|e| !(e.kind == "vfx_graph" && &e.id == id)), None => c.effects.retain(|e| e.kind != "vfx_graph") }
                return Ok(json!({"removed": n0 - c.effects.len()}));
            }
            let graph_v = a.get("graph").cloned().ok_or("add_vfx_graph needs 'graph' (the node graph object)")?;
            let g = vfx_graph::parse_graph(&graph_v).map_err(|e| format!("add_vfx_graph: {e}"))?;
            vfx_graph::validate(&g).map_err(|e| format!("add_vfx_graph: {e}"))?;
            let mix = num(a, "mix", 1.0, 0.0, 1.0)?;
            let start = num(a, "start_time", 0.0, 0.0, dur)?;
            let end = num(a, "end_time", dur, start, dur)?;
            let fade = ((end - start) * 0.15).max(0.05);
            let mut params: std::collections::BTreeMap<String, Prop> = std::collections::BTreeMap::new();
            params.insert("mix".into(), ramp_prop(start, end, mix, fade));
            let mut strings: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
            strings.insert("graph".into(), graph_v.to_string());
            if let Some(mid) = a.get("mask_clip_id").and_then(|v| v.as_str()) {
                if ss.project.find_clip(mid).is_none() { return Err(format!("add_vfx_graph: mask_clip_id '{mid}' is not a clip")); }
                strings.insert("mask_clip_id".into(), mid.to_string());
            }
            let eid = match &eid_in {
                Some(id) => { if ss.project.find_clip(&cid).unwrap().effects.iter().any(|e| &e.id == id && e.kind == "vfx_graph") { id.clone() } else { return Err(format!("add_vfx_graph: no existing vfx_graph effect_id '{id}' on clip {cid}")); } }
                None => ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "vfx_graph").map(|e| e.id.clone()).unwrap_or_else(|| ss.project.gen_id("fx")),
            };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = strings; true }
                else { c.effects.push(Effect { id: eid.clone(), kind: "vfx_graph".into(), params, strings }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "nodes": g.nodes.len(), "mix": mix}))
        } },
    Command { name: "vfx_graph_describe", description: "KIMO 8.5 VFX graph: the compact node catalog (types, inputs, params) a small model needs to build a vfx_graph.", mutates: false,
        schema: obj(vec![], &[]),
        run: |_, _| Ok(json!({"catalog": vfx_graph::describe_catalog()})) },
    Command { name: "vfx_preview", description: "KIMO 8.5 VFX graph: render 1-6 frames of a graph to PNG without committing it, with cheap metrics, so a model can tweak before a full render.", mutates: false,
        schema: obj(vec![
            ("clip_id", s("clip to preview the graph against")),
            ("graph", json!({"type":"object","description":"the node graph, same shape as add_vfx_graph's"})),
            ("times", json!({"type":"array","items":{"type":"number"},"minItems":1,"maxItems":6,"description":"1-6 clip-local seconds to render"})),
            ("mask_clip_id", s("optional: another clip rendered at the same instant, exposed as \"__mask_clip__\"")),
            ("out_dir", s("directory to write the PNGs into; default the project's own directory, or the current directory"))], &["clip_id", "graph", "times"]),
        run: |ss, a| {
            let cid = str_arg(a, "clip_id")?.to_string();
            let graph_v = a.get("graph").cloned().ok_or("vfx_preview needs 'graph'")?;
            let g = vfx_graph::parse_graph(&graph_v).map_err(|e| format!("vfx_preview: {e}"))?;
            vfx_graph::validate(&g).map_err(|e| format!("vfx_preview: {e}"))?;
            let times: Vec<f64> = a.get("times").and_then(|v| v.as_array()).ok_or("vfx_preview needs 'times' (1-6 numbers)")?
                .iter().map(|v| v.as_f64().ok_or_else(|| "times entries must be numbers".to_string())).collect::<Result<_, _>>()?;
            if times.is_empty() || times.len() > 6 { return Err("vfx_preview: 'times' must have 1-6 entries".into()); }
            let out_dir = match a.get("out_dir") { None | Some(Value::Null) => ss.project.base_dir.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| ".".to_string()), Some(Value::String(s)) if !s.is_empty() => s.clone(), _ => return Err("out_dir must be a non-empty string".into()) };
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create out_dir {out_dir:?}: {e}"))?;
            let mask_id = a.get("mask_clip_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let mut frames = Vec::new();
            for (i, &t) in times.iter().enumerate() {
                let src = render_clip_at(ss, &cid, t)?;
                let mask = match &mask_id { Some(id) => Some(render_clip_at(ss, id, t)?), None => None };
                let out = vfx_graph::interpret(&g, &vfx_graph::validate(&g).unwrap(), &src, mask.as_ref(), t, 1).map_err(|e| format!("vfx_preview at t={t}: {e}"))?;
                let path = format!("{}/vfx_preview_{i:02}_t{t:.3}s.png", out_dir.trim_end_matches('/'));
                crate::render::write_image(&out, &path).map_err(|e| format!("vfx_preview: PNG write failed: {e}"))?;
                frames.push(json!({
                    "time": t, "path": path,
                    "mean_luma": mean_luma(&out), "clipping_pct": clipping_pct(&out),
                    "edge_energy": edge_energy(&out), "diff_from_source": diff_from_source(&out, &src),
                }));
            }
            Ok(json!({"frames": frames}))
        } },
] }
