//! Optional, disposable preview media. Asset paths always refer to the originals.
use crate::model::Project;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Proxies {
    pub enabled: bool,
    pub entries: BTreeMap<String, Proxy>,
}
impl Proxies {
    pub fn is_empty(&self) -> bool {
        !self.enabled && self.entries.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub bytes: u64,
    pub modified_ns: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fingerprint {
    pub stamp: Stamp,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proxy {
    pub path: String,
    pub source: Fingerprint,
    pub proxy: Fingerprint,
    pub source_width: u32,
    pub source_height: u32,
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frames: usize,
}
pub fn stamp(path: &Path) -> Result<Stamp, String> {
    let m = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !m.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    let t = m
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok(Stamp {
        bytes: m.len(),
        modified_ns: t.as_nanos().to_string(),
    })
}
/// Deliberately uncached: explicit deep validation also detects edits preserving size and mtime.
pub fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    use std::io::Read;
    let before = stamp(path)?;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = crate::deps::Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        crate::guard::check();
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    if stamp(path)? != before {
        return Err("media changed while hashing; retry".into());
    }
    Ok(Fingerprint {
        stamp: before,
        sha256: hash.finish_hex(),
    })
}
pub fn status(p: &Project, id: &str, deep: bool) -> Value {
    let Some(a) = p.assets.get(id) else {
        return json!({"asset_id":id,"status":"asset_missing"});
    };
    let Some(q) = p.proxies.entries.get(id) else {
        return json!({"asset_id":id,"status":"unregistered"});
    };
    let check = |path: &Path, f: &Fingerprint, missing: &str, changed: &str| -> Option<String> {
        if deep {
            match fingerprint(path) {
                Ok(now) if now.sha256 == f.sha256 => None,
                Ok(_) => Some(changed.into()),
                Err(_) if !path.is_file() => Some(missing.into()),
                Err(e) => Some(format!("validation_failed: {e}")),
            }
        } else {
            match stamp(path) {
                Ok(s) if s == f.stamp => None,
                Ok(_) => Some(changed.into()),
                Err(_) => Some(missing.into()),
            }
        }
    };
    let state = check(
        Path::new(&p.resolve(&a.path)),
        &q.source,
        "source_missing",
        "source_changed",
    )
    .or_else(|| {
        check(
            Path::new(&p.resolve(&q.path)),
            &q.proxy,
            "proxy_missing",
            "proxy_changed",
        )
    })
    .unwrap_or_else(|| "ready".into());
    let fingerprints_current = stamp(Path::new(&p.resolve(&a.path))).ok().as_ref()
        == Some(&q.source.stamp)
        && stamp(Path::new(&p.resolve(&q.path))).ok().as_ref() == Some(&q.proxy.stamp);
    let eligible = fingerprints_current
        && state == "ready"
        && !p.color_pipeline.enabled
        && a.width == q.source_width
        && a.height == q.source_height;
    json!({"asset_id":id,"path":q.path,"status":state,"deep":deep,"eligible":eligible,
        "fingerprints_current":fingerprints_current,"requires_relink":state == "ready" && !fingerprints_current,
        "preview_active":eligible && p.proxies.enabled,"managed_color_bypass":p.color_pipeline.enabled,
        "width":q.width,"height":q.height,"frames":q.frames,"fps_num":q.fps_num,"fps_den":q.fps_den,
        "source_bytes":q.source.stamp.bytes,"proxy_bytes":q.proxy.stamp.bytes})
}
/// Fast preview checks use stat fingerprints. Export and analysis never call this selector.
pub fn preview_path(p: &Project, id: &str) -> Option<String> {
    if !p.proxies.enabled || p.color_pipeline.enabled {
        return None;
    }
    let a = p.assets.get(id)?;
    let q = p.proxies.entries.get(id)?;
    if a.width != q.source_width || a.height != q.source_height {
        return None;
    }
    if stamp(Path::new(&p.resolve(&a.path))).ok()? != q.source.stamp
        || stamp(Path::new(&p.resolve(&q.path))).ok()? != q.proxy.stamp
    {
        return None;
    }
    Some(p.resolve(&q.path))
}

#[derive(Debug)]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub num: u32,
    pub den: u32,
    pub frames: usize,
}
fn number(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| v[key].as_f64())
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("missing video {key}"))
}
/// Inspect every decoded timestamp rather than guessing CFR from container rate tags.
pub fn inspect(path: &Path, timeout: Duration) -> Result<Video, String> {
    let bytes = crate::color::process::decode_capture(Command::new("ffprobe").args(["-v","error","-select_streams","v:0","-show_entries",
        "stream=width,height,r_frame_rate,avg_frame_rate,start_time,pix_fmt,sample_aspect_ratio,color_transfer:stream_tags=rotate:stream_side_data=rotation:stream_disposition=attached_pic:frame=best_effort_timestamp_time",
        "-of","json"]).arg(path), 32 << 20, timeout)?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let s = data["streams"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or("proxy requires a video stream")?;
    let rate = |key: &str| -> Result<(u32, u32), String> {
        let (n, d) = s[key]
            .as_str()
            .and_then(|x| x.split_once('/'))
            .ok_or("missing rational video rate")?;
        let (n, d) = (
            n.parse::<u32>().map_err(|e| e.to_string())?,
            d.parse::<u32>().map_err(|e| e.to_string())?,
        );
        if n == 0 || d == 0 || n > 1_000_000 || !(1.0..=240.0).contains(&(n as f64 / d as f64)) {
            return Err("unsupported video rate (requires 1..240 fps)".into());
        }
        Ok((n, d))
    };
    let (num, den) = rate("r_frame_rate")?;
    let (an, ad) = rate("avg_frame_rate")?;
    if (num as f64 / den as f64 - an as f64 / ad as f64).abs() > 1e-6 {
        return Err(
            "variable frame rate is not supported for proxies; conform the source first".into(),
        );
    }
    if number(s, "start_time")?.abs() > 0.0001 {
        return Err("proxy requires a video stream starting at zero".into());
    }
    if s["sample_aspect_ratio"]
        .as_str()
        .is_some_and(|x| x != "1:1" && x != "N/A")
    {
        return Err("proxy requires square pixels".into());
    }
    if s["tags"]["rotate"]
        .as_str()
        .is_some_and(|x| x.parse::<f64>().unwrap_or(1.0).abs() > 0.0001)
        || s["side_data_list"].as_array().is_some_and(|a| {
            a.iter()
                .any(|v| v["rotation"].as_f64().unwrap_or(0.0).abs() > 0.0001)
        })
        || s["disposition"]["attached_pic"].as_u64().unwrap_or(0) != 0
    {
        return Err("proxy does not support rotated video or attached pictures".into());
    }
    if ![
        "yuv420p", "yuvj420p", "yuv422p", "yuvj422p", "yuv444p", "yuvj444p", "rgb24", "bgr24",
        "gbrp", "nv12",
    ]
    .contains(&s["pix_fmt"].as_str().unwrap_or(""))
        || s["color_transfer"].as_str().is_some_and(|x| {
            ![
                "unknown",
                "bt709",
                "iec61966-2-1",
                "smpte170m",
                "gamma22",
                "gamma28",
                "bt470m",
                "bt470bg",
            ]
            .contains(&x)
        })
    {
        return Err(
            "proxy supports opaque 8-bit SDR video only; alpha/HDR/float media stays on originals"
                .into(),
        );
    }
    let frames = data["frames"]
        .as_array()
        .ok_or("video timestamps missing")?;
    if frames.is_empty() || frames.len() > 216_000 {
        return Err("proxy requires 1..216000 video frames".into());
    }
    let dt = den as f64 / num as f64;
    for (i, f) in frames.iter().enumerate() {
        crate::guard::check();
        if (number(f, "best_effort_timestamp_time")? - i as f64 * dt).abs() > dt * 0.02 + 0.00001 {
            return Err("nonuniform video timestamps: conform variable frame rate before generating proxies".into());
        }
    }
    let width = s["width"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or("invalid video width")?;
    let height = s["height"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or("invalid video height")?;
    Ok(Video {
        width,
        height,
        num,
        den,
        frames: frames.len(),
    })
}
pub fn dimensions(w: u32, h: u32, max_side: u32) -> Result<(u32, u32), String> {
    if w == 0 || h == 0 {
        return Err("invalid source dimensions".into());
    }
    let (mut a, mut b) = (w, h);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let (rw, rh) = (w / a, h / a);
    let mut k = (max_side / rw.max(rh)).min(a);
    if rw % 2 != 0 || rh % 2 != 0 {
        k -= k % 2;
    }
    if k == 0 {
        return Err(
            "cannot preserve the exact source aspect ratio at this max_side; choose a larger bound"
                .into(),
        );
    }
    Ok((rw * k, rh * k))
}

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
pub fn generate(
    p: &Project,
    id: &str,
    destination: &str,
    max_side: u32,
    timeout: Duration,
) -> Result<Proxy, String> {
    if !(64..=4096).contains(&max_side) {
        return Err("max_side must be an integer in 64..4096".into());
    }
    let a = p.assets.get(id).ok_or("no such asset")?;
    if p.color_pipeline.enabled {
        return Err(
            "proxy generation is limited to unmanaged SDR projects; managed previews use originals"
                .into(),
        );
    }
    let source_path = PathBuf::from(p.resolve(&a.path));
    let dest = PathBuf::from(p.resolve(destination));
    if !dest
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("mp4"))
    {
        return Err("proxy path must end in .mp4".into());
    }
    if dest.exists() {
        return Err(
            "proxy destination already exists; choose a new path (files are never overwritten)"
                .into(),
        );
    }
    let parent = dest
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = crate::deps::plain_path(parent
        .canonicalize()
        .map_err(|e| format!("proxy parent directory must exist: {e}"))?);
    let dest = parent.join(dest.file_name().ok_or("invalid proxy path")?);
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or("invalid proxy timeout")?;
    let remaining = || deadline.saturating_duration_since(std::time::Instant::now());
    let source = fingerprint(&source_path)?;
    let v = inspect(&source_path, remaining())?;
    if v.width != a.width
        || v.height != a.height
        || (v.num as f64 / v.den as f64 - a.fps).abs() > 0.01
    {
        return Err(
            "source metadata differs from the imported asset; reimport before generating a proxy"
                .into(),
        );
    }
    let (width, height) = dimensions(v.width, v.height, max_side)?;
    let tmp_path = parent.join(format!(
        ".kimo-proxy-{}-{}.mp4",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| e.to_string())?;
    let tmp = Temp(tmp_path);
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-y", "-noautorotate", "-i"])
        .arg(&source_path)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-map_metadata",
            "-1",
            "-vf",
            &format!("scale={width}:{height}:flags=fast_bilinear,setsar=1"),
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-g",
            "1",
            "-bf",
            "0",
            "-pix_fmt",
            "yuv420p",
            "-r",
            &format!("{}/{}", v.num, v.den),
            "-fps_mode",
            "cfr",
            "-video_track_timescale",
            &v.num.to_string(),
            "-f",
            "mp4",
        ])
        .arg(&tmp.0);
    crate::color::process::wait_command(&mut cmd, remaining())?;
    let encoded = inspect(&tmp.0, remaining())?;
    if encoded.frames != v.frames
        || encoded.width != width
        || encoded.height != height
        || (encoded.num as f64 / encoded.den as f64 - v.num as f64 / v.den as f64).abs() > 1e-6
    {
        return Err("proxy failed frame-count/rate/dimension validation".into());
    }
    let proxy = fingerprint(&tmp.0)?;
    let source_after = fingerprint(&source_path)?;
    if source_after.stamp != source.stamp || source_after.sha256 != source.sha256 {
        return Err("source changed during proxy generation; retry".into());
    }
    crate::guard::check();
    // Atomic, no-clobber publication. Neither errors nor cancellation attach a partial proxy.
    std::fs::hard_link(&tmp.0, &dest)
        .map_err(|e| format!("cannot publish proxy without overwriting: {e}"))?;
    Ok(Proxy {
        path: dest.to_string_lossy().into_owned(),
        source,
        proxy,
        source_width: v.width,
        source_height: v.height,
        width,
        height,
        fps_num: v.num,
        fps_den: v.den,
        frames: v.frames,
    })
}
