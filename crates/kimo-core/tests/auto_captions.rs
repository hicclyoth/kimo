//! Automatic animated captions (lane KC): auto_captions, edit_caption_words, export_captions.
//! Behaviour is checked on rendered frames: words pop in as spoken, the spoken word is coloured, a keyword is coloured,
//! captions sit in the 9:16 safe band, a fix keeps the timing, and the recognizer path (a stub whisper-cli writing whisper.cpp
//! JSON) maps words onto the timeline with their starts snapped to the audio.
use kimo_core::commands::{dispatch, Session};
use kimo_core::render::{render_frame, Cache};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// `no_recognizer_at_all_names_the_real_situation_and_a_real_fallback` and `a_missing_recognizer_says_how_to_get_one`
/// mutate process-global PATH/KIMO_MODELS/KIMO_WHISPER_BIN to simulate "no recognizer installed"; under the default
/// parallel test runner every test in this binary shares that one process environment, so any other test spawning
/// ffmpeg/ffprobe (directly or via dispatch) while PATH is restricted can get a spurious NotFound. Every test here
/// takes this lock for its whole body so the two env-mutating tests never overlap with anything else in the file.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn env_guard() -> std::sync::MutexGuard<'static, ()> { ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()) }

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_auto_captions_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn portrait() -> Session {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 360, "height": 640, "fps": 30}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "solid", "color": [0.1, 0.1, 0.12], "start": 0, "duration": 6})); ss
}
fn words() -> Value { json!([
    {"text": "this", "start": 0.50, "end": 0.80}, {"text": "is", "start": 0.90, "end": 1.10}, {"text": "the", "start": 1.15, "end": 1.35},
    {"text": "best", "start": 1.60, "end": 1.95}, {"text": "bottle.", "start": 2.05, "end": 2.60}]) }
#[derive(Debug, Default, Clone, Copy)]
struct Ink { n: usize, yellow: usize, green: usize, x0: usize, x1: usize, y0: usize, y1: usize, ycx: f64 }
/// Text ink in a frame: bright pixels, and how many are the active yellow / keyword green; `ycx` = mean x of yellow pixels.
fn ink(ss: &Session, t: f64) -> Ink {
    let f = render_frame(&ss.project, t, &Cache::new()).unwrap(); let mut k = Ink { x0: usize::MAX, y0: usize::MAX, ..Default::default() }; let mut sx = 0.0;
    for y in 0..f.h { for x in 0..f.w { let i = (y * f.w + x) * 4; let (r, g, b) = (f.d[i], f.d[i + 1], f.d[i + 2]);
        if r + g + b > 1.2 { k.n += 1; k.x0 = k.x0.min(x); k.x1 = k.x1.max(x); k.y0 = k.y0.min(y); k.y1 = k.y1.max(y); }
        if r > 0.7 && g > 0.4 && b < 0.2 { k.yellow += 1; sx += x as f64; }
        if g > 0.7 && r < 0.45 && b < 0.5 { k.green += 1; } } }
    k.ycx = if k.yellow > 0 { sx / k.yellow as f64 } else { -1.0 }; k
}

#[test] fn pop_style_reveals_each_word_as_it_is_spoken_and_colours_it() { let _env_guard = env_guard();
    let mut ss = portrait();
    let r = call(&mut ss, "auto_captions", json!({"words": words(), "style": "pop", "position": "bottom"}));
    assert_eq!(r["words"], 5); assert_eq!(r["pages"], 2, "{r}"); assert!(r["caption_id"].is_string());
    let first = ink(&ss, 0.55); let all3 = ink(&ss, 1.30);
    assert!(first.n > 30 && all3.n > first.n, "only the spoken word shows at 0.55 s: ink {} vs {} when three words are out", first.n, all3.n);
    assert!(all3.x1 - all3.x0 > first.x1 - first.x0 + 20, "the line grows as words arrive: width {} then {}", first.x1 - first.x0, all3.x1 - all3.x0);
    let w1 = ink(&ss, 0.97); let w2 = ink(&ss, 1.25);
    assert!(w1.yellow > 20 && w2.yellow > 20, "the spoken word is coloured: {} / {} yellow px", w1.yellow, w2.yellow);
    assert!(w2.ycx > w1.ycx + 10.0, "the highlight moves right with the speech: x {} then {}", w1.ycx, w2.ycx);
    assert_eq!(ink(&ss, 0.3).n, 0, "nothing shows before the first word");
}

#[test] fn captions_sit_in_the_portrait_safe_band() { let _env_guard = env_guard();
    let mut ss = portrait(); call(&mut ss, "auto_captions", json!({"words": words(), "style": "karaoke", "position": "bottom"}));
    for t in [0.6, 1.3, 2.2] { let k = ink(&ss, t); assert!(k.n > 0, "caption at {t}");
        assert!(k.x0 >= 18 && k.x1 <= 342, "inside 5% side margins at {t}: x {}..{}", k.x0, k.x1);
        assert!(k.y0 as f64 >= 0.5 * 640.0 && k.y1 as f64 <= 0.86 * 640.0, "lower band, above the app strip at {t}: y {}..{}", k.y0, k.y1); }
    let mut top = portrait(); call(&mut top, "auto_captions", json!({"words": words(), "style": "karaoke", "position": "top"}));
    let k = ink(&top, 1.3); assert!((k.y1 as f64) < 0.4 * 640.0, "top band: y {}..{}", k.y0, k.y1);
}

/// A boxed/pop-with-background style's actual visible pill (text ink padded by its own bg_pad) must stay
/// inside a real side margin -- not run edge to edge with its corners clipped by the canvas. compute_text_bounds
/// deliberately excludes the background box from "ink" (the right answer for a safe-area ink check), so this
/// reads the rendered pixels directly: the box is near-black (bg [0,0,0,0.82]) against a bright background,
/// clearly darker than anything else in frame.
#[test] fn a_boxed_captions_actual_pill_stays_inside_a_real_side_margin() { let _env_guard = env_guard();
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 360, "height": 640, "fps": 30}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "solid", "color": [0.85, 0.85, 0.85], "start": 0, "duration": 6}));
    // a long line: the kind of caption most likely to run the pill edge to edge if its wrap width doesn't
    // account for the pill's own padding.
    let long_words = json!([
        {"text": "honestly", "start": 0.2, "end": 0.6}, {"text": "this", "start": 0.6, "end": 0.8}, {"text": "is", "start": 0.8, "end": 0.95},
        {"text": "the", "start": 0.95, "end": 1.05}, {"text": "best", "start": 1.05, "end": 1.3}]);
    call(&mut ss, "auto_captions", json!({"words": long_words, "style": "boxed", "position": "bottom"}));
    let f = render_frame(&ss.project, 0.9, &Cache::new()).unwrap();
    let (mut x0, mut x1) = (usize::MAX, 0usize);
    for y in 0..f.h { for x in 0..f.w { let i = (y * f.w + x) * 4; let (r, g, b) = (f.d[i], f.d[i + 1], f.d[i + 2]);
        if r + g + b < 1.2 { x0 = x0.min(x); x1 = x1.max(x); } } }
    assert!(x1 > x0, "no dark pill pixels found at all");
    let w = f.w as f64;
    assert!(x0 as f64 >= 0.04 * w, "pill's left edge should clear a real side margin, not run to the canvas edge: x0={x0} of {w}");
    assert!((x1 as f64) <= 0.96 * w, "pill's right edge should clear a real side margin, not run to the canvas edge: x1={x1} of {w}");
}

/// The real benchmark shape: a caption group created as "pop" (no background), then switched to "boxed" by
/// text_over_subject's own fix (set_caption_style) -- the SEPARATE styling path in editorial_tools.rs that
/// set_caption_style/add_caption_cues/add_captions share, distinct from auto_captions.rs's own build() (fixed
/// two commits ago). Tab's own re-render still showed a full-frame-width pill after that fix landed, because
/// this second path had the identical bug and nothing had touched it.
#[test] fn switching_an_existing_caption_to_boxed_also_respects_the_side_margin() { let _env_guard = env_guard();
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 360, "height": 640, "fps": 30}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "solid", "color": [0.85, 0.85, 0.85], "start": 0, "duration": 6}));
    let long_words = json!([
        {"text": "honestly", "start": 0.2, "end": 0.6}, {"text": "this", "start": 0.6, "end": 0.8}, {"text": "is", "start": 0.8, "end": 0.95},
        {"text": "the", "start": 0.95, "end": 1.05}, {"text": "best", "start": 1.05, "end": 1.3}]);
    let r = call(&mut ss, "auto_captions", json!({"words": long_words, "style": "pop", "position": "bottom"}));
    let caption_id = r["caption_id"].as_str().unwrap().to_string();
    // the exact move text_over_subject's fix makes: switch an existing (no-background) group to "boxed".
    call(&mut ss, "set_caption_style", json!({"caption_id": caption_id, "style": "boxed"}));
    let f = render_frame(&ss.project, 0.9, &Cache::new()).unwrap();
    let (mut x0, mut x1) = (usize::MAX, 0usize);
    for y in 0..f.h { for x in 0..f.w { let i = (y * f.w + x) * 4; let (r, g, b) = (f.d[i], f.d[i + 1], f.d[i + 2]);
        if r + g + b < 1.2 { x0 = x0.min(x); x1 = x1.max(x); } } }
    assert!(x1 > x0, "no dark pill pixels found at all");
    let w = f.w as f64;
    assert!(x0 as f64 >= 0.04 * w, "pill's left edge should clear a real side margin after switching to boxed: x0={x0} of {w}");
    assert!((x1 as f64) <= 0.96 * w, "pill's right edge should clear a real side margin after switching to boxed: x1={x1} of {w}");
}

#[test] fn boxed_style_colours_a_keyword_and_the_spoken_word() { let _env_guard = env_guard();
    let mut ss = portrait(); let r = call(&mut ss, "auto_captions", json!({"words": words(), "style": "boxed", "position": "bottom"}));
    assert_eq!(r["pages"], 2);
    // page 2 is "best bottle.": while "best" is spoken it is green (active) and "bottle" is the yellow keyword
    let k = ink(&ss, 1.75); assert!(k.green > 20, "active word green: {k:?}"); assert!(k.yellow > 20, "keyword yellow: {k:?}");
    assert!(k.ycx > (k.x0 + k.x1) as f64 / 2.0, "the keyword is the second word (right half): {k:?}");
}

#[test] fn a_word_fix_keeps_the_timing_and_the_animation() { let _env_guard = env_guard();
    let mut ss = portrait(); let r = call(&mut ss, "auto_captions", json!({"words": words(), "style": "pop", "position": "bottom"}));
    let id = r["caption_id"].as_str().unwrap().to_string(); let before = ink(&ss, 2.2);
    let clips_before: Vec<(f64, f64)> = ss.project.tracks.last().unwrap().clips.iter().map(|c| (c.start, c.duration)).collect();
    let e = call(&mut ss, "edit_caption_words", json!({"caption_id": id, "edits": [{"find": "bottle", "text": "tumbler."}, {"index": 0, "text": "This"}]}));
    assert_eq!(e["changed"].as_array().unwrap().len(), 2, "{e}"); assert_eq!(e["changed"][0]["from"], "bottle.");
    let clips_after: Vec<(f64, f64)> = ss.project.tracks.last().unwrap().clips.iter().map(|c| (c.start, c.duration)).collect(); assert_eq!(clips_before, clips_after, "timing unchanged");
    let texts: Vec<String> = ss.project.tracks.last().unwrap().clips.iter().filter_map(|c| if let kimo_core::model::Source::Text { text, .. } = &c.source { Some(text.clone()) } else { None }).collect();
    assert!(texts.iter().any(|t| t.contains("tumbler.")) && texts.iter().any(|t| t.starts_with("This")), "{texts:?}");
    let after = ink(&ss, 2.2); assert!(after.yellow > 20 && before.yellow > 20, "still highlighted at the same time");
    let tr = ss.project.editorial.transcripts.values().next().unwrap(); assert!(tr.words.iter().any(|w| w.recognized == "bottle." && w.display == "tumbler."), "recognized text kept");
    let bad = dispatch(&mut ss, "edit_caption_words", &json!({"edits": [{"index": 9, "text": "x"}]})).unwrap_err(); assert!(bad.contains("past the last word"), "{bad}");
    let none = dispatch(&mut ss, "edit_caption_words", &json!({"edits": [{"find": "zebra", "text": "x"}]})).unwrap_err(); assert!(none.contains("no caption word matches"), "{none}");
}

#[test] fn sidecars_match_the_pages() { let _env_guard = env_guard();
    let mut ss = portrait(); let srt = dir().join("c.srt"); let vtt = dir().join("c.vtt"); let _ = std::fs::remove_file(&srt); let _ = std::fs::remove_file(&vtt);
    let r = call(&mut ss, "auto_captions", json!({"words": words(), "style": "clean", "position": "bottom", "srt_path": srt.to_str().unwrap()}));
    assert_eq!(r["files"][0], srt.to_str().unwrap()); let s = std::fs::read_to_string(&srt).unwrap();
    assert!(s.starts_with("1\n00:00:00,500 --> "), "{s}"); assert!(s.contains("this is the best bottle.") || s.contains("bottle."), "{s}");
    let e = call(&mut ss, "export_captions", json!({"path": vtt.to_str().unwrap()})); assert_eq!(e["format"], "vtt");
    assert!(std::fs::read_to_string(&vtt).unwrap().starts_with("WEBVTT\n\n00:00:00.500 --> "));
    let again = dispatch(&mut ss, "export_captions", &json!({"path": vtt.to_str().unwrap()})).unwrap_err(); assert!(again.contains("dest_exists"), "{again}");
}

/// A stub whisper-cli: records its arguments, then writes whisper.cpp JSON for three words with DTW times shaped like real
/// whisper.cpp output (about 250 ms late). `-l auto` reports `lang_auto` as the language.
fn stub(lang_auto: &str) -> (PathBuf, PathBuf) { stub_as(lang_auto, lang_auto) }
/// The same stub under its own file names (`tag`), so tests running in parallel keep separate argument logs.
fn stub_as(lang_auto: &str, tag: &str) -> (PathBuf, PathBuf) {
    let d = dir(); let log = d.join(format!("args_{tag}.txt")); let bin = d.join(format!("whisper_stub_{tag}.sh"));
    let tok = |t: &str, ms: u32, dtw: u32| format!("{{\"text\":\"{t}\",\"offsets\":{{\"from\":{ms},\"to\":{}}},\"p\":0.93,\"t_dtw\":{dtw}}}", ms + 200);
    // two tokens per word, like real output: the first token's DTW time is the onset + 0.25 s, the last one's is the next onset - 0.04 s
    let body = format!("[{{\"text\":\"[_BEG_]\",\"offsets\":{{\"from\":0,\"to\":0}},\"p\":0.9,\"t_dtw\":-1}},{},{},{},{},{},{},{}]",
        tok(" o", 300, 80), tok("ne", 500, 101), tok(" tw", 800, 130), tok("o", 900, 156), tok(" thr", 1200, 185), tok("ee", 1400, 200), tok(".", 1500, 201));
    let script = format!("#!/bin/sh\necho \"$@\" >> {log}\nOF=''\nL=''\nwhile [ $# -gt 0 ]; do case \"$1\" in -of) OF=\"$2\"; shift;; -l) L=\"$2\"; shift;; esac; shift; done\nif [ \"$L\" = auto ]; then LG={lang_auto}; else LG=$L; fi\ncat > \"$OF.json\" <<EOF\n{{\"result\":{{\"language\":\"$LG\"}},\"transcription\":[{{\"offsets\":{{\"from\":0,\"to\":2000}},\"text\":\" one two three.\",\"tokens\":{body}}}]}}\nEOF\n", log = log.display());
    std::fs::write(&bin, script).unwrap(); Command::new("chmod").arg("+x").arg(&bin).status().unwrap(); let _ = std::fs::remove_file(&log); (bin, log)
}
/// 2 s of 360x640 video with three 1 kHz tone bursts (speech stand-ins) starting at 0.55, 1.05 and 1.60 s.
fn bursts() -> PathBuf {
    let p = dir().join("bursts.mp4"); if !p.exists() {
        let st = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=gray:s=360x640:r=30:d=2", "-f", "lavfi", "-i",
            "aevalsrc='0.5*sin(2*PI*1000*t)*(between(t,0.55,0.85)+between(t,1.05,1.40)+between(t,1.60,1.95))':s=48000:d=2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&p).stdin(Stdio::null()).status().unwrap(); assert!(st.success()); }
    p
}
fn with_clip(start: f64) -> Session {
    let mut ss = portrait(); let p = bursts();
    let a = call(&mut ss, "import_media", json!({"path": p.to_str().unwrap()})); let asset = a["asset_id"].as_str().unwrap().to_string();
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": asset, "start": start, "duration": 2.0})); ss
}

#[cfg(unix)] // the recognizer here is a sh shim
#[test] fn recognized_words_land_on_the_timeline_snapped_to_the_audio() { let _env_guard = env_guard();
    let (bin, log) = stub("en"); let model = dir().join("ggml-small-q5_1.bin"); std::fs::write(&model, b"stub").unwrap();
    let mut ss = with_clip(2.0);
    let r = call(&mut ss, "auto_captions", json!({"whisper_bin": bin.to_str().unwrap(), "model": model.to_str().unwrap(), "language": "en", "style": "pop", "position": "bottom"}));
    assert_eq!(r["words"], 3, "{r}"); assert_eq!(r["preview"], "one two three.");
    let args = std::fs::read_to_string(&log).unwrap(); assert!(args.contains("-l en") && args.contains("-dtw small") && args.contains("-ojf"), "{args}");
    let tr = ss.project.editorial.transcripts.values().next().unwrap(); let st: Vec<f64> = tr.words.iter().map(|w| w.start).collect();
    for (got, want) in st.iter().zip([2.55, 3.05, 3.60]) { assert!((got - want).abs() <= 0.04, "word start {got:.3} vs burst onset {want} (raw DTW was 250 ms late): {st:?}"); }
}

#[cfg(unix)] // the recognizer here is a sh shim
#[test] fn hindi_speech_is_captioned_as_roman_hinglish() { let _env_guard = env_guard();
    let (bin, log) = stub("hi"); let model = dir().join("ggml-small-q5_1.bin"); std::fs::write(&model, b"stub").unwrap();
    let mut ss = with_clip(0.0);
    let r = call(&mut ss, "auto_captions", json!({"whisper_bin": bin.to_str().unwrap(), "model": model.to_str().unwrap(), "hint": "Kimo"}));
    assert_eq!(r["mode"], "hinglish", "{r}"); assert_eq!(r["language"], "hi");
    let args = std::fs::read_to_string(&log).unwrap(); let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 2, "detect, then a Roman-script pass: {args}"); assert!(lines[0].contains("-l auto") && lines[0].contains("-dl"), "language id only: {args}"); assert!(lines[1].contains("-l en") && lines[1].contains("Haan toh") && lines[1].contains("Kimo"), "{args}");
}

#[cfg(unix)] // the recognizer here is a sh shim
#[test] fn a_turbo_model_writes_hinglish_from_a_hindi_pass_not_an_english_one() { let _env_guard = env_guard();
    // large-v3-turbo given -l en (plus the Roman prompt) on Hindi speech translates it into English (Naman's clips: 62.7% WER);
    // -l hi then romanize keeps the words that were said (49.7% strict / 37.9% ignoring spelling).
    let (bin, log) = stub_as("hi", "hi_turbo"); let model = dir().join("ggml-large-v3-turbo-q5_0.bin"); std::fs::write(&model, b"stub").unwrap();
    for (lang, passes) in [("auto", 2usize), ("hinglish", 1), ("hi", 1)] {
        let _ = std::fs::remove_file(&log); let mut ss = with_clip(0.0);
        let r = call(&mut ss, "auto_captions", json!({"whisper_bin": bin.to_str().unwrap(), "model": model.to_str().unwrap(), "language": lang}));
        assert_eq!(r["mode"], "hinglish", "{lang}: {r}");
        let args = std::fs::read_to_string(&log).unwrap(); let lines: Vec<&str> = args.lines().collect(); assert_eq!(lines.len(), passes, "{lang}: {args}");
        let rec = lines[passes - 1];
        assert!(rec.contains("-l hi") && rec.contains("-dtw large.v3.turbo"), "{lang}: turbo must recognize Hindi as Hindi: {rec}");
        assert!(!rec.contains("-l en") && !rec.contains("Haan toh"), "{lang}: no English pass or Roman prompt on turbo: {rec}");
    }
    let (bin, log) = stub_as("hi", "hi_small"); let small = dir().join("ggml-small-q5_1.bin"); std::fs::write(&small, b"stub").unwrap();
    let mut ss = with_clip(0.0); call(&mut ss, "auto_captions", json!({"whisper_bin": bin.to_str().unwrap(), "model": small.to_str().unwrap(), "language": "hinglish"}));
    let args = std::fs::read_to_string(&log).unwrap(); assert!(args.contains("-l en") && args.contains("Haan toh"), "small keeps its Roman-prompt pass: {args}");
}

/// A stub whisper-cli that "hears" `said` (one token per word, 300 ms apart from 0.3 s), whatever the language asked for.
fn stub_said(tag: &str, said: &[&str]) -> PathBuf {
    let d = dir(); let bin = d.join(format!("whisper_said_{tag}.sh"));
    let toks: Vec<String> = said.iter().enumerate().map(|(i, w)| { let ms = 300 + 300 * i as u32; format!("{{\"text\":\" {w}\",\"offsets\":{{\"from\":{ms},\"to\":{}}},\"p\":0.93,\"t_dtw\":{}}}", ms + 250, ms / 10 + 25) }).collect();
    let script = format!("#!/bin/sh\nOF=''\nL=''\nwhile [ $# -gt 0 ]; do case \"$1\" in -of) OF=\"$2\"; shift;; -l) L=\"$2\"; shift;; esac; shift; done\nif [ \"$L\" = auto ]; then LG=hi; else LG=$L; fi\ncat > \"$OF.json\" <<EOF\n{{\"result\":{{\"language\":\"$LG\"}},\"transcription\":[{{\"offsets\":{{\"from\":0,\"to\":2000}},\"text\":\" {}\",\"tokens\":[{}]}}]}}\nEOF\n", said.join(" "), toks.join(","));
    std::fs::write(&bin, script).unwrap(); Command::new("chmod").arg("+x").arg(&bin).status().unwrap(); bin
}

#[cfg(unix)] // the recognizer here is a sh shim
#[test] fn hinglish_variants_are_written_one_way_and_english_is_left_alone() { let _env_guard = env_guard();
    // the recognizer spells the same Hinglish word several ways (bahut / bohat / bohot, pani / paani, ek dum / ekdum);
    // Hinglish captions get one spelling, keeping the speaker's capitals and punctuation; English mode is untouched
    let said = ["Bahut", "pani,", "ek", "dum", "YEH", "nahin."];
    let bin = stub_said("variants", &said); let model = dir().join("ggml-small-q5_1.bin"); std::fs::write(&model, b"stub").unwrap();
    let run = |a: Value| { let mut ss = with_clip(0.0); let mut a = a; a["whisper_bin"] = json!(bin.to_str().unwrap()); a["model"] = json!(model.to_str().unwrap());
        let r = call(&mut ss, "auto_captions", a); let tr = ss.project.editorial.transcripts.values().next().unwrap();
        let w: Vec<(String, f64, f64)> = tr.words.iter().map(|w| (w.display.replace('\u{a0}', " "), w.start, w.end)).collect(); (r, w) };
    for lang in ["hinglish", "auto", "hi"] {
        let (r, w) = run(json!({"language": lang}));
        assert_eq!(r["mode"], "hinglish", "{lang}: {r}");
        let text: Vec<&str> = w.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(text, ["Bohot", "paani,", "ekdum", "YE", "nahi."], "{lang}: Hinglish spelling must be normalised: {r}");
        assert_eq!(r["words"], 5, "{lang}: 'ek dum' is one word: {r}");
        let (_, w0) = run(json!({"language": lang, "normalize_spelling": false}));
        assert!((w[2].1 - w0[2].1).abs() < 1e-9 && (w[2].2 - w0[3].2).abs() < 1e-9, "{lang}: ekdum spans 'ek' start to 'dum' end: {w:?} vs {w0:?}");
    }
    let (r, w) = run(json!({"language": "hinglish", "normalize_spelling": false}));
    assert_eq!(w.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), said, "normalize_spelling false keeps what was heard: {r}");
    let (r, w) = run(json!({"language": "en"}));
    assert_eq!(w.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), said, "English mode is never respelled: {r}");
    let mut ss = with_clip(0.0);
    let e = dispatch(&mut ss, "auto_captions", &json!({"whisper_bin": bin.to_str().unwrap(), "model": model.to_str().unwrap(), "language": "hinglish", "normalize_spelling": "yes"})).unwrap_err();
    assert!(e.contains("normalize_spelling"), "{e}");
}

#[test] fn devanagari_is_romanized_the_way_hinglish_is_written() { let _env_guard = env_guard();
    use kimo_core::speech_words::romanize;
    assert_eq!(romanize("पानी बहुत ठंडा रहता है।"), "pani bahut thanda rahta hai.");
    assert_eq!(romanize("ये मेरी बोतल प्लास्टिक की है"), "ye meri botal plastik ki hai");
    assert_eq!(romanize("हमेशा"), "hamesha"); assert_eq!(romanize("I love it"), "I love it");
}

#[cfg(unix)] // symlinks a one-tool PATH
#[test] fn no_recognizer_at_all_names_the_real_situation_and_a_real_fallback() { let _env_guard = env_guard();
    // The real no-setup-box scenario: no whisper-cli anywhere, no model, nothing overridden --
    // the exact shape director_mode.md's own small model hits when it calls auto_captions with no
    // speech model installed. The old message led with a shell script and an env var the caller
    // can't run/set itself, and buried "skip captions, or use caption_from_words" behind "Or pass
    // words:[...]" worded as a parameter hint, not a real next step. This checks the caller is told
    // plainly what's wrong and given a real fallback it can actually do in one more tool call.
    let mut ss = with_clip(0.0); // builds its fixture media via ffmpeg -- before PATH is touched
    // A clean PATH with only ffmpeg symlinked in (recognize() shells out to ffmpeg before it ever
    // looks for whisper-cli) and nothing else -- guarantees no whisper-cli resolves, on any box.
    let ffmpeg_only = dir().join("ffmpeg_only_path"); std::fs::create_dir_all(&ffmpeg_only).unwrap();
    let real_ffmpeg = String::from_utf8(Command::new("which").arg("ffmpeg").output().unwrap().stdout).unwrap().trim().to_string();
    let _ = std::os::unix::fs::symlink(&real_ffmpeg, ffmpeg_only.join("ffmpeg"));
    let saved_path = std::env::var_os("PATH"); let saved_bin = std::env::var_os("KIMO_WHISPER_BIN");
    std::env::set_var("PATH", &ffmpeg_only); std::env::remove_var("KIMO_WHISPER_BIN");
    std::env::set_var("KIMO_MODELS", dir().join("truly_no_models_anywhere"));
    let e = dispatch(&mut ss, "auto_captions", &json!({})).unwrap_err();
    if let Some(p) = saved_path { std::env::set_var("PATH", p); } if let Some(b) = saved_bin { std::env::set_var("KIMO_WHISPER_BIN", b); }
    assert!(e.contains("no speech model is installed"), "names the real situation plainly: {e}");
    assert!(e.contains("caption_from_words"), "names the real director-mode fallback tool: {e}");
    assert!(e.to_lowercase().contains("skip captions") || e.to_lowercase().contains("ship without captions"), "names skipping captions as an option: {e}");
    assert!(e.contains("tools/whisper/fetch.sh"), "still says how to add a recognizer later: {e}");
}

#[test] fn a_missing_recognizer_says_how_to_get_one() { let _env_guard = env_guard();
    let mut ss = with_clip(0.0); std::env::set_var("KIMO_MODELS", dir().join("no_models_here"));
    let e = dispatch(&mut ss, "auto_captions", &json!({"whisper_bin": "/nonexistent/whisper-cli"})).unwrap_err();
    assert!(e.contains("tools/whisper/fetch.sh") || e.contains("speech recognizer"), "{e}");
    let e2 = dispatch(&mut ss, "auto_captions", &json!({"words": [{"text": "a", "start": 1.0, "end": 0.5}]})).unwrap_err(); assert!(e2.contains("start < end"), "{e2}");
    let e3 = dispatch(&mut ss, "auto_captions", &json!({"words": words(), "style": "neon"})).unwrap_err(); assert!(e3.contains("pop|karaoke|clean|boxed|impact"), "{e3}");
}

/// 2 s of 360x640 video: a dark room with a skin-toned face-sized patch whose vertical extent is `y0..y1` of the frame.
fn face_plate(name: &str, y0: f64, y1: f64) -> PathBuf {
    let p = dir().join(name); if !p.exists() {
        let (y, h) = ((y0 * 640.0) as i32, ((y1 - y0) * 640.0) as i32);
        let st = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=0x202830:s=360x640:r=30:d=2", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono",
            "-vf", &format!("drawbox=x=110:y={y}:w=140:h={h}:color=0xD9A07E:t=fill"), "-t", "2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&p).stdin(Stdio::null()).status().unwrap(); assert!(st.success()); }
    p
}
#[test] fn captions_move_off_a_face_low_in_the_frame() { let _env_guard = env_guard();
    for (name, y0, y1, want) in [("face_low.mp4", 0.52, 0.86, "top"), ("face_high.mp4", 0.10, 0.38, "bottom")] {
        let mut ss = portrait(); let a = call(&mut ss, "import_media", json!({"path": face_plate(name, y0, y1).to_str().unwrap()}));
        let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
        call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "media", "asset_id": a["asset_id"], "start": 0, "duration": 2.0}));
        let r = call(&mut ss, "auto_captions", json!({"words": [{"text": "hello", "start": 0.2, "end": 0.6}, {"text": "there", "start": 0.7, "end": 1.2}], "style": "boxed"}));
        assert_eq!(r["position"], want, "face at {y0}..{y1}: {r}");
        let cap = ss.project.tracks.last().unwrap().clips[0].props["y"].at(0.0); let cy = 0.5 + cap / 640.0;
        assert!(cy < y0 - 0.05 || cy > y1 + 0.03, "caption centre {cy:.2} is clear of the face {y0}..{y1}");
    }
}
