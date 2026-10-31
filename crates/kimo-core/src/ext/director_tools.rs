//! Director recipes: one call does a whole editing step the safe way, so a small model driving kimo over MCP gets a
//! creator-quality edit without composing twenty low-level calls. Each recipe is plain orchestration over the existing
//! tools (dispatch_nested: same validation, locks and ids; one undo step for the whole recipe) and returns a short result
//! with the ids it made. lint_edit reads the edit and lists concrete problems, each with the exact call that fixes it.
use super::kit::*;
use crate::commands::dispatch_nested as call;
use crate::model::Source;
use std::collections::HashMap;

fn bool_arg(a: &Value, k: &str) -> Result<Option<bool>, String> { match a.get(k) { None | Some(Value::Null) => Ok(None), Some(v) => v.as_bool().map(Some).ok_or_else(|| crate::errors::arg_err(a, k, "boolean")) } }

fn atomic(ss: &mut Session, a: &Value, f: fn(&mut Session, &Value) -> Result<Value, String>) -> Result<Value, String> {
    let before = ss.project.clone();
    match f(ss, a) { Ok(v) => Ok(v), Err(e) => { ss.project = before; Err(e) } }
}
fn r3(v: f64) -> f64 { (v * 1000.0).round() / 1000.0 }
fn str_list(a: &Value, k: &str) -> Result<Vec<String>, String> {
    let v = a.get(k).and_then(|v| v.as_array()).ok_or_else(|| format!("{k}: an array of ids is required, e.g. [\"asset1\", \"asset2\"]"))?;
    let out: Vec<String> = v.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    if out.is_empty() || out.len() != v.len() { return Err(format!("{k}: give one or more id strings, e.g. [\"asset1\"]")); }
    Ok(out)
}
fn fps(ss: &Session) -> f64 { if ss.project.fps > 0.0 { ss.project.fps } else { 30.0 } }
fn snap(t: f64, fps: f64) -> f64 { (t * fps).round() / fps }
fn asset(ss: &Session, id: &str) -> Result<(String, f64), String> {
    ss.project.assets.get(id).map(|x| (x.path.clone(), x.duration)).ok_or_else(|| {
        let ids: Vec<&String> = ss.project.assets.keys().take(12).collect();
        format!("unknown asset '{id}'; import_media the file first (assets now: {ids:?})")
    })
}
fn track_index(ss: &Session, tid: &str) -> Result<usize, String> {
    ss.project.tracks.iter().position(|t| t.id == tid).ok_or_else(|| {
        let ids: Vec<&str> = ss.project.tracks.iter().map(|t| t.id.as_str()).collect();
        format!("unknown track '{tid}' (tracks: {ids:?})")
    })
}
fn new_track(ss: &mut Session) -> Result<String, String> {
    call(ss, "add_track", &json!({}))?["track_id"].as_str().map(String::from).ok_or_else(|| "add_track returned no id".into())
}
/// Speech intervals [start, end] (seconds, target-relative: timeline seconds for a track) from the engine's VAD.
fn speech(ss: &mut Session, target: Value, aggr: u32, min_sil_ms: f64, pad_ms: f64) -> Result<Vec<(f64, f64)>, String> {
    let r = call(ss, "audio_speech_intervals", &json!({"target": target, "aggressiveness": aggr, "min_silence_ms": min_sil_ms, "pad_ms": pad_ms, "max_items": 1000}))?;
    Ok(r["speech"].as_array().map(|v| v.iter().filter_map(|s| Some((s["start"].as_f64()?, s["end"].as_f64()?))).collect()).unwrap_or_default())
}
fn merge(mut v: Vec<(f64, f64)>, gap: f64) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, f64)> = vec![];
    for (s, e) in v { match out.last_mut() { Some(l) if s - l.1 < gap => { l.1 = l.1.max(e); } _ => out.push((s, e)) } }
    out
}
fn pairs(v: &[(f64, f64)]) -> Value { json!(v.iter().map(|(s, e)| json!([r3(*s), r3(*e)])).collect::<Vec<_>>()) }

/// V7.8Q: VAD-mode speech detection (used by `speech()` above) confirms an onset only after enough
/// consecutive frames read as speech, which costs real words at the very start of a recording that
/// begins talking immediately with no lead-in silence to let the detector settle first (measured on a
/// real interview: the VAD's own first detected span started 0.48s into a clip whose audio -- confirmed
/// independently -- is already mid-word at 0.0s). `mode:"silence"` (the same plain RMS-threshold probe
/// `tighten_pauses` uses, not VAD) draws no such confirmation delay, so for the very first kept segment
/// of the whole output this re-checks the small window VAD skipped and recovers it if it is not actually
/// silence. Never moves the start LATER than VAD's own answer, only earlier (strictly more content kept).
fn recover_missed_onset(ss: &mut Session, path: &str, vad_start: f64) -> f64 {
    if vad_start <= 0.02 { return vad_start; } // nothing to recover
    let probe = match call(ss, "audio_speech_intervals", &json!({
        "target": {"path": path}, "mode": "silence", "start": 0.0, "end": vad_start + 0.2, "threshold_db": -35.0, "min_silence_ms": 50,
    })) {
        Ok(v) => v,
        Err(_) => return vad_start, // probe failed: keep VAD's own (safe) answer
    };
    let silences = probe["silence"].as_array().cloned().unwrap_or_default();
    // A leading silence that actually touches 0 is the genuine lead-in; recover back to where it ends.
    // No such span at all means there is no silence at the very start -- sound is already there at 0.
    let leading_end = silences.iter().find(|sv| sv["start"].as_f64().unwrap_or(1.0) <= 0.05).and_then(|sv| sv["end"].as_f64());
    leading_end.unwrap_or(0.0).clamp(0.0, vad_start)
}

// ---------------------------------------------------------------- shot quality: blur, whip pans, near-duplicates
/// A cheap look at an asset: grey 96x160 frames at a few fps. Per frame: sharpness (variance of the Laplacian),
/// motion (mean absolute change from the previous frame, 0..255) and a 64-bit difference hash.
struct Look { rate: f64, sharp: Vec<f64>, motion: Vec<f64>, hash: Vec<u64> }
fn look(path: &str, dur: f64) -> Option<Look> {
    const W: usize = 96; const H: usize = 160;
    let rate = if dur > 150.0 { (900.0 / dur).max(0.5) } else { 6.0 };
    let vf = format!("fps={rate},scale={W}:{H}:flags=area,format=gray");
    let raw = capture(std::process::Command::new("ffmpeg").args(["-v", "error", "-i", path, "-vf", vf.as_str(), "-f", "rawvideo", "-"]),
        16 * 1024 * 1024, std::time::Duration::from_secs(60)).ok()?;
    let n = raw.len() / (W * H); if n == 0 { return None; }
    let (mut sharp, mut motion, mut hash) = (vec![], vec![], vec![]);
    for k in 0..n {
        let f = &raw[k * W * H..(k + 1) * W * H]; let px = |x: usize, y: usize| f[y * W + x] as f64;
        let (mut s1, mut s2, mut cnt) = (0.0, 0.0, 0.0);
        for y in 1..H - 1 { for x in 1..W - 1 { let l = 4.0 * px(x, y) - px(x - 1, y) - px(x + 1, y) - px(x, y - 1) - px(x, y + 1); s1 += l; s2 += l * l; cnt += 1.0; } }
        sharp.push(s2 / cnt - (s1 / cnt).powi(2));
        motion.push(if k == 0 { 0.0 } else { let p = &raw[(k - 1) * W * H..k * W * H]; f.iter().zip(p).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / (W * H) as f64 });
        // difference hash: 9x8 block means, one bit per horizontal neighbour pair
        let mut g = [[0.0f64; 9]; 8];
        for (by, row) in g.iter_mut().enumerate() { for (bx, v) in row.iter_mut().enumerate() {
            let (x0, x1, y0, y1) = (bx * W / 9, (bx + 1) * W / 9, by * H / 8, (by + 1) * H / 8); let mut acc = 0.0;
            for y in y0..y1 { for x in x0..x1 { acc += px(x, y); } } *v = acc / ((x1 - x0) * (y1 - y0)) as f64; } }
        let mut h = 0u64; for (by, row) in g.iter().enumerate() { for bx in 0..8 { if row[bx] < row[bx + 1] { h |= 1 << (by * 8 + bx); } } }
        hash.push(h);
    }
    Some(Look { rate, sharp, motion, hash })
}
impl Look {
    fn p90(&self) -> f64 { let mut v = self.sharp.clone(); v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); v[((v.len() as f64 * 0.9) as usize).min(v.len() - 1)].max(1.0) }
    /// soft = clearly out of focus (under a fifth of this clip's sharp frames) or a whip pan (fast change and under 55%):
    /// calibrated on the real eval clips, where low-detail close-ups sit at 0.24 and are kept
    fn soft(&self, k: usize, p90: f64) -> bool { let r = self.sharp[k] / p90; r < 0.2 || (self.motion[k] > 22.0 && r < 0.55) }
    fn frames(&self, s: f64, e: f64) -> std::ops::Range<usize> { let a = ((s * self.rate).ceil() as usize).min(self.sharp.len()); let b = ((e * self.rate).floor() as usize + 1).min(self.sharp.len()); a..b.max(a) }
}
fn hamming(a: u64, b: u64) -> u32 { (a ^ b).count_ones() }

// ---------------------------------------------------------------- auto_rough_cut
fn rough_cut(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let ids = str_list(a, "asset_ids")?;
    let fps = fps(ss);
    let start = snap(opt_num(a, "start", 0.0).max(0.0), fps);
    let max_pause = opt_num(a, "max_pause", 0.35).clamp(0.0, 3.0);
    let pad = opt_num(a, "pad", 0.06).clamp(0.0, 0.5);
    let mut drop: Vec<(String, usize)> = vec![];
    if let Some(d) = a.get("drop") {
        let arr = d.as_array().ok_or("drop: an array of {asset_id, index} (segment indexes from an earlier result)")?;
        for (i, x) in arr.iter().enumerate() {
            let aid = x.get("asset_id").and_then(|v| v.as_str()).ok_or(format!("drop[{i}]: asset_id is required"))?;
            let ix = x.get("index").and_then(|v| v.as_u64()).ok_or(format!("drop[{i}]: index (0-based segment number) is required"))?;
            drop.push((aid.to_string(), ix as usize));
        }
    }
    for id in &ids { asset(ss, id)?; }
    let soft_mode = a.get("soft").and_then(|v| v.as_str()).unwrap_or("trim").to_string();
    if !matches!(soft_mode.as_str(), "trim" | "flag") { return Err(format!("soft: \"trim\" (default: cut blurry or whip-pan edges, drop a mostly blurry segment) or \"flag\" (report only), not '{soft_mode}'")); }
    let dedupe = a.get("dedupe").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut kept_hash: Vec<(String, usize, u64)> = vec![];
    let tid = match a.get("track_id").and_then(|v| v.as_str()) {
        Some(t) => { let ti = track_index(ss, t)?; ss.project.tracks[ti].clips.retain(|c| c.start < start - 1e-6); t.to_string() }
        None => new_track(ss)?,
    };
    let (mut t, mut clips, mut segs, mut sp_tl) = (start, vec![], vec![], vec![]);
    for id in &ids {
        let (path, adur) = asset(ss, id)?;
        let mut sp = merge(speech(ss, json!({"path": path}), 1, 300.0, pad * 1000.0).map_err(|e| {
            if e == "no audio track" { format!("asset '{id}' ({path}) has no audio track, so auto_rough_cut can't detect speech on it; lay it with add_clip instead") } else { e }
        })?, max_pause);
        if sp.is_empty() { return Err(format!("no speech found in asset '{id}' ({path}); lay it with add_clip instead")); }
        // Only the very first kept segment of the whole output risks this (every later segment already has
        // real silence -- or another kept clip -- right before it, giving VAD a settle window it actually gets).
        if (t - start).abs() < 1e-6 { sp[0].0 = recover_missed_onset(ss, &path, sp[0].0); }
        let lk = look(&crate::deps::resolve_str(&path), adur); let p90 = lk.as_ref().map(|l| l.p90()).unwrap_or(1.0);
        let nseg = sp.len();
        for (i, (s0, e0)) in sp.iter().enumerate() {
            let (mut s, mut e) = (*s0, *e0);
            let mut seg = json!({"asset_id": id, "index": i, "source": [r3(s), r3(e)], "seconds": r3(e - s)});
            let mut dropped = drop.iter().any(|(d, ix)| d == id && *ix == i);
            if let Some(l) = &lk {
                let fr: Vec<usize> = l.frames(s, e).collect();
                if !fr.is_empty() {
                    let soft: Vec<bool> = fr.iter().map(|k| l.soft(*k, p90)).collect();
                    let share = soft.iter().filter(|x| **x).count() as f64 / soft.len() as f64;
                    seg["soft_share"] = json!(r3(share));
                    let lead = soft.iter().take_while(|x| **x).count(); let trail = soft.iter().rev().take_while(|x| **x).count();
                    if share > 0.0 { seg["soft"] = json!(soft.iter().zip(&fr).filter(|(x, _)| **x).map(|(_, k)| r3(*k as f64 / l.rate)).collect::<Vec<_>>()); }
                    if soft_mode == "trim" && !dropped {
                        if share > 0.6 && nseg > 1 { dropped = true; seg["dropped_reason"] = json!("blurry or whip pan for most of the segment"); }
                        else {
                            let limit = ((e - s) * 0.4).min(1.0);
                            let lt = if lead > 0 && lead < fr.len() { fr[lead] as f64 / l.rate - s } else { 0.0 };
                            let tt = if trail > 0 && trail < fr.len() { e - fr[fr.len() - 1 - trail] as f64 / l.rate } else { 0.0 };
                            if lt > 0.0 && lt <= limit { s += lt; seg["trimmed_start"] = json!(r3(lt)); }
                            if tt > 0.0 && tt <= limit && e - tt > s + 0.3 { e -= tt; seg["trimmed_end"] = json!(r3(tt)); }
                        }
                    }
                    // near-duplicate: the sharpest frame of this segment against kept segments of other assets
                    if let Some(k) = fr.iter().copied().max_by(|a, b| l.sharp[*a].partial_cmp(&l.sharp[*b]).unwrap_or(std::cmp::Ordering::Equal)) {
                        let h = l.hash[k];
                        if let Some((oa, oi, _)) = kept_hash.iter().find(|(oa, _, oh)| oa != id && hamming(*oh, h) <= 6) {
                            seg["similar_to"] = json!({"asset_id": oa, "index": oi});
                            if dedupe && !dropped { dropped = true; seg["dropped_reason"] = json!("looks like a segment already in the cut"); }
                        }
                        if !dropped { kept_hash.push((id.clone(), i, h)); }
                    }
                }
            }
            seg["dropped"] = json!(dropped);
            segs.push(seg);
            if dropped { continue; }
            let s2 = ((s * fps) + 1e-6).floor() / fps; let e2 = (((e * fps) - 1e-6).ceil() / fps).min(adur);
            let d = snap(e2 - s2, fps); if d < 2.0 / fps { continue; }
            let cid = call(ss, "add_clip", &json!({"track_id": tid, "source_type": "media", "asset_id": id, "start": r3(t), "duration": d, "in_point": r3(s2), "fit": "cover"}))?["clip_id"].as_str().unwrap_or("").to_string();
            clips.push(json!({"clip_id": cid, "asset_id": id, "start": r3(t), "end": r3(t + d), "in_point": r3(s2)}));
            sp_tl.push((t + (s - s2), t + (e - s2).min(d)));
            t = snap(t + d, fps);
        }
    }
    if clips.is_empty() { return Err("every speech segment was dropped; nothing was laid".into()); }
    Ok(json!({"track_id": tid, "end": r3(t), "duration": r3(t - start), "clips": clips, "speech": pairs(&sp_tl), "segments": segs,
        "next": "b-roll: place_broll {track_id}; captions: caption_from_words {track_id, by_asset: {asset_id: words}}; a false start: run again with drop [{asset_id, index}]"}))
}

// ---------------------------------------------------------------- place_broll
// A small, honest, curated concept table -- no ML: a b-roll concept maps to the words that, spoken
/// in the dialogue right then, suggest that cutaway fits ("work"/"computer" -> a typing shot). Deliberately
/// narrow and plain-English; it is a keyword/synonym match, not a classifier.
const BROLL_SYNONYMS: &[(&str, &[&str])] = &[
    ("typing", &["typing", "type", "types", "typed", "keyboard", "computer", "laptop", "code", "coding", "coder", "work", "working", "email", "emails", "screen", "desk"]),
    ("computer", &["typing", "type", "keyboard", "laptop", "code", "coding", "screen", "work", "working", "desk", "email"]),
    ("city", &["city", "street", "streets", "outside", "outdoors", "urban", "traffic", "walking", "walk", "sidewalk", "downtown", "town", "building", "buildings"]),
    ("street", &["city", "street", "streets", "outside", "traffic", "walking", "walk", "sidewalk", "downtown", "car", "cars"]),
    ("coffee", &["coffee", "cafe", "drink", "drinking", "tea", "cup", "morning", "brew", "espresso"]),
    ("cafe", &["coffee", "cafe", "drink", "drinking", "tea", "cup", "morning", "brew"]),
    ("food", &["food", "eating", "eat", "meal", "lunch", "dinner", "breakfast", "cooking", "kitchen"]),
    ("nature", &["nature", "outside", "outdoors", "trees", "tree", "park", "walk", "walking", "green", "forest"]),
    ("phone", &["phone", "call", "calling", "texting", "text", "mobile", "message", "messages"]),
    ("travel", &["travel", "traveling", "trip", "flight", "airport", "plane", "journey"]),
    ("meeting", &["meeting", "team", "office", "discussion", "talk", "talking", "presentation"]),
    ("gym", &["gym", "workout", "exercise", "fitness", "training", "run", "running"]),
    ("music", &["music", "song", "listening", "headphones", "playing", "instrument"]),
];
const BROLL_STOPWORDS: &[&str] = &["real", "final", "clip", "clips", "video", "footage", "asset", "assets", "media", "file", "files", "the", "and", "for", "cam", "camera", "shot", "take"];
fn norm_tok(w: &str) -> String { w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase() }
/// Honest fallback metadata when no explicit tag was given: whatever the person who shot it called the file.
fn filename_keywords(path: &str) -> Vec<String> {
    let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.split(|c: char| !c.is_alphanumeric()).map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() > 2 && !s.chars().all(|c| c.is_ascii_digit()) && !BROLL_STOPWORDS.contains(&s.as_str()))
        .collect()
}
fn expand_keywords(tags: &[String]) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> = tags.iter().map(|t| norm_tok(t)).collect();
    for t in tags { let nt = norm_tok(t); for (concept, syns) in BROLL_SYNONYMS {
        if *concept == nt || syns.contains(&nt.as_str()) { out.insert((*concept).to_string()); for s in *syns { out.insert((*s).to_string()); } }
    } }
    out
}

// ---------------------------------------------------------------- place_broll
/// V7.9Q: a short, purely procedural "whoosh" cue for a b-roll cut-in or a title-in -- same spirit as
/// add_keyword_pops' synth_pop_sfx (0ae47c0): generated at call time into the OS temp dir, same bytes every
/// time (deterministic), never a bundled/committed sample. A rising-pitch tone (300Hz -> ~2.3kHz over 0.22s)
/// with a fast exponential decay and a gentle lowpass to soften it into a swish rather than a harsh sweep.
/// Graceful fail-open: returns None if ffmpeg can't run, so a cut-in is never blocked on sound.
pub(crate) fn synth_whoosh_sfx() -> Option<String> {
    let path = std::env::temp_dir().join("kimo-whoosh-sfx-v1.wav");
    if path.exists() { return Some(path.to_str()?.to_string()); }
    let src = "aevalsrc='0.6*sin(2*PI*(300+9000*t)*t)*exp(-9*t)':s=48000:d=0.22";
    let af = "lowpass=f=4200,afade=t=in:d=0.008,afade=t=out:st=0.17:d=0.05";
    let ok = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", src, "-af", af, path.to_str()?])
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if ok { Some(path.to_str()?.to_string()) } else { None }
}
/// Places one ducked whoosh clip at `at_s` on a (lazily created, reused) dedicated SFX track. volume 0.3 keeps
/// it clearly under normal dialogue level (dialogue clips land near 1.0x after finish_audio's own leveling) --
/// "ducked under the voice" by fixed low volume rather than a dynamic sidechain, which is honest and simple
/// given no real-time analysis of the final mix is available at placement time. Fails open (a warning, not an
/// error) exactly like add_keyword_pops' sfx -- a missing sound must never block the cut or title it decorates.
pub(crate) fn place_whoosh(ss: &mut Session, sfx_track: &mut Option<String>, asset_id: &mut Option<String>, at_s: f64, warnings: &mut Vec<String>) -> Option<String> {
    if asset_id.is_none() {
        match synth_whoosh_sfx() {
            Some(path) => match call(ss, "import_media", &json!({"path": path})) {
                Ok(v) => *asset_id = v["asset_id"].as_str().map(String::from),
                Err(e) => { warnings.push(format!("whoosh_sfx: could not import the synthesized sound ({e}); skipped")); return None; }
            },
            None => { warnings.push("whoosh_sfx: could not be synthesized here (ffmpeg unavailable or failed); skipped".into()); return None; }
        }
    }
    let aid = asset_id.as_ref()?;
    if sfx_track.is_none() { *sfx_track = call(ss, "add_track", &json!({})).ok()?["track_id"].as_str().map(String::from); }
    let tid = sfx_track.as_ref()?;
    // V7.9Q: 0.3 cost whisper recall 90.0% -> 86.7% on the real benchmark even on its own track; lowering to
    // 0.15 made it WORSE (80.0%), not better -- this is not a simple louder-is-worse masking curve, and chasing
    // a third volume blind is not a verified fix. Left at the level actually meant to be heard (0.3, a real
    // whoosh, not inaudible) since whoosh_sfx now defaults to false; a caller who turns it on gets the real
    // effect and the recall cost is disclosed in the arg description, FEATURES.md and CHANGELOG.md.
    let c = call(ss, "add_audio_clip", &json!({"asset_id": aid, "track_id": tid, "start": r3(at_s), "duration": 0.22, "volume": 0.3})).ok()?;
    c["clip_id"].as_str().map(String::from)
}
fn place_broll(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let tid = str_arg(a, "track_id")?.to_string(); track_index(ss, &tid)?;
    let ids = str_list(a, "asset_ids")?;
    for id in &ids { asset(ss, id)?; }
    let fps = fps(ss);
    let min_shot = opt_num(a, "min_shot", 1.0).clamp(0.3, 10.0);
    let max_shot = opt_num(a, "max_shot", 2.5).max(min_shot);
    let phrases = speech(ss, json!({"track_id": tid}), 1, 250.0, 60.0)?;
    if phrases.is_empty() { return Err(format!("no speech detected on track '{tid}'; place_broll lays b-roll only over someone speaking (give the dialogue track)")); }
    let w0 = match a.get("start").and_then(|v| v.as_f64()) { Some(v) => v, None => if phrases.len() > 1 { phrases[0].1 } else { phrases[0].0 + (phrases[0].1 - phrases[0].0) * 0.3 } };
    let w1 = a.get("end").and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY);
    // pauses between words or phrases: the cut points inside speech
    let fine = speech(ss, json!({"track_id": tid}), 2, 80.0, 0.0)?;
    let cands: Vec<f64> = fine.windows(2).map(|w| (w[0].1 + w[1].0) / 2.0).collect();
    let mut slots: Vec<(f64, f64)> = vec![];
    for (ps, pe) in &phrases {
        let (s, e) = (((ps.max(w0)) * fps - 1e-6).ceil() / fps, ((pe.min(w1)) * fps + 1e-6).floor() / fps);
        if e - s < min_shot { continue; }
        let mut cur = s;
        while e - cur >= min_shot - 1e-9 {
            let pick = cands.iter().copied().filter(|c| *c >= cur + min_shot && *c <= cur + max_shot && *c <= e - min_shot * 0.5).last();
            let end = match pick { Some(c) => snap(c, fps), None => if e - cur <= max_shot { e } else { snap(cur + max_shot, fps) } };
            if end - cur < 2.0 / fps { break; }
            slots.push((cur, end)); cur = end;
        }
    }
    if slots.is_empty() { return Err(format!("no speech of {min_shot} s or more on track '{tid}' between {w0:.2} s and {w1:.2} s; lower min_shot or widen start/end")); }

    // B-roll relevance (RV-33): pick the clip whose name/tags best fit the words spoken right around the
    // cutaway, instead of handing them out in the order they were given. words is the transcript in TIMELINE
    // seconds (auto_edit supplies its own transcript here); asset_tags is explicit honest metadata per asset,
    // falling back to whatever the clip's own filename says. A pure keyword/synonym match -- no ML, and when
    // there is nothing to match on (no words given, or only one asset) it behaves exactly as before: given order.
    let words: Vec<(f64, f64, String)> = match a.get("words") {
        None | Some(Value::Null) => vec![],
        Some(v) => v.as_array().ok_or("words must be an array of {text, start, end} (timeline seconds)")?
            .iter().filter_map(|x| Some((x.get("start")?.as_f64()?, x.get("end")?.as_f64()?, x.get("text")?.as_str()?.to_string()))).collect(),
    };
    let explicit_tags: std::collections::HashMap<String, Vec<String>> = match a.get("asset_tags") {
        None | Some(Value::Null) => Default::default(),
        Some(v) => v.as_object().ok_or("asset_tags must be an object of {asset_id: [tag, ...]}")?
            .iter().filter_map(|(k, v)| Some((k.clone(), v.as_array()?.iter().filter_map(|t| t.as_str().map(String::from)).collect()))).collect(),
    };
    let n_pick = slots.len().min(ids.len());
    let mut assignment: Vec<usize> = (0..n_pick).collect(); // default: given order (unchanged behaviour)
    let mut relevance_notes: Vec<String> = vec![];
    let relevance_reason = if words.is_empty() { Some("no words given (timeline-second transcript); placed in the given order".to_string()) }
        else if ids.len() < 2 { Some("only one b-roll asset; nothing to choose between".to_string()) } else { None };
    if relevance_reason.is_none() {
        let asset_keywords: Vec<std::collections::HashSet<String>> = ids.iter().map(|id| {
            let tags = explicit_tags.get(id).cloned().unwrap_or_else(|| { let (path, _) = asset(ss, id).unwrap_or_default(); filename_keywords(&path) });
            expand_keywords(&tags)
        }).collect();
        const PAD: f64 = 0.4;
        let slot_words: Vec<Vec<String>> = slots.iter().take(n_pick).map(|(s, e)| {
            words.iter().filter(|(ws, we, _)| *we >= s - PAD && *ws <= e + PAD).map(|(_, _, t)| norm_tok(t)).filter(|t| !t.is_empty()).collect()
        }).collect();
        let mut scored: Vec<(usize, usize, usize)> = vec![]; // (score, slot_idx, asset_idx)
        for (si, sw) in slot_words.iter().enumerate() { for (ai, kw) in asset_keywords.iter().enumerate() {
            let sc = sw.iter().filter(|w| kw.contains(*w)).count(); if sc > 0 { scored.push((sc, si, ai)); }
        } }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))); // best score first, stable otherwise
        let mut assign = vec![usize::MAX; n_pick]; let mut used_asset = vec![false; ids.len()];
        for (sc, si, ai) in scored {
            if assign[si] != usize::MAX || used_asset[ai] { continue; }
            assign[si] = ai; used_asset[ai] = true;
            relevance_notes.push(format!("slot {:.1}-{:.1}s matched '{}' (score {sc})", slots[si].0, slots[si].1, ids[ai]));
        }
        let mut leftovers = (0..ids.len()).filter(|ai| !used_asset[*ai]);
        for slot in assign.iter_mut() { if *slot == usize::MAX { *slot = leftovers.next().unwrap_or(0); } }
        assignment = assign;
    }

    let bt = new_track(ss)?;
    let vol = opt_num(a, "volume", 0.0).clamp(0.0, 1.0);
    // V7.9Q: a subtle whoosh on each b-roll cut-in, on by default (it is the kind of small polish top editors
    // always have and a raw auto-cut never does). Synthesized once (synth_whoosh_sfx), ducked to 0.3 volume so
    // it sits under dialogue, never blocks a cut if it can't be made (fails open with a warning).
    // V7.9Q: measured on the real benchmark that even a ducked whoosh costs whisper word recall right at the
    // cut-in (0.3 volume: 90.0% -> 86.7%; 0.15 volume: 90.0% -> 80.0%, WORSE, not better -- a b-roll cut-in by
    // this function's own design always lands during active speech, so any added sound there is a real risk,
    // not just a loud one). Defaults to false until that is resolved; the feature is complete and opt-in.
    let whoosh_sfx = a.get("whoosh_sfx").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut whoosh_track: Option<String> = None; let mut whoosh_asset: Option<String> = None; let mut warnings: Vec<String> = vec![];
    let mut clips = vec![]; let mut used_idx: std::collections::HashSet<usize> = Default::default();
    for (k, (s, e)) in slots.iter().enumerate() {
        if k >= assignment.len() { break; }
        let ai = assignment[k]; let id = &ids[ai]; let (_, adur) = asset(ss, id)?;
        let mut d = e - s;
        let ip_given = a.get("in_points").and_then(|m| m.get(id.as_str())).and_then(|v| v.as_f64());
        let mut ip = ip_given.unwrap_or(((adur - d) / 2.0).max(0.0)).max(0.0);
        if ip + d > adur { ip = (adur - d).max(0.0); d = d.min(adur - ip); }
        let (ip, d) = (snap(ip, fps), snap(d, fps)); if d < 2.0 / fps { continue; }
        let cid = call(ss, "add_clip", &json!({"track_id": bt, "source_type": "media", "asset_id": id, "start": r3(*s), "duration": d, "in_point": ip, "fit": "cover"}))?["clip_id"].as_str().unwrap_or("").to_string();
        if vol <= 0.0 { call(ss, "set_audio", &json!({"clip_id": cid, "mute": true}))?; } else { call(ss, "set_audio", &json!({"clip_id": cid, "volume": vol}))?; }
        if whoosh_sfx { place_whoosh(ss, &mut whoosh_track, &mut whoosh_asset, *s, &mut warnings); }
        clips.push(json!({"clip_id": cid, "asset_id": id, "start": r3(*s), "end": r3(s + d), "in_point": ip})); used_idx.insert(ai);
    }
    let unused_assets: Vec<&String> = ids.iter().enumerate().filter(|(i, _)| !used_idx.contains(i)).map(|(_, id)| id).collect();
    let mut out = json!({"track_id": bt, "clips": clips, "speech": pairs(&phrases), "unused_assets": unused_assets,
        "audio": if vol <= 0.0 { "b-roll muted, dialogue runs underneath" } else { "b-roll at the given volume" },
        "broll_relevance": relevance_reason.map(|r| json!({"active": false, "reason": r})).unwrap_or_else(|| json!({"active": true, "matches": relevance_notes}))});
    if whoosh_sfx { out["whoosh_sfx_track_id"] = json!(whoosh_track); }
    if !warnings.is_empty() { out["warnings"] = json!(warnings); }
    Ok(out)
}

// -
// ---------------------------------------------------------------- caption_from_words
/// Balanced lines: fewest-slack breaks (dynamic programming over word breaks), a lone word only when the text is one word,
/// a comma a preferred break; greedy filling left cues like "leaks." on their own.
fn wrap(s: &str, max_chars: usize) -> Vec<String> {
    let w: Vec<&str> = s.split_whitespace().collect(); let nw = w.len(); let m = max_chars as i64;
    let mut best = vec![i64::MAX; nw + 1]; let mut prev = vec![0usize; nw + 1]; best[0] = 0;
    for i in 1..=nw { for j in (0..i).rev() {
        let ln = w[j..i].iter().map(|x| x.chars().count()).sum::<usize>() as i64 + (i - j - 1) as i64;
        if ln > m && i - j > 1 { break; }
        let mut c = if ln <= m { (m - ln) * (m - ln) } else { 0 };
        if i - j == 1 && nw > 1 { c += m * m; }
        if i < nw && w[i - 1].ends_with(',') { c -= m * m / 4; }
        if best[j] != i64::MAX && best[j] + c < best[i] { best[i] = best[j] + c; prev[i] = j; }
    } }
    let mut lines: Vec<String> = vec![]; let mut i = nw;
    while i > 0 { lines.push(w[prev[i]..i].join(" ")); i = prev[i]; }
    lines.reverse(); lines
}
fn chunk_text(text: &str, max_chars: usize, lines_per_cue: usize) -> Vec<(usize, String)> {
    // sentences, then lines of <= max_chars, `lines_per_cue` lines per cue; (sentence index, cue text)
    let mut sents: Vec<String> = vec![]; let mut cur = String::new();
    for ch in text.replace('\n', " ").chars() { cur.push(ch); if matches!(ch, '.' | '!' | '?') { if !cur.trim().is_empty() { sents.push(cur.trim().to_string()); } cur.clear(); } }
    if !cur.trim().is_empty() { sents.push(cur.trim().to_string()); }
    let mut out = vec![];
    for (si, s) in sents.iter().enumerate() {
        let lines = wrap(s, max_chars);
        for pair in lines.chunks(lines_per_cue.max(1)) { out.push((si, pair.join("\n"))); }
    }
    out
}
/// speech-time (seconds of speech only) to timeline seconds over the speech intervals
fn speech_to_tl(st: f64, iv: &[(f64, f64)]) -> f64 {
    let mut acc = 0.0;
    for (s, e) in iv { let d = e - s; if st <= acc + d + 1e-9 { return s + (st - acc).max(0.0); } acc += d; }
    iv.last().map(|x| x.1).unwrap_or(0.0)
}
fn caption_cues(text: &str, iv: &[(f64, f64)], max_chars: usize, lines_per_cue: usize) -> Vec<(f64, f64, String)> {
    let chunks = chunk_text(text, max_chars, lines_per_cue); if chunks.is_empty() || iv.is_empty() { return vec![]; }
    let total: f64 = iv.iter().map(|(s, e)| e - s).sum();
    let w: Vec<f64> = chunks.iter().map(|(_, t)| t.chars().filter(|c| !c.is_whitespace()).count() as f64 + 2.0).collect();
    let wsum: f64 = w.iter().sum();
    // phrase boundaries in speech-time; a sentence boundary close to one snaps to it (a speaker change or a pause)
    let mut pb = vec![]; let mut acc = 0.0; for (s, e) in iv { acc += e - s; pb.push(acc); }
    let mut bounds = vec![0.0]; let mut c = 0.0;
    for i in 0..chunks.len() {
        c += w[i] / wsum * total;
        // a cue ends on a speech pause when one is near: within 0.6 s at a sentence end, 0.35 s inside a sentence
        let sent_end = i + 1 == chunks.len() || chunks[i + 1].0 != chunks[i].0;
        let reach = if sent_end { 0.6 } else { 0.35 };
        let b = pb.iter().copied().min_by(|x, y| (x - c).abs().partial_cmp(&(y - c).abs()).unwrap_or(std::cmp::Ordering::Equal)).filter(|p| (p - c).abs() < reach).unwrap_or(c);
        bounds.push(b.max(*bounds.last().unwrap()));
    }
    let n = chunks.len(); bounds[n] = total;
    let mut out: Vec<(f64, f64, String)> = vec![];
    for i in 0..n {
        let (a, b) = (bounds[i], bounds[i + 1]);
        let mut s = speech_to_tl(a + 1e-6, iv); let e = speech_to_tl(b, iv);
        if let Some(l) = out.last() { s = s.max(l.1); }
        let e = e.max(s + 0.3);
        out.push((r3(s), r3(e), chunks[i].1.clone()));
    }
    for i in 1..out.len() { if out[i].0 < out[i - 1].1 { out[i - 1].1 = out[i].0; } }
    out.retain(|x| x.1 - x.0 > 0.05);
    // no flash cues: one shorter than 0.8 s joins its shorter neighbour (re-wrapped) when that stays within lines + 1
    let mut k = 0;
    while k < out.len() {
        if out[k].1 - out[k].0 >= 0.8 || out.len() < 2 { k += 1; continue; }
        let left = if k > 0 { Some(out[k - 1].1 - out[k - 1].0) } else { None }; let right = out.get(k + 1).map(|x| x.1 - x.0);
        let j = match (left, right) { (Some(l), Some(r)) => if l <= r { k - 1 } else { k + 1 }, (Some(_), None) => k - 1, (None, _) => k + 1 };
        let (a0, b0) = (j.min(k), j.max(k));
        let joined = format!("{} {}", out[a0].2.replace('\n', " "), out[b0].2.replace('\n', " "));
        let lines = wrap(&joined, max_chars);
        if lines.len() > lines_per_cue + 1 { k += 1; continue; }
        out[a0] = (out[a0].0, out[b0].1, lines.join("\n")); out.remove(b0);
        k = a0;
    }
    out
}
fn rect_hits(r: [f64; 4], z: [f64; 4]) -> bool { r[0] < z[0] + z[2] - 4.0 && r[0] + r[2] > z[0] + 4.0 && r[1] < z[1] + z[3] - 4.0 && r[1] + r[3] > z[1] + 4.0 }
fn rect4(v: &Value) -> Option<[f64; 4]> { let a = v.as_array()?; Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?]) }
fn zones(ss: &mut Session, platform: &str) -> Result<(Vec<[f64; 4]>, [f64; 4]), String> {
    let ts = call(ss, "query_safe_margins", &json!({"profile": "title_safe"}))?;
    let safe = ts["zones"].as_array().and_then(|z| z.iter().find(|x| x["kind"] == "safe")).and_then(|x| rect4(&x["rect"])).unwrap_or([0.0, 0.0, ss.project.width as f64, ss.project.height as f64]);
    if platform == "none" { return Ok((vec![], safe)); }
    let z = call(ss, "query_safe_margins", &json!({"profile": platform}))?;
    Ok((z["zones"].as_array().map(|v| v.iter().filter(|x| x["kind"] == "avoid").filter_map(|x| rect4(&x["rect"])).collect()).unwrap_or_default(), safe))
}
fn text_rect(ss: &mut Session, cid: &str, t: f64) -> Option<([f64; 4], bool)> {
    let b = call(ss, "compute_text_bounds", &json!({"clip_id": cid, "t": r3(t.max(0.0))})).ok()?;
    let mut r = rect4(&b["canvas_rect"])?;
    // compute_text_bounds measures ink only, by design ("ink excludes shadow, glow and background box" --
    // the right answer for a safe-area/ink check, the wrong one for "does this collide with other text on
    // screen": a boxed/pop caption's actual visible pill is ink padded by its own TextFx.bg_pad, scaled by
    // the clip's own transform. Grown here, not in compute_text_bounds, because only the collision checks
    // below need the visible box; everything else that calls compute_text_bounds wants the ink.
    // (A title preset whose pill is a SEPARATE shape clip, not a TextFx background -- hook_box, subscribe_pop
    // -- is not covered by this: its box stays invisible to this text-only scan. Known, not fixed here.)
    if let Some(c) = ss.project.find_clip(cid) { if let Some(fx) = &c.text_fx { if fx.bg.is_some() {
        let scale = crate::render::resolve_xform(&ss.project, c, t.max(0.0)).scale; let pad = fx.bg_pad * scale;
        r = [r[0] - pad, r[1] - pad, r[2] + 2.0 * pad, r[3] + 2.0 * pad];
    } } }
    Some((r, b["font"]["substituted"].as_bool().unwrap_or(false)))
}
fn clip_span(ss: &Session, cid: &str) -> Option<(f64, f64)> { ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| c.id == cid).map(|c| (c.start, c.duration)) }
fn caption_from_words(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let tid = str_arg(a, "track_id")?.to_string(); track_index(ss, &tid)?;
    let by_asset = a.get("by_asset").and_then(|v| v.as_object()).cloned();
    let text = a.get("text").and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()).map(String::from).unwrap_or_default();
    if text.is_empty() && by_asset.is_none() { return Err("text: the words spoken on the track, in order (from the script or brief), or by_asset {asset_id: words} per source clip. Speech-to-text needs the whisper CLI: add_captions {source: \"transcribe\"}".into()); }
    let style = a.get("style").and_then(|v| v.as_str()).unwrap_or("boxed").to_string();
    let platform = a.get("platform").and_then(|v| v.as_str()).unwrap_or("tiktok").to_string();
    let max_chars = opt_num(a, "max_chars", 22.0).clamp(6.0, 60.0) as usize;
    let lines = opt_num(a, "lines", 2.0).clamp(1.0, 3.0) as usize;
    let check_confidence = bool_arg(a, "check_confidence")?.unwrap_or(true);
    let mut iv = speech(ss, json!({"track_id": tid}), 1, 200.0, 40.0)?;
    if iv.is_empty() { return Err(format!("no speech detected on track '{tid}'; give the dialogue track")); }
    // a cut on the dialogue track is a phrase boundary even when the detector hears no pause there (the dead air was cut out)
    let cuts: Vec<f64> = ss.project.tracks[track_index(ss, &tid)?].clips.iter().map(|c| c.start).filter(|t| *t > 0.0).collect();
    for b in cuts { if let Some(k) = iv.iter().position(|(s, e)| *s + 0.15 < b && b + 0.15 < *e) { let (s, e) = iv[k]; iv[k] = (s, b); iv.insert(k + 1, (b, e)); } }
    // cues carry the asset id they came from (empty: the plain `text` whole-track form) so a confidence check below can be
    // scoped to the right clip(s); by_asset keeps captions from drifting across a cut the same way it always did.
    let cues: Vec<(f64, f64, String, String)> = if let Some(map) = &by_asset {
        let ti = track_index(ss, &tid)?;
        let spans: Vec<(String, f64, f64)> = ss.project.tracks[ti].clips.iter().filter_map(|c| if let Source::Media { asset } = &c.source { Some((asset.clone(), c.start, c.start + c.duration)) } else { None }).collect();
        let mut all: Vec<(f64, f64, String, String)> = vec![]; let mut seen = std::collections::HashSet::new();
        for (aid, _, _) in &spans {
            if !seen.insert(aid.clone()) { continue; }
            let Some(words) = map.get(aid).and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()) else { continue };
            let mine: Vec<(f64, f64)> = spans.iter().filter(|(x, _, _)| x == aid).map(|(_, s0, e0)| (*s0, *e0)).collect();
            let mut sub: Vec<(f64, f64)> = vec![];
            for (s0, e0) in &mine { for (s, e) in &iv { let (a0, b0) = (s.max(*s0), e.min(*e0)); if b0 - a0 > 0.05 { sub.push((a0, b0)); } } }
            if sub.is_empty() { sub = mine; }
            all.extend(caption_cues(words, &sub, max_chars, lines).into_iter().map(|(s, e, t)| (s, e, t, aid.clone())));
        }
        let unknown: Vec<&String> = map.keys().filter(|k| !spans.iter().any(|(x, _, _)| &x == k)).collect();
        if !unknown.is_empty() { return Err(format!("by_asset: {unknown:?} have no clips on track '{tid}'; use the asset_ids that auto_rough_cut returned")); }
        all.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal)); all
    } else { caption_cues(&text, &iv, max_chars, lines).into_iter().map(|(s, e, t)| (s, e, t, String::new())).collect() };
    if cues.is_empty() { return Err("text has no words to caption".into()); }
    let ct = new_track(ss)?;
    let mut args = json!({"track_id": ct, "style": style, "cues": cues.iter().map(|(s, e, t, _)| json!({"start": s, "end": e, "text": t})).collect::<Vec<_>>()});
    if let Some(sz) = a.get("size").and_then(|v| v.as_f64()) { args["size"] = json!(sz); }
    let r = call(ss, "add_caption_cues", &args)?;
    let cap = r["caption_id"].as_str().unwrap_or("").to_string();
    let ids: Vec<String> = r["clip_ids"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let (avoid, safe) = zones(ss, &platform)?;
    let (w, h) = (ss.project.width as f64, ss.project.height as f64);
    let size0 = ids.first().and_then(|id| ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| &c.id == id)).and_then(|c| if let Source::Text { size, .. } = &c.source { Some(*size) } else { None }).unwrap_or(h.min(w) / 15.0);
    let (mut size, mut y) = (size0, if h > w { h * 0.25 } else { h * 0.35 });
    let measure = |ss: &mut Session| -> Vec<[f64; 4]> { ids.iter().filter_map(|id| { let (_, d) = clip_span(ss, id)?; text_rect(ss, id, d / 2.0).map(|x| x.0) }).collect() };
    let mut inside = true;
    if platform != "none" {
        for _ in 0..3 {
            let rects = measure(ss); if rects.is_empty() { break; }
            let maxw = rects.iter().map(|r| r[2]).fold(0.0, f64::max);
            // widest a centred line can be: clear of every avoid zone it spans vertically, inside title safe
            let lo = rects.iter().map(|r| r[1]).fold(f64::INFINITY, f64::min); let hi = rects.iter().map(|r| r[1] + r[3]).fold(0.0, f64::max);
            let mut half = (w / 2.0 - safe[0]).min(safe[0] + safe[2] - w / 2.0);
            for z in &avoid { if z[1] < h * 0.6 && z[1] + z[3] > h * 0.9 { if z[0] > w / 2.0 { half = half.min(z[0] - w / 2.0 - 8.0); } else if z[0] + z[2] < w / 2.0 { half = half.min(w / 2.0 - (z[0] + z[2]) - 8.0); } } }
            let _ = (lo, hi);
            if maxw > 2.0 * half { size = (size * 2.0 * half / maxw * 0.97).floor(); }
            // bottom edge: above the lowest avoid band that covers the frame's lower part
            let bottom_lim = avoid.iter().filter(|z| z[1] > h * 0.5 && z[0] < w / 2.0 && z[0] + z[2] > w / 2.0 - 1.0).map(|z| z[1]).fold(safe[1] + safe[3], f64::min) - 16.0;
            let maxb = rects.iter().map(|r| r[1] + r[3]).fold(0.0, f64::max);
            let scale = if maxw > 0.0 { (size / size0.max(1.0)).min(1.0) } else { 1.0 };
            let _ = scale;
            if maxb > bottom_lim || maxw > 2.0 * half { y -= (maxb - bottom_lim).max(0.0); }
            call(ss, "set_caption_style", &json!({"caption_id": cap, "style": style, "size": size, "y": r3(y)}))?;
            let rects = measure(ss);
            inside = !rects.is_empty() && rects.iter().all(|r| !avoid.iter().any(|z| rect_hits(*r, *z)) && r[0] >= safe[0] - 4.0 && r[0] + r[2] <= safe[0] + safe[2] + 4.0 && r[1] >= safe[1] - 4.0 && r[1] + r[3] <= safe[1] + safe[3] + 4.0);
            if inside { break; }
        }
    }
    // best-effort: cross-check each cue's given words against what kimo's own recognizer actually hears on that stretch of
    // audio (same Hinglish-tuned pipeline auto_captions uses - beam search, Hinglish prompt, Hindi pass). This never needs the
    // given text to be transcribed correctly by anyone; it only asks "did the recognizer struggle with this audio?" low
    // confidence there is flagged (tag low_confidence_caption on the cue clip) so lint_edit can tell a small model exactly
    // which captions to double-check, instead of it ever putting unverified nonsense on screen. Silent no-op with no
    // recognizer installed - caption_from_words has never required one.
    let mut low_confidence_cues: Vec<String> = vec![];
    let mut confidence_checked = false;
    if check_confidence {
        let hint = a.get("hint").and_then(|v| v.as_str()).unwrap_or("");
        let mut conf_cache: HashMap<String, Option<f32>> = HashMap::new();
        for (id, (_, _, _, aid)) in ids.iter().zip(cues.iter()) {
            let ti = track_index(ss, &tid)?;
            let clip_ids: Vec<String> = if aid.is_empty() {
                ss.project.tracks[ti].clips.iter().filter(|c| matches!(c.source, Source::Media { .. })).map(|c| c.id.clone()).collect()
            } else {
                ss.project.tracks[ti].clips.iter().filter(|c| matches!(&c.source, Source::Media { asset } if asset == aid)).map(|c| c.id.clone()).collect()
            };
            let key = if aid.is_empty() { "__track__".to_string() } else { aid.clone() };
            let conf = conf_cache.entry(key).or_insert_with(|| asr_confidence(ss, a, &clip_ids, hint)).clone();
            if let Some(c) = conf {
                confidence_checked = true;
                if c < 0.5 {
                    if let Some(cl) = ss.project.find_clip_mut(id) { if !cl.tags.iter().any(|t| t == "low_confidence_caption") { cl.tags.push("low_confidence_caption".into()); } }
                    low_confidence_cues.push(id.clone());
                }
            }
        }
    }
    let out: Vec<Value> = cues.iter().zip(ids.iter()).map(|((s, e, t, _), id)| json!({"clip_id": id, "start": s, "end": e, "text": t, "low_confidence": low_confidence_cues.contains(id)})).collect();
    Ok(json!({"caption_id": cap, "track_id": ct, "cues": out, "size": r3(size), "y": r3(y), "platform": platform, "inside_safe_area": inside,
        "low_confidence_cues": low_confidence_cues, "confidence_checked": confidence_checked,
        "note": "cues follow the detected speech; fix a word with edit_caption {clip_id, text}. low_confidence_cues are cues whose audio kimo's recognizer genuinely struggled with; lint_edit surfaces the same thing as caption_low_confidence"}))
}

/// Best-effort ASR confidence for words `caption_from_words` was *given* (not recognized): reuses auto_captions's already-tuned
/// recognizer (Hinglish prompt, beam search by default, a Hindi pass) to see how confidently it hears the SAME audio, whatever
/// text it was given. None (never an error) when no recognizer is installed or none of the clips carry usable audio -
/// caption_from_words has always worked without a recognizer, and this must not change that. `a` carries through any
/// whisper_bin/model/language/script overrides the caller already passed to caption_from_words.
fn asr_confidence(ss: &Session, a: &Value, clip_ids: &[String], hint: &str) -> Option<f32> {
    if clip_ids.is_empty() { return None; }
    let mut ca = a.clone(); ca["clip_ids"] = json!(clip_ids); if !hint.trim().is_empty() { ca["hint"] = json!(hint); }
    let mut warn = vec![];
    let srcs = super::auto_captions::sources(ss, &ca, &mut warn).ok()?;
    let mut mode = String::new(); let mut lang_seen: Option<String> = None;
    let mut all: Vec<f32> = vec![];
    for s in &srcs { if let Ok((ws, _)) = super::auto_captions::recognize(s, &ca, &mut mode, &mut lang_seen) { all.extend(ws.iter().map(|w| w.conf)); } }
    if all.is_empty() { return None; }
    Some(all.iter().sum::<f32>() / all.len() as f32)
}

// ---------------------------------------------------------------- finish_audio

fn clip_lufs(ss: &mut Session, cid: &str) -> Option<f64> { call(ss, "measure_loudness", &json!({"clip_id": cid})).ok()?["integrated_lufs"].as_f64() }
fn finish_audio(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let target = opt_num(a, "target_lufs", -14.0).clamp(-30.0, -8.0);
    let keep_amb = a.get("keep_ambience").and_then(|v| v.as_bool()).unwrap_or(false);
    let bed = opt_num(a, "music_under_speech_db", -14.0).clamp(-40.0, 0.0);
    // Gentle master-bus speech leveling (speechnorm, a slow raise/fall leveler -- see Project::master_speech_level's
    // doc for why it runs once over the whole mix rather than per clip) narrows the loudness swing a real speaker's
    // own voice has WITHIN and across clips; the per-clip average-matching loop right below only ever matched clips'
    // AVERAGES to each other and left whatever variance already lived inside/between them untouched -- on real
    // speech that is most of why a finished edit's loudness range came out wider than the raw source (RV-28: 3.4 LU
    // raw -> 11.1 LU after rough-cut + the old flat per-clip compressor, on real CC0 interview footage). Default on;
    // level_dynamics:false restores the old compressor-only behaviour exactly (master_speech_level stays 0).
    let level_on = a.get("level_dynamics").and_then(|v| v.as_bool()).unwrap_or(true);
    let level_strength = opt_num(a, "level_strength", 2.5).clamp(1.0, 4.0);
    // V7.9Q: the per-clip AVERAGE-matching loop below (separate from level_dynamics' WITHIN-clip speechnorm
    // above) was previously all-or-nothing -- every dialogue clip's average got pulled the FULL distance to
    // the loudest one. That is right when two genuinely different speakers sit far apart in mic level (see
    // finish_audio_levels_speakers_and_hits_the_target) -- match_strength lets a caller dial that down when
    // it is NOT what they want. Measured on real rough-cut footage first, though: clips from the SAME speaker
    // were already within ~2.3 dB of each other at this step, so lowering match_strength barely moved
    // loudness_range (1.2 -> 1.3 LU at 0.6) while it measurably hurt whisper word recall on the now-quieter
    // clips (88.3% -> 56.7%, reproduced twice) -- this loop was not actually the real footage's flattening
    // cause. Default stays 1.0 (old exact behaviour, recall-safe); the real lever for loudness_range turned
    // out to be finish_audio's hardcoded dialogue compressor just below (now `compress_strength`).
    let match_strength = opt_num(a, "match_strength", 1.0).clamp(0.0, 1.0);
    // V7.9Q: finish_audio always ran a fixed acompressor (ratio 3:1 @ -18dB, no knob) on every dialogue clip.
    // compress_strength (0..1) scales that ratio from 1.0 (old fixed 3:1, 1.0 at the knob) down to no
    // compression at all (0.0). Measured on the real CC0 benchmark: even with level_dynamics AND the compressor
    // both fully off, loudness_range only reached 2.3 LU (raw unprocessed source measures 2.4-3.6 LU) -- some
    // headroom is lost to export's own true-peak limiter, and the edit's ~9s of kept dialogue is just a shorter,
    // calmer excerpt than the 20s window the raw number was measured over. Recall is the harder constraint: ANY
    // reduction in match_strength, or in level_strength below its default, reproducibly collapsed whisper word
    // recall from 88% to 57% by dropping the opening sentence entirely (a hard threshold effect, not a gradual
    // one) -- but compress_strength:0.0 alone was the one change that is SAFE, reproducibly measured at 90.0%
    // recall (54/60, better than 7.8's 88.3% baseline) with loudness_range 1.4 (up from 1.2). Default 0.0: a
    // real, verified, recall-safe gain; short of the >=3 LU target, marked [~] rather than faked.
    let compress_strength = opt_num(a, "compress_strength", 0.0).clamp(0.0, 1.0);
    // classify every media clip on an unmuted track: dialogue (speech in it), music (audio-only or named), ambience (the rest)
    let music_ids: Vec<String> = a.get("music_clip_ids").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut list: Vec<(String, bool)> = vec![];
    for t in &ss.project.tracks { if t.muted { continue; } for c in &t.clips { if let Source::Media { asset } = &c.source { let audio_only = ss.project.assets.get(asset).map(|x| x.width == 0 || x.kind.as_deref() == Some("audio")).unwrap_or(false); list.push((c.id.clone(), audio_only)); } } }
    if list.is_empty() { return Err("no media clips with sound on the timeline".into()); }
    let (mut dia, mut mus, mut amb) = (vec![], vec![], vec![]);
    for (id, audio_only) in &list {
        if music_ids.contains(id) || *audio_only { mus.push(id.clone()); continue; }
        // V7.8Q fix (flagged by a whisper-recall check on real interview footage: most of the real
        // dialogue was coming back MUTED as "ambience" on every real render back to real_v1, hidden
        // because finish_audio's own gain trim just cranks up whatever survived to hit the LUFS target
        // regardless of how much got muted first). aggressiveness 2 rejects steady tones (a real
        // precaution against classifying music/hum as dialogue) but on real human speech it rejected
        // the SPEECH too, on this footage: 4 of 6 real rough-cut segments read speech_ratio under the
        // 0.2 bar at aggressiveness 2 (as low as -0.0 on an 0.8s segment that is unmistakably someone
        // talking), while aggressiveness 1 read all but one of those same segments confidently (0.69 to
        // 0.90), and aggressiveness 0 read every single one high (0.82 to 0.96) including the one
        // aggressiveness 1 still missed. Try the stricter level first (keeps most of the original
        // protection against calling a musical tone "dialogue"); only fall back to the lenient level,
        // and only act on it at a materially higher bar, to rescue a real segment the stricter level
        // still misses rather than let it go muted.
        let mut probe = |agg: i64| -> f64 {
            call(ss, "audio_speech_intervals", &json!({"target": {"clip_id": id}, "aggressiveness": agg, "min_silence_ms": 300, "pad_ms": 0, "max_items": 50}))
                .ok().and_then(|v| v["speech_ratio"].as_f64()).unwrap_or(0.0)
        };
        let ratio = probe(1);
        let is_dialogue = ratio >= 0.2 || probe(0) >= 0.5;
        if is_dialogue { dia.push(id.clone()) } else { amb.push(id.clone()) }
    }
    let mut report_d = vec![];
    for id in &dia {
        call(ss, "set_audio", &json!({"clip_id": id, "highpass": 80, "compress": true, "compress_amount": compress_strength}))?;
    }
    // RV-28: speech leveling runs ONCE over the whole mixed timeline (master_speech_level, read by audio::mix),
    // not per clip -- a per-clip version was tried first and widened loudness range instead of narrowing it
    // (restarting speechnorm's state at every short rough-cut boundary is itself a new transient). See the
    // field's doc on Project for the measured before/after.
    ss.project.master_speech_level = if level_on { level_strength } else { 0.0 };
    // level the dialogue clips to the loudest one (so a quiet speaker comes up), then trim the whole mix to the target below
    let meas: Vec<(String, Option<f64>)> = dia.iter().map(|id| (id.clone(), clip_lufs(ss, id))).collect();
    let refl = meas.iter().filter_map(|(_, l)| *l).fold(f64::NEG_INFINITY, f64::max);
    for (id, l) in &meas {
        match l {
            Some(l) => { let g_db = (refl - l) * match_strength; let g = 10f64.powf(g_db / 20.0).clamp(0.05, 8.0); ss.project.find_clip_mut(id).unwrap().audio.volume = crate::keyframe::Prop::new(g); report_d.push(json!({"clip_id": id, "measured_lufs": r3(*l), "gain_db": r3(20.0 * g.log10())})); }
            None => report_d.push(json!({"clip_id": id, "measured_lufs": null, "note": "too short to measure; left as is"})),
        }
    }
    let mut report_m = vec![];
    for id in &mus {
        let goal = if dia.is_empty() { target } else { target + bed };
        if let Some(l) = clip_lufs(ss, id) { let g = 10f64.powf((goal - l) / 20.0).clamp(0.0, 4.0); ss.project.find_clip_mut(id).unwrap().audio.volume = crate::keyframe::Prop::new(g); report_m.push(json!({"clip_id": id, "measured_lufs": r3(l), "gain_db": r3(20.0 * g.log10())})); }
    }
    if !keep_amb { for id in &amb { ss.project.find_clip_mut(id).unwrap().audio.mute = true; } }
    // the mix: trim every audible clip by the same gain until the timeline reads the target
    let mut mix = None;
    for _ in 0..3 {
        let m = call(ss, "measure_loudness", &json!({}))?; let Some(l) = m["integrated_lufs"].as_f64() else { break };
        mix = Some(l); let d = target - l; if d.abs() <= 0.3 { break; }
        let g = 10f64.powf(d / 20.0);
        for id in dia.iter().chain(mus.iter()) { let c = ss.project.find_clip_mut(id).unwrap(); let v = (c.audio.volume.base * g).clamp(0.0, 16.0); c.audio.volume = crate::keyframe::Prop::new(v); }
    }
    let m = call(ss, "measure_loudness", &json!({"true_peak": true}))?;
    let l = m["integrated_lufs"].as_f64().or(mix);
    Ok(json!({"mix_lufs": l.map(r3), "target_lufs": target, "sample_peak_dbfs": m["sample_peak_dbfs"].as_f64().map(r3), "true_peak_dbfs": m["true_peak_dbfs"].as_f64().map(r3),
        "dialogue": report_d, "music": report_m, "ambience_muted": if keep_amb { json!([]) } else { json!(amb) },
        "note": if level_on { "dialogue: 80 Hz high-pass + gentle speech leveling + compressor, each clip to the same loudness; export limits peaks at -1.5 dBFS" } else { "dialogue: 80 Hz high-pass + compressor, each clip to the same loudness; export limits peaks at -1.5 dBFS" }}))
}

// ---------------------------------------------------------------- auto_grade
/// The same cover/contain/stretch choice render.rs's decode() makes for a clip's `fit`, built as an ffmpeg -vf fragment
/// so a sampler can see what fit actually puts on screen instead of the untouched source frame (KC1, 2026-10-24: a
/// pillarboxed/letterboxed source -- black bars the fit crop never shows -- made the unfit reading read as false-dark).
fn fit_filter(fit: f64, w: u32, h: u32) -> String {
    if fit >= 1.5 { format!("scale={w}:{h}") }
    else if fit >= 0.5 { format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}") }
    else { format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0") }
}
/// Mean RGB (0..1) of what the clip's own `fit` transform actually shows at time `t` -- the project's fit/crop/pad is
/// applied FIRST (matching render.rs's decode()), then the result is downsampled to 64x64 for a cheap, stable read.
/// Measuring the raw, unfit source frame instead (the bug this replaced) reads a pillarboxed/letterboxed or off-center
/// source as false-dark or false-bright and grades the real, already-correct on-screen picture by that wrong number.
fn mean_rgb(path: &str, t: f64, fit: f64, w: u32, h: u32) -> Option<[f64; 3]> {
    let vf = format!("{},scale=64:64", fit_filter(fit, w.max(1), h.max(1)));
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-v", "error", "-ss", &format!("{:.3}", t.max(0.0)), "-i", path, "-frames:v", "1", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
    let b = capture(&mut cmd, 64 * 64 * 3 + 16, std::time::Duration::from_secs(30)).ok()?;
    if b.len() < 64 * 64 * 3 { return None; }
    let mut s = [0f64; 3]; for p in b[..64 * 64 * 3].chunks(3) { for k in 0..3 { s[k] += p[k] as f64 / 255.0; } }
    Some([s[0] / 4096.0, s[1] / 4096.0, s[2] / 4096.0])
}
fn luma(c: [f64; 3]) -> f64 { 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] }
fn auto_grade(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let strength = opt_num(a, "strength", 0.85).clamp(0.0, 1.0);
    let goal = opt_num(a, "target_luma", 0.42).clamp(0.2, 0.7);
    let ids: Vec<String> = match a.get("clip_ids") { Some(_) => str_list(a, "clip_ids")?, None => ss.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| matches!(c.source, Source::Media { .. })).map(|c| c.id.clone()).collect() };
    let mut out = vec![]; let mut skipped = vec![];
    for id in &ids {
        let Some(c) = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| &c.id == id).cloned() else { return Err(format!("unknown clip '{id}'")); };
        let Source::Media { asset: aid } = &c.source else { skipped.push(json!({"clip_id": id, "why": "not a media clip"})); continue };
        let Some(asset) = ss.project.assets.get(aid).cloned() else { continue };
        if asset.width == 0 { continue; }
        let fit = c.props.get("fit").map(|q| q.at(0.0)).unwrap_or(0.0);
        let Some(m) = mean_rgb(&crate::deps::resolve_str(&asset.path), c.in_point + c.duration * c.speed / 2.0, fit, ss.project.width, ss.project.height) else { skipped.push(json!({"clip_id": id, "why": "could not read a frame"})); continue };
        let l = luma(m).max(1e-3);
        let cast = (m[0] - m[1]).abs().max((m[2] - m[1]).abs());
        // a shot inside the normal range with no visible cast is left as shot
        if (0.32..=0.6).contains(&l) && cast < 0.05 {
            // the source is fine: if the picture on the timeline is still dark, an earlier grade crushed it, so that grade goes
            let now = call(ss, "get_scopes", &json!({"time": r3(c.start + c.duration / 2.0)})).ok().and_then(|v| v["luma_mean"].as_f64()).unwrap_or(l);
            if now < 0.6 * l && now < 0.2 {
                // reset every primaries grade on the clip in place (a new neutral grade stacked on the crushing one fixes nothing)
                let prim: Vec<String> = c.effects.iter().filter(|e| e.kind == "primaries").map(|e| e.id.clone()).collect();
                if prim.is_empty() { call(ss, "set_color_wheels", &json!({"clip_id": id, "gain": [1.0, 1.0, 1.0, 1.0]}))?; }
                for fx in &prim { call(ss, "set_color_wheels", &json!({"clip_id": id, "effect_id": fx, "gain": [1.0, 1.0, 1.0, 1.0]}))?; }
                out.push(json!({"clip_id": id, "luma_before": r3(now), "source_luma": r3(l), "gain": [1.0, 1.0, 1.0], "note": "the source is well exposed; the grade that darkened it was reset"})); continue; }
            skipped.push(json!({"clip_id": id, "why": "already well exposed and neutral", "luma": r3(l), "cast": r3(cast)})); continue; }
        // grey-world balance and an exposure gain, both clamped, so a grade can never push a shot to black or white
        let ch: Vec<f64> = (0..3).map(|k| (l / m[k].max(1e-3)).powf(strength).clamp(0.72, 1.4)).collect();
        let bal = [m[0] * ch[0], m[1] * ch[1], m[2] * ch[2]];
        let master = (goal / luma(bal).max(1e-3)).powf(strength).clamp(0.55, 2.4);
        let pred = (luma(bal) * master).min(1.0);
        // update the clip's primaries grade when it has one (running auto_grade twice must not stack two grades)
        let mut wargs = json!({"clip_id": id, "gain": [r3(ch[0] * master), r3(ch[1] * master), r3(ch[2] * master), 1.0]});
        if let Some(fx) = c.effects.iter().find(|e| e.kind == "primaries") { wargs["effect_id"] = json!(fx.id); }
        call(ss, "set_color_wheels", &wargs)?;
        out.push(json!({"clip_id": id, "luma_before": r3(l), "cast_before": r3(cast), "gain": [r3(ch[0] * master), r3(ch[1] * master), r3(ch[2] * master)], "luma_expected": r3(pred)}));
    }
    Ok(json!({"graded": out, "skipped": skipped, "note": "per clip: grey-world white balance + exposure toward target_luma, gains clamped; undo reverts all of it"}))
}

// ---------------------------------------------------------------- cut_to_beat
fn cut_to_beat(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let music = str_arg(a, "music_asset_id")?.to_string(); let (mpath, mdur) = asset(ss, &music)?;
    let ids = str_list(a, "asset_ids")?; for id in &ids { asset(ss, id)?; }
    let fps = fps(ss);
    let n = opt_num(a, "beats_per_shot", 4.0).clamp(1.0, 32.0) as usize;
    let start = snap(opt_num(a, "start", 0.0).max(0.0), fps);
    let b = call(ss, "detect_beats", &json!({"path": mpath}))?;
    let beats: Vec<f64> = b["beats"].as_array().map(|v| v.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
    if beats.len() < 4 { return Err("detect_beats found fewer than 4 beats in the music; cut by hand with add_clip".into()); }
    // a steady grid through the detected beats (median period, median phase), extended back to the first beat at or after 0
    let mut d: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect(); d.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let period = d[d.len() / 2];
    let mut ph: Vec<f64> = beats.iter().map(|t| t.rem_euclid(period)).collect(); ph.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let phase = ph[ph.len() / 2]; let phase = if phase > period * 0.9 { phase - period } else { phase };
    let grid: Vec<f64> = (0..).map(|k| phase.max(0.0) + k as f64 * period).take_while(|t| *t <= mdur).collect();
    let want = a.get("duration").and_then(|v| v.as_f64());
    let shots = match want { Some(w) => ((w / (period * n as f64)).round() as usize).max(1), None => ids.len() };
    if grid.len() < shots * n + 1 { return Err(format!("the music holds {} beats; {shots} shots of {n} beats need {}", grid.len(), shots * n + 1)); }
    let m0 = grid[0];
    let vt = new_track(ss)?; let mut clips = vec![]; let mut cuts = vec![];
    for k in 0..shots {
        let (bs, be) = (grid[k * n] - m0, grid[(k + 1) * n] - m0);
        let (s, e) = (snap(start + bs, fps), snap(start + be, fps)); let dd = e - s;
        let id = &ids[k % ids.len()]; let (_, adur) = asset(ss, id)?;
        let ip = a.get("in_points").and_then(|m| m.get(id.as_str())).and_then(|v| v.as_f64()).unwrap_or(((adur - dd) / 2.0).max(0.0));
        let cid = call(ss, "add_clip", &json!({"track_id": vt, "source_type": "media", "asset_id": id, "start": r3(s), "duration": r3(dd.min(adur)), "in_point": snap(ip.min((adur - dd).max(0.0)), fps), "fit": "cover"}))?["clip_id"].as_str().unwrap_or("").to_string();
        call(ss, "set_audio", &json!({"clip_id": cid, "mute": true}))?;
        clips.push(json!({"clip_id": cid, "asset_id": id, "start": r3(s), "end": r3(s + dd.min(adur))})); cuts.push(r3(s));
    }
    let total = snap(grid[shots * n] - m0, fps);
    let mc = call(ss, "add_audio_clip", &json!({"asset_id": music, "start": start, "duration": total, "in_point": r3(m0)}))?;
    Ok(json!({"video_track_id": vt, "music_clip_id": mc["clip_id"], "music_track_id": mc["track_id"], "clips": clips, "cuts_at": cuts, "beat_period": r3(period), "end": r3(start + total),
        "note": "every cut is on a beat of the music (shot clip sound muted); add text on a new track, then finish_audio"}))
}


// FEATURES.md "auto reframe (cover-fit decode), scene edit detection, auto-cut to beats / to speech": auto_reframe,
// detect_cuts and scene_split already shipped pre-v7; cut_to_beat above is the existing "to beats" recipe. This is
// its "to speech" sibling -- same shape, same guarantees, but the fenceposts are where a narration/voiceover track's
// own VAD-detected speech intervals start (via the `speech()` helper above), not a detected music beat grid.
fn cut_to_speech(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let dia = str_arg(a, "dialogue_asset_id")?.to_string(); let (dpath, _ddur) = asset(ss, &dia)?;
    let ids = str_list(a, "asset_ids")?; for id in &ids { asset(ss, id)?; }
    let fps = fps(ss);
    let n = opt_num(a, "phrases_per_shot", 1.0).clamp(1.0, 32.0) as usize;
    let start = snap(opt_num(a, "start", 0.0).max(0.0), fps);
    let aggr = opt_num(a, "aggressiveness", 1.0).clamp(0.0, 3.0) as u32;
    let min_sil_ms = opt_num(a, "min_silence_ms", 300.0);
    let pad_ms = opt_num(a, "pad_ms", 120.0);
    let fenceposts: Vec<f64> = speech(ss, json!({"path": dpath}), aggr, min_sil_ms, pad_ms)?.into_iter().map(|(s, _e)| s).collect();
    if fenceposts.len() < n + 1 { return Err("audio_speech_intervals found fewer than phrases_per_shot+1 speech intervals in the dialogue; cut by hand with add_clip".into()); }
    // a steady grid doesn't apply here (speech isn't periodic); each interval's start is its own fencepost, grouped
    // n at a time exactly the way cut_to_beat groups n beats -- shot k spans fencepost[k*n] to fencepost[(k+1)*n],
    // so shots*n+1 fenceposts are needed for `shots` shots, same arithmetic as the beat grid above.
    let shots = ids.len().min((fenceposts.len() - 1) / n).max(1);
    if fenceposts.len() < shots * n + 1 { return Err(format!("the dialogue holds {} speech intervals; {shots} shots of {n} phrases need {}", fenceposts.len(), shots * n + 1)); }
    let t0 = fenceposts[0];
    let vt = new_track(ss)?; let mut clips = vec![]; let mut cuts = vec![];
    for k in 0..shots {
        let (ps, pe) = (fenceposts[k * n] - t0, fenceposts[(k + 1) * n] - t0);
        let (s, e) = (snap(start + ps, fps), snap(start + pe, fps)); let dd = e - s;
        if dd <= 0.0 { return Err(format!("shot {k} came out zero or negative duration; try a smaller phrases_per_shot")); }
        let id = &ids[k % ids.len()]; let (_, adur) = asset(ss, id)?;
        let ip = a.get("in_points").and_then(|m| m.get(id.as_str())).and_then(|v| v.as_f64()).unwrap_or(((adur - dd) / 2.0).max(0.0));
        let cid = call(ss, "add_clip", &json!({"track_id": vt, "source_type": "media", "asset_id": id, "start": r3(s), "duration": r3(dd.min(adur)), "in_point": snap(ip.min((adur - dd).max(0.0)), fps), "fit": "cover"}))?["clip_id"].as_str().unwrap_or("").to_string();
        call(ss, "set_audio", &json!({"clip_id": cid, "mute": true}))?;
        clips.push(json!({"clip_id": cid, "asset_id": id, "start": r3(s), "end": r3(s + dd.min(adur))})); cuts.push(r3(s));
    }
    let total = snap(fenceposts[shots * n] - t0, fps);
    let dc = call(ss, "add_audio_clip", &json!({"asset_id": dia, "start": start, "duration": total, "in_point": r3(t0)}))?;
    Ok(json!({"video_track_id": vt, "dialogue_clip_id": dc["clip_id"], "dialogue_track_id": dc["track_id"], "clips": clips, "cuts_at": cuts, "phrases_used": shots * n, "end": r3(start + total),
        "note": "every cut lands where a new speech interval starts (shot clip sound muted); add captions with caption_from_words, then finish_audio"}))
}

// ---------------------------------------------------------------- timeline_summary
fn timeline_summary(ss: &mut Session, _a: &Value) -> Result<Value, String> {
    let p = &ss.project; let mut end: f64 = 0.0;
    let tracks: Vec<Value> = p.tracks.iter().map(|t| {
        let clips: Vec<Value> = t.clips.iter().map(|c| {
            end = end.max(c.start + c.duration);
            let src = match &c.source { Source::Media { asset } => p.assets.get(asset).map(|x| std::path::Path::new(&x.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_else(|| asset.clone()),
                Source::Text { text, .. } => format!("text: {}", text.replace('\n', " ").chars().take(40).collect::<String>()), _ => "generated".to_string() };
            let mut o = json!({"id": c.id, "src": src, "start": r3(c.start), "end": r3(c.start + c.duration)});
            if matches!(c.source, Source::Media { .. }) { o["in"] = json!(r3(c.in_point)); if c.audio.mute { o["muted"] = json!(true); } else if (c.audio.volume.base - 1.0).abs() > 1e-6 { o["vol"] = json!(r3(c.audio.volume.base)); } }
            if !c.effects.is_empty() { o["fx"] = json!(c.effects.iter().map(|e| e.kind.clone()).collect::<Vec<_>>()); }
            if c.props.get("x").map(|p| !p.keys.is_empty()).unwrap_or(false) { o["keyed"] = json!(true); } // xform-lint: ok (asks only whether x has keys; no transform value is read)
            o }).collect();
        json!({"track_id": t.id, "clips": clips}) }).collect();
    Ok(json!({"width": p.width, "height": p.height, "fps": p.fps, "duration": r3(end), "tracks_bottom_to_top": tracks}))
}

// ---------------------------------------------------------------- lint_edit
fn lint_edit(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let platform = a.get("platform").and_then(|v| v.as_str()).unwrap_or("tiktok").to_string();
    let target = opt_num(a, "target_lufs", -14.0);
    let mut probs: Vec<Value> = vec![];
    let end = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.start + c.duration).fold(0.0, f64::max);
    if end <= 0.0 { return Err("the timeline is empty".into()); }
    let (w, h) = (ss.project.width as f64, ss.project.height as f64);
    // which clip is on top at t (media and text are both pictures; text is reported separately)
    let top_media = |ss: &Session, t: f64| -> Option<String> {
        let mut best = None;
        for tr in ss.project.tracks.iter().filter(|tr| !tr.muted) { for c in &tr.clips {
            let Source::Media { asset } = &c.source else { continue };
            if c.start <= t && t < c.start + c.duration && ss.project.assets.get(asset).map(|x| x.width > 0).unwrap_or(false) { best = Some(c.id.clone()); }
        } }
        best
    };
    // 1 picture: dark, blown or empty frames, sampled every 0.5 s
    let mut t = 0.25; let mut bad_at: Vec<(f64, String, Option<String>)> = vec![];
    while t < end { let top = top_media(ss, t);
        // Perf history (KP1, KQ1): this loop used to pre-warm the decode cache itself (a big,
        // export-sized window, optionally through a short-GOP analysis proxy) immediately before
        // every get_scopes call, because get_scopes' OWN internal preparation once used a much
        // smaller window and paid a real, repeated decode-and-discard cost walking deeper into a
        // clip on every 0.5s sample. KQ1 (fb6ddbf) changed get_scopes' own call site to use the
        // exact same function this loop was calling to pre-warm it (render::prepare_analysis_range,
        // same 256-frame window, same proxy eligibility) - so by KR2 this prewarm call had become a
        // byte-for-byte duplicate of the preparation get_scopes was about to do for itself one line
        // later: same project, same t, same function, same cache. Removing it does not change what
        // gets decoded, selected (proxy or original) or rendered - get_scopes' own call now does the
        // only preparation that ever happens per sample - it only removes the duplicate work (active-
        // clip scan, decode-cache signature building, analysis_proxy::ensure's stat+sidecar read)
        // that duplicate call paid for nothing. Errors are unaffected: get_scopes' own prepare_at
        // call is, and always was, what actually reports a real problem (e.g. a genuinely missing or
        // corrupt file) to the caller. Guarded by analysis_prepare_calls_count in render_state_leak-
        // style tests: render::ANALYSIS_PREPARE_CALLS must advance by exactly 1 per sample now, not 2.
        match &top { None => bad_at.push((t, "empty_frame".into(), None)), Some(cid) => { if let Ok(s) = call(ss, "get_scopes", &json!({"time": r3(t)})) { let l = s["luma_mean"].as_f64().unwrap_or(0.5); if l < 0.08 { bad_at.push((t, "dark_frame".into(), Some(cid.clone()))); } else if l > 0.92 { bad_at.push((t, "blown_frame".into(), Some(cid.clone()))); } } } }
        t += 0.5; }
    let mut seen = std::collections::HashSet::new();
    for (t, code, cid) in &bad_at { let key = format!("{code}{cid:?}"); if !seen.insert(key) { continue; }
        let fix = match (code.as_str(), cid) { ("empty_frame", _) => json!({"tool": "timeline_summary", "args": {}, "then": "fill the gap or end the timeline earlier"}), (_, Some(c)) => json!({"tool": "auto_grade", "args": {"clip_ids": [c]}, "or": "undo the grade that crushed it"}), _ => json!(null) };
        probs.push(json!({"code": code, "at": r3(*t), "clip_id": cid, "message": format!("{code} at {t:.2} s"), "fix": fix})); }
    // 1b a transition into a clip with no other picture under it fades in from black (a dissolve on a cut where the clips only touch)
    for tr in ss.project.tracks.iter().filter(|tr| !tr.muted) { for c in &tr.clips {
        let Some(tx) = &c.transition_in else { continue }; if tx.duration <= 0.0 || !matches!(c.source, Source::Media { .. }) { continue; }
        let mid = c.start + tx.duration * 0.5;
        let under = ss.project.tracks.iter().filter(|t2| !t2.muted).flat_map(|t2| t2.clips.iter()).any(|o| o.id != c.id && matches!(o.source, Source::Media { .. }) && o.start <= mid && mid < o.start + o.duration);
        if under { continue; }
        probs.push(json!({"code": "transition_from_black", "clip_id": c.id, "at": r3(c.start), "message": format!("the {} into {} at {:.2} s has no picture under it, so the export dips to black there", tx.kind, c.id, c.start),
            "fix": {"tool": "remove_transition", "args": {"clip_id": c.id}, "or": "overlap the previous clip by the transition's duration"}}));
    } }
    // 1c text a phone cannot read, and characters the font cannot draw
    let min_px = (h.min(w) / 18.0).round();
    for tr in ss.project.tracks.iter().filter(|tr| !tr.muted) { for c in &tr.clips {
        let Source::Text { text, font, size, .. } = &c.source else { continue };
        let scale = crate::render::resolve_xform(&ss.project, c, c.duration / 2.0).scale; let eff = size * scale;  // the drawn scale mid-clip, parenting included
        if eff < min_px && ss.project.editorial.captions.iter().all(|(_, g)| !g.clips.iter().any(|x| x == &c.id)) {
            probs.push(json!({"code": "text_too_small", "clip_id": c.id, "size": r3(eff), "message": format!("text '{}' is {:.0} px, under {:.0} px it is hard to read on a phone", text.chars().take(24).collect::<String>(), eff, min_px),
                "fix": {"tool": "set_property", "args": {"clip_id": c.id, "property": "scale", "value": r3(min_px / size.max(1.0))}}})); }
        let miss = crate::text::missing_glyphs(text, font, c.text_fx.as_ref().map(|x| x.weight).unwrap_or(0.0));
        if !miss.is_empty() { let m: String = miss.iter().collect();
            probs.push(json!({"code": "missing_glyph", "clip_id": c.id, "chars": m, "message": format!("the font has no glyph for '{m}' (emoji or symbol); it is left out of the picture"), "fix": {"tool": "set_text", "args": {"clip_id": c.id, "text": "<the text without those characters>"}}})); }
    } }
    // 1d two texts on screen at once in the same place (a second title over the first reads as garbage)
    let tclips: Vec<(String, f64, f64)> = ss.project.tracks.iter().filter(|tr| !tr.muted).flat_map(|t| t.clips.iter()).filter(|c| matches!(c.source, Source::Text { .. })).map(|c| (c.id.clone(), c.start, c.start + c.duration)).collect();
    let groups: Vec<Option<String>> = tclips.iter().map(|(id, _, _)| ss.project.editorial.captions.iter().find(|(_, g)| g.clips.iter().any(|c| c == id)).map(|(k, _)| k.clone())).collect();
    let mut overl = 0;
    for i in 0..tclips.len() { for k in i + 1..tclips.len() {
        if groups[i].is_some() && groups[i] == groups[k] { continue; }
        let (a, b) = (tclips[i].clone(), tclips[k].clone()); let (s0, e0) = (a.1.max(b.1), a.2.min(b.2)); if e0 - s0 < 0.2 { continue; }
        let m = (s0 + e0) / 2.0;
        let (Some((ra, _)), Some((rb, _))) = (text_rect(ss, &a.0, m - a.1), text_rect(ss, &b.0, m - b.1)) else { continue };
        let ix = (ra[0] + ra[2]).min(rb[0] + rb[2]) - ra[0].max(rb[0]); let iy = (ra[1] + ra[3]).min(rb[1] + rb[3]) - ra[1].max(rb[1]);
        if ix <= 0.0 { continue; }
        if iy > 0.0 {
            let small = (ra[2] * ra[3]).min(rb[2] * rb[3]).max(1.0); if ix * iy / small < 0.25 { continue; }
            overl += 1; if overl > 6 { continue; }
            probs.push(json!({"code": "text_overlap", "clip_id": b.0, "other_clip_id": a.0, "at": r3(m), "message": format!("{} and {} are on screen together at {:.2} s in the same place", a.0, b.0, m),
                "fix": {"tool": "delete_clip", "args": {"clip_id": b.0}, "or": {"tool": "set_property", "args": {"clip_id": b.0, "property": "y", "value": "<pixels from centre, clear of the other text>"}}}}));
            continue;
        }
        // No pixel overlap, but with less than min_gap of clear space between them while sharing the same
        // horizontal band, two text boxes read as one cluttered stack, not two separate lines (the v7.6Q
        // benchmark finding: a caption pill moved to clear a subject landed a hair above the hook title's
        // own pill -- zero overlap, same visual mess, and the overlap-only check above missed it). min_gap
        // scales with the smaller box so two small labels aren't held to a big title's own spacing.
        let gap = -iy;
        let min_gap = (ra[3].min(rb[3]) * 0.15).max(12.0);
        if gap >= min_gap { continue; }
        overl += 1; if overl > 6 { continue; }
        let cur_y = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| c.id == b.0).and_then(|c| c.props.get("y").map(|p| p.base)).unwrap_or(0.0);
        let push_down = (ra[1] + ra[3] / 2.0) < (rb[1] + rb[3] / 2.0); // a is already above b -> add the clearance below b, else above it
        let dy = (min_gap - gap) + 6.0; let new_y = r3(cur_y + if push_down { dy } else { -dy });
        probs.push(json!({"code": "text_crowded", "severity": "warning", "clip_id": b.0, "other_clip_id": a.0, "at": r3(m), "gap_px": r3(gap.max(0.0)),
            "message": format!("{} sits only {:.0}px from {} with no breathing room -- reads as one cluttered stack, not two separate lines", b.0, gap.max(0.0), a.0),
            "fix": {"tool": "set_property", "args": {"clip_id": b.0, "property": "y", "value": new_y}}}));
    } }
    // 1e media clip aspect: a clip's source aspect doesn't match the canvas, so add_clip's default framing crops or
    // pads it without telling anyone. Non-mutating, additive: names auto_reframe as the fix.
    const ASPECT_TOL: f64 = 0.05;
    let canvas_aspect = w / h;
    for tr in ss.project.tracks.iter().filter(|tr| !tr.muted) { for c in &tr.clips {
        let Source::Media { asset } = &c.source else { continue };
        let Some(info) = ss.project.assets.get(asset) else { continue };
        if info.width == 0 || info.height == 0 { continue; }
        let src_aspect = info.width as f64 / info.height as f64;
        if ((src_aspect - canvas_aspect).abs() / canvas_aspect) <= ASPECT_TOL { continue; }
        probs.push(json!({"code": "aspect_mismatch", "severity": "warning", "clip_id": c.id, "asset_id": asset,
            "source_size": [info.width, info.height], "canvas_size": [w as i64, h as i64],
            "message": format!("clip {} source is {}x{} ({:.2}:1) but the canvas is {:.2}:1 ({}x{}); the default framing crops or pads it", c.id, info.width, info.height, src_aspect, canvas_aspect, w as i64, h as i64),
            "fix": {"tool": "auto_reframe", "args": {"clip_id": c.id}}}));
    } }
    // 2 text: off the platform's safe area, or a substituted font
    let (avoid, safe) = zones(ss, &platform)?;
    let texts: Vec<(String, f64, f64)> = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| matches!(c.source, Source::Text { .. })).map(|c| (c.id.clone(), c.start, c.duration)).collect();
    let caption_of = |ss: &Session, id: &str| -> Option<String> { ss.project.editorial.captions.iter().find(|(_, g)| g.clips.iter().any(|c| c == id)).map(|(k, _)| k.clone()) };
    let mut flagged_caps = std::collections::HashSet::new(); let mut font_flag = false;
    for (id, _s, d) in &texts {
        let Some((r, subst)) = text_rect(ss, id, d / 2.0) else { continue };
        if subst && !font_flag { font_flag = true; probs.push(json!({"code": "font_substituted", "clip_id": id, "message": "a text clip draws with a fallback font (preset font not installed)", "fix": {"tool": "set_text_style", "args": {"clip_id": id, "font": "DejaVu Sans"}, "or": "run bash fonts/fetch.sh or set KIMO_FONTS"}})); }
        let off = r[0] < safe[0] - 4.0 || r[1] < safe[1] - 4.0 || r[0] + r[2] > safe[0] + safe[2] + 4.0 || r[1] + r[3] > safe[1] + safe[3] + 4.0 || avoid.iter().any(|z| rect_hits(r, *z));
        if !off { continue; }
        let cap = caption_of(ss, id);
        if let Some(c) = &cap { if !flagged_caps.insert(c.clone()) { continue; } }
        let bottom_lim = avoid.iter().filter(|z| z[1] > h * 0.5 && z[0] < w / 2.0 && z[0] + z[2] > w / 2.0 - 1.0).map(|z| z[1]).fold(safe[1] + safe[3], f64::min) - 16.0;
        let cur_y = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| &c.id == id).and_then(|c| c.props.get("y").map(|p| p.base)).unwrap_or(0.0); // xform-lint: ok (the stored base, to set a new base with set_property)
        let dy = if r[1] + r[3] > bottom_lim { bottom_lim - (r[1] + r[3]) } else if r[1] < safe[1].max(140.0) { safe[1].max(140.0) - r[1] } else { 0.0 };
        let mut half = (w / 2.0 - safe[0]).min(safe[0] + safe[2] - w / 2.0);
        for z in &avoid { if z[1] < r[1] + r[3] + dy && z[1] + z[3] > r[1] + dy && z[0] > w / 2.0 { half = half.min(z[0] - w / 2.0 - 8.0); } }
        let too_wide = r[2] > 2.0 * half;
        let fix = match &cap { Some(c) => { let mut st = json!({"caption_id": c, "style": "boxed", "y": r3(cur_y + dy)}); if too_wide { st["size"] = json!(((h.min(w) / 15.0) * 2.0 * half / r[2]).floor()); }
                json!({"tool": "caption_from_words", "args": {"track_id": "<dialogue track>", "text": "<the spoken words>"}, "or": {"tool": "set_caption_style", "args": st}}) },
            None => if too_wide { json!({"tool": "set_text_style", "args": {"clip_id": id, "box_width": r3(2.0 * half)}, "then": {"tool": "set_property", "args": {"clip_id": id, "property": "y", "value": r3(cur_y + dy)}}}) } else { json!({"tool": "set_property", "args": {"clip_id": id, "property": "y", "value": r3(cur_y + dy)}}) } };
        probs.push(json!({"code": "text_outside_safe_area", "clip_id": id, "caption_id": cap, "rect": [r[0].round(), r[1].round(), r[2].round(), r[3].round()], "message": format!("text box {:?} is under the {platform} UI or outside title safe{}", [r[0].round(), r[1].round(), r[2].round(), r[3].round()], if too_wide { " and too wide" } else { "" }), "fix": fix}));
    }
    // 2b text over the subject: the drawn rect of a title/caption overlapping the saliency peak of the
    // MEDIA under it (not the text itself - saliency is computed on a text-free clone of the timeline, since
    // a bright caption would otherwise light up its own spot and self-flag). Reuses add_smart_punch's own
    // saliency machinery (ext/saliency_map.rs: spectral residual, Hou & Zhang 2007) at a small, cheap proxy
    // size (work_px 32, a fraction of the default 64), sampled only at a few instants inside each text
    // clip's own span - additive, non-mutating, same pattern as aspect_mismatch above.
    //
    // V7.7Q (RV-34): PER-PAGE, not per-group. A caption group's pages are on screen while the speaker moves,
    // so the one spot that clears their face at 0:01 is not where their face is at 0:04. The old version
    // flagged only the FIRST colliding page of a group and moved the WHOLE group by that one page's dy --
    // fixing page 1 and relocating every other page (including ones that were already fine, or needed a
    // different nudge) by the same amount. Now every page is evaluated and fixed independently, each against
    // its OWN sampled subject position, with its own set_property (the same per-clip "y" prop style() already
    // writes at creation -- a per-page override is just that prop set again for one clip, not a new mechanism).
    {
        let mut media_proj = ss.project.clone();
        for tr in media_proj.tracks.iter_mut() { tr.clips.retain(|c| !matches!(c.source, Source::Text { .. })); }
        for (id, s0, d) in &texts {
            if *d <= 0.0 { continue; }
            let mut hit: Option<(f64, [f64; 4], f64)> = None;
            for frac in [0.25, 0.5, 0.75] {
                let off = (d * frac).clamp(0.0, (d - 0.01).max(0.0));
                let Some((r, _)) = text_rect(ss, id, off) else { continue };
                let t_abs = r3(s0 + off);
                if crate::render::prepare_analysis_range(&media_proj, &mut ss.cache, t_abs, t_abs).is_err() { continue; }
                let Ok(frame) = crate::render::render_frame(&media_proj, t_abs, &ss.cache) else { continue };
                let l = crate::ext::saliency_map::luma_of(&frame);
                let long = w.max(h).max(1.0); let work_px = 32.0;
                let (ww, wh) = (((w * work_px) / long).round().max(1.0) as usize, ((h * work_px) / long).round().max(1.0) as usize);
                let g = crate::ext::saliency_map::area_down(&l, w as usize, h as usize, ww, wh);
                let spec = crate::ext::saliency_map::spectral_residual(&g, ww, wh, 2.5);
                let st = crate::ext::saliency_map::stats(&spec, ww, wh, w as usize, h as usize, &[r], 3);
                if st["empty"] == json!(true) { continue; }
                let mx = st["rects"][0]["max"].as_f64().unwrap_or(0.0);
                if mx >= 0.92 { hit = Some((t_abs, r, st["peak"]["y"].as_f64().unwrap_or(h / 2.0))); break; }
            }
            let Some((t_abs, r, peak_y)) = hit else { continue };
            let cap = caption_of(ss, id);
            let cur_y = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| &c.id == id).and_then(|c| c.props.get("y").map(|p| p.base)).unwrap_or(0.0); // xform-lint: ok (the stored base, to set a new base with set_property)
            let top_lim = safe[1].max(140.0); let bottom_lim = safe[1] + safe[3] - r[3] - 16.0;
            // Clearing the subject needs to get the box off the salient peak, not relocate it across the
            // whole frame: a caption sitting in the bottom band that trips this on one sampled instant used
            // to jump all the way to top_lim -- right into a cold-open hook title's own territory, the exact
            // "a fix introduced a new problem" shape apply_review_fixes' re-check (two commits ago) exists to
            // catch, except this one is avoidable at the source. Capped to one box-height of clearance plus a
            // margin, in the same direction the un-capped math already picked; a subject too large for that
            // to clear shows up as a remaining/introduced problem on the re-check instead of silently landing
            // somewhere worse.
            let full_dy = if peak_y >= r[1] + r[3] / 2.0 { top_lim - r[1] } else { bottom_lim - r[1] };
            let nudge_cap = r[3] + 24.0;
            let mut dy = full_dy.clamp(-nudge_cap, nudge_cap);
            // Never cross the vertical midline: a bottom-band caption clearing a subject must not land in
            // the upper half (where a cold-open hook title typically sits) and vice versa -- the nudge stops
            // at the midline even when the capped distance above would have carried it further.
            let box_center = r[1] + r[3] / 2.0; let mid = h / 2.0; let was_bottom = box_center >= mid;
            let new_center = box_center + dy;
            if was_bottom && new_center < mid { dy = mid - box_center; }
            if !was_bottom && new_center > mid { dy = mid - box_center; }
            // Still not enough on its own: a box can stay on its own side of the midline and still land on
            // another text clip that's on screen at the same moment (a hook title straddles the midline by
            // design, reaching down into the bottom half). Real benchmark finding: the capped, midline-safe
            // nudge above still put a caption on top of a cold-open hook title, which apply_review_fixes'
            // re-check caught as an introduced text_overlap -- but the honest fix here is to not create that
            // collision in the first place, not to let a later pass delete the hook title to silence it.
            // Checked against every OTHER text clip on screen during THIS PAGE's own span only now (not the
            // whole group's spans) since the fix only ever moves this one page.
            let collides_other_text = {
                let shifted = [r[0], r[1] + dy, r[2], r[3]];
                texts.iter().any(|(id2, s2, d2)| {
                    let (s2, d2) = (*s2, *d2);
                    if id2 == id || d2 <= 0.0 { return false; }
                    if caption_of(ss, id2) == cap { return false; } // another page of the same group (or, same as before, another plain non-caption text): not a collision with itself
                    let ws = s0.max(s2); let we = (s0 + d).min(s2 + d2); if we <= ws { return false; }
                    let off2 = ((ws + we) / 2.0 - s2).clamp(0.0, (d2 - 0.01).max(0.0));
                    let Some((r2, _)) = text_rect(ss, id2, off2) else { return false };
                    let (ix, iy) = ((shifted[0] + shifted[2]).min(r2[0] + r2[2]) - shifted[0].max(r2[0]), (shifted[1] + shifted[3]).min(r2[1] + r2[3]) - shifted[1].max(r2[1]));
                    ix > 0.0 && iy > 0.0
                })
            };
            if collides_other_text { dy = 0.0; }
            let fix = json!({"tool": "set_property", "args": {"clip_id": id, "property": "y", "value": r3(cur_y + dy)}});
            probs.push(json!({"code": "text_over_subject", "severity": "warning", "clip_id": id, "caption_id": cap, "at": t_abs,
                "rect": [r[0].round(), r[1].round(), r[2].round(), r[3].round()],
                "message": format!("text box {:?} sits over the subject at {t_abs:.2} s", [r[0].round(), r[1].round(), r[2].round(), r[3].round()]),
                "fix": fix}));
        }
    }
    // 3 speech: the dialogue track is the lowest track with speech
    let mut dia: Option<(String, Vec<(f64, f64)>)> = None;
    let tids: Vec<String> = ss.project.tracks.iter().filter(|t| t.clips.iter().any(|c| matches!(c.source, Source::Media { .. }))).map(|t| t.id.clone()).collect();
    if let Some(d) = a.get("dialogue_track").and_then(|v| v.as_str()) { let sp = speech(ss, json!({"track_id": d}), 1, 250.0, 60.0)?; dia = Some((d.to_string(), sp)); }
    else { for tid in &tids { let sp = speech(ss, json!({"track_id": tid}), 2, 250.0, 60.0).unwrap_or_default(); if !sp.is_empty() { dia = Some((tid.clone(), speech(ss, json!({"track_id": tid}), 1, 250.0, 60.0)?)); break; } } }
    if let Some((dtid, sp)) = &dia {
        let di = track_index(ss, dtid)?;
        let dil: Vec<(f64, f64)> = sp.iter().map(|(s, e)| (s - 0.2, e + 0.2)).collect();
        // V7.7Q: a clip that starts only after ALL speech on the dialogue track has ended is epilogue
        // territory (an end screen / outro card), not b-roll placed over a gap mid-interview -- there is no
        // "speech it should have overlapped" once the person is done talking for good, so flagging it here
        // and auto-"fixing" it by yanking it back onto the nearest speech window (auto_edit's own end-screen
        // background got dragged back into the middle of the dialogue and trimmed to a sliver this way,
        // emptying the end screen it was supposed to sit behind) is simply the wrong call for this clip.
        let speech_end = sp.iter().map(|(_, e)| *e).fold(0.0_f64, f64::max);
        for tr in ss.project.tracks.iter().skip(di + 1) { for c in &tr.clips {
            let Source::Media { asset } = &c.source else { continue }; if ss.project.assets.get(asset).map(|x| x.width == 0).unwrap_or(true) { continue; }
            if c.start + 1e-6 >= speech_end { continue; }
            let (s, e) = (c.start, c.start + c.duration); let ov: f64 = dil.iter().map(|(a, b)| (e.min(*b) - s.max(*a)).max(0.0)).sum();
            if ov / (e - s).max(1e-6) < 0.9 {
                let best = sp.iter().filter(|(a, b)| b - a >= 0.6).min_by(|x, y| ((x.0 + x.1) / 2.0 - (s + e) / 2.0).abs().partial_cmp(&((y.0 + y.1) / 2.0 - (s + e) / 2.0).abs()).unwrap()).copied();
                let fix = match best { Some((a0, b0)) => { let ns = a0.max(s.min(b0 - 0.5)); let nd = (b0.min(ns + (e - s))) - ns; json!({"tool": "move_clip", "args": {"clip_id": c.id, "start": r3(ns)}, "then": {"tool": "trim_clip", "args": {"clip_id": c.id, "duration": r3(nd.max(0.4))}}}) } None => json!({"tool": "delete_clip", "args": {"clip_id": c.id}}) };
                probs.push(json!({"code": "broll_over_silence", "clip_id": c.id, "span": [r3(s), r3(e)], "message": format!("b-roll {} plays {:.0}% outside speech", c.id, 100.0 * (1.0 - ov / (e - s).max(1e-6))), "fix": fix}));
            } } }
        // captions missing
        let stot: f64 = sp.iter().map(|(s, e)| e - s).sum();
        let cov: f64 = sp.iter().map(|(s, e)| texts.iter().map(|(_, ts, td)| (e.min(ts + td) - s.max(*ts)).max(0.0)).sum::<f64>().min(e - s)).sum();
        if stot > 0.5 && cov / stot < 0.6 { probs.push(json!({"code": "captions_missing", "message": format!("only {:.0}% of the speech has text on screen", 100.0 * cov / stot), "fix": {"tool": "caption_from_words", "args": {"track_id": dtid, "by_asset": "<{asset_id: the words spoken in it}>"}}})); }
    }
    // 3b caption confidence: caption_from_words tags a cue low_confidence_caption when kimo's own recognizer genuinely could
    // not make out that stretch of audio, whatever text it was given - surfaced here so a small model knows exactly which
    // captions to check against the real audio instead of trusting (or guessing at) an auto-transcript that may be wrong.
    let mut low_conf_ids: Vec<String> = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| c.tags.iter().any(|tg| tg == "low_confidence_caption")).map(|c| c.id.clone()).collect();
    low_conf_ids.truncate(20);
    for cid in low_conf_ids {
        let cap = caption_of(ss, &cid);
        probs.push(json!({"code": "caption_low_confidence", "clip_id": cid, "caption_id": cap,
            "message": "kimo's recognizer could not make out this clip's audio confidently; the caption text may be wrong - check it against the clip",
            "fix": {"tool": "edit_caption", "args": {"clip_id": cid, "text": "<verified against the actual audio>"}}}));
    }
    // 4 loudness of the mix
    if let Ok(m) = call(ss, "measure_loudness", &json!({})) { if let Some(l) = m["integrated_lufs"].as_f64() { if (l - target).abs() > 1.0 { probs.push(json!({"code": "loudness_off", "message": format!("mix reads {l:.1} LUFS, target {target:.0}"), "fix": {"tool": "finish_audio", "args": {"target_lufs": target}}})); } } else { probs.push(json!({"code": "silent_mix", "message": "the mix has no measurable sound", "fix": {"tool": "finish_audio", "args": {}}})); } }
    // 5 length
    if let Some(d) = a.get("duration").and_then(|v| v.as_array()) { if let (Some(lo), Some(hi)) = (d.first().and_then(|v| v.as_f64()), d.get(1).and_then(|v| v.as_f64())) { if end < lo - 0.05 || end > hi + 0.05 { probs.push(json!({"code": "duration", "message": format!("timeline is {end:.2} s, wanted {lo}-{hi} s"), "fix": {"tool": "timeline_summary", "args": {}}})); } } }
    probs.truncate(20);
    Ok(json!({"ok": probs.is_empty(), "problems": probs, "checked": ["frames every 0.5 s", "text vs title safe and the platform UI", "text vs the subject in frame", "fonts", "media clip aspect vs canvas", "b-roll inside speech", "captions cover speech", "caption confidence vs the recognizer", "mix loudness", "duration"], "platform": platform, "duration": r3(end)}))
}

// ---------------------------------------------------------------- review_export
fn remove_transition(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let id = a.get("clip_id").and_then(|v| v.as_str()).ok_or("clip_id is required")?.to_string();
    let c = ss.project.find_clip_mut(&id).ok_or_else(|| format!("no clip {id}; timeline_summary lists the ids"))?;
    let had = c.transition_in.take(); Ok(json!({"clip_id": id, "removed": had.map(|t| t.kind)}))
}
fn review_export(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let path = str_arg(a, "path")?.to_string();
    if !std::path::Path::new(&path).is_file() { return Err(format!("no file at {path}; export first (or give a source clip's path)")); }
    let n = opt_num(a, "n", 8.0).clamp(2.0, 16.0) as usize;
    let mut pr = std::process::Command::new("ffprobe");
    pr.args(["-v", "error", "-show_entries", "format=duration,size:stream=width,height,codec_type,codec_name", "-of", "json", &path]);
    let pj: Value = serde_json::from_slice(&capture(&mut pr, 1 << 20, std::time::Duration::from_secs(30))?).map_err(|e| format!("ffprobe: {e}"))?;
    let dur = pj["format"]["duration"].as_str().and_then(|s| s.parse::<f64>().ok()).ok_or("not a video file (no duration)")?;
    let size = pj["format"]["size"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let v = pj["streams"].as_array().and_then(|s| s.iter().find(|x| x["codec_type"] == "video")).cloned().unwrap_or(json!({}));
    let audio_codec = pj["streams"].as_array().and_then(|s| s.iter().find(|x| x["codec_type"] == "audio")).and_then(|x| x["codec_name"].as_str()).map(String::from);
    // the sheet goes to the temp dir by default: the video may sit in a read-only footage folder
    let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".into());
    let out = a.get("sheet_path").and_then(|v| v.as_str()).map(String::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("kimo_review_{}_{}_{}.png", stem, std::process::id(), ss.media.len())).to_string_lossy().into_owned());
    crate::render::check_still_path(&out, true)?;
    let cols = if n <= 4 { n } else { 4 }; let rows = n.div_ceil(cols);
    let fr = n as f64 / dur.max(0.1);
    let mut ff = std::process::Command::new("ffmpeg");
    ff.args(["-v", "error", "-y", "-i", &path, "-vf", &format!("fps={fr:.5},scale=180:-2,tile={cols}x{rows}:padding=4", ), "-frames:v", "1", &out]);
    capture(&mut ff, 1 << 20, std::time::Duration::from_secs(120))?;
    // loudness of the file itself, through the same EBU R128 meter as measure_loudness
    let mut af = std::process::Command::new("ffmpeg");
    af.args(["-v", "error", "-i", &path, "-vn", "-ac", "2", "-ar", "48000", "-f", "f32le", "-"]);
    let pcm: Vec<f32> = crate::color::process::decode_capture(&mut af, 256 << 20, std::time::Duration::from_secs(120)).map(|b| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()).unwrap_or_default();
    let lufs = crate::editorial::loudness(&pcm);
    let peak = pcm.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
    let (sw, sh) = (cols as u32 * 184, rows as u32 * 184 * 16 / 9);
    ss.media.push(crate::commands::Media { path: out.clone(), w: sw, h: sh, mime: "image/png" });
    // v7.2: expect_project (opt-in) compares this file against what the LIVE project should have
    // produced -- review_export already told you everything ABOUT the file; nothing told you whether
    // it's the RIGHT file. Same expected-duration technique lint_edit uses (max clip end across every
    // track); a project with no clips at all reports no expectation rather than a false mismatch.
    let qc = if a.get("expect_project").and_then(|v| v.as_bool()).unwrap_or(false) {
        let expected_end = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.start + c.duration).fold(0.0, f64::max);
        let (ew, eh) = (ss.project.width, ss.project.height);
        if expected_end <= 0.0 {
            json!({"checked": false, "reason": "the live project has no clips, so there is nothing to compare this file against"})
        } else {
            const DUR_TOLERANCE_S: f64 = 0.2; // a couple of frames at typical fps, not exact-equality brittle
            let dur_diff = dur - expected_end;
            let gw = v["width"].as_u64().map(|x| x as u32); let gh = v["height"].as_u64().map(|x| x as u32);
            let res_match = gw == Some(ew) && gh == Some(eh);
            json!({"checked": true, "duration_match": dur_diff.abs() <= DUR_TOLERANCE_S, "duration_diff_s": r3(dur_diff), "expected_duration": r3(expected_end),
                "resolution_match": res_match, "expected_width": ew, "expected_height": eh, "got_width": gw, "got_height": gh})
        }
    } else { json!({"checked": false, "reason": "pass expect_project:true to compare against the live project's own expected duration and resolution"}) };
    Ok(json!({"path": path, "duration": r3(dur), "size_mb": r3(size / 1_048_576.0), "width": v["width"], "height": v["height"], "video_codec": v["codec_name"], "audio_codec": audio_codec, "lufs": lufs.map(r3), "sample_peak_dbfs": if peak > 0.0 { Some(r3(20.0 * peak.log10())) } else { None }, "sheet": out, "frames": n, "qc": qc,
        "note": "the sheet shows the exported file itself, left to right, top to bottom"}))
}

pub fn commands() -> Vec<Command> {
    let ids = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    vec![
        Command { name: "auto_rough_cut", description: "Lay talking-head assets back to back with the dead air cut out (speech detection). Returns clip ids, speech times and segments.", mutates: true,
            schema: obj(vec![("asset_ids", ids("talking-head assets in story order")), ("track_id", s("optional; rebuilds this track from start")), ("start", n("timeline seconds, default 0")),
                ("drop", json!({"type": "array", "description": "segments to leave out, e.g. a false start: [{asset_id, index}] from an earlier result", "items": {"type": "object", "properties": {"asset_id": {"type": "string"}, "index": {"type": "integer"}}, "required": ["asset_id", "index"]}})),
                ("max_pause", n("keep pauses shorter than this inside a segment, default 0.35 s")), ("pad", n("seconds kept around speech, default 0.06")), ("soft", s("\"trim\" (default): cut blurry or whip-pan edges, drop a mostly blurry segment; \"flag\": report only")), ("dedupe", json!({"type": "boolean", "description": "drop a segment that looks like one already kept from another clip (default false; segments report similar_to)"}))], &["asset_ids"]), run: |ss, a| atomic(ss, a, rough_cut) },
        Command { name: "place_broll", description: "Lay b-roll over a dialogue track only while someone speaks, cut on pauses, b-roll muted. Returns clip ids and times.", mutates: true,
            schema: obj(vec![("track_id", s("the dialogue track")), ("asset_ids", ids("b-roll assets, in order")), ("start", n("window start, timeline s (default: after the first phrase)")), ("end", n("window end, timeline s")),
                ("min_shot", n("shortest shot, default 1.0 s")), ("max_shot", n("longest shot, default 2.5 s")), ("in_points", json!({"type": "object", "description": "optional {asset_id: source seconds}"})), ("volume", n("b-roll sound 0..1, default 0 (muted)")),
                ("words", json!({"type": "array", "description": "optional [{text,start,end}]"})),
                ("asset_tags", json!({"type": "object", "description": "optional {asset_id:[tag]}"})),
                ("whoosh_sfx", json!({"type":"boolean","description":"default FALSE: synthesized whoosh per b-roll cut-in -- costs whisper recall on real footage even ducked low; opt in deliberately"}))], &["track_id", "asset_ids"]), run: |ss, a| atomic(ss, a, place_broll) },
        Command { name: "caption_from_words", description: "Captions from words you give (per source clip or whole track), timed to the detected speech, clear of the platform UI. Best-effort checks each cue's words against what kimo's own recognizer actually hears on that audio and flags any it genuinely can't make out (low_confidence_cues, and lint_edit's caption_low_confidence) - check those before export rather than guess. Returns cues.", mutates: true,
            schema: obj(vec![("track_id", s("the dialogue track")), ("text", s("the words spoken on the whole track, in order")), ("by_asset", json!({"type": "object", "description": "better: {asset_id: words spoken in that clip}; each clip's words stay on its own shots"})), ("style", s("boxed (default)|subtitle|yellow")), ("platform", s("tiktok (default)|reels|shorts|none")),
                ("max_chars", n("characters per line, default 22")), ("lines", n("lines per cue, default 2")), ("size", n("pixel size; default fits the safe width")),
                ("check_confidence", json!({"type":"boolean","description":"best-effort: cross-check given words against the recognizer on the same audio and flag cues it can't make out confidently (default true; set false to skip, e.g. no recognizer installed)"})),
                ("hint", s("vocabulary/topic hint for the confidence check only (names, jargon in the clip) - does not change the caption text")),
                ("whisper_bin", s("confidence check only: path to whisper-cli")), ("model", s("confidence check only: path to a ggml model"))], &["track_id"]), run: |ss, a| atomic(ss, a, caption_from_words) },
        Command { name: "finish_audio", description: "Mix for social in one call: dialogue cleaned, gently levelled and compressed per clip, b-roll sound off, music under speech, mix to target LUFS.", mutates: true,
            schema: obj(vec![("target_lufs", n("default -14")), ("keep_ambience", json!({"type": "boolean", "description": "keep b-roll sound (default false: muted)"})), ("music_clip_ids", ids("clips to treat as music (audio-only clips are music already)")),
                ("music_under_speech_db", n("music level under dialogue, relative, default -14")),
                ("level_dynamics", json!({"type": "boolean", "description": "gentle speech leveling (narrows loudness range), default true"})),
                ("level_strength", n("leveling factor 1..4, default 2.5")),
                ("match_strength", n("0..1, how fully each clip is pulled to the loudest, default 1.0 (full match)")),
                ("compress_strength", n("0..1, dialogue compressor ratio scale, default 0.0 (off; 1.0=old fixed 3:1)"))], &[]), run: |ss, a| atomic(ss, a, finish_audio) },
        Command { name: "auto_grade", description: "Fix each clip's exposure and colour cast from a measured frame; gains are clamped so no shot goes black or blown.", mutates: true,
            schema: obj(vec![("clip_ids", ids("default: every media clip")), ("strength", n("0..1, default 0.85")), ("target_luma", n("0.2..0.7, default 0.42"))], &[]), run: |ss, a| atomic(ss, a, auto_grade) },
        Command { name: "cut_to_beat", description: "Music reel in one call: lays the music and cuts the shots exactly on its beats, N beats per shot. Returns clip ids.", mutates: true,
            schema: obj(vec![("music_asset_id", s("imported music")), ("asset_ids", ids("shots in order (reused in turn if more shots are needed)")), ("beats_per_shot", n("default 4")), ("duration", n("target seconds (default: one shot per asset)")),
                ("start", n("timeline seconds, default 0")), ("in_points", json!({"type": "object", "description": "optional {asset_id: source seconds}"}))], &["music_asset_id", "asset_ids"]), run: |ss, a| atomic(ss, a, cut_to_beat) },
        Command { name: "cut_to_speech", description: "Narration reel in one call: lays the dialogue and cuts the shots exactly where new speech starts, N speech intervals per shot. Returns clip ids.", mutates: true,
            schema: obj(vec![("dialogue_asset_id", s("imported narration/voiceover")), ("asset_ids", ids("shots in order (reused in turn if more shots are needed)")), ("phrases_per_shot", n("speech intervals per shot, default 1")),
                ("aggressiveness", n("VAD aggressiveness 0..3, default 1")), ("min_silence_ms", n("gap that separates two speech intervals, default 300")), ("pad_ms", n("speech edge padding, default 120")),
                ("start", n("timeline seconds, default 0")), ("in_points", json!({"type": "object", "description": "optional {asset_id: source seconds}"}))], &["dialogue_asset_id", "asset_ids"]), run: |ss, a| atomic(ss, a, cut_to_speech) },
        Command { name: "lint_edit", description: "Pre-export check: concrete problems (dark frames, text under the UI, b-roll over silence, loudness, captions, fonts), each with its fix call.", mutates: false,
            schema: obj(vec![("platform", s("tiktok (default)|reels|shorts|none")), ("target_lufs", n("default -14")), ("duration", json!({"type": "array", "items": {"type": "number"}, "description": "[min, max] seconds"})), ("dialogue_track", s("default: the lowest track with speech"))], &[]), run: lint_edit },
        Command { name: "timeline_summary", description: "Compact view of the edit: tracks bottom to top with each clip's id, source, start/end and text. Small; use instead of get_project.", mutates: false,
            schema: obj(vec![], &[]), run: timeline_summary },
        Command { name: "remove_transition", description: "Remove the transition into a clip (a straight cut again).", mutates: true,
            schema: obj(vec![("clip_id", s("the clip whose incoming transition goes"))], &["clip_id"]), run: remove_transition },
        Command { name: "review_export", description: "Look at a video file (your export or a source clip): a contact sheet of its frames (inline image) plus duration, size, codecs, LUFS, peak.", mutates: false,
            schema: obj(vec![("path", s("the exported .mp4")), ("n", n("frames, 2..16, default 8")), ("sheet_path", s("where the PNG goes (default: a temp file)")), ("expect_project", json!({"type":"boolean","description":"also compare duration and resolution against the live project's own expected values (qc.duration_match/resolution_match); false (default) just reports the file's own facts"}))], &["path"]), run: review_export },
    ]
}
