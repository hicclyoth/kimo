//! Surreal VFX tools (KIMO 8.4): dedicated, keyframable wrappers around new effect kinds (droste_zoom, portal_mirror in
//! effects2.rs) plus a pure-keyframe convenience for the Hitchcock dolly-zoom / vertigo move built entirely on the
//! existing depth_parallax camera move (no new pixel math -- see depth_parallax_tools.rs). Each add_* tool here takes
//! intensity (0..1, subtle default) and a start/end clip-local time window, and keyframes the underlying effect's
//! `mix` (or `dolly`) param to ramp in across that window -- so set_effect_param/set_keyframe/set_effect_mask/remove_effect
//! all still work on the resulting effect afterward, same as any other clip effect.
use super::kit::*;
use crate::keyframe::{Ease, Key, Prop};
use crate::model::*;

fn ramp_prop(start: f64, end: f64, peak: f64, fade: f64) -> Prop {
    // 0 before start, ramps up to `peak` over `fade` seconds, holds, ramps back to 0 over the last `fade` seconds before end.
    let fade = fade.max(0.001).min(((end - start) / 2.0).max(0.001));
    let mut pr = Prop::new(0.0);
    pr.keys.push(Key { t: start, v: 0.0, ease: Ease::InOut, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: start + fade, v: peak, ease: Ease::Linear, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: (end - fade).max(start + fade), v: peak, ease: Ease::InOut, bez: None, out: None, inn: None });
    pr.keys.push(Key { t: end, v: 0.0, ease: Ease::Linear, bez: None, out: None, inn: None });
    pr
}

const DROSTE_ARGS: [&str; 9] = ["clip_id", "intensity", "start_time", "end_time", "cx", "cy", "twist", "drift", "remove"];
const PORTAL_ARGS: [&str; 10] = ["clip_id", "intensity", "start_time", "end_time", "cx", "cy", "radius", "flip_vertical", "invert_color", "remove"];
const VERTIGO_ARGS: [&str; 7] = ["clip_id", "depth_clip_id", "intensity", "start_time", "end_time", "direction", "focal_depth"];

fn num(a: &Value, k: &str, d: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match a.get(k) { None => Ok(d), Some(v) if v.is_null() => Ok(d),
        Some(v) => { let x = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{k} must be a number ({lo}..{hi})"))?; if !(lo..=hi).contains(&x) { Err(format!("{k} must be between {lo} and {hi}, got {x}")) } else { Ok(x) } } }
}

pub fn commands() -> Vec<Command> { vec![
    Command { name: "add_droste_zoom", description: "Droste/Escher infinite recursive zoom: the frame spirals into a self-similar copy of itself. intensity (0..1, subtle default) sets how dense and twisted the spiral is; the effect ramps in and out across start_time..end_time (clip-local seconds).", mutates: true,
        schema: obj(vec![("clip_id", s("clip to apply it to")),
            ("intensity", n("0..1, how strong the recursive spiral reads, default 0.35 (subtle)")),
            ("start_time", n("clip-local seconds the effect ramps in from, default 0")),
            ("end_time", n("clip-local seconds the effect ramps out by, default the clip's own duration")),
            ("cx", n("spiral center x, 0..1, default 0.5")),
            ("cy", n("spiral center y, 0..1, default 0.5")),
            ("twist", n("degrees of rotation added per recursion ring, default 10 (0 = plain tunnel, no spiral)")),
            ("drift", n("how fast the spiral pulls inward over time, rings/s, default 0.1")),
            ("remove", json!({"type":"boolean","description":"true removes the clip's droste_zoom"}))], &["clip_id"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !DROSTE_ARGS.contains(&k.as_str()) { return Err(format!("add_droste_zoom: unknown argument '{k}' (valid: {})", DROSTE_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("add_droste_zoom needs clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            if a.get("remove").and_then(|v| v.as_bool()).unwrap_or(false) {
                let c = ss.project.find_clip_mut(&cid).unwrap(); let n0 = c.effects.len(); c.effects.retain(|e| e.kind != "droste_zoom");
                return Ok(json!({"removed": n0 - c.effects.len()})); }
            let intensity = num(&a, "intensity", 0.35, 0.0, 1.0)?;
            let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
            let cx = num(&a, "cx", 0.5, 0.0, 1.0)?; let cy = num(&a, "cy", 0.5, 0.0, 1.0)?;
            let twist = num(&a, "twist", 10.0, -90.0, 90.0)?; let drift = num(&a, "drift", 0.1, 0.0, 2.0)?;
            // zoom ratio scales with intensity: 1.15 (gentle, many rings) .. 2.2 (fewer, chunkier rings) at full intensity
            let zoom = 1.15 + intensity * 1.05; let fade = ((end - start) * 0.2).max(0.1);
            let mut params: std::collections::BTreeMap<String, Prop> = [("cx", cx), ("cy", cy), ("zoom", zoom), ("twist", twist), ("drift", drift)].iter().map(|(k, v)| (k.to_string(), Prop::new(*v))).collect();
            params.insert("mix".into(), ramp_prop(start, end, intensity.max(0.05), fade));
            let eid = { let existing = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "droste_zoom").map(|e| e.id.clone()); existing.unwrap_or_else(|| ss.project.gen_id("fx")) };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = Default::default(); true } else { c.effects.push(Effect { id: eid.clone(), kind: "droste_zoom".into(), params, strings: Default::default() }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "intensity": intensity, "window": [start, end], "zoom": zoom, "twist": twist})) } },

    Command { name: "add_portal_mirror", description: "A soft-edged circular portal showing the same frame flipped and colour-inverted inside it -- a doorway into an upside-down world. Pair with set_effect_mask (or layer over a subject matte) for a portal shaped like something other than a circle. Ramps in/out across start_time..end_time.", mutates: true,
        schema: obj(vec![("clip_id", s("clip to apply it to")),
            ("intensity", n("0..1, portal opacity/strength, default 0.4 (subtle)")),
            ("start_time", n("clip-local seconds the portal opens from, default 0")),
            ("end_time", n("clip-local seconds the portal closes by, default the clip's own duration")),
            ("cx", n("portal center x, 0..1, default 0.5")),
            ("cy", n("portal center y, 0..1, default 0.4")),
            ("radius", n("portal radius, fraction of the short side, 0.05..0.6, default 0.22")),
            ("flip_vertical", json!({"type":"boolean","description":"flip the world vertically inside the portal, default true"})),
            ("invert_color", json!({"type":"boolean","description":"invert colour inside the portal, default true"})),
            ("remove", json!({"type":"boolean","description":"true removes the clip's portal_mirror"}))], &["clip_id"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !PORTAL_ARGS.contains(&k.as_str()) { return Err(format!("add_portal_mirror: unknown argument '{k}' (valid: {})", PORTAL_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("add_portal_mirror needs clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            if a.get("remove").and_then(|v| v.as_bool()).unwrap_or(false) {
                let c = ss.project.find_clip_mut(&cid).unwrap(); let n0 = c.effects.len(); c.effects.retain(|e| e.kind != "portal_mirror");
                return Ok(json!({"removed": n0 - c.effects.len()})); }
            let intensity = num(&a, "intensity", 0.4, 0.0, 1.0)?;
            let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
            let cx = num(&a, "cx", 0.5, 0.0, 1.0)?; let cy = num(&a, "cy", 0.4, 0.0, 1.0)?; let radius = num(&a, "radius", 0.22, 0.05, 0.6)?;
            let flip_v = match a.get("flip_vertical") { None | Some(Value::Null) => true, Some(v) => v.as_bool().ok_or("flip_vertical must be true or false")? };
            let invert_c = match a.get("invert_color") { None | Some(Value::Null) => true, Some(v) => v.as_bool().ok_or("invert_color must be true or false")? };
            let fade = ((end - start) * 0.2).max(0.1);
            let mut params: std::collections::BTreeMap<String, Prop> = [("cx", cx), ("cy", cy), ("radius", radius), ("feather", radius * 0.35), ("flip_vertical", if flip_v { 1.0 } else { 0.0 }), ("invert_color", if invert_c { 1.0 } else { 0.0 })].iter().map(|(k, v)| (k.to_string(), Prop::new(*v))).collect();
            params.insert("mix".into(), ramp_prop(start, end, intensity.max(0.05), fade));
            let eid = { let existing = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "portal_mirror").map(|e| e.id.clone()); existing.unwrap_or_else(|| ss.project.gen_id("fx")) };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = Default::default(); true } else { c.effects.push(Effect { id: eid.clone(), kind: "portal_mirror".into(), params, strings: Default::default() }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "intensity": intensity, "window": [start, end], "radius": radius})) } },

    Command { name: "add_vertigo_zoom", description: "Hitchcock dolly-zoom / vertigo move: the subject stays locked in place and size while the background stretches or compresses around it. Pure keyframes on the existing depth_parallax camera move (dolly ramps, zoom stays 1, so anything at focal_depth never moves) -- needs a depth map clip same as add_depth_parallax.", mutates: true,
        schema: obj(vec![("clip_id", s("clip to move the camera on")),
            ("depth_clip_id", s("depth map: another clip read per pixel, white = near, black = far (the map clip is hidden)")),
            ("intensity", n("0..1, how strong the background stretch reads, default 0.4 (subtle)")),
            ("start_time", n("clip-local seconds the move starts, default 0")),
            ("end_time", n("clip-local seconds the move ends, default the clip's own duration")),
            ("direction", json!({"type":"string","enum":["in","out"],"description":"'in' pushes toward the subject (background grows), 'out' pulls away (background shrinks); default in"})),
            ("focal_depth", n("depth value that stays locked (the subject's depth in the map), 0..1, default 0.5"))], &["clip_id", "depth_clip_id"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !VERTIGO_ARGS.contains(&k.as_str()) { return Err(format!("add_vertigo_zoom: unknown argument '{k}' (valid: {})", VERTIGO_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("add_vertigo_zoom needs clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            let m = a.get("depth_clip_id").and_then(|v| v.as_str()).ok_or("add_vertigo_zoom needs depth_clip_id (the clip read as the depth map)")?.to_string();
            if m == cid { return Err(format!("depth_clip_id: a clip cannot be its own depth map ({m} is the moved clip itself)")); }
            if ss.project.find_clip(&m).is_none() { return Err(format!("depth_clip_id: no such clip {m}")); }
            let intensity = num(&a, "intensity", 0.4, 0.0, 1.0)?;
            let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
            let dir = match a.get("direction") { None | Some(Value::Null) => "in".to_string(), Some(v) => { let s = v.as_str().unwrap_or(""); if s != "in" && s != "out" { return Err(format!("direction must be 'in' or 'out', got {v}")); } s.to_string() } };
            let focal = num(&a, "focal_depth", 0.5, 0.0, 1.0)?;
            // dolly target scaled by intensity, 0..0.72 (stays inside depth_parallax's -0.9..0.9 clamp); sign by direction
            let target = intensity * 0.72 * if dir == "in" { 1.0 } else { -1.0 };
            let mut dolly = Prop::new(0.0);
            dolly.keys.push(Key { t: start, v: 0.0, ease: Ease::InOut, bez: None, out: None, inn: None });
            dolly.keys.push(Key { t: end, v: target, ease: Ease::InOut, bez: None, out: None, inn: None });
            let mut params: std::collections::BTreeMap<String, Prop> = [("pan_x", 0.0), ("pan_y", 0.0), ("zoom", 1.0), ("focal_depth", focal)].iter().map(|(k, v)| (k.to_string(), Prop::new(*v))).collect();
            params.insert("dolly".into(), dolly);
            let strings: std::collections::BTreeMap<String, String> = [("depth_clip".to_string(), m.clone()), ("depth_channel".to_string(), "luma".to_string())].into_iter().collect();
            let eid = { let existing = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "depth_parallax").map(|e| e.id.clone()); existing.unwrap_or_else(|| ss.project.gen_id("fx")) };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = strings; true } else { c.effects.push(Effect { id: eid.clone(), kind: "depth_parallax".into(), params, strings }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "depth": {"clip_id": m, "focal_depth": focal}, "dolly_target": target, "window": [start, end], "note": "same effect kind as add_depth_parallax (depth_parallax) -- this is a keyframed convenience over it, not a separate effect"})) } },
] }
