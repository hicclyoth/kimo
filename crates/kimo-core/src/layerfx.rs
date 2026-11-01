//! Displacement Map and Set Matte (After Effects), effects driven by another clip of the project. The other clip is rendered
//! alone at the same time (as a track-matte source is) and read in this clip's frame, stretched to fit when the sizes differ.
//! Both run in render.rs apply_effects_masked, where the project is at hand; reached without one (no source) they do nothing.
//! Displacement Map reads its map where the clip lands on screen by default (map_space "screen"): each pixel of the clip is
//! carried through the clip's transform (x, y, scale, rotation, motion path, parent, and the move, zoom or turn of its own
//! transition in, such as a push sliding it in) to the canvas, the map is read there,
//! and max px are screen px along the screen axes, so a map placed over a scaled, moved or rotated clip warps what is seen
//! under it. map_space "clip" reads the map in the clip's own frame before its transform (it then scales and turns with it).
//! A clip drawn alone (the outgoing picture of a push, a matte source) brings its map clip along, hidden, read at the same time
//! held to the map's own span (so a map laid over the outgoing shot of a push stays on it through the push), and the push's move
//! of the outgoing picture is part of its place, so its map stays on screen like the incoming clip's.
//! Camera Lens Blur takes a depth (blur) map the same way when add_lens_blur names depth_clip_id (lens_depth below; lens.rs apply_map).
//! add_depth_parallax (a 2.5D "3D photo" camera move) reads its depth map clip the same way (depth_parallax below).
use crate::effects::Frame;
use crate::model::Effect;

pub const CHANNELS: &[&str] = &["red", "green", "blue", "alpha", "luma", "hue", "saturation", "lightness"];
pub const EDGES: &[&str] = &["clamp", "wrap", "transparent"];
pub const KINDS: &[&str] = &["displacement_map", "set_matte", "lens_blur", "depth_parallax", "channel_combiner", "set_channels", "match_grain", "subject_clone"];
pub const MAP_SPACES: &[&str] = &["screen", "clip"];

/// Where the clip lands on the canvas: f = c + (dx, dy) + R(rot) * scale * (q - c), c the canvas centre, y down (render.rs transform_bb).
#[derive(Clone, Copy, Debug)] pub struct Place { pub scale: f64, pub rot_deg: f64, pub dx: f64, pub dy: f64 }
impl Place { pub fn identity(&self) -> bool { (self.scale - 1.0).abs() < 1e-9 && self.rot_deg == 0.0 && self.dx == 0.0 && self.dy == 0.0 } }

/// The string key holding the driving clip's id.
/// lens_blur carries one only when add_lens_blur was given a depth map (depth_clip); without it the lens blur is an ordinary effect.
pub fn source_key(kind: &str) -> Option<&'static str> { match kind { "displacement_map" => Some("map_clip"), "set_matte" => Some("matte_clip"), "lens_blur" | "depth_parallax" => Some("depth_clip"), "channel_combiner" | "set_channels" | "match_grain" => Some("ref_clip"), "subject_clone" => Some("mask_clip"), "vfx_graph" => Some("mask_clip_id"), _ => None } }
/// True when the effect is driven by another clip right now (its source key is set).
pub fn driven(e: &Effect) -> bool { source_key(&e.kind).is_some_and(|k| e.strings.contains_key(k)) }

/// A channel of a premultiplied pixel, 0..1, read as if composited over a flat `bg` grey (0.5 = no displacement, 0 = no matte).
pub fn channel(px: &[f32], ch: &str, bg: f32) -> f32 {
    let a = px[3].clamp(0.0, 1.0); let c = [px[0] + bg * (1.0 - a), px[1] + bg * (1.0 - a), px[2] + bg * (1.0 - a)].map(|v| v.clamp(0.0, 1.0));
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2])); let l = (mx + mn) / 2.0;
    match ch { "red" => c[0], "green" => c[1], "blue" => c[2], "alpha" => a, "luma" => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2], "lightness" => l,
        "saturation" => if mx - mn < 1e-6 { 0.0 } else { (mx - mn) / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6) },
        "hue" => { if mx - mn < 1e-6 { 0.0 } else { let d = mx - mn; let h = if mx == c[0] { ((c[1] - c[2]) / d).rem_euclid(6.0) } else if mx == c[1] { (c[2] - c[0]) / d + 2.0 } else { (c[0] - c[1]) / d + 4.0 }; h / 6.0 } }
        _ => 0.0 }
}
/// Pixel (x, y) of `f` mapped onto `src` (stretch to fit).
fn at<'a>(src: &'a Frame, f: &Frame, x: usize, y: usize) -> &'a [f32] { let (sx, sy) = (x * src.w / f.w.max(1), y * src.h / f.h.max(1)); let i = (sy.min(src.h - 1) * src.w + sx.min(src.w - 1)) * 4; &src.d[i..i + 4] }

/// The map's pixel at canvas position (x, y) (pixel index units), nearest; None outside the map (no shift there).
fn map_px(map: &Frame, x: f64, y: f64) -> Option<[f32; 4]> { let (xi, yi) = (x.round(), y.round()); if xi < 0.0 || yi < 0.0 || xi >= map.w as f64 || yi >= map.h as f64 { return None; } let i = (yi as usize * map.w + xi as usize) * 4; Some([map.d[i], map.d[i + 1], map.d[i + 2], map.d[i + 3]]) }
fn num(e: &Effect, k: &str, lt: f64, d: f64) -> f32 { e.params.get(k).map(|p| p.at(lt)).unwrap_or(d) as f32 }
fn word<'a>(e: &'a Effect, k: &str, d: &'a str) -> &'a str { e.strings.get(k).map(|s| s.as_str()).unwrap_or(d) }

/// Displacement Map: each pixel samples the clip at an offset of (value - 0.5) * 2 * max px, per axis, from the map's channels.
pub fn displace(e: &Effect, f: &mut Frame, map: &Frame, lt: f64, place: Option<Place>) {
    if map.w == 0 || map.h == 0 { return; }
    let (mh, mv) = (num(e, "max_h", lt, 20.0), num(e, "max_v", lt, 20.0)); let (ch, cv, edges) = (word(e, "h_channel", "red"), word(e, "v_channel", "green"), word(e, "edges", "clamp"));
    let (w, h) = (f.w, f.h); let src = f.d.clone();
    let screen = place.filter(|pl| !pl.identity() && pl.scale.abs() > 1e-6 && word(e, "map_space", "screen") == "screen").map(|pl| { let r = pl.rot_deg.to_radians(); (pl, w as f64 / 2.0, h as f64 / 2.0, r.sin(), r.cos()) });
    let fetch = |x: i64, y: i64| -> Option<usize> { let (x, y) = match edges { "wrap" => (x.rem_euclid(w as i64), y.rem_euclid(h as i64)), "transparent" => { if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 { return None; } (x, y) } _ => (x.clamp(0, w as i64 - 1), y.clamp(0, h as i64 - 1)) }; Some((y as usize * w + x as usize) * 4) };
    for y in 0..h { for x in 0..w {
        let (dx, dy) = match screen { None => { let m = at(map, f, x, y); (if ch == "off" { 0.0 } else { (channel(m, ch, 0.5) - 0.5) * 2.0 * mh }, if cv == "off" { 0.0 } else { (channel(m, cv, 0.5) - 0.5) * 2.0 * mv }) }
            Some((pl, cx, cy, sn, co)) => { // the pixel's place on the canvas; the map's screen shift brought back into the clip's own frame (R^-1 v / scale)
                let (qx, qy) = ((x as f64 - cx) * pl.scale, (y as f64 - cy) * pl.scale); let (px_, py_) = (qx * co - qy * sn + cx + pl.dx, qx * sn + qy * co + cy + pl.dy);
                let Some(m) = map_px(map, px_, py_) else { continue };
                let (vx, vy) = (if ch == "off" { 0.0 } else { (channel(&m, ch, 0.5) - 0.5) as f64 * 2.0 * mh as f64 }, if cv == "off" { 0.0 } else { (channel(&m, cv, 0.5) - 0.5) as f64 * 2.0 * mv as f64 });
                let inv = 1.0 / pl.scale.abs().max(1e-6); (((vx * co + vy * sn) * inv) as f32, ((-vx * sn + vy * co) * inv) as f32) } };
        let (sx, sy) = (x as f32 + dx, y as f32 + dy); let (x0, y0) = (sx.floor(), sy.floor()); let (fx, fy) = (sx - x0, sy - y0); let (x0, y0) = (x0 as i64, y0 as i64);
        let mut acc = [0f32; 4];
        for (ox, oy, wt) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] { if wt <= 0.0 { continue; } if let Some(i) = fetch(x0 + ox, y0 + oy) { let a = src[i + 3]; for j in 0..3 { acc[j] += src[i + j] * a * wt; } acc[3] += a * wt; } }
        let o = (y * w + x) * 4; if acc[3] > 1e-6 { for j in 0..3 { f.d[o + j] = acc[j] / acc[3]; } } f.d[o + 3] = acc[3];
    } }
}
/// Set Matte: the clip's alpha times the matte clip's channel (or its inverse).
pub fn set_matte(e: &Effect, f: &mut Frame, matte: &Frame) {
    if matte.w == 0 || matte.h == 0 { return; }
    let (ch, inv) = (word(e, "channel", "alpha"), e.strings.get("invert").map(|s| s == "1").unwrap_or(false));
    for y in 0..f.h { for x in 0..f.w { let m = channel(at(matte, f, x, y), ch, 0.0); let m = if inv { 1.0 - m } else { m }; f.d[(y * f.w + x) * 4 + 3] *= m.clamp(0.0, 1.0); } }
}

/// Channel Combiner (AE's Calculations / Compound Arithmetic): blend this clip with a second,
/// referenced clip using a named operator -- the true two-clip arithmetic the plain `blend` field
/// (this clip's own 22-mode blend against the clips below it on the timeline) cannot express, since
/// the second input here is an arbitrary OTHER clip, not whatever happens to sit underneath.
/// `operation`: add, subtract, multiply, screen, difference, darken, lighten (clamped 0..1 each).
/// `opacity` (0..1, default 1) mixes the combined result back toward the clip's own original pixel.
/// `ref_channel` ("rgb" default, or any layerfx::CHANNELS name) lets the second input drive from a
/// single channel (e.g. "luma") instead of full RGB, broadcast to all three output channels.
pub fn channel_combiner(e: &Effect, f: &mut Frame, refc: &Frame, lt: f64) {
    if refc.w == 0 || refc.h == 0 { return; }
    let op = word(e, "operation", "add"); let opacity = num(e, "opacity", lt, 1.0).clamp(0.0, 1.0);
    let rch = word(e, "ref_channel", "rgb");
    for y in 0..f.h { for x in 0..f.w {
        let o = (y * f.w + x) * 4; let rp = at(refc, f, x, y);
        let rc = if rch == "rgb" { [rp[0], rp[1], rp[2]] } else { let v = channel(rp, rch, 0.0); [v, v, v] };
        for j in 0..3 { let a = f.d[o + j]; let b = rc[j];
            let r = match op { "subtract" => a - b, "multiply" => a * b, "screen" => 1.0 - (1.0 - a) * (1.0 - b),
                "difference" => (a - b).abs(), "darken" => a.min(b), "lighten" => a.max(b), _ => a + b };
            f.d[o + j] = (a + (r.clamp(0.0, 1.0) - a) * opacity).clamp(0.0, 1.0); }
    } }
}

/// Set Channels (AE's Set Channels): construct each output channel (R,G,B,A) from an arbitrary
/// source -- this clip's own data, the referenced clip's data, or a flat on/off value -- the real gap
/// `shift_channels` (FEATURES.md L469, this-clip-only) left open: a second clip as a channel source.
/// Params per output channel `{r,g,b,a}_source`: "this" (default r->red, g->green, b->blue, a->alpha),
/// "ref" (the referenced clip), "on" (flat 1.0), "off" (flat 0.0); `{r,g,b,a}_channel` names which of
/// layerfx::CHANNELS to read when the source is "this" or "ref" (defaults to the output channel's own
/// name, so an untouched channel is a true identity pass-through).
pub fn set_channels(e: &Effect, f: &mut Frame, refc: &Frame, _lt: f64) {
    let out_ch = ["red", "green", "blue", "alpha"];
    let src = f.d.clone();
    for y in 0..f.h { for x in 0..f.w {
        let o = (y * f.w + x) * 4; let this_px = &src[o..o + 4]; let ref_px = if refc.w > 0 && refc.h > 0 { Some(at(refc, f, x, y)) } else { None };
        for (j, out_name) in out_ch.iter().enumerate() {
            let letter = &out_name[0..1];
            let source = word(e, &format!("{letter}_source"), "this");
            let chan_key = format!("{letter}_channel");
            let chan = word(e, &chan_key, out_name);
            let v = match source { "on" => 1.0, "off" => 0.0,
                "ref" => ref_px.map(|rp| channel(rp, chan, 0.0)).unwrap_or(this_px[j]),
                _ => channel(this_px, chan, 0.0) };
            f.d[o + j] = v.clamp(0.0, 1.0);
        }
    } }
}

/// Match Grain: estimate the reference clip's own grain amplitude (a high-pass residual -- the
/// reference minus a small box blur of itself, RMS over luma) and reproduce an equivalent synthetic
/// grain on THIS clip via the existing Film-Look-Creator grain engine (color/grain_look.rs), closing
/// the two-clip-reference gap named on FEATURES.md L481. `strength` (default 1.0) scales the
/// estimated amount; `size` (default 1.0) is the synthetic grain's own size parameter (grain size
/// itself is not estimated from the reference -- an honest simplification, noted in FEATURES.md).
pub fn match_grain(e: &Effect, f: &mut Frame, refc: &Frame, lt: f64, seed: u64) {
    if refc.w == 0 || refc.h == 0 { return; }
    let mut hp = Frame { w: refc.w, h: refc.h, d: refc.d.clone() };
    let blurred = { let mut b = Frame { w: refc.w, h: refc.h, d: refc.d.clone() }; crate::effects::box_blur3(&mut b, 2); b };
    for i in 0..hp.w * hp.h { for j in 0..3 { hp.d[i * 4 + j] -= blurred.d[i * 4 + j]; } }
    let n = (hp.w * hp.h).max(1);
    let sum_sq: f64 = (0..n).map(|i| { let l = 0.2126 * hp.d[i * 4] as f64 + 0.7152 * hp.d[i * 4 + 1] as f64 + 0.0722 * hp.d[i * 4 + 2] as f64; l * l }).sum();
    let rms = (sum_sq / n as f64).sqrt();
    let strength = num(e, "strength", lt, 1.0).max(0.0) as f64;
    let size = num(e, "size", lt, 1.0).clamp(1.0, 32.0) as f64;
    let amount = (rms * 4.0 * strength).clamp(0.0, 0.5); // the grain effect's own amount range is 0..0.5
    crate::color::grain_look::apply(amount, size, 0.0, 0.0, [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], f, seed);
}
/// Camera Lens Blur with a depth map: the map's channel (depth_channel, default luma, read over black, inverted with invert 1) at each pixel,
/// its distance from focal_depth (default 1 = white in focus) normalised by the farthest a value can be (max(focal, 1 - focal)), scales the radius.
pub fn lens_depth(e: &Effect, f: &mut Frame, map: &Frame, lt: f64) {
    if map.w == 0 || map.h == 0 { return; }
    let (ch, inv) = (word(e, "depth_channel", "luma"), num(e, "invert", lt, 0.0) >= 0.5); let focal = num(e, "focal_depth", lt, 1.0).clamp(0.0, 1.0); let span = focal.max(1.0 - focal).max(1e-6);
    let mut dm = vec![0f32; f.w * f.h];
    for y in 0..f.h { for x in 0..f.w { let v = channel(at(map, f, x, y), ch, 0.0); let v = if inv { 1.0 - v } else { v }; dm[y * f.w + x] = ((v - focal).abs() / span).clamp(0.0, 1.0); } }
    crate::lens::apply_map(f, &crate::lens::from_params(&e.params, lt), Some(&dm));
}
/// 2.5D depth parallax (a "3D photo" camera move): the map's channel (depth_channel, default luma, read over black, invert 1 flips
/// it) is depth, 1 = near, 0 = far. A point at depth d moves by pan * (d - focal_depth) (pan_x of the width, pan_y of the height)
/// and scales about the canvas centre by 1 + dolly * (d - focal_depth); zoom then scales the whole picture (overscan). Each output
/// pixel marches its ray from near to far through the inverse of that move, reading the depth map (nearest pixel) until the map
/// is at or in front of the ray: near layers hide far ones. Where the ray meets a cliff (the map jumps more than 0.12 past the ray,
/// the side of a near object) the pixel is background the camera has just uncovered and takes the colour 1 px beyond the cliff
/// on the far side, so it is filled from the background, not smeared from the object. No move (pan 0, dolly 0, zoom 1) is bit-exact.
pub fn depth_parallax(e: &Effect, f: &mut Frame, map: &Frame, lt: f64) {
    if map.w == 0 || map.h == 0 || f.w == 0 || f.h == 0 { return; }
    let (pxf, pyf, dolly, zoom) = (num(e, "pan_x", lt, 0.0) as f64, num(e, "pan_y", lt, 0.0) as f64, (num(e, "dolly", lt, 0.0) as f64).clamp(-0.9, 0.9), (num(e, "zoom", lt, 1.0) as f64).max(0.01));
    if pxf == 0.0 && pyf == 0.0 && dolly == 0.0 && zoom == 1.0 { return; }
    let (ch, inv) = (word(e, "depth_channel", "luma"), num(e, "invert", lt, 0.0) >= 0.5); let focal = (num(e, "focal_depth", lt, 0.5) as f64).clamp(0.0, 1.0);
    let (w, h) = (f.w, f.h); let (wf, hf) = (w as f64, h as f64); let (cx, cy) = (wf / 2.0, hf / 2.0); let (pxx, pyy) = (pxf * wf, pyf * hf);
    let mut dm = vec![0f32; w * h];
    for y in 0..h { for x in 0..w { let v = channel(at(map, f, x, y), ch, 0.0); dm[y * w + x] = if inv { 1.0 - v } else { v }; } }
    let depth = |px: f64, py: f64| -> f64 { let (xi, yi) = ((px.floor() as i64).clamp(0, w as i64 - 1) as usize, (py.floor() as i64).clamp(0, h as i64 - 1) as usize); dm[yi * w + xi] as f64 };
    let src = f.d.clone();
    // premultiplied bilinear read (clamped at the frame edge); an exact pixel centre returns that pixel untouched
    let read = |px: f64, py: f64| -> [f32; 4] { let (x, y) = ((px - 0.5).clamp(0.0, wf - 1.0), (py - 0.5).clamp(0.0, hf - 1.0)); let (x0, y0) = (x.floor() as usize, y.floor() as usize); let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        if fx == 0.0 && fy == 0.0 { let i = (y0 * w + x0) * 4; return [src[i], src[i + 1], src[i + 2], src[i + 3]]; }
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1)); let mut o = [0f32; 4];
        for (xx, yy, k) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)), (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] { let i = (yy * w + xx) * 4; let a = src[i + 3]; for j in 0..3 { o[j] += src[i + j] * a * k; } o[3] += a * k; }
        if o[3] > 1e-6 { for j in 0..3 { o[j] /= o[3]; } } o };
    let span = focal.max(1.0 - focal); let reach = ((pxx.abs() + pyy.abs()) + dolly.abs() * (cx.hypot(cy))) * span;
    let steps = ((reach * 1.5).ceil() as usize).clamp(8, 160);
    for y in 0..h { for x in 0..w {
        let (qx, qy) = (cx + (x as f64 + 0.5 - cx) / zoom, cy + (y as f64 + 0.5 - cy) / zoom);
        let pos = |d: f64| -> (f64, f64) { let k = 1.0 + dolly * (d - focal); (cx + (qx - cx - pxx * (d - focal)) / k, cy + (qy - cy - pyy * (d - focal)) / k) };
        let hit = |d: f64| { let (a, b) = pos(d); depth(a, b) >= d };
        let mut prev = None; let mut found = 0.0;
        for i in 0..=steps { let d = 1.0 - i as f64 / steps as f64; if hit(d) { found = d; break; } prev = Some(d); }
        let out = match prev {
            None => { let (a, b) = pos(found); read(a, b) }
            Some(lo0) => { let (mut lo, mut hi) = (lo0, found); for _ in 0..10 { let m = 0.5 * (lo + hi); if hit(m) { hi = m; } else { lo = m; } }
                let (ha, hb) = pos(hi);
                if depth(ha, hb) - hi > 0.12 { let (la, lb) = pos(lo); let (mut vx, mut vy) = (la - ha, lb - hb); let mut n = vx.hypot(vy);
                    if n < 1e-9 { let (a2, b2) = pos((lo + 1.0 / steps as f64).min(1.0)); vx = a2 - ha; vy = b2 - hb; n = vx.hypot(vy); }
                    if n > 1e-9 { read(la + vx / n, lb + vy / n) } else { read(la, lb) } }
                else { read(ha, hb) } } };
        let i = (y * w + x) * 4; f.d[i..i + 4].copy_from_slice(&out);
    } }
}
/// Clone/multiply the subject: a second clip (mask_clip, e.g. a person matte) rendered at the same time marks the subject
/// (its channel, mask_channel default luma, at or above mask_threshold = subject, white-on-black convention same as a matte).
/// `count` extra copies of exactly that masked region (the subject's OWN original pixels, not re-warped) are pasted at growing
/// offsets (dx, dy) * k px, each dimmer by decay^(k-1) * amount, painted farthest-first so the real subject (already in f,
/// never moved) always reads on top and sharpest -- the classic "duplicate yourself through the frame" look.
pub fn subject_clone(e: &Effect, f: &mut Frame, mask: &Frame, lt: f64) {
    if mask.w == 0 || mask.h == 0 || f.w == 0 || f.h == 0 { return; }
    let count = (num(e, "count", lt, 2.0).round() as i64).clamp(0, 8);
    let amount = num(e, "amount", lt, 1.0).clamp(0.0, 1.0);
    if count == 0 || amount <= 0.0 { return; }
    let (dx, dy) = (num(e, "dx", lt, 40.0) as f64, num(e, "dy", lt, 0.0) as f64);
    let decay = num(e, "decay", lt, 0.6).clamp(0.0, 1.0);
    let (ch, thr) = (word(e, "mask_channel", "luma"), num(e, "mask_threshold", lt, 0.5).clamp(0.0, 1.0));
    let (w, h) = (f.w, f.h);
    let subject = f.d.clone(); // the subject's own look, lifted before any copy is pasted so copies never sample an already-cloned frame
    let mut cov = vec![0f32; w * h];
    for y in 0..h { for x in 0..w { let m = channel(at(mask, f, x, y), ch, 0.0); cov[y * w + x] = if m >= thr { 1.0 } else { 0.0 }; } }
    for k in (1..=count).rev() {
        let op = amount * decay.powi((k - 1) as i32);
        if op <= 0.002 { continue; }
        let (ox, oy) = ((dx * k as f64).round() as i64, (dy * k as f64).round() as i64);
        for y in 0..h { for x in 0..w {
            let (sx, sy) = (x as i64 - ox, y as i64 - oy); if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 { continue; }
            let (sxu, syu) = (sx as usize, sy as usize); if cov[syu * w + sxu] < 0.5 { continue; }
            let si = (syu * w + sxu) * 4; let a = subject[si + 3] * op; if a <= 0.001 { continue; }
            let di = (y * w + x) * 4; for j in 0..3 { f.d[di + j] = f.d[di + j] * (1.0 - a) + subject[si + j] * a; } f.d[di + 3] += a * (1.0 - f.d[di + 3]);
        } }
    }
}
pub fn apply(e: &Effect, f: &mut Frame, src: &Frame, lt: f64, place: Option<Place>, seed: u64) { match e.kind.as_str() { "displacement_map" => displace(e, f, src, lt, place), "set_matte" => set_matte(e, f, src), "lens_blur" => lens_depth(e, f, src, lt), "depth_parallax" => depth_parallax(e, f, src, lt), "channel_combiner" => channel_combiner(e, f, src, lt), "set_channels" => set_channels(e, f, src, lt), "match_grain" => match_grain(e, f, src, lt, seed), "subject_clone" => subject_clone(e, f, src, lt), "vfx_graph" => crate::vfx_graph::apply_driven(e, f, src, lt, seed), _ => {} } }
