//! v6 eyes: pick_thumbnail. Picks the best cover-image frames from a clip or the whole timeline and writes them as PNGs.
//! Samples `sample_fps` project times across the span (same analysis::render path as black_frames, frozen_frames and
//! focus_series) and scores each surviving candidate on three reused signals: sharpness (focus_series' own lap_var,
//! normalized against the sharpest surviving candidate of this call), exposure (1 - crushed_frac - clipped_frac from
//! frame_quality's own quality(), default thresholds) and a cheap subject-centred signal (saliency_map's own spectral-
//! residual saliency map, reduced to its mass centroid's closeness to frame centre; 0.5, neither bonus nor penalty, when
//! the frame carries no saliency mass). A candidate is excluded before scoring, never scored and never returned, when it
//! sits inside an active `transition_in` window (read from the project model, no extra render), at or under
//! black_frames' own near-black threshold, or within a small difference of the previous surviving sample (frozen_frames'
//! own frame-difference measure) -- in that last case only the earlier of the pair survives, so a long static hold
//! contributes one candidate, not many identical ones. Weights: 0.6 sharpness + 0.25 exposure + 0.15 subject-centred.
//! Picks the top `count` surviving candidates at least `min_gap_s` apart (greedy by score, highest first, skipping any
//! candidate too close to one already picked), writes each at project resolution to `out_dir`, and reports time, score,
//! score_parts and the png path for each. Pure function of the project and its media: same input, same output.
use super::kit::*;
use crate::commands::Media;
use crate::effects::Frame;
use crate::perception::analysis::{self, Image, Request};

/// Two sampled frames differing by at most this much (the same 0..1 scale frozen_frames::max_diff reads) count as one
/// candidate -- a long static hold yields a single surviving candidate rather than many identical copies.
const FREEZE_TOL: f64 = 0.004;
/// Candidates in one call, bounded the same way focus_series bounds its samples (raise sample_fps's divisor, i.e. lower
/// sample_fps, or shorten the span for more).
const MAX_SAMPLES: usize = 2000;

fn r6(x: f64) -> f64 { (x * 1e6).round() / 1e6 }
fn num(a: &Value, k: &str, d: f64, lo: f64, hi: f64) -> Result<f64, String> {
    match a.get(k) { None | Some(Value::Null) => Ok(d), Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= lo && *x <= hi).ok_or(format!("out_of_range: {k} must be a number in [{lo}, {hi}]")) }
}
fn int(a: &Value, k: &str, d: u64, lo: u64, hi: u64) -> Result<u64, String> {
    match a.get(k) { None | Some(Value::Null) => Ok(d), Some(v) => v.as_u64().filter(|x| (lo..=hi).contains(x)).ok_or(format!("out_of_range: {k} must be a whole number {lo}..{hi}")) }
}

/// True when `t` (timeline seconds) sits inside a clip's own transition_in window: [clip.start, clip.start + duration).
/// Scoped to one clip when `clip_id` is given (its own transition only), otherwise any clip on any track.
fn in_transition(ss: &Session, clip_id: &Option<String>, t: f64) -> bool {
    fn hits(c: &crate::model::Clip, t: f64) -> bool { c.transition_in.as_ref().is_some_and(|tr| t >= c.start - 1e-9 && t < c.start + tr.duration - 1e-9) }
    match clip_id {
        Some(id) => ss.project.find_clip(id).map(|c| hits(c, t)).unwrap_or(false),
        None => { for tr in &ss.project.tracks { for c in &tr.clips { if hits(c, t) { return true; } } } false }
    }
}

/// Cheap composition signal, two parts: (a) spectral-residual saliency (saliency_map's own code, default blur) on a
/// small grid (48 px long side), reduced to the share of saliency mass sitting in the centre cell of a 3x3 rule-of-
/// thirds grid (saliency_map's own `stats` grid, cell 4) -- rewards a subject placed centrally, 0 with no saliency mass
/// at all; (b) a detail term, the Rec.709 luma standard deviation of the whole frame against a fixed 0.12 reference --
/// rewards texture/subject matter and penalises a large flat low-detail area (a plain wall), 0..1. Combined 0.65a+0.35b.
pub(crate) fn subject_score(im: &Image) -> f64 {
    let luma: Vec<f64> = (0..im.w * im.h).map(|i| im.luma(i)).collect();
    let mean: f64 = luma.iter().sum::<f64>() / luma.len().max(1) as f64;
    let var: f64 = luma.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / luma.len().max(1) as f64;
    let detail = (var.sqrt() / 0.12).min(1.0);

    let long = im.w.max(im.h).max(1);
    let work_px = 48usize;
    let (ww, wh) = (((im.w * work_px) as f64 / long as f64).round().max(1.0) as usize, ((im.h * work_px) as f64 / long as f64).round().max(1.0) as usize);
    let g = super::saliency_map::area_down(&luma, im.w, im.h, ww, wh);
    let spec = super::saliency_map::spectral_residual(&g, ww, wh, 2.5);
    let st = super::saliency_map::stats(&spec, ww, wh, im.w, im.h, &[], 3);
    let center_mass = if st["empty"].as_bool().unwrap_or(false) { 0.0 } else { st["grid"]["mass"][4].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) };
    (0.65 * center_mass + 0.35 * detail).clamp(0.0, 1.0)
}

/// Build a writable Frame (f32 RGBA, alpha 1.0) from an already-rendered Image (f64 RGB, 0..1); writes the identical
/// 8-bit values analysis::render's own Image::from_frame already quantized, so this round-trips cleanly.
fn frame_from_image(im: &Image) -> Frame {
    let mut fr = Frame::new(im.w, im.h);
    for i in 0..im.w * im.h { fr.d[i * 4] = im.rgb[0][i] as f32; fr.d[i * 4 + 1] = im.rgb[1][i] as f32; fr.d[i * 4 + 2] = im.rgb[2][i] as f32; fr.d[i * 4 + 3] = 1.0; }
    fr
}

struct Cand { time: f64, lap_var: f64, crushed: f64, clipped: f64, subject: f64, excluded: Option<&'static str> }

fn run(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let clip_id = match a.get("clip_id") { None | Some(Value::Null) => None, Some(Value::String(s)) if !s.is_empty() => Some(s.clone()), _ => return Err("invalid_source: clip_id must be a non-empty string".into()) };
    let (source, bounds) = match &clip_id {
        Some(id) => { let c = ss.project.find_clip(id).ok_or("invalid_source: missing clip")?; (format!("clip:{id}"), (c.start, c.start + c.duration)) }
        None => ("composite".to_string(), (0.0, ss.project.duration())),
    };
    if !(bounds.1 > bounds.0) { return Err("out_of_range: the source span is empty".into()); }

    let count = int(a, "count", 3, 1, 12)? as usize;
    let min_gap_s = num(a, "min_gap_s", 1.0, 0.0, 1e6)?;
    let sample_fps = num(a, "sample_fps", 4.0, 1e-3, 60.0)?;
    let out_dir = match a.get("out_dir") { None | Some(Value::Null) => ss.project.base_dir.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| ".".to_string()), Some(Value::String(s)) if !s.is_empty() => s.clone(), _ => return Err("invalid_source: out_dir must be a non-empty string".into()) };

    let fps = ss.project.fps; if !(fps > 0.0 && fps <= 1000.0) { return Err("out_of_range: project fps must be in (0, 1000]".into()); }
    if ss.project.width == 0 || ss.project.height == 0 { return Err("out_of_range: project has no canvas size".into()); }

    let step = 1.0 / sample_fps;
    let n = ((bounds.1 - bounds.0) / step).floor() as i64;
    if n < 1 { return Err("out_of_range: the span holds no sample at this sample_fps; lower sample_fps or lengthen the span".into()); }
    let n = n as usize;
    if n > MAX_SAMPLES { return Err(format!("out_of_range: sample_fps over this span is {n} samples; at most {MAX_SAMPLES} (lower sample_fps or shorten the span)")); }

    std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create out_dir {out_dir:?}: {e}"))?;

    let mut cands: Vec<Cand> = Vec::with_capacity(n);
    let mut prev_rgb: Option<[Vec<f64>; 3]> = None;
    let (mut n_black, mut n_frozen, mut n_transition) = (0usize, 0usize, 0usize);
    for i in 0..n {
        crate::guard::check();
        let t = bounds.0 + i as f64 * step;
        let trans = in_transition(ss, &clip_id, t);
        let im = analysis::render(ss, &Request { time: t, source: source.clone() })?;
        let black = super::black_frames::max_luma(&im) <= super::black_frames::DEFAULT_LUMA_MAX;
        let frozen = !trans && !black && prev_rgb.as_ref().is_some_and(|p| super::frozen_frames::max_diff(p, &im.rgb) <= FREEZE_TOL);
        let excluded = if trans { n_transition += 1; Some("transition") } else if black { n_black += 1; Some("black") } else if frozen { n_frozen += 1; Some("frozen_duplicate") } else { None };

        let y = super::focus_series::luma_plane(&im);
        let lap_var = super::focus_series::score_plane(&y, im.w, im.h, None).map(|s| s.lap_var).unwrap_or(0.0);
        let q = super::frame_quality::quality(&im, None, &super::frame_quality::QParams::default());
        let crushed = q["crushed_frac"].as_f64().unwrap_or(0.0);
        let clipped = q["clipped_frac"].as_f64().unwrap_or(0.0);
        let subject = subject_score(&im);

        if excluded.is_none() { prev_rgb = Some(im.rgb.clone()); }
        cands.push(Cand { time: t, lap_var, crushed, clipped, subject, excluded });
    }

    let max_lap = cands.iter().filter(|c| c.excluded.is_none()).map(|c| c.lap_var).fold(0.0f64, f64::max).max(1e-9);
    struct Scored { time: f64, score: f64, sharpness: f64, exposure: f64, subject: f64, lap_var: f64, crushed: f64, clipped: f64 }
    let mut scored: Vec<Scored> = cands.iter().filter(|c| c.excluded.is_none()).map(|c| {
        let sharpness = (c.lap_var / max_lap).min(1.0);
        let exposure = (1.0 - (c.crushed + c.clipped).min(1.0)).max(0.0);
        let score = 0.6 * sharpness + 0.25 * exposure + 0.15 * c.subject;
        Scored { time: c.time, score, sharpness, exposure, subject: c.subject, lap_var: c.lap_var, crushed: c.crushed, clipped: c.clipped }
    }).collect();
    if scored.is_empty() { return Err("out_of_range: every sampled frame in this span was black, frozen or mid-transition; nothing to pick from".into()); }
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(a.time.partial_cmp(&b.time).unwrap()));

    let mut picked: Vec<&Scored> = vec![];
    for s in &scored {
        if picked.iter().all(|p: &&Scored| (p.time - s.time).abs() >= min_gap_s - 1e-9) {
            picked.push(s);
            if picked.len() == count { break; }
        }
    }

    let mut picks = vec![];
    for (rank, s) in picked.iter().enumerate() {
        let im = analysis::render(ss, &Request { time: s.time, source: source.clone() })?;
        let fr = frame_from_image(&im);
        let path = format!("{}/pick_thumbnail_{rank:02}_t{:.3}s.png", out_dir.trim_end_matches('/'), s.time);
        crate::render::check_still_path(&path, true)?;
        crate::render::write_image(&fr, &path)?;
        ss.media.push(Media { path: path.clone(), w: im.w as u32, h: im.h as u32, mime: "image/png" });
        picks.push(json!({"time": r6(s.time), "score": r6(s.score), "score_parts": {"sharpness": r6(s.sharpness), "exposure": r6(s.exposure), "subject_centered": r6(s.subject)}, "lap_var": r6(s.lap_var), "crushed_frac": r6(s.crushed), "clipped_frac": r6(s.clipped), "png": path}));
    }

    let mut out = json!({
        "source": clip_id.as_ref().map(|id| format!("clip:{id}")).unwrap_or_else(|| "timeline".to_string()),
        "span": [bounds.0, bounds.1], "sample_fps": sample_fps, "min_gap_s": min_gap_s,
        "candidates_sampled": n, "candidates_scored": scored.len(),
        "candidates_excluded": {"black": n_black, "frozen_duplicate": n_frozen, "transition": n_transition},
        "count_requested": count, "count_returned": picks.len(), "picks": picks, "out_dir": out_dir,
    });
    if picks.len() < count { out["note"] = json!(format!("only {} of the requested {count} candidates are at least min_gap_s ({min_gap_s}) apart after excluding black/frozen/transition frames", picks.len())); }
    Ok(out)
}

pub fn commands() -> Vec<Command> { vec![Command { name: "pick_thumbnail", mutates: false,
    description: "Pick the best cover image frames from a clip or the whole timeline, scored on sharpness, exposure and subject framing, and write them as PNGs. Samples sample_fps (default 4) project times across clip_id's own span (default the whole timeline) through the same render path as black_frames/frozen_frames/focus_series. Each candidate is scored 0.6x sharpness (focus_series' own Laplacian-variance measure, normalized against this call's sharpest surviving candidate) + 0.25x exposure (1 - crushed_frac - clipped_frac, frame_quality's own measure, default thresholds) + 0.15x a cheap composition signal (saliency_map's own spectral-residual saliency: the share of saliency mass in the centre cell of a rule-of-thirds 3x3 grid, plus the whole frame's own luma-detail/flatness, 0.65/0.35; 0 with no saliency mass and a flat frame). A candidate inside a clip's own transition_in window, at or under black_frames' own near-black threshold, or within a small difference of the previous surviving sample (frozen_frames' own measure, so a static hold yields one candidate) is excluded before scoring, never returned. Returns the top count (1..12, default 3) surviving candidates at least min_gap_s (default 1.0s) apart, ranked by score, each with time, score, score_parts {sharpness, exposure, subject_centered}, lap_var and the written png path (project resolution, out_dir default the project's own directory); fewer than count when the span has no more qualifying frames that far apart. Deterministic: same project and args, same output.",
    schema: obj(vec![
        ("clip_id", s("restrict to one clip's own timeline span; default the whole timeline")),
        ("count", json!({"type": "integer", "minimum": 1, "maximum": 12, "description": "thumbnails to return, default 3"})),
        ("min_gap_s", json!({"type": "number", "minimum": 0, "description": "minimum seconds between two returned picks, default 1.0"})),
        ("out_dir", s("directory to write the PNGs into; default the project's own directory, or the current directory")),
        ("sample_fps", json!({"type": "number", "exclusiveMinimum": 0, "maximum": 60, "description": "frames per second to sample and score, default 4"})),
    ], &[]),
    run }] }
