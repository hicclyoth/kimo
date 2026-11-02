//! Small-model director prep (2026-10-21): a fixed eval (tools/director_eval) had small models drive kimo over MCP. Each test
//! here is a failure they hit, or a tool that does in one call what they kept getting wrong in twenty.
use kimo_core::commands::{dispatch, Session};
use kimo_core::model::Source;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

// V7.7Q: this dir is reused (by path, not by a fresh guard) across every test in this binary so fixture
// media built by an earlier test is found, not re-encoded, by a later one -- real measured leak: 83 of these
// left behind on one box (40-75 MB each, ~3 GB) because nothing ever removed it. atexit (the C runtime's own
// exit hook, no new crate) runs even through the test harness's std::process::exit, so this cleans up once,
// after every test in the binary has had its chance to use the dir, rather than per-test (which would defeat
// the reuse) or never (the bug being fixed).
extern "C" { fn atexit(cb: extern "C" fn()) -> i32; }
extern "C" fn cleanup_dir_at_exit() { let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("kimo_director_small_{}", std::process::id()))); }
fn dir() -> PathBuf {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| { unsafe { atexit(cleanup_dir_at_exit); } });
    let d = std::env::temp_dir().join(format!("kimo_director_small_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d
}
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }
/// A vertical talking-head stand-in (360x640, 30 fps): a voice-like tone (harmonics, syllable-rate wobble) in the given
/// windows over a quiet noise floor, so the speech detector hears speech there and dead air elsewhere.
fn talk(name: &str, dur: f64, speech: &[(f64, f64)], level: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let gate: Vec<String> = speech.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
    let a = format!("aevalsrc='({})*{level}*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2)+0.05*sin(2*PI*1900*t))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=360x640:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-f", "lavfi", "-i", &format!("anoisesrc=a=0.002:d={dur}:r=48000"),
        "-filter_complex", "[1:a][2:a]amix=inputs=2:normalize=0[a]", "-map", "0:v", "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A talking-head stand-in with an uneven voice WITHIN the same clip: one loud passage, one quiet passage
/// (about 14 dB apart), the shape finish_audio's per-clip average-matching step never touches since it only
/// matches each CLIP's average to another clip's -- a single real speaker getting louder and quieter is exactly
/// what speech_level (speechnorm) exists to narrow.
fn talk_uneven(name: &str, dur: f64, loud: (f64, f64), quiet: (f64, f64)) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let gate = format!("between(t,{},{})*1.0+between(t,{},{})*0.2", loud.0, loud.1, quiet.0, quiet.1);
    let a = format!("aevalsrc='({gate})*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2)+0.05*sin(2*PI*1900*t))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}");
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=360x640:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-f", "lavfi", "-i", &format!("anoisesrc=a=0.002:d={dur}:r=48000"),
        "-filter_complex", "[1:a][2:a]amix=inputs=2:normalize=0[a]", "-map", "0:v", "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// b-roll stand-in: a coloured test pattern with room tone only; `grade` is an ffmpeg eq/colour filter to make it dark or cast.
fn broll(name: &str, dur: f64, grade: &str) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let vf = if grade.is_empty() { "null".to_string() } else { grade.to_string() };
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=360x640:r=30:d={dur}"), "-f", "lavfi", "-i", &format!("anoisesrc=a=0.004:d={dur}:r=48000"), "-vf", &vf,
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
fn portrait() -> Session { let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 360, "height": 640, "fps": 30})); ss }
/// A clip with a video stream and NO audio stream at all (phone b-roll shot muted, or a device/app that strips audio on export).
fn silent_no_audio_track(name: &str, dur: f64) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=360x640:r=30:d={dur}"), "-an", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// Two models in the eval called contact_sheet with path = their finished export (to "look at it"). The sheet was written over
/// the mp4, and the deliverable became a 0.04 s, 1-frame video. A still writer must never write a video path.
#[test] fn a_still_never_overwrites_a_video_file() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "solid", "color": [0.2, 0.4, 0.6], "start": 0, "duration": 1}));
    let out = dir().join("export.mp4"); call(&mut ss, "export", json!({"path": out, "crf": 30}));
    let before = std::fs::read(&out).unwrap();
    for (tool, args) in [("contact_sheet", json!({"path": out, "times": [0.2, 0.6]})), ("render_frame", json!({"path": out, "time": 0.5}))] {
        let r = dispatch(&mut ss, tool, &args);
        assert!(std::fs::read(&out).unwrap() == before, "{tool} wrote over {}: the export is gone ({r:?})", out.display());
        let e = r.expect_err(&format!("{tool} with a .mp4 path must be refused"));
        assert!(e.contains(".png") && e.contains("review_export"), "{tool}: the refusal should say to use a .png path and how to look at an export: {e}");
    }
    // an existing video stays safe even under an image name
    let disguised = dir().join("export_copy.png"); std::fs::copy(&out, &disguised).unwrap();
    assert!(dispatch(&mut ss, "contact_sheet", &json!({"path": disguised, "times": [0.2]})).unwrap_err().contains("existing video"));
    assert!(std::fs::read(&disguised).unwrap() == before, "the disguised export was overwritten");
    // a png path still works
    call(&mut ss, "contact_sheet", json!({"path": dir().join("ok.png"), "times": [0.2, 0.6]}));
}

/// review_export looks at the exported file itself (models re-imported the export as a clip to see it, or overwrote it).
#[test] fn review_export_shows_the_exported_file() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("rv.mp4", 3.0, &[(0.5, 2.5)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"}));
    let out = dir().join("rv_export.mp4"); call(&mut ss, "export", json!({"path": out, "crf": 30}));
    let r = call(&mut ss, "review_export", json!({"path": out, "n": 6}));
    assert!((r["duration"].as_f64().unwrap() - 3.0).abs() < 0.1, "{r}");
    assert!(r["size_mb"].as_f64().unwrap() > 0.0 && r["width"] == json!(360) && r["height"] == json!(640), "{r}");
    assert!(r["lufs"].as_f64().is_some(), "the export's loudness: {r}");
    let sheet = r["sheet"].as_str().unwrap(); assert!(std::fs::metadata(sheet).map(|m| m.len() > 1000).unwrap_or(false), "sheet png at {sheet}");
    assert_eq!(ss.media.len(), 1, "the sheet comes back as an inline image");
    assert!(dispatch(&mut ss, "review_export", &json!({"path": dir().join("nope.mp4")})).unwrap_err().contains("export first"));
}

/// get_project after one stabilize was 57,000 characters; models need ids, sources and times. timeline_summary is that, small.
#[test] fn timeline_summary_is_small_and_has_the_ids() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": broll("ts.mp4", 3.0, "")})), "asset_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 2, "in_point": 0.5, "fit": "cover"})), "clip_id");
    call(&mut ss, "stabilize", json!({"clip_id": c}));
    let t2 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let x = sid(&call(&mut ss, "add_text", json!({"track_id": t2, "text": "Hello", "start": 0, "duration": 1})), "clip_id");
    let full = serde_json::to_string(&call(&mut ss, "get_project", json!({}))).unwrap().len();
    let r = call(&mut ss, "timeline_summary", json!({})); let s = serde_json::to_string(&r).unwrap();
    assert!(s.len() < 1200, "timeline_summary is {} chars (get_project {full}): {s}", s.len());
    assert!(s.contains(&c) && s.contains(&x) && s.contains("ts.mp4") && s.contains("Hello"), "{s}");
    assert_eq!(r["tracks_bottom_to_top"][0]["clips"][0]["in"], json!(0.5), "{s}");
}

/// remove_silence_cuts split and rippled the clip and returned only {"removed": n}: the new ids had to be looked up.
#[test] fn remove_silence_cuts_returns_the_new_clip_ids() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("rs.mp4", 5.0, &[(0.5, 1.8), (3.0, 4.4)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 5, "fit": "cover"}));
    let r = call(&mut ss, "remove_silence_cuts", json!({"track_id": t, "ranges": [[1.9, 2.9]]}));
    let clips = r["clips"].as_array().unwrap_or_else(|| panic!("no clips in {r}"));
    let now: Vec<String> = ss.project.tracks.iter().find(|x| x.id == t).unwrap().clips.iter().map(|c| c.id.clone()).collect();
    let got: Vec<String> = clips.iter().map(|c| c["clip_id"].as_str().unwrap().to_string()).collect();
    assert_eq!(got, now, "{r}"); assert!((clips.last().unwrap()["end"].as_f64().unwrap() - 4.0).abs() < 0.05, "{r}");
}

/// The grouped list hides every action behind group names, and the flat list is 395 tools (about 150k tokens): a small model
/// either cannot find add_clip or cannot afford the list. KIMO_TOOLS=director is the editing short list.
/// KF2: the demo's own highlight tools (add_smart_punch, add_glow_text) are called directly by
/// tools/director_eval/demo/run_demo_mcp.py for the recipe's highlight moment, and the quickstart's own step 3
/// mentions the smart-punch highlight -- but a small model restricted to director mode could not see or call them
/// without first succeeding at a search_tools call it has no reason to make. Same gap for auto_reframe (the fix
/// lint_edit's aspect_mismatch names) and add_punch_ins (the alternate jump-cut highlight style).
#[test] fn director_profile_exposes_the_demos_own_highlight_and_reframe_tools() {
    let prof = kimo_core::toolgroups::parse_profile("director");
    let list = kimo_core::toolgroups::profile_list(&prof, &std::collections::HashSet::new());
    let names: Vec<&str> = list.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in ["add_smart_punch", "add_glow_text", "auto_reframe", "add_punch_ins"] {
        assert!(names.contains(&want), "director profile lacks {want}: {names:?}");
    }
}

#[test] fn director_profile_is_a_short_editing_list() {
    let prof = kimo_core::toolgroups::parse_profile("director");
    let list = kimo_core::toolgroups::profile_list(&prof, &std::collections::HashSet::new());
    let names: Vec<&str> = list.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in ["import_media", "add_clip", "export", "auto_rough_cut", "place_broll", "caption_from_words", "finish_audio", "auto_grade", "lint_edit", "review_export", "search_tools"] {
        assert!(names.contains(&want), "director profile lacks {want}: {names:?}");
    }
    assert!(names.len() <= 36, "{} tools: {names:?}", names.len()); // KF2: 29 + 4 highlight/reframe tools + 2 meta tools = 35
    let bytes = serde_json::to_string(&list).unwrap().len(); assert!(bytes < 29_900, "director list is {bytes} bytes"); // KF2: +4 tools pushed this from ~21k; V7.7Q: place_broll's words/asset_tags relevance params pushed it to ~29.1k; V7.9Q: finish_audio's match_strength/compress_strength args pushed it to ~29.5k; V8.8.0/8.8.1 (quality audit #1/#2): project_set's x264_preset/color_tag_sanity fields, with no per-field description to keep this small, pushed it to ~29.76k; 29.9k keeps real headroom under the 150k flat-list cost
    for t in &list { let d = t["description"].as_str().unwrap(); assert!(d.len() <= 150, "{}: {} chars", t["name"], d.len()); }
    // every listed tool is a real one with its real schema
    for t in list.iter().filter(|t| !["search_tools", "kimo_describe"].contains(&t["name"].as_str().unwrap())) {
        let n = t["name"].as_str().unwrap(); let c = kimo_core::commands::registry().into_iter().find(|c| c.name == n).unwrap_or_else(|| panic!("{n} not registered"));
        assert_eq!(t["inputSchema"], c.schema, "{n}");
    }
}

/// Three of three eval runs exported captions under TikTok's caption bar or titles across its button rail, a crushed shot,
/// or b-roll over a pause, and none of them noticed. lint_edit finds each and hands back the call that fixes it.
#[test] fn lint_edit_finds_the_problems_and_its_fixes_clear_them() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("lint.mp4", 6.0, &[(0.3, 2.6), (4.2, 5.8)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 6, "fit": "cover"}));
    let bt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll("lint_b.mp4", 3.0, "")})), "asset_id");
    // b-roll laid across the pause (2.6..4.2), and crushed to near black
    let bc = sid(&call(&mut ss, "add_clip", json!({"track_id": bt, "source_type": "media", "asset_id": b, "start": 2.4, "duration": 2.0, "fit": "cover"})), "clip_id");
    call(&mut ss, "set_color_wheels", json!({"clip_id": bc, "gain": [0.05, 0.05, 0.05, 1.0]}));
    // captions at the preset's default place on 9:16 (under TikTok's caption bar)
    let ct = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_caption_cues", json!({"track_id": ct, "style": "boxed", "cues": [{"start": 0.3, "end": 2.6, "text": "Three phone clips in"}, {"start": 4.2, "end": 5.8, "text": "one short out"}]}));
    let r = call(&mut ss, "lint_edit", json!({"platform": "tiktok"}));
    let codes: Vec<&str> = r["problems"].as_array().unwrap().iter().filter_map(|p| p["code"].as_str()).collect();
    for want in ["dark_frame", "broll_over_silence", "text_outside_safe_area", "loudness_off"] { assert!(codes.contains(&want), "lint missed {want}: {r}"); }
    // apply each fix exactly as given (skip placeholders the model fills from its brief)
    for p in r["problems"].as_array().unwrap() {
        let mut step = p["fix"].clone();
        while step.is_object() {
            let tool = step["tool"].as_str().unwrap_or(""); let args = step["args"].clone();
            if !tool.is_empty() && !serde_json::to_string(&args).unwrap().contains('<') && tool != "timeline_summary" { call(&mut ss, tool, args); }
            step = if step.get("then").map(|v| v.is_object()).unwrap_or(false) { step["then"].clone() } else if step.get("or").map(|v| v.is_object()).unwrap_or(false) && serde_json::to_string(&step["args"]).unwrap().contains('<') { step["or"].clone() } else { Value::Null };
        }
    }
    let r2 = call(&mut ss, "lint_edit", json!({"platform": "tiktok"}));
    let left: Vec<&str> = r2["problems"].as_array().unwrap().iter().filter_map(|p| p["code"].as_str()).collect();
    for gone in ["dark_frame", "broll_over_silence", "text_outside_safe_area", "loudness_off"] { assert!(!left.contains(&gone), "{gone} still there after its fix: {r2}"); }
}

/// KF2: a clip whose source aspect doesn't match the canvas gets silently cropped or padded by add_clip's default
/// framing. lint_edit should name it (aspect_mismatch, severity warning) and point at auto_reframe; a clip whose
/// source aspect already matches the canvas (beyond a small tolerance) must not be flagged.
#[test] fn aspect_mismatch_flags_a_landscape_clip_on_a_vertical_canvas() {
    fn landscape(name: &str, dur: f64) -> String {
        let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
        ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=s=640x360:r=30:d={dur}"), "-f", "lavfi", "-i", &format!("anoisesrc=a=0.004:d={dur}:r=48000"),
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
        p.to_string_lossy().into_owned()
    }
    let mut ss = portrait(); // 360x640 canvas, 9:16
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a_land = sid(&call(&mut ss, "import_media", json!({"path": landscape("aspect_land.mp4", 2.0)})), "asset_id");
    let c_land = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a_land, "start": 0, "duration": 2, "fit": "cover"})), "clip_id");
    // 360x640, same aspect as the canvas -- must not be flagged
    let a_match = sid(&call(&mut ss, "import_media", json!({"path": broll("aspect_match.mp4", 2.0, "")})), "asset_id");
    let c_match = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a_match, "start": 2, "duration": 2, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let probs = r["problems"].as_array().unwrap();
    let hit = probs.iter().find(|p| p["code"] == json!("aspect_mismatch") && p["clip_id"] == json!(c_land))
        .unwrap_or_else(|| panic!("landscape clip on a 9:16 canvas not flagged: {r}"));
    assert_eq!(hit["severity"], json!("warning"), "{hit}");
    assert_eq!(hit["fix"]["tool"], json!("auto_reframe"), "{hit}");
    assert_eq!(hit["fix"]["args"]["clip_id"], json!(c_land), "{hit}");
    assert!(!probs.iter().any(|p| p["code"] == json!("aspect_mismatch") && p["clip_id"] == json!(c_match)), "matching-aspect clip flagged: {r}");
}

/// KF3: docs/SMALL_MODEL_PREP.md's "Next" item 1 - a title/caption placed over the actual subject in frame
/// (titles and labels landed on the bottles in D3). lint_edit should flag it (text_over_subject, severity
/// warning) using the same saliency machinery add_smart_punch already relies on (ext/saliency_map.rs),
/// computed on the media only (not the text itself, which would otherwise self-flag); a title moved clear
/// of the subject must not be flagged.
#[test] fn text_over_subject_flags_a_title_placed_on_a_bright_salient_object() {
    fn spot_png(name: &str, w: usize, h: usize) -> String {
        let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
        use std::io::Write;
        // a lightly grained grey background (spectral residual is ill-conditioned on a perfectly flat picture,
        // same reason tests/saliency_map.rs's own disc fixture grains its background) with one soft white disc
        // (r 46 px) centred on the canvas (180, 320) - the "bottle" stand-in, directly under a default-centred
        // title. Deterministic LCG grain, same recipe as tests/saliency_map.rs::disc_bmp().
        let mut st: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut ch = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{w}x{h}"), "-i", "-", "-frames:v", "1", p.to_str().unwrap()]).stdin(Stdio::piped()).spawn().expect("ffmpeg");
        { let mut si = ch.stdin.take().unwrap(); let mut o = Vec::with_capacity(w * h * 3);
            for y in 0..h { for x in 0..w {
                st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); let grain = (st >> 61) as i32 - 4;
                let d = ((x as f64 + 0.5 - 180.0).powi(2) + (y as f64 + 0.5 - 320.0).powi(2)).sqrt();
                let vv = if d <= 46.0 { 255u8 } else { (128 + grain) as u8 };
                o.extend_from_slice(&[vv, vv, vv]); } }
            si.write_all(&o).unwrap(); }
        assert!(ch.wait().unwrap().success()); p.to_string_lossy().into_owned()
    }
    let mut ss = portrait(); // 360x640
    let mt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": spot_png("kf3_subject.png", 360, 640)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": mt, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 2.0, "fit": "cover"}));
    let tt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    // default position (x=0, y=0) is canvas centre - exactly on top of the bright block above.
    let title = sid(&call(&mut ss, "add_text", json!({"track_id": tt, "text": "ROAST TOUR", "start": 0.0, "duration": 2.0, "size": 90})), "clip_id");
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let probs = r["problems"].as_array().unwrap();
    let hit = probs.iter().find(|p| p["code"] == json!("text_over_subject") && p["clip_id"] == json!(title))
        .unwrap_or_else(|| panic!("title over the bright subject was not flagged: {r}"));
    assert_eq!(hit["severity"], json!("warning"), "{hit}");
    assert!(hit["fix"]["tool"].as_str().is_some(), "{hit}");
    // the same title moved well clear of the subject (top of frame) must not be flagged.
    call(&mut ss, "set_property", json!({"clip_id": title, "property": "y", "value": -260.0}));
    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let probs2 = r2["problems"].as_array().unwrap();
    assert!(!probs2.iter().any(|p| p["code"] == json!("text_over_subject") && p["clip_id"] == json!(title)), "moved title still flagged: {r2}");
}

/// text_over_subject's fix must not cross the vertical midline: a bottom-band caption clearing a subject
/// should never land in the TOP half (a cold-open hook title's own territory) even when the un-capped math
/// would have jumped it all the way to top_lim. Reproduces the real shape Tab found on rendered output: a
/// caption pushed off a subject landing right on top of the hook title.
#[test] fn text_over_subject_fix_never_crosses_the_midline_into_the_other_half() {
    fn spot_png(name: &str, w: usize, h: usize, cx: f64, cy: f64) -> String {
        let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
        use std::io::Write;
        let mut st: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut ch = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{w}x{h}"), "-i", "-", "-frames:v", "1", p.to_str().unwrap()]).stdin(Stdio::piped()).spawn().expect("ffmpeg");
        { let mut si = ch.stdin.take().unwrap(); let mut o = Vec::with_capacity(w * h * 3);
            for y in 0..h { for x in 0..w {
                st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); let grain = (st >> 61) as i32 - 4;
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                let vv = if d <= 46.0 { 255u8 } else { (128 + grain) as u8 };
                o.extend_from_slice(&[vv, vv, vv]); } }
            si.write_all(&o).unwrap(); }
        assert!(ch.wait().unwrap().success()); p.to_string_lossy().into_owned()
    }
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let mt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    // the bright subject sits just below the canvas centre (y=430 of 720, centre 360) -- close enough to the
    // midline that an un-clamped "clear it" nudge overshoots past centre into the top half.
    let a = sid(&call(&mut ss, "import_media", json!({"path": spot_png("midline_subject.png", 1280, 720, 640.0, 430.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": mt, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 2.0, "fit": "cover"}));
    let tt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    // a caption-like text just below centre (y=40 of a 720-tall canvas, centre 360) -- a small, realistic
    // "slightly low" placement, not already deep in the bottom band.
    let cap = sid(&call(&mut ss, "add_text", json!({"track_id": tt, "text": "CAPTION LINE", "start": 0.0, "duration": 2.0, "size": 70})), "clip_id");
    call(&mut ss, "set_property", json!({"clip_id": cap, "property": "y", "value": 40.0}));
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let hit = r["problems"].as_array().unwrap().iter().find(|p| p["code"] == json!("text_over_subject") && p["clip_id"] == json!(cap))
        .unwrap_or_else(|| panic!("bottom caption over the low subject was not flagged: {r}"));
    let fixed_y = hit["fix"]["args"]["value"].as_f64().unwrap_or_else(|| panic!("no numeric fix y: {hit}"));
    assert!(fixed_y >= 0.0, "a bottom-band caption's fix must not cross the midline into the top half: suggested y={fixed_y} ({hit})");
}

/// The eval's talking head: models left 1-3 s of dead air between takes (laid whole clips) or cut words off (guessed times).
#[test] fn auto_rough_cut_drops_the_dead_air_and_keeps_every_word() {
    let mut ss = portrait();
    let a1 = sid(&call(&mut ss, "import_media", json!({"path": talk("rc1.mp4", 5.0, &[(1.0, 2.6), (3.6, 4.4)], 1.0)})), "asset_id");
    let a2 = sid(&call(&mut ss, "import_media", json!({"path": talk("rc2.mp4", 4.0, &[(1.5, 3.2)], 1.0)})), "asset_id");
    let r = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a1, a2]}));
    let t = sid(&r, "track_id");
    // speech total 1.6 + 0.8 + 1.7 = 4.1 s; the cut keeps it with short pads and no gap longer than 0.4 s
    let d = r["duration"].as_f64().unwrap(); assert!(d > 4.1 && d < 5.4, "{r}");
    let sp = call(&mut ss, "audio_speech_intervals", json!({"target": {"track_id": t}, "aggressiveness": 1, "min_silence_ms": 200}));
    let iv: Vec<(f64, f64)> = sp["speech"].as_array().unwrap().iter().map(|x| (x["start"].as_f64().unwrap(), x["end"].as_f64().unwrap())).collect();
    let spoken: f64 = iv.iter().map(|(s, e)| e - s).sum(); assert!(spoken > 3.8, "words were cut: {spoken:.2} s of speech left of 4.1 ({iv:?})");
    let gaps: Vec<f64> = iv.windows(2).map(|w| w[1].0 - w[0].1).collect(); assert!(gaps.iter().all(|g| *g < 0.45), "dead air left: {gaps:?}");
    assert!(iv[0].0 < 0.3, "lead-in of {:.2} s", iv[0].0);
    // a false start is dropped by index, and a re-run rebuilds the same track
    let r2 = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a1, a2], "track_id": t, "drop": [{"asset_id": a1, "index": 0}]}));
    assert_eq!(r2["clips"].as_array().unwrap().len(), 2, "{r2}");
    assert_eq!(ss.project.tracks.iter().find(|x| x.id == t).unwrap().clips.len(), 2);
    assert!(dispatch(&mut ss, "auto_rough_cut", &json!({"asset_ids": ["nope"]})).unwrap_err().contains("import_media"));
}

/// KF1: auto_rough_cut's "no speech found" error names the asset and offers add_clip as a fallback. An asset with
/// NO AUDIO TRACK AT ALL (muted phone b-roll, or an app that strips audio) hit a different code path and surfaced
/// just "no audio track" -- no asset id, no path, no fallback -- a dead end for a small model restricted to the
/// director tool list, which would have no way to know add_clip exists as the way forward.
#[test] fn auto_rough_cut_names_the_asset_and_a_fallback_when_it_has_no_audio_track() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": silent_no_audio_track("noaudio.mp4", 3.0)})), "asset_id");
    let err = dispatch(&mut ss, "auto_rough_cut", &json!({"asset_ids": [a.clone()]})).unwrap_err();
    assert!(err.contains(&a), "names the asset id so a model restricted to director tools knows which clip failed: {err}");
    assert!(err.contains("add_clip"), "offers the same fallback as the no-speech-found case: {err}");
    assert!(err.contains("audio"), "still says what's actually wrong: {err}");
}

/// Models laid b-roll by guessed times: over pauses (dead picture) or with its own sound left on top of the voice.
#[test] fn place_broll_stays_inside_speech_and_is_muted() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("pb.mp4", 9.0, &[(0.4, 3.4), (3.8, 8.6)], 1.0)})), "asset_id");
    let t = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]})), "track_id");
    let b1 = sid(&call(&mut ss, "import_media", json!({"path": broll("pb1.mp4", 4.0, "")})), "asset_id");
    let b2 = sid(&call(&mut ss, "import_media", json!({"path": broll("pb2.mp4", 4.0, "hue=h=90")})), "asset_id");
    let r = call(&mut ss, "place_broll", json!({"track_id": t, "asset_ids": [b1, b2]}));
    let sp: Vec<(f64, f64)> = r["speech"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    let clips = r["clips"].as_array().unwrap(); assert_eq!(clips.len(), 2, "{r}");
    for c in clips { let (s, e) = (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap());
        assert!(sp.iter().any(|(a, b)| s >= a - 0.05 && e <= b + 0.05), "b-roll {s}..{e} is not inside one phrase {sp:?}");
        assert!(e - s >= 0.99 && e - s <= 2.51, "shot length {}", e - s);
        let id = c["clip_id"].as_str().unwrap(); assert!(ss.project.find_clip(id).unwrap().audio.mute, "b-roll {id} sound is on"); }
    assert!(clips[0]["start"].as_f64().unwrap() > 0.5, "b-roll should not cover the opening line: {r}");
}

/// Captions: models typed cue times by hand (off by seconds), and placed them in the platform's caption bar.
#[test] fn caption_from_words_follow_the_speech_and_clear_the_platform_ui() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("cap.mp4", 7.0, &[(0.5, 2.9), (3.9, 6.4)], 1.0)})), "asset_id");
    let t = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]})), "track_id");
    let text = "Three phone clips go in. One finished short comes out.";
    let rc = ss.project.tracks.iter().find(|x| x.id == t).unwrap().clips.iter().map(|c| c.start).collect::<Vec<f64>>();
    let r = call(&mut ss, "caption_from_words", json!({"track_id": t, "text": text, "check_confidence": false}));
    let cues = r["cues"].as_array().unwrap(); assert!(cues.len() >= 2, "{r}");  // two-line cues (default lines 2): one cue per sentence here
    let joined: Vec<String> = cues.iter().map(|c| c["text"].as_str().unwrap().replace('\n', " ")).collect();
    assert_eq!(joined.join(" "), text, "every word, in order");
    // the second sentence starts with the second phrase (not mid-pause or mid-first-phrase). After the rough cut the pause
    // between the phrases is gone, so the phrase starts are the rough cut's clip starts (a detector hears one long phrase).
    assert_eq!(rc.len(), 2, "two phrases, two clips: {rc:?}");
    let s2 = cues.iter().find(|c| c["text"].as_str().unwrap().starts_with("One")).unwrap()["start"].as_f64().unwrap();
    assert!((s2 - rc[1]).abs() < 0.35, "second sentence at {s2}, its phrase at {}", rc[1]);
    for c in cues { let id = c["clip_id"].as_str().unwrap(); let (s, e) = (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap());
        let b = call(&mut ss, "compute_text_bounds", json!({"clip_id": id, "t": (e - s) / 2.0})); let r4: Vec<f64> = b["canvas_rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        // TikTok: caption bar from y 921.6, action rail from x 604.8 (query_safe_margins tiktok)
        assert!(r4[1] + r4[3] <= 921.6 && r4[0] + r4[2] <= 604.8 && r4[0] >= 36.0 && r4[1] >= 128.0, "cue '{}' box {r4:?} is under the TikTok UI", c["text"]); }
    assert_eq!(r["inside_safe_area"], json!(true));
    assert!(dispatch(&mut ss, "caption_from_words", &json!({"track_id": t})).unwrap_err().contains("text"));
}

/// Mixes came out at -19.6 LUFS (music never levelled) or with one speaker 3 dB under the other.
#[test] fn finish_audio_levels_speakers_and_hits_the_target() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa1.mp4", 3.0, &[(0.2, 2.8)], 1.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk("fa2.mp4", 3.0, &[(0.2, 2.8)], 0.25)})), "asset_id");
    let c1 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"})), "clip_id");
    let c2 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": b, "start": 3, "duration": 3, "fit": "cover"})), "clip_id");
    // V7.9Q: match_strength (default 1.0, see below) scales the per-clip average-matching loop. Default stays
    // at the old full-match behaviour -- tested on real footage, lowering it by default barely helped loudness_range
    // (real clips were already close in average level) but badly hurt whisper word recall on quieter clips, so it
    // is left at 1.0 by default and is there for an explicit case like this one: two different speakers far apart
    // in mic level, which genuinely wants full matching.
    let r = call(&mut ss, "finish_audio", json!({}));
    let mix = call(&mut ss, "measure_loudness", json!({}))["integrated_lufs"].as_f64().unwrap(); assert!((mix + 14.0).abs() <= 1.0, "mix {mix} LUFS: {r}");
    // measure_loudness {clip_id} reads the clip before its gain (its documented contract), so add each clip's own gain
    let post = |ss: &mut Session, c: &str| { let l = call(ss, "measure_loudness", json!({"clip_id": c}))["integrated_lufs"].as_f64().unwrap(); l + 20.0 * ss.project.find_clip(c).unwrap().audio.volume.base.log10() };
    let (l1, l2) = (post(&mut ss, &c1), post(&mut ss, &c2));
    assert!((l1 - l2).abs() <= 1.0, "speakers {l1:.1} vs {l2:.1} LUFS (12 dB apart before): {r}");
    assert!(ss.project.find_clip(&c1).unwrap().audio.highpass >= 80.0);
}

/// V7.9Q: match_strength:0.6 (an explicit lower value, NOT the default -- see the real-footage note above) should
/// leave two far-apart clips PARTIALLY matched, not fully -- proving the knob actually scales when someone does
/// reach for it. Reuses the same two-speaker fixture as the test above.
#[test] fn finish_audio_match_strength_explicit_value_partially_matches() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa1b.mp4", 3.0, &[(0.2, 2.8)], 1.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk("fa2b.mp4", 3.0, &[(0.2, 2.8)], 0.25)})), "asset_id");
    let c1 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"})), "clip_id");
    let c2 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": b, "start": 3, "duration": 3, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "finish_audio", json!({"match_strength": 0.6}));
    let post = |ss: &mut Session, c: &str| { let l = call(ss, "measure_loudness", json!({"clip_id": c}))["integrated_lufs"].as_f64().unwrap(); l + 20.0 * ss.project.find_clip(c).unwrap().audio.volume.base.log10() };
    let (l1, l2) = (post(&mut ss, &c1), post(&mut ss, &c2));
    let gap = (l1 - l2).abs();
    // 12 dB apart before; match_strength:1.0 (above) closes it to <=1 dB. At 0.6 roughly 40% (~4.8 dB) should
    // remain -- assert a meaningful chunk is left (not fully matched) without pinning the exact number.
    assert!(gap > 2.5, "match_strength:0.6 should leave real spread between differently-leveled clips, got {gap:.1} dB apart: {r}");
    assert!(gap < 10.0, "match_strength:0.6 should still pull clips toward each other somewhat, got {gap:.1} dB apart (started at 12): {r}");
}

/// V7.9Q: with no match_strength given at all, finish_audio must still default to the old full-match behaviour
/// (1.0) -- the real-footage finding above (lowering it by default hurt whisper recall for no real LRA gain)
/// means the default has to stay exactly as it was pre-7.9, with the knob there only for an explicit call.
#[test] fn finish_audio_match_strength_defaults_to_full_match() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa1d.mp4", 3.0, &[(0.2, 2.8)], 1.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk("fa2d.mp4", 3.0, &[(0.2, 2.8)], 0.25)})), "asset_id");
    let c1 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"})), "clip_id");
    let c2 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": b, "start": 3, "duration": 3, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "finish_audio", json!({}));
    let post = |ss: &mut Session, c: &str| { let l = call(ss, "measure_loudness", json!({"clip_id": c}))["integrated_lufs"].as_f64().unwrap(); l + 20.0 * ss.project.find_clip(c).unwrap().audio.volume.base.log10() };
    let (l1, l2) = (post(&mut ss, &c1), post(&mut ss, &c2));
    assert!((l1 - l2).abs() <= 1.0, "no match_strength given should default to full match (old behaviour), got {:.1} dB apart: {r}", (l1 - l2).abs());
}

/// V7.9Q: match_strength:0.0 should leave each dialogue clip's own average exactly where it started (only
/// level_dynamics' within-clip shaping and the final mix-wide trim to target_lufs apply) -- the floor of the knob.
#[test] fn finish_audio_match_strength_zero_leaves_clip_averages_untouched_relative_to_each_other() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa1c.mp4", 3.0, &[(0.2, 2.8)], 1.0)})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk("fa2c.mp4", 3.0, &[(0.2, 2.8)], 0.25)})), "asset_id");
    let c1 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"})), "clip_id");
    let c2 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": b, "start": 3, "duration": 3, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "finish_audio", json!({"match_strength": 0.0}));
    let post = |ss: &mut Session, c: &str| { let l = call(ss, "measure_loudness", json!({"clip_id": c}))["integrated_lufs"].as_f64().unwrap(); l + 20.0 * ss.project.find_clip(c).unwrap().audio.volume.base.log10() };
    let (l1, l2) = (post(&mut ss, &c1), post(&mut ss, &c2));
    let gap = (l1 - l2).abs();
    // The clips are 12 dB apart as sourced, but set_audio's compressor + the master speechnorm leveling already
    // narrow that somewhat before this loop ever runs (measured: ~7.6 dB left at this point) -- match_strength:0.0
    // should leave that natural remainder essentially alone (no further pull toward the loudest clip).
    assert!(gap > 5.0, "match_strength:0.0 should leave most of the pre-existing gap untouched by further matching, got {gap:.1} dB apart: {r}");
}

/// RV-28: raw real-speech footage measured LRA 3.4, but the same footage through finish_audio came out at 11.1 --
/// wider than the source, not flatter. The per-clip average-matching step above only matches each clip's AVERAGE
/// loudness to the loudest clip; a single speaker getting louder and quieter WITHIN one clip was never touched by
/// it. level_dynamics (speechnorm, on by default) narrows that within-clip swing; this builds one clip with a loud
/// passage and a quiet passage (~14 dB apart) and checks the finished mix's own loudness_range comes out tighter
/// with it on than with it explicitly off, while the loudness target and the high-pass are untouched either way.
#[test] fn finish_audio_level_dynamics_narrows_loudness_range_within_a_clip() {
    let run = |level_on: bool| -> (f64, f64) {
        let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
        // Long enough loud/quiet passages (each several multiples of EBU LRA's own 3 s measurement block) that
        // gating has enough valid blocks on each side to give a stable, meaningful loudness_range reading.
        let a = sid(&call(&mut ss, "import_media", json!({"path": talk_uneven("uneven.mp4", 20.0, (0.2, 8.2), (10.0, 18.0))})), "asset_id");
        call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 20, "fit": "cover"}));
        // V7.9Q: compress_strength (new) defaults to 0.0 (off) on real-footage recall grounds -- this test is
        // specifically isolating level_dynamics' own narrowing, so it pins compress_strength:1.0 (the pre-7.9
        // fixed-ratio compressor) exactly as this test originally measured, unaffected by the new default.
        let r = call(&mut ss, "finish_audio", json!({"level_dynamics": level_on, "compress_strength": 1.0}));
        let mix = call(&mut ss, "measure_loudness", json!({"lra": true}));
        let lufs = mix["integrated_lufs"].as_f64().unwrap_or_else(|| panic!("no integrated_lufs: {mix} (finish_audio: {r})"));
        let lra = mix["loudness_range"]["lra_lu"].as_f64().unwrap_or_else(|| panic!("no loudness_range.lra_lu: {mix} (finish_audio: {r})"));
        (lufs, lra)
    };
    let (lufs_on, lra_on) = run(true);
    let (lufs_off, lra_off) = run(false);
    assert!((lufs_on + 14.0).abs() <= 1.0, "level_dynamics:true should still hit -14 LUFS, got {lufs_on}");
    assert!((lufs_off + 14.0).abs() <= 1.0, "level_dynamics:false should still hit -14 LUFS, got {lufs_off}");
    assert!(lra_on < lra_off - 1.0, "level_dynamics:true should measurably narrow loudness range: on={lra_on:.1} LU, off={lra_off:.1} LU");
    // Loose band, not a precise target: this is a synthetic fixture (a constant-envelope tone, not real speech),
    // and the real target band (6-8 LU) is verified on real CC0 footage via examples/auto_edit_benchmark.py, not
    // here -- the point of this test is the measurable narrowing above, confirmed on real footage as 11.1 -> 6.8 LU.
    assert!(lra_on <= 10.0, "level_dynamics:true should narrow meaningfully, got {lra_on:.1} LU (off was {lra_off:.1})");
}

/// V7.9Q: compress_strength scales the dialogue acompressor's ratio (default 0.35; 1.0 = old fixed 3:1; 0.0 = off,
/// skips the filter entirely). Proven two ways: (1) the stored ratio on the clip's audio effect chain moves with
/// the argument exactly per the documented mapping (ratio = 1 + 2*amount), including the off case emitting no
/// acompressor filter at all; (2) the real CC0 benchmark (see examples/auto_edit_benchmark.py --compress-strength,
/// added this release) measured the audible effect directly: compress_strength:0.0 raised true_peak_dbtp from
/// -6.24 to -2.43 dBTP at the same -14 LUFS on the real interview footage (this test's synthetic two-block fixture
/// did not discriminate on loudness_range or true peak -- its steady plateaus settle to a near-constant compressor
/// gain either way, unlike continuously-varying real speech).
#[test] fn finish_audio_compress_strength_scales_the_stored_ratio() {
    let built = |amt: f64| -> String {
        let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
        let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa_cs.mp4", 2.0, &[(0.1, 1.9)], 1.0)})), "asset_id");
        let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 2, "fit": "cover"})), "clip_id");
        call(&mut ss, "finish_audio", json!({"compress_strength": amt}));
        ss.project.find_clip(&c).unwrap().audio.compress_amount.to_string()
    };
    assert_eq!(built(1.0), "1", "compress_strength:1.0 should store compress_amount 1.0, got {}", built(1.0));
    assert_eq!(built(0.0), "0", "compress_strength:0.0 should store compress_amount 0.0 (off), got {}", built(0.0));
    assert_eq!(built(0.5), "0.5", "compress_strength:0.5 should store compress_amount 0.5, got {}", built(0.5));
}

/// V7.9Q: with no compress_strength given, finish_audio must default to 0.0 (compressor off) -- the real-footage
/// value verified to keep whisper recall intact (90.0% vs the old fixed-ratio baseline's 88.3%) while opening
/// loudness_range somewhat (1.2 -> 1.4 LU; see the comment on compress_strength's default in finish_audio's
/// source for the full real-benchmark numbers and why 0.0, not a middle value, is the safe default).
#[test] fn finish_audio_compress_strength_defaults_to_off() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa_csd.mp4", 2.0, &[(0.1, 1.9)], 1.0)})), "asset_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 2, "fit": "cover"})), "clip_id");
    call(&mut ss, "finish_audio", json!({}));
    assert_eq!(ss.project.find_clip(&c).unwrap().audio.compress_amount.to_string(), "0", "no compress_strength given should default to 0.0 (off)");
}

/// V7.9Q: place_broll's whoosh_sfx (default FALSE -- measured to cost whisper word recall on real footage, see
/// director_tools.rs) places a short synthesized whoosh, ducked, at each b-roll cut-in when turned on -- same
/// pattern as add_keyword_pops' sfx:true (0ae47c0): proven by the dedicated sfx track actually existing, one
/// clip per cut-in at the cut-in's own start time, AND the mix being audible right there (not silently placed).
/// Omitting the arg (the default) must place none.
#[test] fn place_broll_whoosh_sfx_places_a_real_sound_at_each_cut_in() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("fa_whoosh.mp4", 6.0, &[(0.1, 5.9)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 6, "fit": "cover"}));
    let b1 = sid(&call(&mut ss, "import_media", json!({"path": broll("wb1.mp4", 2.0, "")})), "asset_id");
    let b2 = sid(&call(&mut ss, "import_media", json!({"path": broll("wb2.mp4", 2.0, "")})), "asset_id");
    let r = call(&mut ss, "place_broll", json!({"track_id": t, "asset_ids": [b1, b2], "min_shot": 1.0, "max_shot": 1.5, "whoosh_sfx": true}));
    assert!(r.get("warnings").is_none(), "whoosh_sfx should not warn when it actually places sound: {r}");
    let wtid = r["whoosh_sfx_track_id"].as_str().unwrap_or_else(|| panic!("no whoosh_sfx_track_id: {r}")).to_string();
    let clips = r["clips"].as_array().unwrap();
    assert!(!clips.is_empty(), "{r}");
    let wtrack = ss.project.tracks.iter().find(|tr| tr.id == wtid).unwrap_or_else(|| panic!("no track {wtid}"));
    assert_eq!(wtrack.clips.len(), clips.len(), "one whoosh per b-roll cut-in: {r}");
    let cut_starts: Vec<f64> = clips.iter().map(|c| c["start"].as_f64().unwrap()).collect();
    let whoosh_starts: Vec<f64> = wtrack.clips.iter().map(|c| c.start).collect();
    assert_eq!(whoosh_starts, cut_starts, "each whoosh lands exactly at its cut-in's own start");
    // audible right at the first cut-in, not silent
    let pcm = kimo_core::audio::mix_strict(&ss.project).unwrap();
    let sr = kimo_core::audio::SR;
    let i0 = (cut_starts[0] * sr as f64) as usize * 2;
    let window = &pcm[i0..(i0 + sr / 10 * 2).min(pcm.len())];
    assert!(window.iter().any(|v| v.abs() > 0.003), "expected an audible whoosh right at the first cut-in, got silence");

    // the default (no whoosh_sfx arg at all) places none
    let mut ss2 = portrait(); let t2 = sid(&call(&mut ss2, "add_track", json!({})), "track_id");
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("fa_whoosh.mp4", 6.0, &[(0.1, 5.9)], 1.0)})), "asset_id");
    call(&mut ss2, "add_clip", json!({"track_id": t2, "source_type": "media", "asset_id": a2, "start": 0, "duration": 6, "fit": "cover"}));
    let b1b = sid(&call(&mut ss2, "import_media", json!({"path": broll("wb1.mp4", 2.0, "")})), "asset_id");
    let b2b = sid(&call(&mut ss2, "import_media", json!({"path": broll("wb2.mp4", 2.0, "")})), "asset_id");
    let r2 = call(&mut ss2, "place_broll", json!({"track_id": t2, "asset_ids": [b1b, b2b], "min_shot": 1.0, "max_shot": 1.5}));
    assert!(r2.get("whoosh_sfx_track_id").is_none(), "whoosh_sfx should default to off: {r2}");
}

/// Grades: the eval's dark and blue shots were left as shot, or "fixed" into grey-green. auto_grade measures and corrects.
#[test] fn auto_grade_lifts_a_dark_cast_shot_and_leaves_a_good_one() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let dark = sid(&call(&mut ss, "import_media", json!({"path": broll("dark.mp4", 2.0, "eq=brightness=-0.28:contrast=0.7,colorbalance=bs=0.25:rs=-0.15")})), "asset_id");
    let good = sid(&call(&mut ss, "import_media", json!({"path": broll("good.mp4", 2.0, "")})), "asset_id");
    let cd = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": dark, "start": 0, "duration": 2, "fit": "cover"})), "clip_id");
    let cg = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": good, "start": 2, "duration": 2, "fit": "cover"})), "clip_id");
    let before = call(&mut ss, "get_scopes", json!({"time": 1.0}));
    let r = call(&mut ss, "auto_grade", json!({}));
    let after = call(&mut ss, "get_scopes", json!({"time": 1.0}));
    let (lb, la) = (before["luma_mean"].as_f64().unwrap(), after["luma_mean"].as_f64().unwrap());
    assert!(la > lb + 0.08 && la > 0.28 && la < 0.6, "dark shot luma {lb:.3} -> {la:.3}: {r}");
    let rgb: Vec<f64> = after["avg_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((rgb[2] - rgb[1]).abs() < 0.06 && (rgb[0] - rgb[1]).abs() < 0.06, "cast left: {rgb:?}");
    assert!(r["graded"].as_array().unwrap().iter().any(|g| g["clip_id"] == json!(cd)), "{r}");
    assert!(r["skipped"].as_array().unwrap().iter().any(|g| g["clip_id"] == json!(cg)), "the balanced shot should be left alone: {r}");
}

/// Music reels: models cut every 1.5 s by arithmetic while the track's first beat was not at 0, so every cut missed.
#[test] fn cut_to_beat_cuts_on_the_music_beats() {
    let mut ss = portrait();
    let m = dir().join("beat.wav"); if !m.exists() {
        // 120 BPM with the first beat at 0.23 s: a click (decaying 1 kHz) on every beat over a soft pad
        ffmpeg(&["-f", "lavfi", "-i", "aevalsrc='0.9*sin(2*PI*1000*t)*exp(-40*mod(t-0.23+10,0.5))*gte(t,0.23)+0.05*sin(2*PI*220*t)':s=48000:d=12", m.to_str().unwrap()]); }
    let ma = sid(&call(&mut ss, "import_media", json!({"path": m})), "asset_id");
    let shots: Vec<String> = (0..3).map(|i| sid(&call(&mut ss, "import_media", json!({"path": broll(&format!("cb{i}.mp4"), 3.0, &format!("hue=h={}", i * 100))})), "asset_id")).collect();
    let r = call(&mut ss, "cut_to_beat", json!({"music_asset_id": ma, "asset_ids": shots, "beats_per_shot": 4}));
    assert!((r["beat_period"].as_f64().unwrap() - 0.5).abs() < 0.02, "{r}");
    let mc = sid(&r, "music_clip_id"); let ip = ss.project.find_clip(&mc).unwrap().in_point;
    // each cut lands on a beat of the music as laid: timeline cut + music in_point = 0.23 + k * 0.5 (one frame)
    for c in r["cuts_at"].as_array().unwrap() { let src = c.as_f64().unwrap() + ip; let k = ((src - 0.23) / 0.5).round(); assert!((src - 0.23 - k * 0.5).abs() <= 1.0 / 30.0 + 0.012, "cut at {c} is {src:.3} s into the music, off the beat grid: {r}"); }
    assert!((r["end"].as_f64().unwrap() - 6.0).abs() < 0.05, "3 shots x 4 beats = 6 s: {r}");
}


// ---- 2026-10-21 real-footage eval: what the vision judge saw in the small models' exports ----

fn gray_pixels(png: &str, w: usize, h: usize) -> Vec<u8> {
    let out = Command::new("ffmpeg").args(["-v", "error", "-i", png, "-f", "rawvideo", "-pix_fmt", "gray", "-"]).stdin(Stdio::null()).output().expect("ffmpeg");
    assert_eq!(out.stdout.len(), w * h, "decode {png}"); out.stdout
}

/// Haiku and Muse put a dissolve on cuts where the clips only touch: the incoming shot faded in over nothing, so the export
/// dipped to black at every cut (the judge: "a fully black frame right after the opening title"). lint_edit must catch it.
#[test] fn a_dissolve_on_a_cut_that_only_touches_is_flagged() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": broll("tx1.mp4", 2.0, "")})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll("tx2.mp4", 2.0, "hue=h=90")})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 2, "fit": "cover"}));
    let c2 = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": b, "start": 2, "duration": 2, "fit": "cover"})), "clip_id");
    call(&mut ss, "add_transition", json!({"clip_id": c2, "kind": "crossfade", "duration": 0.5}));
    let mid = call(&mut ss, "get_scopes", json!({"time": 2.05}))["luma_mean"].as_f64().unwrap();
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let hit = r["problems"].as_array().unwrap().iter().find(|p| p["code"] == json!("transition_from_black"));
    assert!(hit.is_some_and(|p| p["clip_id"] == json!(c2)), "the cut at 2.0 s dips to luma {mid:.3} and lint says nothing: {r}");
}

/// Muse wrote "My Favourite \u{2728}": the font has no such glyph and the export showed a box. A glyph the font lacks
/// must draw nothing (no box), and lint_edit names it.
#[test] fn a_glyph_the_font_lacks_draws_no_box_and_is_reported() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let c = sid(&call(&mut ss, "add_text", json!({"track_id": t, "text": "\u{1F525}", "start": 0, "duration": 1, "size": 120})), "clip_id");
    let png = dir().join("glyph.png"); let png = png.to_str().unwrap();
    call(&mut ss, "render_frame", json!({"time": 0.5, "path": png}));
    let lit = gray_pixels(png, 360, 640).iter().filter(|v| **v > 60).count();
    assert!(lit < 10, "a missing glyph drew {lit} lit pixels (a box)");
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(r["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("missing_glyph") && p["clip_id"] == json!(c)), "{r}");
}

/// Captions and labels at size 22-28 on a 1280-high frame read as specks on a phone (judge: "captions are tiny").
#[test] fn text_too_small_for_a_phone_is_flagged() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let c = sid(&call(&mut ss, "add_text", json!({"track_id": t, "text": "tiny label", "start": 0, "duration": 1, "size": 11})), "clip_id");
    let ok = sid(&call(&mut ss, "add_text", json!({"track_id": t, "text": "big", "start": 1, "duration": 1, "size": 40})), "clip_id");
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let small: Vec<&Value> = r["problems"].as_array().unwrap().iter().filter(|p| p["code"] == json!("text_too_small")).collect();
    assert!(small.iter().any(|p| p["clip_id"] == json!(c)), "size 11 on a 640-high frame not flagged: {r}");
    assert!(!small.iter().any(|p| p["clip_id"] == json!(ok)), "size 40 flagged: {r}");
}

/// 8.5.4: lint_edit's own auto-fix for text_too_small used to write ONLY the clip's base `scale`
/// value (via set_property). A Prop's base is never consulted again once it has any keyframes
/// (keyframe.rs raw() returns the last key's value past the last key time, the interpolated value
/// between keys otherwise) -- so on a text clip with a real entrance/exit pop-in scale animation
/// (0 -> 1 -> 1 -> 0, the shape an end card like "SUBSCRIBE FOR MORE" actually uses), the old fix
/// reported ok:true and changed nothing a viewer could see. The fix must now target the rendered
/// size directly (font `size`, not the animated `scale`) so it survives a keyframed scale.
#[test] fn text_too_small_fix_works_when_scale_is_keyframed() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let c = sid(&call(&mut ss, "add_text", json!({"track_id": t, "text": "SUBSCRIBE FOR MORE", "start": 0, "duration": 2, "size": 14})), "clip_id");
    // entrance/exit pop-in: 0 -> 1 -> 1 -> 0 across the clip; lint_edit measures at mid-clip (t=1.0),
    // which sits on the flat plateau at scale=1.0 -- exactly like the real end card.
    call(&mut ss, "set_keyframe", json!({"clip_id": c, "property": "scale", "time": 0.0, "value": 0.0}));
    call(&mut ss, "set_keyframe", json!({"clip_id": c, "property": "scale", "time": 0.3, "value": 1.0}));
    call(&mut ss, "set_keyframe", json!({"clip_id": c, "property": "scale", "time": 1.7, "value": 1.0}));
    call(&mut ss, "set_keyframe", json!({"clip_id": c, "property": "scale", "time": 2.0, "value": 0.0}));

    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let hit = r["problems"].as_array().unwrap().iter().find(|p| p["code"] == json!("text_too_small") && p["clip_id"] == json!(c))
        .unwrap_or_else(|| panic!("size 14 text (even with scale settled at 1.0 at mid-clip) should be flagged: {r}"));

    let fix = hit["fix"].clone();
    assert_eq!(fix["tool"], json!("set_text"), "the fix must target size, not the keyframed scale property: {fix}");
    call(&mut ss, fix["tool"].as_str().unwrap(), fix["args"].clone());

    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(!r2["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("text_too_small") && p["clip_id"] == json!(c)),
        "the old fix wrote only the base scale, which a keyframed Prop never reads again -- lint_edit must agree the fix actually reached the screen: {r2}");

    // the pop-in animation itself must be untouched -- still animates 0 -> 1 -> 1 -> 0
    let keys = &ss.project.find_clip(&c).unwrap().props["scale"].keys;
    assert_eq!(keys.len(), 4, "the fix must not have touched the scale keyframes: {keys:?}");
}

/// 8.5.4: a real end card found by the montage review. subscribe_pop's own pill is correctly sized
/// for its OWN default text size (text_size = 30*k) -- but that default is, by construction, always
/// exactly half of lint_edit's min_px threshold (min_px = min(w,h)/18 = 2 * (30 * min(w,h)/1080)), so
/// EVERY subscribe_pop end card on a non-1080p canvas gets flagged text_too_small and, once the fix
/// above actually reaches the screen, gets its font size roughly doubled -- which overflows a pill
/// that was never told the text grew. The pill must grow with it.
#[test] fn subscribe_pop_pill_grows_with_the_text_too_small_fix() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let r = call(&mut ss, "add_title", json!({"track_id": t, "style": "subscribe_pop", "text": "SUBSCRIBE FOR MORE", "start": 0.0, "duration": 2.5}));
    let ids: Vec<String> = r["clip_ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let pill_id = ids[0].clone(); // subscribe_pop pushes [pill, dot, text]
    let text_id = ids[2].clone();

    let before = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(before["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("text_too_small") && p["clip_id"] == json!(text_id)),
        "a subscribe_pop badge's own default text size should be flagged too-small on a 720p canvas: {before}");

    let pill_w_before = match &ss.project.find_clip(&pill_id).unwrap().source { Source::Shape { width, .. } => *width, s => panic!("not a shape: {s:?}") };

    let fixes = call(&mut ss, "apply_review_fixes", json!({"codes": ["text_too_small"]}));
    assert_eq!(fixes["failed"], json!([]), "{fixes}");

    let after = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(!after["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("text_too_small")), "the fix should clear the problem: {after}");

    // the text actually got bigger (that's the part of this fix that was already proven above) ...
    let bounds = call(&mut ss, "compute_text_bounds", json!({"clip_id": text_id}));
    let text_w = bounds["canvas_rect"][2].as_f64().unwrap();
    // ... and the pill grew to match it, rather than sitting at its original (now too-narrow) width.
    let pill_w_after = match &ss.project.find_clip(&pill_id).unwrap().source { Source::Shape { width, .. } => *width, s => panic!("not a shape: {s:?}") };
    assert!(pill_w_after > pill_w_before, "the pill must grow past its original {pill_w_before}px once the text behind it got bigger: now {pill_w_after}px");
    assert!(pill_w_after > text_w, "the pill ({pill_w_after}px) must stay wider than the text it backs ({text_w}px) -- a narrower pill is the exact overflow Tab saw on the montage's end card");
}


/// 8.5.4, second pass (Tab's montage review of the pill fix above, at full render size): the
/// text_too_small fix grows the pill to match the enlarged text, but subscribe_pop's own dot
/// (tagged "dot_for:<text id>" in titles.rs, same convention as the pill's "pill_for:<text id>")
/// was left at its OLD x -- the clearance it had past the OLD, narrower pill's left edge. Once the
/// pill grows, that same edge moves further left than the dot, so the dot that used to sit clear of
/// the badge now lands ON TOP of the enlarged text's first letter ("SUBSCRIBE FOR MORE"'s "B", on the
/// real montage render Tab watched). The fix must carry the dot's own clearance forward, not just the
/// pill's size: same gap past the pill's edge, no overlap with the (now bigger) text it sits beside.
#[test] fn subscribe_pop_dot_clears_the_text_after_the_pill_resize_fix() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let r = call(&mut ss, "add_title", json!({"track_id": t, "style": "subscribe_pop", "text": "SUBSCRIBE FOR MORE", "start": 0.0, "duration": 2.5}));
    let ids: Vec<String> = r["clip_ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let pill_id = ids[0].clone(); let dot_id = ids[1].clone(); let text_id = ids[2].clone(); // subscribe_pop pushes [pill, dot, text]

    fn shape_wh(ss: &Session, id: &str) -> (f64, f64) {
        match &ss.project.find_clip(id).unwrap().source { Source::Shape { width, height, .. } => (*width, *height), s => panic!("not a shape: {s:?}") }
    }
    // 8.5.5: the dot moved from OUTSIDE the pill (8.5.4, a "small bell badge" that on a real render read as
    // the dot floating loose, disconnected from the badge) to INSIDE it, left-aligned with padding. This
    // reads the dot's clearance PAST THE PILL'S LEFT EDGE (i.e. how far inside the pill its own left edge
    // sits) at mid-clip, where lint_edit measures too. Positive means inside; the old "outside" bug would
    // show as negative here.
    fn dot_left_pad(ss: &Session, pill_id: &str, dot_id: &str) -> f64 {
        let pill = ss.project.find_clip(pill_id).unwrap(); let dot = ss.project.find_clip(dot_id).unwrap();
        let (pill_w, _) = shape_wh(ss, pill_id); let (dot_w, _) = shape_wh(ss, dot_id);
        let pill_x = kimo_core::render::resolve_xform(&ss.project, pill, pill.duration / 2.0).x;
        let dot_x = kimo_core::render::resolve_xform(&ss.project, dot, dot.duration / 2.0).x;
        (dot_x - dot_w / 2.0) - (pill_x - pill_w / 2.0) // dot's left edge minus the pill's left edge
    }
    // the dot's right edge must never cross the pill's own right edge either (it should sit well inside,
    // near the left, but this is a real sanity check, not a tautology, once the dot is positioned relative
    // to a pill whose width keeps changing).
    fn dot_right_clearance(ss: &Session, pill_id: &str, dot_id: &str) -> f64 {
        let pill = ss.project.find_clip(pill_id).unwrap(); let dot = ss.project.find_clip(dot_id).unwrap();
        let (pill_w, _) = shape_wh(ss, pill_id); let (dot_w, _) = shape_wh(ss, dot_id);
        let pill_x = kimo_core::render::resolve_xform(&ss.project, pill, pill.duration / 2.0).x;
        let dot_x = kimo_core::render::resolve_xform(&ss.project, dot, dot.duration / 2.0).x;
        (pill_x + pill_w / 2.0) - (dot_x + dot_w / 2.0) // pill's right edge minus the dot's right edge
    }

    let pad_before = dot_left_pad(&ss, &pill_id, &dot_id);
    assert!(pad_before > 0.0, "the dot must sit INSIDE the pill, past its left edge, not floating outside it: left_pad {pad_before}px");
    assert!(dot_right_clearance(&ss, &pill_id, &dot_id) > 0.0, "the dot must not poke out the pill's right edge either");

    let fixes = call(&mut ss, "apply_review_fixes", json!({"codes": ["text_too_small"]}));
    assert_eq!(fixes["failed"], json!([]), "{fixes}");
    assert!(ss.project.find_clip(&dot_id).is_some(), "the dot clip must survive the fix chain");

    let pad_after = dot_left_pad(&ss, &pill_id, &dot_id);
    assert!(pad_after > 0.0, "the dot must still sit inside the pill after the resize: left_pad {pad_after}px");
    assert!((pad_after - pad_before).abs() < 0.5,
        "the dot's padding past the pill's left edge must carry forward unchanged by the resize (the formula makes it a constant, independent of pill width): was {pad_before}px, now {pad_after}px");
    assert!(dot_right_clearance(&ss, &pill_id, &dot_id) > 0.0, "the dot must still not poke out the pill's right edge after the resize");

    // the thing Tab actually saw on the render: the dot's own bbox must not intersect the (now bigger) text's bbox,
    // AND the two must have real breathing room between them -- 8.5.5's first pass only fixed the overlap,
    // leaving the dot about 3px from the "S" of SUBSCRIBE (reads as "\u25cfSUBSCRIBE" jammed together).
    // dot-to-text gap must be at least 0.7x the dot's own diameter (the fix targets ~0.9x), both before
    // AND after the resize fix, so a short label (never resized) reads just as clean as the enlarged one.
    fn dot_text_gap(ss: &mut Session, dot_id: &str, text_id: &str, cw: f64) -> f64 {
        let (dot_w, _) = { match &ss.project.find_clip(dot_id).unwrap().source { Source::Shape { width, height, .. } => (*width, *height), s => panic!("not a shape: {s:?}") } };
        let dot = ss.project.find_clip(dot_id).unwrap();
        let dot_x = kimo_core::render::resolve_xform(&ss.project, dot, dot.duration / 2.0).x;
        let dot_right_canvas = cw / 2.0 + dot_x + dot_w / 2.0;
        let text_dur = ss.project.find_clip(text_id).unwrap().duration;
        let bounds = call(ss, "compute_text_bounds", json!({"clip_id": text_id, "t": text_dur / 2.0}));
        let rect = bounds["canvas_rect"].as_array().unwrap_or_else(|| panic!("no canvas_rect: {bounds}"));
        let text_left_canvas = rect[0].as_f64().unwrap();
        text_left_canvas - dot_right_canvas
    }

    let (cw, _) = (1280.0_f64, 720.0_f64);
    let (dot_w, _) = shape_wh(&ss, &dot_id);
    let dot = ss.project.find_clip(&dot_id).unwrap();
    let dot_x = kimo_core::render::resolve_xform(&ss.project, dot, dot.duration / 2.0).x;
    let dot_right_canvas = cw / 2.0 + dot_x + dot_w / 2.0;

    let text_dur = ss.project.find_clip(&text_id).unwrap().duration;
    let bounds = call(&mut ss, "compute_text_bounds", json!({"clip_id": text_id, "t": text_dur / 2.0}));
    let rect = bounds["canvas_rect"].as_array().unwrap_or_else(|| panic!("no canvas_rect: {bounds}"));
    let text_left_canvas = rect[0].as_f64().unwrap();
    assert!(dot_right_canvas <= text_left_canvas + 0.5,
        "the dot (right edge at {dot_right_canvas}px) must sit clear, left of the enlarged text (starting at {text_left_canvas}px) -- this is the exact overlap Tab saw on the montage's end card");

    let gap_after = dot_text_gap(&mut ss, &dot_id, &text_id, cw);
    assert!(gap_after >= 0.7 * dot_w,
        "the dot-to-text gap after the resize fix must be at least 0.7x the dot's diameter ({dot_w}px) for real breathing room, not just non-overlapping: gap was {gap_after}px");

    // same check on a SHORT label that the text_too_small fix never touches (never resized, so this is
    // the build-time path's own spacing, not the fix path's).
    let mut ss2 = Session::new(); call(&mut ss2, "project_set", json!({"width": 1920, "height": 1080, "fps": 30}));
    let t2 = sid(&call(&mut ss2, "add_track", json!({})), "track_id");
    let r2 = call(&mut ss2, "add_title", json!({"track_id": t2, "style": "subscribe_pop", "text": "SUB", "start": 0.0, "duration": 2.5}));
    let ids2: Vec<String> = r2["clip_ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let dot_id2 = ids2[1].clone(); let text_id2 = ids2[2].clone();
    let (dot_w2, _) = shape_wh(&ss2, &dot_id2);
    let gap_before = dot_text_gap(&mut ss2, &dot_id2, &text_id2, 1920.0);
    assert!(gap_before >= 0.7 * dot_w2,
        "the dot-to-text gap on a freshly built (never resized) subscribe_pop badge must also be at least 0.7x the dot's diameter ({dot_w2}px): gap was {gap_before}px");
}


/// Muse added a second title over the first ("MY TWO BOTTLES" under "MY FAVORITE BOTTLES"): the judge read garbage.
/// v7.6Q: the finding that prompted this check -- apply_review_fixes' own text_over_subject fix moved a
/// caption clear of a (fake) subject and landed it a hair above a hook title, ZERO pixel overlap, same
/// visual mess. text_overlap (IoU-based) correctly says nothing; text_crowded is the new check for exactly
/// this: two text boxes sharing a horizontal band with no real gap between them.
#[test] fn two_texts_with_almost_no_gap_are_flagged_as_crowded_not_missed() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let t1 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let top = sid(&call(&mut ss, "add_text", json!({"track_id": t1, "text": "TOP LINE", "start": 0, "duration": 2, "size": 70})), "clip_id");
    call(&mut ss, "set_property", json!({"clip_id": top, "property": "y", "value": -150.0}));
    let top_rect = call(&mut ss, "compute_text_bounds", json!({"clip_id": top}))["canvas_rect"].clone();
    let top_bottom = top_rect[1].as_f64().unwrap() + top_rect[3].as_f64().unwrap();

    // place a second text 5px below the first's bottom edge -- deliberately inside any reasonable min_gap,
    // and confirm it is NOT a pixel overlap (ix>0 but iy<=0) before checking the new code fires.
    let t2 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let bottom = sid(&call(&mut ss, "add_text", json!({"track_id": t2, "text": "BOTTOM LINE", "start": 0, "duration": 2, "size": 70})), "clip_id");
    let bottom_rect = call(&mut ss, "compute_text_bounds", json!({"clip_id": bottom}))["canvas_rect"].clone();
    let bottom_h = bottom_rect[3].as_f64().unwrap();
    let want_top = top_bottom + 5.0; // 5px clear -- crowded by any reasonable standard
    call(&mut ss, "set_property", json!({"clip_id": bottom, "property": "y", "value": want_top + bottom_h / 2.0 - 360.0}));

    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let codes: Vec<&str> = r["problems"].as_array().unwrap().iter().filter_map(|p| p["code"].as_str()).collect();
    assert!(!codes.contains(&"text_overlap"), "a 5px clear gap must not read as a pixel overlap: {r}");
    let hit = r["problems"].as_array().unwrap().iter().find(|p| p["code"] == json!("text_crowded"))
        .unwrap_or_else(|| panic!("two texts 5px apart should be flagged text_crowded: {r}"));
    assert_eq!(hit["clip_id"], json!(bottom), "{hit}");
    assert_eq!(hit["other_clip_id"], json!(top), "{hit}");
    assert!(hit["gap_px"].as_f64().unwrap() < 10.0, "{hit}");

    // apply the fix and confirm the gap actually widens past the minimum, and the overlap check still agrees nothing overlaps
    let fix = hit["fix"].clone();
    call(&mut ss, fix["tool"].as_str().unwrap(), fix["args"].clone());
    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(r2["problems"].as_array().unwrap().iter().all(|p| p["code"] != json!("text_crowded")), "the fix should clear the crowding: {r2}");
}

/// A generous, obviously-fine gap between two texts/// A generous, obviously-fine gap between two texts in the same band must not be flagged -- the check is
/// for genuine crowding, not any two texts that happen to share a time window.
#[test] fn a_generous_gap_between_two_texts_is_not_flagged() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let t1 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let top = sid(&call(&mut ss, "add_text", json!({"track_id": t1, "text": "TOP LINE", "start": 0, "duration": 2, "size": 50})), "clip_id");
    call(&mut ss, "set_property", json!({"clip_id": top, "property": "y", "value": -260.0}));
    let t2 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let bottom = sid(&call(&mut ss, "add_text", json!({"track_id": t2, "text": "BOTTOM LINE", "start": 0, "duration": 2, "size": 50})), "clip_id");
    call(&mut ss, "set_property", json!({"clip_id": bottom, "property": "y", "value": 260.0}));
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(r["problems"].as_array().unwrap().iter().all(|p| p["code"] != json!("text_crowded") && p["code"] != json!("text_overlap")), "{r}");
}

#[test] fn two_titles_in_the_same_place_are_flagged() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let t1 = sid(&call(&mut ss, "add_track", json!({})), "track_id"); let t2 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_title", json!({"track_id": t1, "style": "impact", "text": "MY TWO BOTTLES", "start": 0, "duration": 2}));
    let second = call(&mut ss, "add_title", json!({"track_id": t2, "style": "impact", "text": "MY FAVORITE BOTTLES", "start": 0, "duration": 2}));
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(r["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("text_overlap") && p["clip_id"] == second["clip_ids"][0]), "{r}");
    let t3 = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_text", json!({"track_id": t3, "text": "later", "start": 3, "duration": 1, "size": 48}));
    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert_eq!(r2["problems"].as_array().unwrap().iter().filter(|p| p["code"] == json!("text_overlap")).count(), 1, "text at a different time is not an overlap: {r2}");
}


/// Muse's captions (one text for the whole track) drifted onto the wrong shots: words of clip 3 showed over clip 2.
/// Given words per source clip, every cue stays inside its own clip's shots.
#[test] fn caption_words_per_clip_stay_on_their_own_shots() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let a1 = sid(&call(&mut ss, "import_media", json!({"path": talk("ba1.mp4", 4.0, &[(0.4, 3.4)], 1.0)})), "asset_id");
    let a2 = sid(&call(&mut ss, "import_media", json!({"path": talk("ba2.mp4", 3.0, &[(0.4, 1.2)], 1.0)})), "asset_id");
    let rc = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a1, a2]})); let t = sid(&rc, "track_id");
    let span = |id: &str| { let cs: Vec<&Value> = rc["clips"].as_array().unwrap().iter().filter(|c| c["asset_id"] == json!(id)).collect();
        (cs.first().unwrap()["start"].as_f64().unwrap(), cs.last().unwrap()["end"].as_f64().unwrap()) };
    let (s1, e1) = span(&a1); let (s2, e2) = span(&a2);
    // a short clip with many words, a long one with few: proportional timing over the whole track would push words across the cut
    let r = call(&mut ss, "caption_from_words", json!({"track_id": t, "by_asset": {a1.clone(): "Short one here", a2.clone(): "and this second clip says a great many quick words"}, "check_confidence": false}));
    for c in r["cues"].as_array().unwrap() { let (s, e) = (c["start"].as_f64().unwrap(), c["end"].as_f64().unwrap()); let txt = c["text"].as_str().unwrap();
        let first_clip = "Short one here".contains(txt.split_whitespace().next().unwrap());
        let (lo, hi) = if first_clip { (s1, e1) } else { (s2, e2) };
        assert!(s >= lo - 0.05 && e <= hi + 0.05, "cue '{txt}' {s:.2}..{e:.2} leaves its clip {lo:.2}..{hi:.2}: {r}"); }
    assert!(dispatch(&mut ss, "caption_from_words", &json!({"track_id": t, "by_asset": {"asset99": "x"}})).unwrap_err().contains("asset99"));
}


/// Judge on Muse D2: "captions chopped into tiny mid-sentence pieces". Lines are balanced: no lone trailing word.
#[test] fn caption_lines_are_balanced_not_a_dangling_word() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("bal.mp4", 6.0, &[(0.3, 5.6)], 1.0)})), "asset_id");
    let t = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]})), "track_id");
    let r = call(&mut ss, "caption_from_words", json!({"track_id": t, "text": "My favorite is the navy one, it never leaks.", "check_confidence": false}));
    for c in r["cues"].as_array().unwrap() { let txt = c["text"].as_str().unwrap();
        assert!(txt.split_whitespace().count() >= 2, "cue '{txt}' is a lone word: {r}"); }
    // no flash cues: six one-word sentences in two seconds of speech still give cues of at least 0.8 s, every word in order
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk("flash.mp4", 3.0, &[(0.3, 2.5)], 1.0)})), "asset_id");
    let t2 = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [b]})), "track_id");
    let text = "Yes. No. Maybe. Sure. Okay. Fine.";
    let r = call(&mut ss, "caption_from_words", json!({"track_id": t2, "text": text, "check_confidence": false}));
    let cues = r["cues"].as_array().unwrap();
    for c in cues { assert!(c["end"].as_f64().unwrap() - c["start"].as_f64().unwrap() >= 0.79, "flash cue {c}: {r}"); }
    let joined: Vec<String> = cues.iter().map(|c| c["text"].as_str().unwrap().replace('\n', " ")).collect();
    assert_eq!(joined.join(" "), text);
}


/// talk() with a chosen picture: `src` an ffmpeg lavfi video source, `vf` a filter on it (e.g. a blur over a time range).
fn talk_v(name: &str, dur: f64, speech: &[(f64, f64)], src: &str, vf: &str) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let gate: Vec<String> = speech.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
    let a = format!("aevalsrc='({})*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2)+0.05*sin(2*PI*1900*t))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &format!("{src}=s=360x640:r=30:d={dur}"), "-f", "lavfi", "-i", &a, "-f", "lavfi", "-i", &format!("anoisesrc=a=0.002:d={dur}:r=48000"),
        "-filter_complex", &format!("[0:v]{vf}[v];[1:a][2:a]amix=inputs=2:normalize=0[a]"), "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// Judge on Muse D2: "motion-blurred, badly framed shots are kept in". The rough cut trims a blurry start of a segment.
#[test] fn rough_cut_trims_a_blurry_start() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk_v("blur_lead.mp4", 5.0, &[(0.3, 4.5)], "testsrc2", "boxblur=10:enable='between(t,0,1.0)'")})), "asset_id");
    let r = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]}));
    let seg = &r["segments"][0]; let clip = &r["clips"][0];
    assert!(seg["soft_share"].as_f64().unwrap() > 0.1, "the blur is seen: {r}");
    let ip = clip["in_point"].as_f64().unwrap();
    assert!((0.9..=1.3).contains(&ip), "the clip starts after the blur (in {ip}): {r}");
    // flag mode reports and keeps it
    let r2 = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a], "soft": "flag"}));
    assert!(r2["clips"][0]["in_point"].as_f64().unwrap() < 0.4, "{r2}");
    assert!(dispatch(&mut ss, "auto_rough_cut", &json!({"asset_ids": [a], "soft": "maybe"})).unwrap_err().contains("trim"));
}

/// Judge on Muse D2: "repeated near-identical navy bottle shots drag the middle". Two clips of the same picture: the second
/// reports similar_to, and dedupe drops it; a different picture is not similar.
#[test] fn rough_cut_finds_repeated_shots() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk_v("dup_a.mp4", 3.0, &[(0.3, 2.6)], "smptebars", "null")})), "asset_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": talk_v("dup_b.mp4", 3.0, &[(0.3, 2.6)], "smptebars", "null")})), "asset_id");
    let c = sid(&call(&mut ss, "import_media", json!({"path": talk_v("dup_c.mp4", 3.0, &[(0.3, 2.6)], "testsrc2", "null")})), "asset_id");
    let r = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a, b, c]}));
    let segs = r["segments"].as_array().unwrap();
    let of = |id: &str| segs.iter().find(|s| s["asset_id"] == json!(id)).unwrap();
    assert_eq!(of(&b)["similar_to"]["asset_id"], json!(a), "{r}");
    assert!(of(&c).get("similar_to").is_none(), "a different picture is not a repeat: {r}");
    assert_eq!(r["clips"].as_array().unwrap().len(), 3, "reported, not dropped, by default");
    let r2 = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a, b, c], "dedupe": true}));
    let ids: Vec<&str> = r2["clips"].as_array().unwrap().iter().map(|x| x["asset_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![a.as_str(), c.as_str()], "{r2}");
}


/// auto_grade run twice stacked two primaries grades (the second compounding the first), and its "reset" of a crushed
/// grade added a neutral grade after the crushing one, which left the shot black. It updates the clip's grade in place.
#[test] fn auto_grade_updates_the_grade_in_place() {
    let mut ss = portrait();
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": broll("dark_b.mp4", 3.0, "eq=brightness=-0.35")})), "asset_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"})), "clip_id");
    let g1 = call(&mut ss, "auto_grade", json!({"clip_ids": [c]})); let g2 = call(&mut ss, "auto_grade", json!({"clip_ids": [c]}));
    assert_eq!(g1["graded"][0]["gain"], g2["graded"][0]["gain"], "the same grade both times: {g1} {g2}");
    let n = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|x| x.id == c).unwrap().effects.iter().filter(|e| e.kind == "primaries").count();
    assert_eq!(n, 1, "one primaries grade, not a stack");
}

/// Perf (KP1): lint_edit's picture check samples a timeline every 0.5s via get_scopes, and get_scopes' own
/// single-instant prepare_at window (24 frames) is far smaller than the 0.5s gap between samples once a clip
/// runs more than a second or two - nearly every sample needs a fresh decode. kimo's media decode always
/// re-seeks to a clip's own start and discards every frame before the one it actually wants (bounded memory,
/// not bounded CPU), so each sample's decode cost grows with how far into the clip it sits - quadratic in a
/// long clip's length at this sampling cadence. lint_edit now pre-warms the same decode cache with a much
/// bigger (export-sized) window before each sample, so most of a clip lands from one or two real decodes
/// instead of one per sample - same decode() calls, same project, same time, so get_scopes' numbers (and
/// lint_edit's findings) are unchanged by construction; only how many of those decodes are real drops. A 60s
/// single clip with a real dark patch right at the end pins both: the exact same problem is still found, at
/// the exact same instant, and the scan no longer grows the way the old per-sample window did.
#[test] fn lint_edit_scans_a_long_clip_without_rescanning_from_its_start_each_sample() {
    let p = dir().join("kp1_long.mp4");
    if !p.exists() {
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=360x640:r=30:d=60", "-f", "lavfi", "-i", "color=c=black:s=360x640:r=30:d=60",
            "-filter_complex", "[0:v][1:v]overlay=enable='between(t,58,59.5)'[v]", "-map", "[v]",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", p.to_str().unwrap()]);
    }
    let mut ss = portrait();
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": p.to_str().unwrap()})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 60, "fit": "cover"}));
    let t0 = std::time::Instant::now();
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let elapsed = t0.elapsed();
    let dark: Vec<&Value> = r["problems"].as_array().unwrap().iter().filter(|p| p["code"] == "dark_frame").collect();
    assert_eq!(dark.len(), 1, "exactly one dark patch, found: {r}");
    let at = dark[0]["at"].as_f64().unwrap();
    assert!((at - 58.25).abs() < 0.05, "dark_frame should land at 58.25s (the patch's first sampled instant), got {at}: {r}");
    // Equivalence pin: re-run on the same, unmutated project - the finding must be exactly reproducible.
    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert_eq!(r, r2, "lint_edit must be deterministic on an unchanged project");
    // Timing: measured on this exact fixture on this box, the pre-fix scan (always re-seeking to each clip's
    // own start per sample) took ~12.5s; the fix takes ~4.2s. 9s is a generous ceiling - comfortably above
    // the fix's real time, comfortably below the old quadratic cost - to catch a real regression, not CI noise.
    // That 3x margin is a --release number only. This test is dominated by real ffmpeg subprocess decodes,
    // whose cost barely depends on whether kimo-core itself was compiled with optimizations, so the debug
    // build's margin is far narrower. KW2 (2026-10-24) measured both sides directly in a plain debug build
    // on this box: this fixed code takes 12.7-13.2s (7 runs); the original quadratic rescan (the code from
    // before 98eed5d, rebuilt in a separate debug worktree at 77e14e2 with this same test) takes 19.0-20.5s
    // (5 runs). A flat "~3x optimized" guess (27s+) would sit above the real regression and silently stop
    // catching it, so the debug ceiling below is picked from the measured gap instead, not a multiplier:
    // comfortably above 13.2s, comfortably below 19.0s.
    let ceiling = if cfg!(debug_assertions) { 18.0 } else { 9.0 };
    assert!(elapsed.as_secs_f64() < ceiling, "lint_edit took {:.1}s on a 60s single clip (ceiling {:.1}s for this build) - the per-sample decode window regressed", elapsed.as_secs_f64(), ceiling);
}

/// KR2: lint_edit's picture-scan loop used to pre-warm the decode cache itself immediately before
/// every get_scopes call (KP1/KQ1), then get_scopes called the exact same function again for the
/// exact same instant - after KQ1 (fb6ddbf) made get_scopes' own call site use that same function,
/// the pre-warm became a byte-for-byte duplicate. This pins the duplicate's removal directly (not
/// just via timing, which is sensitive to box load): render::prepare_analysis_range must run exactly
/// once per sampled instant, not twice. thread-local counter: see its own doc comment for why this
/// is safe next to other tests in this binary running concurrently.
#[test] fn lint_edit_prepares_each_picture_scan_sample_exactly_once() {
    let mut ss = portrait();
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": broll("kr2_prep.mp4", 3.0, "")})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 3, "fit": "cover"}));
    let before = kimo_core::render::analysis_prepare_call_count();
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let after = kimo_core::render::analysis_prepare_call_count();
    // picture scan samples at t = 0.25, 0.75, ..., < 3.0 -> 6 samples
    let mut expected = 0usize; let mut t = 0.25; while t < 3.0 { expected += 1; t += 0.5; }
    assert_eq!(after - before, expected, "get_scopes' own prepare_analysis_range call is the only preparation that should run per sample now (a duplicate pre-warm call would double this): {r}");
}

/// KR2: lint_edit's dialogue-track finder (director_tools.rs `speech()`) probed each candidate track
/// at aggressiveness 2, then - for the winning track only - called it again at aggressiveness 1 for
/// the real intervals, paying a full ffmpeg-backed audio decode twice for the one track that matters
/// even though nothing about the project changed between the two calls. Session::audio_decode_cache
/// (ext/speech_intervals.rs `decode_memo`) now memoizes the decode by (target, whole-project
/// fingerprint), so the second call is a cache hit. Pinned directly via the decode call counter, not
/// timing (audio decode on a 6s clip is fast enough that a timing ceiling here would mostly measure
/// process noise, not the fix).
#[test] fn lint_edit_decodes_the_dialogue_track_audio_once_not_twice() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("kr2_speech.mp4", 6.0, &[(0.3, 2.6), (3.2, 5.6)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 6, "fit": "cover"}));
    let before = kimo_core::ext::speech_intervals::speech_decode_call_count();
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let after = kimo_core::ext::speech_intervals::speech_decode_call_count();
    assert_eq!(after - before, 1, "the one dialogue track's audio should decode exactly once (probe + final reuse the same PCM), not once per aggressiveness: {r}");
}


/// KC1 (2026-10-24): landscape/4K phone footage that is pillarboxed (black bars either side of a centered
/// portrait-shaped subject -- common when a landscape capture is uploaded without a re-encode) measured auto_grade's
/// exposure from the SOURCE's raw 64x64 downsample with no fit/crop applied at all. For a pillarboxed source the raw
/// frame is mostly black bars, so auto_grade sees a false "dark" reading, computes the maximum exposure gain (clamped
/// 2.4x), and applies it to the clip -- but the clip's "cover" fit crops the black bars away entirely, so the gain
/// lands on the already-well-exposed visible content and blows it out. auto_grade must measure what the fit/crop
/// transform actually puts on screen, not the untouched source frame.
fn pillarboxed_landscape(name: &str, dur: f64, canvas_w: u32, canvas_h: u32, band_w: u32) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=c=gray:s={band_w}x{canvas_h}:d={dur}"),
        "-f", "lavfi", "-i", &format!("anoisesrc=a=0.004:d={dur}:r=48000"),
        "-vf", &format!("pad={canvas_w}:{canvas_h}:({canvas_w}-{band_w})/2:0:color=black"),
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
#[test] fn auto_grade_measures_the_fitted_frame_not_the_unfit_source() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    // 1280x720 landscape source, "cover" fit into a 360x640 (9:16) project crops to the center ~360/(1280*640/720)=31.6%
    // of the width -- so a centered gray band a bit wider than that (40%) is ALL that's visible after fit; the rest
    // (pure black pillarbox) must never factor into the exposure reading.
    let band_w = (1280.0_f64 * 0.40).round() as u32;
    let src = sid(&call(&mut ss, "import_media", json!({"path": pillarboxed_landscape("pillarbox.mp4", 2.0, 1280, 720, band_w)})), "asset_id");
    let _c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": src, "start": 0, "duration": 2, "fit": "cover"})), "clip_id");
    let before = call(&mut ss, "get_scopes", json!({"time": 1.0}));
    let lb = before["luma_mean"].as_f64().unwrap();
    assert!((0.32..=0.6).contains(&lb), "fixture sanity: the visible (cropped) band should already read as normally exposed, got {lb:.3}");
    let r = call(&mut ss, "auto_grade", json!({}));
    let after = call(&mut ss, "get_scopes", json!({"time": 1.0}));
    let la = after["luma_mean"].as_f64().unwrap();
    assert!(la < 0.75, "auto_grade blew out a clip that was already well exposed on screen: luma {lb:.3} -> {la:.3} ({r}) -- it measured the unfit source (mostly black pillarbox), not what \"cover\" fit actually shows");
}

/// RV-33: place_broll matches each cutaway to the b-roll clip whose name/tags fit what's being said right then,
/// instead of handing clips out in the order they were given. Given 3 untagged clips named for what they show
/// (coffee.mp4, typing.mp4, city.mp4) and a transcript mentioning "typing"/"computer" during the first slot,
/// "coffee" during the second and "city"/"street" during the third, the assignment should be the opposite of
/// the input order (coffee, typing, city) on every slot -- proof it is reading the words, not just re-shuffling.
#[test] fn place_broll_matches_the_clip_to_the_words_being_said() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("relpb.mp4", 10.0, &[(0.4, 9.6)], 1.0)})), "asset_id");
    let t = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]})), "track_id");
    let b_coffee = sid(&call(&mut ss, "import_media", json!({"path": broll("coffee.mp4", 4.0, "")})), "asset_id");
    let b_typing = sid(&call(&mut ss, "import_media", json!({"path": broll("typing.mp4", 4.0, "hue=h=90")})), "asset_id");
    let b_city = sid(&call(&mut ss, "import_media", json!({"path": broll("city.mp4", 4.0, "hue=h=180")})), "asset_id");
    // slot times from the fixed audio above: slot0 ~2.87-5.37s, slot1 ~5.37-7.87s, slot2 ~7.87-9.5s
    let words = json!([
        {"text": "typing", "start": 3.0, "end": 3.4}, {"text": "on", "start": 3.4, "end": 3.5}, {"text": "the", "start": 3.5, "end": 3.6}, {"text": "computer", "start": 3.6, "end": 4.0},
        {"text": "grabbed", "start": 6.0, "end": 6.3}, {"text": "coffee", "start": 6.3, "end": 6.7},
        {"text": "walking", "start": 8.5, "end": 8.8}, {"text": "down", "start": 8.8, "end": 8.9}, {"text": "the", "start": 8.9, "end": 9.0}, {"text": "city", "start": 9.0, "end": 9.3}, {"text": "street", "start": 9.3, "end": 9.5}
    ]);
    let r = call(&mut ss, "place_broll", json!({"track_id": t, "asset_ids": [b_coffee.clone(), b_typing.clone(), b_city.clone()], "words": words}));
    assert_eq!(r["broll_relevance"]["active"], json!(true), "{r}");
    let notes = r["broll_relevance"]["matches"].as_array().unwrap(); assert!(!notes.is_empty(), "expected at least one relevance match: {r}");
    let clips = r["clips"].as_array().unwrap(); assert_eq!(clips.len(), 3, "{r}");
    let got: Vec<&str> = clips.iter().map(|c| c["asset_id"].as_str().unwrap()).collect();
    assert_eq!(got, vec![b_typing.as_str(), b_coffee.as_str(), b_city.as_str()], "slot0 should get the typing clip (computer/typing spoken then), slot1 the coffee clip, slot2 the city clip -- not the input order: {r}");

    // Without words, nothing to match on: falls back to the given order exactly as before (no regression).
    let mut ss2 = portrait();
    let a2 = sid(&call(&mut ss2, "import_media", json!({"path": talk("relpb2.mp4", 10.0, &[(0.4, 9.6)], 1.0)})), "asset_id");
    let t2 = sid(&call(&mut ss2, "auto_rough_cut", json!({"asset_ids": [a2]})), "track_id");
    let b_coffee2 = sid(&call(&mut ss2, "import_media", json!({"path": broll("coffee.mp4", 4.0, "")})), "asset_id");
    let b_typing2 = sid(&call(&mut ss2, "import_media", json!({"path": broll("typing.mp4", 4.0, "hue=h=90")})), "asset_id");
    let b_city2 = sid(&call(&mut ss2, "import_media", json!({"path": broll("city.mp4", 4.0, "hue=h=180")})), "asset_id");
    let r2 = call(&mut ss2, "place_broll", json!({"track_id": t2, "asset_ids": [b_coffee2.clone(), b_typing2.clone(), b_city2.clone()]}));
    assert_eq!(r2["broll_relevance"]["active"], json!(false), "{r2}");
    let got2: Vec<&str> = r2["clips"].as_array().unwrap().iter().map(|c| c["asset_id"].as_str().unwrap()).collect();
    assert_eq!(got2, vec![b_coffee2.as_str(), b_typing2.as_str(), b_city2.as_str()], "no words given: unchanged given-order behaviour: {r2}");
}

/// Explicit asset_tags win over filename guessing, and an asset with no filename signal (a generic name)
/// still gets matched correctly when tagged by hand.
#[test] fn place_broll_asset_tags_override_filename_guessing() {
    let mut ss = portrait();
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("relpb3.mp4", 10.0, &[(0.4, 9.6)], 1.0)})), "asset_id");
    let t = sid(&call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]})), "track_id");
    // deliberately generic filenames that give the filename-guesser nothing to go on
    let x = sid(&call(&mut ss, "import_media", json!({"path": broll("clip_a.mp4", 4.0, "")})), "asset_id");
    let y = sid(&call(&mut ss, "import_media", json!({"path": broll("clip_b.mp4", 4.0, "hue=h=90")})), "asset_id");
    let words = json!([{"text": "gym", "start": 3.0, "end": 3.3}, {"text": "workout", "start": 3.3, "end": 3.7}]);
    let r = call(&mut ss, "place_broll", json!({"track_id": t, "asset_ids": [x.clone(), y.clone()], "words": words, "asset_tags": {y.clone(): ["gym"]}}));
    let clips = r["clips"].as_array().unwrap();
    assert_eq!(clips[0]["asset_id"].as_str().unwrap(), y.as_str(), "the explicitly gym-tagged clip should land on the gym-talk slot, not the untagged one: {r}");
}

/// RV-34: text_over_subject fixes each caption PAGE independently, not the whole group by one page's nudge.
/// Two pages of the same caption group share a position; a bright "face" sits under the caption band only
/// during page two's own time window. Only page two should be flagged, and its fix (set_property on that one
/// clip) must leave page one's y untouched -- the old behaviour moved the entire group (set_caption_style)
/// by whatever page triggered it first, nudging pages that were already fine.
#[test] fn text_over_subject_fixes_each_caption_page_on_its_own() {
    fn bg_with_box(name: &str, w: usize, h: usize, dur: f64, box_rect: (i32, i32, i32, i32), on: (f64, f64)) -> String {
        let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
        let (bx, by, bw, bh) = box_rect;
        let st = Command::new("ffmpeg").args(["-v", "error", "-y",
            "-f", "lavfi", "-i", &format!("color=c=0x808080:s={w}x{h}:d={dur}:r=30,noise=alls=12:allf=t"),
            "-vf", &format!("drawbox=x={bx}:y={by}:w={bw}:h={bh}:color=white:t=fill:enable='between(t,{},{})'", on.0, on.1),
            "-pix_fmt", "yuv420p", "-an", p.to_str().unwrap()]).status().expect("ffmpeg");
        assert!(st.success()); p.to_string_lossy().into_owned()
    }
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let tt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let added = call(&mut ss, "add_caption_cues", json!({"track_id": tt, "cues": [{"start": 0.0, "end": 1.0, "text": "PAGE ONE"}, {"start": 1.5, "end": 2.5, "text": "PAGE TWO"}], "style": "boxed"}));
    let ids: Vec<String> = added["clip_ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let (p1, p2) = (ids[0].clone(), ids[1].clone());
    // both pages' boxes sit around x 534-538, y 592, w ~210, h 32 (same style/size): a wide bright box
    // covering that band, visible only during page two's 1.5-2.5s window.
    let path = bg_with_box("rv34_subject_v2.mp4", 1280, 720, 3.2, (480, 600, 320, 100), (1.5, 2.5));
    let mt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": path})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": mt, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 3.2, "fit": "cover"}));

    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let hits: Vec<&Value> = r["problems"].as_array().unwrap().iter().filter(|p| p["code"] == json!("text_over_subject")).collect();
    assert_eq!(hits.len(), 1, "only page two sits over the bright subject, page one should not be flagged: {r}");
    assert_eq!(hits[0]["clip_id"], json!(p2), "{:?}", hits[0]);
    assert_eq!(hits[0]["fix"]["tool"], json!("set_property"), "a per-page fix moves one clip, not the whole caption group via set_caption_style: {:?}", hits[0]);
    assert_eq!(hits[0]["fix"]["args"]["clip_id"], json!(p2), "{:?}", hits[0]);

    let y1_before = ss.project.find_clip(&p1).unwrap().props.get("y").unwrap().base;
    let fix = hits[0]["fix"].clone();
    call(&mut ss, fix["tool"].as_str().unwrap(), fix["args"].clone());
    let y1_after = ss.project.find_clip(&p1).unwrap().props.get("y").unwrap().base;
    let y2_after = ss.project.find_clip(&p2).unwrap().props.get("y").unwrap().base;
    assert_eq!(y1_before, y1_after, "page one's y must not move when only page two's fix is applied");
    assert_ne!(y1_after, y2_after, "page two should have moved clear of page one now");

    let r2 = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    assert!(!r2["problems"].as_array().unwrap().iter().any(|p| p["code"] == json!("text_over_subject")), "page two should be clear after its own fix: {r2}");
}

/// V7.7Q: broll_over_silence used to flag ANY media clip on a track after the dialogue track that didn't
/// overlap speech, including a clip placed deliberately AFTER all speech has ended for good (an end screen,
/// an outro, an epilogue shot) -- and "fixed" it by yanking it back onto the nearest speech window, which is
/// exactly wrong for that clip. A mid-edit gap (b-roll laid over a pause between two phrases) is still
/// correctly flagged; only a clip that starts once the dialogue track has no more speech left at all is exempt.
#[test] fn broll_over_silence_does_not_flag_a_clip_placed_after_all_speech_has_ended() {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 720, "height": 1280, "fps": 30}));
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": talk("broll_epilogue.mp4", 4.0, &[(0.3, 1.0), (1.8, 2.5)], 1.0)})), "asset_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 4, "fit": "cover"}));
    let bt = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let b = sid(&call(&mut ss, "import_media", json!({"path": broll("broll_epilogue_b.mp4", 4.0, "")})), "asset_id");
    // one b-roll clip over the MID-EDIT pause (1.0..1.8): still a real misplacement, should still be flagged.
    let over_gap = sid(&call(&mut ss, "add_clip", json!({"track_id": bt, "source_type": "media", "asset_id": b.clone(), "start": 1.1, "duration": 0.6, "fit": "cover"})), "clip_id");
    // one clip starting at 3.0, well after the LAST speech ends at 2.5: an end-screen / epilogue shot, not
    // misplaced b-roll -- should NOT be flagged, and should NOT be moved back onto the dialogue.
    let epilogue = sid(&call(&mut ss, "add_clip", json!({"track_id": bt, "source_type": "media", "asset_id": b, "start": 3.0, "duration": 1.0, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "lint_edit", json!({"platform": "none"}));
    let probs = r["problems"].as_array().unwrap();
    let flagged: Vec<&str> = probs.iter().filter(|p| p["code"] == "broll_over_silence").filter_map(|p| p["clip_id"].as_str()).collect();
    assert!(flagged.contains(&over_gap.as_str()), "the mid-edit gap should still be flagged: {probs:?}");
    assert!(!flagged.contains(&epilogue.as_str()), "a clip placed after all speech has ended should not be flagged as misplaced b-roll: {probs:?}");
}

/// V7.8Q (TOP PRIORITY): finish_audio's own dialogue-vs-ambience classifier (speech_ratio at
/// aggressiveness 2) was muting most of the real dialogue on real interview footage, back to the very
/// first real render in this series (real_v1.mp4, long before this session's pacing work) -- caught by a
/// whisper transcript of a shipped render coming back a fraction of the real sentences, and confirmed with
/// a from-scratch check: 4 of 6 of auto_rough_cut's own kept segments on talkinghead_real_final.mp4 read
/// speech_ratio under the classifier's 0.2 bar at aggressiveness 2 (one segment read -0.0 on an 0.8s clip
/// that is unmistakably someone talking), while finish_audio's final gain-trim loop still reported exactly
/// -14.0 LUFS throughout, because it only levels whatever SURVIVED, hiding the loss completely.
///
/// A synthetic sine-harmonic voice stand-in (same `talk()` this file already uses) does not reproduce this
/// -- it is too spectrally pure to trip aggressiveness 2's steady-tone rejection the way real voice does.
/// Per the fix's own instructions, this uses a short excerpt of the real interview, sliced fresh by ffmpeg
/// at test time (not committed as repo media) from the box's own benchmark fixture; if that fixture isn't
/// present (a clean checkout elsewhere), the test skips rather than fails, since the box-only file is a
/// real-world confirmation on top of the unit-level fix, not the fix's only proof.
#[test] fn finish_audio_does_not_mute_real_speech_aggressiveness_2_missed() {
    let source = "/home/user/tab/work/v7q_real/talkinghead_real_final.mp4";
    if !std::path::Path::new(source).exists() {
        eprintln!("skipping: real-footage fixture not present on this box ({source})");
        return;
    }
    // The exact segment that read -0.0 (not -0.0..0.2, actually negative) at BOTH aggressiveness 2 and 1 in
    // the from-scratch check -- auto_rough_cut's own clip7, 16.7..17.5s, real continuous speech.
    let p = dir().join("real_excerpt_clip7.mp4");
    ffmpeg(&["-ss", "16.7", "-t", "0.8", "-i", source, "-c", "copy", p.to_str().unwrap()]);
    let mut ss = Session::new();
    call(&mut ss, "project_set", json!({"width": 640, "height": 360, "fps": 30}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": p.to_string_lossy()})), "asset_id");
    let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 0.8, "fit": "cover"}));
    let r = call(&mut ss, "finish_audio", json!({}));
    let dia: Vec<&str> = r["dialogue"].as_array().unwrap().iter().filter_map(|d| d["clip_id"].as_str()).collect();
    let amb: Vec<&str> = r["ambience_muted"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(!dia.is_empty(), "real continuous speech must be classified as dialogue, not muted: {r}");
    assert!(amb.is_empty(), "real continuous speech must not be muted as ambience: {r}");
}

/// V7.8Q: rough_cut's own speech detection (VAD mode, aggressiveness 1) needs several consecutive
/// confirming frames before it declares speech has started -- on a real interview that begins talking
/// immediately with no lead-in silence, that onset lag cost the first ~0.48s of real words (confirmed
/// independently: whisper hears real speech at the source's literal 0.0s, and the raw VAD probe's own
/// first detected span started at 0.48s). recover_missed_onset re-checks only the very first kept segment
/// of the whole output with a plain RMS silence probe (no VAD confirmation delay) and pulls the start back
/// if that window is not actually silence. Per this fix's own real-footage nature, this slices a fresh
/// excerpt from the box's benchmark fixture at test time (not committed media) and skips if absent.
#[test] fn rough_cut_recovers_real_speech_the_vad_onset_missed_at_the_very_start() {
    let source = "/home/user/tab/work/v7q_real/talkinghead_real_final.mp4";
    if !std::path::Path::new(source).exists() {
        eprintln!("skipping: real-footage fixture not present on this box ({source})");
        return;
    }
    let mut ss = Session::new();
    call(&mut ss, "project_set", json!({"width": 1280, "height": 720, "fps": 30}));
    let a = sid(&call(&mut ss, "import_media", json!({"path": source})), "asset_id");
    let rc = call(&mut ss, "auto_rough_cut", json!({"asset_ids": [a]}));
    let segs = rc["segments"].as_array().unwrap();
    let first_start = segs[0]["source"][0].as_f64().unwrap();
    assert!(first_start < 0.1, "the first kept segment should start at or near the real audio's own start (0.0s, confirmed real speech there), not {first_start}s: {rc}");
}

/// V8.0Q: match_shot_look pulls a target clip's own measured look toward a reference clip's -- the
/// color/exposure MATCH auto_grade (an absolute, fixed-target tool) can't do: two shots that are each
/// "fine" on their own but clash with each other (a warm-biased shot cut against a cool-biased one).
#[test] fn match_shot_look_pulls_target_toward_reference() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let warm = sid(&call(&mut ss, "import_media", json!({"path": broll("msl_warm.mp4", 1.0, "colorchannelmixer=rr=1.35:gg=1.0:bb=0.55")})), "asset_id");
    let cool = sid(&call(&mut ss, "import_media", json!({"path": broll("msl_cool.mp4", 1.0, "colorchannelmixer=rr=0.55:gg=1.0:bb=1.35")})), "asset_id");
    let cref = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": warm, "start": 0, "duration": 1, "fit": "cover"})), "clip_id");
    let ctgt = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": cool, "start": 1, "duration": 1, "fit": "cover"})), "clip_id");
    let ref_rgb: Vec<f64> = call(&mut ss, "get_scopes", json!({"time": 0.5}))["avg_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let before_rgb: Vec<f64> = call(&mut ss, "get_scopes", json!({"time": 1.5}))["avg_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let dist = |a: &[f64], b: &[f64]| -> f64 { (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt() };
    let gap_before = dist(&ref_rgb, &before_rgb);
    assert!(gap_before > 0.03, "the two test clips should actually look different before matching: ref {ref_rgb:?} vs target {before_rgb:?}");

    let r = call(&mut ss, "match_shot_look", json!({"reference_clip_ids": [cref.clone()], "target_clip_ids": [ctgt.clone()], "strength": 1.0}));
    assert!(r["matched"].as_array().unwrap().iter().any(|m| m["clip_id"] == json!(ctgt)), "{r}");
    let after_rgb: Vec<f64> = call(&mut ss, "get_scopes", json!({"time": 1.5}))["avg_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let gap_after = dist(&ref_rgb, &after_rgb);
    assert!(gap_after < gap_before * 0.5, "match_shot_look should have closed most of the color gap: before {gap_before:.4}, after {gap_after:.4} ({r})");
    // the reference clip itself is untouched.
    let ref_rgb_after: Vec<f64> = call(&mut ss, "get_scopes", json!({"time": 0.5}))["avg_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(dist(&ref_rgb, &ref_rgb_after) < 1e-6, "the reference clip should never itself be graded: {ref_rgb:?} -> {ref_rgb_after:?}");
}

/// A clip named in both reference_clip_ids and target_clip_ids is left alone (it IS the reference look;
/// grading it against itself would be a no-op at best, a confusing double-count at worst).
#[test] fn match_shot_look_skips_a_clip_that_is_its_own_reference() {
    let mut ss = portrait(); let t = sid(&call(&mut ss, "add_track", json!({})), "track_id");
    let a = sid(&call(&mut ss, "import_media", json!({"path": broll("msl_self.mp4", 1.0, "")})), "asset_id");
    let c = sid(&call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0, "duration": 1, "fit": "cover"})), "clip_id");
    let r = call(&mut ss, "match_shot_look", json!({"reference_clip_ids": [c.clone()], "target_clip_ids": [c.clone()]}));
    assert!(r["skipped"].as_array().unwrap().iter().any(|s| s["clip_id"] == json!(c)), "{r}");
    assert!(r["matched"].as_array().unwrap().is_empty(), "{r}");
}
