//! RV-20 (e): classified failures. A fake `ffmpeg` first on PATH fails the encode of chunk 1 (then chunk 2 with a disk-full message); the job fails with a
//! classified error, keeps the finished chunk and its index entry, and resume_job completes once the fault is gone. One test: it owns PATH.
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};
fn c(ss: &mut Session, n: &str, v: Value) -> Value { dispatch(ss, n, &v).unwrap_or_else(|e| panic!("{n}: {e}")) }
fn scene() -> Session {
    let mut ss = Session::new(); c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25})); let t = c(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    for i in 0..3 { c(&mut ss, "add_shape", json!({"track_id": t, "shape": "rect", "width": 20 + i * 3, "height": 20, "start": i as f64 * 2.0, "duration": 2.0})); } ss
}
#[cfg(unix)] // ffmpeg is a sh shim here
#[test] fn encoder_failure_and_disk_full_are_classified_keep_finished_chunks_and_resume_completes() {
    use kimo_core::ext::export_job::classify;
    assert!(classify("write failed: No space left on device (os error 28)").starts_with("disk_full:")); assert!(classify("ffmpeg exited with status 1").starts_with("encoder_failed:")); assert_eq!(classify("cancelled"), "cancelled");
    let d = std::env::temp_dir().join(format!("kimo_rv20e_{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(d.join("bin")).unwrap();
    let real = String::from_utf8(std::process::Command::new("which").arg("ffmpeg").output().unwrap().stdout).unwrap().trim().to_string();
    let fault = d.join("fault"); let shim = d.join("bin/ffmpeg");
    std::fs::write(&shim, format!("#!/bin/sh\nF=$(cat {f} 2>/dev/null)\nfor a in \"$@\"; do case \"$a\" in *chunk_00001.partial.mp4) if [ \"$F\" = enc ]; then echo 'x264 [error]: could not open codec' >&2; echo 'Conversion failed!' >&2; exit 1; fi;; *.joining.tmp) if [ \"$F\" = joindisk ]; then echo 'av_interleaved_write_frame(): No space left on device' >&2; exit 1; fi; if [ \"$F\" = joinenc ]; then echo 'Conversion failed!' >&2; exit 1; fi;; *chunk_00002.partial.mp4) if [ \"$F\" = disk ]; then cat >/dev/null; echo 'av_interleaved_write_frame(): No space left on device' >&2; exit 1; fi;; esac; done\nexec {r} \"$@\"\n", f = fault.display(), r = real)).unwrap();
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap(); }
    let old = std::env::var("PATH").unwrap(); std::env::set_var("PATH", format!("{}:{old}", d.join("bin").display()));
    let out = d.join("out.mp4"); let a = json!({"path": out.to_str().unwrap(), "chunk_s": 2, "wait": true}); let mut ss = scene();
    std::fs::write(&fault, "enc").unwrap(); let s = c(&mut ss, "export_job_start", a.clone())["status"].clone();
    assert_eq!(s["state"], "failed", "{s}"); let e = s["error"].as_str().unwrap(); assert!(e.starts_with("encoder_failed:"), "{e}"); assert!(e.contains("Conversion failed"), "stderr tail kept: {e}");
    assert_eq!(s["completed_ranges"], json!([[0, 50]]), "the finished chunk is preserved"); assert!(!out.exists(), "no output from a failed job");
    let idx: Value = serde_json::from_slice(&std::fs::read(d.join("out.mp4.chunks/index.json")).unwrap()).unwrap(); assert!(idx["chunks"]["0"]["sha256"].is_string() && idx["chunks"].get("1").is_none());
    assert!(!d.join("out.mp4.chunks/job.lock").exists(), "lock released after a failure");
    std::fs::write(&fault, "disk").unwrap(); let s = c(&mut ss, "resume_job", json!({"path": out.to_str().unwrap(), "wait": true})); let s = s["status"].clone();
    assert_eq!(s["state"], "failed", "{s}"); assert!(s["error"].as_str().unwrap().starts_with("disk_full:"), "{s}"); assert_eq!(s["completed_ranges"], json!([[0, 50], [50, 100]]), "{s}");
    std::fs::write(&fault, "none").unwrap(); let s = c(&mut ss, "resume_job", json!({"path": out.to_str().unwrap(), "wait": true}))["status"].clone();
    assert_eq!(s["state"], "done", "{s}"); assert_eq!(s["result"]["reverified"]["ok"], 2, "both earlier chunks were verified and reused: {s}"); assert_eq!(s["result"]["verified"], true);
    // join stage: ENOSPC and a non-zero encoder exit both discard the temp file, leave no output and no lock, keep every chunk, and resume completes
    let out2 = d.join("out2.mp4"); let a2 = json!({"path": out2.to_str().unwrap(), "chunk_s": 2, "wait": true}); let ch2 = d.join("out2.mp4.chunks");
    std::fs::write(&fault, "joindisk").unwrap(); let s = c(&mut ss, "export_job_start", a2.clone())["status"].clone();
    assert_eq!(s["state"], "failed", "{s}"); assert!(s["error"].as_str().unwrap().starts_with("disk_full:"), "{s}"); assert_eq!(s["completed_ranges"], json!([[0, 50], [50, 100], [100, 150]]));
    assert!(!out2.exists() && !d.join("out2.mp4.joining.tmp").exists() && !ch2.join("job.lock").exists(), "no output, no temp file, no lock");
    std::fs::write(&fault, "joinenc").unwrap(); let s = c(&mut ss, "resume_job", json!({"path": out2.to_str().unwrap(), "wait": true}))["status"].clone();
    assert_eq!(s["state"], "failed", "{s}"); assert!(s["error"].as_str().unwrap().starts_with("encoder_failed:"), "{s}"); assert_eq!(s["result"], Value::Null); assert!(!out2.exists() && !d.join("out2.mp4.joining.tmp").exists() && !ch2.join("job.lock").exists());
    std::fs::write(&fault, "none").unwrap(); let s = c(&mut ss, "resume_job", json!({"path": out2.to_str().unwrap(), "wait": true}))["status"].clone();
    assert_eq!(s["state"], "done", "{s}"); assert_eq!(s["result"]["reverified"]["ok"], 3, "{s}"); assert_eq!(s["result"]["verified"], true);
    std::env::set_var("PATH", old); std::fs::remove_dir_all(&d).ok();
}
