//! `auto_edit`: one call, raw clips -> a finished, polished edit, chaining Kimo's own existing tools with
//! sensible defaults -- rough cut (dead air out) -> b-roll placement (RV-33: matched to the words spoken,
//! given a transcript) -> jump-cut punch-ins (only on the segments that stay on screen, so the zoom is never
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

fn auto_edit(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let th_ids = str_list(a, "talking_head_asset_ids");
    if th_ids.is_empty() { return Err("talking_head_asset_ids: give one or more imported talking-head asset ids, in story order (import_media first)".into()); }
    let broll_ids = str_list(a, "broll_asset_ids");
    let music_id = a.get("music_asset_id").and_then(|v| v.as_str()).map(String::from);
    let hook_text = a.get("hook_text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
    let hook_style = a.get("hook_style").and_then(|v| v.as_str()).unwrap_or("hook_box").to_string();
    let caption_style = a.get("caption_style").and_then(|v| v.as_str()).unwrap_or("pop").to_string();
    let captions_on = a.get("captions").and_then(|v| v.as_bool()).unwrap_or(true);
    let target_lufs = opt_num(a, "target_lufs", -14.0);
    let level_dynamics = a.get("level_dynamics").and_then(|v| v.as_bool());
    let level_strength = a.get("level_strength").and_then(|v| v.as_f64());
    let match_strength = a.get("match_strength").and_then(|v| v.as_f64());
    let compress_strength = a.get("compress_strength").and_then(|v| v.as_f64());
    let whoosh_sfx = a.get("whoosh_sfx").and_then(|v| v.as_bool());
    let punch_scale = opt_num(a, "punch_scale", 1.15).clamp(1.01, 3.0);
    let punch_on = a.get("punch_ins").and_then(|v| v.as_bool()).unwrap_or(true);
    let grade_on = a.get("grade").and_then(|v| v.as_bool()).unwrap_or(true);
    // V8.0Q: on by default whenever there's a dialogue clip and at least one b-roll clip to match -- the
    // free color-cohesion win a raw auto-cut never gets (measured on the real benchmark: an indoor warm
    // talking head cut against a cool-toned city b-roll and a bright-daylight coffee b-roll, each already
    // "well exposed and neutral" by auto_grade's own absolute band, so auto_grade alone left the mismatch
    // untouched). Off entirely for callers who want their own look preserved.
    let broll_match_on = a.get("broll_color_match").and_then(|v| v.as_bool()).unwrap_or(true);
    let broll_match_strength = a.get("broll_color_match_strength").and_then(|v| v.as_f64());
    let platform = a.get("platform").and_then(|v| v.as_str()).unwrap_or("none").to_string();
    // default on for the landscape/youtube-style edit (platform none); a vertical short usually ends on
    // whatever the platform's own UI shows next, so it defaults off there unless explicitly asked for.
    let end_screen_on = a.get("end_screen").and_then(|v| v.as_bool()).unwrap_or(platform == "none");
    let end_screen_text = a.get("end_screen_text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("SUBSCRIBE FOR MORE").to_string();
    let end_screen_duration = opt_num(a, "end_screen_duration", 3.0).clamp(2.0, 4.0);
    let pacing_on = a.get("pacing").and_then(|v| v.as_bool()).unwrap_or(true);

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

    // 3. Punch-ins (jump-cut zoom variation) only on the dialogue clips that stay mostly on screen -- a
    // clip b-roll already covers for most of its length would animate a zoom nobody ever sees.
    if punch_on {
        let visible: Vec<String> = dialogue_clips.iter().filter(|(_, s, e)| covered_fraction(*s, *e, &broll_ranges) < 0.5).map(|(id, _, _)| id.clone()).collect();
        if visible.len() >= 2 {
            let pin = call(ss, "add_punch_ins", &json!({"clip_ids": visible, "scale": punch_scale, "pattern": "alternate", "ease": 0.2, "variety": true}))?;
            stages.insert("punch_ins".into(), pin);
        } else {
            warnings.push(format!("punch-ins skipped: only {} dialogue clip(s) stay on screen past the b-roll coverage, not enough for wide/tight variation", visible.len()));
        }
    }

    // 4. Captions: word-synced, from the transcript taken in step 1b (same words b-roll relevance used --
    // one recognizer pass for the whole edit, not two).
    if captions_on {
        let mut cap_args = json!({"style": caption_style, "position": "bottom"});
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
        let mut match_args = json!({"reference_clip_ids": reference_clip_ids, "target_clip_ids": broll_clip_ids});
        if let Some(v) = broll_match_strength { match_args["strength"] = json!(v); }
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
            let last_source_t = (last_in + last_dur - 1.0 / proj_fps).max(last_in);
            let bg = call(ss, "add_clip", &json!({"track_id": bg_tt, "source_type": "media", "asset_id": last_asset, "start": edit_duration, "duration": end_screen_duration, "in_point": last_source_t, "fit": "cover"}))?;
            if let Some(bg_cid) = bg["clip_id"].as_str() {
                call(ss, "freeze_frame", &json!({"clip_id": bg_cid, "at": edit_duration}))?;
                call(ss, "set_audio", &json!({"clip_id": bg_cid, "mute": true}))?;
                call(ss, "add_effect", &json!({"clip_id": bg_cid, "kind": "blur", "params": {"radius": 18.0}}))?;
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

    Ok(json!({
        "ok": true, "track_id": track_id, "duration": edit_duration, "total_duration": total_duration, "mix_lufs": audio["mix_lufs"],
        "stages": stages, "warnings": warnings, "lint_clean": lint_ok, "remaining_problems": remaining,
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
                ("captions", json!({"type": "boolean", "description": "default true"})),
                ("punch_ins", json!({"type": "boolean", "description": "default true"})),
                ("punch_scale", n("punched-in zoom over wide, default 1.15")),
                ("grade", json!({"type": "boolean", "description": "default true: auto_grade every clip"})),
                ("broll_color_match", json!({"type": "boolean", "description": "default true (when there is b-roll): pull each b-roll clip's own measured look toward the talking head's, so a cut against footage shot in different light still reads as one edit"})),
                ("broll_color_match_strength", n("0..1, default 0.6, how fully each b-roll clip is pulled toward the talking head's look")),
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
            ], &["talking_head_asset_ids"]), run: |ss, a| atomic(ss, a, auto_edit) },
    ]
}
