//! Lazy, disposable analysis-only proxy for long-GOP source media.
//!
//! `render::decode()` always issues `-ss {clip.in_point} -t {span}` and reaches the requested
//! frame by decoding-and-discarding everything in between (see render.rs). For sequential/export
//! access this is fine (the window usually already covers the next request). For *random-access*
//! sampling deep into a clip - get_scopes/lint_edit's picture scan, lint_frame, scene_split - the
//! cost grows with how far into the clip the sample sits, and on media whose own keyframes are
//! sparse (phone footage is often one keyframe every several seconds, sometimes only one for the
//! whole clip) there is no cheaper point in the ORIGINAL file to seek to.
//!
//! This module builds, once per asset (cached by source path + size + mtime + project fps), a
//! visually near-lossless copy of the asset re-encoded with a short, fixed GOP (`-g`, `-sc_threshold
//! 0`, so keyframes land exactly every `gop` frames with no adaptive scene-cut insertion). Analysis
//! decode (render.rs, when asked) can then seek to the nearest preceding keyframe of *this* proxy
//! instead of the clip's fixed in_point, bounding the decode-and-discard cost by `gop` frames
//! instead of by how deep the sample is.
//!
//! Never used by export: export always resolves the original asset path and never looks at this
//! cache. Never mutates the project or the source media; the proxy is pure cache, safe to delete.
use crate::model::Project;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A ready, cached analysis proxy for one asset.
pub struct AnalysisProxy {
    pub path: String,
    pub gop: usize,
    pub fps: f64,
}

/// Minimum native max-keyframe-gap (seconds) before building a proxy is worth it at all.
const MIN_GAP_SECONDS: f64 = 2.0;
/// Target keyframe spacing in the proxy itself.
const GOP_SECONDS: f64 = 0.5;

/// Disk cache folder: KIMO_ANALYSIS_PROXY_CACHE ("off" = disabled), else <project dir>/.kimo_cache/analysis_proxy,
/// else the temp dir. Mirrors codeclip::cache_dir's convention.
fn cache_dir(p: &Project) -> Option<PathBuf> {
    match std::env::var("KIMO_ANALYSIS_PROXY_CACHE") {
        Ok(v) if v == "off" || v == "0" => return None,
        Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
        _ => {}
    }
    Some(match &p.base_dir {
        Some(b) => b.join(".kimo_cache").join("analysis_proxy"),
        None => std::env::temp_dir().join("kimo_analysis_proxy_cache"),
    })
}

fn stamp_key(meta: &std::fs::Metadata, fps: f64) -> String {
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}-{:.6}", meta.len(), mtime_ns, fps)
}

fn proxy_filename(source_path: &str, key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source_path.hash(&mut h);
    key.hash(&mut h);
    format!("{:016x}.mp4", h.finish())
}

fn sidecar_path(proxy_path: &Path) -> PathBuf {
    proxy_path.with_extension("json")
}

/// Probe the source's own keyframe times and return the largest gap between consecutive ones,
/// including the lead-in from 0 and the tail to `duration`. ffprobe-only; never writes anything.
fn max_keyframe_gap(path: &str, duration: f64) -> Result<f64, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error", "-select_streams", "v:0", "-show_entries", "frame=key_frame,pkt_dts_time",
            "-of", "default=noprint_wrappers=1:nokey=1", path,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let mut times: Vec<f64> = vec![0.0];
    while let (Some(kf), Some(t)) = (lines.next(), lines.next()) {
        if kf.trim() == "1" {
            if let Ok(v) = t.trim().parse::<f64>() {
                times.push(v);
            }
        }
    }
    let tail = duration.max(*times.last().unwrap_or(&0.0));
    times.push(tail);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut max_gap = 0.0f64;
    for w in times.windows(2) {
        max_gap = max_gap.max(w[1] - w[0]);
    }
    Ok(max_gap)
}

/// Lazily build (if missing or stale) and return a short-GOP analysis proxy for one video asset,
/// when its own keyframe spacing is long enough to make random-access decode expensive. Returns
/// None (never an error) for anything that makes the fast path unavailable: stills, disabled cache,
/// an asset whose keyframes are already frequent, or any ffprobe/ffmpeg failure - callers always
/// fall back to decoding the original, exactly as before this module existed.
pub fn ensure(p: &Project, asset_id: &str, original_path: &str) -> Option<AnalysisProxy> {
    if crate::render::is_still_path(original_path) || original_path.ends_with(".kimopx") {
        return None;
    }
    if std::env::var("KIMO_ANALYSIS_PROXY").ok().as_deref() == Some("off") {
        return None;
    }
    let fps = p.fps;
    if !(1.0..=240.0).contains(&fps) {
        return None;
    }
    let dir = cache_dir(p)?;
    let meta = std::fs::metadata(original_path).ok()?;
    let key = stamp_key(&meta, fps);
    let fname = proxy_filename(original_path, &key);
    let proxy_path = dir.join(&fname);
    let side = sidecar_path(&proxy_path);
    if proxy_path.is_file() {
        if let Ok(j) = std::fs::read_to_string(&side) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&j) {
                if v.get("source_key").and_then(|x| x.as_str()) == Some(key.as_str()) {
                    if let Some(gop) = v.get("gop").and_then(|x| x.as_u64()) {
                        return Some(AnalysisProxy { path: proxy_path.to_string_lossy().into_owned(), gop: gop as usize, fps });
                    }
                }
            }
        }
        // Stale/corrupt sidecar for this key: fall through and rebuild cleanly.
    }
    let asset = p.assets.get(asset_id)?;
    if asset.width == 0 {
        return None; // audio only: nothing to proxy, and ffmpeg would fail on every frame
    }
    let gap = max_keyframe_gap(original_path, asset.duration).ok()?;
    if gap <= MIN_GAP_SECONDS {
        return None; // already cheap to seek on the original; a proxy would only cost disk and time
    }
    std::fs::create_dir_all(&dir).ok()?;
    let gop = ((fps * GOP_SECONDS).round() as usize).max(1);
    let tmp = proxy_path.with_extension("mp4.part");
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-i", original_path,
            "-vf", &format!("fps={fps}"),
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "14",
            "-g", &gop.to_string(), "-keyint_min", &gop.to_string(), "-sc_threshold", "0",
            "-pix_fmt", "yuv420p", "-an", "-f", "mp4",
        ])
        .arg(&tmp)
        .status()
        .ok()?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    if std::fs::rename(&tmp, &proxy_path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    let _ = std::fs::write(&side, serde_json::json!({"source_key": key, "gop": gop, "fps": fps}).to_string());
    Some(AnalysisProxy { path: proxy_path.to_string_lossy().into_owned(), gop, fps })
}

/// The nearest-preceding-keyframe seek point for `start` (a clip-relative frame index, same
/// indexing as render::decode's `a`): `(seek_time_seconds, frame_index_at_that_seek_point)`.
/// `frame_index_at_that_seek_point` is clip-relative too (can be negative when the clip's own
/// in_point is not itself gop-aligned; render::decode handles that with signed arithmetic) and is
/// always <= `start`, so the caller only ever decodes forward.
pub fn seek_base_for(ap: &AnalysisProxy, c: &crate::model::Clip, start: usize) -> Option<(f64, i64)> {
    if ap.gop == 0 {
        return None;
    }
    let in_point_frames = (c.in_point * ap.fps).round() as i64;
    let abs_frame = in_point_frames + start as i64;
    if abs_frame < 0 {
        return None;
    }
    let gop = ap.gop as i64;
    let kf_abs = (abs_frame / gop) * gop;
    let frame0 = kf_abs - in_point_frames;
    let seek_time = (kf_abs as f64 / ap.fps).max(0.0);
    Some((seek_time, frame0))
}

/// Prune this process's own cached proxies (and sidecars) down to a byte budget, oldest-used first.
/// KIMO_ANALYSIS_PROXY_CACHE_MAX_MB (default 2048). A hit is not currently timestamped as a use
/// (unlike codeclip's LRU); this is a simple size cap, not a recency cap, intentionally: build cost
/// dominates reuse cost here, so naive mtime-order (oldest build first) is an adequate first cut.
pub fn prune_cache(dir: &Path, max_bytes: u64) -> (usize, u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return (0, 0) };
    let mut v: Vec<(std::time::SystemTime, u64, PathBuf)> = vec![];
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.len() == 20 && name.ends_with(".mp4") && name[..16].chars().all(|c| c.is_ascii_hexdigit())) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let mt = md.modified().unwrap_or(std::time::UNIX_EPOCH);
        v.push((mt, md.len(), e.path()));
    }
    let mut total: u64 = v.iter().map(|x| x.1).sum();
    v.sort();
    let mut left = v.len();
    for (_, n, path) in &v {
        if total <= max_bytes {
            break;
        }
        let side = sidecar_path(path);
        if std::fs::remove_file(path).is_ok() {
            let _ = std::fs::remove_file(&side);
            total -= n;
            left -= 1;
        }
    }
    (left, total)
}

pub fn cache_max_bytes() -> u64 {
    std::env::var("KIMO_ANALYSIS_PROXY_CACHE_MAX_MB").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(2048) * 1024 * 1024
}
