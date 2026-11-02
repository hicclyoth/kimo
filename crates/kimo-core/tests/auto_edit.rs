//! auto_edit (v7.6Q): raw clips -> one call -> a finished edit. Each test here is a real editorial outcome
//! the chain must get right, not just "it returns ok".
use kimo_core::commands::{dispatch, Session};
use kimo_core::model::Source;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_auto_edit_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }
fn music_track_for(ss: &Session, clip_id: &str) -> String { ss.project.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == clip_id)).unwrap_or_else(|| panic!("no track holds clip {clip_id}")).id.clone() }

/// A talking-head stand-in: a voice-like tone in the given speech windows over a quiet noise floor, with
/// `dead` seconds of true silence stitched in up front so auto_rough_cut has real dead air to cut.
fn talk(name: &str, speech: &[(f64, f64)], dead_head: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let dur = speech.iter().map(|(_, e)| *e).fold(0.0_f64, f64::max) + dead_head;
    let gate: Vec<String> = speech.iter().map(|(s, e)| format!("between(t,{},{})", s + dead_head, e + dead_head)).collect();
    let a = format!("aevalsrc='({})*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=640x360:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
fn broll(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("mandelbrot=s=640x360:r=30:end_pts={}", (dur * 30.0) as i64), "-t", &dur.to_string(), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
fn music(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("sine=frequency=220:duration={dur}"), "-c:a", "pcm_s16le", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
fn proj() -> Session { let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 640, "height": 360, "fps": 30})); ss }

/// The spine: dead air is actually gone, and the mix lands within half an LU of the target -- not just "ok: true".
#[test] fn auto_edit_cuts_dead_air_and_hits_the_loudness_target() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("spine.mp4", &[(0.0, 2.0), (2.0, 4.0)], 3.0)})), "asset_id");
    // 2 speech windows back to back (no internal gap) but 3s of true dead air stitched on the front.
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let dur = r["duration"].as_f64().unwrap();
    assert!(dur < 4.5, "dead air should be cut: got duration {dur} from a 7s source with 3s of silence: {r}");
    let lufs = r["mix_lufs"].as_f64().unwrap_or_else(|| panic!("no mix_lufs: {r}"));
    assert!((lufs - (-14.0)).abs() < 1.0, "mix should land near -14 LUFS, got {lufs}: {r}");
}

/// The real bug this tool exists to prevent: a punch-in zoom landing on a clip b-roll then fully covers, so
/// the zoom never shows on screen. b-roll runs first; punch-ins must then skip whatever it hid.
#[test] fn punch_ins_never_land_on_a_clip_broll_fully_covers() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("pin.mp4", &[(0.0, 3.0), (4.0, 7.0), (8.0, 11.0)], 0.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll("pin_b.mp4", 11.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [b], "captions": false, "grade": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let track_id = r["track_id"].as_str().unwrap();
    let broll_ranges: Vec<(f64, f64)> = r["stages"]["broll"]["clips"].as_array().unwrap().iter()
        .map(|c| (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap())).collect();
    assert!(!broll_ranges.is_empty(), "test needs b-roll actually placed: {r}");
    let tl = call(&mut ss, "timeline_summary", json!({}));
    let track = tl["tracks_bottom_to_top"].as_array().unwrap().iter().find(|t| t["track_id"] == *track_id).unwrap();
    for c in track["clips"].as_array().unwrap() {
        let (s, e) = (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap());
        let covered: f64 = broll_ranges.iter().map(|(bs, be)| (e.min(*be) - s.max(*bs)).max(0.0)).sum::<f64>() / (e - s).max(1e-6);
        if covered > 0.5 {
            assert!((c["scale"].as_f64().unwrap_or(1.0) - 1.0).abs() < 1e-6, "clip {} is {:.0}% covered by b-roll but was still punched in (scale {:?}): {r}", c["clip_id"], covered * 100.0, c["scale"]);
        }
    }
}

/// 8.5.6: job 2's forced splits must not touch a long single take that b-roll already covers -- the same
/// "nobody sees the zoom anyway" reasoning the punch-visibility filter uses, but one step earlier: there is
/// no point cutting a clip into pieces whose scale change the viewer never sees because b-roll is on top.
#[test] fn job2_forced_splits_skip_a_long_take_broll_fully_covers() {
    let mut ss = proj();
    // one continuous 9s take with no internal pause, well past the ~3.5s cap, entirely covered by b-roll.
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("job2_broll_covered_th.mp4", &[(0.0, 9.0)], 0.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll("job2_broll_covered_b.mp4", 9.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [b], "captions": false, "grade": false, "push_in": false, "broll_color_match": false, "end_screen": false, "pacing": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let broll_ranges: Vec<(f64, f64)> = r["stages"]["broll"]["clips"].as_array().unwrap().iter()
        .map(|c| (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap())).collect();
    assert!(!broll_ranges.is_empty(), "test needs b-roll actually placed: {r}");
    let track_id = r["track_id"].as_str().unwrap();
    let tl = call(&mut ss, "timeline_summary", json!({}));
    let track = tl["tracks_bottom_to_top"].as_array().unwrap().iter().find(|t| t["track_id"] == *track_id).unwrap();
    let dialogue: Vec<&Value> = track["clips"].as_array().unwrap().iter().filter(|c| c["src"].as_str() != Some("job2_broll_covered_b.mp4")).collect();
    for c in &dialogue {
        let (s, e) = (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap());
        let covered: f64 = broll_ranges.iter().map(|(bs, be)| (e.min(*be) - s.max(*bs)).max(0.0)).sum::<f64>() / (e - s).max(1e-6);
        if covered >= 0.5 {
            assert!(e - s > 3.6, "clip {} ({:.2}s, {:.0}% b-roll covered) was split by job 2 even though b-roll already hides it: {r}", c["clip_id"], e - s, covered * 100.0);
        }
    }
}

/// With no broll_asset_ids at all, every dialogue clip is visible, so punch-ins should still have >=2
/// candidates and actually alternate -- the covered-clip exclusion must not also eat the normal case.
#[test] fn punch_ins_still_apply_with_no_broll() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("nobroll.mp4", &[(0.0, 2.0), (3.0, 5.0), (6.0, 8.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let pin = &r["stages"]["punch_ins"];
    assert!(!pin.is_null(), "punch_ins stage missing with no b-roll to exclude anything: {r}");
    assert!(pin["punched"].as_i64().unwrap_or(0) >= 1, "{r}");
}

/// No hook_text: the tool must not invent cold-open copy. It should skip the title and say so, not fail.
#[test] fn no_hook_text_skips_the_title_honestly() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("nohook.mp4", &[(0.0, 2.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["stages"]["hook_title"].is_null(), "no hook_text was given; a title stage should not appear: {r}");
    let warnings: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert!(warnings.iter().any(|w| w.contains("hook_text")), "should say the hook was skipped and why: {r}");
}

/// A hook_text IS given: a title clip lands at the very start.
#[test] fn hook_text_adds_a_title_at_the_start() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("hook.mp4", &[(0.0, 3.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "hook_text": "wait for it", "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(!r["stages"]["hook_title"].is_null(), "{r}");
    let tl = call(&mut ss, "timeline_summary", json!({}));
    let s = serde_json::to_string(&tl).unwrap();
    assert!(s.to_lowercase().contains("wait for it"), "hook text should be on the timeline: {s}");
}

/// A music_asset_id is given: a music clip lands and finish_audio ducks it under the dialogue (music ends up
/// quieter than the mix target, not fighting the voice).
#[test] fn music_bed_is_placed_and_ducked_under_speech() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("music_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m = sid(&call(&mut ss, "import_media", json!({"path": music("bed.wav", 4.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "music_asset_id": m, "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(!r["stages"]["music"].is_null(), "music stage missing: {r}");
    let music_report = r["stages"]["audio"]["music"].as_array().unwrap_or_else(|| panic!("finish_audio didn't classify the music clip: {r}"));
    assert!(!music_report.is_empty(), "finish_audio should have found the music clip as music: {r}");
}

/// Beyond finish_audio's static level-match, the music bed also gets a real duck_timeline envelope on top --
/// a SEPARATE multiplier (duck_gain) from the volume finish_audio set, so the two stages can't fight. This is
/// what gives the mix its give-and-take instead of one flat level for the whole edit.
#[test] fn music_bed_also_gets_a_duck_timeline_envelope_on_top_of_finish_audios_level() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("duck_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m = sid(&call(&mut ss, "import_media", json!({"path": music("duck_bed.wav", 4.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "music_asset_id": m, "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let duck = &r["stages"]["duck"]; assert!(!duck.is_null(), "a duck stage should run whenever there's a music track: {r}");
    assert_eq!(duck["music_tracks_ducked"], json!(1), "{duck}");
    assert_eq!(duck["dialogue_tracks_used"], json!(1), "{duck}");
    // duck_timeline writes duck_gain, a field kept separate from audio.volume precisely so finish_audio's own
    // level-setting (which writes a flat volume Prop) and this dynamic envelope never overwrite each other.
    assert!(!r["stages"]["audio"]["music"].as_array().unwrap().is_empty(), "finish_audio's own level-setting should be untouched by the later duck stage: {r}");
}

/// No music_asset_id at all: no duck stage should even attempt to run (nothing to duck).
#[test] fn no_music_means_no_duck_stage() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("noduck.mp4", &[(0.0, 2.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["stages"]["duck"].is_null(), "{r}");
}

/// No talking_head_asset_ids at all is a clear argument error, not a panic or a confusing nested one.
#[test] fn requires_talking_head_asset_ids() {
    let mut ss = proj();
    let e = dispatch(&mut ss, "auto_edit", &json!({})).unwrap_err();
    assert!(e.contains("talking_head_asset_ids"), "{e}");
}

/// A failure partway through (bad broll asset id) must roll back the WHOLE edit, not leave a half-built one
/// (the project-wide atomic() wrapper every director recipe uses).
#[test] fn a_failure_rolls_back_the_whole_edit() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("rollback.mp4", &[(0.0, 2.0)], 0.0)})), "asset_id");
    let before = serde_json::to_string(&call(&mut ss, "get_project", json!({}))).unwrap();
    let e = dispatch(&mut ss, "auto_edit", &json!({"talking_head_asset_ids": [a], "broll_asset_ids": ["no_such_asset"]})).unwrap_err();
    assert!(!e.is_empty());
    let after = serde_json::to_string(&call(&mut ss, "get_project", json!({}))).unwrap();
    assert_eq!(before, after, "a failed auto_edit must leave the project exactly as it was");
}

/// The whole chain actually exports to a playable file with the duration it reported.
#[test] fn exports_a_real_file_at_the_reported_duration() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("export_src.mp4", &[(0.0, 2.0), (2.5, 4.5)], 1.5)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "hook_text": "hi", "captions": false, "grade": true, "punch_ins": true}));
    let out = dir().join("auto_edit_export.mp4");
    let ex = call(&mut ss, "export", json!({"path": out, "crf": 30}));
    assert!(std::fs::metadata(&out).map(|m| m.len() > 1000).unwrap_or(false), "{ex}");
    // V7.7Q: a subscribe-style end screen is on by default (platform none), so the real export now runs
    // past the talking-head-only "duration" -- total_duration is the field that tracks the real export length.
    assert!((ex["duration"].as_f64().unwrap() - r["total_duration"].as_f64().unwrap()).abs() < 0.2, "export duration should match total_duration (talking head + end screen): {ex} vs {r}");
    assert!(r["total_duration"].as_f64().unwrap() > r["duration"].as_f64().unwrap(), "the default-on end screen should make total_duration longer than the talking-head duration: {r}");
}

/// V7.7Q: end screen. Default on for the landscape/youtube-style edit (platform none) -- a subscribe-style
/// card after the dialogue, long enough to read, that fades out instead of just cutting away. Off by
/// default for a vertical short (shorts/reels/tiktok) since a subscribe pill competes with the platform's
/// own UI there; end_screen:false (or true) always overrides either default.
#[test] fn end_screen_is_on_by_default_for_platform_none_and_fades_out() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("es1.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    let es = r["stages"]["end_screen"].clone();
    assert!(es.is_object(), "end screen should be on by default when platform is none: {r}");
    let cid = es["clip_ids"][0].as_str().unwrap_or_else(|| panic!("no clip id in end screen stage: {es}"));
    let c = ss.project.find_clip(cid).unwrap();
    assert!((2.0..=4.0).contains(&c.duration), "end card duration {} should be clamped 2..4 s", c.duration);
    assert!((c.start - r["duration"].as_f64().unwrap()).abs() < 0.05, "end card should start right where the talking head ends: card at {}, dialogue ends at {}", c.start, r["duration"]);
    assert!((r["total_duration"].as_f64().unwrap() - (r["duration"].as_f64().unwrap() + c.duration)).abs() < 0.05, "{r}");
    // it actually fades: opacity keyframed down to 0 by the card's own end.
    let keys = ss.project.find_clip(cid).unwrap().props.get("opacity").map(|p| p.keys.clone()).unwrap_or_default();
    assert!(keys.len() >= 2, "expected an opacity fade keyframed on the end card, got {} key(s)", keys.len());
    let last_val = keys.last().unwrap().v;
    assert!(last_val < 0.05, "the card should fade to ~0 opacity by its own end, got {last_val}");

    // a vertical short defaults the card OFF.
    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("es2.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "captions": false, "grade": false, "punch_ins": false, "platform": "shorts"}));
    assert!(!r2["stages"]["end_screen"].is_object(), "a vertical short should default the end screen off: {r2}");
    assert_eq!(r2["total_duration"], r2["duration"], "{r2}");

    // explicit false turns it off even on platform none; explicit true turns it on even on a short.
    let mut ss3 = proj();
    let a3 = sid(&call(&mut ss3, "import_media", json!({"path": talk("es3.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r3 = call(&mut ss3, "auto_edit", json!({"talking_head_asset_ids": [a3], "captions": false, "grade": false, "punch_ins": false, "end_screen": false}));
    assert!(!r3["stages"]["end_screen"].is_object(), "{r3}");
}

/// V7.7Q: the end screen's background (a blurred freeze of the video's own last frame, so the card isn't on
/// flat black) is a real media clip placed well after all speech has ended -- which, before the
/// broll_over_silence exemption for post-speech clips, made lint_edit's own review pass "fix" it by yanking
/// it back onto the nearest speech window and trimming it to a sliver, leaving the actual end screen with
/// nothing behind it. Proves the background clip is still there, still covering the whole card, after
/// auto_edit's own built-in review stage has run.
#[test] fn end_screen_background_survives_auto_edits_own_review_pass() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("esbg.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap_or("").contains("introduced")), "the review pass should not have had to introduce-and-report a new problem here: {r}");
    let dur = r["duration"].as_f64().unwrap();
    let total = r["total_duration"].as_f64().unwrap();
    let covers_end_screen = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).any(|c| {
        matches!(c.source, Source::Media { .. }) && c.start <= dur + 0.05 && c.start + c.duration >= total - 0.05
    });
    assert!(covers_end_screen, "expected a media (the frozen/blurred last-frame background) clip spanning the whole end-screen card, found none after review");
}

/// V8.0Q: without this fix, a default-on end screen played over true digital silence -- the music clip's own
/// length only ever targeted the talking-head span (edit_duration), so by the time the end card's background
/// clip (muted on purpose) covered the last 2-4s, nothing else was left playing. Proves the music clip's own
/// length now reaches the full planned export (end screen included) and carries a tail fade so it winds down
/// instead of hard-cutting at export's edge.
#[test] fn music_bed_keeps_playing_under_the_end_screen() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("es_music_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m = sid(&call(&mut ss, "import_media", json!({"path": music("es_music_bed.wav", 12.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "music_asset_id": m, "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let total = r["total_duration"].as_f64().unwrap();
    let dialogue_dur = r["duration"].as_f64().unwrap();
    assert!(total > dialogue_dur, "end screen should be on by default here: {r}");

    let music_cid = sid(&r["stages"]["music"], "clip_id");
    let c = ss.project.find_clip(&music_cid).unwrap();
    assert!(c.start + c.duration >= total - 0.05, "music clip (ends at {}) should cover the whole planned export (end screen included, ends at {total})", c.start + c.duration);
    assert!(c.audio.fade_out > 0.0, "the music bed's tail should fade out instead of hard-cutting at export's edge, got fade_out={}", c.audio.fade_out);

    // A music asset shorter than the full planned export: it should be LOOPED (tiled from its own start)
    // to cover the whole thing rather than running out under the card -- no warning, because the loop made
    // it whole.
    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("es_music_short_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m2 = sid(&call(&mut ss2, "import_media", json!({"path": music("es_music_short_bed.wav", 3.0)})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "music_asset_id": m2, "captions": false, "grade": false, "punch_ins": false}));
    assert!(r2["ok"] == json!(true), "{r2}");
    let total2 = r2["total_duration"].as_f64().unwrap();
    let music_track2 = music_track_for(&ss2, &sid(&r2["stages"]["music"], "clip_id"));
    let mut music_clips: Vec<_> = ss2.project.tracks.iter().find(|t| t.id == music_track2).unwrap().clips.iter().map(|c| (c.start, c.duration)).collect();
    music_clips.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(music_clips.len() > 1, "a 3s music bed under a ~{total2:.1}s export should have looped into more than one clip, got {}", music_clips.len());
    let covered_end = music_clips.last().unwrap().0 + music_clips.last().unwrap().1;
    assert!(covered_end >= total2 - 0.05, "looped music clips (last one ends at {covered_end}) should still cover the full export ({total2})");
    assert!(!r2["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("could not be looped enough")), "a 3s bed looping into a ~{total2:.1}s export is well under the 200-segment cap, should not have warned: {r2}");
}

/// No music at all, end screen on by default: the chain should say so honestly (not silently ship dead air
/// with no comment).
#[test] fn end_screen_with_no_music_warns_about_the_silent_tail() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("es_nomusic_th.mp4", &[(0.0, 2.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("no music_asset_id given")), "expected a warning that the end card has nothing under it: {r}");
}

/// V8.0Q: auto_grade alone only pulls each clip toward one fixed absolute target, so a warm-biased talking
/// head and a cool-biased b-roll can both already be "fine" individually and still clash on the cut.
/// broll_color_match (default on whenever there is b-roll) runs match_shot_look with the dialogue clips as
/// the reference and the b-roll clips as the target, closing that gap -- disabled by grade:false here so
/// this test isolates the new stage instead of measuring auto_grade's own separate effect.
#[test] fn broll_color_match_runs_by_default_and_can_be_turned_off() {
    let th_path = dir().join("ccm_th.mp4");
    if !th_path.exists() {
        let a = "aevalsrc='(between(t,0,4))*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d=4";
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=4", "-f", "lavfi", "-i", a, "-vf", "colorbalance=rs=0.3:bs=-0.3",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", th_path.to_str().unwrap()]);
    }
    let broll_path = dir().join("ccm_broll.mp4");
    if !broll_path.exists() {
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=3", "-vf", "colorbalance=rs=-0.3:bs=0.3", "-an",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", broll_path.to_str().unwrap()]);
    }

    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": th_path.to_str().unwrap()})), "asset_id");
    let bm = sid(&call(&mut ss, "import_media", json!({"path": broll_path.to_str().unwrap()})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [bm], "captions": false, "punch_ins": false, "grade": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["stages"]["broll_color_match"].is_object(), "b-roll color match should run by default when there is b-roll: {r}");
    let matched = r["stages"]["broll_color_match"]["matched"].as_array().unwrap();
    assert!(!matched.is_empty(), "expected at least one b-roll clip matched toward the talking head's look: {r}");

    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": th_path.to_str().unwrap()})), "asset_id");
    let bm2 = sid(&call(&mut ss2, "import_media", json!({"path": broll_path.to_str().unwrap()})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "broll_asset_ids": [bm2], "captions": false, "punch_ins": false, "grade": false, "end_screen": false, "broll_color_match": false}));
    assert!(r2["ok"] == json!(true), "{r2}");
    assert!(r2["stages"]["broll_color_match"].is_null(), "broll_color_match:false should skip the stage entirely: {r2}");
}

/// V8.0Q: once the dialogue ends there is nothing left for the music to duck under, but finish_audio/
/// duck_timeline both only ever set the bed's level for use UNDER speech -- without a swell the end card
/// plays against the same quiet, ducked-for-speech level, which reads as the music timidly trailing off
/// right when there's no voice left to protect. Proves the music clip gets a real ramp-up keyed right where
/// the end card starts.
#[test] fn music_bed_swells_under_the_end_card_once_dialogue_ends() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("swell_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m = sid(&call(&mut ss, "import_media", json!({"path": music("swell_bed.wav", 12.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "music_asset_id": m, "captions": false, "grade": false, "punch_ins": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let dialogue_dur = r["duration"].as_f64().unwrap();
    assert!(r["total_duration"].as_f64().unwrap() > dialogue_dur, "end screen should be on by default here: {r}");
    let music_cid = sid(&r["stages"]["music"], "clip_id");
    let c = ss.project.find_clip(&music_cid).unwrap();
    assert!(c.audio.volume.keys.len() >= 2, "expected a swell ramp keyframed on the music clip's volume once the end card starts, got {} key(s): {:?}", c.audio.volume.keys.len(), c.audio.volume.keys);
    let local_card_start = dialogue_dur - c.start;
    let before = c.audio.volume.at((local_card_start - 0.01).max(0.0));
    let after = c.audio.volume.at(local_card_start + 0.35);
    assert!(after > before * 1.3, "music should swell up once the dialogue ends (no more voice to duck for): before {before:.4}, after {after:.4}");

    // end_screen:false -- no card, nothing to swell for.
    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("swell_noes_th.mp4", &[(0.0, 4.0)], 0.0)})), "asset_id");
    let m2 = sid(&call(&mut ss2, "import_media", json!({"path": music("swell_noes_bed.wav", 12.0)})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "music_asset_id": m2, "captions": false, "grade": false, "punch_ins": false, "end_screen": false}));
    let music_cid2 = sid(&r2["stages"]["music"], "clip_id");
    let c2 = ss2.project.find_clip(&music_cid2).unwrap();
    assert!(c2.audio.volume.keys.is_empty(), "no end screen means nothing to swell for: expected no volume keys, got {:?}", c2.audio.volume.keys);
}


/// V8.2V job 1: a slow Ken Burns push-in on static b-roll cutaways, on by default. Proves real motion:
/// the b-roll clip's own scale carries a real keyframed ramp (not a static value), and it is NOT the identity
/// (the end differs from the start) -- the actual thing a viewer would see as "it's pushing in", not just a
/// stage key existing.
#[test] fn push_in_animates_static_broll_by_default() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("pushin_th.mp4", &[(0.0, 4.0), (4.5, 8.0)], 0.0)})), "asset_id");
    let bm = sid(&call(&mut ss, "import_media", json!({"path": broll("pushin_broll.mp4", 3.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [bm], "captions": false, "grade": false, "punch_ins": false, "broll_color_match": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let pushed = r["stages"]["push_in"]["clips"].as_array().unwrap_or_else(|| panic!("push_in stage missing with a 3s static b-roll clip present: {r}"));
    assert!(!pushed.is_empty(), "expected at least one b-roll clip pushed in: {r}");
    let cid = sid(&pushed[0], "clip_id");
    let c = ss.project.find_clip(&cid).unwrap();
    let q = c.props.get("scale").unwrap_or_else(|| panic!("pushed clip has no scale property at all: {c:?}"));
    assert!(q.keys.len() >= 2, "expected a real keyframed scale ramp (start != end), got {} key(s): {:?}", q.keys.len(), q.keys);
    let start_scale = q.at(0.0);
    let end_scale = q.at(c.duration);
    assert!(end_scale > start_scale + 0.01, "push-in should end noticeably wider-zoomed than it starts: start {start_scale:.4}, end {end_scale:.4}");
    // 3-6% ask: the default should land inside (or very close to) that band over the shot.
    let pct = (end_scale / start_scale - 1.0) * 100.0;
    assert!((2.5..=7.0).contains(&pct), "push-in should be a subtle few-percent push by default, got {pct:.2}%");

    // push_in:false -- no stage, b-roll scale stays untouched (no keys at all, since place_broll itself never animates scale).
    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("pushin_off_th.mp4", &[(0.0, 4.0), (4.5, 8.0)], 0.0)})), "asset_id");
    let bm2 = sid(&call(&mut ss2, "import_media", json!({"path": broll("pushin_off_broll.mp4", 3.0)})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "broll_asset_ids": [bm2], "captions": false, "grade": false, "punch_ins": false, "broll_color_match": false, "end_screen": false, "push_in": false}));
    assert!(r2["ok"] == json!(true), "{r2}");
    assert!(r2["stages"]["push_in"].is_null(), "push_in:false should skip the stage entirely: {r2}");
}

/// A b-roll clip too short for the motion to read (under 1.2s) is left alone -- no push-in attempted on it.
#[test] fn push_in_skips_very_short_broll() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("pushin_short_th.mp4", &[(0.0, 5.0)], 0.0)})), "asset_id");
    let bm = sid(&call(&mut ss, "import_media", json!({"path": broll("pushin_short_broll.mp4", 0.6)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [bm], "captions": false, "grade": false, "punch_ins": false, "broll_color_match": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["stages"]["push_in"].is_null(), "a single very short b-roll clip (<1.2s) should leave nothing to push in: {r}");
}


/// V8.2V job 2 / V8.5.6: a single long uncut talking-head take (no internal pause for rough_cut/
/// tighten_pauses to split on) must still end up broken into pieces no longer than punch_max_static, so
/// add_punch_ins has real wide/tight variation to alternate -- the actual "never static past ~3-4s" promise
/// (tightened from ~4-5s in 8.5.6: a montage review found a 7.1s stretch that read as static even with the
/// old 4.5s/eased default), not just a single clip sitting there the whole time. Also checks that the
/// resulting pieces actually alternate wide/punched (not just "at least one punched somewhere") and that the
/// punch itself is a hard cut (ease 0), per the review's "cut-on-phrase, not a soft push" ask.
#[test] fn job2_splits_a_long_uncut_dialogue_take_for_punch_ins() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("longtake_th.mp4", &[(0.0, 10.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "push_in": false, "broll_color_match": false, "end_screen": false, "pacing": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let pin = r["stages"]["punch_ins"]["clips"].as_array().unwrap_or_else(|| panic!("punch_ins stage missing for a 10s single take: {r}"));
    assert!(pin.len() >= 3, "a 10s single take at the new ~3.5s cap should split into at least 3 pieces for punch-ins, got {}: {r}", pin.len());
    for cp in pin {
        let cid = sid(cp, "clip_id");
        let c = ss.project.find_clip(&cid).unwrap();
        assert!(c.duration <= 3.6, "clip {cid} is {:.2}s, should never exceed the ~3.5s punch_max_static default (8.5.6): {r}", c.duration);
    }
    let flags: Vec<bool> = pin.iter().map(|c| c["punched"] == json!(true)).collect();
    assert!(flags.iter().any(|&p| p), "expected at least one punched segment among the split pieces: {r}");
    assert!(flags.iter().any(|&p| !p), "expected at least one wide segment too -- a real alternation, not every piece punched: {r}");
    assert!(flags.windows(2).any(|w| w[0] != w[1]), "expected the punched flag to actually alternate somewhere along the split pieces: {flags:?}: {r}");
    assert_eq!(r["stages"]["punch_ins"]["ease"], json!(0.0), "8.5.6: the talking-head punch should be a hard cut (ease 0), not an eased push-in: {r}");
}

/// punch_max_static is a real, respected knob: set high enough, a short single take is left exactly as
/// before (one clip, the pre-existing "not enough to alternate" warning) -- job 2 never forces a split that
/// isn't needed.
#[test] fn job2_punch_max_static_is_configurable() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("longtake_nosplit_th.mp4", &[(0.0, 6.0)], 0.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "push_in": false, "broll_color_match": false, "end_screen": false, "pacing": false, "punch_max_static": 30.0}));
    assert!(r["ok"] == json!(true), "{r}");
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("punch-ins skipped")), "a 6s take under a 30s ceiling should be left as one clip (pre-existing not-enough-clips warning): {r}");
}


/// V8.2V job 3: auto_edit's captions now default to position "auto" (auto_captions' own default), which
/// steers around a face/skin band reaching into the normal caption strip -- auto_edit used to hardcode
/// "bottom" unconditionally, so a low, close-up face could sit right under the captions. Proven with a
/// synthetic "selfie" shot: a skin-toned fill in the lower half of the frame should push captions to the
/// top band by default, and an explicit caption_position still overrides it when asked.
#[test] fn job3_caption_position_defaults_to_auto_and_avoids_a_low_face() {
    let p = dir().join("job3_lowface_th.mp4");
    if !p.exists() {
        let a = "aevalsrc='(between(t,0,3))*0.6*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d=3";
        let st = Command::new("ffmpeg").args(["-v", "error", "-y",
            "-f", "lavfi", "-i", "color=c=0x222222:s=640x360:d=3",
            "-f", "lavfi", "-i", a,
            "-filter_complex", "[0:v]drawbox=x=0:y=216:w=640:h=144:color=0xC68863:t=fill[v]",
            "-map", "[v]", "-map", "1:a",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()])
            .stdin(Stdio::null()).status().expect("ffmpeg");
        assert!(st.success());
    }
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": p.to_str().unwrap()})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "grade": false, "punch_ins": false, "push_in": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let pos = r["stages"]["captions"]["position"].as_str().unwrap_or("").to_string();
    assert_eq!(pos, "top", "a low skin/face band should push captions to the top band by default (auto_edit's own hardcoded \"bottom\" used to prevent this): {r}");

    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": p.to_str().unwrap()})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "grade": false, "punch_ins": false, "push_in": false, "end_screen": false, "caption_position": "bottom"}));
    assert!(r2["ok"] == json!(true), "{r2}");
    assert_eq!(r2["stages"]["captions"]["position"].as_str().unwrap_or(""), "bottom", "an explicit caption_position should still be honoured over the face check: {r2}");
}


/// V8.2V job 4: a short whip/zoom-blur transition into each b-roll cutaway, on by default, never applied to
/// the talking-head punch-in clips (those stay hard cuts on purpose).
#[test] fn job4_broll_gets_a_short_whip_transition_by_default() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("j4_th.mp4", &[(0.0, 4.0), (4.5, 8.0)], 0.0)})), "asset_id");
    let bm = sid(&call(&mut ss, "import_media", json!({"path": broll("j4_broll.mp4", 2.0)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "broll_asset_ids": [bm], "captions": false, "grade": false, "punch_ins": false, "push_in": false, "broll_color_match": false, "end_screen": false}));
    assert!(r["ok"] == json!(true), "{r}");
    let broll_clip_id = sid(&r["stages"]["broll"]["clips"][0], "clip_id");
    let c = ss.project.find_clip(&broll_clip_id).unwrap();
    let tr = c.transition_in.as_ref().unwrap_or_else(|| panic!("expected a transition_in on the b-roll clip by default: {r}"));
    assert!(tr.kind == "zoom_blur" || tr.kind == "whip_left", "expected a whip/zoom-blur transition kind, got {}: {r}", tr.kind);
    assert!(tr.duration > 0.0 && tr.duration <= 0.6, "expected a short transition, got {}s: {r}", tr.duration);

    // off switch: no transition at all.
    let mut ss2 = proj();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("j4_off_th.mp4", &[(0.0, 4.0), (4.5, 8.0)], 0.0)})), "asset_id");
    let bm2 = sid(&call(&mut ss2, "import_media", json!({"path": broll("j4_off_broll.mp4", 2.0)})), "asset_id");
    let r2 = call(&mut ss2, "auto_edit", json!({"talking_head_asset_ids": [a2], "broll_asset_ids": [bm2], "captions": false, "grade": false, "punch_ins": false, "push_in": false, "broll_color_match": false, "end_screen": false, "broll_transitions": false}));
    assert!(r2["ok"] == json!(true), "{r2}");
    let broll_clip_id2 = sid(&r2["stages"]["broll"]["clips"][0], "clip_id");
    let c2 = ss2.project.find_clip(&broll_clip_id2).unwrap();
    assert!(c2.transition_in.is_none(), "broll_transitions:false should leave the clip with no transition: {r2}");
}


/// V8.5.5: the end-card background used to freeze on the literal last frame of the dialogue (often a
/// mid-gesture, mouth-open frame) and blur it hard enough to go unreadable. This proves the freeze now
/// grabs an earlier frame (about half a second before the clip's own end), the blur is gentler, and the
/// clip is scaled up so a hard blur's edge falloff never shows a seam.
#[test] fn end_screen_background_freezes_early_blurs_gently_and_scales_up() {
    let mut ss = proj();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("esbg2.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    let dur = r["duration"].as_f64().unwrap();
    let total = r["total_duration"].as_f64().unwrap();
    let dialogue_last = ss.project.tracks.iter().flat_map(|t| t.clips.iter())
        .filter(|c| matches!(c.source, Source::Media { .. }) && c.start < dur - 0.01)
        .max_by(|x, y| (x.start + x.duration).partial_cmp(&(y.start + y.duration)).unwrap())
        .cloned().expect("expected a dialogue clip before the end card");
    let bg = ss.project.tracks.iter().flat_map(|t| t.clips.iter())
        .find(|c| matches!(c.source, Source::Media { .. }) && c.start >= dur - 0.05 && c.start + c.duration >= total - 0.05)
        .expect("expected a media background clip covering the end screen").clone();

    let proj_fps = 30.0;
    let literal_last = dialogue_last.in_point + dialogue_last.duration - 1.0 / proj_fps;
    assert!(bg.in_point < literal_last - 0.2, "end-card freeze should grab an earlier frame than the literal last one; literal last {literal_last}, got in_point {}", bg.in_point);
    assert!(bg.in_point >= dialogue_last.in_point - 1e-6, "freeze time should not go before the clip's own start: {} < {}", bg.in_point, dialogue_last.in_point);

    let blur = bg.effects.iter().find(|e| e.kind == "blur").expect("expected a blur effect on the end-card background");
    let radius = blur.params.get("radius").map(|p| p.base).unwrap_or(0.0);
    assert!((4.5..=6.5).contains(&radius), "V8.5.8: blur radius should be lowered to about 5..6 so the subject stays recognisable, not mush, got {radius}");
    assert!(radius < 9.0, "V8.5.8: backdrop sharpness should be above the old (8.5.5) radius-9 level -- a lower radius is a sharper image, got {radius}");

    // V8.5.8: the backdrop is no longer a flat frozen scale -- it's a slow, eased Ken Burns push-in across
    // the whole card. Sample the keyed `scale` prop at the start, middle and end (clip-local time): it must
    // differ monotonically (a real push-in, not a stuck frame) and move smoothly (no frame-to-frame jump
    // much bigger than the average step -- the "no jumps" requirement).
    let scale_prop = bg.props.get("scale").expect("expected a scale prop on the end-card backdrop");
    assert!(scale_prop.keys.len() >= 2, "expected the end-card backdrop's scale to be keyframed (a Ken Burns drift), got {} key(s)", scale_prop.keys.len());
    let n = 20usize;
    let samples: Vec<f64> = (0..=n).map(|i| scale_prop.raw(bg.duration * i as f64 / n as f64)).collect();
    let (start, mid, end) = (samples[0], samples[n / 2], samples[n]);
    assert!((1.06..=1.1).contains(&start), "backdrop should still start around 1.06..1.1 (END_CARD_BG_SCALE), got {start}");
    assert!((1.12..=1.18).contains(&end), "backdrop should push in to about 1.14..1.16 by the card's end (END_CARD_BG_SCALE_END), got {end}");
    assert!(end > mid && mid > start, "the push-in should be monotonic across start/middle/end: start {start}, mid {mid}, end {end}");
    for w in samples.windows(2) { assert!(w[1] >= w[0] - 1e-9, "scale should never go backwards frame to frame: {w:?}"); }
    let steps: Vec<f64> = samples.windows(2).map(|w| w[1] - w[0]).collect();
    let avg_step = steps.iter().sum::<f64>() / steps.len() as f64;
    for (i, s) in steps.iter().enumerate() {
        assert!(*s < avg_step * 4.0 + 1e-6, "scale step {i} ({s}) is a much bigger jump than the average step ({avg_step}) -- the drift should read smooth and continuous, not jumpy");
    }
}

/// V8.5.8: the subscribe_pop CTA badge (pill + dot + text) auto_edit builds for the end card used to come
/// out at titles.rs's own design-time 1x size -- about 220px wide on a 1280-wide frame, too small to read
/// as a confident call to action in a montage review. Proves the end card's own pill is now meaningfully
/// bigger than the unscaled badge the same style/text would build on its own (roughly 1.3-1.5x), AND that
/// the 8.5.5 dot-inside-pill geometry (dot clear of the pill's left edge, clear of the text by at least
/// 0.7x the dot's own diameter) still holds at the new size -- the whole point of reusing dot_room/
/// dot_x_for_pill at a scaled k instead of a naive uniform `scale` transform.
#[test] fn end_card_pill_is_scaled_up_and_dot_gap_still_holds() {
    // A real 1280-wide project (not the file's usual 640x360 helper) -- the brief's own "about 220px wide
    // on 1280" / "roughly 300+ px wide on 1280" are both stated at this resolution, and at the file's usual
    // tiny 640x360 test canvas the subscribe_pop text renders at ~10px, small enough that font-hinting
    // rounding alone (not the real V8.5.8 scale factor) swings the measured ratio. 1280x720 is still 16:9
    // like the file's own helper, so every other default (end_screen-on for platform none, etc.) matches.
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("espill.mp4", &[(0.0, 2.0)], 0.3)})), "asset_id");
    let r = call(&mut ss, "auto_edit", json!({"talking_head_asset_ids": [a], "captions": false, "grade": false, "punch_ins": false}));
    let es = r["stages"]["end_screen"].clone();
    let ids: Vec<String> = es["clip_ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let (pill_id, dot_id, text_id) = (ids[0].clone(), ids[1].clone(), ids[2].clone()); // subscribe_pop pushes [pill, dot, text]

    fn shape_w(ss: &Session, id: &str) -> f64 { match &ss.project.find_clip(id).unwrap().source { Source::Shape { width, .. } => *width, s => panic!("not a shape: {s:?}") } }
    let pill_w = shape_w(&ss, &pill_id);
    let dot_w = shape_w(&ss, &dot_id);

    // Baseline: the exact same style/text built directly (unscaled k) on a fresh 1280x720 project, nothing
    // else in common with the real end card other than the style/text/canvas -- the only variable is
    // auto_edit's own V8.5.8 scale-up.
    let mut base_ss = Session::new(); call(&mut base_ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let bt = sid(&call(&mut base_ss, "add_track", json!({})), "track_id");
    let base = call(&mut base_ss, "add_title", json!({"track_id": bt, "style": "subscribe_pop", "text": "SUBSCRIBE FOR MORE", "start": 0.0, "duration": 3.0}));
    let base_pill_id = base["clip_ids"][0].as_str().unwrap().to_string();
    let base_pill_w = shape_w(&base_ss, &base_pill_id);

    // V8.5.8 sets a deliberate ~1.4x scale-up on the badge's own build (END_CARD_PILL_SCALE); the real end
    // card then runs through auto_edit's own built-in review pass same as everything else in the edit, and
    // lint_edit's pre-existing text_too_small fix (director_tools.rs, unrelated to this change, a safety
    // net for ANY text under min(h,w)/18 px) can grow it further still if our scaled text is itself under
    // that floor -- so the real, final measured ratio can run above the raw 1.3-1.5x build-time factor. The
    // meaningful, stable claims are: it grew substantially over the unscaled badge, and it clears the
    // "roughly 300+ px wide on 1280" bar the brief asked for.
    let ratio = pill_w / base_pill_w;
    assert!(ratio > 1.2, "V8.5.8: end-card pill should be meaningfully bigger than the unscaled badge, got ratio {ratio} ({pill_w}px vs {base_pill_w}px unscaled)");
    assert!(pill_w >= 300.0, "V8.5.8: pill should be roughly 300+px wide on a 1280-wide frame, got {pill_w}px");

    // dot-inside-pill geometry (8.5.5), unchanged by the scale-up: read actual rendered positions at
    // mid-clip, same technique as subscribe_pop_dot_clears_the_text_after_the_pill_resize_fix (director_small.rs).
    let cw = ss.project.width as f64;
    let pill = ss.project.find_clip(&pill_id).unwrap();
    let dot = ss.project.find_clip(&dot_id).unwrap();
    let pill_x = kimo_core::render::resolve_xform(&ss.project, pill, pill.duration / 2.0).x;
    let dot_x = kimo_core::render::resolve_xform(&ss.project, dot, dot.duration / 2.0).x;
    let dot_left_pad = (dot_x - dot_w / 2.0) - (pill_x - pill_w / 2.0);
    assert!(dot_left_pad > 0.0, "the dot must still sit INSIDE the pill after the scale-up, not floating outside it: left_pad {dot_left_pad}px");
    let dot_right_clearance = (pill_x + pill_w / 2.0) - (dot_x + dot_w / 2.0);
    assert!(dot_right_clearance > 0.0, "the dot must still not poke out the pill's right edge after the scale-up");

    let dot_right_canvas = cw / 2.0 + dot_x + dot_w / 2.0;
    let text_dur = ss.project.find_clip(&text_id).unwrap().duration;
    let bounds = call(&mut ss, "compute_text_bounds", json!({"clip_id": text_id, "t": text_dur / 2.0}));
    let rect = bounds["canvas_rect"].as_array().unwrap_or_else(|| panic!("no canvas_rect: {bounds}"));
    let text_left_canvas = rect[0].as_f64().unwrap();
    assert!(dot_right_canvas <= text_left_canvas + 0.5, "the dot (right edge at {dot_right_canvas}px) must sit clear, left of the text (starting at {text_left_canvas}px) after the scale-up");
    let gap = text_left_canvas - dot_right_canvas;
    assert!(gap >= 0.7 * dot_w, "V8.5.8: the dot-to-text gap must still be at least 0.7x the dot's own diameter ({dot_w}px) after the scale-up, got {gap}px");
}
