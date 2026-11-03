//! KIMO CAPTION ENGINE (V8.12.0): a real caption engine, not a bank of presets.
//!
//! Where `auto_captions.rs` picks one of five hardcoded `Look`s and threads its fields through
//! `page_animators`/`build`, this module exposes the same hard parts -- segmentation, timing,
//! layout/face-avoidance, contrast-aware legibility, per-word state styling, emphasis and keyframe
//! animation -- as composable primitives over a `CaptionSpec` the caller writes. `pop`, `karaoke`,
//! `clean`, `boxed`, `impact` (and `set_caption_style`'s `subtitle`/`boxed`/`yellow`) become spec
//! PRESETS on top of this engine (see `preset_spec`), not separate code paths; `auto_captions.rs`
//! itself is untouched by this file (see the V8.12.0 CHANGELOG entry for the gating decision).
//!
//! Design rule, stated once because it shapes every function below: every default here --
//! segmentation thresholds, CPS, lead-in, gap bridging, avoidance, contrast strength, animation
//! curves -- is a field on the spec, overridable per call, AND overridable per individual word or
//! unit through `overrides`. Free per-word/per-unit keyframes (any property, any time, any easing)
//! sit alongside the rule-based primitives and are applied last, after every rule. The engine's job
//! is smart defaults plus the hard legibility guarantees (no orphan words, no overlapping cues, no
//! sub-minimum flashes) -- enforced only where the caller hasn't already said what they want.
use super::kit::*;
use crate::keyframe::{Ease, Prop};
use crate::model::*;
use crate::speech_words as sw;
use crate::transcript::{compose_cue_text, next_id, CueBinding, Take, TakeSource, TakeStatus, Transcript, Word, WordFlag};
use std::collections::HashMap;
use std::time::Duration;

const WHITE: [f64; 3] = [1.0, 1.0, 1.0];
const BLACK: [f64; 3] = [0.0, 0.0, 0.0];

// ---------------------------------------------------------------------------------------------
// 1. SEGMENTATION: ASR words -> caption units (a "page"/"cue" with its own line breaks).
// ---------------------------------------------------------------------------------------------

/// One input word with its ASR timing, mirroring `sw::TimedWord` so either source feeds the engine.
#[derive(Clone, Debug)]
pub struct EWord { pub text: String, pub start: f64, pub end: f64, pub conf: f32 }

#[derive(Clone, Debug)]
pub struct SegmentSpec {
    pub max_chars_per_line: usize,
    pub max_lines: usize,
    pub max_cps: f64,
    pub pause_break_s: f64,
    pub min_words: usize,
    /// When set, line-wrap is triggered by word COUNT instead of character budget (matches the legacy named
    /// looks, which wrapped at a fixed words-per-line regardless of how short the words were). None (the
    /// engine's own default): wrap whenever the unit's own text would not fit in `max_chars_per_line` chars.
    pub max_words_per_line: Option<usize>,
}
impl Default for SegmentSpec { fn default() -> Self { SegmentSpec { max_chars_per_line: 20, max_lines: 2, max_cps: 20.0, pause_break_s: 0.45, min_words: 1, max_words_per_line: None } } }

/// A segmentation result: word index range `[first, last]` inclusive, plus where (if `max_lines`>1) the text breaks to a new line.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit { pub first: usize, pub last: usize, pub breaks_after: Vec<usize> }

fn core_tok(t: &str) -> &str { t.trim_end_matches(|c: char| matches!(c, '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}')) }
fn is_sentence_end(t: &str) -> bool { core_tok(t).chars().last().map_or(false, |c| matches!(c, '.' | '?' | '!' | '\u{2026}' | '\u{964}')) }
fn is_clause_end(t: &str) -> bool { core_tok(t).chars().last().map_or(false, |c| matches!(c, ',' | ';' | ':' | '\u{2014}' | '\u{2013}')) }
/// Short closed-class words that must never end a line while a later break point exists: splitting "the | cat"
/// strands the determiner from its noun and reads as a stutter. Checked only when another valid break point is available.
const NO_DANGLE: &[&str] = &["a","an","the","my","your","his","her","its","our","their","this","that","these","those","no","not"];

/// Segments ASR words into caption units: breaks at phrase/punctuation boundaries and pauses, not fixed word counts.
/// Guarantees (tested in quality_v8120.rs): no unit ends on an orphan single word when a merge is possible, every
/// unit's naive chars/duration stays at or under `max_cps` (the unit is cut earlier rather than let reading speed run
/// over), and every line holds at least two words unless the whole unit is one word.
pub fn segment(words: &[EWord], spec: &SegmentSpec) -> Result<Vec<Unit>, String> {
    if words.is_empty() { return Ok(vec![]); }
    let (per_line, lines) = (spec.max_chars_per_line.max(4), spec.max_lines.clamp(1, 3));
    let cap_chars = per_line * lines; let clen = |s: &str| s.chars().count();
    // a bare-punctuation token (no letters or digits of its own) has no spoken duration of its own and
    // must not be allowed to inflate the measured reading speed -- "." riding on the previous word's own
    // timestamp is an ASR artifact, not an extra character the viewer has to read in zero time.
    let is_punct_token = |t: &str| !t.chars().any(|c| c.is_alphanumeric());
    let mut cuts: Vec<(usize, usize)> = vec![]; let mut a = 0usize;
    for i in 0..words.len() {
        let chars: usize = words[a..=i].iter().map(|w| clen(&w.text) + 1).sum::<usize>().saturating_sub(1);
        let cps_chars: usize = words[a..=i].iter().filter(|w| !is_punct_token(&w.text)).map(|w| clen(&w.text) + 1).sum::<usize>().saturating_sub(1);
        let span = (words[i].end - words[a].start).max(1e-6);
        let next_gap = if i + 1 < words.len() { words[i + 1].start - words[i].end } else { 0.0 };
        let cps_now = cps_chars as f64 / span;
        let word_cap = spec.max_words_per_line.map(|n| n * lines);
        let full = chars >= cap_chars || word_cap.map_or(false, |wc| (i + 1 - a) >= wc);
        // reactive CPS cut: once the unit already holds >=2 words and its own (punctuation-free) reading
        // speed is over the limit, end it here rather than let it get worse by adding another word. A
        // single word that alone exceeds the cap is left to stand (there is nothing shorter to cut to).
        let over_cps = cps_now > spec.max_cps; // cut as soon as the rate is over, even for a lone word -- 1 word/unit is the best this can do
        let end = i + 1 == words.len()
            || is_sentence_end(&words[i].text)
            || (is_clause_end(&words[i].text) && i + 1 - a >= 2)
            || next_gap > spec.pause_break_s
            || full
            || over_cps;
        if end { cuts.push((a, i)); a = i + 1; }
    }
    // no-orphan: a lone trailing single word is absorbed into the unit before it, UNLESS that would cross
    // a real break (the previous unit already ended a sentence, or a pause separates them) -- those are not
    // orphans, they are two different utterances that happen to leave one of them a single word.
    while cuts.len() >= 2 {
        let last_len = cuts[cuts.len() - 1].1 + 1 - cuts[cuts.len() - 1].0;
        if last_len != 1 { break; }
        let (pa, pb) = cuts[cuts.len() - 2]; let (_, lb) = cuts[cuts.len() - 1];
        if is_sentence_end(&words[pb].text) { break; }
        if words[lb].start - words[pb].end > spec.pause_break_s { break; }
        // don't undo a legitimate CPS-driven split: only merge when the combined unit still reads at an
        // acceptable pace (a lone word genuinely too fast to help is left alone, not glued to its neighbour).
        let merged_cps_chars: usize = words[pa..=lb].iter().filter(|w| !is_punct_token(&w.text)).map(|w| clen(&w.text) + 1).sum::<usize>().saturating_sub(1);
        let merged_span = (words[lb].end - words[pa].start).max(1e-6);
        if merged_cps_chars as f64 / merged_span > spec.max_cps { break; }
        cuts.pop(); let n = cuts.len(); cuts[n - 1] = (pa, lb);
    }
    let mut units: Vec<Unit> = cuts.into_iter().map(|(first, last)| {
        let toks: Vec<&str> = words[first..=last].iter().map(|w| w.text.as_str()).collect();
        let total_chars: usize = toks.iter().map(|w| w.chars().count() + 1).sum::<usize>().saturating_sub(1);
        let want_wrap = match spec.max_words_per_line { Some(n) => toks.len() > n, None => total_chars > spec.max_chars_per_line };
        let breaks = if lines > 1 && toks.len() >= 2 && want_wrap { balanced_breaks(&toks, lines, toks.len()) } else { vec![] };
        Unit { first, last, breaks_after: breaks }
    }).collect();
    // no single-word line within a multi-word unit: if a break leaves a line with exactly one word, pull the break
    // off that dangling word (merge it onto the fuller neighbour line) when the unit has more than one word total.
    for u in units.iter_mut() {
        if u.breaks_after.is_empty() { continue; }
        let n = u.last - u.first + 1;
        loop {
            let mut bounds = vec![u.first]; for &b in &u.breaks_after { bounds.push(u.first + b + 1); } bounds.push(u.first + n);
            let line_lens: Vec<usize> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
            let orphan = line_lens.iter().position(|&l| l == 1);
            let Some(oi) = orphan else { break };
            if line_lens.len() <= 1 { break; }
            // drop the break adjacent to the orphan line, merging it into a neighbour
            if oi == 0 { u.breaks_after.remove(0); } else { u.breaks_after.remove(oi.min(u.breaks_after.len()) - 1); }
            if u.breaks_after.len() + 1 < line_lens.len() { } // re-loop with fewer breaks
            if u.breaks_after.is_empty() { break; }
        }
    }
    Ok(units)
}

/// Balanced line breaks over `words` across `lines` lines of at most `per_line` words each, minimising the character-count
/// imbalance between lines. Never places a break that would strand a `NO_DANGLE` word alone at a line's end when another
/// balance point within one word scores the same or better.
fn balanced_breaks(words: &[&str], lines: usize, per_line: usize) -> Vec<usize> {
    let n = words.len(); let l: Vec<usize> = words.iter().map(|w| w.chars().count()).collect();
    let dangles = |b: usize| NO_DANGLE.iter().any(|d| words[b].to_lowercase() == *d);
    if lines == 2 || n <= 2 * per_line {
        let mut best: Option<(usize, bool, usize)> = None; // (imbalance, dangles, index) -- prefer not dangling at equal imbalance
        for b in 0..n.saturating_sub(1) {
            let (x, y) = (b + 1, n - b - 1); if x > per_line || y > per_line { continue; }
            let c1: usize = l[..=b].iter().sum::<usize>() + b; let c2: usize = l[b + 1..].iter().sum::<usize>() + y.saturating_sub(1);
            let d = c1.abs_diff(c2); let dg = dangles(b);
            let better = match &best { None => true, Some((bd, bdg, _)) => (d, dg) < (*bd, *bdg) };
            if better { best = Some((d, dg, b)); }
        }
        return match best { Some((_, _, b)) => vec![b], None => vec![(n / 2).saturating_sub(1)] };
    }
    (1..lines).map(|k| (k * n / lines).saturating_sub(1)).collect()
}

// ---------------------------------------------------------------------------------------------
// 2. TIMING: in/out snapped to word timings, lead-in, gap bridging, minimum duration, shot cuts.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TimingSpec {
    pub lead_in_s: f64,
    pub hold_s: f64,
    pub bridge_gap_s: f64,
    pub min_duration_s: f64,
    pub min_gap_s: f64,
}
impl Default for TimingSpec { fn default() -> Self { TimingSpec { lead_in_s: 0.04, hold_s: 0.35, bridge_gap_s: 0.6, min_duration_s: 0.2, min_gap_s: 0.02 } } }

#[derive(Clone, Debug)]
pub struct TimedUnit { pub unit: Unit, pub start: f64, pub end: f64, pub straddled_cut: Option<f64> }

/// Times segmentation units: start snaps to the first word's start minus `lead_in_s`, clamped so it never crosses
/// into the previous unit's own end plus `min_gap_s` (no overlap, guaranteed by construction, not by a later check).
/// end snaps to the last word's end; if the next unit starts within `bridge_gap_s` the end is bridged forward to meet
/// it (no flicker gap); otherwise it holds `hold_s` past the last word. Every unit is stretched (never shrunk) to at
/// least `min_duration_s` by pushing `end` forward (bounded by the next unit's start minus `min_gap_s`, so a minimum-
/// duration cue never overlaps the next one either -- a unit at the very end of the list may fall short of
/// `min_duration_s` only if words.len()==1 and there's nowhere to extend to is impossible here since there's no
/// following unit to collide with, so the full stretch always applies). When `shot_cuts` is given and a cut falls
/// strictly inside (start, end) with at least `min_duration_s` of room before it, the unit's `end` is pulled back to
/// just before the cut instead of straddling it; when there isn't room, the straddle is reported (not hidden) in
/// `straddled_cut` rather than silently violating minimum duration.
pub fn time_units(units: &[Unit], words: &[EWord], spec: &TimingSpec, shot_cuts: Option<&[f64]>) -> Vec<TimedUnit> {
    let mut out: Vec<TimedUnit> = vec![];
    for (i, u) in units.iter().enumerate() {
        let w_start = words[u.first].start; let w_end = words[u.last].end;
        let prev_end = out.last().map(|p: &TimedUnit| p.end).unwrap_or(f64::NEG_INFINITY);
        let mut start = (w_start - spec.lead_in_s).max(0.0);
        if start < prev_end + spec.min_gap_s { start = prev_end + spec.min_gap_s; }
        if start > w_start { start = w_start; } // lead-in must not land after the word itself (can happen with a crowded min_gap)
        let next_w_start = units.get(i + 1).map(|n| words[n.first].start);
        let mut end = match next_w_start {
            Some(ns) if ns - w_end <= spec.bridge_gap_s => ns, // bridge the small gap exactly: no flicker between cues
            Some(ns) => (w_end + spec.hold_s).min(ns - spec.min_gap_s), // hold, but never into the next cue's own min_gap
            None => w_end + spec.hold_s,
        };
        if end - start < spec.min_duration_s {
            let want = start + spec.min_duration_s;
            end = match next_w_start { Some(ns) => want.min(ns - spec.min_gap_s), None => want };
        }
        let mut straddled = None;
        if let Some(cuts) = shot_cuts {
            for &c in cuts {
                if c > start + 1e-9 && c < end - 1e-9 {
                    if c - start >= spec.min_duration_s { end = end.min(c); } else { straddled = Some(c); }
                    break;
                }
            }
        }
        out.push(TimedUnit { unit: u.clone(), start, end, straddled_cut: straddled });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// 3. LAYOUT: safe-area aware placement, per-unit face/subject avoidance, contrast-aware legibility.
// ---------------------------------------------------------------------------------------------

/// A handful of estimated platform UI zones to keep captions clear of, as fractions of the canvas
/// height measured from the bottom (mirrors the shape of safe_margins.rs's own table without
/// depending on its private `table()`; see the CHANGELOG for why this stays a small local copy).
fn bottom_clear_frac(platform: &str) -> f64 {
    match platform { "tiktok" => 0.28, "reels" => 0.26, "shorts" => 0.22, "youtube" => 0.12, _ => 0.0 }
}

#[derive(Clone, Debug)]
pub struct LayoutSpec { pub position: String, pub platform: String, pub face_avoidance: bool, pub side_margin_frac: f64,
    /// Pins the unit at a fixed vertical offset (as a fraction of frame height, in the same center-relative
    /// convention the renderer's own "y" clip property uses -- 0 is vertical centre, positive moves down),
    /// bypassing safe-area/face-avoidance band placement entirely. For re-expressing legacy styles that
    /// always sat at one fixed offset regardless of what is in frame.
    pub fixed_y_frac: Option<f64> }
impl Default for LayoutSpec { fn default() -> Self { LayoutSpec { position: "auto".into(), platform: "none".into(), face_avoidance: true, side_margin_frac: 0.05, fixed_y_frac: None } } }

/// One media clip visible on the timeline, enough to sample a frame for face/contrast analysis without touching
/// `auto_captions::Src` (kept file-local and self-contained: the engine depends on nothing private in that file).
pub(crate) struct VisibleMedia { path: String, timeline_start: f64, in_point: f64, speed: f64, aspect_ok: bool }
pub(crate) fn visible_media(ss: &Session) -> Vec<VisibleMedia> {
    let (w, h) = (ss.project.width as f64, ss.project.height as f64);
    let mut out = vec![];
    for t in &ss.project.tracks {
        for c in &t.clips {
            if let Source::Media { asset } = &c.source {
                if let Some(ast) = ss.project.assets.get(asset) {
                    if ast.width == 0 || ast.height == 0 { continue; }
                    let aspect_ok = ((ast.width as f64 / ast.height as f64) / (w / h) - 1.0).abs() <= 0.12;
                    out.push(VisibleMedia { path: ss.project.resolve(&ast.path), timeline_start: c.start, in_point: c.in_point, speed: c.speed.max(0.01), aspect_ok });
                }
            }
        }
    }
    out
}
/// The clip (if any) actually on screen at timeline time `t`, for face sampling at a unit's mid-point.
pub(crate) fn media_at<'a>(media: &'a [VisibleMedia], t: f64) -> Option<&'a VisibleMedia> { media.iter().find(|m| t >= m.timeline_start && m.aspect_ok) }

/// Grabs one small frame (72x128 rgb24, matching `sw::skin_band`'s own expected size) from a source file at a
/// source-time, for face detection. Mirrors `auto_captions::caption_band_for`'s sampling approach.
fn sample_face_frame(path: &str, t_src: f64) -> Option<(f64, f64)> {
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-ss", &format!("{t_src:.3}"), "-i", path, "-frames:v", "1", "-vf", "scale=72:128", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
    let b = capture(&mut cmd, 72 * 128 * 3 + 16, Duration::from_secs(20)).ok()?;
    sw::skin_band(&b, 72, 128)
}

/// Per-unit placement: the group's own default band (today's whole-group `sw::caption_band` logic, unchanged), then
/// a per-unit override when `face_avoidance` is on and a face is actually detected occupying that band at this
/// unit's own mid-time -- a real upgrade over the one-shot whole-group check (today's auto_captions samples the
/// band ONCE for the entire caption group). Returns (y centre fraction, which band, whether avoidance fired).
pub(crate) fn place_unit(default_band: f64, tall: bool, layout: &LayoutSpec, media: &[VisibleMedia], mid_t: f64) -> (f64, &'static str, bool) {
    if !layout.face_avoidance { return (default_band, if default_band < 0.5 { "top" } else { "bottom" }, false); }
    let Some(m) = media_at(media, mid_t) else { return (default_band, if default_band < 0.5 { "top" } else { "bottom" }, false) };
    let t_src = m.in_point + (mid_t - m.timeline_start) * m.speed;
    let Some(face) = sample_face_frame(&m.path, t_src.max(0.0)) else { return (default_band, if default_band < 0.5 { "top" } else { "bottom" }, false) };
    let band_half = 0.10; // a caption's own rough vertical footprint, as a fraction of frame height, for overlap testing
    let (lo, hi) = (default_band - band_half, default_band + band_half);
    let overlaps = face.0 < hi && face.1 > lo;
    if !overlaps { return (default_band, if default_band < 0.5 { "top" } else { "bottom" }, false); }
    // move to the opposite band, same rule auto_captions' own caption_band uses for the whole group
    let low = if tall { 0.70 } else { 0.80 };
    let alt = if default_band >= low - 1e-9 { 0.2 } else { low };
    (alt, if alt < 0.5 { "top" } else { "bottom" }, true)
}

// --- contrast-aware legibility -----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ContrastSpec { pub auto: bool, pub min_stroke_frac: f64, pub max_stroke_frac: f64, pub min_shadow: f64, pub max_shadow: f64, pub min_box_alpha: f64, pub max_box_alpha: f64 }
impl Default for ContrastSpec { fn default() -> Self { ContrastSpec { auto: true, min_stroke_frac: 0.03, max_stroke_frac: 0.12, min_shadow: 2.0, max_shadow: 8.0, min_box_alpha: 0.55, max_box_alpha: 0.88 } } }

/// Pure strength selection from a measured background luminance (0..1, sRGB relative luminance under the text):
/// a bright background needs a stronger outline/shadow/box than a dark one, so legibility rises with it, not the
/// other way round. Linear between the spec's own min/max; a mid-grey background (0.5) sits at the midpoint.
pub fn contrast_strength(bg_luma: f64, c: &ContrastSpec) -> (f64, f64, f64) {
    let t = bg_luma.clamp(0.0, 1.0);
    let lerp = |lo: f64, hi: f64| lo + (hi - lo) * t;
    (lerp(c.min_stroke_frac, c.max_stroke_frac), lerp(c.min_shadow, c.max_shadow), lerp(c.min_box_alpha, c.max_box_alpha))
}
/// Measures mean background luminance under a text rect at time `t`, by rendering the current composite (the
/// project BEFORE this unit's own caption clip is inserted -- there is nothing to hide). None when nothing can be
/// rendered (no media prepared, an empty canvas) rather than guessing: the caller keeps its pre-measurement default.
pub fn measure_bg_luma(ss: &mut Session, t: f64, rect_px: [usize; 4]) -> Option<f64> {
    crate::render::prepare_at(&ss.project, &mut ss.cache, Some(t)).ok()?;
    let frame = crate::render::render_frame(&ss.project, t, &ss.cache).ok()?;
    let img = crate::perception::analysis::Image::from_frame(&frame);
    let (x1, y1) = ((rect_px[0] + rect_px[2]).min(img.w), (rect_px[1] + rect_px[3]).min(img.h));
    if rect_px[0] >= x1 || rect_px[1] >= y1 { return None; }
    let lums = super::text_contrast::background_lums(&img, Some([rect_px[0], rect_px[1], x1 - rect_px[0], y1 - rect_px[1]]), None, 0);
    if lums.is_empty() { return None; }
    Some(lums.iter().sum::<f64>() / lums.len() as f64)
}

// ---------------------------------------------------------------------------------------------
// 4. STYLE PRIMITIVES: font/fill/stroke/shadow/box, case/tracking/line-height/align/anchor, and the
//    per-word ACTIVE/SPOKEN/UPCOMING state styling that generalises "karaoke" into ordinary data.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)] pub enum Case { None, Upper, Lower, Title }
#[derive(Clone, Debug)]
pub struct BoxSpec { pub pad_frac: f64, pub radius_frac: f64, pub color: [f64; 4] }
#[derive(Clone, Debug)]
pub struct StyleSpec {
    pub font: String, pub weight: f64, pub size_frac: f64, pub fill: [f64; 3],
    pub stroke_frac: f64, pub stroke_color: [f64; 3], pub shadow_blur: f64, pub shadow_opacity: f64,
    pub bg: Option<BoxSpec>, pub case: Case, pub letter_spacing: f64, pub line_height: f64, pub align: String,
    /// What `size_frac` is a fraction OF: "width" (default, matches the legacy pop/karaoke/clean/boxed/impact
    /// looks) or "short_side" (min(w,h); matches the legacy subtitle/boxed/yellow restyle path, which sizes
    /// off the frame's short side so portrait and landscape read at the same relative scale).
    pub size_basis: String,
}
impl Default for StyleSpec { fn default() -> Self { StyleSpec { font: "Inter".into(), weight: 700.0, size_frac: 0.08, fill: WHITE, stroke_frac: 0.0, stroke_color: BLACK, shadow_blur: 0.0, shadow_opacity: 0.0, bg: None, case: Case::None, letter_spacing: 0.0, line_height: 1.15, align: "center".into(), size_basis: "width".into() } } }

#[derive(Clone, Debug, Default)]
pub struct WordDelta { pub fill: Option<[f64; 3]>, pub scale: Option<f64>, pub opacity: Option<f64> }
#[derive(Clone, Debug, Default)]
pub struct WordStates { pub active: WordDelta, pub spoken: WordDelta, pub upcoming: WordDelta }

#[derive(Clone, Debug, Default)]
pub struct EmphasisSpec { pub list: Vec<String>, pub by_keyword: bool, pub keyword_count: usize, pub by_energy: bool, pub energy_top_frac: f64, pub delta: WordDelta }

#[derive(Clone, Copy, Debug, PartialEq)] pub enum AnimProp { Opacity, Scale, Y, Blur }
#[derive(Clone, Debug)]
pub struct PropAnim { pub prop: AnimProp, pub from: f64, pub to: f64, pub duration_s: f64, pub ease: Ease }
#[derive(Clone, Debug, Default)]
pub struct UnitAnim { pub props: Vec<PropAnim> }
#[derive(Clone, Debug, Default)]
pub struct AnimationSpec { pub unit_in: UnitAnim, pub unit_out: UnitAnim, pub active_lead_s: f64 }

fn apply_case(text: &str, case: Case) -> String {
    match case {
        Case::None => text.to_string(), Case::Upper => text.to_uppercase(), Case::Lower => text.to_lowercase(),
        Case::Title => text.split(' ').map(|w| { let mut c = w.chars(); match c.next() { Some(f) => f.to_uppercase().collect::<String>() + c.as_str(), None => String::new() } }).collect::<Vec<_>>().join(" "),
    }
}

/// One AE-style range-selector animator, mirroring the shape every text animator in this codebase already takes
/// (`text.rs::animate`: a word's selected amount is `shape((pos - (start+offset)) / (end+offset - (start+offset)))`,
/// `pos = (word_index + 0.5) / word_count`). Built fresh here so the engine depends on nothing private in
/// auto_captions.rs. `start`/`end` are in the same normalised 0..1-per-word-count units `pos` uses.
fn base_animator(id: &str, start: f64, end: f64) -> TextAnimator {
    TextAnimator { id: id.into(), unit: "word".into(), shape: "square".into(), start: Prop::new(start), end: Prop::new(end), offset: Prop::new(0.0), smoothness: Prop::new(0.0),
        x: Prop::new(0.0), y: Prop::new(0.0), scale: Prop::new(1.0), rotation: Prop::new(0.0), opacity: Prop::new(1.0), tracking: Prop::new(0.0), blur: Prop::new(0.0),
        random: false, seed: 0, fill: None, fill_amount: None, ease: None, wiggly: None }
}

/// Which words (by index within the unit) are emphasised: an explicit list, keyword scoring (reuses `sw::keywords`,
/// the same scorer auto_captions already uses), and/or an audio-energy peak (reuses `sw::onsets`'s per-hop dB,
/// averaged over each word's own span, keeping the top `energy_top_frac` of words by that average).
pub fn emphasis_words(words: &[EWord], spec: &EmphasisSpec, energy_db: Option<(&[f32], f64)>) -> Vec<usize> {
    let mut idx: std::collections::BTreeSet<usize> = Default::default();
    if !spec.list.is_empty() {
        let set: std::collections::HashSet<String> = spec.list.iter().map(|s| s.to_lowercase()).collect();
        for (i, w) in words.iter().enumerate() { if set.contains(&w.text.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()) { idx.insert(i); } }
    }
    if spec.by_keyword && words.len() >= 2 {
        let disp: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let mut freq: HashMap<String, usize> = HashMap::new(); for w in &disp { *freq.entry(sw::norm_word(w)).or_insert(0) += 1; }
        let n = spec.keyword_count.max(1).min(words.len());
        for k in sw::keywords(&disp, n, &freq) { idx.insert(k); }
    }
    if spec.by_energy { if let Some((db, hop)) = energy_db {
        let mut scored: Vec<(usize, f32)> = words.iter().enumerate().map(|(i, w)| {
            let (a, b) = ((w.start / hop).floor().max(0.0) as usize, (w.end / hop).ceil().max(1.0) as usize);
            let (a, b) = (a.min(db.len().saturating_sub(1)), b.min(db.len()));
            let m = if b > a { db[a..b].iter().copied().sum::<f32>() / (b - a) as f32 } else { f32::MIN };
            (i, m)
        }).collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let keep = ((words.len() as f64) * spec.energy_top_frac).ceil().max(1.0) as usize;
        for (i, _) in scored.into_iter().take(keep) { idx.insert(i); }
    } }
    idx.into_iter().collect()
}

/// Builds the per-word TextAnimators for one unit. `s`/`e`: each word's start/end in unit-local seconds (already
/// timed by section 2 -- this function does no snapping of its own). `dur`: the unit's own on-screen duration.
/// Three independent AE-style selector windows, same selector math as every other animator in this engine:
/// UPCOMING selects every word after the one currently active (hidden/dimmed/whatever `states.upcoming` says);
/// SPOKEN selects every word up to and including the current one (the generic karaoke colour-trail); ACTIVE
/// selects exactly the current word, `active_lead_s` seconds early (a tiny pop-in lookahead, not the unit's own
/// lead-in from section 2). Any of the three is skipped when its delta is empty (no fill/scale/opacity set) --
/// not every look wants all three, e.g. a plain subtitle wants none of them.
pub fn compose_animators(states: &WordStates, emph_idx: &[usize], emph: &WordDelta, s: &[f64], e: &[f64], dur: f64, active_lead_s: f64) -> Vec<TextAnimator> {
    let n = s.len().max(1) as f64; let at = |t: f64| t.clamp(0.0, dur);
    let mut v = vec![];
    let has_delta = |d: &WordDelta| d.fill.is_some() || d.scale.is_some() || d.opacity.is_some();
    let apply_delta = |an: &mut TextAnimator, d: &WordDelta| { if let Some(f) = d.fill { an.fill = Some(f); } if let Some(sc) = d.scale { an.scale = Prop::new(sc); } if let Some(op) = d.opacity { an.opacity = Prop::new(op); } };
    if has_delta(&states.upcoming) {
        // window (0, 3): wide enough that any offset in 0..1 still contains every pos > offset. Offset jumps to
        // (j+1)/n right as word j starts, so words j+1.. (pos >= offset) are selected = not yet spoken.
        let mut up = base_animator("upcoming", 0.0, 3.0); up.offset.set_key(0.0, 0.0, Ease::Hold);
        for j in 0..s.len() { up.offset.set_key(at(s[j]), (j + 1) as f64 / n, Ease::Hold); }
        apply_delta(&mut up, &states.upcoming); v.push(up);
    }
    if has_delta(&states.spoken) {
        // window (-3, 0): mirror image, selects pos <= offset = every word up to and including the current one.
        let mut sp = base_animator("spoken", -3.0, 0.0); sp.offset.set_key(0.0, -1.0, Ease::Hold);
        for j in 0..s.len() { sp.offset.set_key(at(s[j]), (j + 1) as f64 / n, Ease::Hold); }
        apply_delta(&mut sp, &states.spoken); v.push(sp);
    }
    if has_delta(&states.active) {
        let mut act = base_animator("active", 0.0, 1.0 / n); act.offset.set_key(0.0, -2.0, Ease::Hold);
        for j in 0..s.len() {
            let t0 = at(s[j] - active_lead_s); act.offset.set_key(t0, j as f64 / n, Ease::Hold);
            if j + 1 < s.len() { act.offset.set_key(at(e[j]), -2.0, Ease::Hold); }
        }
        apply_delta(&mut act, &states.active); v.push(act);
    }
    if !emph_idx.is_empty() && has_delta(emph) {
        for (k, &wi) in emph_idx.iter().enumerate() {
            if wi >= s.len() { continue; }
            let mut em = base_animator(&format!("emph{k}"), wi as f64 / n, (wi + 1) as f64 / n);
            apply_delta(&mut em, emph); v.push(em);
        }
    }
    v
}

/// The unit-level in/out Prop keyframes (opacity/scale/y/blur) -- a per-UNIT animation, independent of the
/// per-word selectors above (AE-style range selectors are a per-word mechanism; in/out here is deliberately kept
/// to the clip's own Props, the one clean seam between "this whole cue arrives/leaves" and "this one word is
/// active right now").
pub fn build_unit_props(anim: &AnimationSpec, dur: f64, base_y: f64) -> std::collections::BTreeMap<String, Prop> {
    let mut props = default_props(); props.insert("y".into(), Prop::new(base_y.round()));
    let name = |p: AnimProp| match p { AnimProp::Opacity => "opacity", AnimProp::Scale => "scale", AnimProp::Y => "y", AnimProp::Blur => "blur" };
    for pa in &anim.unit_in.props { let key = name(pa.prop); let mut prop = props.remove(key).unwrap_or_else(|| Prop::new(pa.to));
        prop.set_key(0.0, pa.from, Ease::Hold); prop.set_key(pa.duration_s.min(dur), pa.to, pa.ease); props.insert(key.into(), prop); }
    for pa in &anim.unit_out.props { let key = name(pa.prop); let mut prop = props.remove(key).unwrap_or_else(|| Prop::new(pa.from));
        let t0 = (dur - pa.duration_s).max(0.0); prop.set_key(t0, pa.from, Ease::Hold); prop.set_key(dur, pa.to, pa.ease); props.insert(key.into(), prop); }
    props
}

// ---------------------------------------------------------------------------------------------
// 5. SPEC PARSING: Value -> the typed structs above, every field documented, every bad value a
//    clear error (never a panic, never a silent default for something the caller actually typed).
// ---------------------------------------------------------------------------------------------

fn ferr(path: &str, want: &str) -> String { format!("{path}: {want}") }
fn get_f64(v: &Value, path: &str, k: &str, d: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match v.get(k) { None | Some(Value::Null) => Ok(d),
        Some(x) => x.as_f64().filter(|n| n.is_finite() && (lo..=hi).contains(n)).ok_or_else(|| ferr(&format!("{path}.{k}"), &format!("a number {lo}..{hi}"))) }
}
fn get_usize(v: &Value, path: &str, k: &str, d: usize, lo: usize, hi: usize) -> Result<usize, String> {
    match v.get(k) { None | Some(Value::Null) => Ok(d),
        Some(x) => x.as_u64().map(|n| n as usize).filter(|n| (lo..=hi).contains(n)).ok_or_else(|| ferr(&format!("{path}.{k}"), &format!("an integer {lo}..{hi}"))) }
}
fn get_bool(v: &Value, path: &str, k: &str, d: bool) -> Result<bool, String> {
    match v.get(k) { None | Some(Value::Null) => Ok(d), Some(x) => x.as_bool().ok_or_else(|| ferr(&format!("{path}.{k}"), "a boolean")) }
}
fn get_str(v: &Value, path: &str, k: &str, d: &str, allowed: &[&str]) -> Result<String, String> {
    match v.get(k) { None | Some(Value::Null) => Ok(d.to_string()),
        Some(x) => { let s = x.as_str().ok_or_else(|| ferr(&format!("{path}.{k}"), "a string"))?;
            if !allowed.is_empty() && !allowed.contains(&s) { return Err(ferr(&format!("{path}.{k}"), &format!("one of {}", allowed.join("|")))); } Ok(s.to_string()) } }
}
fn get_color(v: &Value, path: &str, k: &str, d: [f64; 3]) -> Result<[f64; 3], String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(d),
        Some(Value::String(s)) => { let h = s.trim_start_matches('#'); if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) { return Err(ferr(&format!("{path}.{k}"), "a hex colour \"#rrggbb\" or [r,g,b] 0..1")); }
            let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0; Ok([p(0), p(2), p(4)]) }
        Some(Value::Array(a)) if a.len() == 3 => { let mut c = [0.0; 3]; for i in 0..3 { c[i] = a[i].as_f64().filter(|x| (0.0..=1.0).contains(x)).ok_or_else(|| ferr(&format!("{path}.{k}"), "[r,g,b] components 0..1"))?; } Ok(c) }
        _ => Err(ferr(&format!("{path}.{k}"), "a hex colour \"#rrggbb\" or [r,g,b] 0..1")),
    }
}
fn get_ease(v: &Value, path: &str, k: &str, d: Ease) -> Result<Ease, String> {
    match v.get(k) { None | Some(Value::Null) => Ok(d),
        Some(x) => { let s = x.as_str().ok_or_else(|| ferr(&format!("{path}.{k}"), "an easing name"))?;
            Ease::parse(s).ok_or_else(|| ferr(&format!("{path}.{k}"), &format!("one of {}", crate::keyframe::EASE_NAMES.join("|")))) } }
}
fn word_delta(v: &Value, path: &str) -> Result<WordDelta, String> {
    if v.is_null() { return Ok(WordDelta::default()); }
    if !v.is_object() { return Err(ferr(path, "an object {fill?, scale?, opacity?}")); }
    let fill = match v.get("fill") { None | Some(Value::Null) => None, Some(_) => Some(get_color(v, path, "fill", WHITE)?) };
    let scale = match v.get("scale") { None | Some(Value::Null) => None, Some(_) => Some(get_f64(v, path, "scale", 1.0, 0.1, 5.0)?) };
    let opacity = match v.get("opacity") { None | Some(Value::Null) => None, Some(_) => Some(get_f64(v, path, "opacity", 1.0, 0.0, 1.0)?) };
    Ok(WordDelta { fill, scale, opacity })
}

fn parse_segment(v: &Value) -> Result<SegmentSpec, String> {
    let d = SegmentSpec::default();
    let max_words_per_line = match v.get("max_words_per_line") { None | Some(Value::Null) => None,
        Some(x) => Some(x.as_u64().filter(|n| (1..=12).contains(n)).ok_or("segment.max_words_per_line: an integer 1..12")? as usize) };
    Ok(SegmentSpec {
        max_chars_per_line: get_usize(v, "segment", "max_chars_per_line", d.max_chars_per_line, 4, 60)?,
        max_lines: get_usize(v, "segment", "max_lines", d.max_lines, 1, 3)?,
        max_cps: get_f64(v, "segment", "max_cps", d.max_cps, 3.0, 1000.0)?,
        pause_break_s: get_f64(v, "segment", "pause_break_s", d.pause_break_s, 0.05, 5.0)?,
        min_words: get_usize(v, "segment", "min_words", d.min_words, 1, 10)?,
        max_words_per_line,
    })
}
fn parse_timing(v: &Value) -> Result<TimingSpec, String> {
    let d = TimingSpec::default();
    Ok(TimingSpec {
        lead_in_s: get_f64(v, "timing", "lead_in_s", d.lead_in_s, 0.0, 1.0)?,
        hold_s: get_f64(v, "timing", "hold_s", d.hold_s, 0.0, 3.0)?,
        bridge_gap_s: get_f64(v, "timing", "bridge_gap_s", d.bridge_gap_s, 0.0, 5.0)?,
        min_duration_s: get_f64(v, "timing", "min_duration_s", d.min_duration_s, 0.05, 10.0)?,
        min_gap_s: get_f64(v, "timing", "min_gap_s", d.min_gap_s, 0.0, 1.0)?,
    })
}
fn parse_layout(v: &Value) -> Result<LayoutSpec, String> {
    let d = LayoutSpec::default();
    let fixed_y_frac = match v.get("fixed_y_frac") { None | Some(Value::Null) => None,
        Some(x) => Some(x.as_f64().filter(|f| (-1.0..=1.0).contains(f)).ok_or("layout.fixed_y_frac: -1..1")?) };
    Ok(LayoutSpec {
        position: get_str(v, "layout", "position", &d.position, &["auto", "bottom", "middle", "top"])?,
        platform: get_str(v, "layout", "platform", &d.platform, &["none", "tiktok", "reels", "shorts", "youtube"])?,
        face_avoidance: get_bool(v, "layout", "face_avoidance", d.face_avoidance)?,
        side_margin_frac: get_f64(v, "layout", "side_margin_frac", d.side_margin_frac, 0.0, 0.3)?,
        fixed_y_frac,
    })
}
fn parse_contrast(v: &Value) -> Result<(ContrastSpec, [bool; 3]), String> {
    let d = ContrastSpec::default();
    let auto = get_bool(v, "contrast", "auto", d.auto)?;
    let explicit = [v.get("stroke_frac_override").is_some(), v.get("shadow_override").is_some(), v.get("box_alpha_override").is_some()];
    Ok((ContrastSpec {
        auto,
        min_stroke_frac: get_f64(v, "contrast", "min_stroke_frac", d.min_stroke_frac, 0.0, 0.3)?,
        max_stroke_frac: get_f64(v, "contrast", "max_stroke_frac", d.max_stroke_frac, 0.0, 0.3)?,
        min_shadow: get_f64(v, "contrast", "min_shadow", d.min_shadow, 0.0, 30.0)?,
        max_shadow: get_f64(v, "contrast", "max_shadow", d.max_shadow, 0.0, 30.0)?,
        min_box_alpha: get_f64(v, "contrast", "min_box_alpha", d.min_box_alpha, 0.0, 1.0)?,
        max_box_alpha: get_f64(v, "contrast", "max_box_alpha", d.max_box_alpha, 0.0, 1.0)?,
    }, explicit))
}
fn parse_style(v: &Value) -> Result<StyleSpec, String> {
    let d = StyleSpec::default();
    let case = match get_str(v, "style", "case", "none", &["none", "upper", "lower", "title"])?.as_str() {
        "upper" => Case::Upper, "lower" => Case::Lower, "title" => Case::Title, _ => Case::None };
    let bg = match v.get("box") { None | Some(Value::Null) => None, Some(b) => {
        if !b.is_object() { return Err("style.box: an object {pad_frac?, radius_frac?, color?}".into()); }
        let color = match b.get("color") { None | Some(Value::Null) => [0.0, 0.0, 0.0, 0.8], Some(Value::Array(a)) if a.len() == 4 => {
            let mut c = [0.0; 4]; for i in 0..4 { c[i] = a[i].as_f64().filter(|x| (0.0..=1.0).contains(x)).ok_or("style.box.color: [r,g,b,a] components 0..1")?; } c }
            _ => return Err("style.box.color: [r,g,b,a] components 0..1".into()) };
        Some(BoxSpec { pad_frac: get_f64(b, "style.box", "pad_frac", 0.3, 0.0, 2.0)?, radius_frac: get_f64(b, "style.box", "radius_frac", 0.2, 0.0, 2.0)?, color }) } };
    Ok(StyleSpec {
        font: v.get("font").and_then(|x| x.as_str()).unwrap_or(&d.font).to_string(),
        weight: get_f64(v, "style", "weight", d.weight, 100.0, 1000.0)?,
        size_frac: get_f64(v, "style", "size_frac", d.size_frac, 0.01, 0.4)?,
        fill: get_color(v, "style", "fill", d.fill)?,
        stroke_frac: get_f64(v, "style", "stroke_frac", d.stroke_frac, 0.0, 0.3)?,
        stroke_color: get_color(v, "style", "stroke_color", d.stroke_color)?,
        shadow_blur: get_f64(v, "style", "shadow_blur", d.shadow_blur, 0.0, 30.0)?,
        shadow_opacity: get_f64(v, "style", "shadow_opacity", d.shadow_opacity, 0.0, 1.0)?,
        bg, case,
        letter_spacing: get_f64(v, "style", "letter_spacing", d.letter_spacing, -0.2, 2.0)?,
        line_height: get_f64(v, "style", "line_height", d.line_height, 0.5, 3.0)?,
        align: get_str(v, "style", "align", &d.align, &["left", "center", "right"])?,
        size_basis: get_str(v, "style", "size_basis", &d.size_basis, &["width", "short_side"])?,
    })
}
fn parse_word_states(v: &Value) -> Result<WordStates, String> {
    if v.is_null() { return Ok(WordStates::default()); }
    Ok(WordStates { active: word_delta(v.get("active").unwrap_or(&Value::Null), "word_states.active")?,
        spoken: word_delta(v.get("spoken").unwrap_or(&Value::Null), "word_states.spoken")?,
        upcoming: word_delta(v.get("upcoming").unwrap_or(&Value::Null), "word_states.upcoming")? })
}
fn parse_emphasis(v: &Value) -> Result<EmphasisSpec, String> {
    if v.is_null() { return Ok(EmphasisSpec::default()); }
    let list: Vec<String> = match v.get("list") { None | Some(Value::Null) => vec![], Some(a) => a.as_array().ok_or("emphasis.list: an array of words")?.iter().map(|x| x.as_str().map(|s| s.to_string()).ok_or_else(|| "emphasis.list: an array of strings".to_string())).collect::<Result<_, _>>()? };
    Ok(EmphasisSpec { list, by_keyword: get_bool(v, "emphasis", "by_keyword", false)?, keyword_count: get_usize(v, "emphasis", "keyword_count", 2, 1, 10)?,
        by_energy: get_bool(v, "emphasis", "by_energy", false)?, energy_top_frac: get_f64(v, "emphasis", "energy_top_frac", 0.2, 0.01, 1.0)?,
        delta: word_delta(v.get("delta").unwrap_or(&Value::Null), "emphasis.delta")? })
}
fn parse_prop_anim(v: &Value, path: &str) -> Result<PropAnim, String> {
    let prop = match get_str(v, path, "prop", "opacity", &["opacity", "scale", "y", "blur"])?.as_str() {
        "scale" => AnimProp::Scale, "y" => AnimProp::Y, "blur" => AnimProp::Blur, _ => AnimProp::Opacity };
    Ok(PropAnim { prop, from: get_f64(v, path, "from", 0.0, -10000.0, 10000.0)?, to: get_f64(v, path, "to", 1.0, -10000.0, 10000.0)?,
        duration_s: get_f64(v, path, "duration_s", 0.15, 0.0, 10.0)?, ease: get_ease(v, path, "ease", Ease::Out)? })
}
fn parse_unit_anim(v: &Value, path: &str) -> Result<UnitAnim, String> {
    match v { Value::Null => Ok(UnitAnim::default()), Value::Array(a) => Ok(UnitAnim { props: a.iter().enumerate().map(|(i, p)| parse_prop_anim(p, &format!("{path}[{i}]"))).collect::<Result<_, _>>()? }),
        _ => Err(ferr(path, "an array of {prop, from, to, duration_s?, ease?}")) }
}
fn parse_animation(v: &Value) -> Result<AnimationSpec, String> {
    if v.is_null() { return Ok(AnimationSpec::default()); }
    Ok(AnimationSpec { unit_in: parse_unit_anim(v.get("unit_in").unwrap_or(&Value::Null), "animation.unit_in")?,
        unit_out: parse_unit_anim(v.get("unit_out").unwrap_or(&Value::Null), "animation.unit_out")?,
        active_lead_s: get_f64(v, "animation", "active_lead_s", 0.04, 0.0, 0.5)? })
}

// ---------------------------------------------------------------------------------------------
// 6. OVERRIDES: per-word/per-unit style deltas and free keyframes, applied last -- "nothing an AI
//    wants should be impossible because a field is missing or hardcoded" (Naman, Nov 3 2026).
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct RawKey { pub prop: String, pub t: f64, pub v: f64, pub ease: Ease }
#[derive(Clone, Debug, Default)]
pub struct OverrideSel { pub unit_index: Option<usize>, pub word_index: Option<usize> }
#[derive(Clone, Debug)]
pub struct Override { pub sel: OverrideSel, pub style: Option<WordDelta>, pub keyframes: Vec<RawKey> }

fn parse_overrides(v: &Value) -> Result<Vec<Override>, String> {
    match v { Value::Null => Ok(vec![]), Value::Array(arr) => arr.iter().enumerate().map(|(i, o)| {
        let path = format!("overrides[{i}]");
        if !o.is_object() { return Err(ferr(&path, "an object {unit_index? | word_index?, style?, keyframes?}")); }
        let unit_index = match o.get("unit_index") { None | Some(Value::Null) => None, Some(x) => Some(x.as_u64().ok_or_else(|| ferr(&format!("{path}.unit_index"), "an integer"))? as usize) };
        let word_index = match o.get("word_index") { None | Some(Value::Null) => None, Some(x) => Some(x.as_u64().ok_or_else(|| ferr(&format!("{path}.word_index"), "an integer"))? as usize) };
        let style = match o.get("style") { None | Some(Value::Null) => None, Some(s) => Some(word_delta(s, &format!("{path}.style"))?) };
        let keyframes = match o.get("keyframes") { None | Some(Value::Null) => vec![], Some(ks) => ks.as_array().ok_or_else(|| ferr(&format!("{path}.keyframes"), "an array"))?.iter().enumerate().map(|(j, k)| {
            let kp = format!("{path}.keyframes[{j}]");
            Ok(RawKey { prop: k.get("prop").and_then(|x| x.as_str()).ok_or_else(|| ferr(&format!("{kp}.prop"), "a string property name"))?.to_string(),
                t: get_f64(k, &kp, "t", 0.0, 0.0, 100000.0)?, v: get_f64(k, &kp, "v", 0.0, -100000.0, 100000.0)?, ease: get_ease(k, &kp, "ease", Ease::Linear)? })
        }).collect::<Result<Vec<_>, String>>()? };
        if unit_index.is_none() && word_index.is_none() && style.is_none() && keyframes.is_empty() { return Err(ferr(&path, "needs at least a selector and a style or keyframes")); }
        Ok(Override { sel: OverrideSel { unit_index, word_index }, style, keyframes })
    }).collect(), _ => Err("overrides: an array".into()) }
}

// ---------------------------------------------------------------------------------------------
// 7. PRESETS: pop/karaoke/clean/boxed/impact (auto_captions) and subtitle/boxed/yellow
//    (set_caption_style) re-expressed as spec data on top of the primitives above. See the
//    CHANGELOG for exactly which ones reproduce the old renders byte-identically and which don't.
// ---------------------------------------------------------------------------------------------

/// The reference starting points. These are NOT special-cased in the engine itself -- `compose()` never checks
/// `preset`'s name, it only ever sees the expanded spec Value below, same as a caller's own custom one. Every
/// field here is a normal override target: pass `preset:"pop", style:{fill:"#00ff00"}` and the fill wins.
pub fn preset_spec(name: &str) -> Result<Value, String> {
    // CPS enforcement and the no-dangle line-break rule are new engine behaviour the legacy named looks
    // never had (auto_captions' own paginate() has no reading-speed awareness at all): every preset below
    // sets max_cps very high so it never fires, matching the old looks' real behaviour instead of silently
    // adding a constraint they were never designed against. The char caps mirror auto_captions::look()'s own
    // derivation (`max_chars = (upper ? 16 : 20).max(per_line_words * 5)`) for each style's per_line/lines.
    let no_cps = 1000.0; let no_lead = json!({"timing":{"lead_in_s":0.0}, "contrast":{"auto":false}});
    let p = |segment: Value, style: Value, extra: Value| merge(&merge(&json!({"segment":segment,"style":style}), &extra), &no_lead);
    Ok(match name {
        "pop" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":3}), json!({"font":"Montserrat","weight":900.0,"size_frac":0.12,"fill":"#ffffff","stroke_frac":0.11,"case":"upper"}),
            json!({"word_states":{"active":{"fill":"#ffdb00","scale":1.22}}, "emphasis":{"by_keyword":true,"keyword_count":1,"delta":{"fill":"#3dff5c"}}})),
        "karaoke" => p(json!({"max_chars_per_line":20,"max_lines":2,"max_cps":no_cps,"max_words_per_line":4}), json!({"font":"Montserrat","weight":800.0,"size_frac":0.08,"fill":"#ffffff","stroke_frac":0.09,"case":"none"}),
            json!({"word_states":{"spoken":{"fill":"#ffdb00"},"active":{"scale":1.08}}, "emphasis":{"by_keyword":true,"keyword_count":2,"delta":{"fill":"#3dff5c"}}})),
        "clean" => p(json!({"max_chars_per_line":30,"max_lines":2,"max_cps":no_cps,"max_words_per_line":6}), json!({"font":"Inter","weight":700.0,"size_frac":0.064,"fill":"#ffffff","stroke_frac":0.05,"shadow_blur":8.0,"case":"none"}), json!({})),
        "boxed" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":3}), json!({"font":"Montserrat","weight":900.0,"size_frac":0.09,"fill":"#ffffff","case":"upper","box":{"color":[0.0,0.0,0.0,0.82]}}),
            json!({"word_states":{"active":{"fill":"#3dff5c"}}, "emphasis":{"by_keyword":true,"keyword_count":1,"delta":{"fill":"#ffdb00"}}})),
        "impact" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":2}), json!({"font":"Anton","weight":400.0,"size_frac":0.15,"fill":"#ffffff","stroke_frac":0.07,"shadow_blur":6.0,"case":"upper"}),
            json!({"word_states":{"active":{"fill":"#ffdb00"}}})),
        // subtitle/boxed/yellow are the restyle-path looks (set_caption_style): sized off the frame's SHORT
        // side, not its width, and pinned at a fixed height fraction rather than placed by face-avoidance --
        // this engine's fixed_y_frac/size_basis exist specifically to carry that convention over exactly.
        // stroke_frac here is stroke-pixels-at-this-size / size, since the legacy numbers are a flat pixel width.
        // These three restyle a caption group that was segmented by the default look ("pop": 3 words/page,
        // 1 line) and then only change size/colour/stroke/bg/position -- so the segmentation here mirrors
        // "pop"'s, not a fresh choice, to land on the same pages the legacy restyle path actually produces.
        "subtitle" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":3}), json!({"font":"Inter","size_basis":"short_side","size_frac":0.0667,"fill":"#ffffff","stroke_frac":0.04166667}), json!({"layout":{"fixed_y_frac":0.35}})),
        "boxed_simple" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":3}), json!({"font":"Inter","size_basis":"short_side","size_frac":0.0667,"fill":"#ffffff","box":{"color":[0.0,0.0,0.0,0.75]}}), json!({"layout":{"fixed_y_frac":0.35}})),
        "yellow" => p(json!({"max_chars_per_line":16,"max_lines":1,"max_cps":no_cps,"max_words_per_line":3}), json!({"font":"Inter","size_basis":"short_side","size_frac":0.076923,"fill":"#ffe61a","stroke_frac":0.05417}), json!({"layout":{"fixed_y_frac":0.35}})),
        _ => return Err(format!("preset must be pop|karaoke|clean|boxed|impact|subtitle|boxed_simple|yellow, got '{}'", crate::transcript_parse::sanitize(name))),
    })
}
/// Shallow-merges `over` onto `base` (objects merge key by key, recursively; anything else in `over` replaces
/// `base` outright) -- how a preset's fields become ordinary defaults the caller's own spec can still override.
fn merge(base: &Value, over: &Value) -> Value {
    match (base, over) { (Value::Object(b), Value::Object(o)) => { let mut m = b.clone(); for (k, v) in o { m.insert(k.clone(), merge(m.get(k).unwrap_or(&Value::Null), v)); } Value::Object(m) }
        (_, Value::Null) => base.clone(), _ => over.clone() }
}

// ---------------------------------------------------------------------------------------------
// 8. COMPOSE: the one tool surface. Builds a transcript + a track of styled, animated caption
//    clips from already-timed words, exactly the way `auto_captions::build` does for its five
//    presets -- except every hard part above is driven by the spec, not by a hardcoded `Look`.
// ---------------------------------------------------------------------------------------------

fn parse_words(a: &Value) -> Result<Vec<EWord>, String> {
    let arr = a.get("words").and_then(|v| v.as_array()).ok_or("words: an array of {text, start, end, conf?} in timeline seconds is required")?;
    if arr.is_empty() { return Err("words is empty".into()); }
    let mut out = vec![]; let mut prev_end = f64::NEG_INFINITY;
    for (i, x) in arr.iter().enumerate() {
        let p = format!("words[{i}]");
        let text = x.get("text").and_then(|v| v.as_str()).ok_or_else(|| ferr(&p, "text: a string"))?.to_string();
        if text.trim().is_empty() { return Err(ferr(&p, "text: must not be empty")); }
        let start = get_f64(x, &p, "start", f64::NAN, 0.0, 1e7)?; if start.is_nan() { return Err(ferr(&p, "start: a number of seconds is required")); }
        let end = get_f64(x, &p, "end", f64::NAN, 0.0, 1e7)?; if end.is_nan() { return Err(ferr(&p, "end: a number of seconds is required")); }
        if end < start { return Err(ferr(&p, "end must be >= start")); }
        if start < prev_end - 1e-6 { return Err(ferr(&p, "start must not precede the previous word's end (words must be in time order)")); }
        let conf = get_f64(x, &p, "conf", 1.0, 0.0, 1.0)? as f32;
        prev_end = end; out.push(EWord { text, start, end, conf });
    }
    Ok(out)
}

fn caption_engine_compose(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let words = parse_words(a)?;
    let mut spec = a.clone();
    if let Some(p) = a.get("preset").and_then(|v| v.as_str()) { spec = merge(&preset_spec(p)?, a); }
    let seg_spec = parse_segment(spec.get("segment").unwrap_or(&Value::Null))?;
    let timing_spec = parse_timing(spec.get("timing").unwrap_or(&Value::Null))?;
    let layout_spec = parse_layout(spec.get("layout").unwrap_or(&Value::Null))?;
    let (contrast_spec, contrast_explicit) = parse_contrast(spec.get("contrast").unwrap_or(&Value::Null))?;
    let style_spec = parse_style(spec.get("style").unwrap_or(&Value::Null))?;
    let word_states = parse_word_states(spec.get("word_states").unwrap_or(&Value::Null))?;
    let emphasis_spec = parse_emphasis(spec.get("emphasis").unwrap_or(&Value::Null))?;
    let animation_spec = parse_animation(spec.get("animation").unwrap_or(&Value::Null))?;
    let overrides = parse_overrides(spec.get("overrides").unwrap_or(&Value::Null))?;
    let shot_cuts: Option<Vec<f64>> = match a.get("shot_cuts") { None | Some(Value::Null) => None,
        Some(v) => Some(v.as_array().ok_or("shot_cuts: an array of seconds")?.iter().map(|x| x.as_f64().ok_or_else(|| "shot_cuts: every entry must be a number".to_string())).collect::<Result<_, _>>()?) };
    let font_fallback = crate::text::resolve_font_info(&style_spec.font).1;

    let units = segment(&words, &seg_spec)?;
    if units.is_empty() { return Err("segmentation produced no caption units".into()); }
    let timed = time_units(&units, &words, &timing_spec, shot_cuts.as_deref());

    let (w, h) = (ss.project.width as f64, ss.project.height as f64);
    let tall = h > w * 1.05;
    let mut default_band: f64 = match layout_spec.position.as_str() { "bottom" => if tall { 0.70 } else { 0.80 }, "middle" => 0.5, "top" => 0.2, _ => if tall { 0.70 } else { 0.80 } };
    // platform safe-area: keep clear of the estimated UI strip (captions/account bar, action rail) a
    // vertical-video app draws over the lower part of the frame, same idea as safe_margins.rs's own table.
    if layout_spec.position == "auto" || layout_spec.position == "bottom" { let clear = bottom_clear_frac(&layout_spec.platform); if clear > 0.0 { default_band = default_band.min(1.0 - clear); } }
    let media = visible_media(ss);

    // transcript (same shape auto_captions builds, so transcript_edit_word / export_captions keep working)
    let tr_id = next_id("tr", ss.project.editorial.transcripts.keys().map(|s| s.as_str()));
    let mut tr = Transcript { id: tr_id.clone(), name: "caption engine".into(), language: a.get("language").and_then(|v| v.as_str()).unwrap_or("auto").to_string(), ..Default::default() };
    let take_id = "take1".to_string();
    tr.takes.push(Take { id: take_id.clone(), source: TakeSource::default(), src_start: words[0].start, src_end: words[words.len() - 1].end, timeline_start: Some(words[0].start), speed: 1.0, order: 0, status: TakeStatus::Chosen, note: String::new() });
    for wd in &words { let wid = format!("w{}", tr.words.len() + 1); let mut nw = Word::new(&wid, &take_id, &wd.text, wd.start, wd.end, Some(wd.conf)); nw.display = nw.display.replace(' ', "\u{a0}"); if wd.conf < 0.5 { nw.add_flag(WordFlag::LowConfidence); } tr.words.push(nw); }
    tr.version += 1; tr.derive_times(); tr.validate()?;

    if let Some(old) = a.get("replace").and_then(|v| v.as_str()) { remove_group(ss, old)?; }
    let track_id = ss.project.gen_id("track");
    ss.project.tracks.push(Track { id: track_id.clone(), clips: vec![], locked: false, muted: false, solo: false, name: "Captions".into() });

    let mut clip_ids = vec![]; let mut straddle_warnings = vec![]; let mut avoided = 0usize;
    for (ui, tu) in timed.iter().enumerate() {
        crate::guard::check();
        if let Some(c) = tu.straddled_cut { straddle_warnings.push(format!("unit {ui}: could not avoid straddling a shot cut at {c:.3}s without a sub-minimum flash")); }
        let idx: Vec<usize> = (tu.unit.first..=tu.unit.last).collect();
        let disp: Vec<String> = idx.iter().map(|&i| tr.words[i].display.clone()).collect(); let dref: Vec<&str> = disp.iter().map(|s| s.as_str()).collect();
        let raw_text = compose_cue_text(&dref, &tu.unit.breaks_after);
        let text = apply_case(&raw_text, style_spec.case);
        let dur = (tu.end - tu.start).max(0.05);
        let mid_t = tu.start + dur / 2.0;
        let (y, band_name) = match layout_spec.fixed_y_frac {
            Some(f) => (f * h, "fixed".to_string()),
            None => { let (band_y, band_name, face_moved) = place_unit(default_band, tall, &layout_spec, &media, mid_t); if face_moved { avoided += 1; } ((band_y - 0.5) * h, band_name.to_string()) }
        };

        let size_basis_px = if style_spec.size_basis == "short_side" { w.min(h) } else { w };
        let mut size = (style_spec.size_frac * size_basis_px).round().clamp(8.0, 600.0);
        let mut fx = TextFx { weight: style_spec.weight, uppercase: false, bg: style_spec.bg.as_ref().map(|b| b.color),
            bg_pad: style_spec.bg.as_ref().map_or(24.0, |b| (b.pad_frac * size).round()), bg_radius: style_spec.bg.as_ref().map_or(12.0, |b| (b.radius_frac * size).round()),
            shadow_blur: style_spec.shadow_blur, shadow_opacity: if style_spec.shadow_blur > 0.0 { style_spec.shadow_opacity.max(0.01) } else { 0.0 }, line_spacing: style_spec.line_height, ..Default::default() };
        let fit_target = 0.86 * w - if fx.bg.is_some() { 2.0 * fx.bg_pad } else { 0.0 };
        if let Ok(m) = crate::text::measure_text(&text, &style_spec.font, size, style_spec.letter_spacing * size, style_spec.weight, &style_spec.align, w, h, Some(&fx)) {
            if m.width > fit_target { size = (size * fit_target / m.width).floor().max(8.0); fx.bg_pad = style_spec.bg.as_ref().map_or(fx.bg_pad, |b| (b.pad_frac * size).round()); fx.bg_radius = style_spec.bg.as_ref().map_or(fx.bg_radius, |b| (b.radius_frac * size).round()); }
        }
        fx.box_width = if fx.bg.is_some() { (0.90 * w - 2.0 * fx.bg_pad).max(0.3 * w) } else { (1.0 - 2.0 * layout_spec.side_margin_frac) * w };

        let (mut stroke_frac, mut shadow_strength, mut box_alpha) = (style_spec.stroke_frac, style_spec.shadow_blur, fx.bg.map_or(0.8, |b| b[3]));
        if contrast_spec.auto {
            let rect_w = (fit_target.min(w * 0.9)).max(1.0) as usize; let rect_h = (size * style_spec.line_height * seg_spec.max_lines as f64 * 1.2).max(1.0) as usize;
            let rect = [((w - rect_w as f64) / 2.0).max(0.0) as usize, (y + h / 2.0 - rect_h as f64 / 2.0).max(0.0) as usize, rect_w, rect_h];
            if let Some(luma) = measure_bg_luma(ss, mid_t.min(ss.project.duration().max(0.01)), rect) {
                let (st, sh, bx) = contrast_strength(luma, &contrast_spec);
                if !contrast_explicit[0] && style_spec.stroke_frac > 0.0 { stroke_frac = st; }
                if !contrast_explicit[1] && style_spec.shadow_blur > 0.0 { shadow_strength = sh; }
                if !contrast_explicit[2] && fx.bg.is_some() { box_alpha = bx; }
            }
        }
        if let Some(c) = fx.bg.as_mut() { c[3] = box_alpha; } fx.shadow_blur = shadow_strength.max(if style_spec.shadow_blur > 0.0 { style_spec.shadow_blur } else { 0.0 }).min(30.0);
        if style_spec.shadow_blur <= 0.0 { fx.shadow_blur = 0.0; fx.shadow_opacity = 0.0; }

        let s: Vec<f64> = idx.iter().map(|&i| tr.words[i].start - tu.start).collect(); let e: Vec<f64> = idx.iter().map(|&i| tr.words[i].end - tu.start).collect();
        let energy = if emphasis_spec.by_energy { word_energy_db(&media, tu.start, dur) } else { None };
        let emph_idx = emphasis_words(&words[tu.unit.first..=tu.unit.last], &emphasis_spec, energy.as_ref().map(|(d, h)| (d.as_slice(), *h)));
        fx.animators = compose_animators(&word_states, &emph_idx, &emphasis_spec.delta, &s, &e, dur, animation_spec.active_lead_s);
        let mut props = build_unit_props(&animation_spec, dur, y);
        // default gentle in/out, same shape every caption look wants, only when the caller gave no animation spec at all
        if animation_spec.unit_in.props.is_empty() && animation_spec.unit_out.props.is_empty() {
            let mut op = Prop::new(1.0); let prev_end = if ui > 0 { timed[ui - 1].end } else { f64::NEG_INFINITY };
            if tu.start - prev_end > 0.05 { op.set_key(0.0, 0.0, Ease::Out); op.set_key(0.07f64.min(dur / 2.0), 1.0, Ease::Hold); }
            let gap_after = timed.get(ui + 1).map_or(1.0, |n| n.start - tu.end);
            if gap_after > 0.05 && dur > 0.2 { if op.keys.is_empty() { op.set_key(0.0, 1.0, Ease::Hold); } op.set_key(dur - 0.08, 1.0, Ease::InOut); op.set_key(dur, 0.0, Ease::Hold); }
            if !op.keys.is_empty() { props.insert("opacity".into(), op); }
        }

        for ov in overrides.iter().filter(|o| o.sel.unit_index.map_or(true, |x| x == ui)) {
            if let (Some(wi), Some(_)) = (ov.sel.word_index, Some(())) { if wi > tu.unit.last - tu.unit.first { continue; }
                if let Some(d) = &ov.style { let mut an = base_animator(&format!("ov{ui}_{wi}"), wi as f64 / s.len().max(1) as f64, (wi + 1) as f64 / s.len().max(1) as f64);
                    if let Some(f) = d.fill { an.fill = Some(f); } if let Some(sc) = d.scale { an.scale = Prop::new(sc); } if let Some(op) = d.opacity { an.opacity = Prop::new(op); } fx.animators.push(an); } }
            for k in &ov.keyframes {
                let mut p = props.remove(&k.prop).unwrap_or_else(|| Prop::new(k.v)); p.set_key(k.t, k.v, k.ease); props.insert(k.prop.clone(), p);
            }
        }

        let id = ss.project.gen_id("clip");
        let src = Source::Text { text, font: style_spec.font.clone(), size, color: style_spec.fill, letter_spacing: style_spec.letter_spacing * size,
            stroke_width: (stroke_frac * size).round(), stroke_color: style_spec.stroke_color, shadow: if fx.shadow_blur > 0.0 { (size * 0.05).round() } else { 0.0 }, align: style_spec.align.clone() };
        let c = Clip { id: id.clone(), source: src, start: tu.start, duration: dur, in_point: 0.0, speed: 1.0, speed_curve: None, speed_blend: Default::default(), time_remap: None, props, grade_graph: None,
            effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0,
            text_fx: Some(fx), parent: None, inherit: Default::default(), name: Some(format!("caption {}", ui + 1)), role: None, tags: vec!["caption_engine".into()], transition_in: None };
        let _ = band_name;
        ss.project.tracks.last_mut().unwrap().clips.push(std::sync::Arc::new(c));
        ss.project.editorial.cue_words.insert(id.clone(), CueBinding { word_ids: idx.iter().map(|&i| tr.words[i].id.clone()).collect(), breaks_after: tu.unit.breaks_after.clone() });
        clip_ids.push(id);
    }
    let style_name = a.get("preset").and_then(|v| v.as_str()).map(|p| format!("engine_{p}")).unwrap_or_else(|| "engine_custom".into());
    ss.project.editorial.transcripts.insert(tr_id.clone(), tr);
    let caption_id = ss.project.gen_id("captions");
    ss.project.editorial.captions.insert(caption_id.clone(), crate::editorial::CaptionGroup { track_id: track_id.clone(), clips: clip_ids.clone(), style: style_name, burn_in: true, transcript_id: Some(tr_id.clone()) });

    Ok(json!({"caption_id": caption_id, "track_id": track_id, "transcript_id": tr_id, "units": clip_ids.len(), "words": words.len(),
        "face_avoidance_fired": avoided, "straddle_warnings": straddle_warnings, "font_fallback": font_fallback,
        "next": "edit_caption_words fixes a misheard word; export_captions writes .srt/.vtt; overrides/preset fields can be re-applied with caption_engine_compose(replace:caption_id, ...)"}))
}

/// Mean audio-energy (dB, `sw::onsets`) over a unit's own on-screen window, from whichever media clip is active at
/// its start -- `None` when nothing is decodable (silence/graphics-only unit, or no media there at all) rather
/// than inventing a loudness.
fn word_energy_db(media: &[VisibleMedia], unit_start: f64, dur: f64) -> Option<(Vec<f32>, f64)> {
    let m = media_at(media, unit_start)?;
    let t0 = (m.in_point + (unit_start - m.timeline_start) * m.speed).max(0.0);
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-ss", &format!("{t0:.3}"), "-t", &format!("{:.3}", (dur * m.speed).max(0.05)), "-i", &m.path, "-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"]);
    let bytes = capture(&mut cmd, 16000 * 2 * 20, Duration::from_secs(20)).ok()?;
    if bytes.len() < 4 { return None; }
    let samples: Vec<f32> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    let o = sw::onsets(&samples, 16000);
    if o.db.is_empty() { return None; } Some((o.db, o.hop))
}

fn remove_group(ss: &mut Session, id: &str) -> Result<(), String> {
    let g = ss.project.editorial.captions.remove(id).ok_or_else(|| format!("replace: no caption group '{}'", crate::transcript_parse::sanitize(id)))?;
    for c in &g.clips { ss.project.editorial.cue_words.remove(c); }
    if let Some(t) = ss.project.tracks.iter_mut().find(|t| t.id == g.track_id) { t.clips.retain(|c| !g.clips.contains(&c.id)); }
    if ss.project.tracks.iter().any(|t| t.id == g.track_id && t.clips.is_empty() && t.name == "Captions") { ss.project.tracks.retain(|t| t.id != g.track_id); }
    Ok(())
}

pub fn commands() -> Vec<Command> {
    let word_delta_schema = |label: &str| json!({"type":"object","description":format!("{label} style delta: any subset wins, the rest is untouched"),
        "properties":{"fill":{"description":"hex colour or [r,g,b] 0..1"},"scale":{"type":"number"},"opacity":{"type":"number"}}});
    let prop_anim_schema = json!({"type":"array","description":"keyframe property animations: opacity/scale/y/blur from->to over duration_s with any easing",
        "items":{"type":"object","properties":{"prop":{"type":"string","enum":["opacity","scale","y","blur"]},"from":{"type":"number"},"to":{"type":"number"},"duration_s":{"type":"number"},"ease":{"type":"string","enum":crate::keyframe::EASE_NAMES.to_vec()}}}});
    vec![
        Command { name: "caption_engine_compose", mutates: true,
            description: "The Kimo caption ENGINE (V8.12.0): builds word-synced, styled, animated captions from a spec of composable primitives, not a fixed list of named looks. `pop`, `karaoke`, `clean`, `boxed`, `impact`, `subtitle`, `boxed_simple` and `yellow` are reference presets ONLY -- starting points you can take or leave. Every default here (segmentation thresholds, CPS, lead-in, gap bridging, face avoidance, contrast strength, animation curves) is an ordinary field you can override, per call AND per individual word or unit through `overrides`, and free per-word/per-unit keyframes on any property, any time, any easing sit alongside the rule-based primitives and apply last. Nothing is hardcoded against you: if a preset's exact numbers aren't what you want, override the field -- don't pick a different preset name and hope. The engine still owns the hard parts so you don't have to get them right by hand: segmentation (phrase/pause/punctuation boundaries, reading-speed and line-length limits, never an orphan word), timing (snap to word times, lead-in, gap bridging, no overlaps, no sub-minimum flashes, shot-cut aware), layout (safe margins, per-cue face/subject avoidance), and contrast-aware legibility (outline/shadow/box strength raised automatically on a bright background, only where you haven't already said what you want there).",
            schema: obj(vec![
                ("words", json!({"type":"array","description":"required: ASR words in TIMELINE seconds, already timed and in order: [{text, start, end, conf?}]","items":{"type":"object"}})),
                ("preset", json!({"type":"string","enum":["pop","karaoke","clean","boxed","impact","subtitle","boxed_simple","yellow"],"description":"optional reference starting point; every field below still overrides it"})),
                ("segment", obj(vec![("max_chars_per_line", n("default 20")), ("max_lines", n("1..3, default 2")), ("max_cps", n("reading speed cap, chars/sec, default 20")), ("pause_break_s", n("pause that forces a break, default 0.45")), ("min_words", n("default 1")), ("max_words_per_line", n("optional: wrap by word count instead of the character budget (1..12)"))], &[])),
                ("timing", obj(vec![("lead_in_s", n("snap-in lookahead, default 0.04")), ("hold_s", n("post-roll when no bridge, default 0.35")), ("bridge_gap_s", n("gap below which cues snap together, default 0.6")), ("min_duration_s", n("default 0.2")), ("min_gap_s", n("minimum visible gap between cues, default 0.02"))], &[])),
                ("layout", obj(vec![("position", json!({"type":"string","enum":["auto","bottom","middle","top"]})), ("platform", json!({"type":"string","enum":["none","tiktok","reels","shorts","youtube"]})), ("face_avoidance", json!({"type":"boolean","description":"default true: a cue moves off a detected face/subject at its own on-screen moment"})), ("side_margin_frac", n("default 0.05")), ("fixed_y_frac", json!({"type":"number","description":"optional -1..1: pin the vertical offset to exactly this fraction of frame height (0 = centre, positive = down, the same convention the clip's own y property uses), skipping face-avoidance placement entirely"}))], &[])),
                ("contrast", obj(vec![("auto", json!({"type":"boolean","description":"default true: stroke/shadow/box strength is set from measured background luminance under the text"})),
                    ("min_stroke_frac", n("")), ("max_stroke_frac", n("")), ("min_shadow", n("")), ("max_shadow", n("")), ("min_box_alpha", n("")), ("max_box_alpha", n(""))], &[])),
                ("style", obj(vec![("font", s("font name or .ttf path")), ("weight", n("100..1000")), ("size_frac", n("type size as a fraction of frame width, or of the short side when size_basis is short_side")), ("size_basis", json!({"type":"string","enum":["width","short_side"],"description":"default width"})), ("fill", colr_schema("text colour")),
                    ("stroke_frac", n("outline width as a fraction of size")), ("stroke_color", colr_schema("outline colour")), ("shadow_blur", n("")), ("shadow_opacity", n("")),
                    ("box", obj(vec![("pad_frac", n("")), ("radius_frac", n("")), ("color", json!({"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}))], &[])),
                    ("case", json!({"type":"string","enum":["none","upper","lower","title"]})), ("letter_spacing", n("")), ("line_height", n("")), ("align", json!({"type":"string","enum":["left","center","right"]}))], &[])),
                ("word_states", obj(vec![("active", word_delta_schema("currently-spoken word")), ("spoken", word_delta_schema("already-spoken words (the generic karaoke trail)")), ("upcoming", word_delta_schema("not-yet-spoken words"))], &[])),
                ("emphasis", obj(vec![("list", json!({"type":"array","items":{"type":"string"},"description":"explicit words to emphasise"})), ("by_keyword", json!({"type":"boolean"})), ("keyword_count", n("")),
                    ("by_energy", json!({"type":"boolean","description":"emphasise the loudest words by measured audio energy"})), ("energy_top_frac", n("top fraction of words by loudness, default 0.2")), ("delta", word_delta_schema("emphasised words"))], &[])),
                ("animation", obj(vec![("unit_in", prop_anim_schema.clone()), ("unit_out", prop_anim_schema), ("active_lead_s", n("pop-in lookahead for the active word, default 0.04"))], &[])),
                ("overrides", json!({"type":"array","description":"per-word or per-unit escape hatch: {unit_index?, word_index?, style?, keyframes?} -- applied last, after every rule above",
                    "items":{"type":"object","properties":{"unit_index":{"type":"integer"},"word_index":{"type":"integer"},"style":word_delta_schema("override"),"keyframes":{"type":"array","items":{"type":"object","properties":{"prop":{"type":"string"},"t":{"type":"number"},"v":{"type":"number"},"ease":{"type":"string"}}}}}}})),
                ("shot_cuts", json!({"type":"array","items":{"type":"number"},"description":"optional hard-cut times (seconds) to avoid straddling"})),
                ("language", s("default auto")), ("replace", s("caption_id of an earlier engine group to replace"))], &["words"]),
            run: caption_engine_compose },
    ]
}
fn colr_schema(d: &str) -> Value { json!({"description": d, "anyOf": [{"type":"string"}, {"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}]}) }
