//! Raw audio API on the ONE fixed-48k stereo strict mixer decoder.
use super::kit::*;
use crate::perception::{
    audio_raw::{self, Samples},
    audio_targets,
};
fn count(n: usize, c: usize) -> Result<(), String> {
    let used = n
        .checked_mul(c)
        .ok_or("invalid_samples: sample count overflow")? as u64;
    let cap = crate::guard::effective_limits().max_samples;
    if used > cap {
        return Err(limit_error("max_samples", used, cap));
    }
    crate::guard::reserve(used as usize * 16);
    Ok(())
}
fn usize_arg(a: &Value, k: &str, d: usize) -> Result<usize, String> {
    a.get(k)
        .map(|v| {
            v.as_u64()
                .map(|v| v as usize)
                .ok_or_else(|| format!("invalid_samples: {k} nonnegative integer required"))
        })
        .transpose()
        .map(|v| v.unwrap_or(d))
}
fn finite(a: &Value, k: &str, d: f64) -> Result<f64, String> {
    a.get(k)
        .map(|v| {
            v.as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("invalid_samples: {k} finite number required"))
        })
        .transpose()
        .map(|v| v.unwrap_or(d))
}
fn json_samples(rate: u32, c: usize, v: &Value, planar: bool) -> Result<Samples, String> {
    let arr = v.as_array().ok_or("invalid_samples: data array required")?;
    let mut raw = vec![];
    if planar {
        if arr.len() != c {
            return Err("invalid_samples: planar data needs channels arrays".into());
        }
        let n = arr
            .first()
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        count(n, c)?;
        for (ch, p) in arr.iter().enumerate() {
            let p = p
                .as_array()
                .filter(|p| p.len() == n)
                .ok_or("invalid_samples: equal-length channel arrays required")?;
            for (i, v) in p.iter().enumerate() {
                let f = v
                    .as_f64()
                    .map(|v| v as f32)
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| {
                        format!("invalid_samples: non-finite sample index {}", ch * n + i)
                    })?;
                raw.extend(f.to_le_bytes());
            }
        }
    } else {
        if arr.len() % c != 0 {
            return Err("invalid_samples: length not divisible by channels".into());
        }
        count(arr.len() / c, c)?;
        for (i, v) in arr.iter().enumerate() {
            let f = v
                .as_f64()
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("invalid_samples: non-finite sample index {i}"))?;
            raw.extend(f.to_le_bytes());
        }
    }
    audio_raw::from_bytes(rate, c, &raw, planar, 4)
}
fn import(ss: &mut Session, a: &Value) -> Result<Value, String> {
    let rate = usize_arg(a, "rate", 48000)?;
    let channels = usize_arg(a, "channels", 2)?;
    if !(1000..=384000).contains(&rate) || channels == 0 || channels > 64 {
        return Err("out_of_range: rate 1000..384000, channels1..64 required".into());
    }
    if a.get("normalize").and_then(Value::as_bool) == Some(true) {
        return Err(
            "invalid_samples: normalize must be false; raw samples never normalized/clamped".into(),
        );
    }
    let layout = a.get("layout").and_then(Value::as_str).unwrap_or("planar");
    if !["planar", "interleaved"].contains(&layout) {
        return Err("unknown layout; valid: planar, interleaved".into());
    }
    let planar = layout == "planar";
    let format = a.get("format").and_then(Value::as_str).unwrap_or("json");
    let s = match format {
        "json" => json_samples(
            rate as u32,
            channels,
            a.get("data").ok_or("missing array 'data'")?,
            planar,
        )?,
        "base64_f32le" | "base64_s16le" | "base64_f64le" => {
            let text = str_arg(a, "data")?;
            if text.len() as u64
                > crate::guard::effective_limits()
                    .max_samples
                    .saturating_mul(12)
            {
                return Err(limit_error(
                    "max_samples",
                    text.len() as u64 / 6,
                    crate::guard::effective_limits().max_samples,
                ));
            }
            let raw = audio_raw::unb64(text)?;
            let width = if format == "base64_s16le" {
                2
            } else if format == "base64_f64le" {
                8
            } else {
                4
            };
            count(raw.len() / width / channels, channels)?;
            audio_raw::from_bytes(rate as u32, channels, &raw, planar, width)?
        }
        "path" => {
            let path = ss.project.resolve(str_arg(a, "path")?);
            let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
            if size
                > crate::guard::effective_limits()
                    .max_samples
                    .saturating_mul(8)
                    + 65536
            {
                return Err(limit_error(
                    "max_samples",
                    size / 4,
                    crate::guard::effective_limits().max_samples,
                ));
            }
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            count(bytes.len() / 4 / channels, channels)?;
            if path.ends_with(".npy") {
                audio_raw::npy_read(rate as u32, channels, &bytes)?
            } else if path.ends_with(".wav") {
                parse_wav(&bytes)?
            } else {
                audio_raw::from_bytes(rate as u32, channels, &bytes, planar, 4)?
            }
        }
        _ => {
            return Err(
                "unknown format; valid: json, base64_f32le, base64_s16le, base64_f64le, path"
                    .into(),
            )
        }
    };
    let n = s.validate()?;
    if !(1000..=384000).contains(&s.rate) || s.rate as usize != rate || s.planes.len() != channels {
        return Err(
            "invalid_samples: native path rate/channels must match explicit rate/channels".into(),
        );
    }
    count(n, s.planes.len())?;
    let hash = crate::deps::sha256_hex(&audio_raw::bytes(&s));
    let wav = audio_raw::wav(&s)?;
    let (path, store) = store_audio(ss, &hash, s.rate, s.planes.len(), &wav)?;
    let absolute = crate::deps::plain_path(path
        .canonicalize()
        .map_err(|e| e.to_string())?)
        .to_string_lossy()
        .into_owned();
    let id = ss
        .project
        .assets
        .values()
        .find(|v| v.path == absolute)
        .map(|v| v.id.clone())
        .unwrap_or_else(|| {
            let id = ss.project.gen_id("asset");
            ss.project.assets.insert(
                id.clone(),
                crate::model::Asset {
                    origin: Some("import_samples".into()),
                    hash: Some(hash.clone()),
                    kind: Some("audio".into()),
                    id: id.clone(),
                    path: absolute.clone(),
                    width: 0,
                    height: 0,
                    duration: n as f64 / s.rate as f64,
                    fps: 0.,
                    bin: None,
                },
            );
            id
        });
    Ok(
        json!({"asset_id":id,"path":absolute,"rate":s.rate,"channels":s.planes.len(),"n_samples":n,"hash":hash,"duration_s":n as f64/s.rate as f64,"origin":"import_samples","store":store,"asset_key":format!("{}hz-{}ch/{hash}",s.rate,s.planes.len()),"decoded_equivalent":{"rate":48000,"channels":2,"bit_equal_roundtrip":s.rate==48000&&s.planes.len()==2},"conversion":if format=="base64_f64le"{"f64 rounded to f32"}else{"none"}}),
    )
}
fn parse_wav(b: &[u8]) -> Result<Samples, String> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("invalid_samples: WAV header required".into());
    }
    let mut fmt = None;
    let mut data = None;
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        let v = b
            .get(i + 8..i + 8 + len)
            .ok_or("invalid_samples: truncated WAV")?;
        if &b[i..i + 4] == b"fmt " {
            fmt = Some(v);
        }
        if &b[i..i + 4] == b"data" {
            data = Some(v);
        }
        i += 8 + len + len % 2;
    }
    let f = fmt
        .filter(|f| f.len() >= 16)
        .ok_or("invalid_samples: missing WAV fmt")?;
    let code = u16::from_le_bytes(f[..2].try_into().unwrap());
    let c = u16::from_le_bytes(f[2..4].try_into().unwrap()) as usize;
    let rate = u32::from_le_bytes(f[4..8].try_into().unwrap());
    if code != 3 || u16::from_le_bytes(f[14..16].try_into().unwrap()) != 32 {
        return Err("invalid_samples: path WAV requires float32 PCM".into());
    }
    audio_raw::from_bytes(
        rate,
        c,
        data.ok_or("invalid_samples: WAV data missing")?,
        false,
        4,
    )
}
fn read(ss: &mut Session, a: &Value) -> Result<Value, String> {
    if usize_arg(a, "rate", 48000)? != 48000 {
        return Err("out_of_range: mixer rate is fixed 48000".into());
    }
    let target = a.get("target").and_then(Value::as_str).unwrap_or("mix");
    let default_stage = if target == "mix" || target.starts_with("track:") {
        "post_mix"
    } else {
        "post_filter"
    };
    let stage = a
        .get("stage")
        .and_then(Value::as_str)
        .unwrap_or(default_stage);
    if !["source", "post_filter", "post_mix"].contains(&stage) {
        return Err("unknown stage; valid: source, post_filter, post_mix".into());
    }
    let mut temp = Session::new();
    temp.project = ss.project.clone();
    let mut origin = 0.;
    let translated = if let Some(id) = target.strip_prefix("clip:") {
        temp.project
            .find_clip(id)
            .ok_or("invalid_samples: clip not found")?;
        origin = 0.;
        if stage == "source" {
            let c = temp.project.find_clip_mut(id).unwrap();
            c.audio = crate::model::Audio::default();
        }
        json!({"clip_id":id})
    } else if let Some(id) = target.strip_prefix("track:") {
        if stage != "post_mix" {
            return Err("invalid_samples: track stage must be post_mix".into());
        }
        json!({"track_id":id})
    } else if let Some(p) = target.strip_prefix("file:") {
        json!({"path":p})
    } else if target == "mix" {
        if stage != "post_mix" {
            return Err("invalid_samples: mix stage must be post_mix".into());
        }
        json!("mix")
    } else {
        return Err(
            "invalid_samples: target clip:<id>, track:<id>, mix or file:<path> required".into(),
        );
    };
    let anticipated = if let Some(id) = target.strip_prefix("clip:") {
        temp.project.find_clip(id).unwrap().duration
    } else if target == "mix" || target.starts_with("track:") {
        temp.project.duration()
    } else {
        let path = target.strip_prefix("file:").unwrap();
        crate::render::probe(&temp.project.resolve(path))?.2
    };
    let total = ((anticipated * 48000.).ceil() as u64).saturating_mul(2);
    let cap = crate::guard::effective_limits().max_samples;
    if total > cap {
        return Err(limit_error("max_samples", total, cap));
    }
    let decoded = audio_targets::decode(&temp, &translated)?;
    let mut pcm = decoded.pcm;
    if let Some(id) = target.strip_prefix("clip:") {
        if stage == "post_mix" {
            let c = ss.project.find_clip(id).unwrap();
            let mut project = ss.project.clone();
            project
                .tracks
                .retain(|t| t.clips.iter().any(|v| v.id == id));
            for t in &mut project.tracks {
                t.clips.retain(|v| v.id == id);
                for v in &mut t.clips {
                    std::sync::Arc::make_mut(v).start = 0.;
                }
            }
            pcm = crate::audio::mix_strict(&project).map_err(|e| e.to_string())?;
            let _ = c;
        }
    }
    let n = pcm.len() / 2;
    let st = finite(a, "start", origin)?;
    let max = usize_arg(
        a,
        "max_samples",
        (crate::guard::effective_limits().max_samples / 2) as usize,
    )?;
    let end = finite(a, "end", n as f64 / 48000.)?;
    if st < 0. || end < st || st > n as f64 / 48000. || end > n as f64 / 48000. + 1e-9 {
        return Err("out_of_range: sample range outside decoded duration".into());
    }
    let lo = (st * 48000.).round() as usize;
    let hi = ((end * 48000.).round() as usize).min(n);
    if hi - lo > max {
        return Err(limit_error(
            "max_samples",
            ((hi - lo) * 2) as u64,
            (max * 2) as u64,
        ));
    }
    let sel = a.get("channels").unwrap_or(&Value::Null);
    let mono = sel.as_str() == Some("mono");
    let map = if mono || sel.is_null() || sel.as_str() == Some("all") {
        vec![0usize, 1]
    } else if let Some(i) = sel.as_u64() {
        vec![i as usize]
    } else if let Some(v) = sel.as_array() {
        v.iter()
            .map(|v| {
                v.as_u64()
                    .map(|v| v as usize)
                    .ok_or("invalid_samples: channel index integer required")
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        return Err("invalid_samples: channels all, mono, index or indices required".into());
    };
    if map.is_empty() || map.iter().any(|&i| i > 1) {
        return Err("out_of_range: available decoded channels [0,1]".into());
    }
    count(hi - lo, if mono { 1 } else { map.len() })?;
    let planes = if mono {
        vec![(lo..hi)
            .map(|i| (pcm[i * 2] + pcm[i * 2 + 1]) * 0.5)
            .collect()]
    } else {
        map.iter()
            .map(|&c| (lo..hi).map(|i| pcm[i * 2 + c]).collect())
            .collect()
    };
    let s = Samples {
        rate: 48000,
        planes,
    };
    let bytes = audio_raw::bytes(&s);
    let hash = crate::deps::sha256_hex(&bytes);
    let mut out = json!({"rate":48000,"channels":s.planes.len(),"channel_map":map,"n_samples":hi-lo,"start":st,"end":hi as f64/48000.,"stage":stage,"hash":hash,"clipped_count":s.planes.iter().flatten().filter(|v|v.abs()>1.).count(),"layout":"planar"});
    if let Some(path) = target.strip_prefix("file:") {
        out["source_info"] =
            crate::audio::source_info(&ss.project.resolve(path)).map_err(|e| e.to_string())?;
    }
    if let Some(id) = target.strip_prefix("clip:") {
        if let crate::model::Source::Media { asset } = &ss.project.find_clip(id).unwrap().source {
            out["source_info"] =
                crate::audio::source_info(&ss.project.resolve(&ss.project.assets[asset].path))
                    .map_err(|e| e.to_string())?;
        }
    }
    if let Some(v) = a.get("peaks") {
        let buckets = usize_arg(v, "n", 128)?;
        if buckets == 0 || buckets > 4096 {
            return Err("out_of_range: peaks n1..4096".into());
        }
        out["peaks"] = json!(audio_raw::peaks(&s, buckets));
        return Ok(out);
    }
    let format = a.get("format").and_then(Value::as_str).unwrap_or("json");
    out["data"] = match format {
        "json" => {
            if hi - lo > 4096 {
                return Err(
                    "out_of_range: json <=4096 samples/channel; choose base64_f32le or file".into(),
                );
            }
            json!(s.planes)
        }
        "base64_f32le" => json!(audio_raw::b64(&bytes)),
        "file" => {
            let fmt = a
                .get("file_format")
                .and_then(Value::as_str)
                .unwrap_or("wav_f32");
            let (ext, b) = match fmt {
                "wav_f32" => ("wav", audio_raw::wav(&s)?),
                "raw_f32le" => ("f32", bytes),
                "npy" => ("npy", audio_raw::npy(&s)),
                _ => return Err("unknown file_format; valid: wav_f32, raw_f32le, npy".into()),
            };
            let path = std::env::temp_dir().join(format!("kimo-samples-{hash}.{ext}"));
            std::fs::write(&path, b).map_err(|e| e.to_string())?;
            json!(path)
        }
        _ => return Err("unknown format; valid: json, base64_f32le, file".into()),
    };
    Ok(out)
}
pub fn commands() -> Vec<Command> {
    vec![Command{name:"read_samples",mutates:false,description:"Read planar f32 samples through the same strict decoder/mixer at fixed48000 stereo; no second decoder or resampler. target clip:<id> uses clip-local seconds, track:<id>/mix timeline, file:<path> source. stage source clears clip audio filters, post_filter before volume/pan, post_mix includes volume/pan/duck/limiter. Channel selector all|mono arithmetic mean|index|indices (decoded indices0,1). json <=4096 samples/ch, base64_f32le planar or file wav_f32/raw_f32le/npy; peaks min/max/RMS. Hash SHA256 canonical planar f32le, clipped_count informational, never clamps. Native source rate/channel metadata separately. f64 read not offered.",schema:obj(vec![("target",s("clip:<id>,track:<id>,mix,file:<path>")),("start",n("seconds")),("end",n("seconds")),("channels",json!({})),("stage",json!({"type":"string","enum":["source","post_filter","post_mix"]})),("rate",n("fixed48000")),("format",json!({"type":"string","enum":["json","base64_f32le","file"]})),("file_format",json!({"type":"string","enum":["wav_f32","raw_f32le","npy"]})),("peaks",json!({"type":"object"})),("max_samples",n("max frames per selected channel, total capped by limits"))],&[]),run:read},Command{name:"import_samples",mutates:true,description:"Import finite raw samples as hash-addressed file-backed float32 WAV audio asset, one undo step, collectable and usable by add_audio_clip. Planar API default, interleaved optional. JSON/f32le/s16le/f64le base64 or path WAVfloat32/rawf32/NPY; f64 rounds to f32, s16 divides32768. Never normalize/clamp. Rate1000..384000 stored native; existing mixer decodes fixed48000 stereo. Bit-equal import/read source roundtrip only48000 stereo. SHA256 planar f32le dedup. Project/env/user_data assets/audio storage, absolute path returned.",schema:obj(vec![("name",s("label")),("rate",n("1000..384000")),("channels",n("1..64")),("layout",json!({"type":"string","enum":["planar","interleaved"]})),("format",json!({"type":"string","enum":["json","base64_f32le","base64_s16le","base64_f64le","path"]})),("data",json!({})),("path",s("input file")),("normalize",json!({"type":"boolean"}))],&["rate","channels"]),run:import}]
}

fn store_audio(
    s: &Session,
    hash: &str,
    rate: u32,
    channels: usize,
    bytes: &[u8],
) -> Result<(std::path::PathBuf, &'static str), String> {
    let dir = crate::assets::store_dir(&s.project, "audio")?.join(format!("{rate}hz-{channels}ch"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let rule = if s.project.base_dir.is_some() {
        "project"
    } else if std::env::var_os("KIMO_ASSET_DIR").is_some() {
        "env"
    } else {
        "user_data"
    };
    let path = dir.join(format!("{hash}.wav"));
    if path.exists() {
        if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
            return Err(format!(
                "invalid_samples: hash collision at {} vs incoming audio asset",
                path.display()
            ));
        }
        return Ok((path, rule));
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp = dir.join(format!(
        ".{hash}-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let r = (|| {
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        match std::fs::hard_link(&temp, &path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    })();
    let _ = std::fs::remove_file(temp);
    r?;
    Ok((path, rule))
}

fn limit_error(knob:&str,used:u64,cap:u64)->String{format!("limit_exceeded: {knob} used {used}, cap {cap}; lower usage or adjust set_limits {knob}")}
