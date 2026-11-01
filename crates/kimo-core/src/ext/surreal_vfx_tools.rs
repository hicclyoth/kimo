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
const MELT_ARGS: [&str; 7] = ["clip_id", "intensity", "start_time", "end_time", "size", "speed", "angle"];
const CLONE_ARGS: [&str; 11] = ["clip_id", "mask_clip_id", "intensity", "start_time", "end_time", "count", "dx", "dy", "decay", "mask_channel", "mask_threshold"];
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

    Command { name: "add_subject_clone", description: "Clone/multiply the subject: duplicates whatever a mask clip marks as the subject (white = subject) at growing spatial offsets, each copy a little dimmer -- the subject appears to repeat across the frame. Needs a mask/matte clip the same way add_depth_parallax needs a depth map. Ramps in/out across start_time..end_time.", mutates: true,
        schema: obj(vec![("clip_id", s("clip to clone the subject of")),
            ("mask_clip_id", s("mask/matte clip: another clip read per pixel, white = subject, black = background (hidden, same convention as add_depth_parallax's depth map)")),
            ("intensity", n("0..1, overall clone opacity, default 0.5 (subtle)")),
            ("start_time", n("clip-local seconds the clones ramp in from, default 0")),
            ("end_time", n("clip-local seconds the clones ramp out by, default the clip's own duration")),
            ("count", n("number of extra copies, 1..8, default 3")),
            ("dx", n("x offset per copy in px, default 55")),
            ("dy", n("y offset per copy in px, default 0")),
            ("decay", n("opacity falloff per further copy, 0..1, default 0.6 (each copy 60% of the one before it)")),
            ("mask_channel", json!({"type":"string","enum":crate::layerfx::CHANNELS,"description":"channel of the mask read as subject coverage, default luma"})),
            ("mask_threshold", n("mask value at/above which a pixel counts as subject, 0..1, default 0.5"))], &["clip_id", "mask_clip_id"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !CLONE_ARGS.contains(&k.as_str()) { return Err(format!("add_subject_clone: unknown argument '{k}' (valid: {})", CLONE_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("add_subject_clone needs clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            let m = a.get("mask_clip_id").and_then(|v| v.as_str()).ok_or("add_subject_clone needs mask_clip_id (the clip read as the subject mask: white = subject)")?.to_string();
            if m == cid { return Err(format!("mask_clip_id: a clip cannot be its own subject mask ({m} is the cloned clip itself)")); }
            if ss.project.find_clip(&m).is_none() { return Err(format!("mask_clip_id: no such clip {m}")); }
            let intensity = num(&a, "intensity", 0.5, 0.0, 1.0)?;
            let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
            let count = num(&a, "count", 3.0, 1.0, 8.0)?; let dx = num(&a, "dx", 55.0, -2000.0, 2000.0)?; let dy = num(&a, "dy", 0.0, -2000.0, 2000.0)?;
            let decay = num(&a, "decay", 0.6, 0.0, 1.0)?; let thr = num(&a, "mask_threshold", 0.5, 0.0, 1.0)?;
            let ch = match a.get("mask_channel") { None | Some(Value::Null) => "luma".to_string(), Some(v) => { let c = v.as_str().unwrap_or(""); if !crate::layerfx::CHANNELS.contains(&c) { return Err(format!("mask_channel must be one of {}, got {v}", crate::layerfx::CHANNELS.join(", "))); } c.to_string() } };
            let fade = ((end - start) * 0.2).max(0.1);
            let mut params: std::collections::BTreeMap<String, Prop> = [("count", count), ("dx", dx), ("dy", dy), ("decay", decay), ("mask_threshold", thr)].iter().map(|(k, v)| (k.to_string(), Prop::new(*v))).collect();
            params.insert("amount".into(), ramp_prop(start, end, intensity.max(0.05), fade));
            let strings: std::collections::BTreeMap<String, String> = [("mask_clip".to_string(), m.clone()), ("mask_channel".to_string(), ch.clone())].into_iter().collect();
            let eid = { let existing = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "subject_clone").map(|e| e.id.clone()); existing.unwrap_or_else(|| ss.project.gen_id("fx")) };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = strings; true } else { c.effects.push(Effect { id: eid.clone(), kind: "subject_clone".into(), params, strings }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "mask": {"clip_id": m, "channel": ch, "threshold": thr}, "count": count, "window": [start, end]})) } },

    Command { name: "add_melt_drip", description: "Directional dripping/melting warp: the image streaks and sags toward one edge (default straight down), like wet paint or a melting frame -- distinct from the existing symmetric liquid wobble (turbulent_displace). Ramps in/out across start_time..end_time.", mutates: true,
        schema: obj(vec![("clip_id", s("clip to apply it to")),
            ("intensity", n("0..1, how far it drips at its strongest, default 0.3 (subtle)")),
            ("start_time", n("clip-local seconds the drip ramps in from, default 0")),
            ("end_time", n("clip-local seconds the drip ramps out by, default the clip's own duration")),
            ("size", n("noise feature width px, how wide each drip streak reads, default 40")),
            ("speed", n("drip animation speed, default 0.3")),
            ("angle", n("drip direction in degrees, 90 = straight down, default 90"))], &["clip_id"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !MELT_ARGS.contains(&k.as_str()) { return Err(format!("add_melt_drip: unknown argument '{k}' (valid: {})", MELT_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("add_melt_drip needs clip_id")?.to_string();
            let dur = ss.project.find_clip(&cid).ok_or_else(|| format!("no such clip {cid}"))?.duration;
            let intensity = num(&a, "intensity", 0.3, 0.0, 1.0)?;
            let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
            let size = num(&a, "size", 40.0, 4.0, 400.0)?; let speed = num(&a, "speed", 0.3, 0.0, 5.0)?; let angle = num(&a, "angle", 90.0, -360.0, 360.0)?;
            // amount scales with intensity: up to 60px of drip at full intensity
            let amount = intensity * 60.0; let fade = ((end - start) * 0.2).max(0.1);
            let mut params: std::collections::BTreeMap<String, Prop> = [("amount", amount), ("size", size), ("speed", speed), ("angle", angle)].iter().map(|(k, v)| (k.to_string(), Prop::new(*v))).collect();
            params.insert("mix".into(), ramp_prop(start, end, 1.0, fade));
            let eid = { let existing = ss.project.find_clip(&cid).unwrap().effects.iter().find(|e| e.kind == "melt_drip").map(|e| e.id.clone()); existing.unwrap_or_else(|| ss.project.gen_id("fx")) };
            let c = ss.project.find_clip_mut(&cid).unwrap();
            let replaced = if let Some(e) = c.effects.iter_mut().find(|e| e.id == eid) { e.params = params; e.strings = Default::default(); true } else { c.effects.push(Effect { id: eid.clone(), kind: "melt_drip".into(), params, strings: Default::default() }); false };
            Ok(json!({"effect_id": eid, "updated": replaced, "intensity": intensity, "window": [start, end], "amount_px": amount})) } },

    Command { name: "apply_vfx_preset", description: "One-call surreal look: dreamy (soft glow + halation + a touch of chromatic fringe + vignette), glitch (datamosh pixel-sort + digital tear blocks + chromatic split + VHS noise), vertigo (Hitchcock dolly-zoom, needs depth_clip_id), or echo (ghost-trail frame blending). intensity (0..1) scales every sub-effect together so a small model doesn't have to tune each one by hand.", mutates: true,
        schema: obj(vec![("clip_id", s("clip to apply the preset to")),
            ("preset", json!({"type":"string","enum":PRESETS,"description":"dreamy | glitch | vertigo | echo"})),
            ("intensity", n("0..1, how strong the whole look reads, default 0.4 (subtle)")),
            ("start_time", n("clip-local seconds the look starts (dreamy/glitch/echo apply for the clip's whole visible range regardless; vertigo ramps across this window), default 0")),
            ("end_time", n("clip-local seconds the look ends (vertigo only), default the clip's own duration")),
            ("depth_clip_id", s("required for preset=vertigo: the depth map clip (see add_vertigo_zoom)"))], &["clip_id", "preset"]),
        run: |ss, a| {
            for k in a.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() { if !PRESET_ARGS.contains(&k.as_str()) { return Err(format!("apply_vfx_preset: unknown argument '{k}' (valid: {})", PRESET_ARGS.join(", "))); } }
            let cid = a.get("clip_id").and_then(|v| v.as_str()).ok_or("apply_vfx_preset needs clip_id")?.to_string();
            if ss.project.find_clip(&cid).is_none() { return Err(format!("no such clip {cid}")); }
            let preset = a.get("preset").and_then(|v| v.as_str()).ok_or("apply_vfx_preset needs preset")?.to_string();
            if !PRESETS.contains(&preset.as_str()) { return Err(format!("preset must be one of {}, got {preset}", PRESETS.join(", "))); }
            let intensity = num(&a, "intensity", 0.4, 0.0, 1.0)?;
            let mut applied = vec![];
            match preset.as_str() {
                "dreamy" => {
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "glow", "params": {"amount": 0.15 + 0.35 * intensity, "radius": 14.0, "threshold": 0.62}}))?; applied.push("glow");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "halation", "params": {"threshold": 0.75, "radius": 20.0, "amount": 0.5 * intensity}}))?; applied.push("halation");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "chromatic_aberration", "params": {"amount": 1.0 + 2.0 * intensity}}))?; applied.push("chromatic_aberration");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "vignette", "params": {"amount": 0.15 + 0.3 * intensity}}))?; applied.push("vignette"); }
                "glitch" => {
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "pixel_sort", "params": {"threshold": 0.55 - 0.3 * intensity, "max_run": 150.0 + 300.0 * intensity}}))?; applied.push("pixel_sort");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "glitch_blocks", "params": {"amount": 10.0 + 40.0 * intensity, "slices": 10.0, "rate": 10.0}}))?; applied.push("glitch_blocks");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "chromatic_aberration", "params": {"amount": 2.0 + 6.0 * intensity}}))?; applied.push("chromatic_aberration");
                    crate::commands::dispatch(ss, "add_effect", &json!({"clip_id": cid, "kind": "vhs", "params": {"wobble": 1.0 + 4.0 * intensity, "bleed": 2.0 + 6.0 * intensity, "noise": 0.08 + 0.2 * intensity, "bands": 1.0}}))?; applied.push("vhs"); }
                "vertigo" => {
                    let m = a.get("depth_clip_id").and_then(|v| v.as_str()).ok_or("preset=vertigo needs depth_clip_id (the depth map clip)")?.to_string();
                    let dur = ss.project.find_clip(&cid).unwrap().duration;
                    let start = num(&a, "start_time", 0.0, 0.0, dur)?; let end = num(&a, "end_time", dur, start, dur)?;
                    crate::commands::dispatch(ss, "add_vertigo_zoom", &json!({"clip_id": cid, "depth_clip_id": m, "intensity": intensity, "start_time": start, "end_time": end}))?; applied.push("vertigo_zoom (depth_parallax)"); }
                "echo" => {
                    let count = (3.0 + intensity * 7.0).round(); let spacing = 0.02 + intensity * 0.03; let decay = 0.55 + intensity * 0.3;
                    crate::commands::dispatch(ss, "set_echo", &json!({"clip_id": cid, "count": count, "spacing": spacing, "decay": decay, "mode": "composite"}))?; applied.push("set_echo"); }
                _ => unreachable!(),
            }
            Ok(json!({"clip_id": cid, "preset": preset, "intensity": intensity, "sub_effects": applied})) } },
] }


const PRESET_ARGS: [&str; 6] = ["clip_id", "preset", "intensity", "start_time", "end_time", "depth_clip_id"];
const PRESETS: [&str; 4] = ["dreamy", "glitch", "vertigo", "echo"];


