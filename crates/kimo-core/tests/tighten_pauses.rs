//! tighten_pauses: shrink the dead air a rough cut leaves INSIDE one of its own kept clips -- a breath
//! between two phrases, or the lead-in before the first word -- down to a natural pad, reusing
//! `trim_filler_words`'s existing ripple-delete + caption-retime path. Deterministic: one synthetic clip
//! with known silences at known positions (same `talk()` recipe as cut_to_speech.rs / director_small.rs,
//! so the VAD hears real speech-shaped tones and real silence, not a guess).
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_tighten_pauses_{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }
fn ffmpeg(args: &[&str]) { let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg"); assert!(st.success(), "ffmpeg {args:?}"); }
fn c(s: &mut Session, n: &str, a: Value) -> Value { dispatch(s, n, &a).unwrap_or_else(|e| panic!("{n}: {e}")) }
fn sid(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or_else(|| panic!("no {k} in {v}")).to_string() }

/// Same voice-like tone recipe as cut_to_speech.rs's `talk()` / director_small.rs's `talk()`: harmonics +
/// syllable-rate wobble inside `windows`, quiet noise floor everywhere else, so the VAD hears distinct
/// speech there and silence between.
fn talk(name: &str, dur: f64, windows: &[(f64, f64)]) -> String {
    let p = dir().join(name); if p.exists() { return p.to_string_lossy().into_owned(); }
    let gate: Vec<String> = windows.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
    let a = format!("aevalsrc='({})*0.5*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2)+0.05*sin(2*PI*1900*t))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={dur}", gate.join("+"));
    ffmpeg(&["-f", "lavfi", "-i", &a, "-f", "lavfi", "-i", &format!("anoisesrc=a=0.0015:d={dur}:r=48000"),
        "-filter_complex", "[0:a][1:a]amix=inputs=2:normalize=0[a]", "-map", "[a]", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}
/// A 2s black video track so the clip is a plain, ordinary media clip (video + audio), not audio-only.
fn video_with_audio(name: &str, dur: f64, windows: &[(f64, f64)]) -> String {
    let wav = talk(&format!("{name}.wav"), dur, windows);
    let p = dir().join(format!("{name}.mp4")); if p.exists() { return p.to_string_lossy().into_owned(); }
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=c=gray:s=160x90:r=25:d={dur}"), "-i", &wav,
        "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", p.to_str().unwrap()]);
    p.to_string_lossy().into_owned()
}

/// speech windows (0.5,1.5) and (2.4,3.4) over a 6s clip:
///   - leading silence  [0.0, 0.5)  -- 500ms before the first word (the "cold open" lead-in)
///   - interior silence [1.5, 2.4)  -- 900ms breath between the two phrases
///   - trailing silence [3.4, 6.0)  -- 2.6s after the last word (must never be touched here)
const DUR: f64 = 6.0;
const WINDOWS: [(f64, f64); 2] = [(0.5, 1.5), (2.4, 3.4)];

fn fixture() -> (Session, String, String) {
    let mut ss = Session::new();
    c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25}));
    let asset = sid(&c(&mut ss, "import_media", json!({"path": video_with_audio("tp-talk", DUR, &WINDOWS)})), "asset_id");
    let track = sid(&c(&mut ss, "add_track", json!({})), "track_id");
    let clip = sid(&c(&mut ss, "add_clip", json!({"track_id": &track, "source_type": "media", "asset_id": &asset, "start": 0.0, "duration": DUR})), "clip_id");
    (ss, track, clip)
}

/// Non-silent duration across the whole track's mix, via the same unpadded RMS silence probe
/// `tighten_pauses` itself uses (not VAD mode: VAD's own pad_ms/hangover_ms would inflate each speech
/// region and then MERGE once a tightened gap gets small, which moves the total non-linearly even
/// though the real underlying speech never changed -- the silence probe has no such padding, so
/// duration-minus-silence is a direct, undistorted measure of real content for a before/after compare).
/// Used as the "never cut mid-word" check: if a cut had landed inside real speech, this would drop.
fn total_speech_s(ss: &mut Session, track: &str) -> f64 {
    let r = c(ss, "audio_speech_intervals", json!({"target": {"track_id": track}, "mode": "silence", "threshold_db": -45.0, "min_silence_ms": 20.0}));
    let total = r["duration_s"].as_f64().unwrap();
    let silent: f64 = r["silence"].as_array().unwrap().iter().map(|s| s["duration"].as_f64().unwrap()).sum();
    total - silent
}
fn track_media_clips(ss: &Session, track: &str) -> Vec<Value> {
    let t = ss.project.tracks.iter().find(|t| t.id == track).unwrap();
    let mut v: Vec<Value> = t.clips.iter().map(|cl| json!({"id": cl.id, "start": cl.start, "duration": cl.duration})).collect();
    v.sort_by(|a, b| a["start"].as_f64().unwrap().partial_cmp(&b["start"].as_f64().unwrap()).unwrap());
    v
}
fn track_total_duration(ss: &Session, track: &str) -> f64 {
    track_media_clips(ss, track).iter().map(|c| c["start"].as_f64().unwrap() + c["duration"].as_f64().unwrap()).fold(0.0, f64::max)
}

#[test]
fn shrinks_the_interior_breath_and_the_opening_without_touching_speech_or_the_tail() {
    let (mut ss, track, _clip) = fixture();
    let speech_before = total_speech_s(&mut ss, &track);
    let duration_before = track_total_duration(&ss, &track);
    assert!((duration_before - DUR).abs() < 1e-6);

    let r = c(&mut ss, "tighten_pauses", json!({"track_id": &track}));

    // Both the opening (500ms -> ~80ms) and the interior breath (900ms -> ~120ms) are long enough past
    // their targets (min_gap_ms default 260ms) to be found and acted on; the 2.6s trailing silence must
    // never be touched (it isn't "between phrases", it's the handoff after the last word).
    assert_eq!(r["gaps_tightened"].as_u64().unwrap(), 2, "expected the opening gap and the interior breath, nothing else: {r}");
    let gaps = r["gaps"].as_array().unwrap();
    let leading: Vec<&Value> = gaps.iter().filter(|g| g["leading"].as_bool().unwrap()).collect();
    let interior: Vec<&Value> = gaps.iter().filter(|g| !g["leading"].as_bool().unwrap()).collect();
    assert_eq!(leading.len(), 1, "{r}");
    assert_eq!(interior.len(), 1, "{r}");

    // Known true gaps are 500ms and 900ms; the silence probe's own 100ms RMS window can shave a bit off
    // each measured edge, so compare with real but generous tolerance, not an exact match.
    let lead_before = leading[0]["before_ms"].as_f64().unwrap();
    let int_before = interior[0]["before_ms"].as_f64().unwrap();
    assert!((350.0..=520.0).contains(&lead_before), "leading gap before_ms out of range: {lead_before} in {r}");
    assert!((750.0..=920.0).contains(&int_before), "interior gap before_ms out of range: {int_before} in {r}");

    // The tool's own target pads (what it tried to leave behind).
    assert!((r["opening_pad_ms"].as_f64().unwrap() - 80.0).abs() < 1e-6);
    assert!((r["pad_ms"].as_f64().unwrap() - 120.0).abs() < 1e-6);

    // Removed roughly (500-80)+(900-120) = 1200ms, loosely (quantization both ways).
    let removed = r["removed_seconds"].as_f64().unwrap();
    assert!((0.85..=1.45).contains(&removed), "removed_seconds out of expected range: {removed} in {r}");

    // The track shrank by exactly what was removed; nothing silently added or lost elsewhere.
    let duration_after = track_total_duration(&ss, &track);
    assert!((duration_before - duration_after - removed).abs() < 1e-3, "track duration must shrink by exactly removed_seconds: before {duration_before} after {duration_after} removed {removed}");

    // NEVER CUT MID-WORD: total detected speech is unchanged within VAD noise -- if either cut had
    // landed inside a word, speech duration would have dropped by that much.
    let speech_after = total_speech_s(&mut ss, &track);
    assert!((speech_before - speech_after).abs() < 0.15, "speech duration must survive the tightening: before {speech_before:.3}s after {speech_after:.3}s");

    // The trailing 2.6s silence after the last word must still be there, untouched -- not shrunk, not split.
    let clips = track_media_clips(&ss, &track);
    assert_eq!(clips.len(), 2, "one interior cut on one original clip makes two surviving pieces: {clips:?}");
    let last = clips.last().unwrap();
    let last_end = last["start"].as_f64().unwrap() + last["duration"].as_f64().unwrap();
    assert!((last_end - duration_after).abs() < 1e-6);
    assert!(duration_after - last_end < 1e-6); // sanity: last clip really is the one ending the track

    // Breaths stay natural: re-probe the resulting mix for any REMAINING silence of 20ms or more; the
    // only one longer than ~300ms allowed is the trailing block at the very end (untouched by design).
    let probe_after = c(&mut ss, "audio_speech_intervals", json!({"target": {"track_id": &track}, "mode": "silence", "min_silence_ms": 20.0, "threshold_db": -45.0}));
    let silences = probe_after["silence"].as_array().unwrap().clone();
    let interior_after: Vec<f64> = silences.iter()
        .filter(|s| s["end"].as_f64().unwrap() < duration_after - 0.2) // exclude the trailing block
        .map(|s| s["duration"].as_f64().unwrap() * 1000.0)
        .collect();
    for &d in &interior_after {
        assert!(d <= 260.0, "an interior pause of {d}ms survived tightening (target was ~80-120ms): {silences:?}");
    }
}

#[test]
fn dry_run_reports_without_mutating_the_project() {
    let (mut ss, track, _clip) = fixture();
    let before = serde_json::to_value(&ss.project).unwrap();
    let r = c(&mut ss, "tighten_pauses", json!({"track_id": &track, "dry_run": true}));
    assert_eq!(r["gaps_tightened"].as_u64().unwrap(), 2, "{r}");
    assert!(r["removed_seconds"].as_f64().unwrap() > 0.5, "{r}");
    let after = serde_json::to_value(&ss.project).unwrap();
    assert_eq!(before, after, "dry_run must not touch the project");
}

#[test]
fn a_gap_already_at_or_under_the_target_pad_is_left_alone() {
    // speech windows with only a 150ms interior gap and a 60ms opening: both already at or under the
    // default targets (120ms / 80ms) once min_gap_ms (260ms default) is applied, and both are short
    // enough that silence_probe's own min_silence_ms floor filters them out entirely.
    let mut ss = Session::new();
    c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25}));
    let asset = sid(&c(&mut ss, "import_media", json!({"path": video_with_audio("tp-tight", 3.0, &[(0.06, 1.0), (1.15, 2.9)])})), "asset_id");
    let track = sid(&c(&mut ss, "add_track", json!({})), "track_id");
    c(&mut ss, "add_clip", json!({"track_id": &track, "source_type": "media", "asset_id": &asset, "start": 0.0, "duration": 3.0}));
    let before = serde_json::to_value(&ss.project).unwrap();
    let r = c(&mut ss, "tighten_pauses", json!({"track_id": &track}));
    assert_eq!(r["gaps_tightened"].as_u64().unwrap(), 0, "{r}");
    assert_eq!(r["removed_seconds"].as_f64().unwrap(), 0.0, "{r}");
    let after = serde_json::to_value(&ss.project).unwrap();
    assert_eq!(before, after, "nothing worth tightening must mean nothing touched: {r}");
}

#[test]
fn auto_calibrated_threshold_finds_the_gap_on_a_noisy_real_world_floor() {
    // Same 500ms/900ms/2.6s gap shape as the main test, but with a much louder background noise floor
    // (-22dB-ish, like a real interview's room tone) that a fixed default of -45dB would never see as
    // "silence" at all -- real_v15's own real-footage render found exactly zero gaps for this reason
    // before this test was added. With no threshold_db given, the tool must calibrate per clip from the
    // clip's own measured noise floor and still find the interior breath.
    let mut ss = Session::new();
    c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25}));
    let p = dir().join("tp-noisy.wav");
    if !p.exists() {
        let windows = [(0.5, 1.5), (2.4, 3.4)];
        let gate: Vec<String> = windows.iter().map(|(s, e)| format!("between(t,{s},{e})")).collect();
        let a = format!("aevalsrc='({})*0.5*(0.25*sin(2*PI*140*t)+0.15*sin(2*PI*280*t+1)+0.1*sin(2*PI*420*t+2)+0.05*sin(2*PI*1900*t))*(0.55+0.45*sin(2*PI*4.3*t))':s=48000:d={DUR}", gate.join("+"));
        ffmpeg(&["-f", "lavfi", "-i", &a, "-f", "lavfi", "-i", &format!("anoisesrc=a=0.02:d={DUR}:r=48000"),
            "-filter_complex", "[0:a][1:a]amix=inputs=2:normalize=0[a]", "-map", "[a]", p.to_str().unwrap()]);
    }
    let asset = sid(&c(&mut ss, "import_media", json!({"path": p.to_string_lossy()})), "asset_id");
    let track = sid(&c(&mut ss, "add_track", json!({})), "track_id");
    c(&mut ss, "add_clip", json!({"track_id": &track, "source_type": "media", "asset_id": &asset, "start": 0.0, "duration": DUR}));

    // The fixed old default would find nothing on this floor -- confirms the scenario is real, not moot.
    let pinned = c(&mut ss, "tighten_pauses", json!({"track_id": &track, "threshold_db": -45.0, "dry_run": true}));
    assert_eq!(pinned["gaps_tightened"].as_u64().unwrap(), 0, "sanity check that -45dB really is too strict for this floor: {pinned}");

    let r = c(&mut ss, "tighten_pauses", json!({"track_id": &track}));
    assert!(r["gaps_tightened"].as_u64().unwrap() >= 1, "auto-calibrated threshold should still find the interior breath on a noisy floor: {r}");
    assert!(r["removed_seconds"].as_f64().unwrap() > 0.3, "{r}");
}

#[test]
fn min_piece_s_refuses_a_cut_that_would_leave_too_short_a_fragment() {
    // Speech starts at 0 (no leading gap to worry about) so this is purely about the two INTERIOR gaps:
    // (0,1.0) speech, a 500ms gap, (1.5,2.1) a 0.6s speech island, a 500ms gap, (2.6,3.6) speech. Cutting
    // BOTH gaps would leave only that 0.6s island between them, under the default min_piece_s (1.0) --
    // this tool must refuse one of the two cuts rather than hand a 0.6s fragment to whatever reads clips
    // downstream (finish_audio's own dialogue/ambience classifier among them).
    let windows = [(0.0, 1.0), (1.5, 2.1), (2.6, 3.6)];
    let mut ss = Session::new();
    c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25}));
    let asset = sid(&c(&mut ss, "import_media", json!({"path": video_with_audio("tp-frag", 4.0, &windows)})), "asset_id");
    let track = sid(&c(&mut ss, "add_track", json!({})), "track_id");
    c(&mut ss, "add_clip", json!({"track_id": &track, "source_type": "media", "asset_id": &asset, "start": 0.0, "duration": 4.0}));

    let r = c(&mut ss, "tighten_pauses", json!({"track_id": &track, "min_piece_s": 1.0}));
    assert_eq!(r["gaps_tightened"].as_u64().unwrap(), 1, "exactly one of the two interior gaps should be cut, not both: {r}");

    // With the floor low enough (0.2, under the 0.6s island either way), both gaps are fine to cut.
    let mut ss2 = Session::new();
    c(&mut ss2, "project_set", json!({"width": 160, "height": 90, "fps": 25}));
    let asset2 = sid(&c(&mut ss2, "import_media", json!({"path": video_with_audio("tp-frag", 4.0, &windows)})), "asset_id");
    let track2 = sid(&c(&mut ss2, "add_track", json!({})), "track_id");
    c(&mut ss2, "add_clip", json!({"track_id": &track2, "source_type": "media", "asset_id": &asset2, "start": 0.0, "duration": 4.0}));
    let r2 = c(&mut ss2, "tighten_pauses", json!({"track_id": &track2, "min_piece_s": 0.2}));
    assert_eq!(r2["gaps_tightened"].as_u64().unwrap(), 2, "with the floor low enough both real gaps should be cut: {r2}");
}

#[test]
fn locked_track_is_rejected() {
    let (mut ss, track, _clip) = fixture();
    c(&mut ss, "set_track", json!({"track_id": &track, "locked": true}));
    let e = dispatch(&mut ss, "tighten_pauses", &json!({"track_id": &track})).unwrap_err();
    assert!(e.contains("locked"), "{e}");
}

#[test]
fn missing_track_defaults_to_the_one_with_the_most_media_clips() {
    let (mut ss, track, _clip) = fixture();
    let r = c(&mut ss, "tighten_pauses", json!({}));
    assert_eq!(r["track_id"].as_str().unwrap(), track, "{r}");
}
