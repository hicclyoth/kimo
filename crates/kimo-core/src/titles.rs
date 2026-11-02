//! Designed title presets. Each builds a few text/shape clips with animators and keyframes in one call.
use crate::commands::Session;
use crate::keyframe::{Ease, Prop};
use crate::model::*;
use serde_json::{json, Value};

fn clip(ss: &mut Session, source: Source, start: f64, dur: f64) -> Clip { Clip { id: ss.project.gen_id("clip"), source, start, duration: dur, in_point: 0.0, speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props: default_props(), grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None } }
/// Tight-ink-extent width for a subscribe_pop-style pill badge (V7.7Q): measure the actual drawn
/// (uppercase, bold) text and pad it, rather than guessing a fixed width. Exposed as its own
/// function (8.5.4) so a later size fix -- lint_edit's text_too_small auto-fix bumping this same
/// text's font size after the badge was already built -- can recompute the SAME pill a second time
/// for the new size, instead of leaving a pill sized for text that no longer exists on screen.
pub(crate) fn pill_width_for_text(text: &str, font_name: &str, text_size: f64, wpx: f64, k: f64) -> f64 {
    let upper = text.to_uppercase();
    let (text_w, _) = crate::text::measure(&upper, font_name, text_size, 0.0, 700.0, "center", wpx).unwrap_or((180.0 * k, 0.0));
    let side_pad = 56.0 * k; // room on both sides so the pill visibly hugs, not clips, the text
    (text_w + side_pad).max(190.0 * k) // a one-word label still reads as a normal pill, not a hugged sliver
}
/// The subscribe_pop dot's own x offset (clip-local, canvas-center-relative), given a pill of width
/// `pill_w` and the dot's own width `dot_w` (8.5.4, second pass; formula corrected 8.5.5): both the
/// initial subscribe_pop build AND the later text_too_small pill-resize fix compute the dot's position
/// from this SAME formula, so a pill that grows after the badge was first laid out carries the dot's
/// clearance with it instead of leaving the dot at its old spot. 8.5.4 put the dot OUTSIDE the pill's
/// left edge (a small bell badge look) -- on a real render (Tab's montage review) that read as the dot
/// floating alone, disconnected from the badge. 8.5.5: the dot now sits INSIDE the pill, 14px (scaled)
/// clear of its left edge; `dot_room` below grows the pill by the dot's own footprint so the dot never
/// has to compete with the text for room. This offset (and the 14*k left clearance) is unchanged by
/// the 8.5.5 follow-up below: that pass only widens the room `dot_room` carves out of the pill, which
/// is what actually opens real breathing room between the dot and the text (see `dot_room`'s own doc
/// comment for why).
pub(crate) fn dot_x_for_pill(pill_w: f64, dot_w: f64, k: f64) -> f64 { -(pill_w / 2.0) + dot_w / 2.0 + 14.0 * k }
/// The gap between the dot's right edge and the text's left edge, as a multiple of the dot's own
/// diameter (8.5.5 follow-up: Tab viewed real_v57.mp4 at 20.5s and found the dot only ~3px clear of the
/// "S" in SUBSCRIBE -- technically not overlapping, but read as a dot jammed right up against the text).
/// Target is the middle of the 0.8-1.0x spec range Tab gave.
const DOT_TEXT_GAP_RATIO: f64 = 0.9;
/// Extra pill width a subscribe_pop badge's dot needs so it has its own clear lane inside the pill's
/// left edge, left of the text, without shrinking the text's own side padding. Added to
/// `pill_width_for_text`'s result on both the initial build and the later resize fix, so the two never
/// drift apart (see `dot_x_for_pill`'s own doc comment).
///
/// The text clip is never given its own x (it stays at the pill's own center, like the pill itself), so
/// widening the pill by some amount `extra` moves BOTH of the pill's edges outward by `extra / 2` --
/// including the edge the dot clings to (`dot_x_for_pill` is pinned `14*k` in from the pill's left edge).
/// The text's own left edge does not move at all (it is fixed at `-text_w / 2`, independent of pill
/// width), so each unit of pill growth only delivers HALF a unit of real gap between the dot and the
/// text: gap = 14*k + extra/2 - dot_w (algebra below). To land a gap of `DOT_TEXT_GAP_RATIO * dot_w`
/// this solves for `extra` and doubles it to account for that 50/50 split -- this is the "account for
/// the centering" fix rather than giving the text clip its own x offset, so the resize-fix path (which
/// already recomputes this same pill width) needs no new step of its own to keep the two in step.
///
/// Derivation (pill and text both centered at the canvas origin, pill width `W`, text ink width `tw`):
///   text_left   = -tw / 2
///   dot_right   = -(W / 2) + dot_w + 14*k                      (from dot_x_for_pill + dot_w/2)
///   gap         = text_left - dot_right = W/2 - tw/2 - dot_w - 14*k
/// `W` is `pill_width_for_text(...)` (== tw + side_pad, side_pad split 50/50) plus this `dot_room`
/// (`extra`), so `W/2 - tw/2` is exactly `side_pad/2 + extra/2` -- substituting and solving for `extra`
/// at the target gap gives the `2.0 * (...)` below, independent of `tw`, `side_pad` or whether
/// `pill_width_for_text`'s own `190*k` floor is active (both sides of the pill's growth cancel the
/// same way either way).
pub(crate) fn dot_room(dot_w: f64, k: f64) -> f64 { 2.0 * ((1.0 + DOT_TEXT_GAP_RATIO) * dot_w - 14.0 * k) }

fn an(unit: &str, shape: &str, sm: f64) -> TextAnimator { TextAnimator { id: String::new(), unit: unit.into(), shape: shape.into(), start: Prop::new(0.0), end: Prop::new(1.0), offset: Prop::new(0.0), smoothness: Prop::new(sm), x: Prop::new(0.0), y: Prop::new(0.0), scale: Prop::new(1.0), rotation: Prop::new(0.0), opacity: Prop::new(1.0), tracking: Prop::new(0.0), blur: Prop::new(0.0), random: false, seed: 0, fill: None, fill_amount: None, ease: None, wiggly: None } }
fn sweep(a: &mut TextAnimator, t0: f64, t1: f64, sm: f64, e: Ease) { let hi = 1.0 + 2.0 * sm.max(0.05); a.offset.set_key(t0, 0.0, e); a.offset.set_key(t1, hi, e); }
fn num(a: &Value, k: &str, d: f64) -> f64 { a.get(k).and_then(|v| v.as_f64()).unwrap_or(d) }
fn rgb(a: &Value, k: &str, d: [f64; 3]) -> [f64; 3] { a.get(k).and_then(|v| v.as_array()).filter(|v| v.len() >= 3).map(|v| [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0), v[2].as_f64().unwrap_or(0.0)]).unwrap_or(d) }
fn txt(text: &str, font: &str, size: f64, color: [f64; 3], spacing: f64, align: &str) -> Source { Source::Text { text: text.into(), font: font.into(), size, color, letter_spacing: spacing, stroke_width: 0.0, stroke_color: [0.0; 3], shadow: 0.0, align: align.into() } }
fn fade(c: &mut Clip, fin: f64, fout: f64) { let d = c.duration; let o = c.props.get_mut("opacity").unwrap(); o.base = 0.0; o.set_key(0.0, 0.0, Ease::Out); o.set_key(fin, 1.0, Ease::Out); o.set_key((d - fout).max(fin + 0.01), 1.0, Ease::In); o.set_key(d, 0.0, Ease::In); }

pub const STYLES: &[&str] = &["cinematic", "impact", "lower_third", "kinetic", "elegant", "neon", "hook_box", "subscribe_pop"];

fn peak(p: &Prop) -> f64 { p.keys.iter().map(|k| k.v).fold(p.base, f64::max) }
/// A title line wider than the canvas (a long word on 9:16) is scaled down to 90% of the width: type size, letter spacing and
/// animated tracking together, measured at the widest pose (largest tracking and scale keys). A title that fits is untouched.
fn fit_width(c: &mut Clip, cw: f64) -> Result<(), String> {
    let Source::Text { text, font, size, letter_spacing, align, .. } = &mut c.source else { return Ok(()) };
    let track = c.text_fx.as_ref().map(|fx| fx.animators.iter().filter(|a| a.unit == "char").map(|a| peak(&a.tracking)).fold(0.0, f64::max)).unwrap_or(0.0);
    let m = crate::text::measure_text(text, font, *size, *letter_spacing + track, c.text_fx.as_ref().map(|f| f.weight).unwrap_or(0.0), align, cw, 0.0, c.text_fx.as_ref())?;
    let wide = m.width * peak(&c.props["scale"]).max(1.0); // xform-lint: ok (clip setup: the new title's own scale keys, before any parenting)
    if wide <= cw * 0.96 { return Ok(()); }
    let r = cw * 0.9 / wide; *size *= r; *letter_spacing *= r;
    if let Some(fx) = c.text_fx.as_mut() { for a in fx.animators.iter_mut() { a.tracking.base *= r; for k in a.tracking.keys.iter_mut() { k.v *= r; } } }
    Ok(())
}
pub fn add_title(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let tid = a.get("track_id").and_then(|v| v.as_str()).ok_or("missing track_id")?.to_string(); let style = a.get("style").and_then(|v| v.as_str()).ok_or("missing style")?.to_string();
    let text = a.get("text").and_then(|v| v.as_str()).ok_or("missing text")?.replace("\\n", "\n"); let sub = a.get("subtitle").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (start, dur) = (num(a, "start", 0.0), num(a, "duration", 4.0)); let k = ss.project.height.min(ss.project.width) as f64 / 1080.0; /* the short side: a 9:16 frame sizes like 16:9 turned on end */ let wpx = ss.project.width as f64;
    let accent = rgb(a, "accent", [0.95, 0.72, 0.35]); let color = rgb(a, "color", [1.0; 3]); let font_o = a.get("font").and_then(|v| v.as_str());
    // a reveal paced for the 4 s default ends by mid-title on a short one, so a 1 s title is on screen for its second half
    let intro = |t0: f64, natural_end: f64| -> f64 { natural_end.min((dur * 0.5).max(t0 + 0.05)) };
    let mut clips: Vec<Clip> = vec![];
    match style.as_str() {
        "cinematic" => {
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("CormorantGaramond"), 120.0 * k, color, 0.0, "center"), start, dur);
            let mut fx = TextFx { weight: 500.0, uppercase: true, shadow_blur: 18.0 * k, shadow_opacity: 0.55, shadow_angle: 90.0, shadow_color: [0.0; 3], ..Default::default() }; fx.shadow_blur = 18.0 * k;
            let mut t = an("char", "square", 0.0); t.tracking.base = 0.0; t.tracking.set_key(0.0, 46.0 * k, Ease::Out); t.tracking.set_key(dur, 8.0 * k, Ease::Out); t.id = "an1".into();
            let mut b = an("char", "square", 0.0); b.id = "an2".into(); b.blur.set_key(0.0, 16.0, Ease::Out); b.blur.set_key(1.4, 0.0, Ease::Out); b.blur.base = 0.0;
            fx.animators = vec![t, b]; c.text_fx = Some(fx); c.props.get_mut("y").unwrap().base = -20.0 * k; fade(&mut c, 1.3, 0.8); clips.push(c);
            if !sub.is_empty() { let mut s = clip(ss, txt(&sub, font_o.unwrap_or("Montserrat"), 28.0 * k, [0.85, 0.85, 0.85], 0.0, "center"), start, dur); let mut fx = TextFx { weight: 400.0, uppercase: true, ..Default::default() }; let mut t = an("char", "square", 0.0); t.id = "an1".into(); t.tracking.base = 14.0 * k; t.tracking.set_key(0.0, 30.0 * k, Ease::Out); t.tracking.set_key(dur, 14.0 * k, Ease::Out); fx.animators = vec![t]; s.text_fx = Some(fx); s.props.get_mut("y").unwrap().base = 105.0 * k; fade(&mut s, 1.8, 0.8); clips.push(s); }
            for (i, yy) in [(0, -125.0 * k), (1, if sub.is_empty() { 70.0 * k } else { 150.0 * k })] { let mut r = clip(ss, Source::Shape { shape: "rect".into(), width: 260.0 * k, height: 1.5 * k.max(1.0), fill: accent, stroke_width: 0.0, stroke_color: [0.0; 3], sides: 4, corner_radius: 0.0 }, start, dur); r.props.get_mut("y").unwrap().base = yy; let sc = r.props.get_mut("scale").unwrap(); sc.base = 0.0; sc.set_key(0.2 + i as f64 * 0.15, 0.0, Ease::Out); sc.set_key(1.4 + i as f64 * 0.15, 1.0, Ease::Out); fade(&mut r, 0.4, 0.8); clips.push(r); }
        }
        "impact" => {
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("Anton"), 300.0 * k, color, 0.0, "center"), start, dur);
            let mut fx = TextFx { uppercase: true, line_mask: true, line_spacing: 0.98, shadow_blur: 24.0 * k, shadow_opacity: 0.5, shadow_angle: 90.0, shadow_color: [0.0; 3], ..Default::default() };
            let mut r = an("line", "square", 0.0); r.id = "an1".into(); r.y.base = 330.0 * k; sweep(&mut r, 0.1, intro(0.1, 0.1 + 0.8 * (text.matches('\n').count() as f64 + 1.0)), 0.45, Ease::InOut); r.smoothness.base = 0.45;
            fx.animators = vec![r]; c.text_fx = Some(fx); let o = c.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.35f64.min(dur * 0.25), 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); clips.push(c);
            if !sub.is_empty() { let mut s = clip(ss, txt(&sub, font_o.unwrap_or("Montserrat"), 34.0 * k, accent, 0.0, "center"), start, dur); let mut fx = TextFx { weight: 600.0, uppercase: true, ..Default::default() }; let mut t = an("char", "square", 0.0); t.id = "an1".into(); t.tracking.base = 12.0 * k; fx.animators = vec![t]; s.text_fx = Some(fx); s.props.get_mut("y").unwrap().base = 190.0 * k * (text.matches('\n').count() as f64 + 1.0); fade(&mut s, 0.8, 0.4); clips.push(s); }
        }
        "lower_third" => {
            let bw = 900.0 * k; let xoff = -(wpx - bw) / 2.0 + 120.0 * k; let ybase = 300.0 * k;
            let mut bar = clip(ss, Source::Shape { shape: "rect".into(), width: 8.0 * k, height: 130.0 * k, fill: accent, stroke_width: 0.0, stroke_color: [0.0; 3], sides: 4, corner_radius: 0.0 }, start, dur); bar.props.get_mut("x").unwrap().base = -wpx / 2.0 + 112.0 * k; bar.props.get_mut("y").unwrap().base = ybase;
            let sc = bar.props.get_mut("scale").unwrap(); sc.base = 0.0; sc.set_key(0.0, 0.0, Ease::Out); sc.set_key(0.45, 1.0, Ease::Out); sc.set_key(dur - 0.45, 1.0, Ease::In); sc.set_key(dur, 0.0, Ease::In); clips.push(bar);
            let mut n = clip(ss, txt(&text, font_o.unwrap_or("Montserrat"), 72.0 * k, color, 0.0, "left"), start, dur); let mut fx = TextFx { weight: 700.0, box_width: bw, shadow_blur: 10.0 * k, shadow_opacity: 0.45, shadow_angle: 90.0, shadow_color: [0.0; 3], line_mask: true, ..Default::default() };
            let mut r = an("line", "square", 0.0); r.id = "an1".into(); r.y.base = 90.0 * k; r.opacity.base = 0.0; sweep(&mut r, 0.3, 1.1, 0.3, Ease::InOut); fx.animators = vec![r]; n.text_fx = Some(fx); { let p = n.props.get_mut("x").unwrap(); p.base = xoff; } n.props.get_mut("y").unwrap().base = ybase - 28.0 * k;
            let o = n.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.4, 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); clips.push(n);
            if !sub.is_empty() { let mut s = clip(ss, txt(&sub, font_o.unwrap_or("Montserrat"), 32.0 * k, [0.9, 0.9, 0.9], 0.0, "left"), start, dur); let mut fx = TextFx { weight: 400.0, uppercase: true, box_width: bw, line_mask: true, ..Default::default() }; let mut t = an("char", "square", 0.0); t.id = "an1".into(); t.tracking.base = 4.5 * k; let mut r = an("line", "square", 0.0); r.id = "an2".into(); r.y.base = 60.0 * k; r.opacity.base = 0.0; sweep(&mut r, 0.5, 1.3, 0.3, Ease::InOut); fx.animators = vec![t, r]; s.text_fx = Some(fx); s.props.get_mut("x").unwrap().base = xoff; s.props.get_mut("y").unwrap().base = ybase + 32.0 * k; let o = s.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.4, 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); clips.push(s); }
        }
        "kinetic" => {
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("BebasNeue"), 300.0 * k, color, 0.0, "center"), start, dur);
            let mut fx = TextFx { uppercase: true, line_spacing: 0.95, ..Default::default() };
            let mut p = an("word", "square", 0.0); p.id = "an1".into(); p.scale.base = 0.0; p.opacity.base = 0.0; p.y.base = 40.0 * k; p.rotation.base = -6.0; sweep(&mut p, 0.1, intro(0.1, 0.1 + 0.45 * (text.split_whitespace().count() as f64)), 0.2, Ease::InOut); p.smoothness.base = 0.2;
            let mut hl = an("word", "square", 0.0); hl.id = "an2".into(); hl.start.base = 0.0; hl.end.base = 0.2; // accent word highlight is handled by color override below
            fx.animators = vec![p]; fx.glow = 0.0; c.text_fx = Some(fx); let o = c.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.3f64.min(dur * 0.25), 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); let sc = c.props.get_mut("scale").unwrap(); sc.base = 1.0; sc.set_key(0.0, 0.94, Ease::Out); sc.set_key(dur, 1.04, Ease::Out); clips.push(c);
        }
        "elegant" => {
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("PlayfairDisplay-Italic"), 104.0 * k, color, 0.0, "center"), start, dur);
            let mut fx = TextFx { weight: 500.0, line_spacing: 1.22, shadow_blur: 14.0 * k, shadow_opacity: 0.5, shadow_angle: 90.0, shadow_color: [0.0; 3], ..Default::default() };
            let mut w = an("word", "square", 0.0); w.id = "an1".into(); w.opacity.base = 0.0; w.y.base = 34.0 * k; w.blur.base = 12.0; sweep(&mut w, 0.2, intro(0.2, 0.9 + 0.4 * (text.split_whitespace().count() as f64)), 0.5, Ease::InOut); w.smoothness.base = 0.5;
            fx.animators = vec![w]; c.text_fx = Some(fx); let o = c.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.8f64.min(dur * 0.25), 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); clips.push(c);
            if !sub.is_empty() { let mut s = clip(ss, txt(&sub, font_o.unwrap_or("Montserrat"), 26.0 * k, accent, 0.0, "center"), start, dur); let mut fx = TextFx { weight: 500.0, uppercase: true, ..Default::default() }; let mut t = an("char", "square", 0.0); t.id = "an1".into(); t.tracking.base = 10.0 * k; fx.animators = vec![t]; s.text_fx = Some(fx); s.props.get_mut("y").unwrap().base = 130.0 * k * (text.matches('\n').count() as f64 + 1.0); fade(&mut s, 1.6, 0.8); clips.push(s); }
        }
        "neon" => {
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("Syne"), 150.0 * k, [1.0, 0.95, 0.98], 0.0, "center"), start, dur);
            let mut fx = TextFx { weight: 800.0, uppercase: true, glow: 22.0 * k, glow_color: accent, ..Default::default() };
            let mut f = an("char", "square", 0.0); f.id = "an1".into(); f.opacity.base = 0.0; f.shape = "square".into(); let mut offs = 0.0; for i in 0..7 { f.offset.set_key(i as f64 * 0.07, if i % 2 == 0 { 0.0 } else { 1.0 }, Ease::Hold); offs += 1.0; } let _ = offs; f.offset.set_key(0.5, 1.5, Ease::Hold); f.smoothness.base = 0.0; f.start.base = 0.0; f.end.base = 1.0;
            fx.animators = vec![f]; c.text_fx = Some(fx); let o = c.props.get_mut("opacity").unwrap(); o.base = 1.0; o.set_key(dur - 0.3f64.min(dur * 0.25), 1.0, Ease::In); o.set_key(dur, 0.0, Ease::In); clips.push(c);
        }
        "hook_box" => {
            // box first: a track's clips draw in order, so the later text sits on top of it
            let pop = |c: &mut Clip| { let sc = c.props.get_mut("scale").unwrap(); sc.base = 0.0; sc.set_key(0.0, 0.0, Ease::Out); sc.set_key(0.3, 1.0, Ease::OutBack); };
            let box_color = rgb(a, "box_color", [0.05, 0.05, 0.07]);
            let mut r = clip(ss, Source::Shape { shape: "rect".into(), width: wpx * 0.86, height: 170.0 * k, fill: box_color, stroke_width: 0.0, stroke_color: [0.0; 3], sides: 4, corner_radius: 22.0 * k }, start, dur); pop(&mut r); fade(&mut r, 0.15, 0.4); clips.push(r);
            let mut c = clip(ss, txt(&text, font_o.unwrap_or("Anton"), 84.0 * k, color, 0.0, "center"), start, dur); c.text_fx = Some(TextFx { uppercase: true, ..Default::default() }); pop(&mut c); fade(&mut c, 0.15, 0.4); clips.push(c);
        }
        "subscribe_pop" => {
            // V7.7Q: the pill used to be a fixed 230px, tuned for the one word "SUBSCRIBE" -- any longer label
            // (e.g. "SUBSCRIBE FOR MORE", auto_edit's own end-screen default) ran past both of its rounded
            // ends, and the dot sat at a fixed offset that landed on top of the first letter once a longer
            // label pushed the text wider than the gap it was drawn for. Same tight-ink-extent approach as
            // 65809ee's caption pill fix: measure the actual drawn (uppercase, bold) text and size the pill
            // to it, with real side padding; the dot moves OUTSIDE the pill, beside it like a small bell badge,
            // not a fixed px offset that only happened to clear a short word.
            let font_name = font_o.unwrap_or("Montserrat");
            let text_size = 30.0 * k;
            let dot_w = 24.0 * k;
            // 8.5.5: the pill is widened by the dot's own footprint (dot_room) so the dot has a clear lane
            // inside the pill's left edge, left of the text, instead of sitting outside the pill altogether
            // (8.5.4's "small bell badge" read, which on a real render looked like the dot had floated loose).
            let pill_w = pill_width_for_text(&text, font_name, text_size, wpx, k) + dot_room(dot_w, k);
            let pill_h = 66.0 * k;
            let y = 260.0 * k;
            let pop = |c: &mut Clip, sh: f64| { let sc = c.props.get_mut("scale").unwrap(); sc.base = 0.0; sc.set_key(0.0, 0.0, Ease::Out); sc.set_key((0.12 - sh).max(0.0), 1.0, Ease::OutBack); sc.set_key((dur - 0.35 - sh).max(0.13), 1.0, Ease::In); sc.set_key((dur - 0.05 - sh).max(0.14), 0.0, Ease::In); c.props.get_mut("opacity").unwrap().base = 1.0; c.props.get_mut("y").unwrap().base = y; };
            let mut p = clip(ss, Source::Shape { shape: "rect".into(), width: pill_w, height: pill_h, fill: accent, stroke_width: 0.0, stroke_color: [0.0; 3], sides: 4, corner_radius: 33.0 * k }, start, dur); pop(&mut p, 0.0);
            let mut d = clip(ss, Source::Shape { shape: "ellipse".into(), width: dot_w, height: dot_w, fill: [1.0, 1.0, 1.0], stroke_width: 0.0, stroke_color: [0.0; 3], sides: 4, corner_radius: 0.0 }, start, dur); pop(&mut d, 0.04); d.props.get_mut("x").unwrap().base = dot_x_for_pill(pill_w, dot_w, k);
            let mut c = clip(ss, txt(&text, font_name, text_size, [1.0, 1.0, 1.0], 0.0, "center"), start, dur); c.text_fx = Some(TextFx { weight: 700.0, uppercase: true, ..Default::default() }); pop(&mut c, 0.0);
            // 8.5.4: tag the pill with the text clip it backs so a later size fix (lint_edit's
            // text_too_small auto-fix) can grow the pill to match -- the box must track what's
            // actually drawn, not just the size it was built for at this moment.
            p.role = Some(format!("pill_for:{}", c.id));
            // 8.5.4 (second pass): same tag shape for the dot, so that same fix can also carry the
            // dot's clearance past the pill's edge forward once the pill grows -- without this a
            // resized pill's own left edge can move past where the dot was left sitting, putting the
            // dot on top of the (also enlarged) text's first letter (Tab's montage review).
            d.role = Some(format!("dot_for:{}", c.id));
            clips.push(p); clips.push(d); clips.push(c);
        }
        o => return Err(format!("unknown style {o}; styles: {:?}", STYLES)),
    }
    for c in clips.iter_mut() { fit_width(c, wpx)?; }
    // a preset font that is not installed is drawn in a fallback sans, which changes the whole look: say so in the result
    let mut warnings: Vec<String> = vec![];
    for c in &clips { if let Source::Text { font, .. } = &c.source { let (file, sub) = crate::text::resolve_font_info(font); let w = format!("font_substituted: '{font}' was not found, so this title is drawn with {file}. The preset fonts are OFL: run bash fonts/fetch.sh in the repo, or set KIMO_FONTS to a folder holding them (list_fonts shows what is installed)"); if sub && !warnings.contains(&w) { warnings.push(w); } } }
    let ids: Vec<String> = clips.iter().map(|c| c.id.clone()).collect(); let t = ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or("no such track")?; t.clips.extend(clips.into_iter().map(std::sync::Arc::new));
    let mut r = json!({"clip_ids": ids, "style": style}); if !warnings.is_empty() { r["warnings"] = json!(warnings); }
    Ok(r)
}
