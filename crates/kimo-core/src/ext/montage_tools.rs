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

// ---------------------------------------------------------------- beat_sync_montage (job 2)
/// Beat-synced montage cuts. beat_cut/cut_to_beat already lay clips end to end on an exact beat grid (reused here
/// verbatim -- this never re-implements cutting or clip placement); what neither does is vary shot length with the
/// music's own energy or stop on a musical phrase. This reads audio_transients' own beat-mode grid (per-beat
/// strength, already computed, no new beat tracker), walks it choosing a 1..max_beats_per_shot step at each beat
/// from that beat's own strength (louder/stronger beat -> shorter shot, quieter -> longer), hands the resulting
/// cut times straight to beat_cut as its own `beats` array (so every cut IS a real detected beat, not an
/// approximation), and stops on a beats_per_phrase-aligned beat so the edit ends on a bar line instead of
/// mid-phrase. Lays the music with add_audio_clip, normalizes it to target_lufs (batch_normalize_loudness, same
/// headroom-checked gain as everywhere else in this engine) BEFORE keying the fade so the integrated measurement
/// is not dragged down by the tail's own silence, then fades it out over fade_out_beats so the montage never just
/// stops dead on the last frame.
fn beat_energy_cuts(grid: &[(f64, f64)], idx0: usize, min_beats: usize, max_beats: usize) -> Vec<usize> {
    let max_strength = grid.iter().map(|(_, s)| *s).fold(0.0f64, f64::max).max(1e-9);
    let mut idx = idx0; let mut idxs = vec![idx];
    while idx + 1 < grid.len() {
        let energy = (grid[idx].1 / max_strength).clamp(0.0, 1.0);
        // stronger beat -> a shorter shot (cut sooner); quieter -> a longer shot. Three tiers, clamped into range.
        let step = if energy >= 0.66 { min_beats } else if energy >= 0.33 { ((min_beats + max_beats) / 2).max(min_beats) } else { max_beats };
        let step = step.clamp(min_beats, max_beats.max(min_beats));
        let next = idx + step;
        if next >= grid.len() { break; }
        idxs.push(next); idx = next;
    }
    idxs
}

fn beat_sync_montage(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let clip_ids = str_list(a, "clip_ids")?;
    let music_asset_id = str_arg(a, "music_asset_id")?.to_string();
    let beats_per_phrase = num(a, "beats_per_phrase", 4.0, 1.0, 32.0)? as usize;
    let min_beats = (num(a, "min_beats_per_shot", 1.0, 1.0, 16.0)?) as usize;
    let max_beats = (num(a, "max_beats_per_shot", 4.0, 1.0, 16.0)?).max(min_beats as f64) as usize;
    let fade_out_beats = num(a, "fade_out_beats", 2.0, 0.0, 16.0)?;
    let target_lufs = num(a, "target_lufs", -14.0, -36.0, -5.0)?;
    let target_duration = a.get("target_duration").and_then(|v| v.as_f64());
    let start = num(a, "start", 0.0, 0.0, 1e7)?;

    let masset = ss.project.assets.get(&music_asset_id).ok_or_else(|| format!("no such asset {music_asset_id}"))?.clone();
    let bt = call(ss, "audio_transients", &json!({"target": masset.path, "mode": "beat"}))?;
    let grid: Vec<(f64, f64)> = bt["beat_grid"].as_array().map(|v| v.iter().filter_map(|b| Some((b["t"].as_f64()?, b["strength"].as_f64().unwrap_or(0.0)))).collect()).unwrap_or_default();
    if grid.len() < beats_per_phrase + 2 { return Err(format!("audio_transients found only {} beats in this music; need at least {} for a phrase-aligned montage (a clearer, more percussive bed will beat-track better)", grid.len(), beats_per_phrase + 2)); }
    let idx0 = grid.iter().position(|(t, _)| *t >= -1e-9).unwrap_or(0);
    let m0 = grid[idx0].0;

    let mut idxs = beat_energy_cuts(&grid, idx0, min_beats, max_beats);
    if let Some(td) = target_duration { while idxs.len() > 1 && grid[*idxs.last().unwrap()].0 - m0 > td { idxs.pop(); } }
    if idxs.len() < 2 { return Err("not enough beats after the energy walk for even one shot; lower min_beats_per_shot or beats_per_phrase, or use a longer/busier music bed".into()); }
    // end on the next beats_per_phrase-aligned beat at or after the last chosen cut (phase relative to idx0), so the
    // edit lands on a bar line, not mid-phrase; falls back to the grid's own last beat if the music runs out first.
    let last_idx = *idxs.last().unwrap();
    let phrase_idx = { let mut p = idx0; while p < last_idx { p += beats_per_phrase; } if p >= grid.len() { grid.len() - 1 } else { p } };
    let end_src = grid[phrase_idx].0;
    let shift = start - m0;
    let beats_tl: Vec<f64> = idxs.iter().map(|&i| grid[i].0 + shift).collect();
    let end_tl = end_src + shift;

    let bc = call(ss, "beat_cut", &json!({"clip_ids": clip_ids, "beats": beats_tl, "end": end_tl}))?;
    let duration = end_tl - start;
    let mc = call(ss, "add_audio_clip", &json!({"asset_id": music_asset_id, "start": start, "duration": duration, "in_point": m0}))?;
    let music_clip_id = mc["clip_id"].as_str().ok_or("add_audio_clip returned no clip_id")?.to_string();
    let mut norm = call(ss, "batch_normalize_loudness", &json!({"clip_ids": [music_clip_id.clone()], "target_lufs": target_lufs}))?;
    // A music bed with sharp, peaky transients (hard clicks, unmastered hits) can refuse the gain normalize_loudness
    // needs without pushing its sample peaks over -1dBFS -- batch_normalize_loudness reports that clip in `failed`,
    // not `applied`. Rather than silently leaving the montage quiet (defeating the -14 LUFS ask), take the refusal's
    // own suggested fix once: a gentle compressor (add_audio_effect, already the engine's own tool for this -- the
    // same params the refusal message itself names) to tame the peaks, then retry the same normalize once.
    if norm["applied"].as_array().map(|a| a.is_empty()).unwrap_or(true) && !norm["failed"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        call(ss, "add_audio_effect", &json!({"clip_id": music_clip_id, "kind": "compressor", "params": {"threshold_db": -24.0, "ratio": 4.0}}))?;
        norm = call(ss, "batch_normalize_loudness", &json!({"clip_ids": [music_clip_id.clone()], "target_lufs": target_lufs}))?;
    }
    // applied carries gain_db when normalize_loudness actually ran; already_on_target means it was within
    // tolerance already (no change, gain 0dB) -- either way this is the real gain now sitting in the clip's
    // own volume prop, which the fade keys below must start from so they don't clobber the loudness pass.
    let gain = norm["applied"].as_array().and_then(|r| r.first()).and_then(|r| r.get("gain_db")).and_then(|v| v.as_f64())
        .map(|db| 10f64.powf(db / 20.0)).unwrap_or(1.0);
    let period = { let mut g: Vec<f64> = grid.windows(2).map(|w| w[1].0 - w[0].0).collect(); g.sort_by(|x, y| x.total_cmp(y)); if g.is_empty() { 0.5 } else { g[g.len() / 2] } };
    let fade_s = (fade_out_beats * period).min(duration);
    if fade_s > 0.0 {
        call(ss, "set_audio_volume_key", &json!({"clip_id": music_clip_id, "time": (duration - fade_s).max(0.0), "value": gain, "ease": "linear"}))?;
        call(ss, "set_audio_volume_key", &json!({"clip_id": music_clip_id, "time": duration, "value": 0.0, "ease": "linear"}))?;
    }
    Ok(json!({"video_track_id": bc["track_id"], "music_clip_id": music_clip_id, "music_track_id": mc["track_id"], "cuts": bc["cuts"], "cut_times": beats_tl,
        "beat_times_used": idxs.iter().map(|&i| grid[i].0 + shift).collect::<Vec<_>>(), "start": start, "end": end_tl, "duration": duration,
        "beats_per_phrase": beats_per_phrase, "fade_out_s": fade_s, "music_gain_applied": gain, "loudness_normalize": norm,
        "note": "every cut_time is a real audio_transients beat (shifted to the timeline), so cut_times and beat_times_used are identical by construction; shot length varies min_beats_per_shot..max_beats_per_shot beats with the beat's own strength, and the edit ends on a beats_per_phrase-aligned beat with the music faded out over the last fade_out_beats"}))
}

// ---------------------------------------------------------------- montage_color_match (job 3)
fn montage_color_match(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let clip_ids = str_list(a, "clip_ids")?;
    if clip_ids.len() < 2 { return Err("need at least 2 clip_ids to match a look across".into()); }
    let strength = num(a, "strength", 0.6, 0.0, 1.0)?;
    // match_shot_look itself skips any target that is also a reference clip (the reference clip IS the look, it
    // doesn't get pulled toward itself) -- so the default anchor is simply the first clip given, and every other
    // listed clip is the target set. An explicit reference_clip_ids lets a caller pick a different anchor (or
    // several, averaged) instead of "whichever clip came first in the list".
    let reference_clip_ids: Vec<String> = a.get("reference_clip_ids").and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![clip_ids[0].clone()]);
    let target_clip_ids: Vec<String> = clip_ids.iter().filter(|id| !reference_clip_ids.contains(id)).cloned().collect();
    if target_clip_ids.is_empty() { return Err("every clip_id given is also in reference_clip_ids; nothing left to match toward it".into()); }
    let r = call(ss, "match_shot_look", &json!({"reference_clip_ids": reference_clip_ids, "target_clip_ids": target_clip_ids, "strength": strength}))?;
    Ok(json!({"anchor_clip_ids": reference_clip_ids, "matched_clip_ids": target_clip_ids, "strength": strength, "match_shot_look_result": r,
        "note": "every clip in matched_clip_ids was pulled `strength` of the way toward anchor_clip_ids' averaged look via match_shot_look; anchor_clip_ids are left untouched (they ARE the target look)"}))
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
    Command { name: "beat_sync_montage", description: "Beat-synced montage cuts: lay the kept clips end to end, cutting exactly on the music's own beats, shot length varying with its energy. Reads audio_transients' own beat-mode grid (per-beat strength, no new beat tracker), walks it choosing a min_beats_per_shot..max_beats_per_shot step at each beat from that beat's own strength (a stronger beat makes a shorter shot, a quieter one a longer shot), hands the resulting cut times straight to the existing beat_cut tool as its own beats array (every cut is a real detected beat, not an approximation), and stops on a beats_per_phrase-aligned beat so the edit ends on a bar line, not mid-phrase. Lays the music (add_audio_clip), normalizes it to target_lufs (batch_normalize_loudness) before keying the fade so the tail's own silence doesn't drag the integrated measurement down, then fades it out over the last fade_out_beats so the montage never stops dead on the final frame.", mutates: true,
        schema: obj(vec![("clip_ids", json!({"type":"array","items":{"type":"string"},"description":"kept montage clips already on the timeline, in order (e.g. best_segments' own kept_clips)"})),
            ("music_asset_id", s("an imported music asset to beat-track and lay under the montage")),
            ("beats_per_phrase", n("beats per musical phrase/bar, 1..32, default 4 (4/4 time): the edit ends on a beat whose index is this many beats on from the first beat used")),
            ("min_beats_per_shot", n("shortest shot length in beats, 1..16, default 1")),
            ("max_beats_per_shot", n("longest shot length in beats, 1..16, default 4")),
            ("fade_out_beats", n("beats of fade-out at the very end, 0..16, default 2")),
            ("target_lufs", n("music integrated loudness target, -36..-5, default -14")),
            ("target_duration", n("optional: stop the energy walk once this many seconds are covered (still snaps to the next phrase boundary after)")),
            ("start", n("timeline seconds to begin the montage at, default 0"))], &["clip_ids", "music_asset_id"]),
        run: beat_sync_montage },
    Command { name: "montage_color_match", description: "Shot-to-shot colour consistency for a montage: pulls every clip toward one anchor's look. The anchor is the first clip_id given, or your own reference_clip_ids; every other listed clip is pulled toward it with the existing match_shot_look (already used for broll-vs-talking-head, reused verbatim, not re-implemented) -- the one-call way to tie a whole montage of travel/sports/pet clips shot in different light to one consistent look, instead of hand-picking reference/target pairs yourself clip by clip.", mutates: true,
        schema: obj(vec![("clip_ids", json!({"type":"array","items":{"type":"string"},"description":"montage clips to bring to one consistent look; the first one is the anchor unless reference_clip_ids is given"})),
            ("reference_clip_ids", json!({"type":"array","items":{"type":"string"},"description":"optional: one or more of clip_ids (or other clips) whose averaged look is the anchor, instead of defaulting to clip_ids[0]"})),
            ("strength", n("0..1, default 0.6 -- same tasteful-nudge default as match_shot_look itself"))], &["clip_ids"]),
        run: montage_color_match },
] }
