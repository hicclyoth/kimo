//! tighten_pauses (V7.8-Q job 1 "pacing"): shrink the dead air left INSIDE a rough-cut track's own kept
//! segments down to a natural breath, and trim the lead-in before the first spoken word so a cold open
//! lands fast. auto_rough_cut already removes a pause once it is long enough to be treated as a cut
//! between two segments (merged under `max_pause`); what it leaves behind is the shorter pause still
//! baked into a SINGLE kept clip's own footage (a breath between two phrases, or dead air before the
//! speaker's first word) -- those never get tightened by anything else in the pipeline, which is exactly
//! the gap this tool closes.
//!
//! Reuses existing primitives, adds no new ripple/split code: for each media clip on the track, this
//! tool (1) asks `audio_speech_intervals` in `mode:"silence"` for that ONE clip's own interior silence
//! windows -- the RMS-threshold probe, not the padded/hangover-inflated VAD speech boundaries, so the
//! reported window already sits a safe margin inside the true acoustic gap (see "never cut mid-word"
//! below) -- then (2) for every window worth shrinking, computes a small span to drop from its MIDDLE
//! (or, for the clip's own leading silence before its first word, from its START) and calls the existing
//! `trim_filler_words` (filler_trim.rs) with that span, exactly the same ripple-delete + caption-retime
//! path a flagged-word trim already uses. No new split/ripple/caption-retime logic exists here.
//!
//! Never cut mid-word: `mode:"silence"` (perception::vad::silence_probe) classifies a 100ms RMS window
//! as silent only when the WHOLE window sits under threshold_db -- a window straddling real speech never
//! qualifies, so a reported silence window is already a safe subset of the true gap, not a guess. On top
//! of that, this tool never removes a window's own edges: it always leaves at least half the target pad
//! on each side of an interior gap (the full target pad before the first word, for the leading gap),
//! so a cut only ever lands inside the already-conservative silent window, never at its boundary.
//!
//! Scope: only plain clips (Source::Media, TimeMode::Normal, no time_remap/speed_curve, finite speed>0)
//! are touched; anything else on the track is left alone and listed in `skipped_clips`. A clip's own
//! trailing silence (touching its own source end) is never touched here -- that is the handoff point to
//! whatever comes after this clip (the next clip, or an end screen), not a mid-speech pause.
use super::kit::*;
use crate::commands::dispatch_nested as call;
use crate::model::{Source, TimeMode};


fn bool_arg_opt(a: &Value, k: &str) -> Result<Option<bool>, String> {
    match a.get(k) { None | Some(Value::Null) => Ok(None), Some(v) => v.as_bool().map(Some).ok_or_else(|| crate::errors::arg_err(a, k, "boolean")) }
}
fn opt_num_checked(a: &Value, k: &str, d: f64) -> Result<f64, String> {
    match a.get(k) { None | Some(Value::Null) => Ok(d), Some(v) => v.as_f64().ok_or_else(|| crate::errors::arg_err(a, k, "number")) }
}

/// The track with the most `Source::Media` clips (ties keep the lowest index) -- same rule and same
/// reasoning as `drop_short_segments::main_track`, duplicated locally (this file does not depend on that
/// one): a rough-cut timeline's dialogue is already the track with by far the most media clips on it.
fn main_track(ss: &Session) -> Option<String> {
    let mut best: Option<(String, usize)> = None;
    for t in &ss.project.tracks {
        let n = t.clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).count();
        if n == 0 { continue; }
        if best.as_ref().map_or(true, |b| n > b.1) { best = Some((t.id.clone(), n)); }
    }
    best.map(|(id, _)| id)
}

/// This track's `Source::Media`, plain-speed, plain-time-mode clips, in TIMELINE order, as (id, duration,
/// speed). Anything else on the track (a title, a solid, a time-remapped or frozen clip) is reported back
/// to the caller in `skipped` rather than silently ignored.
fn eligible_clips(ss: &Session, ti: usize, skipped: &mut Vec<Value>) -> Vec<(String, f64, f64)> {
    let mut rows: Vec<(String, f64, f64, f64)> = ss.project.tracks[ti].clips.iter().map(|c| (c.id.clone(), c.start, c.duration, c.speed)).collect();
    rows.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = vec![];
    for (id, _start, duration, speed) in rows {
        let Some(c) = ss.project.find_clip(&id) else { continue };
        if !matches!(c.source, Source::Media { .. }) { skipped.push(json!({"clip_id": id, "reason": "not a media clip"})); continue; }
        if !matches!(c.time_mode, TimeMode::Normal) { skipped.push(json!({"clip_id": id, "reason": "time_mode is not normal (freeze/reverse)"})); continue; }
        if c.time_remap.is_some() || c.speed_curve.is_some() { skipped.push(json!({"clip_id": id, "reason": "time remap or speed ramp"})); continue; }
        if !(speed.is_finite() && speed > 0.0) { skipped.push(json!({"clip_id": id, "reason": "non-finite or non-positive speed"})); continue; }
        out.push((id, duration, speed));
    }
    out
}

fn run(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let track_id = match a.get("track_id") {
        None | Some(Value::Null) => main_track(ss).ok_or("track_id was not given and no track has a media clip to default to; give track_id")?,
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err(crate::errors::arg_err(a, "track_id", "string")),
    };
    let ti = ss.project.tracks.iter().position(|t| t.id == track_id).ok_or_else(|| format!("no such track '{track_id}'"))?;
    if ss.project.tracks[ti].locked { return Err(format!("track '{track_id}' is locked")); }

    let pad_ms = opt_num_checked(a, "pad_ms", 120.0)?.clamp(60.0, 250.0);
    let opening_pad_ms = opt_num_checked(a, "opening_pad_ms", 80.0)?.clamp(0.0, 250.0);
    let min_gap_ms = opt_num_checked(a, "min_gap_ms", 260.0)?.max(pad_ms.max(opening_pad_ms) + 40.0);
    let threshold_db_fixed = match a.get("threshold_db") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let t = v.as_f64().ok_or_else(|| crate::errors::arg_err(a, "threshold_db", "number"))?;
            if !(t.is_finite() && (-200.0..=0.0).contains(&t)) { return Err("threshold_db must be finite, -200..0".into()); }
            Some(t)
        }
    };
    let floor_margin_frac = opt_num_checked(a, "floor_margin_frac", 0.5)?.clamp(0.1, 0.9);
    let min_piece_s = opt_num_checked(a, "min_piece_s", 1.0)?.clamp(0.2, 5.0);
    let dry_run = bool_arg_opt(a, "dry_run")?.unwrap_or(false);

    let mut skipped_clips: Vec<Value> = vec![];
    let clips = eligible_clips(ss, ti, &mut skipped_clips);

    let total_before_s: f64 = ss.project.tracks[ti].clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).map(|c| c.duration).sum();

    // Real recordings carry room tone well above a fixed "silence" level (a studio-clean synthetic clip
    // can sit at -60dB between phrases; a real interview's own noise floor can sit at -20dB) -- a fixed
    // threshold_db either never fires on noisy real audio or, set loose enough for that, risks reading
    // quiet SPEECH as silence on a clean one. Unless the caller pins threshold_db, calibrate ONCE from the
    // WHOLE TRACK's own mix (not per clip: a short individual kept segment can be almost all speech, which
    // biases its own 20th-percentile "floor" estimate upward toward speech level itself -- caught on the
    // real benchmark, where a per-clip floor read 9.6s of real dialogue as "silence" and gutted it). A
    // track-wide read has far more quiet frames to estimate from, and is additionally clamped to stay
    // comfortably under the quietest SPEECH segment this same probe found, so a noisy-but-real floor can
    // never climb high enough to eat into actual words.
    let threshold_db = match threshold_db_fixed {
        Some(t) => t,
        None => {
            let vad_probe = call(ss, "audio_speech_intervals", &json!({"target": {"track_id": track_id}, "mode": "vad", "aggressiveness": 1}));
            match vad_probe {
                Ok(v) => {
                    let floor = v["noise_floor_db"].as_f64().unwrap_or(-45.0);
                    let quietest_speech = v["speech"].as_array().and_then(|a| a.iter().filter_map(|s| s["level_db"].as_f64()).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))));
                    match quietest_speech {
                        // A flat dB margin above the floor isn't robust: how much headroom a recording
                        // actually has between its own room tone and its own quietest speech varies a lot
                        // (a clean studio clip can have 40dB of it, a compressed real interview under 10) --
                        // caught on the real benchmark, where floor+9dB landed ABOVE both detected speech
                        // levels on a real clip with only ~8dB of headroom, which the old 10dB-under-speech
                        // cap then had to crush back down to almost nothing. Scale into whatever headroom
                        // this recording actually has instead: a fraction of the way from the floor up
                        // toward the quietest speech, with a fixed small buffer kept clear of speech itself.
                        Some(q) if q.is_finite() && q > floor => {
                            let scaled = floor + floor_margin_frac * (q - floor);
                            scaled.min(q - 3.0).clamp(-70.0, -12.0)
                        }
                        _ => (floor + 9.0).clamp(-70.0, -18.0), // no usable speech reading: the old flat fallback
                    }
                }
                Err(_) => -45.0, // couldn't read anything: fall back to the old fixed default
            }
        }
    };

    let mut gaps: Vec<Value> = vec![];
    let mut total_removed = 0.0f64;
    let mut longest_before_ms = 0.0f64;
    let mut opening_before_ms: Option<f64> = None;
    let mut opening_after_ms: Option<f64> = None;

    for (idx, (clip_id, duration, speed)) in clips.iter().enumerate() {
        let src_len = duration * speed;
        let probe = match call(ss, "audio_speech_intervals", &json!({
            "target": {"clip_id": clip_id}, "mode": "silence", "threshold_db": threshold_db, "min_silence_ms": min_gap_ms,
        })) {
            Ok(v) => v,
            Err(e) => { skipped_clips.push(json!({"clip_id": clip_id, "reason": format!("audio_speech_intervals failed: {e}")})); continue; }
        };
        let silences = probe["silence"].as_array().cloned().unwrap_or_default();
        if silences.is_empty() { continue; }

        let is_first_clip = idx == 0;
        let mut candidates: Vec<(f64, f64, f64, bool, f64)> = vec![]; // (rst, ren, before_ms, is_leading, target_pad_ms)
        for sv in &silences {
            let (Some(gs), Some(ge), Some(gdur)) = (sv["start"].as_f64(), sv["end"].as_f64(), sv["duration"].as_f64()) else { continue };
            if ge >= src_len - 0.05 { continue; } // this clip's own trailing silence: the handoff point, not a mid-speech pause
            let is_leading = is_first_clip && gs <= 0.05;
            let target_pad_ms = if is_leading { opening_pad_ms } else { pad_ms };
            if gdur * 1000.0 <= target_pad_ms + 40.0 { continue; } // not meaningfully longer than the pad we'd leave anyway

            let (rst, ren) = if is_leading {
                (gs.max(0.0), ge - target_pad_ms / 1000.0)
            } else {
                let margin = target_pad_ms / 1000.0 / 2.0;
                (gs + margin, ge - margin)
            };
            if ren <= rst + 0.01 { continue; } // under 10ms isn't worth a cut
            candidates.push((rst, ren, gdur * 1000.0, is_leading, target_pad_ms));
        }
        if candidates.is_empty() { continue; }

        // Downstream per-clip tools (finish_audio's own dialogue/ambience classifier, among others) read a
        // SHORT clip's speech content less reliably than a long one -- fragmenting one rough-cut segment into
        // many small pieces (every qualifying gap cut) raises the odds some piece reads as "mostly silence"
        // and gets muted entirely as ambience, which is a far worse quality loss than the dead air this tool
        // is trying to fix. Keep every surviving piece at least min_piece_s long: walk the candidates in
        // order and skip (not cut) any gap that would leave the piece BEFORE it shorter than that.
        let mut spans: Vec<(f64, f64)> = vec![];
        let mut accepted: Vec<(f64, bool, f64)> = vec![]; // (before_ms, is_leading, target_pad_ms), parallel to spans
        let mut piece_start = 0.0f64;
        for &(rst, ren, before_ms, is_leading, target_pad_ms) in &candidates {
            // The leading gap has no real "piece before" it to protect (there is nothing on the timeline
            // before the clip's own start) -- only a non-leading cut needs the floor on its lead-in side.
            if !is_leading && rst - piece_start < min_piece_s { continue; }
            spans.push((rst, ren));
            accepted.push((before_ms, is_leading, target_pad_ms));
            piece_start = ren;
        }
        // The piece AFTER the last accepted cut, to this clip's own end, needs the same floor; drop accepted
        // cuts from the tail until it is satisfied (almost always at most the very last one).
        while let Some(&(_, last_ren)) = spans.last() {
            if src_len - last_ren >= min_piece_s { break; }
            spans.pop();
            accepted.pop();
        }
        for (span, (before_ms, is_leading, target_pad_ms)) in spans.iter().zip(accepted.iter()) {
            longest_before_ms = longest_before_ms.max(*before_ms);
            if *is_leading { opening_before_ms = Some(*before_ms); opening_after_ms = Some(*target_pad_ms); }
            gaps.push(json!({"clip_id": clip_id, "leading": is_leading, "before_ms": before_ms, "after_ms": target_pad_ms, "removed_s": span.1 - span.0}));
        }
        if spans.is_empty() { continue; }
        if dry_run { total_removed += spans.iter().map(|(s, e)| e - s).sum::<f64>(); continue; }

        let span_args: Vec<Value> = spans.iter().map(|(s, e)| json!({"start": s, "end": e})).collect();
        let tr = call(ss, "trim_filler_words", &json!({"clip_id": clip_id, "spans": span_args, "pad": 0.0, "retime_captions": true}))?;
        total_removed += tr["removed_seconds"].as_f64().unwrap_or(0.0);
    }

    let (clips_after, total_after_s) = if dry_run {
        (clips.len(), (total_before_s - total_removed).max(0.0))
    } else {
        let t = &ss.project.tracks[ti];
        (t.clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).count(), t.clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).map(|c| c.duration).sum::<f64>())
    };

    Ok(json!({
        "track_id": track_id, "dry_run": dry_run, "threshold_db_used": threshold_db,
        "pad_ms": pad_ms, "opening_pad_ms": opening_pad_ms, "min_gap_ms": min_gap_ms,
        "gaps_tightened": gaps.len(), "gaps": gaps, "skipped_clips": skipped_clips,
        "removed_seconds": total_removed, "total_before_s": total_before_s, "total_after_s": total_after_s,
        "clips_after": clips_after,
        "longest_interior_pause_before_ms": longest_before_ms,
        "opening_before_ms": opening_before_ms, "opening_after_ms": opening_after_ms,
    }))
}

pub fn commands() -> Vec<Command> {
    vec![Command {
        name: "tighten_pauses",
        description: "Shrink dead air a rough cut leaves INSIDE its own kept clips to a natural pad, never cutting mid-word. Tightens a breath between two phrases and the lead-in before the first word.",
        mutates: true,
        schema: obj(vec![
            ("track_id", s("track to tighten, default: the track with the most media clips")),
            ("pad_ms", n("target remaining gap between phrases after tightening, milliseconds; default 120, clamped 60..250")),
            ("opening_pad_ms", n("target remaining gap before the very first spoken word on the track's first clip (the cold-open hook), milliseconds; default 80, clamped 0..250")),
            ("min_gap_ms", n("only act on a pause at least this long (milliseconds); default 260, raised automatically to pad_ms/opening_pad_ms + 40 if given lower")),
            ("threshold_db", n("RMS level below which a 100ms window counts as silence; default: auto-calibrated once from the whole track's own measured noise floor and quietest speech level (give a number to pin one fixed level instead)")),
            ("floor_margin_frac", n("when threshold_db is not pinned: how far from the track's own noise floor (0) toward its own quietest speech level (1) counts as silence; default 0.5, clamped 0.1..0.9")),
            ("min_piece_s", n("never cut a gap if it would leave a surviving piece shorter than this (seconds); a short clip is read less reliably by other per-clip tools downstream (finish_audio's own dialogue/ambience classifier among them), so this trades a little extra dead air for not risking a chunk of real speech getting muted as ambience. Default 1.0, clamped 0.2..5")),
            ("dry_run", json!({"type":"boolean","description":"report what would be tightened, change nothing. Default false."})),
        ], &[]),
        run,
    }]
}
