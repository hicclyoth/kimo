//! check_environment: one read-only call that says whether this machine can run a director demo: the whisper.cpp recognizer
//! and a model for captions, the title preset fonts, and ffmpeg with libx264 for export. Every gap comes with the exact
//! command that fixes it. Nothing is downloaded, installed or written.
use super::kit::*;
use std::path::{Path, PathBuf};

/// The fonts the title presets draw with (titles.rs); a missing one is silently drawn with a fallback.
const PRESET_FONTS: [&str; 5] = ["Anton", "BebasNeue", "CormorantGaramond", "Montserrat", "Syne"];
/// Model files in the order auto_captions picks them.
const MODELS: [&str; 4] = ["ggml-large-v3-turbo-q5_0.bin", "ggml-small-q5_1.bin", "ggml-small.bin", "ggml-base-q5_1.bin"];
const FIX_WHISPER: &str = "bash tools/whisper/fetch.sh   # in the kimo repo (Windows: from Git Bash): builds whisper-cli and downloads the small model (~200 MB) into ~/.cache/kimo_models; or set KIMO_WHISPER_BIN and KIMO_WHISPER_MODEL";
const FIX_FONTS: &str = if cfg!(windows) { "powershell -ExecutionPolicy Bypass -File fonts/fetch.ps1   # in the kimo repo; kimo finds them next to its build, or set KIMO_FONTS to that folder (also in the MCP server's env)" } else { "bash fonts/fetch.sh && export KIMO_FONTS=\"$PWD/fonts\"   # in the kimo repo; put KIMO_FONTS in the MCP server's env too" };
const FIX_FFMPEG: &str = if cfg!(windows) { "winget install Gyan.FFmpeg   # needs an ffmpeg built with libx264" } else { "sudo apt-get install -y ffmpeg   # macOS: brew install ffmpeg (needs an ffmpeg built with libx264)" };
const FIX_FALLBACK: &str = if cfg!(windows) { "restore arial.ttf in C:\\Windows\\Fonts   # the fallback font kimo draws with when a font is missing" } else { "sudo apt-get install -y fonts-dejavu-core   # the fallback font kimo draws with when a font is missing" };

fn models_dir() -> PathBuf { std::env::var_os("KIMO_MODELS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/kimo_models")) }
fn on_path(bin: &str) -> Option<PathBuf> { std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(bin)).find(|p| p.is_file()) }
fn file_row(p: Option<PathBuf>, via: &str) -> Value {
    match p { Some(p) => { let ok = p.is_file(); json!({"present": ok, "path": p.to_string_lossy(), "via": via, "bytes": std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)}) } None => json!({"present": false, "via": via}) }
}
fn whisper() -> Value {
    let bin = match std::env::var_os("KIMO_WHISPER_BIN") { Some(p) => file_row(Some(p.into()), "KIMO_WHISPER_BIN"), None => { let local = models_dir().join("bin/whisper-cli");
        if local.is_file() { file_row(Some(local), "models dir") } else { file_row(on_path("whisper-cli"), "PATH") } } };
    let model = match std::env::var_os("KIMO_WHISPER_MODEL") { Some(p) => file_row(Some(p.into()), "KIMO_WHISPER_MODEL"), None => { let d = models_dir(); file_row(MODELS.iter().map(|f| d.join(f)).find(|p| p.is_file()), "models dir") } };
    let ready = bin["present"] == true && model["present"] == true;
    json!({"ready": ready, "binary": bin, "model": model, "models_dir": models_dir().to_string_lossy(),
        "transcribe_cli": {"present": on_path("whisper").is_some(), "note": "the transcribe tool's openai-whisper CLI (optional; captions use whisper-cli)"}})
}
fn ffmpeg() -> Value {
    let info = crate::buildinfo::info(); let f = info["backends"]["ffmpeg"].clone(); let avail = f["available"] == true;
    let x264 = avail && capture(std::process::Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).stdin(std::process::Stdio::null()), 1 << 20, std::time::Duration::from_secs(10))
        .map(|o| String::from_utf8_lossy(&o).contains("libx264")).unwrap_or(false);
    json!({"available": avail, "version": f["version"].as_str().unwrap_or(""), "libx264": x264, "ffprobe": info["backends"]["ffprobe"]["available"] == true})
}
fn fonts() -> Value {
    let presets: Vec<Value> = PRESET_FONTS.iter().map(|f| { let (file, sub) = crate::text::resolve_font_info(f); if sub { json!({"font": f, "found": false, "would_draw_with": file}) } else { json!({"font": f, "found": true, "path": file}) } }).collect();
    let fallback = crate::text::fallback_font();
    json!({"ready": presets.iter().all(|p| p["found"] == true), "presets": presets, "fallback": {"present": Path::new(&fallback).is_file(), "path": fallback},
        "searched": crate::text::font_dirs().iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>()})
}
fn check(_ss: &mut Session, _a: &Value) -> Result<Value, String> {
    let (w, f, ff) = (whisper(), fonts(), ffmpeg());
    let mut fixes: Vec<&str> = vec![];
    if ff["available"] != true || ff["libx264"] != true || ff["ffprobe"] != true { fixes.push(FIX_FFMPEG); }
    if w["ready"] != true { fixes.push(FIX_WHISPER); }
    if f["ready"] != true { fixes.push(FIX_FONTS); }
    if f["fallback"]["present"] != true && f["ready"] != true { fixes.push(FIX_FALLBACK); }
    Ok(json!({"ok": fixes.is_empty(), "ffmpeg": ff, "whisper": w, "fonts": f, "fixes": fixes, "downloads": "none (read-only check)"}))
}
pub fn commands() -> Vec<Command> { vec![
    Command { name: "check_environment", description: "Demo-machine check, read-only, no downloads: whisper-cli + model, title fonts, ffmpeg + libx264. Each gap lists its exact fix command.", mutates: false,
        schema: obj(vec![], &[]), run: check },
] }
