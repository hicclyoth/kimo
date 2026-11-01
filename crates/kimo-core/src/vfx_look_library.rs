//! KIMO 8.5 VFX look library: 9 named looks built entirely on vfx_graph (node chains of existing effects,
//! generators and the particles node), each a self-contained graph JSON parameterized by one 0..1 `intensity`.
//! Exposed through apply_vfx_preset (surreal_vfx_tools.rs) alongside V8.4's dreamy/glitch/vertigo/echo, which
//! stay exactly as they were -- these are additional preset names in the same tool, same intensity dial.
//!
//! V8.5.1: a look-library pass driven by looking at real footage, not just technical correctness. 6 of the 9 looks
//! (ember_drift, chrome_bloom, dream_haze, vhs_decay, aurora_wash, glitch_surge) read as "barely there" on a phone
//! screen at a glance -- reworked below, each verified against real footage with vfx_preview before this file
//! changed (see the 8.5.1 commit message / CHANGELOG for the per-look verdict). magic_sparkle and solar_flare were
//! visible but mild and are pushed further. frost_veil was already strong and is untouched. Two lessons the review
//! surfaced: (1) magic_dust's preset default (x=0.5, y=0.5) sits exactly on a centered subject's face -- pushing an
//! already-centered particle preset "further" without repositioning it means more particles ON the face, so
//! magic_sparkle's dust emitters are now framed to the sides/lower-thirds, never centered; (2) light_rays + bloom +
//! exposure stacked at their old intensity ceilings blew solar_flare's highlights past half the frame (41.7%
//! clipping, mean luma 0.86 measured on real footage) -- "unmistakable" and "washed-out" are different failures,
//! and solar_flare's ranges are tuned to stop well short of the second one.
//!
//! V8.5.2: three looks still didn't read at phone size after the 8.5.1 pass -- glitch_surge, vhs_decay, frost_veil.
//! glitch_surge's peak-frame capture (fixed t=0.8s against a 0.55s pulse period) had landed on an already-decayed
//! frame, and even on-hit the ceilings were too low; now two stacked glitch_blocks layers (coarse + fine bands) with
//! a lowered slice_threshold so most of the frame's bands trigger together, a shortened ~0.6s period, and a harder
//! rgb_split pulse. vhs_decay had no low-res softness and no warm cast (a real camcorder skews warm, not neutrally
//! desaturated); added a pre-blur and a post-grain temperature push, and raised wobble/bleed/noise/bands. frost_veil
//! (reworked once already before 8.5.0, then made subtle again in 8.5.1's untouched pass) is graded with
//! color_wheels instead of temperature (temperature's amount is scaled 0.2x internally and reads as almost nothing)
//! and edge_glow now runs on the blurred/graded buffer instead of the raw frame -- edge_glow on the raw frame is
//! exactly what produced v01's "wireframe" face; the vignette is now tinted icy white-blue instead of plain black,
//! and two off-center snow emitters add light drift clear of a centered face.
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
/// A particles node with NO preset -- every field given explicitly. Used where a preset's own defaults (e.g.
/// magic_dust's frame-centered x/y) would fight the placement a look actually needs.
fn particles_raw(id: &str, params: Value) -> Value {
    json!({"id": id, "type": "particles", "params": params})
}

/// V8.5.3 vhs_decay: a hard-edged duty-cycle stripe at a FIXED pixel period (not spacing-in-rows like the native
/// `scanlines` effect), via pixel_expr. A smooth sine-wave luminance test read as a soft vignette rather than
/// scanline texture (the eye needs real edges, not slow brightness drift) and, like the native `scanlines` effect's
/// 1-row darkening, vanished under a ~6x thumbnail downscale (200px wide, as a 4fps strip renders). This survives
/// that downscale as real horizontal banding because the period (22px @720p) and duty cycle give wide enough bands
/// that an area-correct resize still shows them. `strength` (how dark the stripe gets) scales with intensity.
fn vhs_scanline_expr(i: f64) -> String {
    let period = 22.0_f64;
    let duty = 0.42_f64;
    let feather = 2.0_f64;
    let strength = 0.16 + 0.25 * i;
    let m = format!("(y - floor(y/{period})*{period})");
    let dark = format!("(1.0 - smoothstep({:.4}, {:.4}, {m}))", period * duty - feather, period * duty);
    let factor = format!("(1.0 - {strength}*{dark})");
    format!("vec4(red(src)*{factor}, green(src)*{factor}, blue(src)*{factor}, alpha(src))")
}

/// V8.5.3 vhs_decay: the `vhs` effect's own tracking band (its `bands` param) only adds extra horizontal
/// displacement in a narrow region -- it never brightens or adds noise there, so "a bright, noisy tracking band
/// rolling through the frame" (what real worn tape looks like) was never actually visible, only a faint wobble.
/// This pixel_expr node adds a second band at the SAME roll phase as the `vhs` effect's own (fract(t*0.35)*h, so
/// the two read as one event, not two uncorrelated glitches): inside a smoothstep-falloff half-width band it
/// brightens the image, resamples with extra per-row horizontal jitter (sample(u+offset,v)), and adds strong
/// hashed per-pixel noise, all masked to fall off to nothing outside the band. jitter/brightness/noise amount
/// scale with intensity. (No `abs` builtin in this expression language -- sqrt(x*x) stands in for it.)
fn vhs_tracking_band_expr(i: f64) -> String {
    let speed = 0.35_f64;
    let halfwidth_frac = 0.05_f64;
    let jitter = 0.02 + 0.035 * i;
    let bright = 0.3 + 0.45 * i;
    let noise_amt = 0.3 + 0.45 * i;
    let by = format!("(fract(t*{speed})*h)");
    let dist = format!("sqrt((y - {by})*(y - {by}))");
    let hw = format!("(h*{halfwidth_frac})");
    let mask = format!("smoothstep({hw}, 0.0, {dist})");
    let off = format!("((noise(11.0, floor(y/2.0), floor(t*18.0)) - 0.5) * {jitter} * {mask})");
    let col = format!("sample((u + {off}), v)");
    let nz = format!("((noise(23.0, x, y) - 0.5) * {noise_amt} * {mask})");
    let bb = format!("(1.0 + {bright}*{mask})");
    format!(
        "vec4(clamp(red({col})*{bb} + {nz}, 0.0, 1.0), clamp(green({col})*{bb} + {nz}, 0.0, 1.0), clamp(blue({col})*{bb} + {nz}, 0.0, 1.0), alpha({col}))"
    )
}
/// A punchy, repeating pulse as a keyframe Prop: holds near `low`, spikes to `high` over `attack` seconds, decays
/// back to `low` over the rest of `period`, then repeats forever via loop_out "cycle" -- a deterministic stand-in
/// for "on the beat" with no audio analysis needed (vfx_graph exposes only the current instant t; see vfx_graph.rs's
/// module note). Used to drive glitch_surge's block-glitch and RGB-split amounts so they read as discrete punches,
/// not a constant glitchy hum.
fn pulse_prop(period: f64, low: f64, high: f64) -> Value {
    let (attack, hold) = (0.045, 0.1);
    json!({
        "base": low,
        "keys": [
            {"t": 0.0, "v": low, "ease": "linear"},
            {"t": attack, "v": high, "ease": "out"},
            {"t": attack + hold, "v": low, "ease": "out"},
            {"t": period, "v": low, "ease": "linear"},
        ],
        "expr": [{"op": "loop_out", "mode": "cycle"}],
    })
}
const SRC: &str = "__frame__";

pub const NAMES: [&str; 9] = ["ember_drift", "chrome_bloom", "glitch_surge", "dream_haze", "frost_veil", "magic_sparkle", "vhs_decay", "aurora_wash", "solar_flare"];

pub fn describe(name: &str) -> Option<&'static str> {
    Some(match name {
        "ember_drift" => "warm glowing embers rising off the lower frame edge (deterministic particle line emitter, not a preset default) into a soft teal/orange bloom grade",
        "chrome_bloom" => "cool metallic grade (temperature pulled down, desaturated, contrast lifted) with chrome edge fringe and a strong, cool-tinted bloom on highlights",
        "glitch_surge" => "unmistakable block-glitch and RGB-split punches on a repeating ~0.6s pulse -- bold horizontal slice displacement across most of the frame on each hit, clean frames between hits",
        "dream_haze" => "clear halation bleeding from highlights, soft overall glow, and a lifted pastel grade (shadows lifted and gently tinted, slightly desaturated)",
        "frost_veil" => "unmistakable icy blue-white grade with a frosted glowing rim, crystalline frost vignette at the edges, and light snow drifting at the sides",
        "magic_sparkle" => "golden/violet magic dust framed to the sides and lower-thirds (kept off the face on purpose) over a strong warm/violet bloom",
        "vhs_decay" => "obvious worn-tape 90s-camcorder VHS: soft/low-res, line wobble, strong chroma bleed, a rolling tracking band, tape noise, scanlines, warm tint and vignette",
        "aurora_wash" => "a flowing multi-stop teal/blue/violet colour wash that slowly swings and breathes across the frame, with drifting fireflies",
        "solar_flare" => "a strong golden solar flare: light rays plus halation plus bloom and a warm grade, tuned to stay short of blowing out the whole frame",
        _ => return None,
    })
}

/// Builds the full vfx_graph JSON for a named look, scaled by intensity (0..1, matching apply_vfx_preset's existing dial).
pub fn graph_for(name: &str, i: f64) -> Result<Value, String> {
    let i = i.clamp(0.0, 1.0);
    let g = match name {
        // V8.5.1: embers now emit from a wide line along the very bottom of frame (not the "sparks" preset's fixed
        // point), rising with gentle buoyancy and real size/brightness; life is kept short enough (~2.2s at modest
        // upward speed) that they fade out around a third of the way up frame -- visible and warm, clear of a
        // typical centered subject's face. Verified on real footage: unmistakable at i=0.6, stronger at i=1.0.
        "ember_drift" => json!({"nodes": [
            eff("grade", SRC, "teal_orange", json!({"amount": 0.3 + 0.35 * i})),
            eff("glow", "grade", "bloom", json!({"amount": 0.3 + 0.5 * i, "threshold": 0.55})),
            particles_raw("embers", json!({
                "shape": "line", "x": 0.5, "y": 0.99, "width": 0.6,
                "rate": 90.0 + 170.0 * i, "life": 2.3, "life_var": 0.6,
                "speed": 85.0, "speed_var": 40.0, "angle": 270, "spread": 14,
                "gravity": -15, "drag": 0.3, "turbulence": 70, "turb_scale": 0.35,
                "size0": 9.0 + 5.0 * i, "size1": 1.5, "size_var": 0.4,
                "color0": [1.0, 0.68, 0.18], "color1": [0.6, 0.08, 0.02],
                "alpha0": 1.0, "alpha1": 0.0, "sprite": "soft", "additive": true,
                "prewarm": true, "seed": 11,
            })),
            blend("out", "glow", "embers", "screen", 1.0),
        ], "output": "out"}),
        // V8.5.1: bloom pushed harder (lower threshold, higher amount, a cool tint via tint_r/g/b) and the grade
        // redone as cool temperature + desaturation + a touch more contrast, instead of relying on fringe alone.
        // Chromatic aberration and sharpen are dialled back from an earlier pass that aliased badly on a patterned
        // shirt at i=1.0 (visible moire from the fringe amplifying a high-frequency pattern).
        "chrome_bloom" => json!({"nodes": [
            eff("split", SRC, "chromatic_aberration", json!({"amount": 1.3 + 2.8 * i})),
            eff("sharp", "split", "sharpen", json!({"amount": 0.12 + 0.18 * i})),
            eff("cool", "sharp", "temperature", json!({"amount": -(0.22 + 0.3 * i)})),
            eff("desat", "cool", "saturation", json!({"amount": (1.0 - 0.22 * i).max(0.55)})),
            eff("contrast", "desat", "contrast", json!({"amount": 1.0 + 0.18 * i})),
            eff("out", "contrast", "bloom", json!({"amount": 0.45 + 0.65 * i, "threshold": 0.52, "tint_r": 0.75, "tint_g": 0.87, "tint_b": 1.0})),
        ], "output": "out"}),
        // V8.5.1: glitch_surge was a constant mild hum before (fixed amounts at every frame). Now amount/rgb_split
        // ride pulse_prop -- a repeating keyframed spike (period 0.55s) -- so the frame punches hard on the attack
        // and settles to a light scanline/fringe texture between hits. Peak-intensity ceilings dialled back once
        // from an even punchier first pass that fully scrambled a face at the attack frame.
        // V8.5.2: period shortened to 0.6s (every ~0.5-0.7s per the brief) and both the coarse-band and fine-band
        // amounts raised hard, with slice_threshold lowered so most slices (not a scattered few) trigger on each
        // hit -- "a couple of frames show small edge blocks" was the exact failure: the old peak-frame capture
        // (fixed t=0.8s against a 0.55s period) landed on a decayed frame, and even on-hit the old ceilings were
        // too low to read as unmistakable. Two stacked glitch_blocks layers (9 coarse slices for bold full-width
        // bands, 24 fine slices layered on top for texture) plus a harder rgb_split pulse reads as a real punch;
        // between hits everything settles to near zero (clean) with only a light scanline texture.
        "glitch_surge" => json!({"nodes": [
            eff("bigblocks", SRC, "glitch_blocks", json!({"amount": pulse_prop(0.6, 0.0, 50.0 + 60.0 * i), "slices": 9.0, "rate": 8.0, "slice_threshold": 0.25, "green_offset": 14.0 + 10.0 * i})),
            eff("fineblocks", "bigblocks", "glitch_blocks", json!({"amount": pulse_prop(0.6, 1.5 + 1.5 * i, 18.0 + 16.0 * i), "slices": 24.0, "rate": 20.0, "slice_threshold": 0.45, "green_offset": 6.0})),
            eff("split", "fineblocks", "rgb_split", json!({"amount": pulse_prop(0.6, 0.4 + 0.4 * i, 14.0 + 16.0 * i)})),
            eff("out", "split", "scanlines", json!({"amount": 0.08 + 0.1 * i, "spacing": 3.0})),
        ], "output": "out"}),
        // V8.5.1: added a real halation pass (the dedicated `halation` effect, not just bloom) for the "clear
        // halation" ask, plus a `grade` lift/gain pass for the "lifted pastel" ask (gentle lift on all three
        // channels, slight gain pull on blue/green) and a touch of desaturation so the lift reads as pastel rather
        // than just "brighter".
        "dream_haze" => json!({"nodes": [
            eff("soft", SRC, "blur", json!({"radius": 1.0 + 2.0 * i})),
            eff("hal", "soft", "halation", json!({"threshold": 0.6, "radius": 18.0 + 14.0 * i, "amount": 0.45 + 0.65 * i, "r": 1.0, "g": 0.5, "b": 0.45})),
            eff("glow", "hal", "bloom", json!({"amount": 0.3 + 0.4 * i, "threshold": 0.6})),
            eff("lift", "glow", "grade", json!({"lift_r": 0.05 + 0.06 * i, "lift_g": 0.04 + 0.05 * i, "lift_b": 0.07 + 0.07 * i, "gain_r": 0.98, "gain_g": 0.98, "gain_b": 1.0})),
            eff("pastelsat", "lift", "saturation", json!({"amount": (1.0 - 0.2 * i).max(0.78)})),
            eff("out", "pastelsat", "vignette", json!({"amount": 0.1 + 0.15 * i})),
        ], "output": "out"}),
        // Untouched: Tab's verdict was "strong" on real footage. The previous worker already reworked this once
        // (edge_glow had read as a full wireframe, not a frosted rim) before the 8.5.0 wrap.
        // V8.5.2: v02's rework (temperature + edge_glow on the raw frame) read as subtle -- `temperature`'s amount
        // is internally scaled by 0.2 so even a -1 push is a mild shift, and edge_glow at a readable amount applied
        // directly to the source outlines facial features (the exact "wireframe" failure from v01). Fixed by (1)
        // grading with color_wheels instead (lift/gamma/gain per channel pushed cool/blue, a much stronger and more
        // controllable push than temperature alone), (2) running edge_glow on the already-blurred/graded buffer
        // instead of the raw frame so the glow reads as a soft rim rather than a crisp outline on facial detail,
        // (3) a vignette tinted icy white-blue (color_r/g/b) instead of the default black darken, for an actual
        // "crystalline frost at the edges" look instead of a plain dark vignette, and (4) two off-center snow
        // particle emitters (sides only, well clear of a centered face) for the "light snow" ask.
        "frost_veil" => json!({"nodes": [
            eff("wheels", SRC, "color_wheels", json!({
                "lift_r": -(0.09 + 0.08 * i), "lift_g": -0.01, "lift_b": 0.09 + 0.1 * i,
                "gamma_r": 1.0 - 0.09 * i, "gamma_g": 1.0, "gamma_b": 1.0 + 0.09 * i,
                "gain_r": 1.0 - 0.12 * i, "gain_g": 1.0 - 0.02 * i, "gain_b": 1.0 + 0.16 * i,
            })),
            eff("desat", "wheels", "saturation", json!({"amount": (1.0 - 0.3 * i).max(0.62)})),
            eff("soft", "desat", "blur", json!({"radius": 0.5 + 0.6 * i})),
            eff("edge", "soft", "edge_glow", json!({"amount": 0.32 + 0.38 * i, "r": 0.45, "g": 0.78, "b": 1.0})),
            blend("rim", "soft", "edge", "screen", 0.5 + 0.3 * i),
            eff("frostvig", "rim", "vignette", json!({"amount": 0.42 + 0.3 * i, "feather": 0.34, "roundness": 0.3, "color_r": 0.5, "color_g": 0.78, "color_b": 1.0})),
            eff("crystals", "frostvig", "grain", json!({"amount": 0.03 + 0.04 * i, "size": 1.4})),
            particles_raw("snow_l", json!({"preset": "snow", "seed": 31, "x": 0.08, "width": 0.18, "rate": 35.0 + 40.0 * i, "size0": 4.0, "size1": 4.0, "alpha0": 0.55, "alpha1": 0.55})),
            particles_raw("snow_r", json!({"preset": "snow", "seed": 47, "x": 0.92, "width": 0.18, "rate": 35.0 + 40.0 * i, "size0": 4.0, "size1": 4.0, "alpha0": 0.55, "alpha1": 0.55})),
            blend("snow_both", "snow_l", "snow_r", "screen", 1.0),
            blend("out", "crystals", "snow_both", "screen", 1.0),
        ], "output": "out"}),
        // V8.5.1: pushed further per the brief, AND fixed a face-safety bug the push would otherwise have made
        // worse -- magic_dust's preset default (x=0.5, y=0.5, a 0.15-radius circle) sits squarely on a centered
        // subject's face. The three dust emitters below are explicit (particles_raw, no preset) and placed left,
        // right and low-center, framing the subject instead of covering it.
        "magic_sparkle" => json!({"nodes": [
            eff("glow", SRC, "bloom", json!({"amount": 0.35 + 0.55 * i, "threshold": 0.55, "tint_r": 1.0, "tint_g": 0.92, "tint_b": 1.0})),
            particles_raw("dust_l", json!({"preset": "magic_dust", "seed": 23, "x": 0.12, "y": 0.55, "width": 0.16, "rate": 90.0 + 160.0 * i, "size0": 10.0 + 4.0 * i})),
            particles_raw("dust_r", json!({"preset": "magic_dust", "seed": 57, "x": 0.88, "y": 0.55, "width": 0.16, "rate": 90.0 + 160.0 * i, "size0": 10.0 + 4.0 * i, "color0": [0.8, 0.65, 1.0], "color1": [1.0, 0.85, 0.4]})),
            particles_raw("dust_h", json!({"preset": "magic_dust", "seed": 71, "x": 0.5, "y": 0.85, "width": 0.28, "rate": 60.0 + 120.0 * i, "size0": 8.0 + 3.0 * i})),
            blend("dust_lr", "dust_l", "dust_r", "screen", 1.0),
            blend("dust_all", "dust_lr", "dust_h", "screen", 1.0),
            blend("out", "glow", "dust_all", "screen", 1.0),
        ], "output": "out"}),
        // V8.5.1: now built on the dedicated `vhs` effect (per-line wobble, chroma bleed via split RGB sampling,
        // noise, and a moving tracking band) layered with scanlines/grain/desaturation/vignette, instead of
        // grain+scanlines+chromatic_aberration+saturation alone -- the old chain had no actual line wobble or
        // tracking band, which is most of what reads as "VHS" rather than just "degraded".
        // V8.5.2: added a pre-blur for a genuine soft/low-res feel, raised wobble/bleed/noise/bands, layered a warm
        // `temperature` push after the grain (90s camcorders skew warm, not neutral-desaturated) and widened the
        // vignette -- the v02 chain (vhs + scanlines + grain + flat desaturation) had no low-res softness and no
        // warm cast, which is why it read as "slightly desaturated video" rather than tape.
        // V8.5.3: still didn't read at PHONE-THUMBNAIL size (a 4fps strip at 200px wide) even though it looked like
        // real tape at full size -- measured directly: the native `scanlines` effect darkens exactly 1 row every
        // `spacing` rows, and a slow sine-wave luminance test (tried first, see git history of this comment) reads
        // as a soft vignette, not scanline texture, because the eye needs real edges, not slow brightness drift --
        // both vanish under a ~6x thumbnail downscale. Replaced with two pixel_expr nodes, each verified at both
        // full size and a resized 200px-wide thumbnail before this file changed: `vhs_scanline_expr` is a hard-
        // edged duty-cycle stripe at a fixed pixel period (not spacing-in-rows), which survives the thumbnail
        // downscale as real banding; `vhs_tracking_band_expr` adds brightness + strong hashed noise + extra
        // horizontal jitter inside a smoothstep-falloff band at the SAME roll phase as the `vhs` effect's own
        // (fract(t*0.35)*h), because the `vhs` effect's own band only displaces, it never brightens or adds noise,
        // so the "roll" by itself was never visible at a glance. Also added `levels` for lifted/crushed blacks
        // (a faded, reduced-contrast tape floor -- the old chain had full 0..1 range) and raised the pre-blur for
        // a softer, lower-resolution feel ("downscale and back up, or similar" -- a real downscale/upscale round
        // trip isn't a vfx_graph node, so a stronger blur stands in for it, same approach the brief allowed).
        // Checked: 0% clipping at both the default intensity (0.4) and dialed to 1.0; face stays recognizable at
        // 1.0 too, though 0.4 is the cleaner default. No OSD text added -- vfx_graph's only generators are
        // solid/gradient/shape (circle/rect), there is no text/glyph node to drive a "PLAY"/"REC" overlay from, so
        // per the brief this is skipped rather than faked with shapes.
        "vhs_decay" => json!({"nodes": [
            eff("soft", SRC, "blur", json!({"radius": 1.3 + 2.0 * i})),
            eff("tape", "soft", "vhs", json!({"wobble": 3.0 + 4.5 * i, "bleed": 4.5 + 5.0 * i, "noise": 0.12 + 0.16 * i, "bands": 1.0 + 1.4 * i})),
            eff("scan", "tape", "pixel_expr", json!({"expr": vhs_scanline_expr(i)})),
            eff("band", "scan", "pixel_expr", json!({"expr": vhs_tracking_band_expr(i)})),
            eff("grain", "band", "grain", json!({"amount": 0.05 + 0.08 * i})),
            eff("warm", "grain", "temperature", json!({"amount": 0.12 + 0.14 * i})),
            eff("desat", "warm", "saturation", json!({"amount": (1.0 - 0.18 * i).max(0.7)})),
            eff("crush", "desat", "levels", json!({"in_black": 0.02 + 0.02 * i, "in_white": 1.0, "gamma": 1.0, "out_black": 0.05 + 0.06 * i, "out_white": 0.95 - 0.05 * i})),
            eff("out", "crush", "vignette", json!({"amount": 0.16 + 0.16 * i})),
        ], "output": "out"}),
        // V8.5.1: a static overlay read as a flat tinted filter, not "flowing colour bands". The gradient's angle
        // and middle stop now ride Sin-expr keyframe Props (no audio/time-offset sampling available -- see
        // vfx_graph.rs's module note -- so this is a slow deterministic breathing motion, not a literal flow), a
        // third colour stop was added for a richer band structure, the wash is blurred before blending so it reads
        // as soft bands rather than a sharp gradient, and overlay opacity and bloom were both raised.
        "aurora_wash" => json!({"nodes": [
            eff("glow", SRC, "bloom", json!({"amount": 0.2 + 0.3 * i, "threshold": 0.68})),
            {"id": "wash", "type": "gradient", "params": {
                "angle_deg": {"base": 35.0, "expr": [{"op": "sin", "freq": 0.07, "amp": 22.0}]},
                "stops": [
                    [0.0, [0.05, 0.85, 0.7]],
                    [0.5, [0.25, 0.55, 0.95]],
                    [1.0, [0.72, 0.18, 0.88]],
                ],
            }},
            eff("washblur", "wash", "blur", json!({"radius": 30})),
            blend("washed", "glow", "washblur", "screen", 0.32 + 0.42 * i),
            particles("fireflies", "fireflies", json!({"seed": 21, "rate": 6.0 + 12.0 * i})),
            blend("out", "washed", "fireflies", "screen", 1.0),
        ], "output": "out"}),
        // V8.5.1: pushed further per the brief, AND fixed washout the push introduced on the first pass -- a
        // stronger light_rays + halation + bloom + exposure stack blew 41.7% of real footage to clipped white
        // (mean luma 0.86). Every amount below is tuned down from that first pass to land a strong, unmistakable
        // flare that still leaves the subject's face and most of the frame readable.
        "solar_flare" => json!({"nodes": [
            eff("rays", SRC, "light_rays", json!({"amount": 0.45 + 0.55 * i, "threshold": 0.58, "cx": 0.68, "cy": 0.16, "length": 0.42})),
            eff("hal", "rays", "halation", json!({"threshold": 0.62, "radius": 24.0, "amount": 0.35 + 0.4 * i, "r": 1.0, "g": 0.62, "b": 0.22})),
            eff("glow", "hal", "bloom", json!({"amount": 0.3 + 0.4 * i, "threshold": 0.58})),
            eff("grade", "glow", "teal_orange", json!({"amount": 0.28 + 0.3 * i})),
            eff("out", "grade", "exposure", json!({"stops": 0.06 + 0.14 * i})),
        ], "output": "out"}),
        other => return Err(format!("unknown vfx look '{other}'; looks: {}", NAMES.join(", "))),
    };
    Ok(g)
}
