//! RV-20 (f): real SIGKILL tests. The test binary re-runs itself as a child process that owns the job (ffmpeg is a slow shim in the child's PATH so a kill
//! reliably lands mid-encode or mid-join); the parent kills the whole process group with SIGKILL, then a fresh process (this one) resumes.
#![cfg(unix)] // needs sh shims and process-group SIGKILL
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
fn c(ss: &mut Session, n: &str, v: Value) -> Value { dispatch(ss, n, &v).unwrap_or_else(|e| panic!("{n}: {e}")) }
fn scene() -> Session {
    let mut ss = Session::new(); c(&mut ss, "project_set", json!({"width": 160, "height": 90, "fps": 25})); let t = c(&mut ss, "add_track", json!({}))["track_id"].as_str().unwrap().to_string();
    for i in 0..3 { c(&mut ss, "add_shape", json!({"track_id": t, "shape": "rect", "width": 20 + i * 3, "height": 20, "start": i as f64 * 2.0, "duration": 2.0})); } ss
}
fn tmp(tag: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("kimo_rv20f_{tag}_{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap(); d }
fn args(out: &Path) -> Value { json!({"path": out.to_str().unwrap(), "chunk_s": 2, "wait": true}) }
/// Runs only when spawned by `spawn_child`: executes the job in this (soon to be killed) process.
#[test] fn child_job_runner() {
    let Ok(out) = std::env::var("KIMO_CHILD_OUT") else { return }; let mut ss = scene(); let _ = dispatch(&mut ss, "export_job_start", &args(Path::new(&out)));
}
/// ffmpeg shim: sleeps 20 s before the encode of chunk K (arg contains chunk_0000K.partial.mp4) or before the join (arg contains .joining.tmp), then runs the real one.
fn shim(d: &Path) -> PathBuf {
    let bin = d.join("bin"); std::fs::create_dir_all(&bin).unwrap(); let real = String::from_utf8(std::process::Command::new("which").arg("ffmpeg").output().unwrap().stdout).unwrap().trim().to_string();
    let p = bin.join("ffmpeg"); std::fs::write(&p, format!("#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in *\"$KIMO_SLOW\"*) sleep 20;; esac; done\nexec {real} \"$@\"\n")).unwrap();
    use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap(); bin
}
fn spawn_child(d: &Path, out: &Path, slow: &str) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    let path = format!("{}:{}", shim(d).display(), std::env::var("PATH").unwrap());
    std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "child_job_runner", "--test-threads=1", "--nocapture"]).env("KIMO_CHILD_OUT", out).env("KIMO_SLOW", slow).env("PATH", path).process_group(0).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap()
}
fn sigkill_group(ch: &mut std::process::Child) { let pid = ch.id(); let _ = std::process::Command::new("kill").args(["-9", "--", &format!("-{pid}")]).status(); let _ = ch.wait(); }
fn wait_for(p: &Path) { for _ in 0..600 { if p.exists() { return; } std::thread::sleep(std::time::Duration::from_millis(50)); } panic!("timed out waiting for {}", p.display()); }
fn framemd5(p: &Path) -> Vec<String> { let o = std::process::Command::new("ffmpeg").args(["-v", "error", "-i"]).arg(p).args(["-map", "0:v:0", "-f", "framemd5", "-"]).output().unwrap(); String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with('#')).map(|l| l.split(',').last().unwrap().trim().to_string()).collect() }
fn uninterrupted(tag: &str) -> Vec<String> { let d = tmp(tag); let mut ss = scene(); let out = d.join("ref.mp4"); c(&mut ss, "export_job_start", args(&out)); let v = framemd5(&out); std::fs::remove_dir_all(&d).ok(); v }

#[test] fn sigkill_mid_encode_at_several_chunks_then_a_fresh_process_resumes_to_identical_frames() {
    let reference = uninterrupted("ref"); assert_eq!(reference.len(), 150);
    for k in [1u64, 2] {
        let d = tmp(&format!("enc{k}")); let out = d.join("out.mp4"); let ch = d.join("out.mp4.chunks");
        let mut child = spawn_child(&d, &out, &format!("chunk_{k:05}.partial.mp4")); wait_for(&ch.join(format!("chunk_{:05}.mp4", k - 1))); std::thread::sleep(std::time::Duration::from_millis(600)); sigkill_group(&mut child);
        assert!(!out.exists(), "no final file after a kill"); assert!(ch.join("job.lock").is_file(), "SIGKILL leaves a stale lock");
        let kept: Vec<Vec<u8>> = (0..k).map(|i| std::fs::read(ch.join(format!("chunk_{i:05}.mp4"))).unwrap()).collect();
        let mut ss = scene(); let s = c(&mut ss, "export_job_start", args(&out))["status"].clone(); // fresh process: reclaims the stale lock
        assert_eq!(s["state"], "done", "k={k}: {s}"); assert_eq!(s["result"]["verified"], true); assert_eq!(s["result"]["reverified"]["ok"], k, "verified chunks are reused: {s}");
        let re: Vec<u64> = s["result"]["rendered"].as_array().unwrap().iter().map(|r| r["index"].as_u64().unwrap()).collect(); assert_eq!(re, (k..3).collect::<Vec<_>>(), "the killed chunk and later ones re-render: {s}");
        for i in 0..k { assert_eq!(kept[i as usize], std::fs::read(ch.join(format!("chunk_{i:05}.mp4"))).unwrap(), "reused chunk untouched"); }
        assert_eq!(framemd5(&out), reference, "decoded per-frame hashes equal an uninterrupted run (k={k})"); assert!(!ch.join("job.lock").exists());
        assert!(std::fs::read_dir(&ch).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".partial.mp4")), "no orphaned encode temp left in the chunk dir");
        std::fs::remove_dir_all(&d).ok();
    }
}
#[test] fn a_second_process_on_a_live_directory_is_refused() {
    let d = tmp("live"); let out = d.join("out.mp4"); let ch = d.join("out.mp4.chunks"); let mut child = spawn_child(&d, &out, "chunk_00001.partial.mp4"); wait_for(&ch.join("chunk_00000.mp4"));
    let mut ss = scene(); let e = dispatch(&mut ss, "export_job_start", &args(&out)).unwrap_err(); assert!(e.starts_with("job_locked:"), "{e}");
    let e = dispatch(&mut ss, "resume_job", &json!({"path": out.to_str().unwrap(), "wait": true})).unwrap_err(); assert!(e.starts_with("job_locked:"), "{e}");
    sigkill_group(&mut child); std::fs::remove_dir_all(&d).ok();
}
#[test] fn sigkill_mid_join_leaves_no_final_file_and_the_orphaned_temp_is_cleaned_on_rerun() {
    let reference = uninterrupted("ref2"); let d = tmp("join"); let out = d.join("out.mp4"); let ch = d.join("out.mp4.chunks");
    let mut child = spawn_child(&d, &out, ".joining.tmp"); wait_for(&ch.join("chunk_00002.mp4")); wait_for(&ch.join("index.json")); std::thread::sleep(std::time::Duration::from_millis(1500)); // all chunks done, ffmpeg join is sleeping in the shim
    sigkill_group(&mut child); assert!(!out.exists(), "no final file after a kill during the join"); assert!(ch.join("job.lock").is_file());
    let tmpf = d.join("out.mp4.joining.tmp"); std::fs::write(&tmpf, b"half a join").unwrap(); // orphaned temp (a kill after ffmpeg started would leave one)
    let mut ss = scene(); let s = c(&mut ss, "export_job_start", args(&out))["status"].clone(); assert_eq!(s["state"], "done", "{s}"); assert_eq!(s["result"]["reverified"]["ok"], 3, "{s}"); assert_eq!(s["result"]["verified"], true);
    assert!(!tmpf.exists() && !d.join("out.mp4.joining.wav").exists() && !d.join("out.mp4.joining.txt").exists(), "orphaned join temp files are gone"); assert_eq!(framemd5(&out), reference); std::fs::remove_dir_all(&d).ok();
}
#[test] fn cancel_then_resume_completes_with_the_finished_chunks_reused() {
    let d = tmp("cancel"); let out = d.join("out.mp4"); let ch = d.join("out.mp4.chunks"); let mut ss = scene();
    let a = json!({"path": out.to_str().unwrap(), "chunk_s": 2}); let r = c(&mut ss, "export_job_start", a); let id = r["job_id"].as_str().unwrap().to_string();
    wait_for(&ch.join("chunk_00000.mp4")); let st = c(&mut ss, "cancel_job", json!({"job_id": id})); assert!(matches!(st["state"].as_str().unwrap(), "cancelled" | "done"), "{st}");
    assert!(!ch.join("job.lock").exists(), "a job that reports {} has released job.lock, so resume_job can take it at once", st["state"]);
    let s = c(&mut ss, "resume_job", json!({"path": out.to_str().unwrap(), "wait": true})); let s = if s.get("status").is_some() { s["status"].clone() } else { s };
    assert_eq!(s["state"], "done", "{s}"); assert_eq!(s["result"]["verified"], true); assert!(s["result"]["reverified"]["ok"].as_u64().unwrap() >= 1, "{s}"); assert!(!ch.join("job.lock").exists()); std::fs::remove_dir_all(&d).ok();
}
