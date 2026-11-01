//! Montage tools (V8.3-M): the non-dialogue sibling of auto_rough_cut / auto_edit. auto_edit and auto_rough_cut are built
//! entirely around talking_head_asset_ids -- auto_edit hard-errors with no speech ("auto_rough_cut kept no speech from
//! talking_head_asset_ids") and there is no call that takes a folder of raw silent/ambient clips (travel, sports, pets,
//! phone footage) and picks a good edit out of them. This file is that path: score each clip's own windows on an absolute,
//! cross-clip-comparable scale (sharpness, exposure, subject/saliency framing, motion and camera shake), then keep only
//! the strongest windows -- the montage equivalent of auto_rough_cut cutting dead air, except what gets cut here is
//! blurry/dark/shaky junk instead of silence.
//!
//! Reuses, not rebuilds: sharpness is focus_series' own Laplacian-variance reader (score_plane/luma_plane), exposure is
//! frame_quality's own quality() (crushed_frac/clipped_frac, already absolute 0..1, no re-normalization needed), subject
//! framing is pick_thumbnail's own subject_score (exposed pub(crate) here instead of being re-derived), and motion is
//! frozen_frames' own max_diff between a window's first and last rendered frame. Shake is the one new number: stabilize.rs
//! already computes, for its own warp-stabilize tool, exactly "how far the raw camera path strays from a smoothed one"
//! (stabilize_clip's own max_correction_px/max_correction_deg, used there to report how much correction a clip needed) --
//! that same read-only call (stabilize_clip never mutates; write_keys is the separate step that does) is reused here as
//! the shake number. Scoped per whole source clip, not per sub-window (stabilize_clip takes a clip's full duration): a
//! clip that is shaky is almost always shaky throughout a handheld take, so a per-clip gate is the honest scope for the
//! time this pass had, documented rather than faked at finer granularity.
use super::kit::*;
use crate::commands::dispatch_nested as call;
use crate::model::Source;
use crate::perception::analysis::{self, Request};

fn r6(x: f64) -> f64 { (x * 1e6).round() / 1e6 }
fn fps(ss: &Session) -> f64 { if ss.project.fps > 0.0 { ss.project.fps } else { 30.0 } }
fn snap(t: f64, fps: f64) -> f64 { (t * fps).round() / fps }
fn str_list(a: &Value, k: &str) -> Result<Vec<String>, String> {
    let v = a.get(k).and_then(|v| v.as_array()).ok_or_else(|| format!("{k}: an array of clip ids is required, e.g. [\"clip1\", \"clip2\"]"))?;
    let out: Vec<String> = v.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    if out.is_empty() || out.len() != v.len() { return Err(format!("{k}: give one or more clip id strings")); }
    Ok(out)
}
fn num(a: &Value, k: &str, d: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match a.get(k) { None | Some(Value::Null) => Ok(d), Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= lo && *x <= hi).ok_or(format!("out_of_range: {k} must be a number in [{lo}, {hi}]")) }
}

/// Shake for a whole clip: stabilize_clip's own read-only path estimate (StabilizeOnly framing, Similarity, Smooth --
/// crop/zoom never computed, so this is the cheapest honest call), max_correction_px/deg straight from its report.
/// None (not an error) for a clip too short to stabilize (< 2 project frames) -- callers treat an absent shake as 0
/// (nothing to disprove) rather than failing the whole scoring pass over one short clip.
fn clip_shake(ss: &Session, clip_id: &str) -> Option<(f64, f64)> {
    let o = crate::stabilize::Opts { method: crate::stabilize::Method::Similarity, mode: crate::stabilize::Mode::Smooth, smoothing: crate::stabilize::Smoothing::Gaussian,
        smoothness: 50.0, framing: crate::stabilize::Framing::StabilizeOnly, max_features: 400, ransac_px: 1.0, seed: 0x5eed, fps: ss.project.fps.max(1.0),
        synthesis_range: 0.3, synth_frames: 9, rolling_shutter: 0.0 };
    match crate::stabilize::stabilize_clip(&ss.project, clip_id, &o) {
        Ok((_, rep)) => Some((rep["max_correction_px"].as_f64().unwrap_or(0.0), rep["max_correction_deg"].as_f64().unwrap_or(0.0))),
        Err(_) => None,
    }
}

struct Win { clip_id: String, asset_id: Option<String>, start: f64, end: f64, lap_var: f64, crushed: f64, clipped: f64, subject: f64, motion: f64, shake_px: f64, shake_deg: f64 }

/// Sample every clip_id into window_s windows (its own span, not snapped past the clip's own bounds) and score each one
/// on an absolute, cross-clip scale: a window from clip A and a window from clip B are directly comparable, unlike
/// pick_thumbnail's per-call top-k (built for "best frame of THIS clip", not "rank windows across many clips").
fn score(ss: &mut Session, a: &Value) -> Result<(Vec<Win>, f64), String> {
    let clip_ids = str_list(a, "clip_ids")?;
    let window_s = num(a, "window_s", 1.0, 0.2, 15.0)?;
    let fpsv = fps(ss);
    let mut wins = vec![];
    let mut shake_cache: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
    for cid in &clip_ids {
        let c = ss.project.find_clip(cid).ok_or_else(|| format!("no such clip {cid}"))?.clone();
        if !matches!(c.source, Source::Media { .. }) { return Err(format!("clip {cid} is not a media clip")); }
        let asset_id = match &c.source { Source::Media { asset } => Some(asset.clone()), _ => None };
        let (shake_px, shake_deg) = *shake_cache.entry(cid.clone()).or_insert_with(|| clip_shake(ss, cid).unwrap_or((0.0, 0.0)));
        // The clip's own span, pulled in by half a frame on each side: a sample time must be strictly inside
        // [c.start, c.start + c.duration) for analysis::render, and snap() can round a boundary time (e.g. the
        // clip's own end) up onto or past the far edge by up to half a frame -- clamp every sampled time into
        // this safe interior range, after snapping, so the last window of a clip never requests an out-of-range time.
        let half_frame = 0.5 / fpsv;
        let lo = c.start + half_frame; let hi = (c.start + c.duration - half_frame).max(lo);
        let clampt = |t: f64| snap(t, fpsv).clamp(lo, hi);
        let n = ((c.duration / window_s).ceil() as usize).max(1);
        for i in 0..n {
            let ws = c.start + i as f64 * window_s;
            let we = (c.start + (i as f64 + 1.0) * window_s).min(c.start + c.duration);
            if we - ws < window_s * 0.25 && i > 0 { continue; } // a sliver shorter than a quarter-window just joins the previous one's tail, not its own window
            let mid = clampt((ws + we) / 2.0);
            let im = analysis::render(ss, &Request { time: mid, source: format!("clip:{cid}") })?;
            let y = super::focus_series::luma_plane(&im);
            let lap_var = super::focus_series::score_plane(&y, im.w, im.h, None).map(|s| s.lap_var).unwrap_or(0.0);
            let q = super::frame_quality::quality(&im, None, &super::frame_quality::QParams::default());
            let crushed = q["crushed_frac"].as_f64().unwrap_or(0.0);
            let clipped = q["clipped_frac"].as_f64().unwrap_or(0.0);
            let subject = super::pick_thumbnail::subject_score(&im);
            let im0 = analysis::render(ss, &Request { time: clampt(ws), source: format!("clip:{cid}") })?;
            let im1 = analysis::render(ss, &Request { time: clampt(we - 1.0 / fpsv), source: format!("clip:{cid}") })?;
            let motion = super::frozen_frames::max_diff(&im0.rgb, &im1.rgb);
            wins.push(Win { clip_id: cid.clone(), asset_id: asset_id.clone(), start: ws, end: we, lap_var, crushed, clipped, subject, motion, shake_px, shake_deg });
        }
    }
    if wins.is_empty() { return Err("no windows: every given clip_id produced none (check clip_ids and window_s)".into()); }
    let max_lap = wins.iter().map(|w| w.lap_var).fold(0.0f64, f64::max).max(1e-9);
    Ok((wins, max_lap))
}

/// Weights: 0.35 sharpness + 0.25 exposure + 0.15 subject-framing + 0.10 motion (some life in the shot, not frozen) +
/// 0.15 stability (1 - shake_px/SHAKE_REF, clamped) -- shake is the only hard gate besides the score floor, since a
/// shaky window is unusable regardless of how sharp or well-exposed any single sampled frame looks.
const SHAKE_REF_PX: f64 = 6.0;
fn window_score(w: &Win, max_lap: f64) -> (f64, f64, f64) {
    let sharpness = (w.lap_var / max_lap).min(1.0);
    let exposure = (1.0 - (w.crushed + w.clipped).min(1.0)).max(0.0);
    let stability = (1.0 - (w.shake_px / SHAKE_REF_PX).min(1.0)).max(0.0);
    let score = 0.35 * sharpness + 0.25 * exposure + 0.15 * w.subject + 0.10 * w.motion.min(1.0) + 0.15 * stability;
    (score, sharpness, stability)
}

/// A window is kept only when EVERY axis clears its own floor, not just the blended score -- a severely blurred window
/// can still blend to a passable score on exposure+subject+motion alone, which would defeat the point of scoring
/// sharpness at all. min_sharpness/min_exposure are the hard per-axis gates the brief's "dropping blurry/dark/shaky
/// junk" asks for; min_score is the overall bar on top of them.
fn kept_of(score: f64, sharpness: f64, exposure: f64, shake_px: f64, min_score: f64, min_sharpness: f64, min_exposure: f64, max_shake_px: f64) -> (bool, Option<String>) {
    // exposure checked before sharpness: a near-black or blown frame also reads as near-zero Laplacian variance
    // (crushed/clipped pixels carry no fine detail either), so exposure is the more honest root cause to report
    // first when both floors miss on the same window.
    if shake_px > max_shake_px { return (false, Some(format!("shake {}px over max_shake_px {}", r6(shake_px), max_shake_px))); }
    if exposure < min_exposure { return (false, Some(format!("exposure {} under min_exposure {} (too dark/blown)", r6(exposure), min_exposure))); }
    if sharpness < min_sharpness { return (false, Some(format!("sharpness {} under min_sharpness {} (too blurry)", r6(sharpness), min_sharpness))); }
    if score < min_score { return (false, Some(format!("score {} under min_score {}", r6(score), min_score))); }
    (true, None)
}

fn score_windows(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let min_score = num(a, "min_score", 0.45, 0.0, 1.0)?;
    let min_sharpness = num(a, "min_sharpness", 0.12, 0.0, 1.0)?;
    let min_exposure = num(a, "min_exposure", 0.25, 0.0, 1.0)?;
    let max_shake_px = num(a, "max_shake_px", 12.0, 0.0, 500.0)?;
    let (wins, max_lap) = score(ss, a)?;
    let out: Vec<Value> = wins.iter().map(|w| {
        let (score, sharpness, stability) = window_score(w, max_lap);
        let exposure = (1.0 - (w.crushed + w.clipped).min(1.0)).max(0.0);
        let (kept, drop_reason) = kept_of(score, sharpness, exposure, w.shake_px, min_score, min_sharpness, min_exposure, max_shake_px);
        json!({"clip_id": w.clip_id, "asset_id": w.asset_id, "start": r6(w.start), "end": r6(w.end), "kept": kept,
            "score": r6(score), "score_parts": {"sharpness": r6(sharpness), "exposure": r6(exposure), "subject_centered": r6(w.subject), "motion": r6(w.motion.min(1.0)), "stability": r6(stability)},
            "lap_var": r6(w.lap_var), "crushed_frac": r6(w.crushed), "clipped_frac": r6(w.clipped), "shake_px": r6(w.shake_px), "shake_deg": r6(w.shake_deg), "drop_reason": drop_reason})
    }).collect();
    let kept = out.iter().filter(|w| w["kept"] == json!(true)).count();
    Ok(json!({"windows": out, "window_count": out.len(), "kept_count": kept, "dropped_count": out.len() - kept, "max_lap_var_in_call": r6(max_lap),
        "min_score": min_score, "min_sharpness": min_sharpness, "min_exposure": min_exposure, "max_shake_px": max_shake_px,
        "note": "shake is scored once per whole clip_id (stabilize_clip's own path-vs-smoothed-path correction), not per window: a handheld take is almost always shaky throughout, so every window of a shaky clip shares its clip's shake_px"}))
}

/// The montage "rough cut": score_windows, then physically cut each clip down to only its kept windows (split_clip at
/// every surviving window's own boundaries, delete_clip on everything dropped) -- the direct analogue of
/// auto_rough_cut cutting dead air, except what it cuts here is blurry/dark/shaky junk instead of silence.
fn best_segments(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let min_score = num(a, "min_score", 0.45, 0.0, 1.0)?;
    let min_sharpness = num(a, "min_sharpness", 0.12, 0.0, 1.0)?;
    let min_exposure = num(a, "min_exposure", 0.25, 0.0, 1.0)?;
    let max_shake_px = num(a, "max_shake_px", 12.0, 0.0, 500.0)?;
    let (wins, max_lap) = score(ss, a)?;
    // group windows back into per-clip ordered lists, each with a kept flag
    let mut by_clip: Vec<(String, Vec<(f64, f64, bool)>)> = vec![];
    for w in &wins {
        let (score, sharpness, _) = window_score(w, max_lap);
        let exposure = (1.0 - (w.crushed + w.clipped).min(1.0)).max(0.0);
        let (kept, _) = kept_of(score, sharpness, exposure, w.shake_px, min_score, min_sharpness, min_exposure, max_shake_px);
        match by_clip.iter_mut().find(|(id, _)| id == &w.clip_id) { Some((_, v)) => v.push((w.start, w.end, kept)), None => by_clip.push((w.clip_id.clone(), vec![(w.start, w.end, kept)])) }
    }
    let fpsv = fps(ss);
    let mut kept_clips = vec![]; let mut dropped = 0usize;
    for (cid, windows) in &by_clip {
        // merge adjacent same-kept-state windows into runs so a clip with no junk is left as one piece, not re-split per window
        let mut runs: Vec<(f64, f64, bool)> = vec![];
        for &(s, e, k) in windows { match runs.last_mut() { Some(r) if r.2 == k && (s - r.1).abs() < 1e-6 => r.1 = e, _ => runs.push((s, e, k)) } }
        if runs.iter().all(|r| r.2) { kept_clips.push(json!({"clip_id": cid, "start": r6(runs[0].0), "end": r6(runs.last().unwrap().1)})); continue; }
        if runs.iter().all(|r| !r.2) { call(ss, "delete_clip", &json!({"clip_id": cid}))?; dropped += 1; continue; }
        // mixed: split at every internal run boundary, left to right, tracking the current right-hand id as split_clip hands it back
        let mut cur = cid.clone(); let mut cur_start = runs[0].0;
        for (i, r) in runs.iter().enumerate() {
            if i + 1 < runs.len() {
                let t = snap(r.1, fpsv);
                let right = call(ss, "split_clip", &json!({"clip_id": cur, "at": t}))?["right_clip_id"].as_str().ok_or("split_clip returned no right_clip_id")?.to_string();
                if r.2 { kept_clips.push(json!({"clip_id": cur, "start": r6(cur_start), "end": r6(t)})); } else { call(ss, "delete_clip", &json!({"clip_id": cur}))?; dropped += 1; }
                cur = right; cur_start = t;
            } else if r.2 { kept_clips.push(json!({"clip_id": cur, "start": r6(cur_start), "end": r6(r.1)})); } else { call(ss, "delete_clip", &json!({"clip_id": cur}))?; dropped += 1; }
        }
    }
    Ok(json!({"kept_clips": kept_clips, "kept_count": kept_clips.len(), "dropped_count": dropped, "window_count": wins.len(), "min_score": min_score, "min_sharpness": min_sharpness, "min_exposure": min_exposure, "max_shake_px": max_shake_px}))
}

pub fn commands() -> Vec<Command> { vec![
    Command { name: "score_windows", description: "Read-only: score each clip's windows for sharpness, exposure, subject framing, motion and shake, so a montage can keep only the strongest footage. Scores window_s windows (default 1.0s) across every given clip_id on an absolute, cross-clip-comparable scale: sharpness (focus_series' own lap_var, normalized against this call's own global max across every window given), exposure (frame_quality's own crushed_frac+clipped_frac, already absolute), subject framing (pick_thumbnail's own subject_score) and motion (frozen_frames' own frame-to-frame diff), plus one shake number per whole clip_id (stabilize_clip's own read-only raw-path-vs-smoothed-path correction, max_correction_px/deg -- scoped per clip, not per window, since a handheld take is shaky throughout). Returns every window with its score, score_parts, raw measurements and a kept bool (score>=min_score and shake_px<=max_shake_px); nothing is cut.", mutates: false,
        schema: obj(vec![("clip_ids", json!({"type":"array","items":{"type":"string"},"description":"clips already on the timeline to score"})),
            ("window_s", n("window length in seconds, 0.2..15, default 1.0")),
            ("min_score", n("0..1, default 0.45: the overall blended-score floor")),
            ("min_sharpness", n("0..1, default 0.12: hard floor on the sharpness axis alone (normalized lap_var) -- a window below this is dropped as too blurry even if other axes are strong")),
            ("min_exposure", n("0..1, default 0.25: hard floor on the exposure axis alone -- a window below this is dropped as too dark/blown even if other axes are strong")),
            ("max_shake_px", n("0..500, default 12.0: a clip whose stabilize_clip max_correction_px exceeds this is dropped regardless of score"))], &["clip_ids"]),
        run: score_windows },
    Command { name: "best_segments", description: "The montage rough cut: cut the blurry, dark, shaky or low-scoring windows out of raw non-dialogue footage, keeping only the strongest segments. Runs the same scoring as score_windows (same args), then actually cuts: split_clip at every kept/dropped window boundary and delete_clip on anything under min_score or over max_shake_px -- the montage-footage analogue of auto_rough_cut, which cuts dead air from talking-head audio; this cuts junk from silent b-roll/travel/sports clips instead. Returns the surviving clip ids and their timeline ranges.", mutates: true,
        schema: obj(vec![("clip_ids", json!({"type":"array","items":{"type":"string"},"description":"clips already on the timeline to cut down"})),
            ("window_s", n("window length in seconds, 0.2..15, default 1.0")),
            ("min_score", n("0..1, default 0.45")),
            ("min_sharpness", n("0..1, default 0.12")),
            ("min_exposure", n("0..1, default 0.25")),
            ("max_shake_px", n("0..500, default 12.0"))], &["clip_ids"]),
        run: best_segments },
] }
