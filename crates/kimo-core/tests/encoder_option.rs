//! `encoder` on export and export_preset: NVENC where the machine can do it, the software encoder where it cannot.
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};

fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }
fn project() -> Session {
    let mut ss = Session::new(); call(&mut ss, "project_set", json!({"width": 320, "height": 240, "fps": 25}));
    let t = call(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    call(&mut ss, "add_clip", json!({"track_id": t, "source_type": "solid", "color": [0.2, 0.4, 0.6], "start": 0, "duration": 0.4})); ss
}
fn out(n: &str) -> String { std::env::temp_dir().join(format!("kimo_enc_{}_{n}.mp4", std::process::id())).to_string_lossy().into_owned() }
fn codec(p: &str) -> String { let o = std::process::Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0", p]).output().unwrap(); String::from_utf8_lossy(&o.stdout).trim().to_string() }

#[test] fn default_export_says_it_used_libx264() { let mut ss = project(); let p = out("def"); let r = call(&mut ss, "export", json!({"path": p})); assert_eq!(r["encoder"], "libx264", "{r}"); assert_eq!(codec(&p), "h264"); }

#[test] fn unknown_encoder_is_refused() { let mut ss = project(); let e = dispatch(&mut ss, "export", &json!({"path": out("bad"), "encoder": "h265_magic"})).unwrap_err(); assert!(e.contains("encoder"), "{e}"); }

/// Whatever the GPU can do, the file exists, and the result says what really encoded it.
#[test] fn nvenc_request_exports_with_nvenc_or_falls_back_and_says_so() {
    for (enc, want) in [("h264_nvenc", "h264"), ("hevc_nvenc", "hevc")] {
        let mut ss = project(); let p = out(enc); let r = call(&mut ss, "export", json!({"path": p, "encoder": enc}));
        let used = r["encoder"].as_str().unwrap_or_else(|| panic!("no encoder in {r}")); let c = codec(&p);
        if used == enc { assert_eq!(c, want, "{r}"); assert!(r.get("encoder_fallback").is_none(), "{r}"); }
        else { assert_eq!(used, "libx264", "{r}"); assert_eq!(c, "h264", "{r}"); assert!(r["encoder_fallback"].as_str().unwrap_or("").contains(enc), "a fallback must say what failed: {r}"); }
    }
}

#[test] fn presets_take_the_encoder_option() {
    let mut ss = project(); let p = out("preset"); let r = call(&mut ss, "export_preset", json!({"path": p, "preset": "web-1080", "encoder": "h264_nvenc"}));
    assert!(r["encoder"].is_string(), "{r}"); assert_eq!(codec(&p), "h264");
    let e = dispatch(&mut ss, "export_preset", &json!({"path": out("x").replace(".mp4", ".mov"), "preset": "prores422", "encoder": "h264_nvenc"})).unwrap_err(); assert!(e.contains("encoder"), "{e}");
}
