//! `auto_edit`: one call, raw clips -> a finished, polished edit, chaining Kimo's own existing tools with
//! sensible defaults -- rough cut (dead air out) -> b-roll placement (RV-33: matched to the words spoken,
//! given a transcript) -> a slow Ken Burns push-in on static b-roll cutaways -> jump-cut punch-ins (only on the segments that stay on screen, so the zoom is never
//! hidden under b-roll) -> word-synced captions -> an optional cold-open hook title -> an optional music bed
//! -> auto color -> a b-roll color/exposure match to the talking head -> a final audio mix to a YouTube
//! loudness target -> a subscribe-style end screen -> lint_edit's
//! own review, auto-applying whatever it can fix by itself. Built because an
//! agent handed a folder of raw clips had no single call for "edit this well"; it had to compose eight-plus
//! tool calls in the right order to get a result that didn't look assembled by hand. Every stage below is an
//! ordinary existing tool, called through dispatch_nested (same validation, same locks, one undo step for
//! the whole edit); nothing here duplicates another tool's logic.
use super::kit::*;
use crate::commands::dispatch_nested as call;
use crate::model::Source;

/// Same rounding convention `auto_rough_cut`'s own returned "duration"/clip times already use
/// (director_tools.rs `r3`): re-deriving edit_duration from raw clip floats after pacing must not
/// introduce sub-millisecond drift relative to what the rest of the pipeline (and lint_edit's own
/// speech re-detection, which this duration is compared against) treats as "the same" instant.
fn r3(v: f64) -> f64 { (v * 1000.0).round() / 1000.0 }

/// V8.8.0: the x264 preset auto_edit sets by default when `encoder_quality` is on. See CHANGELOG.md
/// v8.8.0 for the measured speed/size/SSIM table behind this specific choice.
const AUTO_EDIT_X264_PRESET: &str = "slow";

/// V8.5.5 end-card background tuning: freeze an earlier, calmer source frame instead of the literal last
/// one; blur it gently rather than into mush; scale it up a touch so the blur's edge falloff never shows;
/// darken it slightly so the white pill/text/dot pop against it. See the end_screen stage below.
///
/// V8.5.8: the 8.5.5 tuning held the frozen frame dead still for the whole card (zero motion across the
/// last ~14% of the edit's runtime, flagged as the #1 weakness in a montage review) and blurred it hard
/// enough to read as mush rather than a soft backdrop. Two changes, both scoped to the end card alone:
/// a slow Ken Burns drift on the backdrop's own `scale` (END_CARD_BG_SCALE -> END_CARD_BG_SCALE_END, eased
/// across the whole card so it never jumps) so the card reads as a designed background in motion, not a
/// freeze-frame slide; and a lighter blur radius so the subject stays recognisable, clearly soft, not mush.
const END_CARD_BG_FREEZE_BACK_S: f64 = 0.5;
const END_CARD_BG_BLUR_RADIUS: f64 = 5.5;
const END_CARD_BG_SCALE: f64 = 1.08;
/// V8.5.8: the scale the backdrop's slow push-in has reached by the card's own end (started at
/// END_CARD_BG_SCALE); the drift runs the full end_screen_duration, eased in/out so it never jumps.
const END_CARD_BG_SCALE_END: f64 = 1.15;
const END_CARD_BG_DARKEN_STOPS: f64 = -0.2; // 2^-0.2 ≈ 0.87x brightness, about -13%
/// V8.5.8: the subscribe_pop badge (pill + dot + text) is built once by `add_title` at the design's own
/// 1x size (titles.rs, k = min(h,w)/1080) -- about 220px wide on a 1280-wide frame, read as too small for
/// the CTA to pop in a montage review. Rebuilding the badge's own text size and dot width at k * this
/// factor and recomputing the pill (`pill_width_for_text`) and the dot's room/position (`dot_room`,
/// `dot_x_for_pill`) at that same scaled k reproduces the exact same proportions -- side padding, and
/// critically the dot-inside-pill gap those two functions guarantee -- just bigger, so the 8.5.5 geometry
/// holds unchanged at any size.
const END_CARD_PILL_SCALE: f64 = 1.4;


fn atomic(ss: &mut Session, a: &Value, f: fn(&mut Session, &Value) -> Result<Value, String>) -> Result<Value, String> {
    let before = ss.project.clone();
    match f(ss, a) { Ok(v) => Ok(v), Err(e) => { ss.project = before; Err(e) } }
}
fn str_list(a: &Value, k: &str) -> Vec<String> {
    a.get(k).and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}
fn new_track(ss: &mut Session) -> Result<String, String> {
    call(ss, "add_track", &json!({}))?["track_id"].as_str().map(String::from).ok_or_else(|| "add_track returned no id".into())
}
/// Fraction of [s, e) covered by any range in `cover` (ranges assumed non-overlapping with each other).
fn covered_fraction(s: f64, e: f64, cover: &[(f64, f64)]) -> f64 {
    let span = (e - s).max(1e-6);
    let mut covered = 0.0;
    for (cs, ce) in cover { let a = s.max(*cs); let b = e.min(*ce); if b > a { covered += b - a; } }
    (covered / span).clamp(0.0, 1.0)
}

/// V8.2 job 2 / V8.5.6: where to split a long uncovered dialogue stretch so no piece sits static past
/// `max_static` seconds -- a hard guarantee (including the final piece), not a best-effort one. Prefers a
/// real pause in the words (a gap of >=0.12s between two consecutive words) so a cut lands between
/// sentences/phrases, never mid-word; among the pauses that keep the segment within `max_static` of the
/// last split, the one closest to the `max_static` mark is picked, so the jump-cut rhythm stays as even as
/// the words allow. Where no such pause exists nearby (a single breathless take), splits at the plain
/// `max_static` interval instead, so the rule always holds even on a take with no natural breath.
fn split_points(clip_start: f64, clip_end: f64, words: &[Value], max_static: f64) -> Vec<f64> {
    let mut pauses: Vec<f64> = vec![];
    for w in words.windows(2) {
        let (Some(pe), Some(ns)) = (w[0]["end"].as_f64(), w[1]["start"].as_f64()) else { continue };
        if ns - pe >= 0.12 && ns > clip_start + 0.5 && ns < clip_end - 0.5 { pauses.push(ns); }
    }
    pauses.sort_by(f64::total_cmp);
    let mut points = vec![];
    let mut last = clip_start;
    loop {
        if clip_end - last <= max_static { break; } // the remaining tail is already short enough: done
        let ideal = last + max_static;
        // Only a pause strictly within (last, ideal] is eligible: picking one past `ideal` would let this
        // segment itself exceed max_static, which is exactly the slack job 2 used to allow (and the gap the
        // 8.5.6 review caught). min_by finds the one nearest `ideal` from below, i.e. the latest safe pause.
        let pick = pauses.iter().filter(|&&p| p > last + 1.0 && p <= ideal).min_by(|x, y| (ideal - **x).total_cmp(&(ideal - **y)));
        let t = *pick.unwrap_or(&ideal);
        points.push(t);
        last = t;
    }
    points
}

fn auto_edit(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let th_ids = str_list(a, "talking_head_asset_ids");
    if th_ids.is_empty() { return Err("talking_head_asset_ids: give one or more imported talking-head asset ids, in story order (import_media first)".into()); }
    let broll_ids = str_list(a, "broll_asset_ids");
    let music_id = a.get("music_asset_id").and_then(|v| v.as_str()).map(String::from);
    let hook_text = a.get("hook_text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
    let hook_style = a.get("hook_style").and_then(|v| v.as_str()).unwrap_or("hook_box").to_string();
    let caption_style = a.get("caption_style").and_then(|v| v.as_str()).unwrap_or("pop").to_string();
    let caption_position = a.get("caption_position").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    let captions_on = a.get("captions").and_then(|v| v.as_bool()).unwrap_or(true);
    let target_lufs = opt_num(a, "target_lufs", -14.0);
    let level_dynamics = a.get("level_dynamics").and_then(|v| v.as_bool());
    let level_strength = a.get("level_strength").and_then(|v| v.as_f64());
    let match_strength = a.get("match_strength").and_then(|v| v.as_f64());
    let compress_strength = a.get("compress_strength").and_then(|v| v.as_f64());
    let whoosh_sfx = a.get("whoosh_sfx").and_then(|v| v.as_bool());
    let punch_scale = opt_num(a, "punch_scale", 1.15).clamp(1.01, 3.0);
    let punch_on = a.get("punch_ins").and_then(|v| v.as_bool()).unwrap_or(true);
    let push_in_on = a.get("push_in").and_then(|v| v.as_bool()).unwrap_or(true);
    let push_in_zoom = opt_num(a, "push_in_zoom", 1.05).clamp(1.01, 1.3);
    let broll_transitions_on = a.get("broll_transitions").and_then(|v| v.as_bool()).unwrap_or(true);
    let broll_transition_duration = opt_num(a, "broll_transition_duration", 0.25).clamp(0.1, 0.6);
    let grade_on = a.get("grade").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.0Q: on by default whenever there's a dialogue clip and at least one b-roll clip to match -- the
    // free color-cohesion win a raw auto-cut never gets (measured on the real benchmark: an indoor warm
    // talking head cut against a cool-toned city b-roll and a bright-daylight coffee b-roll, each already
    // "well exposed and neutral" by auto_grade's own absolute band, so auto_grade alone left the mismatch
    // untouched). Off entirely for callers who want their own look preserved.
    let broll_match_on = a.get("broll_color_match").and_then(|v| v.as_bool()).unwrap_or(true);
    let platform = a.get("platform").and_then(|v| v.as_str()).unwrap_or("none").to_string();
    // default on for the landscape/youtube-style edit (platform none); a vertical short usually ends on
    // whatever the platform's own UI shows next, so it defaults off there unless explicitly asked for.
    let end_screen_on = a.get("end_screen").and_then(|v| v.as_bool()).unwrap_or(platform == "none");
    let end_screen_text = a.get("end_screen_text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("SUBSCRIBE FOR MORE").to_string();
    let end_screen_duration = opt_num(a, "end_screen_duration", 3.0).clamp(2.0, 4.0);
    let pacing_on = a.get("pacing").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.7.0: picture-quality render mode for the finished edit -- linear-light compositing (crossfades/
    // b-roll keying/captions blend correctly instead of the gamma-space default: the audit measured a
    // black->white crossfade midpoint at 128/255 by default vs 188/255, the physically correct linear
    // value), lanczos scaling on the clip-to-canvas fit, and TPDF dither on final 8-bit quantization -- all
    // three gated on the SAME project.linear_blend switch (render.rs), a fast compositing-only option, NOT
    // the heavier color_pipeline (which measured ~3.4x slower end to end -- see CHANGELOG v8.7.0). Scoped
    // the way 8.5.7 scoped whip intensity to auto_edit: this flips the option on the PROJECT auto_edit
    // hands back, as the very last change it makes, after every grading/matching/lint call above has
    // already run its own measurement and set its own effect gains against the ordinary display-referred
    // frames those tools have always read -- none of that analysis changes, because linear_blend only
    // touches the actual alpha-composite step (render.rs::blend_region_opt), not decode or effects. Only
    // the later render (export, called separately by the caller) sees the option on. Building a project by
    // hand leaves linear_blend at its default (false), so golden_frames.txt / recipe_frames.txt (built
    // without auto_edit) stay byte-identical.
    let linear_compositing_on = a.get("linear_compositing").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.8.0 (quality audit #1): an explicit x264 preset for the final export. Measured on the real
    // benchmark at crf 18: ffmpeg's own unset default ("medium") is both bigger AND lower-SSIM than
    // "slow" at the SAME crf -- see CHANGELOG.md v8.8.0 for the measured table and the chosen preset.
    // Touches only encoder settings, never a rendered pixel, so it carries no golden_frames.txt/
    // recipe_frames.txt risk either way; still gated to auto_edit (project_set's x264_preset stays None,
    // ffmpeg's own default, for anyone building a project by hand).
    let encoder_quality_on = a.get("encoder_quality").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.8.1 (quality audit #2): decode mistagged/untagged HD source footage as bt709 instead of trusting
    // an incomplete SD tag (see render.rs::color_tag_override and model.rs's color_tag_sanity doc comment).
    let color_tag_sanity_on = a.get("color_tag_sanity").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.9.0 (quality audit #3): shutter-angle motion blur on the push-in Ken Burns ramp (see the push-in
    // section below for why punch-ins, hard instant cuts with no continuous motion, are untouched). Cheap:
    // render.rs's own motion-blur gate only pays the temporal-supersampling cost for the frames where a
    // property is actually moving, so this never slows a render by more than the push-in ramp itself did.
    let motion_blur_on = a.get("motion_blur").and_then(|v| v.as_bool()).unwrap_or(true);

    let mut stages = serde_json::Map::new();
    let mut warnings: Vec<String> = vec![];

    // 1. Rough cut: lay the talking-head assets back to back with the dead air cut out. This is the spine
    // everything else attaches to, so it runs first and un-skippably.
    let rc = call(ss, "auto_rough_cut", &json!({"asset_ids": th_ids}))?;
    let track_id = rc["track_id"].as_str().ok_or("auto_rough_cut returned no track_id")?.to_string();
    let mut dialogue_clips: Vec<(String, f64, f64)> = rc["clips"].as_array().map(|v| v.iter().filter_map(|c| Some((c["clip_id"].as_str()?.to_string(), c["start"].as_f64()?, c["end"].as_f64()?))).collect()).unwrap_or_default();
    if dialogue_clips.is_empty() { return Err("auto_rough_cut kept no speech from talking_head_asset_ids -- check the clips have audible speech".into()); }
    let mut edit_duration = rc["duration"].as_f64().unwrap_or(0.0);
    stages.insert("rough_cut".into(), json!({"track_id": track_id, "clip_ids": dialogue_clips.iter().map(|(id,_,_)| id).collect::<Vec<_>>(), "duration": edit_duration}));

    // 1a. Pacing: tighten_pauses shrinks the dead air STILL left inside rough_cut's own kept clips (a
    // breath between two phrases under the merge threshold, or lead-in before the first word) down to a
    // natural pad. Runs before transcription/b-roll/captions so none of their clip-id bindings ever see a
    // clip that later gets split -- by the time anything downstream reads dialogue_clips, the track is
    // already final. Re-derives dialogue_clips and edit_duration from the track afterward since a tightened
    // clip can split into more than one surviving piece.
    if pacing_on {
        match call(ss, "tighten_pauses", &json!({"track_id": track_id})) {
            Ok(tp) => {
                stages.insert("pacing".into(), tp);
                let mut refreshed: Vec<(String, f64, f64)> = ss.project.tracks.iter().find(|t| t.id == track_id)
                    .map(|t| t.clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).map(|c| (c.id.clone(), r3(c.start), r3(c.start + c.duration))).collect())
                    .unwrap_or_default();
                refreshed.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap());
                if !refreshed.is_empty() {
                    edit_duration = refreshed.iter().map(|(_, _, e)| *e).fold(0.0, f64::max);
                    dialogue_clips = refreshed;
                }
            }
            Err(e) => warnings.push(format!("pacing tightening skipped: {e}")),
        }
    }

    // 1b. Transcribe the dialogue ONCE, here, in timeline seconds -- reused by both b-roll relevance (next)
    // and captions (stage 4), instead of each stage running the recognizer separately. When captions are on,
    // a transcription failure (no speech model installed) is the same honest hard stop auto_captions always
    // gave, just raised earlier so nothing gets built on top of an edit that can't get its captions; when
    // captions are off this degrades quietly and b-roll falls back to its given order (RV-33 honesty rule).
    let mut transcript_words: Vec<Value> = vec![];
    if captions_on || !broll_ids.is_empty() {
        let clip_ids: Vec<&String> = dialogue_clips.iter().map(|(id, _, _)| id).collect();
        let transcribe = || -> Result<Vec<Value>, String> {
            let mut src_warnings = vec![];
            let srcs = super::auto_captions::sources(ss, &json!({"clip_ids": clip_ids}), &mut src_warnings)?;
            let mut mode = String::new(); let mut lang_seen: Option<String> = None; let mut out = vec![];
            for src in &srcs {
                let (ws, _) = super::auto_captions::recognize(src, &json!({}), &mut mode, &mut lang_seen)?;
                for w in ws { out.push(json!({"text": w.text, "start": src.start + w.start / src.speed, "end": src.start + w.end / src.speed})); }
            }
            Ok(out)
        };
        match transcribe() {
            Ok(ws) => transcript_words = ws,
            Err(e) => { if captions_on { return Err(e); } warnings.push(format!("b-roll relevance matching skipped: {e}")); }
        }
    }

    // 2. B-roll over the dialogue, before punch-ins: place_broll decides which stretches get covered, and
    // the punch-in step right after reads those stretches back so it never zooms a clip the viewer can't see.
    // With the transcript above, each cutaway is matched to the clip whose name/tags fit what's said right
    // then (RV-33); without it (no speech recognized, or recognition skipped), it falls back to given order.
    let mut broll_ranges: Vec<(f64, f64)> = vec![];
    let mut broll_clip_ids: Vec<String> = vec![];
    if !broll_ids.is_empty() {
        let mut broll_args = json!({"track_id": track_id, "asset_ids": broll_ids});
        if !transcript_words.is_empty() { broll_args["words"] = json!(transcript_words); }
        if let Some(v) = whoosh_sfx { broll_args["whoosh_sfx"] = json!(v); }
        let br = call(ss, "place_broll", &broll_args)?;
        if let Some(arr) = br["clips"].as_array() { for c in arr { if let (Some(s), Some(e)) = (c["start"].as_f64(), c["end"].as_f64()) { broll_ranges.push((s, e)); } if let Some(cid) = c["clip_id"].as_str() { broll_clip_ids.push(cid.to_string()); } } }
        stages.insert("broll".into(), br);
    } else {
        warnings.push("no broll_asset_ids given: the talking head runs with no cutaways".into());
    }

    // 2a2. V8.2V job 4: a short, subtle whip into each b-roll cutaway -- the thing a top editor does between
    // the head and a cutaway, never on every jump-cut punch-in (those stay hard cuts on purpose; this is
    // scoped to the b-roll boundary only, so it never fires more than place_broll already decided to cut
    // to b-roll). zoom_blur is the gentlest of the engine's whip-style kinds (a quick radial blur + push,
    // not a hard directional wipe), alternated with a directional whip for variety on edits with more than
    // one cutaway. Skipped on a clip too short for the transition to fit.
    if broll_transitions_on && !broll_clip_ids.is_empty() {
        const KINDS: [&str; 2] = ["zoom_blur", "whip_left"];
        for (i, cid) in broll_clip_ids.iter().enumerate() {
            let dur = ss.project.find_clip(cid).map_or(0.0, |c| c.duration);
            if dur < broll_transition_duration * 1.5 { continue; }
            let kind = KINDS[i % KINDS.len()];
            let mut tr_args = json!({"clip_id": cid, "kind": kind, "duration": broll_transition_duration});
            if kind == "whip_left" {
                // V8.5.7: opt-in-safe extra margin on top of the transitions.rs fix (whip_left/
                // whip_right now reads crisp even at the tool's own default intensity, 1.0) --
                // auto_edit fires this on every b-roll cutaway, so a touch gentler here keeps a
                // run of several whips from ever feeling heavy. A caller invoking add_transition
                // directly keeps the tool's own default (1.0) untouched.
                tr_args["intensity"] = json!(0.65);
            }
            if let Err(e) = call(ss, "add_transition", &tr_args) {
                warnings.push(format!("b-roll transition skipped on clip {cid}: {e}"));
            }
        }
    }

    // 2a3. V8.9.0 (quality audit #3): shake-detect and warp-stabilize handheld b-roll. A full
    // rolling-shutter-capable warp stabilizer already exists (stabilize.rs / the "stabilize" tool's warp
    // engine) but nothing in auto_edit ever called it. Scoped to b-roll only (never dialogue): add_punch_ins
    // below sets the talking-head clips' own "scale" prop directly, and stabilize's write_keys REPLACES
    // x/y/scale/rotation wholesale -- running it on a dialogue clip would either get clobbered by the punch
    // right after it, or clobber the punch if ordered the other way, so it is left out of scope here on
    // purpose rather than guessed at. Runs BEFORE push-in on purpose too: push-in's own candidate filter
    // already skips any clip whose x/y/scale carries keys, so a clip this stabilizes is naturally excluded
    // from also being pushed -- no double-write, no extra check needed.
    // Off by default: measured on this benchmark's real footage, even the cheap (low-feature, position-only)
    // shake SCAN this uses to decide whether a clip needs correcting costs on the order of 100ms per
    // decoded frame (there is no cheaper windowed-scan mode in the engine today -- stabilize always reads a
    // clip's full duration -- so the scan's cost is the clip's own length at that per-frame rate); a
    // full-quality correction pass on a clip that actually IS shaky costs far more again. On a project with
    // several b-roll cutaways that is well past the "doesn't tank speed" bar on its own, before any clip
    // even needs correcting. See CHANGELOG.md v8.9.0 for the measured numbers (locked-off vs handheld
    // shake, scan cost per clip). Each stabilized clip's measured shake and correction are reported in
    // stages.stabilize.
    let stabilize_shake_threshold_px = opt_num(a, "stabilize_shake_threshold_px", 1.5).clamp(0.1, 50.0);
    let stabilize_handheld_on = a.get("stabilize_handheld").and_then(|v| v.as_bool()).unwrap_or(false);
    if stabilize_handheld_on && !broll_clip_ids.is_empty() {
        let mut stabilized = vec![];
        // Scan via stabilize_clip directly (not the "stabilize" tool, which always calls write_keys as
        // part of the call): a locked-off clip must come out of this exactly as it went in, so the cheap
        // scan pass below is read-only, and write_keys only ever runs for a clip that measured over threshold.
        let scan_opts = match crate::stabilize::opts_from_args(&json!({"method": "position", "max_features": 100})) {
            Ok(o) => o, Err(e) => { warnings.push(format!("handheld stabilization skipped: {e}")); crate::stabilize::Opts::default() }
        };
        for cid in &broll_clip_ids {
            let dur = ss.project.find_clip(cid).map_or(0.0, |c| c.duration);
            if dur < 0.3 { continue; } // stabilize_clip needs at least 2 frames; anything this short isn't worth scanning
            let shake_px = match crate::stabilize::stabilize_clip(&ss.project, cid, &scan_opts) {
                Ok((_, rep)) => rep.get("max_correction_px").and_then(|v| v.as_f64()).unwrap_or(0.0),
                Err(e) => { warnings.push(format!("handheld shake scan skipped on clip {cid}: {e}")); continue; }
            };
            if shake_px < stabilize_shake_threshold_px {
                stabilized.push(json!({"clip_id": cid, "shake_px": shake_px, "stabilized": false}));
                continue; // locked-off (or close enough): left exactly as decoded, nothing written
            }
            let full_opts = match crate::stabilize::opts_from_args(&json!({"method": "position_scale_rotation", "framing": "crop", "mode": "smooth_motion", "smoothing": "gaussian"})) {
                Ok(o) => o, Err(e) => { warnings.push(format!("stabilize skipped on handheld clip {cid}: {e}")); continue; }
            };
            match crate::stabilize::stabilize_clip(&ss.project, cid, &full_opts).and_then(|(rows, rep)| crate::stabilize::write_keys(&mut ss.project, cid, &full_opts, &rows).map(|_| rep)) {
                Ok(rep) => stabilized.push(json!({"clip_id": cid, "shake_px": shake_px, "stabilized": true, "report": rep})),
                Err(e) => warnings.push(format!("stabilize skipped on handheld clip {cid} (shake {shake_px:.2}px): {e}")),
            }
        }
        if !stabilized.is_empty() { stages.insert("stabilize".into(), json!({"clips": stabilized, "threshold_px": stabilize_shake_threshold_px})); }
    }

    // 2b. Slow push-in (V8.2V, Ken Burns): a gentle, eased push-in on every static b-roll cutaway -- the
    // free "it's not just a photo pasted in" win a raw cutaway never gets. Scoped to b-roll on purpose:
    // the talking head gets its own cut-zoom treatment below (job 2), so a b-roll-only push here never
    // competes with that. Skips a clip whose own scale/x/y already carry keys or an expression (an
    // already-animated cutaway, e.g. a caller-supplied pan, is left exactly as it was, never double-pushed)
    // and any clip too short for a push to read as motion rather than a flicker.
    if push_in_on && !broll_clip_ids.is_empty() {
        let candidates: Vec<String> = broll_clip_ids.iter().filter(|cid| {
            ss.project.find_clip(cid).is_some_and(|c| c.duration >= 1.2
                && ["scale", "x", "y"].iter().all(|k| c.props.get(*k).is_none_or(|q| q.keys.is_empty() && q.expr.is_empty())))
        }).cloned().collect();
        let mut pushed = vec![];
        let mut pushed_ids = vec![];
        for cid in &candidates {
            let dur = ss.project.find_clip(cid).map_or(0.0, |c| c.duration);
            let samples = if dur > 6.0 { 6 } else { 3 };
            match call(ss, "add_smart_punch", &json!({"clip_id": cid, "zoom": push_in_zoom, "samples": samples})) {
                Ok(v) => { pushed.push(v); pushed_ids.push(cid.clone()); }
                Err(e) => warnings.push(format!("push-in skipped on clip {cid}: {e}")),
            }
        }
        if !pushed.is_empty() { stages.insert("push_in".into(), json!({"clips": pushed})); }
        // V8.9.0 (quality audit #3): a push-in is the one ramp auto_edit creates that actually moves a
        // property continuously within a single clip (ease: 0 punch-ins below are hard, instant cuts --
        // nothing moves WITHIN either side of the cut, so render.rs's own "did this property actually
        // move" motion-blur gate (render.rs's `moving` check) would never fire for them even if asked;
        // motion_blur is left untouched there on purpose). Every successful push-in gets SOME motion blur
        // by construction (it always ramps scale from 1.0 to push_in_zoom, so it always moves); the amount
        // is proportional to how fast it actually moves, not a flat constant: sample the written scale
        // ramp, convert the fastest frame-to-frame scale delta to an approximate edge pixel displacement
        // (treating the cutaway as roughly canvas-width), and map that to a shutter angle between a 90 deg
        // floor (a real but gentle push should still read as a touch of blur, not nothing) and a 180 deg
        // ceiling (reached once the ramp is moving 6px/frame or faster at the edge).
        //
        // V8.9.1 investigation (Tab reviewing V8.9.0's own visual proof, quality audit #3 follow-up): the
        // sheet this version adds (motionblur_v04_vs_v05.png) found this benchmark's two real push-ins
        // measure only ~1.2-1.5px/frame of edge displacement (shutter lands at 108/112 deg, barely above
        // the floor) -- genuinely subtle, confirmed localized in the predicted pixels (max diff 30-43/255
        // right where the push moves, not noise) but near-invisible in a still frame. Tried raising the
        // floor to 130 deg and reaching 180 at 3px/frame instead of 6: render.rs's own blur math is
        // `sh = motion_blur/360/fps` (the exposure WINDOW, in seconds) times the property's actual
        // per-frame velocity -- so at this clip's real ~1.2-1.5px/frame, even the full 180 deg ceiling only
        // ever blurs ~0.6-0.75px (half a frame's worth of a sub-2px/frame move), and the retuned mapping
        // measured within noise of the original (diff-vs-no-blur mean 1.94-1.96/255 either way; see
        // CHANGELOG.md v8.9.1) -- not a mapping-curve problem. Reverted the mapping to its V8.9.0 values:
        // shutter angle cannot invent pixel motion that the push itself doesn't have, and there is no
        // physically-sensible way to get more than a sub-pixel blur out of auto_edit's default 5% Ken
        // Burns push without either exceeding a real 180 deg shutter (not honest) or raising the push's own
        // speed (push_in_zoom, an editorial preset choice, out of scope for a core-engine pass). The engine
        // is working correctly; this specific benchmark's default push is just that gentle.
        if motion_blur_on {
            let (pw, fps) = (ss.project.width as f64, ss.project.fps);
            for cid in &pushed_ids {
                let Some(c) = ss.project.find_clip(cid) else { continue };
                let dur = c.duration;
                let Some(scale_prop) = c.props.get("scale").cloned() else { continue };
                let n = 60usize;
                let mut max_px_per_frame = 0.0f64;
                for i in 0..n {
                    let t0 = dur * i as f64 / n as f64;
                    let t1 = dur * (i as f64 + 1.0) / n as f64;
                    let ds = (scale_prop.at(t1) - scale_prop.at(t0)).abs();
                    // one render frame's worth of this sample-to-sample delta, as a px displacement at the
                    // canvas edge (scale is a multiplier about the centre, so an edge point moves by
                    // roughly ds * pw/2 over this span; /fps-interval converts the sampled span to per-frame).
                    let span_frames = ((t1 - t0) * fps).max(1.0);
                    let px_per_frame = (ds * pw * 0.5) / span_frames;
                    max_px_per_frame = max_px_per_frame.max(px_per_frame);
                }
                if max_px_per_frame > 1e-6 {
                    let shutter = (90.0 + max_px_per_frame * 15.0).clamp(90.0, 180.0);
                    if let Err(e) = call(ss, "set_motion_blur", &json!({"clip_id": cid, "shutter_angle": shutter})) {
                        warnings.push(format!("motion blur skipped on push-in clip {cid}: {e}"));
                    }
                }
            }
        }
    }

    // 2c. V8.2V job 2 / V8.5.6: force extra splits on any uncovered dialogue stretch that would otherwise sit
    // static longer than punch_max_static -- natural-pause splitting (rough_cut, tighten_pauses) already
    // breaks up most real takes, but a continuous sentence with no breath (or a single long uncut take) can
    // leave one giant clip with nothing for add_punch_ins below to alternate against. A clip already mostly
    // covered by b-roll is left alone (nobody sees its zoom anyway, same reasoning as the visibility filter
    // just below). Default tightened 4.5 -> 3.5 in 8.5.6: the montage review (montage_v85_v05, ~9.0-16.1s)
    // found a 7.1s stretch that technically alternated wide/punched every ~1.5-3.2s already but still read
    // as a static dead zone -- a top editor's "never more than 3-4s on one setup" is tighter than 4.5s, and
    // split_points now holds every piece (including the last) to the limit exactly, not just within a slop.
    let punch_max_static = opt_num(a, "punch_max_static", 3.5).clamp(1.5, 20.0);
    if punch_on {
        let mut next_dialogue = Vec::with_capacity(dialogue_clips.len());
        for (id, s, e) in dialogue_clips.drain(..) {
            if e - s <= punch_max_static || covered_fraction(s, e, &broll_ranges) >= 0.5 { next_dialogue.push((id, s, e)); continue; }
            let points = split_points(s, e, &transcript_words, punch_max_static);
            let mut cur_id = id; let mut cur_start = s; let mut ok = true;
            for t in &points {
                match call(ss, "split_clip", &json!({"clip_id": cur_id, "at": t})) {
                    Ok(v) => { let right = v["right_clip_id"].as_str().unwrap_or_default().to_string();
                        next_dialogue.push((cur_id, cur_start, *t)); cur_id = right; cur_start = *t; }
                    Err(e2) => { warnings.push(format!("long-take split skipped on clip {cur_id}: {e2}")); ok = false; break; }
                }
            }
            if !ok && cur_start == s { next_dialogue.push((cur_id, s, e)); continue; }
            next_dialogue.push((cur_id, cur_start, e));
        }
        dialogue_clips = next_dialogue;
    }

    // 3. Punch-ins (jump-cut zoom variation) only on the dialogue clips that stay mostly on screen -- a
    // clip b-roll already covers for most of its length would animate a zoom nobody ever sees.
    if punch_on {
        let visible: Vec<String> = dialogue_clips.iter().filter(|(_, s, e)| covered_fraction(*s, *e, &broll_ranges) < 0.5).map(|(id, _, _)| id.clone()).collect();
        if visible.len() >= 2 {
            // 8.5.6: hard punch (ease 0), not an eased push-in -- a quick push-in reads as the shot drifting,
            // where a hard jump-cut-on-phrase scale change is what a demanding editor's "alternate 1.0x /
            // 1.12-1.18x" actually looks like (the subtlety was part of why the 9-16s stretch read as static).
            let pin = call(ss, "add_punch_ins", &json!({"clip_ids": visible, "scale": punch_scale, "pattern": "alternate", "ease": 0.0, "variety": true}))?;
            stages.insert("punch_ins".into(), pin);
        } else {
            warnings.push(format!("punch-ins skipped: only {} dialogue clip(s) stay on screen past the b-roll coverage, not enough for wide/tight variation", visible.len()));
        }
    }

    // 4. Captions: word-synced, from the transcript taken in step 1b (same words b-roll relevance used --
    // one recognizer pass for the whole edit, not two).
    if captions_on {
        let mut cap_args = json!({"style": caption_style, "position": caption_position});
        if !transcript_words.is_empty() { cap_args["words"] = json!(transcript_words); }
        else { cap_args["clip_ids"] = json!(dialogue_clips.iter().map(|(id, _, _)| id).collect::<Vec<_>>()); }
        let cap = call(ss, "auto_captions", &cap_args)?;
        if let Some(ws) = cap["warnings"].as_array() { for w in ws { if let Some(s) = w.as_str() { warnings.push(s.to_string()); } } }
        stages.insert("captions".into(), cap);
    }

    // 5. Cold-open hook title over the first few seconds -- the thing a top editor never skips and an agent
    // handed raw clips usually forgets. Only when the caller gives the line: guessing invented copy nobody
    // wrote would be putting words in the creator's mouth, so with no hook_text this stage is honestly skipped.
    if let Some(text) = hook_text {
        let tt = new_track(ss)?;
        let dur = edit_duration.min(3.0).max(1.0);
        let mut title = call(ss, "add_title", &json!({"track_id": tt, "style": hook_style, "text": text, "start": 0.0, "duration": dur}))?;
        if let Some(ws) = title["warnings"].as_array() { for w in ws { if let Some(s) = w.as_str() { warnings.push(s.to_string()); } } }
        // V7.9Q: the same subtle whoosh as a b-roll cut-in, on the title's own entrance -- on by default.
        // V7.9Q: default false, same reasoning as place_broll's own whoosh_sfx -- see director_tools.rs.
        if whoosh_sfx.unwrap_or(false) {
            let mut wt: Option<String> = None; let mut wa: Option<String> = None; let mut wwarn = vec![];
            if let Some(cid) = crate::ext::director_tools::place_whoosh(ss, &mut wt, &mut wa, 0.0, &mut wwarn) { title["whoosh_sfx_clip_id"] = json!(cid); }
            warnings.extend(wwarn);
        }
        stages.insert("hook_title".into(), title);
    } else {
        warnings.push("no hook_text given: the edit opens on the raw first clip with no hook title (pass hook_text for a cold-open line)".into());
    }

    // 6. Music bed. finish_audio (next) sets its overall level against the dialogue; duck_timeline (after
    // that) layers a dynamic envelope on top through duck_gain, a SEPARATE multiplier from volume -- the two
    // never fight. Without this the bed sits at one flat level the whole edit: no swell in the gaps, no real
    // give-and-take with the voice, the "lifeless" mix a static level-match alone produces.
    //
    // V8.0Q: the music bed's length targets the FULL planned export (end screen included, when end_screen_on
    // -- end_screen_duration is already resolved above, before this stage runs), not just the talking-head
    // span. Before this, every default-on end screen landed on true digital silence: the dialogue track has
    // no clip past edit_duration and the end screen's own background clip is explicitly muted, so the
    // subscribe card played over nothing. A short fade_out on the music clip's own tail (set_audio) means it
    // winds down instead of hard-cutting at export's edge, whether that edge lands mid-card or (no end
    // screen) right at the last word.
    let planned_total_duration = edit_duration + if end_screen_on { end_screen_duration } else { 0.0 };
    let mut music_track_id: Option<String> = None;
    if let Some(mid) = &music_id {
        let mdur = ss.project.assets.get(mid).map(|x| x.duration).unwrap_or(0.0);
        if mdur <= 0.0 { warnings.push(format!("music_asset_id '{mid}' has no measurable duration; music bed skipped")); }
        else {
            let mt = new_track(ss)?;
            // A music asset shorter than the full planned export (talking head + end screen) is tiled from
            // its own start -- a real editor loops a short bed rather than letting it run out under the
            // card -- capped at 200 repeats so a near-zero-length asset can't spin this into thousands of
            // clips; past that cap it plays once, honestly, and the warning below says why the tail is bare.
            const MAX_LOOP_SEGMENTS: usize = 200;
            let segment_count = if mdur >= planned_total_duration { 1 } else { ((planned_total_duration / mdur).ceil() as usize).max(1).min(MAX_LOOP_SEGMENTS) };
            let loop_fade = 0.15_f64.min(mdur * 0.1);
            let mut segments: Vec<(String, f64)> = vec![]; // (clip_id, this segment's own length)
            let mut t = 0.0;
            for i in 0..segment_count {
                let remaining = planned_total_duration - t;
                if remaining <= 1e-6 { break; }
                let seg_len = remaining.min(mdur);
                let clip = call(ss, "add_clip", &json!({"track_id": mt, "source_type": "media", "asset_id": mid, "start": t, "duration": seg_len, "in_point": 0.0}))?;
                if i == 0 { stages.insert("music".into(), clip.clone()); }
                if let Some(cid) = clip["clip_id"].as_str() { segments.push((cid.to_string(), seg_len)); }
                t += seg_len;
            }
            let covered = t;
            let n = segments.len();
            for (i, (cid, seg_len)) in segments.iter().enumerate() {
                let is_last = i + 1 == n;
                // Every loop seam gets a short fade_out/fade_in so a bed shorter than the edit blends
                // instead of popping at each repeat; the true tail (the last segment's own end) instead
                // winds all the way down to silence over up to 0.5s, matching the end card's own fade.
                let fade_out = if is_last { 0.5_f64.min(*seg_len) } else { loop_fade.min(*seg_len * 0.5) };
                let fade_in = if i > 0 { loop_fade.min(*seg_len * 0.5) } else { 0.0 };
                let mut set = json!({"clip_id": cid, "fade_out": fade_out});
                if fade_in > 0.0 { set["fade_in"] = json!(fade_in); }
                call(ss, "set_audio", &set)?;
            }
            if end_screen_on && covered + 0.05 < planned_total_duration {
                warnings.push(format!("music_asset_id '{mid}' ({mdur:.1}s) could not be looped enough times (capped at {MAX_LOOP_SEGMENTS}) to cover the edit plus end screen ({planned_total_duration:.1}s); the last {:.1}s of the end card plays without music under it", planned_total_duration - covered));
            }
            music_track_id = Some(mt);
        }
    } else if end_screen_on {
        warnings.push("no music_asset_id given: the end screen plays over whatever is left of the dialogue track's own room tone (silence once speech ends) -- pass a music_asset_id so the card doesn't land on dead air".into());
    }

    // 7. Color: neutralise cast / fix exposure on every clip auto_grade can read (talking head + b-roll).
    if grade_on {
        let grade = call(ss, "auto_grade", &json!({}))?;
        stages.insert("grade".into(), grade);
    }

    // 7b. B-roll color/exposure match to the talking head: auto_grade (just above) only pulls every clip
    // toward one fixed absolute target, so two shots from different light still clash after it runs. This
    // runs auto_grade's primaries grade as the starting point and nudges it further, toward the dialogue's
    // own measured look specifically -- the match a real cut needs, not just "neither shot is blown out".
    if broll_match_on && !broll_clip_ids.is_empty() {
        let reference_clip_ids: Vec<String> = dialogue_clips.iter().map(|(id, _, _)| id.clone()).collect();
        let match_args = json!({"reference_clip_ids": reference_clip_ids, "target_clip_ids": broll_clip_ids});
        match call(ss, "match_shot_look", &match_args) {
            Ok(m) => { stages.insert("broll_color_match".into(), m); }
            Err(e) => warnings.push(format!("b-roll color match skipped: {e}")),
        }
    }

    // 8. Final mix: clean dialogue, b-roll sound off, music under speech, mix to the loudness target.
    let mut finish_args = json!({"target_lufs": target_lufs});
    if let Some(v) = level_dynamics { finish_args["level_dynamics"] = json!(v); }
    if let Some(v) = level_strength { finish_args["level_strength"] = json!(v); }
    if let Some(v) = match_strength { finish_args["match_strength"] = json!(v); }
    if let Some(v) = compress_strength { finish_args["compress_strength"] = json!(v); }
    let audio = call(ss, "finish_audio", &finish_args)?;
    stages.insert("audio".into(), audio.clone());

    // 8b. Dynamic ducking on top of finish_audio's static levels: the music bed actually swells in the gaps
    // and comes back down under speech, instead of sitting at one flat level for the whole edit.
    if let Some(mt) = &music_track_id {
        match call(ss, "duck_timeline", &json!({"dialogue_tracks": [track_id.clone()], "music_tracks": [mt]})) {
            Ok(duck) => { stages.insert("duck".into(), duck); }
            Err(e) => warnings.push(format!("music ducking skipped: {e}")),
        }
    }

    // 8c. End screen: a short, tasteful outro card after the edit itself -- the subscribe/next-video beat a
    // top editor never skips and an agent handed raw clips usually forgets entirely. Uses add_title's own
    // subscribe_pop preset (a pill badge with a dot and text that pops in with a bounce, already built and
    // tested elsewhere -- nothing new to draw here) placed right after the dialogue ends, then a plain
    // opacity keyframe fades the whole card out in its last half second so the edit doesn't just cut away
    // from it. total_duration (not duration, which stays the talking-head length for anyone already reading
    // it) reports the real export length once this card is added.
    let mut total_duration = edit_duration;
    if end_screen_on {
        // Background: a blurred, darkened freeze of the video's own last frame, not flat black -- the card
        // reads as the edit's own outro, not a slide bolted on after it. Reuses freeze_frame (whole-clip
        // freeze on the frame shown at a given time) and add_effect's existing blur, on a brand new clip on
        // its own track so the real last dialogue clip is never touched.
        if let Some((last_asset, last_in, last_dur)) = ss.project.tracks.iter().find(|t| t.id == track_id)
            .and_then(|t| t.clips.last())
            .and_then(|c| if let Source::Media { asset } = &c.source { Some((asset.clone(), c.in_point, c.duration)) } else { None }) {
            let bg_tt = new_track(ss)?;
            let proj_fps = if ss.project.fps > 0.0 { ss.project.fps } else { 30.0 };
            // V8.5.5: the literal last frame of a talking-head take is usually mid-gesture, mouth open --
            // the worst possible frame to freeze and blur for a background. Stepping back about half a
            // second lands on a calmer moment far more often, cheaply (no pose/motion data to pick a true
            // rest frame exists in this codebase yet), clamped so a clip shorter than that just uses its
            // own first available frame instead of going negative into the previous clip.
            let last_source_t = (last_in + last_dur - 1.0 / proj_fps - END_CARD_BG_FREEZE_BACK_S).max(last_in);
            let bg = call(ss, "add_clip", &json!({"track_id": bg_tt, "source_type": "media", "asset_id": last_asset, "start": edit_duration, "duration": end_screen_duration, "in_point": last_source_t, "fit": "cover"}))?;
            if let Some(bg_cid) = bg["clip_id"].as_str() {
                call(ss, "freeze_frame", &json!({"clip_id": bg_cid, "at": edit_duration}))?;
                call(ss, "set_audio", &json!({"clip_id": bg_cid, "mute": true}))?;
                // Scaled up before the blur so the blur's own edge falloff (which softens toward
                // transparent/black past the clip's own bounds) never shows as a visible seam at the
                // canvas edge once "cover" framing has already filled the frame.
                // V8.5.8: a slow, eased push-in across the whole card instead of one flat scale -- two
                // keyframes (one segment, in_out eased) so the drift is smooth and continuous with no
                // jumps, reading as a deliberate Ken Burns move on a designed background rather than a
                // stuck frame. set_property still stamps the same starting value onto the prop's own
                // `base` first (unchanged from 8.5.5) so anything reading the base before the keys land
                // sees the card's starting scale, not a stale default.
                call(ss, "set_property", &json!({"clip_id": bg_cid, "property": "scale", "value": END_CARD_BG_SCALE}))?;
                call(ss, "set_keyframe", &json!({"clip_id": bg_cid, "property": "scale", "time": 0.0, "value": END_CARD_BG_SCALE, "ease": "in_out"}))?;
                call(ss, "set_keyframe", &json!({"clip_id": bg_cid, "property": "scale", "time": end_screen_duration, "value": END_CARD_BG_SCALE_END, "ease": "in_out"}))?;
                call(ss, "add_effect", &json!({"clip_id": bg_cid, "kind": "blur", "params": {"radius": END_CARD_BG_BLUR_RADIUS}}))?;
                // A touch darker so the white pill/text/dot pop a little more against whatever was on
                // screen, without going so dark the background reads as flat black.
                call(ss, "add_effect", &json!({"clip_id": bg_cid, "kind": "exposure", "params": {"stops": END_CARD_BG_DARKEN_STOPS}}))?;
                // a dark scrim over the blurred freeze so white pill/text/dot stay legible against whatever
                // was on screen (a bright shirt, a window) rather than fighting it for contrast.
                let scrim = call(ss, "add_shape", &json!({"track_id": bg_tt, "shape": "rect", "start": edit_duration, "duration": end_screen_duration, "width": ss.project.width as f64 * 1.2, "height": ss.project.height as f64 * 1.2, "fill": [0.0, 0.0, 0.0]}))?;
                if let Some(scrim_cid) = scrim["clip_id"].as_str() { call(ss, "set_property", &json!({"clip_id": scrim_cid, "property": "opacity", "value": 0.45}))?; }
            }
        } else {
            warnings.push("end screen: could not read the last dialogue frame for its background, falling back to plain black".into());
        }
        let tt = new_track(ss)?;
        let title = call(ss, "add_title", &json!({"track_id": tt, "style": "subscribe_pop", "text": end_screen_text, "start": edit_duration, "duration": end_screen_duration}))?;
        if let Some(ws) = title["warnings"].as_array() { for w in ws { if let Some(s) = w.as_str() { warnings.push(s.to_string()); } } }
        // V8.5.8: grow the whole subscribe_pop badge (pill + dot + text) to END_CARD_PILL_SCALE. subscribe_pop
        // pushes its clips in a fixed order (titles.rs: pill, dot, text), so clip_ids[0]/[1]/[2] are known; read
        // each clip's OWN already-built size/font back (rather than re-deriving k from scratch) so this stays
        // correct even if titles.rs's own base sizes ever change, then rebuild the pill width and the dot's
        // room/position with the SAME shared formulas titles.rs itself uses (pill_width_for_text, dot_room,
        // dot_x_for_pill) at a proportionally scaled k -- every one of those is parametrized entirely by k, so
        // scaling k by the same factor as the text/dot reproduces the identical padding and dot-gap ratio the
        // 8.5.5 geometry guarantees, just bigger, instead of a plain uniform `scale` transform that would leave
        // the dot's px-pinned clearance (dot_x_for_pill's `14.0 * k`) out of step with the grown pill.
        let pill_ids: Vec<&str> = title["clip_ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
        if pill_ids.len() == 3 {
            let (pill_id, dot_id, text_id) = (pill_ids[0], pill_ids[1], pill_ids[2]);
            let (old_font, old_size) = match ss.project.find_clip(text_id).map(|c| c.source.clone()) {
                Some(Source::Text { font, size, .. }) => (font, size),
                _ => (String::new(), 0.0),
            };
            let old_dot_w = match ss.project.find_clip(dot_id).map(|c| c.source.clone()) {
                Some(Source::Shape { width, .. }) => width,
                _ => 0.0,
            };
            if !old_font.is_empty() && old_size > 0.0 && old_dot_w > 0.0 {
                let k = ss.project.height.min(ss.project.width) as f64 / 1080.0;
                let k_pill = k * END_CARD_PILL_SCALE;
                let new_size = old_size * END_CARD_PILL_SCALE;
                let new_dot_w = old_dot_w * END_CARD_PILL_SCALE;
                call(ss, "set_text", &json!({"clip_id": text_id, "size": new_size}))?;
                let new_pill_w = crate::titles::pill_width_for_text(&end_screen_text, &old_font, new_size, ss.project.width as f64, k_pill)
                    + crate::titles::dot_room(new_dot_w, k_pill);
                let new_pill_h = 66.0 * k_pill;
                call(ss, "set_shape_size", &json!({"clip_id": pill_id, "width": new_pill_w, "height": new_pill_h}))?;
                call(ss, "set_shape_size", &json!({"clip_id": dot_id, "width": new_dot_w, "height": new_dot_w}))?;
                let new_dot_x = crate::titles::dot_x_for_pill(new_pill_w, new_dot_w, k_pill);
                call(ss, "set_property", &json!({"clip_id": dot_id, "property": "x", "value": new_dot_x}))?;
            } else {
                warnings.push("end screen: could not read the subscribe_pop badge's own text/dot size back, leaving it at its default (unscaled) size".into());
            }
        }
        if let Some(cid) = title["clip_ids"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()) {
            let fade_start = (end_screen_duration - 0.5).max(0.0);
            call(ss, "set_keyframe", &json!({"clip_id": cid, "property": "opacity", "time": fade_start, "value": 1.0}))?;
            call(ss, "set_keyframe", &json!({"clip_id": cid, "property": "opacity", "time": end_screen_duration, "value": 0.0}))?;
        }
        total_duration = edit_duration + end_screen_duration;
        stages.insert("end_screen".into(), title);
    }

    // 9. Let the edit catch its own remaining mistakes: lint_edit's own checks, auto-applying whatever has
    // a concrete fix (no guessing at anything that needs a human call). apply_review_fixes re-checks with a
    // fresh lint_edit pass after applying anything, so a fix that solves one problem and creates a new one
    // (the real bug this chain exists to prevent -- a caption fix landing on the hook title) is caught here
    // rather than silently shipped.
    let fixes = call(ss, "apply_review_fixes", &json!({"platform": platform}))?;
    let after = if fixes["lint_edit_after"].is_object() { &fixes["lint_edit_after"] } else { &fixes["lint_edit"] };
    let lint_ok = after["ok"].as_bool().unwrap_or(true);
    let applied_keys: Vec<(Option<&str>, Option<&str>)> = fixes["applied"].as_array().map(|v| v.iter().map(|f| (f["code"].as_str(), f["clip_id"].as_str())).collect()).unwrap_or_default();
    let mut remaining: Vec<Value> = after["problems"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|p| !applied_keys.contains(&(p["code"].as_str(), p.get("clip_id").and_then(|v| v.as_str())))).collect();
    if let Some(introduced) = fixes["introduced_problems"].as_array() {
        for p in introduced { if !remaining.iter().any(|r| r["code"] == p["code"] && r["clip_id"] == p["clip_id"]) { remaining.push(p.clone()); } }
        if !introduced.is_empty() { warnings.push(format!("a fix introduced {} new problem(s) of its own (see remaining_problems) -- not auto-chased, worth a look", introduced.len())); }
    }
    stages.insert("review".into(), fixes);

    // 10. End-card music swell: once the dialogue ends there is nothing left for the music to duck under,
    // but finish_audio/duck_timeline both only ever set the bed's level for USE UNDER SPEECH -- measured on
    // real_v21, the end card's own span played ~12.6 LU quieter than the speech body (no voice to protect
    // there, so there is nothing pulling the bed back up). A short ramp (starting right where the dialogue
    // ends, finishing before the card is even readable) brings the bed up to a normal "voice is gone, let
    // the music breathe" level; the segment's own tail fade_out (job 1, set when the music clip was built
    // in step 6) still takes it back down to silence in its last <=0.5s, unchanged by this. Runs LAST, after
    // apply_review_fixes (step 9): a review fix that re-invokes finish_audio replaces a clip's whole volume
    // Prop (Prop::new, dropping any keys) rather than merging into it, so a swell keyed any earlier in the
    // pipeline could be silently wiped by the review pass -- this is the one stage guaranteed to survive.
    //
    // V8.1Q correction (second pass): the V8.0Q note below was wrong about the mechanism, and the swell
    // amount itself was never actually the problem. level_dynamics is now properly scoped to the dialogue
    // span (master_speech_level_scope_end, set by finish_audio, read by audio::mix/mix_strict) and genuinely
    // never sees the end card. The whisper-recall benchmark's own whole-file measurement was the real source
    // of the old "56.7% collapse" -- fixed in examples/auto_edit_benchmark.py (dual_span_recall), which now
    // scores dialogue-span recall (what finish_audio's dialogue-vs-ambience classifier is actually being
    // gated on) separately from a whole-file number, after also fixing a second artifact that crop
    // introduced (whisper drops a clip's opening sentence with no lead-in silence; the benchmark now pads
    // 0.5s before transcribing the cropped span). With both of those real bugs fixed: 8dB measured
    // dialogue-span recall 90.0% but end-card LUFS only -21.4 (short of the -18..-20 ask); 11dB measured
    // dialogue-span recall 90.0% AND full-file recall 91.7% (whisper's whole-file read of this particular
    // render came out fine at 11dB even though it didn't at 8dB -- that whole-file number is noisy near
    // its own cliff, which is exactly why dialogue-span is the real gate now) with end-card LUFS at -18.4,
    // inside the asked -18..-20 range. Render: /home/user/tab/files/v7q/real_v34.mp4.
    const END_CARD_SWELL_DB: f64 = 11.0;
    const END_CARD_SWELL_RAMP_S: f64 = 0.3;
    if end_screen_on {
        if let Some(mt) = &music_track_id {
            let boost = 10f64.powf(END_CARD_SWELL_DB / 20.0);
            let clips: Vec<(String, f64, f64)> = ss.project.tracks.iter().find(|t| &t.id == mt).map(|t| t.clips.iter().map(|c| (c.id.clone(), c.start, c.duration)).collect()).unwrap_or_default();
            for (cid, cstart, cdur) in clips {
                let cend = cstart + cdur;
                if cend <= edit_duration + 1e-6 { continue; } // this segment ends before the card starts; nothing to swell
                let local_start = (edit_duration - cstart).max(0.0);
                let local_ramp_end = (local_start + END_CARD_SWELL_RAMP_S).min(cdur);
                let base = ss.project.find_clip(&cid).map(|c| c.audio.volume.at(local_start)).unwrap_or(1.0);
                call(ss, "set_audio_volume_key", &json!({"clip_id": cid, "time": r3(local_start), "value": r3(base)}))?;
                if local_ramp_end > local_start + 1e-6 {
                    call(ss, "set_audio_volume_key", &json!({"clip_id": cid, "time": r3(local_ramp_end), "value": r3(base * boost)}))?;
                }
            }
        }
    }

    // V8.7.0: turn on the picture-quality render option for the finished project (see linear_compositing_on
    // above) -- last step, after every stage above has already run against the ordinary display-referred
    // frames it has always read. project_set's linear_blend (a fast, compositing-only option -- see
    // render.rs::blend_region_opt) is a plain project field, not the heavier set_color_pipeline (ACEScg
    // working space, 16-bit decode, HDR): that measured ~3.4x slower on this exact benchmark (144.7s vs
    // 42.4s at 720p, see CHANGELOG v8.7.0), well past "doesn't tank speed" -- the audit's measured 188/255
    // crossfade midpoint comes just as correctly from the lighter option.
    if linear_compositing_on {
        call(ss, "project_set", &json!({"linear_blend": true}))?;
    }
    // V8.8.0: see encoder_quality_on's comment above. "slow" is the chosen preset (CHANGELOG.md v8.8.0).
    if encoder_quality_on {
        call(ss, "project_set", &json!({"x264_preset": AUTO_EDIT_X264_PRESET}))?;
    }
    // V8.8.1: see color_tag_sanity_on's comment above.
    if color_tag_sanity_on {
        call(ss, "project_set", &json!({"color_tag_sanity": true}))?;
    }

    Ok(json!({
        "ok": true, "track_id": track_id, "duration": edit_duration, "total_duration": total_duration, "mix_lufs": audio["mix_lufs"],
        "stages": stages, "warnings": warnings, "lint_clean": lint_ok, "remaining_problems": remaining,
        "linear_compositing": linear_compositing_on,
        "encoder_quality": encoder_quality_on,
        "color_tag_sanity": color_tag_sanity_on,
        "motion_blur": motion_blur_on,
        "stabilize_handheld": stabilize_handheld_on,
        "next": "export when ready; self_review or review_export to look at it first",
    }))
}

pub fn commands() -> Vec<Command> {
    let ids = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    vec![
        Command { name: "auto_edit", description: "One call, raw clips to a finished edit. Chains rough cut (dead air out), b-roll, jump-cut punch-ins (only where the face stays on screen), word-synced captions, an optional hook title and music bed, auto color, a final mix to target loudness, and lint_edit's own auto-fixes -- the ordering (b-roll before punch-ins) keeps a zoom from landing on a clip nothing shows, which is the mistake this exists to prevent.", mutates: true,
            schema: obj(vec![
                ("talking_head_asset_ids", ids("required: talking-head assets, in story order (import_media first)")),
                ("broll_asset_ids", ids("optional: b-roll cutaways, in order")),
                ("music_asset_id", s("optional: an imported audio-only asset for a music bed, ducked under speech")),
                ("hook_text", s("optional: a cold-open hook line for the first few seconds; omit to skip the hook honestly rather than invent one")),
                ("hook_style", s("add_title style for the hook, default hook_box")),
                ("caption_style", s("auto_captions style: pop (default)|karaoke|clean|boxed|impact")),
                ("caption_position", s("auto_captions position: auto (default, avoids the face when a safe band can be found)|bottom|middle|top")),
                ("captions", json!({"type": "boolean", "description": "default true"})),
                ("punch_ins", json!({"type": "boolean", "description": "default true"})),
                ("punch_scale", n("punched-in zoom over wide, default 1.15")),
                ("punch_max_static", n("force an extra split so no dialogue stretch sits static past this many seconds, default 3.5, clamped 1.5..20")),
                ("push_in", json!({"type": "boolean", "description": "default true: slow Ken Burns push-in on static b-roll cutaways"})),
                ("push_in_zoom", n("push-in target scale by the cutaway's end, default 1.05 (5%), clamped 1.01..1.3")),
                ("broll_transitions", json!({"type": "boolean", "description": "default true: a short whip/zoom-blur transition into each b-roll cutaway (never on the talking-head punch-ins)"})),
                ("broll_transition_duration", n("b-roll whip transition length, seconds, default 0.25, clamped 0.1..0.6")),
                ("grade", json!({"type": "boolean", "description": "default true: auto_grade every clip"})),
                ("broll_color_match", json!({"type": "boolean", "description": "default true (with b-roll): match b-roll's look to the talking head's"})),
                ("target_lufs", n("final mix loudness target, default -14 (YouTube)")),
                ("platform", s("passed to lint_edit/apply_review_fixes: tiktok|reels|shorts|none (default none)")),
                ("level_dynamics", json!({"type": "boolean", "description": "passed to finish_audio: gentle speech leveling, default true"})),
                ("level_strength", n("passed to finish_audio: leveling factor 1..4, default 2.5")),
                ("match_strength", n("passed to finish_audio: match_strength 0..1, default 1.0")),
                ("compress_strength", n("passed to finish_audio: compress_strength 0..1, default 0.0")),
                ("whoosh_sfx", json!({"type":"boolean","description":"default FALSE: synthesized whoosh on each b-roll cut-in + hook title -- costs whisper recall even ducked low; opt in"})),
                ("end_screen", json!({"type": "boolean", "description": "a short subscribe-style outro card after the edit; default true when platform is none"})),
                ("end_screen_text", s("outro card text, default SUBSCRIBE FOR MORE")),
                ("end_screen_duration", n("outro card seconds, default 3.0, clamped 2..4")),
                ("pacing", json!({"type": "boolean", "description": "tighten_pauses: shrink the dead air rough_cut leaves inside its own kept clips (a breath between phrases, the lead-in before the first word) down to a natural pad; default true"})),
                ("linear_compositing", json!({"type": "boolean", "description": "default true: linear-light blend, lanczos fit, dither"})),
                ("encoder_quality", json!({"type": "boolean", "description": "default true: an explicit x264 preset (slow) for the final export, smaller and higher-SSIM than ffmpeg's own default at the same crf"})),
                ("color_tag_sanity", json!({"type": "boolean", "description": "default true: decode mistagged/untagged HD source footage as bt709 instead of trusting an incomplete SD tag"})),
                ("motion_blur", json!({"type": "boolean", "description": "default true: shutter-angle motion blur (90-180 deg) on the push-in Ken Burns ramp, proportional to how fast it actually moves (never on punch-ins: those are hard instant cuts with nothing to blur)"})),
                ("stabilize_handheld", json!({"type": "boolean", "description": "default FALSE: shake-detect each b-roll cutaway and warp-stabilize it only when measured shake clears stabilize_shake_threshold_px, leaving locked-off shots untouched -- off by default because even the cheap shake scan costs real time per clip (see CHANGELOG.md v8.9.0); never applied to the talking-head track (would fight add_punch_ins for the same x/y/scale props)"})),
                ("stabilize_shake_threshold_px", n("max_correction_px above which a b-roll clip counts as handheld and gets warp-stabilized, default 1.5, clamped 0.1..50 (only read when stabilize_handheld is true)")),
            ], &["talking_head_asset_ids"]), run: |ss, a| atomic(ss, a, auto_edit) },
    ]
}

#[cfg(test)]
mod split_points_tests {
    use super::split_points;
    use serde_json::json;

    fn word(start: f64, end: f64) -> serde_json::Value { json!({"start": start, "end": end, "src": "w"}) }

    /// 8.5.6: a synthetic long talking-head run with no transcript at all (the evenly-spaced fallback) must
    /// still come out with every piece -- including the last one -- at or under the ~3.5s target, with
    /// headroom to spare under the review's "never static past ~3-4s" bar (checked at 3.6s).
    #[test]
    fn split_points_never_leaves_a_span_over_3_6s_with_no_words() {
        let points = split_points(0.0, 12.0, &[], 3.5);
        let mut bounds = vec![0.0]; bounds.extend(points.iter().copied()); bounds.push(12.0);
        for w in bounds.windows(2) {
            assert!(w[1] - w[0] <= 3.6, "span {:.2}..{:.2} is {:.2}s, over the 3.6s bar: {bounds:?}", w[0], w[1], w[1] - w[0]);
        }
        assert!(points.len() >= 2, "a 12s take at a 3.5s cap needs at least 2 forced splits, got {}: {points:?}", points.len());
    }

    /// Same bound holds on an odd duration that does not divide evenly by max_static (a real take never
    /// lands on a clean multiple), proving the guarantee is the loop's own invariant, not a lucky number.
    #[test]
    fn split_points_never_leaves_a_span_over_3_6s_on_an_odd_duration() {
        for &dur in &[5.1, 9.9, 13.37, 20.0, 3.4, 3.6] {
            let points = split_points(0.0, dur, &[], 3.5);
            let mut bounds = vec![0.0]; bounds.extend(points.iter().copied()); bounds.push(dur);
            for w in bounds.windows(2) {
                assert!(w[1] - w[0] <= 3.6, "duration {dur}: span {:.2}..{:.2} is {:.2}s, over the 3.6s bar: {bounds:?}", w[0], w[1], w[1] - w[0]);
            }
        }
    }

    /// A real pause close to the ~max_static mark is preferred over the raw interval point, so the cut lands
    /// between words, not mid-word -- the whole reason job 2 looks at the transcript before falling back.
    #[test]
    fn split_points_prefers_a_pause_near_the_mark_over_the_raw_interval() {
        // clip 0..8, max_static 3.5: the raw interval point would be 3.5, but there is a real pause at 3.3
        // (a 0.2s gap between two words) just before it -- that is what should get picked instead.
        let words = vec![word(1.0, 3.1), word(3.3, 4.0), word(4.0, 7.5)];
        let points = split_points(0.0, 8.0, &words, 3.5);
        assert!(!points.is_empty());
        assert!((points[0] - 3.3).abs() < 1e-9, "expected the real pause at 3.3, got {:?}", points);
    }

    /// 8.5.6 regression: the old job-2 window let a pause up to max_static*1.3 past the last split get
    /// picked, which could itself push a segment past the limit (the exact slack the montage review's
    /// static stretch rode in on). A pause that far out must now be ignored in favour of the plain interval
    /// point, so the segment it closes still respects max_static.
    #[test]
    fn split_points_ignores_a_pause_past_the_max_static_mark() {
        // clip 0..10, max_static 3.5: a pause at 4.6 is well past the 3.5 mark (within the OLD 1.3x = 4.55
        // slack window) and must be rejected; the only pause inside (0, 3.5] is none, so the plain interval
        // point (3.5) must be used instead.
        let words = vec![word(1.0, 4.5), word(4.6, 9.9)];
        let points = split_points(0.0, 10.0, &words, 3.5);
        assert!(!points.is_empty(), "expected at least one forced split: {points:?}");
        assert!(points[0] <= 3.5 + 1e-9, "picked {:.2}, which exceeds max_static (3.5) -- the old 1.3x slack bug: {points:?}", points[0]);
    }

    /// A take already short enough needs no forced split at all (job 2 does not force work that is not
    /// needed) -- the companion fact to the "never over the cap" guarantee above.
    #[test]
    fn split_points_does_nothing_on_a_short_clip() {
        assert!(split_points(0.0, 3.0, &[], 3.5).is_empty());
        assert!(split_points(0.0, 3.5, &[], 3.5).is_empty(), "exactly at the cap needs no split");
    }
}
