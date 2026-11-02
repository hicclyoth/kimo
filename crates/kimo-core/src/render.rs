use crate::effects::{self, Frame};
use crate::model::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Decoded frames for media clips, keyed by clip id. Frames are RGBA8 at project size, project fps,
/// Constant speed is applied during decode; ramps use source-time indices.
/// A decoded window of consecutive frames, contiguous in memory. `first` is the global frame index of buf[0].
pub struct Frames { pub still:bool,pub working_linear:bool, pub quantized: std::sync::OnceLock<Vec<u8>>, pub linear_buf: Option<Vec<f32>>, pub buf: Vec<u8>, pub fs: usize, pub first: usize }
impl Frames {
    pub fn get_float(&self,g:usize)->Option<&[f32]>{let b=self.linear_buf.as_ref()?;let n=self.len();if n==0{return None}let i=g.saturating_sub(self.first).min(n-1);Some(&b[i*self.fs..(i+1)*self.fs])}
    pub fn len(&self) -> usize { if self.fs == 0 { 0 } else { self.linear_buf.as_ref().map(|b|b.len()).unwrap_or(self.buf.len()) / self.fs } }
    pub fn covers(&self, a: usize, b: usize) -> bool { self.len() > 0 && ((a >= self.first && b < self.first + self.len()) || (self.still && self.len() == 1 && self.first == 0)) }
    /// Global-index lookup, clamped into the window.
    pub fn get(&self, g: usize) -> Option<&[u8]> { let n = self.len(); if n == 0 { return None; } let i = g.saturating_sub(self.first).min(n - 1); let buf=if let Some(f)=&self.linear_buf{self.quantized.get_or_init(||f.iter().map(|v|(v.clamp(0.,1.)*255.+0.5) as u8).collect())}else{&self.buf};Some(&buf[i * self.fs..(i + 1) * self.fs]) }
}
impl std::ops::Index<usize> for Frames { type Output = [u8]; fn index(&self, i: usize) -> &[u8] { self.get(self.first+i).expect("no decoded frame") } }
pub struct Cache { frames: HashMap<String, (String, std::sync::Arc<Frames>)>, /// Code clip frames per clip id: (signature, last slot asked for, slot -> canvas-size RGBA8). See codeclip.rs.
    code: HashMap<String, CodeSlots>, pub size: (u32, u32, f64), /// Export-only static layer cache (project is immutable for the cache's lifetime).
    pub export_mode: bool, proxy_clips: std::collections::BTreeMap<String,String>, statics: std::sync::RwLock<HashMap<String, std::sync::Arc<StaticCrop>>> }
/// Rendered frames of one code clip: (signature, last slot prepared, slot -> canvas-size straight-alpha RGBA8).
type CodeSlots = (String, usize, std::collections::BTreeMap<usize, std::sync::Arc<Vec<u8>>>);
pub struct StaticCrop { x0: usize, y0: usize, x1: usize, y1: usize, d: Vec<f32> }
impl Cache { pub fn new() -> Self { Cache { frames: HashMap::new(), code: HashMap::new(), size: (0, 0, 0.0), export_mode: false, proxy_clips: Default::default(), statics: std::sync::RwLock::new(HashMap::new()) } } pub fn new_export() -> Self { let mut c = Cache::new(); c.export_mode = true; c } pub fn frames_of(&self, id: &str) -> Option<&Frames> { self.frames.get(id).map(|x| x.1.as_ref()) }
    /// A cheap point-in-time copy for overlapping export: shares every decoded frame/code-slot buffer by
    /// Arc (no pixel data is duplicated), so a prefetch thread can decode the next window into its own
    /// `Cache` instance without aliasing the one the current batch's compose threads are reading.
    pub fn snapshot(&self) -> Cache { Cache { frames: self.frames.clone(), code: self.code.clone(), size: self.size, export_mode: self.export_mode, proxy_clips: self.proxy_clips.clone(), statics: std::sync::RwLock::new(self.statics.read().map(|g| g.clone()).unwrap_or_default()) } }
}

/// Still-image extensions the decoder holds as one looped frame (ffmpeg `-loop 1`). One list, read by
/// `decode` and by `import_media`'s check, so the two cannot drift apart.
pub const STILL_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "exr", "tif", "tiff", "pfm"];

/// True when `path` ends with one of `STILL_EXT` (case-insensitive; the same test `decode` has always used).
pub fn is_still_path(path: &str) -> bool { let l = path.to_lowercase(); STILL_EXT.iter().any(|e| l.ends_with(e)) }

/// Refuse a single still image whose extension the decoder does not hold (e.g. .ppm, .tga, .heic). Before this
/// check such a file probed fine, imported, and then rendered flat with no error (v5.1 debt, 2026-10-18).
/// A still is what ffprobe reads with an image demuxer ("image2" or "*_pipe"); a numbered sequence (a %d path)
/// is read the same way and judged by its extension. Video, audio, animated GIF and .kimopx are not refused here.
pub fn check_importable(path: &str) -> Result<(), String> {
    // A numbered still sequence ("f_%04d.png") probed and imported, then failed only at render ("media stat: No such
    // file"), because an asset is one file. Refuse it here, at import / project load, with the way out (Naman m:48b79971).
    if is_still_path(path) && crate::deps::seq_frame(path, 0).is_some() {
        return Err(format!("image sequences are not supported: {path} is a numbered frame pattern, and a kimo asset is one file. Join the frames into a video first (e.g. ffmpeg -framerate 25 -i '{path}' -c:v libx264 -pix_fmt yuv420p frames.mp4) and import that, or import a single frame."));
    }
    if is_still_path(path) || path.ends_with(".kimopx") { return Ok(()); }
    let Ok(out) = Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=format_name", "-of", "default=nw=1:nk=1", path]).output() else { return Ok(()) };
    if !out.status.success() { return Ok(()); }
    let format = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if format == "image2" || format.ends_with("_pipe") {
        let ext = std::path::Path::new(path).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| "(no extension)".into());
        return Err(format!("unsupported image extension {ext}: {path} is a still image kimo cannot draw (read as {format}). kimo draws stills only from {}; convert it (e.g. to .png) and import again.", STILL_EXT.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ")));
    }
    Ok(())
}

/// `check_importable`, remembered per path for the life of the process, for the decoder: a project that reached the
/// renderer without import_media (an older project file, a hand-edited path) gets the same error instead of a flat frame.
fn check_decodable(path: &str) -> Result<(), String> {
    if is_still_path(path) || path.ends_with(".kimopx") { return Ok(()); }
    static SEEN: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Result<(), String>>>> = std::sync::OnceLock::new();
    let m = SEEN.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(r) = m.lock().unwrap().get(path) { return r.clone(); }
    let r = check_importable(path);
    m.lock().unwrap().insert(path.to_string(), r.clone());
    r
}

pub fn probe(path: &str) -> Result<(u32, u32, f64, f64), String> {
    if path.ends_with(".kimopx") { let f=crate::pixels::read(path)?; return Ok((f.w as u32,f.h as u32,0.,25.)); }
    if [".tif",".tiff"].iter().any(|e|path.to_lowercase().ends_with(e)){let(w,h)=crate::color::interchange::tiff_size(path)?;return Ok((w,h,0.,25.))}
    let out = Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries",
        "stream=width,height,r_frame_rate,duration:format=duration", "-of", "json", path])
        .output().map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(format!("cannot probe {path}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("invalid ffprobe metadata for {path}: {e}"))?;
    parse_probe_metadata(&metadata).map_err(|e| format!("cannot probe {path}: {e}"))
}

// WebM/Matroska commonly omit stream.duration while format.duration is known.
// Parse named fields, never treat N/A as a known zero duration.
fn parse_probe_metadata(v: &serde_json::Value) -> Result<(u32, u32, f64, f64), String> {
    fn finite_nonnegative(v: &serde_json::Value) -> Option<f64> {
        let n = v.as_f64().or_else(|| v.as_str()?.parse().ok())?;
        (n.is_finite() && n >= 0.).then_some(n)
    }
    let container_duration = finite_nonnegative(&v["format"]["duration"]);
    let stream = v["streams"].as_array().and_then(|streams| streams.first());
    let Some(s) = stream else {
        return container_duration
            .map(|d| (0, 0, d, 0.))
            .ok_or_else(|| "no video stream or finite container duration".into());
    };
    let dimension = |key: &str| {
        s[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("missing/invalid video {key}"))
    };
    let (w, h) = (dimension("width")?, dimension("height")?);
    let fps = s["r_frame_rate"]
        .as_str()
        .and_then(|r| {
            let (a, b) = r.split_once('/')?;
            let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
            let rate = a / b;
            (a.is_finite() && b.is_finite() && a > 0. && b > 0. && rate.is_finite()).then_some(rate)
        })
        .unwrap_or(30.);
    let stream_duration = finite_nonnegative(&s["duration"]);
    let duration = stream_duration
        .filter(|d| *d > 0.)
        .or(container_duration.filter(|d| *d > 0.))
        .or(stream_duration)
        .or(container_duration)
        .unwrap_or(0.);
    // Undated still images keep duration=0, matching existing import semantics.
    Ok((w, h, duration, fps))
}

#[cfg(test)]
mod probe_metadata_tests {
    use super::parse_probe_metadata;
    use serde_json::json;
    #[test]
    fn named_duration_fallbacks() {
        for d in [
            json!("N/A"),
            json!("NaN"),
            json!("inf"),
            json!(-1),
            json!(0),
            serde_json::Value::Null,
        ] {
            let v = json!({"streams":[{"width":320,"height":240,"r_frame_rate":"25/1","duration":d}],"format":{"duration":"32.480000"}});
            assert_eq!(parse_probe_metadata(&v).unwrap(), (320, 240, 32.48, 25.));
        }
        assert_eq!(parse_probe_metadata(&json!({"streams":[{"width":320,"height":240,"r_frame_rate":"30000/1001","duration":"2.5"}],"format":{"duration":"3.5"}})).unwrap(),(320,240,2.5,30000./1001.));
    }
    #[test]
    fn still_audio_and_invalid_metadata() {
        assert_eq!(
            parse_probe_metadata(
                &json!({"streams":[{"width":16,"height":16,"r_frame_rate":"0/0"}]})
            )
            .unwrap(),
            (16, 16, 0., 30.)
        );
        assert_eq!(
            parse_probe_metadata(&json!({"streams":[],"format":{"duration":"1.25"}})).unwrap(),
            (0, 0, 1.25, 0.)
        );
        assert!(parse_probe_metadata(&json!({})).is_err());
        assert!(parse_probe_metadata(&json!({"streams":[{"width":0,"height":16}]})).is_err());
    }
}

struct DecodeProcess(std::process::Child);
impl Drop for DecodeProcess{fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
const WINDOW: usize = 24;

/// Decode `n` frames starting at global clip frame `a` (windowed: memory stays bounded for any clip length).
fn decode(p: &Project, c: &Clip, path: &str, a: usize, n: usize, seek_base: Option<(f64, i64)>) -> Result<Frames, String> {
    let (w, h) = (p.width, p.height);
    check_decodable(path)?;
    if path.ends_with(".kimopx") {
      let src=crate::pixels::read(path)?;
      let src=if crate::reframe::active(c){let(rx,ry)=crate::reframe::pos(c,0,p.fps);crate::reframe::crop_frame(&src,w as usize,h as usize,rx,ry)}else{src};
      let d=if src.w==w as usize && src.h==h as usize {src.d} else {crate::color::interchange::fit(&src,w as usize,h as usize,c.props.get("fit").map(|q|q.at(0.)).unwrap_or(0.))?};
      return Ok(Frames{still:true,working_linear:true,quantized:Default::default(),linear_buf:Some(d),buf:vec![],fs:w as usize*h as usize*4,first:0});
    }
    if p.color_pipeline.enabled&&[".tif",".tiff"].iter().any(|e|path.to_lowercase().ends_with(e)){
      let mut src=crate::color::interchange::tiff_frame(path,decode_cache_limit())?;
      let fit=c.props.get("fit").map(|q|q.at(0.)).unwrap_or(0.);
      crate::color::pipeline::input(p,c,&mut src)?;
      let src=if crate::reframe::active(c){let(rx,ry)=crate::reframe::pos(c,0,p.fps);crate::reframe::crop_frame(&src,w as usize,h as usize,rx,ry)}else{src};
      let d=crate::color::interchange::fit(&src,w as usize,h as usize,fit)?;
      return Ok(Frames{still:true,working_linear:true,quantized:Default::default(),linear_buf:Some(d),buf:vec![],fs:w as usize*h as usize*4,first:0})
    } let is_img0 = is_still_path(path); let (a, n) = if is_img0 { (0, 1) } else { (a, n) };
    let (in_pt, speed, span, a_rel) = if let Some((st, fbase)) = seek_base { let rel = (a as i64 - fbase).max(0) as usize; (st, 1.0, (rel+n) as f64 / p.fps, rel) } else if c.retimed() { (c.in_point, 1.0, (a+n) as f64 / p.fps, a) } else { (c.in_point, c.speed, (a+n) as f64 / p.fps * c.speed, a) };
    let fit = c.props.get("fit").map(|q| q.at(0.0)).unwrap_or(0.0);
    let fitf = if crate::reframe::active(c) { crate::reframe::ffmpeg_fit(c, w, h, p.fps, a, n) } else if fit >= 1.5 { format!("scale={w}:{h}") } else if fit >= 0.5 { format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}") } else { format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0") };
    if p.color_pipeline.enabled&&path.to_lowercase().ends_with(".exr"){crate::color::interchange::validate_exr_source(path)?;}
    let float_source=p.color_pipeline.enabled&&[".exr",".pfm"].iter().any(|ext|path.to_lowercase().ends_with(ext));
    let native_fit=float_source||(p.color_pipeline.enabled&&p.color_pipeline.linear_resample);
    let pix=if float_source{"gbrapf32le"}else if p.color_pipeline.enabled{"rgba64le"}else{"rgba"};
    let vf = format!("setpts=PTS/{},fps={},trim=start_frame={a_rel}:end_frame={},setpts=PTS-STARTPTS,{},format={pix}",speed.max(0.01),p.fps,a_rel+n,fitf);
    let is_img = is_still_path(path);
    let loop_args: Vec<&str> = if is_img { vec!["-loop", "1"] } else { vec![] };
    let fs = (w * h * 4) as usize;


    let (dw,dh)=if native_fit{let(q,r,_,_)=probe(path)?;(q as usize,r as usize)}else{(w as usize,h as usize)};
    let raw_fs=dw.checked_mul(dh).and_then(|v|v.checked_mul(4)).ok_or("source dimensions overflow")?;
    let estimate=raw_fs.max(fs).checked_mul(if p.color_pipeline.enabled{8}else{1}).ok_or("decode allocation overflow")?.checked_mul(n.max(1)).ok_or("decode allocation overflow")?;
    if estimate>decode_cache_limit(){return Err("decode request exceeds bounded cache; reduce size/duration or use windowed timing".into())}
    crate::guard::reserve(estimate);
    let exr_args:Vec<&str>=if path.to_lowercase().ends_with(".exr"){vec!["-apply_trc","linear"]}else{vec![]};
    let vf=if native_fit{format!("setpts=PTS/{},fps={},trim=start_frame={a_rel}:end_frame={},setpts=PTS-STARTPTS,format={pix}",speed.max(0.01),p.fps,a_rel+n)}else{vf};
    let mut command=Command::new("ffmpeg");command.args(["-v","error"]).args(&loop_args).args(&exr_args).args(["-ss",&format!("{in_pt}"),"-t",&format!("{span}"),"-i",path,"-vf",&vf,"-frames:v",&n.max(1).to_string(),"-f","rawvideo","-pix_fmt",pix,"-"]);
    let mut buf=Vec::new();
    let linear_buf=if native_fit{
        let bytes_per_frame=raw_fs.checked_mul(if float_source{4}else{2}).ok_or("native frame size overflow")?;
        let converter=if float_source{None}else{Some(crate::color::pipeline::Input16::new(p,c)?)};
        let peak=bytes_per_frame.checked_mul(2).and_then(|v|v.checked_add(raw_fs.checked_mul(4)?)).and_then(|v|v.checked_add(fs.checked_mul(4)?.checked_mul(n.max(1).checked_add(1)?)?)).ok_or("native streaming allocation overflow")?;
        crate::guard::reserve(peak);
        let mut stream=crate::color::process::DecodeStream::new(&mut command,bytes_per_frame,n.max(1),crate::color::process::decode_deadline(n.max(1)))?;
        let mut values:Vec<f32>=Vec::new();values.try_reserve_exact(fs.checked_mul(n.max(1)).ok_or("decode buffer size overflow")?).map_err(|e|format!("cannot allocate the decode buffer for {n} frames ({} MB): {e}",fs.saturating_mul(n.max(1)).saturating_mul(4)>>20))?;
        let mut fi=0usize;
        while let Some(frame)=stream.next()?{
         let d=if float_source{crate::color::interchange::unpack(&frame,dw,dh)}else{converter.as_ref().ok_or("missing integer input converter")?.decode(&frame)};
         let fitted=d.and_then(|d|{let mut source=Frame{w:dw,h:dh,d};if float_source{crate::color::pipeline::input(p,c,&mut source)?;}
          if crate::reframe::active(c){let(rx,ry)=crate::reframe::pos(c,a+fi,p.fps);source=crate::reframe::crop_frame(&source,w as usize,h as usize,rx,ry);}
          crate::color::interchange::fit(&source,w as usize,h as usize,fit)});values.extend(fitted?);fi+=1;
        }
        stream.finish()?;Some(values)
    }else{
        let bytes_per_frame=fs.checked_mul(if p.color_pipeline.enabled{2}else{1}).ok_or("decode frame size overflow")?;
        let max=bytes_per_frame.checked_mul(n.max(1)).ok_or("decode output size overflow")?;
        buf=crate::color::process::decode_capture(&mut command,max,crate::color::process::decode_deadline(n.max(1)))?;
        if buf.len()%bytes_per_frame!=0{return Err("truncated project-sized decode output".into())}
        if p.color_pipeline.enabled{let values=buf.chunks_exact(2).map(|v|u16::from_le_bytes([v[0],v[1]]) as f32/65535.).collect();buf=Vec::new();Some(values)}else{None}
    };
    let k = buf.len() / fs; buf.truncate(k * fs);
    Ok(Frames { still:is_img,working_linear:native_fit,quantized:Default::default(),linear_buf, buf, fs, first: a })
}

fn media_index(c:&Clip,lt:f64,fps:f64)->usize{if let Some((a,_,_))=c.remap_pos(lt.max(0.),fps){return a}if c.speed_curve.is_some(){return c.source_index(lt,fps)}(lt.max(0.)*fps+1e-6).floor() as usize}
fn media_total(c:&Clip,fps:f64)->usize{let span=if c.time_remap.is_some(){c.remap_span(0.,c.duration).1+1./fps}else if c.speed_curve.is_some(){c.source_time(c.duration)}else{c.duration};((span*fps).ceil() as usize).max(1)}
fn decode_cache_limit()->usize{(crate::guard::limits().mem_bytes/3).min(256*1024*1024).max(1024*1024)}

pub fn prepare(p: &Project, cache: &mut Cache) -> Result<(), String> { prepare_at(p, cache, None) }

/// Decode media clips (only those active at `t` when given). Re-decodes when clip timing/speed/project size changed.
pub fn prepare_at(p: &Project, cache: &mut Cache, t: Option<f64>) -> Result<(), String> { match t { Some(t) => prepare_range(p, cache, t, t), None => prepare_range(p, cache, f64::MIN, f64::MAX) } }

/// Decode clips overlapping [t0, t1] and evict decoded clips outside it (bounds memory).
pub fn prepare_range(p: &Project, cache: &mut Cache, t0: f64, t1: f64) -> Result<(), String> {prepare_window(p,cache,t0,t1,WINDOW,false,false)}
/// Export-only bounded lookahead amortizes stable prefix decode without changing fps phase.
pub fn prepare_export_range(p:&Project,cache:&mut Cache,t0:f64,t1:f64)->Result<(),String>{prepare_window(p,cache,t0,t1,256,false,false)}
/// Explicit editing preview. Ordinary preparation and export always select originals.
pub fn prepare_preview_range(p:&Project,cache:&mut Cache,t0:f64,t1:f64)->Result<(),String>{prepare_window(p,cache,t0,t1,WINDOW,true,false)}
/// Random-access analysis (get_scopes/lint_edit's picture scan and friends): a big (export-sized)
/// lookahead window, same as prepare_export_range, plus permission to use a cached short-GOP
/// analysis proxy (analysis_proxy.rs) with a near-keyframe seek when one is available and the clip
/// is a plain, non-retimed, speed==1 clip in an SDR (non-color-managed) project. Falls back to the
/// original at the first sign of trouble, exactly like the preview proxy does. Export never calls
/// this; it always calls prepare_export_range, which never touches a proxy.
/// KR2: counted PER-THREAD without a new thread-local static (the task bars adding one; this engine has
/// a real history of thread-local render-state bugs - see tests/render_state_leak.rs). Keyed by
/// `std::thread::current().id()` in a shared, lazily-built `Mutex<HashMap<..>>` instead: Rust's
/// default test harness gives every `#[test]` fn its own OS thread, so this is just as immune to
/// unrelated tests calling lint_edit / get_scopes concurrently in the same binary as a real
/// thread-local would be, with no new thread-local storage anywhere in the engine. Pins how many
/// times real preparation work for an analysis sample actually runs - see director_tools.rs's
/// picture scan, which used to call this itself AND rely on get_scopes calling it again for the
/// exact same instant.
pub static ANALYSIS_PREPARE_CALLS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<std::thread::ThreadId, usize>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
pub fn analysis_prepare_call_count() -> usize { *ANALYSIS_PREPARE_CALLS.lock().unwrap().get(&std::thread::current().id()).unwrap_or(&0) }
pub fn prepare_analysis_range(p:&Project,cache:&mut Cache,t0:f64,t1:f64)->Result<(),String>{*ANALYSIS_PREPARE_CALLS.lock().unwrap().entry(std::thread::current().id()).or_insert(0)+=1;prepare_window(p,cache,t0,t1,256,false,true)}
impl Cache {
    pub fn proxy_assets(&self)->Vec<String>{self.proxy_clips.values().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()}
    pub(crate) fn proxy_variant(&self)->u128 {
        if self.proxy_clips.is_empty(){return 0}
        use std::hash::{Hash,Hasher};
        let mut h=std::collections::hash_map::DefaultHasher::new();self.proxy_clips.hash(&mut h);
        ((h.finish() as u128)<<64)|0x70726f78795f7631
    }
}
fn prepare_window(p: &Project, cache: &mut Cache, t0: f64, t1: f64, window:usize, preview:bool, analysis:bool) -> Result<(), String> {
    cache.proxy_clips.clear();
    if !t0.is_finite()||!t1.is_finite()||t1<t0{return Err("decode range requires finite ordered times".into())}
    if cache.size != (p.width, p.height, p.fps) { cache.frames.clear(); cache.code.clear(); cache.size = (p.width, p.height, p.fps); }
    let all = t0 == f64::MIN;
    let flat = flat_clips(&p.tracks, 0.0);
    let mut active: std::collections::HashSet<String> = flat.iter().filter(|c| all || (c.start <= t1 + 1e-9 && c.start + c.duration > t0 + 1e-9)).map(|c| c.id.clone()).collect();
    // morph_cut: a clip inside its morph needs the outgoing clip's last frame even when the window does not reach it
    let mut morph_t: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    if !all { for c in flat.iter().filter(|c| active.contains(&c.id)) { if let Some(tr) = c.transition_in.as_ref().filter(|tr| tr.kind == "morph_cut") { if t0 < c.start + tr.duration { if let Some(a) = crate::transitions::morph_prev(p, c) { if !active.contains(&a.id) { morph_t.insert(a.id.clone(), a.start + a.duration - 0.5 / p.fps); } } } } } }
    // push / gradient_wipe (pushwipe.rs): the outgoing abutting clip's last frame and a luma clip are read too
    if !all { for c in flat.iter().filter(|c| active.contains(&c.id)) { if c.transition_in.as_ref().is_some_and(|tr| crate::pushwipe::KINDS.contains(&tr.kind.as_str()) && t0 < c.start + tr.duration) { for (id, tm) in crate::pushwipe::needs(p, c, t0.max(c.start)) { if !active.contains(&id) { morph_t.insert(id, tm); } } } } }
    // ... and the outgoing clip is decoded lengthened past its out point (pushwipe::outgoing_clip), a superset of its own frames
    let mut pw_ext: std::collections::HashMap<String, Clip> = std::collections::HashMap::new();
    for c in flat.iter().filter(|c| all || active.contains(&c.id)) { if c.transition_in.as_ref().is_some_and(|tr| crate::pushwipe::KINDS.contains(&tr.kind.as_str()) && (all || t0 < c.start + tr.duration)) { if let Some(x) = crate::pushwipe::outgoing_clip(p, c) { if crate::pushwipe::handle_secs(p, c) > 0.0 { pw_ext.insert(x.id.clone(), x); } } } }
    for k in morph_t.keys() { active.insert(k.clone()); }
    if !all { cache.frames.retain(|k, _| active.contains(k)); cache.code.retain(|k, _| active.contains(k)); }
    let media_count=flat.iter().filter(|c|active.contains(&c.id)&&matches!(c.source,Source::Media{..})&&c.props.get("audio_only").map(|q|q.at(0.)).unwrap_or(0.)<0.5).count().max(1);
    if !all{let budget=decode_cache_limit()/media_count;cache.frames.retain(|_,(_,fr)|fr.buf.len()+fr.linear_buf.as_ref().map(|b|b.len()*5).unwrap_or(0)<=budget);}
    for c in &flat {
        let c = pw_ext.get(&c.id).unwrap_or(c);
        let Source::Media { asset } = &c.source else { continue };
        if !active.contains(&c.id) || c.props.get("audio_only").map(|q| q.at(0.0)).unwrap_or(0.0) >= 0.5 { continue; }
        let total = media_total(c,p.fps);
        let (t0, t1) = match morph_t.get(&c.id) { Some(tm) => (*tm, *tm), None => (t0, t1) };
        let (ta, tb) = match c.time_mode { _ if c.time_remap.is_some() => (t0, t1), TimeMode::Normal => (t0, t1), TimeMode::Reverse => (c.start + c.duration - (t1 - c.start), c.start + c.duration - (t0 - c.start)), TimeMode::Freeze { at } => (c.start + at, c.start + at) };
        let (t0, t1) = (ta, tb);
        let (ia, mut ib) = if all { (0, total - 1) } else if let Some(r)=&c.time_remap{let (lo,hi)=c.remap_span((t0-c.start).clamp(0.,c.duration),(t1-c.start).clamp(0.,c.duration));(((lo*p.fps).floor() as usize).min(total-1),((hi*p.fps).ceil() as usize+usize::from(r.blend!=crate::model::FrameBlend::Off)).min(total-1))} else if c.speed_curve.is_some(){(media_index(c,(t0-c.start).min(c.duration),p.fps).min(total-1),(media_index(c,(t1-c.start).min(c.duration),p.fps)+usize::from(!c.speed_blend.is_off())).min(total-1))}else { ((((t0 - c.start).max(0.0) * p.fps).floor() as usize).min(total - 1), (((t1 - c.start).max(0.0) * p.fps).ceil() as usize).min(total - 1)) };
        // Instant normal playback samples floor(index), except flow needs the next frame.
        if !all&&c.time_remap.is_none()&&matches!(c.time_mode,TimeMode::Normal)&&t0==t1{let flow=c.props.get("flow_blur").map(|v|v.at((t0-c.start).max(0.))>0.).unwrap_or(false)||c.ramp_pos((t0-c.start).max(0.),p.fps).is_some_and(|q|q.2>0.);ib=if flow{(ia+1).min(total-1)}else{ia};}
        // Vector flow blur also needs the previous frame.
        let mut ia=if !all&&c.props.get("flow_mode").map(|v|v.base.round()==1.).unwrap_or(false){ia.saturating_sub(1)}else{ia};
        // Stabilize, Synthesize Edges (stab_fill, see synth_edges) reads up to stab_fill frames before and after; other clips decode as before.
        if !all { let nf = c.props.get("stab_fill").map(|q| q.at(0.0)).unwrap_or(0.0); if nf >= 1.0 { let nf = nf.min(30.0) as usize; ia = ia.saturating_sub(nf); ib = (ib + nf).min(total - 1); } }
        let a = p.assets.get(asset).ok_or(format!("asset {asset} missing"))?;
        let original_path=p.resolve(&a.path);
        let preview_path=if preview {crate::proxy::preview_path(p,asset)}else{None};
        let analysis_eligible = analysis && preview_path.is_none() && !p.color_pipeline.enabled && c.speed==1.0 && !c.retimed();
        let analysis_proxy = if analysis_eligible { crate::analysis_proxy::ensure(p, asset, &original_path) } else { None };
        let mut selected = preview_path.or_else(|| analysis_proxy.as_ref().map(|ap| ap.path.clone()));
        let mut apath=selected.clone().unwrap_or_else(||original_path.clone());
        let stat=match std::fs::metadata(&apath) {
            Ok(stat)=>stat,
            Err(_) if selected.is_some()=>{selected=None;apath=original_path.clone();std::fs::metadata(&apath).map_err(|e|format!("media stat: {e}"))?},
            Err(e)=>return Err(format!("media stat: {e}")),
        };
        let using_preview_proxy = selected.is_some() && preview;
        let using_analysis_proxy = selected.is_some() && !using_preview_proxy;
        if using_preview_proxy{cache.proxy_clips.insert(c.id.clone(),asset.clone());}
        let sig = format!("{asset}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}", c.in_point, c.duration, c.speed, c.speed_curve.as_ref().map(|p| serde_json::to_string(p).unwrap_or_default()), p.color_pipeline,c.props.get("input_gamut"),c.props.get("input_transfer"),c.props.get("fit"),apath,stat.len(),stat.modified().ok());
        let sig = if crate::reframe::active(c) { format!("{sig}|reframe|{}", crate::reframe::sig(c)) } else { sig };
        let sig = match &c.time_remap { Some(r) => format!("{sig}|remap|{}|{}", serde_json::to_string(&r.curve).unwrap_or_default(), r.offset), None => sig };
        if let Some((s, fr)) = cache.frames.get(&c.id) { if *s == sig && fr.covers(ia, ib) { continue; } }
        let shared=cache.frames.values().find(|(s,fr)|*s==sig&&fr.covers(ia,ib)).map(|(_,fr)|fr.clone());
        if let Some(fr)=shared{cache.frames.insert(c.id.clone(),(sig,fr));continue}
        if a.width == 0 { cache.frames.insert(c.id.clone(), (sig, std::sync::Arc::new(Frames { still:false,working_linear:false,quantized:Default::default(),linear_buf: None, buf: vec![], fs: 0, first: 0 }))); continue; }
        // Ramps cache source-frame windows; constant-speed clips cache output-frame windows.
        let (start, count) = if all { (0, total) } else { (ia, (ib - ia + 1).max(window).min(total - ia)) };
        let mut count=count;
        if !all{let native=p.color_pipeline.enabled&&p.color_pipeline.linear_resample;let pixels=if native{let(dw,dh,_,_)=probe(&apath)?;(dw as usize*dh as usize).max(p.width as usize*p.height as usize)}else{p.width as usize*p.height as usize};let per=pixels*4*if p.color_pipeline.enabled{8}else{1};count=count.min((decode_cache_limit()/media_count/per.max(1)).max(1));if count<ib-ia+1{return Err("active decode range exceeds cache budget".into())}}
        cache.frames.remove(&c.id);
        let seek_base = if using_analysis_proxy { analysis_proxy.as_ref().and_then(|ap| crate::analysis_proxy::seek_base_for(ap, c, start)) } else { None };
        let (fr,sig) = match decode(p,c,&apath,start,count,seek_base) {
            Ok(fr) if selected.is_some() && !fr.covers(ia,ib)=>{if using_preview_proxy{cache.proxy_clips.remove(&c.id);}(decode(p,c,&original_path,start,count,None)?,format!("{sig}|original-fallback"))},
            Ok(fr)=>(fr,sig),
            Err(_) if selected.is_some()=>{if using_preview_proxy{cache.proxy_clips.remove(&c.id);}(decode(p,c,&original_path,start,count,None)?,format!("{sig}|original-fallback"))},
            Err(e)=>return Err(e),
        };
        if c.retimed()&&!fr.covers(ia,ib){return Err(format!("{} overrun for clip '{}': requested source frames {ia}..={ib}, decoded {} frame(s) starting at {}; shorten the ramp/clip or choose media with enough source duration",if c.time_remap.is_some(){"time remap"}else{"speed ramp"},c.id,fr.len(),fr.first))}
        let bytes=|fr:&Frames|fr.buf.len()+fr.linear_buf.as_ref().map(|b|b.len()*5).unwrap_or(0);
        let mut seen=std::collections::HashSet::new();
        if cache.frames.values().filter(|(_,f)|seen.insert(std::sync::Arc::as_ptr(f))).map(|(_,f)|bytes(f)).sum::<usize>().saturating_add(bytes(&fr))>decode_cache_limit(){return Err("active media cache exceeds memory cap; reduce resolution or active clips".into())}
        cache.frames.insert(c.id.clone(), (sig, std::sync::Arc::new(fr)));
    }
    // Code clips (codeclip.rs): the frame slots this range shows come from RAM, the disk cache or ONE headless-browser batch.
    // Sequential access (playback, export, frame loops) renders ahead in play order; random access renders exactly what is asked.
    for c in &flat {
        let Source::Code(cc) = &c.source else { continue };
        if !active.contains(&c.id) { continue; }
        let n = crate::codeclip::slot_count(c, p.fps);
        let (ka, kb) = if all { (0, n - 1) } else {
            let (a, b) = (c.display_time((t0 - c.start).clamp(0.0, c.duration)), c.display_time((t1 - c.start).clamp(0.0, c.duration)));
            (crate::codeclip::slot_of(a.min(b), p.fps).min(n - 1), crate::codeclip::slot_of(a.max(b), p.fps).min(n - 1)) };
        let cap = (decode_cache_limit() / 2 / (p.width as usize * p.height as usize * 4).max(1)).max(1);
        if kb - ka + 1 > cap { return Err(format!("limit_exceeded: code clip '{}' needs {} frames at once for this range, over its {cap}-frame memory budget; render a shorter range", c.id, kb - ka + 1)); }
        let dir = crate::codeclip::resolve_dir(p, cc);
        let tree = crate::codeclip::tree_hash(&dir)?;
        let sig = format!("{tree:032x}|{}|{}|{}|{}|{:?}|{:?}|{:?}", serde_json::to_string(cc).unwrap_or_default(), c.in_point, c.duration, c.speed,
            c.speed_curve.as_ref().map(|q| serde_json::to_string(q).unwrap_or_default()), c.time_remap.as_ref().map(|r| (serde_json::to_string(&r.curve).unwrap_or_default(), r.offset)), c.time_mode);
        let (last, mut have) = match cache.code.remove(&c.id) { Some((s, l, m)) if s == sig => (Some(l), m), _ => (None, Default::default()) };
        let forward = matches!(c.time_mode, TimeMode::Normal) && c.time_remap.is_none();
        let sequential = window > WINDOW || last.map(|l| ka == l + 1).unwrap_or(false);
        let kb2 = if !all && forward && sequential { (ka + window.min(cap) - 1).max(kb).min(n - 1) } else { kb };
        have.retain(|k, _| all || (*k >= ka && *k <= kb2));
        // Refill only when the range itself is not held, and then fill the whole window in one run: topping the window up by
        // one slot per frame would start a browser for every frame.
        let held = (ka..=kb).all(|k| have.contains_key(&k));
        let want: Vec<usize> = if held { vec![] } else { (ka..=kb2).filter(|k| !have.contains_key(k)).collect() };
        if !want.is_empty() { let (got, _) = crate::codeclip::frames_for(p, c, cc, &want)?; for (k, f) in got { have.insert(k, std::sync::Arc::new(f)); } }
        cache.code.insert(c.id.clone(), (sig, kb, have));
    }
    Ok(())
}

/// All clips including those inside precomps, with starts shifted onto the project clock (precomps nest recursively).
pub fn flat_clips(tracks: &[Track], off: f64) -> Vec<Clip> { let mut v = vec![]; for tr in tracks { for c in &tr.clips { let mut cc = Clip::clone(c); cc.start += off; if let Source::Precomp { tracks: inner } = &c.source { let sub = flat_clips(inner, cc.start); cc.source = Source::Adjustment; v.push(cc); v.extend(sub); } else { v.push(cc); } } } v }

fn source_frame(p: &Project, c: &Clip, lt_in: f64, cache: &Cache) -> Frame { source_frame_h(p, c, lt_in, cache).0 }
/// also returns the pixel region (x0,y0,x1,y1 exclusive) outside which the frame is exactly zero, when the source knows it (bounded plain rect)
fn source_frame_h(p: &Project, c: &Clip, lt_in: f64, cache: &Cache) -> (Frame, Option<(usize, usize, usize, usize)>) {
    let lt = c.display_time(lt_in); let mut hint: Option<(usize, usize, usize, usize)> = None;
    let (w, h) = (p.width as usize, p.height as usize);
    let mut f = Frame::new(w, h);
    let mut source_linear=false;
    match &c.source {
        Source::Solid { color } => { let fa = prop_or(c, "fill_alpha", lt, 1.0).clamp(0.0, 1.0) as f32; for i in 0..w*h { f.d[i*4]=color[0] as f32; f.d[i*4+1]=color[1] as f32; f.d[i*4+2]=color[2] as f32; f.d[i*4+3]=fa; } }, // fill_alpha: the colour's alpha (add_clip) or set_property; layer opacity composes with it
        Source::Gradient { c1, c2 } => { // end alphas from the colours (add_clip color/color2 alpha), interpolated premultiplied; x fill_alpha; layer opacity composes in blend
            let (a1, a2, fa) = (prop_or(c, "color_alpha", lt, 1.0).clamp(0.0, 1.0), prop_or(c, "color2_alpha", lt, 1.0).clamp(0.0, 1.0), prop_or(c, "fill_alpha", lt, 1.0).clamp(0.0, 1.0));
            // k = position from color (0) to color2 (1): vertical by default (unchanged), at gradient_angle (0 top->bottom, 90 left->right; the
            // corners reach 0 and 1), or radial (gradient_radial: centre 0, corners 1). Pixel centres for the new shapes.
            let radial = prop_or(c, "gradient_radial", lt, 0.0) >= 0.5; let ang = if c.props.contains_key("gradient_angle") { Some(prop_or(c, "gradient_angle", lt, 0.0).to_radians()) } else { None };
            let (wf, hf) = (w as f64, h as f64); let (sx, cy) = ang.map_or((0.0, 1.0), |r| (r.sin(), r.cos())); let span = (wf * sx).abs() + (hf * cy).abs(); let rmax = (wf * wf + hf * hf).sqrt() / 2.0;
            let pos = |x: usize, y: usize| -> f64 { if radial { let (dx, dy) = (x as f64 + 0.5 - wf / 2.0, y as f64 + 0.5 - hf / 2.0); ((dx * dx + dy * dy).sqrt() / rmax).clamp(0.0, 1.0) }
                else if ang.is_some() { (((x as f64 + 0.5 - wf / 2.0) * sx + (y as f64 + 0.5 - hf / 2.0) * cy) / span + 0.5).clamp(0.0, 1.0) } else { y as f64 / hf } };
            for y in 0..h { for x in 0..w { let k = pos(x, y); let i=(y*w+x)*4;
                if a1 == 1.0 && a2 == 1.0 { for j in 0..3 { f.d[i+j] = (c1[j]*(1.0-k)+c2[j]*k) as f32; } f.d[i+3]=fa as f32; continue; }
                let al = if a1 == a2 { a1 } else { a1*(1.0-k)+a2*k };
                for j in 0..3 { f.d[i+j] = if al > 0.0 { ((c1[j]*a1*(1.0-k)+c2[j]*a2*k)/al) as f32 } else { (c1[j]*(1.0-k)+c2[j]*k) as f32 }; } f.d[i+3]=(al*fa) as f32; } } },
        Source::Adjustment => {}
        Source::Precomp { tracks } => { let mut sub = Project { links: vec![], proxies: p.proxies.clone(), grade_scopes: p.grade_scopes.subset(tracks), lights: p.lights.clone(), materials: p.materials.clone(), editorial: p.editorial.clone(), ocio_view_delivery: None, occluders: p.occluders.clone(), color_pipeline: p.color_pipeline.clone(), hooks: Default::default(), expr_scripts: Default::default(), tracks: tracks.clone(), assets: p.assets.clone(), bins: p.bins.clone(), subclips: p.subclips.clone(), camera: p.camera.clone(), comp3d: p.comp3d.clone(), markers: vec![], name: String::new(), width: p.width, height: p.height, fps: p.fps, next_id: 0, deps: None, templates: vec![], base_dir: p.base_dir.clone(), master_speech_level: 0.0, master_speech_level_scope_end: 0.0 }; sub.name = c.id.clone(); match render_frame_opts(&sub, lt, cache, true) {Ok(fr)=>f=fr,Err(_)=>f.d.fill(f32::NAN)} }
        Source::Shape { shape, width, height, fill, stroke_width, stroke_color, sides, corner_radius } => {
            let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0); let (hw, hh) = (width / 2.0, height / 2.0);
            // signed distance (negative inside), then shape modifiers (offset/repeater/merge) from shapemods.rs
            let pth = if shape == "path" { crate::shapemods::path_of(&c.effects, lt) } else { None };
            // Round Corners (add_path_operator) acts on the geometry: rect corner radius, exact rounded polygon
            let rc = c.effects.iter().find(|e| e.kind == "shape_op_round_corners").map_or(0.0, |e| e.params.get("radius").map_or(10.0, |p| p.at(lt)).max(0.0));
            let corner_radius = &if rc > 0.0 && shape.as_str() == "rect" { (corner_radius + rc).min(hw.min(hh)) } else { *corner_radius };
            let base = |x: f64, y: f64| -> f64 { let (px, py) = (x - cx, y - cy); if rc > 0.0 && shape.as_str() == "polygon" { return crate::pathops::sd_polygon_rounded(*sides, hw.min(hh), rc, px, py); } match shape.as_str() {
                "path" => match &pth { Some(p) => p.sd(px, py), None => 1e9 },
                "ellipse" | "polygon" | "line" => crate::shapemods::sd(shape, *sides, hw, hh, *corner_radius, px, py),
                _ => { let (qx, qy) = (px.abs() - hw + corner_radius, py.abs() - hh + corner_radius); (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - corner_radius } } };
            let stack = crate::shapemods::build(&c.effects, base, cx, cy, lt); let sd = |x: f64, y: f64| (stack.dist)(x, y);
            // plain rect (exact SDF; the ellipse SDF is approximate and leaks past its box at extreme aspect ratios, found by shape_bound_equiv; no shape modifiers): every pixel farther than margin from the shape's bounding box has d >= 0.5 + stroke/2 and writes nothing, so skip it (bit-identical)
            let (mut bx0, mut bx1, mut by0, mut by1) = (0usize, w, 0usize, h);
            if SHAPE_BOUND.load(std::sync::atomic::Ordering::Relaxed) && !c.effects.iter().any(|e| e.kind.starts_with("shape_")) && shape.as_str() == "rect" { let m = 0.5 + stroke_width.max(0.0) / 2.0 + 4.0; bx0 = ((cx - hw - m).floor().max(0.0)) as usize; bx1 = (((cx + hw + m).ceil()).max(0.0) as usize).min(w); by0 = ((cy - hh - m).floor().max(0.0)) as usize; by1 = (((cy + hh + m).ceil()).max(0.0) as usize).min(h); hint = Some((bx0, by0, bx1, by1)); }
            let (fa, sa) = (prop_or(c, "fill_alpha", lt, 1.0).clamp(0.0, 1.0), prop_or(c, "stroke_alpha", lt, 1.0).clamp(0.0, 1.0)); // fill alpha, stroke alpha and layer opacity are three separate properties
            for y in by0..by1 { for x in bx0..bx1 { let d = sd(x as f64 + 0.5, y as f64 + 0.5); let inside = (0.5 - d).clamp(0.0, 1.0); if inside <= 0.0 && *stroke_width <= 0.0 { continue; }
                let sk = if *stroke_width > 0.0 { (0.5 - (d.abs() - stroke_width / 2.0)).clamp(0.0, 1.0) * stack.trim_gate(x as f64 + 0.5 - cx, y as f64 + 0.5 - cy) } else { 0.0 }; let stroke_color = &stack.stroke_color(x as f64 + 0.5 - cx, y as f64 + 0.5 - cy, *width, *height, *stroke_color); let fill_color = &stack.fill_color(x as f64 + 0.5 - cx, y as f64 + 0.5 - cy, *width, *height, [fill[0], fill[1], fill[2]]); let fl = if fill[0] < 0.0 { 0.0 } else if *stroke_width > 0.0 { (0.5 - (d + stroke_width / 2.0)).clamp(0.0, 1.0) } else { inside };
                let (fl, sk) = (fl * fa, sk * sa);
                let i = (y * w + x) * 4; let a = (fl + sk * (1.0 - fl)).clamp(0.0, 1.0); if a <= 0.0 { continue; }
                for j in 0..3 { f.d[i + j] = ((fill_color[j] * fl + stroke_color[j] * sk * (1.0 - fl)) / a) as f32; } f.d[i + 3] = a as f32; } }
        }
        Source::Emitter(e) => { let alpha_of = |id: &str| silhouette_alpha(p, id, c.start + lt, cache);
            let emit = e.shape == "layer" && !e.emit_clip.is_empty(); if emit { crate::particles::set_emit_mask(alpha_of(&e.emit_clip), e.emit_edge);
                if e.emit_follow { let bw = crate::particles::EMIT_BIN; let lmax = e.life + e.life_var.abs(); let b1 = (lt / bw).floor() as i64; let b0 = if e.prewarm { ((lt - lmax) / bw).floor() as i64 } else { ((lt - lmax).max(0.0) / bw).floor() as i64 };
                    let bins = (b0..=b1).map(|b| (b, silhouette_alpha(p, &e.emit_clip, c.start + ((b as f64 + 0.5) * bw).max(0.0), cache))).collect(); crate::particles::set_emit_bins(bins, e.emit_edge); } }
            let has = e.colliders.iter().any(|c| c.kind == "layer");
            if has { let ms = e.colliders.iter().map(|cc| if cc.kind == "layer" { silhouette_alpha(p, &cc.clip_id, c.start + lt, cache) } else { None }).collect(); crate::particles::set_layer_masks(ms);
                if e.colliders.iter().any(|c| c.kind == "layer" && c.follow) { let bw = crate::particles::EMIT_BIN; let lmax = e.life + e.life_var.abs(); let b1 = (lt / bw).floor() as i64; let b0 = if e.prewarm { ((lt - lmax) / bw).floor() as i64 } else { ((lt - lmax).max(0.0) / bw).floor() as i64 };
                    let mut bins = vec![]; for (ci, cc) in e.colliders.iter().enumerate() { if cc.kind == "layer" && cc.follow { for b in b0..=b1 { bins.push((ci, b, silhouette_alpha(p, &cc.clip_id, c.start + ((b as f64 + 0.5) * bw).max(0.0), cache))); } } } crate::particles::set_layer_bins(bins); } }
            crate::particles::render_fps(e, &mut f, lt, p.fps); if has { crate::particles::set_layer_masks(Vec::new()); crate::particles::clear_layer_bins(); } if emit { crate::particles::set_emit_mask(None, false); crate::particles::clear_emit_bins(); } }
        Source::SpeedLines(s) => crate::speedlines::render(s, &mut f, lt),
        Source::GlitchConfetti(s) => crate::confetti::render(s, &mut f, lt),
        Source::Particles3D(e) => crate::particles3d::render_layer(p, c, e, lt, &mut f, cache),
        Source::Scene3D(e) => { let t0 = c.start + lt; crate::scene3d::render_blur(e, &mut f, lt, p.fps, &|off| crate::three_d::Camera::from_props(&p.camera, t0 + off)) }
        Source::ShardBurst(e) => { if let Some(tid) = e.plane.as_ref().and_then(|pl| pl.occlusion_target.as_ref()) { if let Some(occ) = target_screen_occ(p, tid, c.start + lt) { crate::shards::EXT_OCC.with(|cell| *cell.borrow_mut() = Some(occ)); } } let ctx = crate::shards::RenderCtx { work_linear: p.color_pipeline.enabled && p.color_pipeline.linear_resample }; crate::shards::render_ctx(&ctx, e, &mut f, lt, &crate::three_d::Camera::from_props(&p.camera, c.start + lt), p.fps, c.duration) }
        Source::Particles { kind, count, color, size, seed } => {
            let rnd = |i: u64, k: u64| { let mut x = (seed ^ i.wrapping_mul(0x9E3779B97F4A7C15) ^ k.wrapping_mul(0xBF58476D1CE4E5B9)).wrapping_mul(0x94D049BB133111EB); x ^= x >> 29; (x >> 11) as f64 / (1u64 << 53) as f64 };
            let (wf, hf) = (w as f64, h as f64);
            for i in 0..*count as u64 {
                let (r0, r1, r2, r3) = (rnd(i, 1), rnd(i, 2), rnd(i, 3), rnd(i, 4));
                let life = 1.0 + r3 * 2.0; let ph = (lt / life + r0).fract();
                let (x, y, s, a) = match kind.as_str() {
                    "snow" => (((r1 + 0.05 * (lt * 2.0 + r2 * 6.0).sin()) * wf), ph * hf, size * (0.5 + r2), 0.8),
                    "rain" => (r1 * wf, ph * hf * 1.2 - 0.1 * hf, size * 0.4, 0.5),
                    "embers" | "sparks" => { let sp = if kind == "sparks" { 2.5 } else { 1.0 }; (r1 * wf + (r2 - 0.5) * wf * 0.3 * ph * sp, hf - ph * hf * (0.5 + r2 * 0.6) * sp, size * (1.0 - ph) * (0.5 + r3), (1.0 - ph).powf(1.5)) }
                    _ => ((r1 + 0.03 * (lt + r2 * 9.0).sin()) * wf, ((r2 + 0.02 * lt * (0.5 + r3)).fract()) * hf, size * (0.5 + r3), 0.35 * (0.5 + 0.5 * (lt * 2.0 + r0 * 6.28).sin())),
                };
                let rad = s.max(0.5); let (x0, x1, y0, y1) = ((x - rad * 2.5).max(0.0) as usize, ((x + rad * 2.5) as usize).min(w - 1), (y - rad * 2.5).max(0.0) as usize, ((y + rad * 2.5) as usize).min(h - 1));
                for yy in y0..=y1 { for xx in x0..=x1 { let d = (((xx as f64 - x).powi(2) + (yy as f64 - y).powi(2)).sqrt()) / rad; let k = (a * (1.0 - d / 2.5).max(0.0).powi(2)) as f32; if k <= 0.0 { continue; }
                    let o = (yy * w + xx) * 4; for j in 0..3 { f.d[o + j] = (f.d[o + j] * f.d[o + 3] * (1.0 - k) + color[j] as f32 * k) / (f.d[o + 3] + k * (1.0 - f.d[o + 3])).max(1e-6); } f.d[o + 3] += k * (1.0 - f.d[o + 3]); } }
            }
        }
        Source::Text { text, font, size, color, letter_spacing, stroke_width, stroke_color, shadow, align } => {
            let text = &dyn_text(text, c.start + lt, p.fps);
            let _ = crate::text::render_text(&mut f, text, font, *size, *color, *letter_spacing, *stroke_width, *stroke_color, *shadow, align, prop(c, "reveal", lt), c.text_fx.as_ref(), lt);
        }
        Source::Code(_) => if let Some((_, _, m)) = cache.code.get(&c.id) { // straight-alpha sRGB RGBA8 at canvas size, like decoded media
            let k = crate::codeclip::slot_of(lt, p.fps).min(crate::codeclip::slot_count(c, p.fps) - 1);
            if let Some(b) = m.get(&k) { for (i, v) in b.iter().enumerate() { f.d[i] = *v as f32 / 255.0; } }
        },
        Source::Media { .. } => if let Some(fr) = cache.frames_of(&c.id) {
            source_linear=fr.working_linear;
            let idx = media_index(c,lt,p.fps);
            if let Some(b)=fr.get_float(idx){f.d.copy_from_slice(b)}else if let Some(b) = fr.get(idx) { for (i, v) in b.iter().enumerate() { f.d[i] = *v as f32 / 255.0; } }
            // Time remap between two source frames: frame mix or optical-flow interpolation, in linear light.
            // A speed ramp with Time Interpolation (speed_blend) blends the same way between the two source frames around its source time.
            let tween = match (&c.time_remap, c.remap_pos(lt, p.fps)) { (Some(r), Some((_, nb, wb))) => Some((nb, wb, r.blend)), _ => c.ramp_pos(lt, p.fps).map(|(_, nb, wb)| (nb, wb, c.speed_blend)) };
            if let Some((nb, wb, blend)) = tween { if wb > 0.0 && blend != crate::model::FrameBlend::Off && fr.len() > 0 && nb < fr.first + fr.len() { let mut g2 = Frame::new(w, h);
                if let Some(b)=fr.get_float(nb){g2.d.copy_from_slice(b)}else if let Some(b)=fr.get(nb){for (i,v) in b.iter().enumerate(){g2.d[i]=*v as f32/255.;}}
                if p.color_pipeline.enabled && !source_linear {let a=crate::color::pipeline::input(p,c,&mut f);let b=crate::color::pipeline::input(p,c,&mut g2);source_linear=true;if a.is_err()||b.is_err(){f.d.fill(f32::NAN);}}
                let srgb = !p.color_pipeline.enabled && !source_linear;
                if blend == crate::model::FrameBlend::OpticalFlow { f = crate::flow::flow_interpolate(&f, &g2, wb as f32, srgb, 3); } else { let a = f.d.clone(); crate::flow::mix_linear(&a, &g2.d, wb, srgb, &mut f.d); } } }
            let fb = prop(c, "flow_blur", lt) as f32;
            if fb > 0.0 && c.props.get("flow_mode").map(|v|v.base.round()==1.).unwrap_or(false) {
                let load=|i:usize|->Option<Frame>{if !fr.covers(i,i){return None}let mut g=Frame::new(w,h);if let Some(b)=fr.get_float(i){g.d.copy_from_slice(b)}else if let Some(nb)=fr.get(i){for (k,v) in nb.iter().enumerate(){g.d[k]=*v as f32/255.;}}else{return None}Some(g)};
                let (mut pv,mut nx)=(if idx>0{load(idx-1)}else{None},load(idx+1));
                if p.color_pipeline.enabled && !source_linear {let mut bad=crate::color::pipeline::input(p,c,&mut f).is_err();for g in [&mut pv,&mut nx].into_iter().flatten(){bad|=crate::color::pipeline::input(p,c,g).is_err();}source_linear=true;if bad{f.d.fill(f32::NAN);}}
                let q = (prop(c, "flow_quality", lt) as usize).clamp(3,6); let amt=fb.clamp(0.,2.); let ph=c.props.get("flow_phase").map(|v|(v.at(lt)/360.) as f32).unwrap_or(-amt/2.);
                let cv=crate::shutter::curve_from_props(c.props.get("flow_curve").map(|v|v.base),c.props.get("flow_curve_pts"));
                let src = Frame { w, h, d: f.d.clone() }; f = crate::flow::vector_blur(pv.as_ref(), &src, nx.as_ref(), amt, ph, q, &cv); }
            else if fb > 0.0 { let nxt=if fr.len()>0 {Some(idx+1)}else{None};
                if let Some(next_idx) = nxt { let mut g2 = Frame::new(w, h);
                    if let Some(b)=fr.get_float(next_idx){g2.d.copy_from_slice(b)}else if let Some(nb)=fr.get(next_idx){for (i,v) in nb.iter().enumerate(){g2.d[i]=*v as f32/255.;}}
                    if p.color_pipeline.enabled && !source_linear {let a=crate::color::pipeline::input(p,c,&mut f);let b=crate::color::pipeline::input(p,c,&mut g2);source_linear=true;if a.is_err()||b.is_err(){f.d.fill(f32::NAN);}}
                    let q = (prop(c, "flow_quality", lt) as usize).clamp(3,6);
                    let src = Frame { w, h, d: f.d.clone() }; f = crate::flow::flow_blur(&src, &g2, fb.clamp(0.,2.), q); } }
        },
    }
    if p.color_pipeline.enabled && !source_linear && !matches!(c.source, Source::Precomp{..}|Source::Adjustment) {
        if crate::color::pipeline::input(p,c,&mut f).is_err(){f.d.fill(f32::NAN);}
    }
     (f, hint)
}

/// The whole 2D transform of a clip at clip-local time `lt`, the ONE place the renderer reads x, y, scale, rotation and opacity (A5: parenting composes here).
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Xform { pub x: f64, pub y: f64, pub scale: f64, pub rotation: f64, pub opacity: f64 }
/// Clip-local time at timeline time `t`, exactly as the renderer derives it (posterize-aware, never negative).
pub fn clip_local_time(c: &Clip, t: f64) -> f64 { if c.posterize_fps > 0.0 { (((t - c.start) * c.posterize_fps + 1e-6).floor() / c.posterize_fps).max(0.0) } else { (t - c.start).max(0.0) } }
/// The clip's own (local) transform at clip-local time `lt`, ignoring any parent.
pub fn local_xform(c: &Clip, lt: f64) -> Xform { /* xform-lint: ok */ Xform { x: prop(c, "x", lt), y: prop(c, "y", lt), scale: prop(c, "scale", lt), rotation: prop(c, "rotation", lt), opacity: prop(c, "opacity", lt) } }
/// parent o child, in the renderer's convention (transform_bb: f = c + d + R(rot) S (q - c), y down): d_w = d_P + R_P S_P d_c, scales multiply, rotations add, opacities multiply. `inh` switches each part off.
pub fn compose_xform(pw: &Xform, c: &Xform, inh: &crate::model::Inherit) -> Xform {
    let (mut ox, mut oy) = (c.x, c.y); if inh.scale { ox *= pw.scale; oy *= pw.scale; } if inh.rotation { let r = pw.rotation.to_radians(); let (sn, co) = (r.sin(), r.cos()); let (rx, ry) = (ox * co - oy * sn, ox * sn + oy * co); ox = rx; oy = ry; }
    Xform { x: if inh.position { pw.x + ox } else { c.x }, y: if inh.position { pw.y + oy } else { c.y }, scale: if inh.scale { pw.scale * c.scale } else { c.scale }, rotation: if inh.rotation { pw.rotation + c.rotation } else { c.rotation }, opacity: if inh.opacity { pw.opacity * c.opacity } else { c.opacity } }
}
pub fn resolve_xform(p: &Project, c: &Clip, lt: f64) -> Xform {
    let me = local_xform(c, lt); if c.parent.is_none() { return me; }
    let t = c.start + lt; let mut chain: Vec<&Clip> = vec![]; let mut cur = c;
    for _ in 0..64 { match cur.parent.as_deref().and_then(|id| p.find_clip(id)) { Some(pc) if pc.id != c.id && !chain.iter().any(|x| x.id == pc.id) => { chain.push(pc); cur = pc; } _ => break } }
    if chain.is_empty() { return me; }
    let root = chain[chain.len() - 1]; let mut w = local_xform(root, clip_local_time(root, t));
    for node in chain[..chain.len() - 1].iter().rev() { w = compose_xform(&w, &local_xform(node, clip_local_time(node, t)), &node.inherit); }
    compose_xform(&w, &me, &c.inherit)
}
// thread-local-ok: frame-scoped parented-pose memo; MemoScope clears it at frame entry and restores the previous value on drop (also on error/panic), so nothing survives a render
thread_local! { static XMEMO: std::cell::RefCell<Option<std::collections::HashMap<(usize, u64, u64), (String, Xform)>>> = const { std::cell::RefCell::new(None) }; }
/// Frame-scoped memo for parented clips: the project cannot change inside one render_frame call, so (project, clip id, lt) fixes the resolved transform. Active only between `MemoScope::enter` and drop on this thread; outside a frame every call resolves afresh.
pub struct MemoScope(Option<std::collections::HashMap<(usize, u64, u64), (String, Xform)>>);
impl MemoScope { pub fn enter() -> MemoScope { MemoScope(XMEMO.with(|m| m.borrow_mut().replace(Default::default()))) } }
impl Drop for MemoScope { fn drop(&mut self) { let prev = self.0.take(); XMEMO.with(|m| *m.borrow_mut() = prev); } }
fn id_hash(s: &str) -> u64 { let mut h: u64 = 0xcbf29ce484222325; for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); } h }
fn resolve_memo(p: &Project, c: &Clip, lt: f64) -> Xform {
    let key = (p as *const Project as usize, id_hash(&c.id), lt.to_bits());
    if let Some(x) = XMEMO.with(|m| m.borrow().as_ref().and_then(|h| h.get(&key).filter(|(id, _)| *id == c.id).map(|(_, x)| *x))) { return x; }
    let x = resolve_xform(p, c, lt); XMEMO.with(|m| if let Some(h) = m.borrow_mut().as_mut() { h.insert(key, (c.id.clone(), x)); }); x
}
/// One field of `resolve_xform`. Without a parent it evaluates only the requested field (same cost and values as the old direct prop read).
fn xprop(p: &Project, c: &Clip, k: &str, lt: f64) -> f64 {
    if c.parent.is_some() { let t = resolve_memo(p, c, lt); return match k { "x" => t.x, "y" => t.y, "scale" => t.scale, "rotation" => t.rotation, _ => t.opacity }; }
    /* xform-lint: ok */ match k { "x" => prop(c, "x", lt), "y" => prop(c, "y", lt), "scale" => prop(c, "scale", lt), "rotation" => prop(c, "rotation", lt), _ => prop(c, "opacity", lt) } }
fn prop(c: &Clip, k: &str, lt: f64) -> f64 { c.props.get(k).map(|x| x.at(lt)).unwrap_or(0.0) }
/// like prop, but a missing property is `d` (fill_alpha / stroke_alpha default to 1 so old projects render as before)
fn prop_or(c: &Clip, k: &str, lt: f64, d: f64) -> f64 { c.props.get(k).map(|x| x.at(lt)).unwrap_or(d) }

/// Inverse-mapped transform (scale/rotate/translate around centre) with alpha-correct bilinear sampling and
/// supersampling (2x2 for any resample, up to 4x4 when minifying) so downscaled and rotated layers are antialiased.
/// Alpha bounding box (x0,y0,x1,y1) of a frame, None if fully transparent.
/// Kill switch for the bounded shape raster (default on); tests prove on/off output is bit-identical.
static SHAPE_BOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub fn set_shape_bound(on: bool) { SHAPE_BOUND.store(on, std::sync::atomic::Ordering::Relaxed); }
static SHAPE_HINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub fn set_shape_hint(on: bool) { SHAPE_HINT.store(on, std::sync::atomic::Ordering::Relaxed); }
/// tight alpha bbox when every pixel outside region (x0,y0,x1,y1 exclusive) is known to be zero: same result as alpha_bbox, scans only the region
fn alpha_bbox_in(src: &Frame, r: (usize, usize, usize, usize)) -> Option<(usize, usize, usize, usize)> { let w = src.w; let (rx0, ry0, rx1, ry1) = (r.0.min(src.w), r.1.min(src.h), r.2.min(src.w), r.3.min(src.h)); let (mut x0, mut y0, mut x1, mut y1) = (w, src.h, 0usize, 0usize); let mut any = false;
    for y in ry0..ry1 { let row = &src.d[y * w * 4..(y + 1) * w * 4]; let mut first = None; let mut last = 0; for x in rx0..rx1 { if row[x * 4 + 3] > 0.0 { if first.is_none() { first = Some(x); } last = x; } } if let Some(f0) = first { any = true; x0 = x0.min(f0); x1 = x1.max(last); y0 = y0.min(y); y1 = y1.max(y); } }
    if any { Some((x0, y0, x1, y1)) } else { None } }
fn alpha_bbox(src: &Frame) -> Option<(usize, usize, usize, usize)> { let (w, h) = (src.w, src.h); let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize); let mut any = false;
    for y in 0..h { let row = &src.d[y * w * 4..(y + 1) * w * 4]; let mut first = None; let mut last = 0; for x in 0..w { if row[x * 4 + 3] > 0.0 { if first.is_none() { first = Some(x); } last = x; } } if let Some(f0) = first { any = true; x0 = x0.min(f0); x1 = x1.max(last); y0 = y0.min(y); y1 = y1.max(y); } }
    if any { Some((x0, y0, x1, y1)) } else { None } }
fn transform(src: &Frame, scale: f64, rot_deg: f64, dx: f64, dy: f64) -> Frame { transform_bb(src, alpha_bbox(src), scale, rot_deg, dx, dy) }
thread_local! { static BACKDROP: std::cell::RefCell<Option<std::rc::Rc<Frame>>> = Default::default(); }
/// Composite of all layers below the clip currently being rendered. Only set while an effect that declares `needs_backdrop` runs; None otherwise.
pub fn backdrop() -> Option<std::rc::Rc<Frame>> { BACKDROP.with(|b| b.borrow().clone()) }
thread_local! { static POOL: std::cell::RefCell<Vec<(Vec<f32>, (usize, usize, usize, usize))>> = Default::default(); }
/// Reusable layer buffers: a returned buffer is zero except inside its dirty rect, which is cleared on reuse (avoids a fresh 14 MB page-faulted allocation per layer).
fn pool_take(w: usize, h: usize) -> Frame { let n = w * h * 4; let hit = POOL.with(|p| { let mut p = p.borrow_mut(); p.iter().position(|(v, _)| v.len() == n).map(|i| p.swap_remove(i)) });
    match hit { Some((mut v, (x0, y0, x1, y1))) => { for y in y0..=y1.min(h - 1) { v[(y * w + x0) * 4..(y * w + x1.min(w - 1) + 1) * 4].fill(0.0); } Frame { w, h, d: v } }, None => Frame::new(w, h) } }
fn pool_give(f: Frame, rect: (usize, usize, usize, usize)) { POOL.with(|p| { let mut p = p.borrow_mut(); if p.len() < 3 { p.push((f.d, rect)); } }); }
thread_local! { static FAST_NS: std::cell::Cell<bool> = Default::default(); }
thread_local! { static LAST_RECT: std::cell::Cell<Option<(usize, usize, usize, usize)>> = Default::default(); }
fn transform_bb(src: &Frame, bb: Option<(usize, usize, usize, usize)>, scale: f64, rot_deg: f64, dx: f64, dy: f64) -> Frame {
    if (scale - 1.0).abs() < 1e-9 && rot_deg == 0.0 && dx == 0.0 && dy == 0.0 { return Frame { w: src.w, h: src.h, d: src.d.clone() }; }
    LAST_RECT.with(|l| l.set(None)); let tp0 = std::time::Instant::now(); let (w, h) = (src.w, src.h); let mut o = pool_take(w, h); prof("tb_alloc", tp0);
    let (cx, cy) = (w as f64/2.0, h as f64/2.0); let r = rot_deg.to_radians(); let (s, co) = (r.sin(), r.cos());
    let inv = 1.0 / scale.max(1e-6);
    let pure_shift = (scale - 1.0).abs() < 1e-9 && rot_deg == 0.0 && dx.fract() == 0.0 && dy.fract() == 0.0;
    let ns = if pure_shift || FAST_NS.with(|l| l.get()) { 1 } else { ((inv.ceil() as usize).max(2)).min(4) }; let nsf = ns as f64; let wgt = 1.0 / (ns * ns) as f32;
    // only visit output pixels that can receive content: forward-map the source's opaque bounding box
    let (bx0, by0, bx1, by1) = match bb { Some(b) => b, None => return o };
    let (mut ox0, mut oy0, mut ox1, mut oy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (qx, qy) in [(bx0 as f64 - 1.0, by0 as f64 - 1.0), (bx1 as f64 + 2.0, by0 as f64 - 1.0), (bx0 as f64 - 1.0, by1 as f64 + 2.0), (bx1 as f64 + 2.0, by1 as f64 + 2.0)] {
        let (ux, uy) = ((qx - cx) * scale, (qy - cy) * scale); let fx = ux * co - uy * s + cx + dx; let fy = ux * s + uy * co + cy + dy; ox0 = ox0.min(fx); oy0 = oy0.min(fy); ox1 = ox1.max(fx); oy1 = oy1.max(fy); }
    let (rx0, ry0) = ((ox0.floor() - 2.0).max(0.0) as usize, (oy0.floor() - 2.0).max(0.0) as usize); let (rx1, ry1) = (((ox1.ceil() + 2.0).min(w as f64 - 1.0)).max(0.0) as usize, ((oy1.ceil() + 2.0).min(h as f64 - 1.0)).max(0.0) as usize);
    if ox1 < 0.0 || oy1 < 0.0 || ox0 > w as f64 || oy0 > h as f64 { return o; }
    LAST_RECT.with(|l| l.set(Some((rx0, ry0, rx1, ry1))));
    // rot==0: sx depends only on (x, sub-x) and sy only on (y, sub-y); precompute (exactly the same f64 expressions, since px*1.0+py*0.0 == px)
    let axis_aligned = rot_deg == 0.0;
    let xt: Vec<f64> = if axis_aligned { (0..w * ns).map(|k| { let (x, sx_) = (k / ns, k % ns); let px = x as f64 + (sx_ as f64 + 0.5) / nsf - 0.5 - cx - dx; (px * co + 0.0 * s) * inv + cx }).collect() } else { vec![] };
    let yt: Vec<f64> = if axis_aligned { (0..h * ns).map(|k| { let (y, sy_) = (k / ns, k % ns); let py = y as f64 + (sy_ as f64 + 0.5) / nsf - 0.5 - cy - dy; (-0.0 * s + py * co) * inv + cy }).collect() } else { vec![] };
    let yy_row = |y: usize, _r0: usize| y;
    let (sd, ow) = (&src.d, w); let tpl = std::time::Instant::now();
    crate::par::rows_mut(&mut o.d[ry0 * w * 4..(ry1 + 1) * w * 4], w * 4, |yy, orow| { let y = ry0 + yy; for x in rx0..=rx1 {
        let mut acc = [0f32; 4];
        for sy_ in 0..ns { for sx_ in 0..ns {
            let (sx, sy) = if axis_aligned { (xt[x * ns + sx_], yt[yy_row(y, ry0) * ns + sy_]) } else {
            let px = x as f64 + (sx_ as f64 + 0.5) / nsf - 0.5 - cx - dx; let py = y as f64 + (sy_ as f64 + 0.5) / nsf - 0.5 - cy - dy;
            ((px*co + py*s)*inv + cx, (-px*s + py*co)*inv + cy) };
            if sx < -0.5 || sy < -0.5 || sx > w as f64 - 0.5 || sy > h as f64 - 0.5 { continue; }
            let (sx, sy) = (sx.clamp(0.0, (w-1) as f64), sy.clamp(0.0, (h-1) as f64));
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize); let (x1, y1) = ((x0+1).min(w-1), (y0+1).min(h-1));
            let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
            let wts = [(x0, y0, (1.0-fx)*(1.0-fy)), (x1, y0, fx*(1.0-fy)), (x0, y1, (1.0-fx)*fy), (x1, y1, fx*fy)];
            for (qx, qy, k) in wts { let i = (qy*w+qx)*4; let a = src.d[i+3]; let ka = k * a; acc[0] += src.d[i]*ka; acc[1] += src.d[i+1]*ka; acc[2] += src.d[i+2]*ka; acc[3] += ka; }
        }}
        let a = acc[3]; let oi = x*4; if a > 1e-6 { orow[oi] = acc[0]/a; orow[oi+1] = acc[1]/a; orow[oi+2] = acc[2]/a; orow[oi+3] = a * wgt; }
    } });
    prof("tb_loop", tpl); let _ = ow; o
}

/// Stabilize, Synthesize Edges (stabilize.rs Framing::SynthesizeEdges, AE "Stabilize, Synthesize Edges"). Pixels of the transformed
/// frame `f` with alpha < 1 are filled by compositing UNDER them (premultiplied: out = cur + (1 - cur.a) nb) the clip's neighbouring
/// source frames k = +1, -1, +2, -2, ... up to +-stab_fill, each warped into this output by W_k = A_t B_t^-1 S_t S_u^-1 B_u at
/// u = lt + k / fps: B_u is the clip's own x/y/scale/rotation at u, S_u the smooth path rebuilt from stab_ss/sr/sx/sy the same way,
/// A_t = (scale, rotation, dx, dy) the transform drawn now (== B_t unless a parent, motion path or transition adds to it).
/// The neighbour is this clip's decoded media frame at u (`source_frame`: time mode, speed, fit, flow blur, colour input stage)
/// WITHOUT the clip's effects and masks. Neighbours outside the clip or the decoded window (prepare_window widens it by stab_fill)
/// are skipped; stops once every pixel is opaque. `f` holds straight colour (transform_bb's convention), and so does the result.
fn synth_edges(p: &Project, c: &Clip, f: &mut Frame, lt: f64, a_t: (f64, f64, f64, f64), cache: &Cache) {
    const OPAQUE: f32 = 1.0 - 1e-6;
    let n = prop(c, "stab_fill", lt).min(30.0) as i64; if n < 1 { return; }
    let open = |g: &Frame| g.d.chunks_exact(4).any(|px| px[3] < OPAQUE); if !open(&*f) { return; }
    let (cx, cy) = (f.w as f64 / 2.0, f.h as f64 / 2.0);
    // similarity p -> [a -b; b a] p + (tx, ty) as [a, b, tx, ty], from transform_bb's (scale, rotation deg, x, y) about the centre
    let sim = |s: f64, r: f64, x: f64, y: f64| -> [f64; 4] { let (a, b) = (s * r.to_radians().cos(), s * r.to_radians().sin()); [a, b, cx + x - (a * cx - b * cy), cy + y - (b * cx + a * cy)] };
    let mul = |m: [f64; 4], q: [f64; 4]| -> [f64; 4] { [m[0] * q[0] - m[1] * q[1], m[1] * q[0] + m[0] * q[1], m[0] * q[2] - m[1] * q[3] + m[2], m[1] * q[2] + m[0] * q[3] + m[3]] };
    let inv = |m: [f64; 4]| -> [f64; 4] { let d = (m[0] * m[0] + m[1] * m[1]).max(1e-300); let (a, b) = (m[0] / d, -m[1] / d); [a, b, -(a * m[2] - b * m[3]), -(b * m[2] + a * m[3])] };
    // the stabilizer keyed B_u in the clip's LOCAL props (parenting enters through A_t), so they are read directly here
    let b_at = |u: f64| /* xform-lint: ok */ sim(prop(c, "scale", u), prop(c, "rotation", u), prop(c, "x", u), prop(c, "y", u));
    let s_at = |u: f64| sim(prop(c, "stab_ss", u), prop(c, "stab_sr", u), prop(c, "stab_sx", u), prop(c, "stab_sy", u));
    let pre = mul(sim(a_t.0, a_t.1, a_t.2, a_t.3), mul(inv(b_at(lt)), s_at(lt)));
    let fr = cache.frames_of(&c.id);
    for j in 0..2 * n { let k = if j % 2 == 0 { j / 2 + 1 } else { -(j / 2 + 1) };
        let u = lt + k as f64 / p.fps; if u < 0.0 || u >= c.duration { continue; }
        // only frames inside the decoded window: Frames::get clamps, which would composite the wrong frame
        let idx = media_index(c, c.display_time(u), p.fps); if !fr.is_some_and(|fr| fr.covers(idx, idx)) { continue; }
        let wk = mul(pre, mul(inv(s_at(u)), b_at(u)));
        let (sc, rot) = (wk[0].hypot(wk[1]), wk[1].atan2(wk[0]).to_degrees()); let (dx, dy) = (wk[0] * cx - wk[1] * cy + wk[2] - cx, wk[1] * cx + wk[0] * cy + wk[3] - cy);
        let src = source_frame(p, c, u, cache); let nb = transform_bb(&src, alpha_bbox(&src), sc, rot, dx, dy);
        for (px, q) in f.d.chunks_exact_mut(4).zip(nb.d.chunks_exact(4)) { let (ca, na) = (px[3], q[3]); if ca >= OPAQUE || na <= 0.0 { continue; }
            let kb = (1.0 - ca) * na; let oa = ca + kb; for i in 0..3 { px[i] = (px[i] * ca + q[i] * kb) / oa; } px[3] = oa; }
        if !open(&*f) { break; }
    }
}

/// Blend modes the compositor knows, in set_blend's schema order. The first five keep their original arithmetic; the rest
/// follow W3C Compositing and Blending Level 1: B(cb, cs) on unpremultiplied colour clamped to 0..1, cs' = (1 - ab) cs + ab B,
/// then source-over with the layer's alpha (the canvas holds premultiplied colour, sources are straight).
pub const BLEND_MODES: &[&str] = &["normal", "add", "multiply", "screen", "overlay", "darken", "lighten", "difference", "exclusion", "soft_light", "hard_light", "color_dodge", "color_burn", "linear_burn", "linear_dodge", "vivid_light", "linear_light", "pin_light", "hue", "saturation", "color", "luminosity"];
fn blend(dst: &mut Frame, src: &Frame, opacity: f32, mode: &str, mask: Option<&dyn Fn(usize, usize) -> f32>) { blend_region(dst, src, opacity, mode, mask, None) }
/// `region` (x0,y0,x1,y1 inclusive) must contain every pixel of `src` with alpha > 0; pixels outside are skipped without scanning (bit-identical to a full scan).
fn blend_region(dst: &mut Frame, src: &Frame, opacity: f32, mode: &str, mask: Option<&dyn Fn(usize, usize) -> f32>, region: Option<(usize, usize, usize, usize)>) {
    let w = dst.w; let m_id = match mode { "add" => 1, "multiply" => 2, "screen" => 3, "overlay" => 4, m => BLEND_MODES.iter().position(|x| *x == m).filter(|&k| k >= 5).unwrap_or(0) };
    let (ry0, ry1, rx0, rx1) = match region { Some((a, b, c, d)) => (b.min(dst.h), (d + 1).min(dst.h), a.min(w), (c + 1).min(w)), None => (0, dst.h, 0, w) };
    for y in ry0..ry1 {
        let srow = &src.d[y * w * 4..(y + 1) * w * 4];
        // skip the transparent margins of the row (most layers are small)
        let (mut x0, mut x1) = (rx0, rx1); while x0 < rx1 && srow[x0 * 4 + 3] <= 0.0 { x0 += 1; } if x0 == rx1 { continue; } while x1 > x0 && srow[(x1 - 1) * 4 + 3] <= 0.0 { x1 -= 1; }
        for x in x0..x1 { let i = (y * w + x) * 4;
            let a = srow[x * 4 + 3] * opacity * if let Some(m) = mask { m(x, y) } else { 1.0 }; if a <= 0.0 { continue; }
            if m_id >= 5 { w3c_px(&mut dst.d[i..i + 4], [srow[x * 4], srow[x * 4 + 1], srow[x * 4 + 2]], a, m_id); continue; }
            for j in 0..3 {
                let (b, s) = (dst.d[i + j], srow[x * 4 + j]);
                let r = match m_id { 1 => b + s, 2 => b * s, 3 => 1.0 - (1.0 - b) * (1.0 - s), 4 => if b < 0.5 { 2.0 * b * s } else { 1.0 - 2.0 * (1.0 - b) * (1.0 - s) }, _ => s };
                dst.d[i + j] = b * (1.0 - a) + r * a;
            }
            dst.d[i + 3] = dst.d[i + 3] + a * (1.0 - dst.d[i + 3]);
        }
    }
}

/// One W3C blend-mode pixel: `d` is the premultiplied canvas pixel, `cs` the straight source colour, `a` the layer's effective alpha.
fn w3c_px(d: &mut [f32], cs: [f32; 3], a: f32, m: usize) {
    let ab = d[3]; let cbp = [d[0], d[1], d[2]];
    let cb = if ab > 0.0 { cbp.map(|v| (v / ab).clamp(0.0, 1.0)) } else { [0.0; 3] };
    let bm = w3c_blend(m, cb, cs.map(|v| v.clamp(0.0, 1.0)));
    for j in 0..3 { d[j] = a * ((1.0 - ab) * cs[j] + ab * bm[j]) + (1.0 - a) * cbp[j]; }
    d[3] = ab + a * (1.0 - ab);
}

fn w3c_dodge(b: f32, s: f32) -> f32 { if b <= 0.0 { 0.0 } else if s >= 1.0 { 1.0 } else { (b / (1.0 - s)).min(1.0) } }
fn w3c_burn(b: f32, s: f32) -> f32 { if b >= 1.0 { 1.0 } else if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - b) / s).min(1.0) } }
fn w3c_lum(c: [f32; 3]) -> f32 { 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2] }
fn w3c_sat(c: [f32; 3]) -> f32 { c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]) }
fn w3c_clip(mut c: [f32; 3]) -> [f32; 3] {
    let l = w3c_lum(c); let n = c[0].min(c[1]).min(c[2]); let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 { c = c.map(|v| l + (v - l) * l / (l - n)); }
    if x > 1.0 { c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l)); }
    c
}
fn w3c_set_lum(c: [f32; 3], l: f32) -> [f32; 3] { let d = l - w3c_lum(c); w3c_clip(c.map(|v| v + d)) }
fn w3c_set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    // sort_by is stable: tied channels keep index order
    let mut k = [0usize, 1, 2]; k.sort_by(|&p, &q| c[p].total_cmp(&c[q]));
    let (lo, mid, hi) = (k[0], k[1], k[2]); let mut o = [0.0f32; 3];
    if c[hi] > c[lo] { o[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]); o[hi] = s; }
    o
}
/// W3C B(cb, cs) for the BLEND_MODES index `m` (5..=21); `b` and `s` are unpremultiplied and in 0..1.
fn w3c_blend(m: usize, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let sep = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
    match m {
        5 => sep(|b, s| b.min(s)),
        6 => sep(|b, s| b.max(s)),
        7 => sep(|b, s| (b - s).abs()),
        8 => sep(|b, s| b + s - 2.0 * b * s),
        9 => sep(|b, s| if s <= 0.5 { b - (1.0 - 2.0 * s) * b * (1.0 - b) } else { let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() }; b + (2.0 * s - 1.0) * (d - b) }),
        10 => sep(|b, s| if s <= 0.5 { b * 2.0 * s } else { let t = 2.0 * s - 1.0; b + t - b * t }),
        11 => sep(w3c_dodge),
        12 => sep(w3c_burn),
        13 => sep(|b, s| (b + s - 1.0).clamp(0.0, 1.0)),
        14 => sep(|b, s| (b + s).clamp(0.0, 1.0)),
        15 => sep(|b, s| if s <= 0.5 { w3c_burn(b, 2.0 * s) } else { w3c_dodge(b, 2.0 * (s - 0.5)) }),
        16 => sep(|b, s| (b + 2.0 * s - 1.0).clamp(0.0, 1.0)),
        17 => sep(|b, s| if s <= 0.5 { b.min(2.0 * s) } else { b.max(2.0 * s - 1.0) }),
        18 => w3c_set_lum(w3c_set_sat(s, w3c_sat(b)), w3c_lum(b)),
        19 => w3c_set_lum(w3c_set_sat(b, w3c_sat(s)), w3c_lum(b)),
        20 => w3c_set_lum(s, w3c_lum(b)),
        21 => w3c_set_lum(b, w3c_lum(s)),
        _ => s,
    }
}

pub fn render_frame(p: &Project, t: f64, cache: &Cache) -> Result<Frame, String> { render_frame_opts(p, t, cache, false) }

/// `transparent`: start from a clear canvas (used for precomps) instead of opaque black.
/// Live links (link_property) are resolved fresh here, against the project as given, before anything else reads a
/// clip property -- an edit to a link's driver after link_property ran is picked up on the very next render, with
/// no need to call link_property again. Zero cost when the project has no links (the overwhelmingly common case):
/// `resolve_links` returns the same reference, no clone.
pub fn render_frame_opts(p_in: &Project, t: f64, cache: &Cache, transparent: bool) -> Result<Frame, String> {
    let resolved_owner; let p: &Project = if p_in.links.is_empty() && p_in.expr_scripts.is_empty() { p_in } else { resolved_owner = crate::ext::expr_interp_v7::resolve_live(p_in); resolved_owner.as_ref() };
    if !t.is_finite(){return Err("render time must be finite".into())}
    let _memo = MemoScope::enter();
    p.color_pipeline.validate()?;p.grade_scopes.validate(p)?;
    for c in flat_clips(&p.tracks,0.) {if let Some(g)=&c.grade_graph{g.validate_project(p)?;}for e in c.effects {if let Some(w)=e.strings.get("bound_linear_gamut"){let g=w.parse::<usize>().map_err(|_|"bad bound linear gamut")?;if !p.color_pipeline.enabled||g!=p.color_pipeline.working_gamut||e.params.get("gamut").map(|v|(v.base,v.keys.is_empty(),v.expr.is_empty()))!=Some((g as f64,true,true)){return Err("managed linear effect no longer matches working gamut; re-add it".into())}}if let Some(white)=e.strings.get("wrapper_reference_white"){if white.parse::<f64>().ok()!=Some(p.color_pipeline.reference_white){return Err("effect wrapper reference white changed; reconfigure wrapper".into())}}if let Some(w)=e.strings.get("wrapper_working_gamut"){if !p.color_pipeline.enabled||w.parse::<usize>().ok()!=Some(p.color_pipeline.working_gamut){return Err("effect encoding wrapper no longer matches project working gamut; reconfigure or clear it".into())}}}}
    let (w, h) = (p.width as usize, p.height as usize);
    let mut out = Frame::new(w, h);
    if !transparent { for i in 0..w*h { out.d[i*4+3] = 1.0; } } // opaque black canvas
    let seed = (t * p.fps).round() as u64;
    let hidden: std::collections::HashSet<&str> = p.tracks.iter().flat_map(|tr| tr.clips.iter()).filter_map(|c| c.mask.as_ref().and_then(|m| m.matte.as_ref()).filter(|m| m.hide_source).map(|m| m.source.as_str()))
        .chain(p.tracks.iter().flat_map(|tr| tr.clips.iter()).filter_map(|c| match &c.source { Source::Particles3D(e) if e.sprite.kind == "layer" && e.sprite.hide_source && !e.sprite.clip_id.is_empty() => Some(e.sprite.clip_id.as_str()), _ => None })) // a Particles3D layer sprite's source is drawn only as the sprite
        // a keyer's clean-plate clip (strings.ref_clip) is hidden from the composite like a hidden track-matte source
        .chain(p.tracks.iter().flat_map(|tr| tr.clips.iter()).flat_map(|c| c.effects.iter()).filter(|e| e.kind == "keyer").filter_map(|e| e.strings.get("ref_clip").map(|s| s.as_str())))
        // a displacement map's / set matte's driving clip, unless it was asked to show (layerfx.rs)
        .chain(p.tracks.iter().flat_map(|tr| tr.clips.iter()).flat_map(|c| c.effects.iter()).filter(|e| crate::layerfx::KINDS.contains(&e.kind.as_str()) && e.strings.get("hide").map_or(true, |h| h == "1")).filter_map(|e| crate::layerfx::source_key(&e.kind).and_then(|k| e.strings.get(k)).map(|s| s.as_str())))
        // a gradient wipe's luma clip drives the reveal only (pushwipe.rs)
        .chain(p.tracks.iter().flat_map(|tr| tr.clips.iter()).filter_map(|c| c.transition_in.as_ref().and_then(|t| t.spec.as_ref()).and_then(|s| s.luma_clip.as_deref()))).collect();
    // 3D compositor (comp3d.rs, opt-in): consecutive 3D layers wait here and are depth-composited when a 2D draw or the end flushes them
    let c3 = crate::comp3d::active(p); let mut g3: Vec<crate::comp3d::Layer> = Vec::new();
    for tr in &p.tracks { for c in &tr.clips {
        if !crate::editorial::caption_visible(p, &c.id) { continue; }
        if t < c.start - 1e-9 || t >= c.start + c.duration - 1e-9 || (hidden.contains(c.id.as_str()) && MATTE_DEPTH.with(|d| d.get()) == 0) || tr.id == ALONE_SRC_TRACK { continue; }
        crate::guard::check();
        let lt0 = if c.posterize_fps > 0.0 { (((t - c.start) * c.posterize_fps + 1e-6).floor() / c.posterize_fps).max(0.0) } else { (t - c.start).max(0.0) };
        let (ne, esp) = match &c.echo { Some(e) if !matches!(c.source, Source::Adjustment) => (e.count.min(64) as usize, e.spacing), _ => (0, 0.0) };
        let avg = ne > 0 && matches!(c.echo.as_ref().map(|e| e.mode.as_str()), Some("average")); let mut eacc: Option<Frame> = if avg { Some(Frame::new(w, h)) } else { None };
        for ek in (0..=ne).rev() {
        let lt = lt0 - ek as f64 * esp; if lt < 0.0 || lt >= c.duration { continue; }
        if let Source::Adjustment = c.source {
            crate::comp3d::flush(&mut out, &mut g3, p, t); let tpa = std::time::Instant::now(); let mut adj = Frame { w, h, d: out.d.clone() };
            let fl = resolve_flares(p, &c.effects, t, lt, cache)?;
            for e in fl.as_deref().unwrap_or(&c.effects) { let tq = std::time::Instant::now(); effects::apply(&e.kind, &e.params, &e.strings, &mut adj, lt, seed)?; if std::env::var("KIMO_PROF").is_ok() { eprintln!("ADJ {} {:.3}", e.kind, tq.elapsed().as_secs_f64()); } }
            if let Some(g)=&c.grade_graph{g.apply(&mut adj,lt,seed,p.fps)?;}
            p.grade_scopes.apply_clip(p,&c.id,&mut adj,lt,seed)?;
            prof("adjust", tpa);
            let op = crate::keyframe::clamp_prop("opacity", xprop(p, c, "opacity", lt)) as f32; let mk: Option<Frame> = c.mask.as_ref().map(|m| { let mut mf = Frame::new(w, h); for i in 0..w*h { mf.d[i*4] = out.d[i*4]; mf.d[i*4+1] = out.d[i*4+1]; mf.d[i*4+2] = out.d[i*4+2]; mf.d[i*4+3] = 1.0; } apply_mask(&mut mf, m, lt); { let refs = |id: &str| render_clip_alone(p, id, t, cache); let ctx = crate::masks::Ctx { refs: &refs, fps: p.fps }; crate::masks::apply_stack(&mut mf, &m.layers, lt, &ctx); } mf });
            for i in 0..w*h { let k = op * mk.as_ref().map(|m| m.d[i*4+3]).unwrap_or(1.0); for j in 0..3 { out.d[i*4+j] = out.d[i*4+j] * (1.0 - k) + adj.d[i*4+j] * k; } }
            continue;
        }
        let mut src_hint: Option<(usize, usize, usize, usize)> = None;
        let mut f = if let Some(fr) = static_layer(p, c, lt, seed, t, cache)? { fr } else {
        let tp=std::time::Instant::now(); let (mut f, sh_) = source_frame_h(p, c, lt, cache); src_hint = sh_; prof("source", tp);
        // set only now: nested precomp children render inside source_frame and would otherwise clear it
        if c.effects.iter().any(|e| crate::effects2::needs_backdrop(&e.kind)) { crate::comp3d::flush(&mut out, &mut g3, p, t); BACKDROP.with(|b| *b.borrow_mut() = Some(std::rc::Rc::new(Frame { w, h, d: out.d.clone() }))); }
        if c.effects.iter().any(|e| e.kind == "relight" || e.kind == "fracture_glass") { crate::lighting::set_scene(Some(std::rc::Rc::new(crate::lighting::build_scene(&p.lights, &p.materials, t)))); }
        let fc; let ce: &Clip = match resolve_flares(p, &c.effects, t, lt, cache)? { Some(e) => { fc = Clip { effects: e, ..(**c).clone() }; &fc } None => &**c };
        let tp=std::time::Instant::now(); let r = apply_effects_masked(p, ce, &mut f, lt, seed, t, cache); crate::lighting::set_scene(None); BACKDROP.with(|b| *b.borrow_mut() = None); r?; prof("effects", tp); f };
        if let Some(m) = &c.mask { apply_mask(&mut f, m, lt); if !m.layers.is_empty() { let refs = |id: &str| render_clip_alone(p, id, t, cache); let ctx = crate::masks::Ctx { refs: &refs, fps: p.fps }; crate::masks::apply_stack(&mut f, &m.layers, lt, &ctx); } }
        let mut op = crate::keyframe::clamp_prop("opacity", xprop(p, c, "opacity", lt)) as f32; // the composed (parent x local) opacity is clamped at draw: an elastic/back overshoot never draws past 0..1, while a keep_world local opacity above 1 still composes
        let mut c_blend = c.blend.as_str();
        if let (Some(e), true) = (&c.echo, ek > 0) { match e.mode.as_str() { "average" => {} m => { op *= e.decay.clamp(0.0, 1.0).powi(ek as i32) as f32; if m == "add" { c_blend = "add"; } else if m == "screen" { c_blend = "screen"; } } } }
                let (mut dx, mut dy) = (xprop(p, c, "x", lt), xprop(p, c, "y", lt));
        let mut mask: Option<Box<dyn Fn(usize, usize) -> f32>> = None;
        let (mut sc, mut rot_extra) = (xprop(p, c, "scale", lt), 0.0);
        let mp_at = |l2: f64| -> (f64, f64, f64) { match &c.motion_path { Some(mp) => { let (x, y, a) = crate::motion::sample(mp, prop(c, "path", l2), w, h); (x, y, if mp.auto_orient { a + mp.orient_offset } else { 0.0 }) } None => (0.0, 0.0, 0.0) } };
        let (mp0x, mp0y, mp0a) = mp_at(lt); dx += mp0x; dy += mp0y; rot_extra += mp0a; let mut tplan: Option<crate::transitions::Plan> = None;
        if let Some(tr_in) = &c.transition_in { if lt < tr_in.duration {
            let mut pl = crate::transitions::plan(tr_in, lt, w, h); op *= pl.op; dx += pl.dx; dy += pl.dy; sc *= pl.scale; rot_extra += pl.rot;
            if let Some(m) = crate::pushwipe::luma_mask(p, tr_in, lt, t, w, cache) { pl.mask = Some(m); } // gradient_wipe driven by a clip's luma
            if let Some(u) = pl.under.as_ref() { crate::comp3d::flush(&mut out, &mut g3, p, t); let prev = crate::pushwipe::outgoing(p, c, t, cache); u(&mut out, prev.as_ref()); if !transparent { for i in 0..w * h { out.d[i*4+3] = 1.0; } } } // push / gradient_wipe: the outgoing picture
            if let Some((col, a)) = pl.dip { crate::comp3d::flush(&mut out, &mut g3, p, t); for i in 0..w * h { for j in 0..3 { out.d[i*4+j] = out.d[i*4+j] * (1.0 - a) + col[j] * a; } out.d[i*4+3] = out.d[i*4+3] + a * (1.0 - out.d[i*4+3]); } }
            mask = None; tplan = Some(pl);
        }}
        let mut f = f; if let Some(pl) = tplan.as_mut() { mask = pl.mask.take(); if let Some(po) = &pl.post { po(&mut f); } }
        let f = if let Some(mut pf) = crate::planar::pin_render(c, &f, lt, p.fps) { let adaptive = p.occluders.get(&c.id).map(|s| s.adaptive).unwrap_or(false); if adaptive { if let Some(sp) = p.occluders.get(&c.id) { if let Some(m) = crate::planar::adaptive_matte(c, sp, p.fps, t, w, h) { for i in 0..w * h { pf.d[i * 4 + 3] *= m[i]; } } } } if !adaptive && crate::planar::anchor_src(&c.id).is_none() { if let Some(sp) = p.occluders.get(&c.id) { crate::planar::rebuild_anchor(&c.id, sp, w, h); } } if let Some((sid, sstart)) = crate::planar::anchor_src(&c.id).filter(|_| !adaptive) { if let (Some(fr), Some(q)) = (cache.frames_of(&sid), crate::planar::pin_quad(c, lt)) { if let Some(cur) = fr.get(((t - sstart).max(0.0) * p.fps) as usize) { let g = prop(c, "pin_gain", lt) as f32; if let Some(m) = crate::planar::occluder_matte(&c.id, cur, &q, if g > 0.0 { g } else { 1.0 }) { for i in 0..w * h { pf.d[i * 4 + 3] *= m[i]; } } } } } pf } else if let Some(cs) = &c.corners { corner_pin(&f, cs) } else { f };
        let (rxv, ryv, pzv) = (prop(c, "rot_x", lt), prop(c, "rot_y", lt), prop(c, "pos_z", lt));
        if rxv != 0.0 || ryv != 0.0 || pzv != 0.0 {
            let cam = crate::three_d::Camera::from_props(&p.camera, t); let pose = crate::three_d::Pose { pos: [dx, dy, pzv], rot: [rxv, ryv, xprop(p, c, "rotation", lt) + rot_extra], scale: sc };
            if c3 && !c.effects.iter().any(|e| crate::effects2::is_post(&e.kind)) && tplan.as_ref().map_or(true, |pl| pl.overlay.is_none()) {
                let img = crate::three_d::render_layer(&f, &cam, &pose, prop(c, "backface", lt) != 0.0); let pj = crate::three_d::Projector::new(w, h, &cam, &pose);
                g3.push(crate::comp3d::Layer::new(p, img, f, pj, op, c_blend, mask.as_deref(), prop(c, "casts_shadows", lt) != 0.0, prop_or(c, "accepts_shadows", lt, 1.0) != 0.0)); continue;
            }
            crate::comp3d::flush(&mut out, &mut g3, p, t);
            let mut f3 = crate::three_d::render_layer(&f, &cam, &pose, prop(c, "backface", lt) != 0.0);
            // frosted_glass on a tilted layer: the pane is the projected silhouette, and it frosts what is really under it
            if c.effects.iter().any(|e| crate::effects2::is_post(&e.kind)) { BACKDROP.with(|b| *b.borrow_mut() = Some(std::rc::Rc::new(Frame { w, h, d: out.d.clone() }))); crate::frosted::set_post(true); crate::frosted::set_tilt((ryv.to_radians().sin()) as f32, (-rxv.to_radians().sin()) as f32); crate::frosted::set_slab(Some(crate::three_d::Projector::new(w, h, &cam, &pose)));
                let r: Result<(), String> = (|| { for e in c.effects.iter().filter(|e| crate::effects2::is_post(&e.kind)) { effects::apply(&e.kind, &e.params, &e.strings, &mut f3, lt, seed)?; } Ok(()) })();
                crate::frosted::set_post(false); crate::frosted::set_tilt(0.0, 0.0); crate::frosted::set_slab(None); BACKDROP.with(|b| *b.borrow_mut() = None); r?; }
            blend(&mut out, &f3, op, c_blend, mask.as_deref()); if let Some(pl) = &tplan { if let Some(ov) = &pl.overlay { ov(&mut out); } } continue;
        }
        crate::comp3d::flush(&mut out, &mut g3, p, t);
        // opt-in exact shutter (set_motion_blur samples / shutter_phase / shutter_curve); None keeps the legacy centred fixed-count sampler unchanged
        let ls = crate::shutter::layer_shutter(c, p.fps, lt); let lsw = ls.as_ref().map(|s| [lt + s.open, lt + s.open + s.len * 0.5, lt + s.open + s.len]);
        let moving = if let Some(ws) = lsw { ek == 0 && ws.iter().any(|&t2| ["x", "y", "scale", "rotation"].iter().any(|k| (xprop(p, c, k, t2) - xprop(p, c, k, lt)).abs() > 1e-9) || { let (a, b, r) = mp_at(t2); (a - mp0x).abs() > 1e-9 || (b - mp0y).abs() > 1e-9 || (r - mp0a).abs() > 1e-9 }) } else { c.motion_blur > 0.0 && ek == 0 && { let sh0 = c.motion_blur / 360.0 / p.fps; ["x", "y", "scale", "rotation"].iter().any(|k| (xprop(p, c, k, lt - sh0 * 0.5) - xprop(p, c, k, lt)).abs() > 1e-9 || (xprop(p, c, k, lt + sh0 * 0.5) - xprop(p, c, k, lt)).abs() > 1e-9) } };
        let mut from_tb = false; let tp=std::time::Instant::now(); let hint_ok = SHAPE_HINT.load(std::sync::atomic::Ordering::Relaxed) && src_hint.is_some() && c.effects.is_empty() && c.mask.is_none() && tplan.is_none() && !p.color_pipeline.enabled && !c.props.keys().any(|k| k.starts_with("pin_") || k.starts_with("mesh_")); let bbf = if hint_ok { alpha_bbox_in(&f, src_hint.unwrap()) } else { alpha_bbox(&f) }; prof("bbox", tp); let tp=std::time::Instant::now(); let f = if moving { let lts: Vec<f64> = match (&ls, lsw) { (Some(s), Some(ws)) => { let n = if s.samples > 0 { s.samples.clamp(2, 256) } else {
                // travel (px) of the fastest point per half-shutter: x/y + motion path, scale and rotation swing the bbox half-diagonal; x2 like the mask auto
                let hd = match bbf { Some((x0, y0, x1, y1)) => 0.5 * ((x1 - x0 + 1) as f64).hypot((y1 - y0 + 1) as f64), None => 0.5 * (w as f64).hypot(h as f64) };
                let st = |t2: f64| { let m = mp_at(t2); (xprop(p, c, "x", t2) + m.0, xprop(p, c, "y", t2) + m.1, xprop(p, c, "scale", t2), (xprop(p, c, "rotation", t2) + m.2).to_radians()) };
                let s3 = ws.map(st); let seg = |a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)| (b.0 - a.0).hypot(b.1 - a.1) + (b.2 - a.2).abs() * hd + (b.3 - a.3).abs() * hd;
                crate::shutter::auto_samples(2.0 * seg(s3[0], s3[1]).max(seg(s3[1], s3[2]))) };
                crate::shutter::midpoints(s.open, s.len, n).into_iter().map(|o| lt + o).collect() }
            _ => { let n = ((c.motion_blur / 22.5).ceil() as usize).clamp(4, 16); let sh = c.motion_blur / 360.0 / p.fps; (0..n).map(|k| { let o = (k as f64 + 0.5) / n as f64 - 0.5; lt + o * sh }).collect() } }; let n = lts.len();
            // shutter_curve weights (exact sampler, non-box only): rgb = sum(w*rgb*a)/sum(w*a), alpha = sum(w*a); box (None) keeps the plain sums and alpha / n
            let cw: Option<Vec<f32>> = ls.as_ref().filter(|s| s.curve != crate::shutter::Curve::Box).map(|s| crate::shutter::weighted_midpoints(s.open, s.len, n, &s.curve).into_iter().map(|x| x.1 as f32).collect());
            let mut acc = Frame::new(w, h); let mut wsum = vec![0f32; w * h]; let mut u: Option<(usize, usize, usize, usize)> = None;
            for (ki, &lt2) in lts.iter().enumerate() { let cwk = cw.as_ref().map(|v| v[ki]); let (mut dx2, mut dy2) = (xprop(p, c, "x", lt2), xprop(p, c, "y", lt2)); dx2 += dx - xprop(p, c, "x", lt) - mp0x; dy2 += dy - xprop(p, c, "y", lt) - mp0y; let (mp2x, mp2y, mp2a) = mp_at(lt2); dx2 += mp2x; dy2 += mp2y;
                let mut sc2 = xprop(p, c, "scale", lt2); if sc != xprop(p, c, "scale", lt) { sc2 *= sc / xprop(p, c, "scale", lt).max(1e-9); } LAST_RECT.with(|l| l.set(None)); FAST_NS.with(|l| l.set(true)); let tf = transform_bb(&f, bbf, sc2, xprop(p, c, "rotation", lt2) + rot_extra - mp0a + mp2a, dx2, dy2); FAST_NS.with(|l| l.set(false));
                if let Some((rx0, ry0, rx1, ry1)) = LAST_RECT.with(|l| l.get()) { u = Some(match u { Some((a, b, c2, d)) => (a.min(rx0), b.min(ry0), c2.max(rx1), d.max(ry1)), None => (rx0, ry0, rx1, ry1) });
                for y in ry0..=ry1 { for x in rx0..=rx1 { let i = y * w + x; let a = tf.d[i * 4 + 3]; if a > 0.0 { if let Some(wk) = cwk { let wa = wk * a; for j in 0..3 { acc.d[i * 4 + j] += tf.d[i * 4 + j] * wa; } acc.d[i * 4 + 3] += wa; wsum[i] += wa; } else { for j in 0..3 { acc.d[i * 4 + j] += tf.d[i * 4 + j] * a; } acc.d[i * 4 + 3] += a; wsum[i] += a; } } } } } }
            if let Some((rx0, ry0, rx1, ry1)) = u { for y in ry0..=ry1 { for x in rx0..=rx1 { let i = y * w + x; if wsum[i] > 0.0 { for j in 0..3 { acc.d[i * 4 + j] /= wsum[i]; } } if cw.is_none() { acc.d[i * 4 + 3] /= n as f32; } } } } acc }
            else if (sc - 1.0).abs() < 1e-9 && xprop(p, c, "rotation", lt) + rot_extra == 0.0 && dx == 0.0 && dy == 0.0 { f } else { from_tb = true; transform_bb(&f, bbf, sc, xprop(p, c, "rotation", lt) + rot_extra, dx, dy) };
        prof("transform", tp);
        // Stabilize, Synthesize Edges: opt-in, only media clips the stabilizer gave stab_fill and the stab_s* path; the fill can write outside
        // transform_bb's rect, so the blend scans the whole frame and the buffer is not handed back to the pool
        let mut f = f; if !moving && matches!(c.source, Source::Media { .. }) && prop(c, "stab_fill", lt) >= 1.0 && ["stab_ss", "stab_sr", "stab_sx", "stab_sy"].iter().all(|k| c.props.contains_key(*k)) {
            synth_edges(p, c, &mut f, lt, (sc, xprop(p, c, "rotation", lt) + rot_extra, dx, dy), cache); from_tb = false; }
        let mut f = f; if let Some(mt) = c.mask.as_ref().and_then(|m| m.matte.as_ref()) { if let Some(src) = render_clip_alone(p, &mt.source, t, cache) { let cov = crate::masks::matte_coverage(mt, &src, &p.color_pipeline); for i in 0..w * h { f.d[i * 4 + 3] *= cov[i]; } } }
        // frosted_glass runs here, after the layer transform, so it frosts what is really under the pane
        if c.effects.iter().any(|e| crate::effects2::is_post(&e.kind)) { BACKDROP.with(|b| *b.borrow_mut() = Some(std::rc::Rc::new(Frame { w, h, d: out.d.clone() }))); crate::frosted::set_post(true);
            let r: Result<(), String> = (|| { for e in c.effects.iter().filter(|e| crate::effects2::is_post(&e.kind)) { effects::apply(&e.kind, &e.params, &e.strings, &mut f, lt, seed)?; } Ok(()) })();
            crate::frosted::set_post(false); BACKDROP.with(|b| *b.borrow_mut() = None); r?; }
        // morph_cut: dense optical flow from the outgoing clip's last frame into this frame (both directions, linear-light mix)
        let mut f = f; if let Some(k) = tplan.as_ref().and_then(|pl| pl.morph) { if let Some(a) = crate::transitions::morph_prev(p, c) { if let Some(af) = render_clip_alone(p, &a.id, a.start + a.duration - 0.5 / p.fps, cache) { f = crate::flow::flow_interpolate(&af, &f, k, !p.color_pipeline.enabled, 2); from_tb = false; } } }
        if let Some(cf) = tplan.as_ref().and_then(|pl| pl.compose.as_ref()) { out = cf(&out, &f); continue; }
        if let Source::ShardBurst(sb) = &c.source { if let Some(d) = crate::shards::take_disp() { crate::shards::refract_canvas(&mut out, &d, &f, sb.refract_dispersion.clamp(0.0, 1.0)); } }
        if let Some(acc) = eacc.as_mut() { let wgt = op / (ne + 1) as f32; for i in 0..w * h { let a = f.d[i * 4 + 3] * wgt; if a > 0.0 { for j in 0..3 { acc.d[i * 4 + j] += f.d[i * 4 + j] * a; } acc.d[i * 4 + 3] += a; } } continue; }
        let tp=std::time::Instant::now(); let rect = if from_tb { LAST_RECT.with(|l| l.get()) } else { None }; blend_region(&mut out, &f, op, c_blend, mask.as_deref(), rect); prof("blend", tp);
        if let Some(r) = rect { pool_give(f, r); }
        if let Some(pl) = &tplan { if let Some(ov) = &pl.overlay { ov(&mut out); } }
        }
        if let Some(mut acc) = eacc { crate::comp3d::flush(&mut out, &mut g3, p, t); for i in 0..w * h { let a = acc.d[i * 4 + 3]; if a > 1e-6 { for j in 0..3 { acc.d[i * 4 + j] /= a; } } } blend(&mut out, &acc, 1.0, &c.blend, None); }
    }}
    crate::comp3d::flush(&mut out, &mut g3, p, t);
    prof_dump();
    p.grade_scopes.apply_list(p,&p.grade_scopes.timeline,&mut out,t,seed)?;
    if out.d.iter().any(|v|!v.is_finite()){return Err("render produced nonfinite pixels or invalid input color settings".into())}
    Ok(out)
}

/// The compositor keeps premultiplied color on a transparent canvas (rgb = sum of src*a over below); alpha deliveries need straight alpha.
pub(crate) fn unpremultiply(f: &mut Frame) { for px in f.d.chunks_exact_mut(4) { let a = px[3]; if a > 0.0 && a < 1.0 { for j in 0..3 { px[j] = (px[j] / a).min(1.0); } } } }
/// One export-equivalent RGBA8 frame (opaque canvas): render_frame_opts, the colour pipeline output stage, then to_rgba8, exactly the steps of the export loop.
pub fn export_frame_rgba8(p: &Project, t: f64, cache: &Cache) -> Result<Vec<u8>, String> { render_frame_opts(p, t, cache, false).and_then(|mut f| { crate::color::pipeline::output(p, &mut f)?; Ok(to_rgba8(&f)) }) }
pub fn to_rgba8(f: &Frame) -> Vec<u8> { f.d.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect() }

/// Deterministic TPDF dither (sum of two uniforms, +/-1 LSB triangular PDF) applied in
/// display-referred [0,1] before 8-bit rounding. Fixed splitmix64 seed keyed on channel
/// index: same frame in, same bytes out. Kills mid-gradient quantization terraces.
fn tpdf_lsb(i: usize) -> f64 { fn sm(mut x: u64) -> u64 { x = x.wrapping_add(0x9E3779B97F4A7C15); x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9); x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB); x ^ (x >> 31) } let u1 = (sm(0xD1B54A32D192ED03 ^ (i as u64).wrapping_mul(2)) >> 11) as f64 * (1.0 / 9007199254740992.0); let u2 = (sm(0xD1B54A32D192ED03 ^ (i as u64).wrapping_mul(2).wrapping_add(1)) >> 11) as f64 * (1.0 / 9007199254740992.0); u1 + u2 - 1.0 }
pub fn to_rgba8_tpdf(f: &Frame) -> Vec<u8> { f.d.iter().enumerate().map(|(i, v)| (((v.clamp(0.0, 1.0) + (tpdf_lsb(i) * (1.0 / 255.0)) as f32) * 255.0 + 0.5).clamp(0.0, 255.0)) as u8).collect() }

fn image_encoder_command(w:usize,h:usize,path:&str)->Command{let mut cmd=Command::new("ffmpeg");cmd.args(["-v","error","-y","-f","rawvideo","-pix_fmt","rgba","-s",&format!("{w}x{h}"),"-i","-","-frames:v","1",path]);cmd}
fn write_image_command(f:&Frame,cmd:&mut Command,deadline:std::time::Duration)->Result<(),String>{let size=f.w.checked_mul(f.h).and_then(|n|n.checked_mul(4)).filter(|n|*n>0&&*n==f.d.len()).ok_or("invalid image frame shape")?;crate::guard::reserve(size);let mut encoder=crate::color::process::EncodeStream::new(cmd,size,deadline)?;encoder.write(to_rgba8(f))?;encoder.finish()}
pub(crate) fn write_image(f:&Frame,path:&str)->Result<(),String>{check_still_path(path,false)?;write_image_command(f,&mut image_encoder_command(f.w,f.h,path),crate::color::process::encode_deadline(1))}
/// A still-image writer must never replace a video: two models in the director eval called contact_sheet with path = their
/// finished export, and the mp4 became a 1-frame video. `image_only` also refuses any extension that is not an image.
pub(crate) fn check_still_path(path:&str,image_only:bool)->Result<(),String>{
 let ext=std::path::Path::new(path).extension().and_then(|e|e.to_str()).map(|e|e.to_ascii_lowercase()).unwrap_or_default();
 let image=matches!(ext.as_str(),"png"|"jpg"|"jpeg"|"webp"|"bmp"|"tif"|"tiff"|"exr"|"ppm"|"tga");
 let media=matches!(ext.as_str(),"mp4"|"mov"|"m4v"|"mkv"|"webm"|"avi"|"gif"|"wav"|"mp3"|"m4a"|"aac"|"flac"|"ogg"|"opus");
 let mut head=[0u8;12];let existing_media=std::fs::File::open(path).and_then(|mut f|{use std::io::Read;f.read_exact(&mut head)}).is_ok()&&(&head[4..8]==b"ftyp"||head[..4]==[0x1A,0x45,0xDF,0xA3]||(&head[..4]==b"RIFF"&&(&head[8..12]==b"AVI "||&head[8..12]==b"WAVE"))||&head[..3]==b"ID3"||&head[..4]==b"OggS");
 if existing_media{return Err(format!("{path} is an existing video or audio file; a still image will not overwrite it. Nothing was written. Use a .png path; to look at a video file (an export or a source clip) use review_export {{path}}"))}
 if media||(image_only&&!image){return Err(format!("{path}: this writes a still image, so the path must end in .png (or .jpg). Nothing was written. To look at a video file (an export or a source clip) use review_export {{path}}"))}
 Ok(())
}
fn write_image_tpdf_command(f:&Frame,cmd:&mut Command,deadline:std::time::Duration)->Result<(),String>{let size=f.w.checked_mul(f.h).and_then(|n|n.checked_mul(4)).filter(|n|*n>0&&*n==f.d.len()).ok_or("invalid image frame shape")?;crate::guard::reserve(size);let mut encoder=crate::color::process::EncodeStream::new(cmd,size,deadline)?;encoder.write(to_rgba8_tpdf(f))?;encoder.finish()}
pub(crate) fn write_image_tpdf(f:&Frame,path:&str)->Result<(),String>{write_image_tpdf_command(f,&mut image_encoder_command(f.w,f.h,path),crate::color::process::encode_deadline(1))}

pub fn render_png(p: &Project, t: f64, path: &str, cache: &mut Cache) -> Result<(), String> { render_png_with(p, t, path, cache, None) }
pub fn render_png_with(p: &Project, t: f64, path: &str, cache: &mut Cache, fc: Option<&mut crate::framecache::FrameCache>) -> Result<(), String> {
    if p.color_pipeline.enabled && [6,7].contains(&p.color_pipeline.output_transfer) {return Err("HDR output cannot be shown as an SDR PNG; select an SDR preview output or render_high_bit_still".into())}
    prepare_preview_range(p, cache, t, t)?;
    let mut f = match fc { Some(fc) => crate::framecache::render_cached(fc, p, t, cache)?, None => render_frame(p, t, cache)? };
    crate::color::pipeline::output(p,&mut f)?;
    write_image(&f,path)
}

/// Ordinary video delivery is explicit Rec709 RGB -> limited BT709 YUV.
/// Disabled projects keep their display-sRGB render samples; only delivery changes.
pub(crate) fn legacy_encoder_command(p:&Project,path:&str,crf:u32)->Result<Command,String>{legacy_encoder_command_opts(p,path,crf,false)}
/// `low_memory` adds x264 settings that cut the encoder's RSS (shorter lookahead, fewer refs and b-frames) at a small size/quality cost. Default output bytes are unchanged.
pub(crate) fn legacy_encoder_command_opts(p:&Project,path:&str,crf:u32,low_memory:bool)->Result<Command,String>{
    let q=&p.color_pipeline;
    let transfer=if q.enabled{q.validate()?;if q.output_gamut!=0{return Err("ordinary video export requires Rec709 output gamut=0; select a supported delivery output or export_high_bit".into())}match q.output_transfer{0=>"linear",1=>"iec61966-2-1",8=>"bt709",_=>return Err("ordinary video export requires exactly taggable linear/sRGB/BT709 output transfer; select a supported delivery output or export_high_bit".into())}}else{"iec61966-2-1"};
    let mut cmd=Command::new("ffmpeg");cmd.args(["-v","error","-y","-f","rawvideo","-pix_fmt","rgba","-s",&format!("{}x{}",p.width,p.height),"-r",&p.fps.to_string(),"-i","-","-c:v","libx264","-vf","scale=out_color_matrix=bt709:out_range=tv","-pix_fmt","yuv420p","-color_range","tv","-color_primaries","bt709","-color_trc",transfer,"-colorspace","bt709","-crf",&crf.to_string()]);if low_memory{cmd.args(["-x264-params","rc-lookahead=10:ref=2:bframes=2:sync-lookahead=0"]);}cmd.arg(path);Ok(cmd)
}


/// Export with alpha: `format` is "prores4444" (.mov), "webm" (VP9 + alpha) or "png" (RGBA PNG sequence; `path` must contain %05d or %d).
/// Frames render with a transparent canvas (straight alpha). Audio is not muxed. Requires the managed color pipeline to be off.
pub fn export_alpha(p: &Project, path: &str, format: &str, t_in: f64, t_out: f64) -> Result<f64, String> {
    if !t_in.is_finite() || !t_out.is_finite() || t_in < 0.0 || t_out <= t_in { return Err("range must satisfy 0 <= start < end".into()); }
    if p.color_pipeline.enabled { return Err("alpha export does not support the managed color pipeline yet; disable it (display-sRGB project) or export RGB".into()); }
    let t_out = t_out.min(p.duration()); if t_in >= t_out { return Err("range starts after the end of the timeline".into()); }
    let f0 = (t_in * p.fps).round() as usize; let f1 = ((t_out * p.fps).round() as usize).max(f0 + 1); let n = f1 - f0;
    let mut cmd = Command::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-"]);
    match format {
        "prores4444" => { if !path.ends_with(".mov") { return Err("prores4444 path must end in .mov".into()); } cmd.args(["-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuva444p10le", "-c:v", "prores_ks", "-profile:v", "4444", "-pix_fmt", "yuva444p10le", "-alpha_bits", "16", "-vendor", "apl0", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-bsf:v", "prores_metadata=color_primaries=bt709:color_trc=13:colorspace=bt709", "-movflags", "+write_colr", path]); }
        "webm" => { if !path.ends_with(".webm") { return Err("webm path must end in .webm".into()); } cmd.args(["-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p", "-auto-alt-ref", "0", "-crf", "18", "-b:v", "0", path]); }
        "png" => { if !path.contains('%') { return Err("png path must be a pattern such as /dir/frame_%05d.png".into()); } cmd.args(["-c:v", "png", "-pix_fmt", "rgba", path]); }
        _ => return Err("format must be prores4444, webm or png".into()),
    }
    let frame_bytes = (p.width as usize).checked_mul(p.height as usize).and_then(|v| v.checked_mul(4)).ok_or("frame overflow")?;
    let mut encoder = crate::color::process::EncodeStream::new(&mut cmd, frame_bytes, crate::color::process::encode_deadline(n))?;
    let mut cache = Cache::new_export(); let nt = crate::guard::export_threads_n(p.width, p.height, std::thread::available_parallelism().map(|x| x.get()).unwrap_or(2).max(1), frame_buffers(p)); let mut i = 0;
    while i < n {
        crate::guard::check(); let mut end = (i + nt * 4).min(n);
        loop { let last = (f0 + end - 1) as f64 / p.fps; match prepare_export_range(p, &mut cache, (f0 + i) as f64 / p.fps, last) { Ok(()) => break, Err(e) if end - i > 1 && (e.contains("cache budget") || e.contains("memory cap") || e.contains("bounded cache")) => { end = i + (end - i).div_ceil(2); cache = Cache::new_export(); }, Err(e) => return Err(e) } }
        let next = std::sync::atomic::AtomicUsize::new(i); let slots: Vec<std::sync::Mutex<Option<Result<Vec<u8>, String>>>> = (i..end).map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|sc| { for _ in 0..nt.min(end - i) { let (cache, next, slots) = (&cache, &next, &slots); crate::guard::scope_spawn(sc, move || loop { let k = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst); if k >= end { break; }
            let res = render_frame_opts(p, (f0 + k) as f64 / p.fps, cache, true).map(|mut f| { unpremultiply(&mut f); to_rgba8(&f) }); *slots[k - i].lock().unwrap() = Some(res); }); } });
        for m in slots { encoder.write(m.into_inner().unwrap().unwrap()?)?; }
        i = end;
    }
    encoder.finish()?; Ok(n as f64 / p.fps)
}

/// Join picture-only range exports (`export` with `audio:false`) in order with a stream copy, then mux ONE continuous audio stream for the same
/// timeline span, so no AAC priming gap appears at the seams. `start` is the timeline time of the first part's first frame.
pub fn join_export(p: &Project, parts: &[String], path: &str, start: f64) -> Result<f64, String> {
    if parts.is_empty() { return Err("parts must list at least one picture-only export".into()); }
    if !start.is_finite() || start < 0.0 { return Err("start must be >= 0".into()); }
    let mut total = 0.0; let mut fps0 = 0.0;
    for (i, f) in parts.iter().enumerate() { let (_, _, dur, fps) = probe(f)?; if i == 0 { fps0 = fps; } else if (fps - fps0).abs() > 1e-6 { return Err(format!("part {i} runs at {fps} fps but part 0 runs at {fps0}")); } total += dur; }
    let frames = (total * fps0).round() as usize; if frames == 0 { return Err("parts contain no frames".into()); }
    let list = std::env::temp_dir().join({ static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0); format!("kimo-join-{}-{}-{}.txt", std::process::id(), frames, N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) });
    std::fs::write(&list, parts.iter().map(|f| format!("file '{}'\n", f.replace('\'', "'\\''"))).collect::<String>()).map_err(|e| e.to_string())?;
    let out = Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "concat", "-safe", "0", "-i"]).arg(&list).args(["-c", "copy", "-an", path]).output().map_err(|e| format!("ffmpeg: {e}"))?;
    let _ = std::fs::remove_file(&list);
    if !out.status.success() { return Err(format!("join failed: {}", String::from_utf8_lossy(&out.stderr).trim())); }
    let sr = crate::audio::SR as f64; let (s0, s1) = ((start * sr) as usize * 2, ((start + frames as f64 / fps0) * sr) as usize * 2);
    crate::guard::reserve(s1.saturating_sub(s0) * 4);
    let pcm_full = crate::audio::mix(p); let a = s0.min(pcm_full.len()); let b = s1.min(pcm_full.len()); let mut pcm = pcm_full[a..b].to_vec(); pcm.resize(s1 - s0, 0.0);
    if pcm.iter().any(|v| v.abs() > 1e-6) { crate::color::mux::legacy_audio(path, &pcm, frames)?; }
    Ok(frames as f64 / fps0)
}

/// Full-frame f32 buffers one in-flight frame needs, for sizing export workers. Measured on 1080p (tests/frame_live_peak.rs): plain generators 2.0,
/// glass shards + glow/blur/bloom adjustment stacks up to 5.5, precomp depth 3 = 8, motion blur 4.25, 3D 3.0. Only scenes fully inside the measured
/// set get a lower figure (ceil(1.5 x measured), floor 4): 4 for bare generators, 9 for the measured effect/mask/shard/adjustment set.
/// Anything else (media, precomps, motion blur, 3D, other effects, grade graphs, echo, transitions) keeps the safe 12.
pub fn frame_buffers(p: &Project) -> usize {
    use crate::model::Source;
    const SAFE: usize = crate::guard::FRAME_BUFFERS_SAFE;
    if !p.camera.is_empty() { return SAFE; }
    let mut rich = false;
    for t in &p.tracks { for c in &t.clips {
        match &c.source { Source::Solid { .. } | Source::Gradient { .. } | Source::Shape { .. } => {}, Source::Adjustment | Source::ShardBurst(_) => rich = true, _ => return SAFE }
        if c.motion_blur != 0.0 || c.corners.is_some() || c.grade_graph.is_some() || c.echo.is_some() || c.motion_path.is_some() || c.transition_in.is_some() || c.text_fx.is_some() || c.time_remap.is_some() { return SAFE; }
        if c.props.keys().any(|k| ["rot_x", "rot_y", "pos_z", "flow_blur"].contains(&k.as_str())) { return SAFE; }
        if c.mask.is_some() { rich = true; }
        for e in &c.effects { if !["glow", "blur", "bloom", "exposure"].contains(&e.kind.as_str()) { return SAFE; } rich = true; }
    } }
    if rich { 9 } else { 4 }
}

pub fn export(p: &Project, path: &str, crf: u32) -> Result<f64, String> { export_range(p, path, crf, 0.0, p.duration()) }
pub fn export_opts(p: &Project, path: &str, crf: u32, low_memory: bool) -> Result<f64, String> { export_range_opts(p, path, crf, 0.0, p.duration(), low_memory, true) }
/// Export [t_in, t_out) seconds. Frame k renders at t_in + k/fps (absolute time, so frames equal the same frames of a full export).
pub fn export_range(p: &Project, path: &str, crf: u32, t_in: f64, t_out: f64) -> Result<f64, String> { export_range_opts(p, path, crf, t_in, t_out, false, true) }
pub fn export_range_opts(p: &Project, path: &str, crf: u32, t_in: f64, t_out: f64, low_memory: bool, with_audio: bool) -> Result<f64, String> {
    if !t_in.is_finite() || !t_out.is_finite() || t_in < 0.0 || t_out <= t_in { return Err("range must satisfy 0 <= start < end".into()); }
    let t_out = t_out.min(p.duration()); if t_in >= t_out { return Err("range starts after the end of the timeline".into()); }
    let f0 = (t_in * p.fps).round() as usize; let f1 = ((t_out * p.fps).round() as usize).max(f0 + 1);
    if p.color_pipeline.enabled && [6,7].contains(&p.color_pipeline.output_transfer) { return Err("HDR delivery requires export_high_bit; legacy export is 8-bit SDR".into()); }
    let (_workers, low_memory) = crate::guard::export_plan_n(p.width, p.height, std::thread::available_parallelism().map(|x| x.get()).unwrap_or(2), low_memory, frame_buffers(p))?;
    let command=legacy_encoder_command_opts(p,path,crf,low_memory)?;
    export_range_cmd(p, path, f0, f1, command, false, with_audio)
}

/// Decode the batch starting at local index `i` into `cache`, exactly as the old single-threaded export
/// loop did: start at a batch of `batch` frames and halve it on a cache-budget error (resetting `cache`,
/// which only ever happens on that rare fallback path). Returns the batch's actual end (exclusive).
fn decode_batch(p: &Project, cache: &mut Cache, f0: usize, i: usize, n: usize, batch: usize) -> Result<usize, String> {
    let mut end = (i + batch).min(n);
    loop {
        let last = (f0 + end - 1) as f64 / p.fps;
        match prepare_export_range(p, cache, (f0 + i) as f64 / p.fps, last) {
            Ok(()) => return Ok(end),
            Err(e) if end - i > 1 && (e.contains("cache budget") || e.contains("memory cap") || e.contains("bounded cache")) => { end = i + (end - i).div_ceil(2); *cache = Cache::new_export(); }
            Err(e) => return Err(e),
        }
    }
}

/// Shared export loop: render frames f0..f1 into `command`'s stdin (RGBA8), then mux audio when `audio`.
/// `transparent` renders on a transparent canvas (alpha delivery); the ordinary export passes false.
///
/// V8.6.0 overlap: three stages run concurrently instead of the old decode-then-compose-then-write-and-block
/// sequence. While batch i composes (the existing nt-worker queue below, unchanged), a prefetch thread
/// decodes batch i+1 into a cheap Arc-sharing `snapshot()` of the cache (so it can't alias the frames compose
/// is reading), and the encoder runs on its own thread fed by a bounded channel, so the main loop never
/// blocks on the encoder's pipe — it hands off a composed batch and moves straight to the next one. Output
/// bytes are unchanged: same frames, same order, same encoder command; see tests/overlap_pipeline.rs
/// (framemd5-identical to the old sequential path) for the proof.
pub(crate) fn export_range_cmd(p: &Project, path: &str, f0: usize, f1: usize, mut command: Command, transparent: bool, audio: bool) -> Result<f64, String> {
    let mut cache = Cache::new_export();
    let n = f1 - f0;
    let frame_bytes=(p.width as usize).checked_mul(p.height as usize).and_then(|v|v.checked_mul(4)).ok_or("legacy encoder frame overflow")?;
    let encoder=crate::color::process::EncodeStream::new(&mut command,frame_bytes,crate::color::process::encode_deadline(n))?;
    let nt = crate::guard::export_threads_n(p.width, p.height, std::thread::available_parallelism().map(|x| x.get()).unwrap_or(2).max(1), frame_buffers(p));
    let batch = nt * 4;

    // Encoder-writer thread: owns `encoder`, drains a bounded channel. EncodeStream::write() already
    // waits on its own internal writer's ack per call (unchanged, see color/process.rs), but that wait now
    // happens off this function's critical path instead of stalling the compose/decode of the next batch.
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(batch.max(4) * 2);
    let writer = crate::guard::spawn_with_token(move || -> Result<(), String> {
        let mut encoder = encoder;
        for frame in frame_rx { encoder.write(frame)?; }
        encoder.finish()
    });

    crate::telemetry::progress_frames(0, n, "frames");
    let loop_result: Result<(), String> = (|| {
        let mut i = 0usize;
        let mut end = decode_batch(p, &mut cache, f0, i, n, batch)?;
        while i < n {
            crate::guard::check();
            let mut carried: Option<(Cache, usize)> = None;
            std::thread::scope(|osc| -> Result<(), String> {
                // Kick off the NEXT batch's decode now, on a cache snapshot, before composing this one.
                let prefetch = (end < n).then(|| {
                    let mut snap = cache.snapshot();
                    let next_i = end;
                    crate::guard::scope_spawn(osc, move || decode_batch(p, &mut snap, f0, next_i, n, batch).map(|e| (snap, e)))
                });
                // work queue: nt workers pull frame indices, so one slow frame (shards) no longer idles the other cores
                let next = std::sync::atomic::AtomicUsize::new(i); let slots: Vec<std::sync::Mutex<Option<Result<Vec<u8>, String>>>> = (i..end).map(|_| std::sync::Mutex::new(None)).collect();
                std::thread::scope(|sc| { for _ in 0..nt.min(end - i) { let (cache, next, slots) = (&cache, &next, &slots); crate::guard::scope_spawn(sc, move || loop { let k = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst); if k >= end { break; }
                    let tf0 = std::time::Instant::now(); let res = render_frame_opts(p, (f0 + k) as f64 / p.fps, cache, transparent).and_then(|mut f| {if transparent { unpremultiply(&mut f); } prof("frame_total", tf0); let tq = std::time::Instant::now(); crate::color::pipeline::output(p,&mut f)?; prof("pipe_out", tq); let tq = std::time::Instant::now(); let r = to_rgba8(&f); prof("to_rgba8", tq); Ok(r)}); *slots[k - i].lock().unwrap() = Some(res); }); } });
                let frames: Vec<Result<Vec<u8>, String>> = slots.into_iter().map(|m| m.into_inner().unwrap().unwrap()).collect();
                for f in frames { if frame_tx.send(f?).is_err() { return Err("encoder closed early".into()); } }
                if let Some(handle) = prefetch {
                    let joined: Result<(Cache, usize), String> = handle.join().map_err(|_| "prefetch decode thread panicked".to_string())?;
                    let (new_cache, new_end) = joined?;
                    carried = Some((new_cache, new_end));
                }
                Ok(())
            })?;
            match carried { Some((new_cache, new_end)) => { cache = new_cache; i = end; end = new_end; } None => { i = end; end = n; } }
            crate::telemetry::progress_frames(i, n, "frames");
        }
        Ok(())
    })();
    drop(frame_tx);
    let writer_result: Result<(), String> = writer.join().map_err(|_| "encoder writer thread panicked".to_string())?;
    loop_result?;
    writer_result?;
    // mux mixed audio
    let pcm_full = crate::audio::mix(p); let (s0, s1) = ((f0 as f64 / p.fps * crate::audio::SR as f64) as usize * 2, (f1 as f64 / p.fps * crate::audio::SR as f64) as usize * 2); let pcm: Vec<f32> = { let a = s0.min(pcm_full.len()); let b = s1.min(pcm_full.len()); let mut v = pcm_full[a..b].to_vec(); v.resize(s1 - s0, 0.0); v };
    if audio && pcm.iter().any(|v|v.abs()>1e-6){crate::color::mux::legacy_audio(path,&pcm,n)?}
    Ok(n as f64 / p.fps)
}


thread_local! { static MATTE_DEPTH: std::cell::Cell<u32> = std::cell::Cell::new(0); }
/// Render one clip alone (full transforms/effects) on a transparent canvas: matte sources and difference references.

/// Clip-alone alpha planes shared across tool calls (each call runs on its own thread), so a lone render_frame can reuse them.
static SIL_CACHE: std::sync::Mutex<std::collections::VecDeque<((u64, u64, usize, usize), std::sync::Arc<Vec<f32>>)>> = std::sync::Mutex::new(std::collections::VecDeque::new());
/// Alpha plane of `id` rendered alone at timeline time `tt`, cached across frames. The key hashes the clip's full JSON, the time and the canvas size, so any edit to the clip changes the key (no stale silhouettes); at most 96 planes are kept.
fn silhouette_alpha(p: &Project, id: &str, tt: f64, cache: &Cache) -> Option<(usize, usize, std::sync::Arc<Vec<f32>>)> {
    use std::hash::{Hash, Hasher};
    let clip = p.tracks.iter().flat_map(|tr| tr.clips.iter()).find(|c| c.id == id)?; let mut hs = std::collections::hash_map::DefaultHasher::new(); serde_json::to_string(clip).ok()?.hash(&mut hs); serde_json::to_string(&(&p.grade_scopes, &p.editorial, &p.lights, &p.materials, &p.occluders, &p.color_pipeline, &p.camera, &p.assets, p.fps.to_bits())).ok()?.hash(&mut hs);
    let key = (hs.finish(), tt.to_bits(), p.width as usize, p.height as usize);
    if let Some(hit) = SIL_CACHE.lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) { return Some((key.2, key.3, hit)); }
    let fr = render_clip_alone(p, id, tt, cache)?; let a: Vec<f32> = (0..fr.w * fr.h).map(|i| fr.d[i * 4 + 3]).collect(); let rc = std::sync::Arc::new(a);
    if let Ok(mut c) = SIL_CACHE.lock() { if c.len() >= 96 { c.pop_front(); } c.push_back((key, rc.clone())); } Some((fr.w, fr.h, rc))
}
pub fn render_clip_alone(p_in: &Project, id: &str, t: f64, cache: &Cache) -> Option<Frame> {
    let resolved_owner; let p: &Project = if p_in.links.is_empty() && p_in.expr_scripts.is_empty() { p_in } else { resolved_owner = crate::ext::expr_interp_v7::resolve_live(p_in); resolved_owner.as_ref() };
    let c = p.tracks.iter().flat_map(|tr| tr.clips.iter()).find(|c| c.id == id)?; render_clip_alone_as(p, c, t, cache)
}
/// The track of a clip rendered alone that carries the clips its effects read (render_clip_alone_as); never drawn.
const ALONE_SRC_TRACK: &str = "\u{1}alone_src";
/// render_clip_alone for a clip given by value (e.g. one lengthened past its out point, pushwipe::outgoing_clip).
pub fn render_clip_alone_as(p: &Project, c: &Clip, t: f64, cache: &Cache) -> Option<Frame> {
    if MATTE_DEPTH.with(|d| d.get()) >= 3 { return None; }
    let c = c.clone();
    if t < c.start - 1e-9 || t >= c.start + c.duration - 1e-9 { return Some(Frame::new(p.width as usize, p.height as usize)); }
    let mut c = c; if let Some(m) = c.mask.as_mut() { m.matte = None; }
    // the clips its displacement map / set matte read come along on a hidden track, so those effects run here too (the outgoing
    // picture of a push, a matte source); each is read at t held to its own span, as a gradient wipe's luma clip is, so a map laid
    // over the outgoing shot stays on it while a push carries the shot on past its out point
    let mut srcs: Vec<std::sync::Arc<Clip>> = vec![];
    for e in c.effects.iter() { let Some(k) = crate::layerfx::source_key(&e.kind) else { continue };
        if let Some(m) = e.strings.get(k).and_then(|id| p.find_clip(id)).filter(|m| m.id != c.id && !srcs.iter().any(|s| s.id == m.id)) { let mut m = m.clone(); m.start += t - crate::pushwipe::luma_time(p, &m, t); srcs.push(std::sync::Arc::new(m)); } }
    let sub = Project { links: vec![], proxies: p.proxies.clone(), grade_scopes: p.grade_scopes.subset(&[crate::model::Track{id:"matte".into(),clips:vec![std::sync::Arc::new(c.clone())],locked:false,muted:false,solo:false,name:String::new()}]), lights: p.lights.clone(), materials: p.materials.clone(), editorial: p.editorial.clone(), ocio_view_delivery: None, occluders: p.occluders.clone(), color_pipeline: p.color_pipeline.clone(), hooks: Default::default(), expr_scripts: Default::default(), tracks: std::iter::once(crate::model::Track { id: "matte".into(), clips: vec![std::sync::Arc::new(c)], locked: false, muted: false, solo: false, name: String::new() }).chain((!srcs.is_empty()).then(|| crate::model::Track { id: ALONE_SRC_TRACK.into(), clips: srcs, locked: false, muted: false, solo: false, name: String::new() })).collect(), assets: p.assets.clone(), bins: p.bins.clone(), subclips: p.subclips.clone(), camera: p.camera.clone(), comp3d: p.comp3d.clone(), markers: vec![], name: String::new(), width: p.width, height: p.height, fps: p.fps, next_id: 0, deps: None, templates: vec![], base_dir: p.base_dir.clone(), master_speech_level: 0.0, master_speech_level_scope_end: 0.0 };
    MATTE_DEPTH.with(|d| d.set(d.get() + 1)); let r = render_frame_opts(&sub, t, cache, true).ok(); MATTE_DEPTH.with(|d| d.set(d.get() - 1)); r
}

/// lens_flare mode 1: the source position (strings.source_clip / source_light, else x/y) and the occluder visibility (strings.occluder, rendered alone
/// at t) are resolved here, where the project is known, and injected as params _src_x/_src_y/_visible on a copy of the effect (no render statics).
/// None when no effect needs it, so every other clip keeps its own effect list untouched.
pub fn resolve_flares(p: &Project, effs: &[crate::model::Effect], t: f64, lt: f64, cache: &Cache) -> Result<Option<Vec<crate::model::Effect>>, String> {
    let need = |e: &crate::model::Effect| e.kind == "lens_flare" && e.params.get("mode").map(|q| q.at(lt)).unwrap_or(0.0) >= 0.5 && ["source_clip", "source_light", "occluder"].iter().any(|k| e.strings.contains_key(*k));
    let vneed = |e: &crate::model::Effect| e.kind == "vignette" && crate::vignette::needs_source(&e.strings);
    if !effs.iter().any(|e| need(e) || vneed(e)) { return Ok(None); }
    let mut out = effs.to_vec();
    for e in out.iter_mut().filter(|e| vneed(&**e)) { let (sx, sy) = crate::vignette::source_px(p, &e.params, &e.strings, t, lt)?;
        e.params.insert("_src_x".into(), crate::keyframe::Prop::new(sx)); e.params.insert("_src_y".into(), crate::keyframe::Prop::new(sy)); }
    for e in out.iter_mut().filter(|e| need(&**e)) {
        let (sx, sy) = crate::flare::source_px(p, &e.params, &e.strings, t, lt)?; let mut vis = 1.0f32;
        if let Some(id) = e.strings.get("occluder") { if !p.tracks.iter().any(|tr| tr.clips.iter().any(|c| &c.id == id)) { return Err(format!("lens_flare: no clip '{id}' for occluder")); }
            if let Some(fr) = render_clip_alone(p, id, t, cache) { let a: Vec<f32> = (0..fr.w * fr.h).map(|i| fr.d[i * 4 + 3]).collect(); vis = crate::flare::flare_visibility(&a, fr.w, fr.h, sx, sy, e.params.get("source_r").map(|q| q.at(lt)).unwrap_or(8.0)); } }
        e.params.insert("_src_x".into(), crate::keyframe::Prop::new(sx)); e.params.insert("_src_y".into(), crate::keyframe::Prop::new(sy)); e.params.insert("_visible".into(), crate::keyframe::Prop::new(vis as f64));
    }
    Ok(Some(out))
}

/// Effects with a per-effect mask (strings.mask_layer = id of a layer in the clip's mask stack) only apply where that layer covers.
/// A keyer with strings.ref_clip / aux_path (clean plate) or garbage_mask / core_mask (mask layer ids, mode none) also runs per effect.
fn keyer_aux_fx(e: &crate::model::Effect) -> bool { e.kind == "keyer" && ["ref_clip", "aux_path", "garbage_mask", "core_mask"].iter().any(|k| e.strings.contains_key(*k)) }
fn keyer_aux(p: &Project, c: &Clip, e: &crate::model::Effect, before: &Frame, lt: f64, t: f64, cache: &Cache, ctx: &crate::masks::Ctx) -> Result<crate::keying::KeyerAuxOwned, String> {
    let clean_plate = if let Some(id) = e.strings.get("ref_clip") { Some(render_clip_alone(p, id, t, cache).ok_or_else(|| format!("keyer: strings.ref_clip '{id}' is not a clip (or could not be rendered)"))?) }
        else if let Some(path) = e.strings.get("aux_path") { Some(crate::pixels::read(&crate::deps::resolve_str(path)).map_err(|err| format!("keyer: strings.aux_path '{path}' unreadable: {err}"))?) } else { None };
    let layer = |key: &str, id: Option<&String>| -> Result<Option<Vec<f32>>, String> { let Some(id) = id else { return Ok(None) };
        let l = c.mask.as_ref().and_then(|m| m.layers.iter().find(|l| &l.id == id)).ok_or_else(|| format!("keyer: strings.{key} '{id}' is not a mask layer of clip {}", c.id))?;
        Ok(Some(crate::masks::layer_coverage(l, before, lt, ctx))) };
    Ok(crate::keying::KeyerAuxOwned { clean_plate, garbage: layer("garbage_mask", e.strings.get("garbage_mask"))?, core: layer("core_mask", e.strings.get("core_mask"))? })
}
/// Where clip `c` lands on the canvas at `lt` (its composed x/y/scale/rotation, its motion path and the move, zoom or turn of
/// its own transition in, e.g. a push or slide), for effects that read another clip in screen space (layerfx.rs Displacement Map).
fn layer_place(p: &Project, c: &Clip, lt: f64, w: usize, h: usize) -> crate::layerfx::Place {
    let (mx, my, ma) = match &c.motion_path { Some(mp) => { let (x, y, a) = crate::motion::sample(mp, prop(c, "path", lt), w, h); (x, y, if mp.auto_orient { a + mp.orient_offset } else { 0.0 }) } None => (0.0, 0.0, 0.0) };
    let mut pl = crate::layerfx::Place { scale: xprop(p, c, "scale", lt), rot_deg: xprop(p, c, "rotation", lt) + ma, dx: xprop(p, c, "x", lt) + mx, dy: xprop(p, c, "y", lt) + my };
    if let Some(tr) = c.transition_in.as_ref().filter(|tr| lt < tr.duration) { let tp = crate::transitions::plan(tr, lt, w, h); pl.dx += tp.dx; pl.dy += tp.dy; pl.scale *= tp.scale; pl.rot_deg += tp.rot; } // as the draw composes it
    // the outgoing picture of a push is moved out after it is drawn alone (pushwipe::outgoing puts that move on its copy)
    if let Some(q) = c.props.get(crate::pushwipe::OUT_DX) { pl.dx += q.base; } if let Some(q) = c.props.get(crate::pushwipe::OUT_DY) { pl.dy += q.base; }
    pl
}
fn apply_effects_masked(p: &Project, c: &Clip, f: &mut Frame, lt: f64, seed: u64, t: f64, cache: &Cache) -> Result<(), String> {
    let special = |e: &crate::model::Effect| e.strings.contains_key("mask_layer") || keyer_aux_fx(e) || crate::layerfx::driven(e) || e.kind == "set_channels";
    if !c.effects.iter().any(special) { apply_effects_local(&c.effects, f, lt, seed)?;if let Some(g)=&c.grade_graph{g.apply(f,lt,seed,p.fps)?;}p.grade_scopes.apply_clip(p,&c.id,f,lt,seed)?;return Ok(()); }
    let mut i = 0;
    while i < c.effects.len() {
        let e = &c.effects[i];
        if special(e) {
            let before = Frame { w: f.w, h: f.h, d: f.d.clone() };
            let refs = |id: &str| render_clip_alone(p, id, t, cache); let ctx = crate::masks::Ctx { refs: &refs, fps: p.fps };
            if keyer_aux_fx(e) { if e.strings.contains_key("ocio_domain") || e.strings.contains_key("wrapper_working_gamut") { return Err("keyer: a clean plate or garbage/core matte cannot be combined with a colour-domain wrapper (ocio_domain / wrapper_working_gamut)".into()); }
                // the keyer's per-pixel inputs go straight to keying::apply_keyer (effects::apply carries params and strings only)
                let aux = keyer_aux(p, c, e, &before, lt, t, cache, &ctx)?; crate::keying::apply_keyer(&e.params, f, lt, &aux.view())?; }
            else if e.kind == "set_channels" { // may run with no ref_clip at all (pure this/on/off sources): layerfx::set_channels tolerates an empty reference frame
                let src = e.strings.get("ref_clip").and_then(|id| render_clip_alone(p, id, t, cache)).unwrap_or_else(|| Frame::new(0, 0));
                crate::layerfx::apply(e, f, &src, lt, None, seed);
            }
            else if let Some(k) = crate::layerfx::source_key(&e.kind).filter(|_| crate::layerfx::driven(e)) { if let Some(src) = e.strings.get(k).and_then(|id| render_clip_alone(p, id, t, cache)) { let place = (e.kind == "displacement_map").then(|| layer_place(p, c, lt, f.w, f.h)); crate::layerfx::apply(e, f, &src, lt, place, seed); } } // displacement_map / set_matte: the driving clip rendered alone; the map is read where the clip lands (layer_place)
            else { apply_effects_local(std::slice::from_ref(e), f, lt, seed)?; }
            if let Some(l) = e.strings.get("mask_layer").and_then(|mid| c.mask.as_ref().and_then(|m| m.layers.iter().find(|l| &l.id == mid))) {
                let cov = crate::masks::layer_coverage(l, &before, lt, &ctx);
                for k in 0..f.w * f.h { let a = cov[k]; if a < 1.0 { for j in 0..4 { f.d[k * 4 + j] = before.d[k * 4 + j] * (1.0 - a) + f.d[k * 4 + j] * a; } } }
            }
            i += 1;
        } else { let j = (i..c.effects.len()).find(|&j| special(&c.effects[j])).unwrap_or(c.effects.len()); apply_effects_local(&c.effects[i..j], f, lt, seed)?; i = j; }
    }
    if let Some(g)=&c.grade_graph{g.apply(f,lt,seed,p.fps)?;}p.grade_scopes.apply_clip(p,&c.id,f,lt,seed)?;
    Ok(())
}

fn apply_mask(f: &mut Frame, m: &Mask, t: f64) {
    if m.shape == "stack" { return; }
    let g = |k: &str, d: f64| m.props.get(k).map(|p| p.at(t)).unwrap_or(d) as f32;
    let (cx, cy, mw, mh, feather, rot) = (g("cx",0.5), g("cy",0.5), g("w",0.5), g("h",0.5), g("feather",0.02).max(0.0005), g("rotation",0.0).to_radians());
    let (w, h) = (f.w as f32, f.h as f32); let (s, c) = (rot.sin(), rot.cos());
    for y in 0..f.h { for x in 0..f.w {
        let (nx, ny) = (x as f32 / w - cx, y as f32 / h - cy);
        let (ux, uy) = ((nx*c + ny*s) * (w / h), (-nx*s + ny*c));
        let (ux, uy) = (ux / (w / h), uy);
        let d = match m.shape.as_str() {
            "ellipse" => { let e = ((ux/(mw/2.0)).powi(2) + (uy/(mh/2.0)).powi(2)).sqrt(); (1.0 - e) * (mw.min(mh)/2.0) }
            "linear" => -ux,
            _ => (mw/2.0 - ux.abs()).min(mh/2.0 - uy.abs()),
        };
        let mut a = (d / feather + 0.5).clamp(0.0, 1.0); if m.invert { a = 1.0 - a; }
        f.d[(y*f.w+x)*4+3] *= a;
    }}
}

/// Perspective warp of the whole frame by moving its 4 corners (TL,TR,BR,BL) by pixel offsets.
fn corner_pin(src: &Frame, off: &[[f64; 2]; 4]) -> Frame {
    let (w, h) = (src.w as f64, src.h as f64);
    let from = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]; let to: Vec<[f64; 2]> = (0..4).map(|i| [from[i][0] + off[i][0], from[i][1] + off[i][1]]).collect();
    // homography mapping `to` (dest) -> `from` (source): solve 8x8
    let mut a = [[0f64; 9]; 8];
    for i in 0..4 { let (x, y) = (to[i][0], to[i][1]); let (u, v) = (from[i][0], from[i][1]);
        a[i*2] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u]; a[i*2+1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v]; }
    for c in 0..8 { let mut m = c; for r in c + 1..8 { if a[r][c].abs() > a[m][c].abs() { m = r; } } a.swap(c, m); let d = a[c][c]; if d.abs() < 1e-12 { return Frame { w: src.w, h: src.h, d: src.d.clone() }; }
        for k in c..9 { a[c][k] /= d; } for r in 0..8 { if r != c { let f = a[r][c]; for k in c..9 { a[r][k] -= f * a[c][k]; } } } }
    let hm: Vec<f64> = (0..8).map(|i| a[i][8]).collect();
    let mut o = Frame::new(src.w, src.h);
    for y in 0..src.h { for x in 0..src.w { let (xf, yf) = (x as f64 + 0.5, y as f64 + 0.5); let dnm = hm[6] * xf + hm[7] * yf + 1.0; let sx = (hm[0] * xf + hm[1] * yf + hm[2]) / dnm; let sy = (hm[3] * xf + hm[4] * yf + hm[5]) / dnm;
        if sx < 0.0 || sy < 0.0 || sx > w - 1.0 || sy > h - 1.0 { continue; } let (x0, y0) = (sx.floor() as usize, sy.floor() as usize); let (x1, y1) = ((x0 + 1).min(src.w - 1), (y0 + 1).min(src.h - 1)); let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
        for j in 0..4 { let p00 = src.d[(y0 * src.w + x0) * 4 + j]; let p10 = src.d[(y0 * src.w + x1) * 4 + j]; let p01 = src.d[(y1 * src.w + x0) * 4 + j]; let p11 = src.d[(y1 * src.w + x1) * 4 + j]; o.d[(y * src.w + x) * 4 + j] = (p00 * (1.0 - fx) + p10 * fx) * (1.0 - fy) + (p01 * (1.0 - fx) + p11 * fx) * fy; } } }
    o
}

/// Dynamic text tokens, resolved per frame at render time: `{tc:S}` timecode HH:MM:SS:FF offset by S seconds,
/// `{bar:BPM:S}` musical position "BAR bb.b.s" at BPM offset by S seconds, `{t:S}` seconds with 2 decimals, `{pct:A:B}` percent 0..100 from timeline time A to B,
/// `{tcd:S}` real SMPTE drop-frame timecode (29.97/59.94 only; falls back to a dashed placeholder at any other fps rather than a wrong label) via timecode::label.
pub fn dyn_text(text: &str, gt: f64, fps: f64) -> String {
    if !text.contains('{') { return text.to_string(); }
    let mut out = String::new(); let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]); let tail = &rest[i + 1..];
        if let Some(j) = tail.find('}') {
            let parts: Vec<&str> = tail[..j].split(':').collect(); let f = |k: usize| parts.get(k).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            match parts[0] {
                "tc" => { let g = gt + f(1); out.push_str(&format!("{:02}:{:02}:{:02}:{:02}", (g / 3600.0) as u32, ((g / 60.0) as u32) % 60, (g as u32) % 60, (g.fract() * fps) as u32)); }
                "bar" => { let g = gt + f(2); let bars = g / (60.0 / f(1).max(1.0)); out.push_str(&format!("{:02}.{}.{}", (bars / 4.0) as u32 + 1, (bars as u32) % 4 + 1, (bars.fract() * 4.0) as u32 + 1)); }
                "t" => out.push_str(&format!("{:.2}", gt + f(1))),
                "pct" => { let (a, b) = (f(1), f(2)); out.push_str(&format!("{}", (((gt - a) / (b - a).max(1e-6)).clamp(0.0, 1.0) * 100.0).round() as u32)); }
                "tcd" => { let g = (gt + f(1)).max(0.0); out.push_str(&crate::timecode::label(g, fps, true).unwrap_or_else(|_| "--:--:--;--".into())); }
                _ => { out.push('{'); out.push_str(&tail[..=j]); rest = &tail[j + 1..]; continue; }
            }
            rest = &tail[j + 1..];
        } else { out.push('{'); rest = tail; }
    }
    out.push_str(rest); out
}

/// Local (spatially invariant) effects run on the layer's alpha bounding box plus a margin instead of the full canvas.
/// Big win for small shapes with glow/blur/shadow (echo ghosts, particles, UI plates).
fn apply_effects_local(effs: &[crate::model::Effect], f: &mut Frame, lt: f64, seed: u64) -> Result<(), String> {
    const LOCAL: [&str; 7] = ["blur", "outer_glow", "drop_shadow", "glow", "choker", "feather_alpha", "outline"];
    if effs.is_empty() { return Ok(()); }
    let (w, h) = (f.w, f.h);
    let mut bb: Option<(usize, usize, usize, usize)> = None; // x0,y0,x1,y1
    let mut need = effs.iter().any(|e| LOCAL.contains(&e.kind.as_str()));
    if need { let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize); for y in 0..h { let row = &f.d[y * w * 4..(y + 1) * w * 4]; let mut first = None; let mut last = 0; for x in 0..w { if row[x * 4 + 3] > 0.0 { if first.is_none() { first = Some(x); } last = x; } } if let Some(fx) = first { x0 = x0.min(fx); x1 = x1.max(last); y0 = y0.min(y); y1 = y1.max(y); } }
        if x1 < x0 { need = false; } else { bb = Some((x0, y0, x1, y1)); } }
    let mut i = 0;
    while i < effs.len() {
        let e = &effs[i];
        if need && LOCAL.contains(&e.kind.as_str()) {
            // run the whole consecutive run of local effects on one crop
            let mut j = i; let mut mg = 0.0f64; while j < effs.len() && LOCAL.contains(&effs[j].kind.as_str()) { let rad = effs[j].params.get("radius").map(|p| p.at(lt)).unwrap_or(16.0).abs(); mg += if effs[j].kind == "glow" && crate::glow::mode(&effs[j].params, lt) { crate::glow::support_of(&effs[j].params, lt) as f64 + 12.0 } else { rad * 3.0 + 12.0 }; j += 1; }
            let (x0, y0, x1, y1) = bb.unwrap(); let m = mg.ceil() as usize;
            let (cx0, cy0, cx1, cy1) = (x0.saturating_sub(m), y0.saturating_sub(m), (x1 + m).min(w - 1), (y1 + m).min(h - 1)); let (cw, ch) = (cx1 - cx0 + 1, cy1 - cy0 + 1);
            let mut cf = Frame::new(cw, ch); for y in 0..ch { cf.d[y * cw * 4..(y + 1) * cw * 4].copy_from_slice(&f.d[((y + cy0) * w + cx0) * 4..((y + cy0) * w + cx0 + cw) * 4]); }
            for e2 in &effs[i..j] { effects::apply(&e2.kind, &e2.params, &e2.strings, &mut cf, lt, seed)?; }
            for y in 0..ch { f.d[((y + cy0) * w + cx0) * 4..((y + cy0) * w + cx0 + cw) * 4].copy_from_slice(&cf.d[y * cw * 4..(y + 1) * cw * 4]); }
            i = j; continue;
        }
        effects::apply(&e.kind, &e.params, &e.strings, f, lt, seed)?; i += 1;
    }
    Ok(())
}

thread_local! { static PROF: std::cell::RefCell<std::collections::BTreeMap<&'static str, f64>> = Default::default(); }
fn prof(k: &'static str, t: std::time::Instant) { if std::env::var("KIMO_PROF").is_ok() { PROF.with(|p| *p.borrow_mut().entry(k).or_insert(0.0) += t.elapsed().as_secs_f64()); } }
pub fn prof_dump() { PROF.with(|p| { let mut p = p.borrow_mut(); if !p.is_empty() { eprintln!("PROF {:?}", *p); p.clear(); } }); }

/// Inspect a mask: renders the clip's source (after effects, before its own mask) and shows the coverage of one layer or the whole stack.
pub fn render_mask_png(p: &Project, clip_id: &str, mask_id: Option<&str>, t: f64, view: &str, path: &str, cache: &mut Cache) -> Result<(), String> {
    prepare_at(p, cache, Some(t))?;
    let c = p.tracks.iter().flat_map(|tr| tr.clips.iter()).find(|c| c.id == clip_id).ok_or("no such clip")?; let lt = t - c.start; let seed = (t * p.fps).round() as u64;
    let (w, h) = (p.width as usize, p.height as usize);
    let mut f = source_frame(p, c, lt, cache); apply_effects_masked(p, c, &mut f, lt, seed, t, cache)?;
    if let Some(m) = &c.mask { let mut tmp = Frame { w, h, d: f.d.clone() }; apply_mask(&mut tmp, m, lt); if m.shape != "stack" { for i in 0..w * h { f.d[i * 4 + 3] = tmp.d[i * 4 + 3]; } } }
    let cache_ref: &Cache = cache; let refs = |id: &str| render_clip_alone(p, id, t, cache_ref); let ctx = crate::masks::Ctx { refs: &refs, fps: p.fps };
    let layers = c.mask.as_ref().map(|m| m.layers.clone()).unwrap_or_default();
    let cov: Vec<f32> = match mask_id { Some(id) => { let l = layers.iter().find(|l| l.id == id).ok_or("no such mask")?; crate::masks::layer_coverage(l, &f, lt, &ctx) } None => crate::masks::stack_coverage(&layers, &f, lt, &ctx).unwrap_or_else(|| vec![1.0; w * h]) };
    let mut o = Frame::new(w, h);
    for i in 0..w * h { let k = cov[i]; match view {
        "overlay" => { let g = 0.35 + 0.0; for j in 0..3 { o.d[i * 4 + j] = f.d[i * 4 + j] * (1.0 - k * g) + if j == 0 { 1.0 } else { 0.0 } * k * g; } }
        "cutout" => { for j in 0..3 { o.d[i * 4 + j] = f.d[i * 4 + j]; } } // straight colour; alpha carries the matte so the PNG is a real transparent cutout
        _ => { for j in 0..3 { o.d[i * 4 + j] = k; } } } o.d[i * 4 + 3] = if view == "cutout" { k * f.d[i * 4 + 3] } else { 1.0 }; }
    write_image(&o,path)
}

/// Write a colour-wheel visualisation of the optical flow between a media clip's frame at `t` and the next one.
pub fn render_flow_png(p: &Project, clip_id: &str, t: f64, path: &str, cache: &mut Cache) -> Result<f64, String> { render_flow_field(p, clip_id, t, path, cache).map(|r| r.0) }
/// render_flow_png that also hands back the flow field it drew: (seconds, u, v, field width, field height); v6 render_flow stats.
pub fn render_flow_field(p: &Project, clip_id: &str, t: f64, path: &str, cache: &mut Cache) -> Result<(f64, Vec<f32>, Vec<f32>, usize, usize), String> {
    let c = p.tracks.iter().flat_map(|tr| tr.clips.iter()).find(|c| c.id == clip_id).ok_or("no such clip")?; let (w, h) = (p.width as usize, p.height as usize);
    if !t.is_finite()||t<c.start||t>=c.start+c.duration{return Err("flow time must be within clip".into())}
    let lt0=t-c.start;let last=media_total(c,p.fps).saturating_sub(1);let fidx=|lt:f64|->usize{let d=c.display_time(lt.max(0.));media_index(c,d,p.fps).min(last)};
    let(i0,mut i1)=(fidx(lt0),fidx(lt0+1./p.fps));let mut lt1=lt0+1./p.fps;let mut swapped=false;
    if i1==i0{lt1=lt0-1./p.fps;i1=fidx(lt1);swapped=i1!=i0;}
    if i1==i0{return Err("no motion between displayed frames here (freeze or window end)".into())}
    prepare_range(p,cache,t.min(c.start+lt1),t.max(c.start+lt1))?;
    let fr=cache.frames_of(&c.id).ok_or("not a media clip")?;
    let(a,b)=flow_pair(fr,if swapped{i1}else{i0},if swapped{i0}else{i1},w,h)?;
    let t0=std::time::Instant::now();let(u,v,fw,fh)=crate::flow::flow(&a,&b,5);let secs=t0.elapsed().as_secs_f64();
    let mm = u.iter().zip(&v).map(|(a, b)| (a * a + b * b).sqrt()).fold(0.0f32, f32::max).max(1.0); let viz = crate::flow::flow_viz(&u, &v, fw, fh, mm * 0.8);
    write_image(&viz,path)?;Ok((secs,u,v,fw,fh))
}

fn flow_pair(fr:&Frames,a:usize,b:usize,w:usize,h:usize)->Result<(Frame,Frame),String>{
 if !fr.covers(a.min(b),a.max(b)){return Err("active flow frame pair exceeds cache coverage".into())}
 let n=w.checked_mul(h).and_then(|n|n.checked_mul(4)).ok_or("flow dimensions overflow")?;if fr.fs!=n{return Err("flow cache shape mismatch".into())}
 crate::guard::reserve(n.checked_mul(8).ok_or("flow pair allocation overflow")?);
 let mk=|i:usize|->Result<Frame,String>{let d=if let Some(v)=fr.get_float(i){v.to_vec()}else{fr.get(i).ok_or("no flow frame")?.iter().map(|v|*v as f32/255.).collect()};Ok(Frame{w,h,d})};Ok((mk(a)?,mk(b)?))
}
#[cfg(test)]mod flow_pair_tests{use super::*;
#[test]fn flow_pair_keeps_float_and_rejects_clamped_video(){let d=vec![-0.25,4.,0.18,0.5,1.,2.,3.,1.];let f=Frames{still:false,working_linear:true,quantized:Default::default(),linear_buf:Some(d.clone()),buf:vec![],fs:4,first:10};let(a,b)=flow_pair(&f,10,11,1,1).unwrap();assert_eq!(a.d,&d[..4]);assert_eq!(b.d,&d[4..]);assert!(f.quantized.get().is_none());assert!(flow_pair(&f,10,12,1,1).is_err());}
}

#[cfg(test)]mod decode_lifecycle_test{use super::*;
#[test]fn decoder_reaped_on_unwind(){let mut id=0;let r=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let child=Command::new("sleep").arg("30").spawn().unwrap();id=child.id();let _decoder=DecodeProcess(child);panic!("test cancelled decode");}));assert!(r.is_err());assert!(!std::path::Path::new(&format!("/proc/{id}")).exists());}
}
#[cfg(test)]mod legacy_encoder_tests{use super::*;
#[test]fn ordinary_video_argv_declares_matching_conversion_and_tags(){for(w,h)in[(320,180),(1280,720)]{let mut p=Project::default();p.width=w;p.height=h;p.fps=25.;let cmd=legacy_encoder_command(&p,"proof.mp4",23).unwrap();let logged:Vec<_>=cmd.get_args().map(|v|v.to_string_lossy().to_string()).collect();assert_eq!(logged,vec!["-v","error","-y","-f","rawvideo","-pix_fmt","rgba","-s",&format!("{w}x{h}"),"-r","25","-i","-","-c:v","libx264","-vf","scale=out_color_matrix=bt709:out_range=tv","-pix_fmt","yuv420p","-color_range","tv","-color_primaries","bt709","-color_trc","iec61966-2-1","-colorspace","bt709","-crf","23","proof.mp4"]);}}
#[test]fn ordinary_video_rejects_untaggable_output_before_spawn(){let mut p=Project::default();p.color_pipeline.enabled=true;for g in 1..=7{p.color_pipeline.output_gamut=g;assert!(legacy_encoder_command(&p,"unused.mp4",18).is_err())}p.color_pipeline.output_gamut=0;for t in[2,3,4,5,6,7]{p.color_pipeline.output_transfer=t;assert!(legacy_encoder_command(&p,"unused.mp4",18).is_err())}for(t,name)in[(0,"linear"),(1,"iec61966-2-1"),(8,"bt709")]{p.color_pipeline.output_transfer=t;let cmd=legacy_encoder_command(&p,"unused.mp4",18).unwrap();let args:Vec<_>=cmd.get_args().map(|v|v.to_string_lossy().to_string()).collect();assert!(args.windows(2).any(|v|v==["-color_trc",name]));}}
#[test]fn parallel_out_of_order_arrival_preserves_writer_order(){let path=std::env::temp_dir().join(format!("kimo-ordered-{}.bin",std::process::id()));let mut cmd=Command::new("sh");cmd.args(["-c",&format!("exec cat > {}",path.display())]);let mut encoder=crate::color::process::EncodeStream::new(&mut cmd,4,std::time::Duration::from_secs(2)).unwrap();let slots:Vec<_>=(0..4).map(|_|std::sync::Mutex::new(None)).collect();std::thread::scope(|sc|{for k in 0..4{let slots=&slots;sc.spawn(move||{std::thread::sleep(std::time::Duration::from_millis((3-k)*10));*slots[k as usize].lock().unwrap()=Some(vec![k as u8;4]);});}});for slot in slots{encoder.write(slot.into_inner().unwrap().unwrap()).unwrap();}encoder.finish().unwrap();assert_eq!(std::fs::read(&path).unwrap(),vec![0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3]);std::fs::remove_file(path).unwrap();}
}
#[cfg(test)]mod legacy_unwind_test{use super::*;
#[test]fn render_worker_panic_reaps_encoder(){let pidpath=std::env::temp_dir().join(format!("kimo-legacy-unwind-pid-{}",std::process::id()));let script=format!("echo $$ > {}; exec sleep 30",pidpath.display());let result=std::panic::catch_unwind(||{let _encoder=crate::color::process::EncodeStream::new(Command::new("sh").args(["-c",&script]),4,std::time::Duration::from_secs(2)).unwrap();std::thread::sleep(std::time::Duration::from_millis(30));std::thread::scope(|sc|{sc.spawn(||panic!("injected render worker failure"));});});assert!(result.is_err());let id=std::fs::read_to_string(&pidpath).unwrap();assert!(!std::path::Path::new(&format!("/proc/{}",id.trim())).exists());std::fs::remove_file(pidpath).unwrap();}
}
#[cfg(test)]mod image_writer_tests{use super::*;
#[test]fn alpha_png_exact_bytes_and_decoded_hash(){use std::hash::{Hash,Hasher};let base=std::env::temp_dir().join(format!("kimo-image-base-{}.png",std::process::id()));let after=std::env::temp_dir().join(format!("kimo-image-after-{}.png",std::process::id()));let f=Frame{w:32,h:16,d:(0..512).flat_map(|i|[i as f32/511.,0.25,0.75,(i%32) as f32/31.]).collect()};let mut old=image_encoder_command(32,16,base.to_str().unwrap()).stdin(Stdio::piped()).spawn().unwrap();old.stdin.take().unwrap().write_all(&to_rgba8(&f)).unwrap();assert!(old.wait().unwrap().success());write_image(&f,after.to_str().unwrap()).unwrap();assert_eq!(std::fs::read(&base).unwrap(),std::fs::read(&after).unwrap());let decode=|path:&std::path::Path|Command::new("ffmpeg").args(["-v","error","-i"]).arg(path).args(["-f","rawvideo","-pix_fmt","rgba","-"]).output().unwrap().stdout;let a=decode(&base);let b=decode(&after);let hash=|v:&[u8]|{let mut h=std::collections::hash_map::DefaultHasher::new();v.hash(&mut h);h.finish()};assert_eq!(hash(&a),hash(&b));assert_eq!(a,b);assert!(b.chunks_exact(4).any(|p|p[3]>0&&p[3]<255));std::fs::remove_file(base).unwrap();std::fs::remove_file(after).unwrap();}
#[test]fn stuck_image_child_deadline_reaps(){let pid=std::env::temp_dir().join(format!("kimo-image-stuck-pid-{}",std::process::id()));let mut cmd=Command::new("sh");cmd.args(["-c",&format!("echo $$ > {}; exec sleep 30",pid.display())]);let f=Frame{w:512,h:512,d:vec![0.25;512*512*4]};let start=std::time::Instant::now();assert!(write_image_command(&f,&mut cmd,std::time::Duration::from_millis(100)).unwrap_err().contains("deadline"));assert!(start.elapsed()<std::time::Duration::from_secs(2));let id=std::fs::read_to_string(&pid).unwrap();assert!(!std::path::Path::new(&format!("/proc/{}",id.trim())).exists());std::fs::remove_file(pid).unwrap();}
#[test]fn image_argv_is_unchanged(){let cmd=image_encoder_command(32,16,"alpha.png");let argv:Vec<_>=cmd.get_args().map(|v|v.to_string_lossy().to_string()).collect();assert_eq!(argv,vec!["-v","error","-y","-f","rawvideo","-pix_fmt","rgba","-s","32x16","-i","-","-frames:v","1","alpha.png"]);}
}
#[cfg(test)]mod color_tool_image_argv{use super::*;
#[test]fn each_migrated_color_writer_uses_original_argv(){for writer in ["render_aces2_reference","render_gamut_excursions","render_exposure_false_color"]{let path=format!("{writer}.png");let cmd=image_encoder_command(320,180,&path);let args:Vec<_>=cmd.get_args().map(|v|v.to_string_lossy().to_string()).collect();assert_eq!(args,vec!["-v","error","-y","-f","rawvideo","-pix_fmt","rgba","-s","320x180","-i","-","-frames:v","1",&path],"writer{writer}");}}
}

/// Final-screen occluder matte (1 = occluder) of an adaptive-occluder pinned clip at project time t, in the same space the clip's own layer lands in (scale, pos, rotation, motion path, transition_in, 3D pose and project camera). Evaluated on demand, independent of track order and of the target clip's opacity or visibility. Cached per (clip, time).
pub fn target_screen_occ(p: &Project, id: &str, t: f64) -> Option<std::sync::Arc<Vec<f32>>> {
    let hit = crate::shards::TARGET_MATTE.with(|m| m.borrow().get(id).filter(|(tt, _)| (*tt - t).abs() < 1e-9).map(|(_, o)| o.clone())); if hit.is_some() { return hit; }
    let c = flat_clips(&p.tracks, 0.0).into_iter().find(|x| x.id == id)?; let sp = p.occluders.get(id).filter(|s| s.adaptive)?;
    if t < c.start - 1e-9 || t >= c.start + c.duration - 1e-9 { return None; }
    let (w, h) = (p.width as usize, p.height as usize);
    let lt = if c.posterize_fps > 0.0 { (((t - c.start) * c.posterize_fps + 1e-6).floor() / c.posterize_fps).max(0.0) } else { (t - c.start).max(0.0) };
    let vis = crate::planar::adaptive_matte(&c, sp, p.fps, t, w, h)?;
    let (mut dx, mut dy, mut sc, mut rot_extra) = (xprop(p, &c, "x", lt), xprop(p, &c, "y", lt), xprop(p, &c, "scale", lt), 0.0);
    if let Some(mp) = &c.motion_path { let (x, y, a) = crate::motion::sample(mp, prop(&c, "path", lt), w, h); dx += x; dy += y; if mp.auto_orient { rot_extra += a + mp.orient_offset; } }
    if let Some(tr_in) = &c.transition_in { if lt < tr_in.duration { let pl = crate::transitions::plan(tr_in, lt, w, h); dx += pl.dx; dy += pl.dy; sc *= pl.scale; rot_extra += pl.rot; } }
    let (rxv, ryv, pzv) = (prop(&c, "rot_x", lt), prop(&c, "rot_y", lt), prop(&c, "pos_z", lt));
    let mut mf = Frame::new(w, h); for i in 0..w * h { mf.d[i * 4] = vis[i]; mf.d[i * 4 + 1] = vis[i]; mf.d[i * 4 + 2] = vis[i]; mf.d[i * 4 + 3] = 1.0; }
    let tm = if rxv != 0.0 || ryv != 0.0 || pzv != 0.0 { let cam = crate::three_d::Camera::from_props(&p.camera, t); let pose = crate::three_d::Pose { pos: [dx, dy, pzv], rot: [rxv, ryv, xprop(p, &c, "rotation", lt) + rot_extra], scale: sc }; crate::three_d::render_layer(&mf, &cam, &pose, prop(&c, "backface", lt) != 0.0) } else { transform_bb(&mf, Some((0, 0, w - 1, h - 1)), sc, xprop(p, &c, "rotation", lt) + rot_extra, dx, dy) };
    let occ: Vec<f32> = (0..w * h).map(|i| { let a = tm.d[i * 4 + 3].clamp(0.0, 1.0); 1.0 - (tm.d[i * 4] * a + (1.0 - a)) }).collect();
    let arc = std::sync::Arc::new(occ); crate::shards::TARGET_MATTE.with(|m| { m.borrow_mut().insert(id.to_string(), (t, arc.clone())); }); Some(arc)
}

/// Effects that depend only on their (constant) parameters and the pixels: no time, seed or randomness.
const STATIC_EFFECTS: &[&str] = &["exposure", "contrast", "saturation", "temperature", "tint", "color_wheels", "curves", "teal_orange", "vignette", "blur", "glow", "bloom", "invert", "sharpen", "posterize", "sepia", "outline", "outer_glow", "long_shadow", "choker", "feather_alpha", "silhouette_fill"];
fn is_static_text(c: &Clip) -> bool {
    let plain = match &c.source { Source::Text { text, .. } => !text.contains('{'), Source::Shape { .. } => true, _ => false }; if !plain { return false; }
    let constant = |pr: &crate::keyframe::Prop| pr.keys.is_empty() && pr.expr.is_empty();
    c.text_fx.is_none() && c.mask.is_none() && c.echo.is_none() && c.posterize_fps == 0.0
        && c.props.get("reveal").map_or(true, constant)
        && c.grade_graph.is_none()
        && c.effects.iter().all(|e| STATIC_EFFECTS.contains(&e.kind.as_str()) && e.strings.is_empty() && e.params.values().all(constant))
}
/// Static text layers (constant text, params, effects) render identically every frame: render once per export, keep only the non-zero crop, rebuild by pasting.
/// Only active for export caches; a bit-exact crop (any non-zero channel) so the rebuilt frame equals the full render.
fn static_layer(p: &Project, c: &Clip, lt: f64, seed: u64, t: f64, cache: &Cache) -> Result<Option<Frame>, String> {
    if !cache.export_mode || p.grade_scopes.clip_has_grade(&c.id) || !is_static_text(c) || c.effects.iter().any(|e| e.kind == "relight") { return Ok(None); }
    let (w, h) = (p.width as usize, p.height as usize);
    if let Some(cr) = cache.statics.read().unwrap().get(&c.id).cloned() {
        let mut f = Frame::new(w, h); let cw = cr.x1 - cr.x0;
        for (k, y) in (cr.y0..cr.y1).enumerate() { f.d[(y * w + cr.x0) * 4..(y * w + cr.x1) * 4].copy_from_slice(&cr.d[k * cw * 4..(k + 1) * cw * 4]); }
        return Ok(Some(f));
    }
    let mut f = source_frame(p, c, lt, cache); apply_effects_masked(p, c, &mut f, lt, seed, t, cache)?;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h { let row = &f.d[y * w * 4..(y + 1) * w * 4]; if let Some(a) = (0..w).find(|&x| (0..4).any(|j| row[x * 4 + j].to_bits() != 0)) { let b = (0..w).rev().find(|&x| (0..4).any(|j| row[x * 4 + j].to_bits() != 0)).unwrap(); x0 = x0.min(a); x1 = x1.max(b + 1); y0 = y0.min(y); y1 = y1.max(y + 1); } }
    if x1 <= x0 { x0 = 0; y0 = 0; x1 = 0; y1 = 0; }
    if (x1 - x0) * (y1 - y0) * 16 <= 64 * 1024 * 1024 { let cw = x1 - x0; let mut d = Vec::with_capacity(cw * (y1 - y0) * 4); for y in y0..y1 { d.extend_from_slice(&f.d[(y * w + x0) * 4..(y * w + x1) * 4]); } cache.statics.write().unwrap().insert(c.id.clone(), std::sync::Arc::new(StaticCrop { x0, y0, x1, y1, d })); }
    Ok(Some(f))
}
#[test] fn low_memory_changes_only_the_x264_params() {
    let mut p = Project::default(); p.width = 1280; p.height = 720; p.fps = 25.;
    let args = |c: &Command| -> Vec<String> { c.get_args().map(|v| v.to_string_lossy().to_string()).collect() };
    let (d, d2) = (legacy_encoder_command(&p, "o.mp4", 23).unwrap(), legacy_encoder_command_opts(&p, "o.mp4", 23, false).unwrap());
    assert_eq!(args(&d), args(&d2), "default argv must be unchanged");
    let l = args(&legacy_encoder_command_opts(&p, "o.mp4", 23, true).unwrap());
    assert!(l.windows(2).any(|w| w[0] == "-x264-params" && w[1].contains("rc-lookahead=10")) && l.last().unwrap() == "o.mp4");
    let base = args(&d); assert_eq!(l.iter().filter(|a| !base.contains(a)).count(), 2, "only -x264-params and its value are added");
}

#[cfg(test)]
mod prng_overflow_tests {
    use super::*;
    use crate::commands::{dispatch, Session};
    use serde_json::json;

    // count=3 visits i=2; k=2 already overflows the historical product.
    // Keep this runnable in the default debug profile with overflow checks on.
    #[test]
    fn particle_path_overflowing_indices_are_deterministic() {
        let mut ss = Session::new();
        dispatch(&mut ss, "project_set", &json!({"width":32,"height":24,"fps":25})).unwrap();
        let track = dispatch(&mut ss, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
        dispatch(&mut ss, "add_particles", &json!({"track_id":track,"kind":"sparks","start":0,"duration":1,"count":3,"seed":5})).unwrap();
        let a = render_frame(&ss.project, 0.5, &Cache::new()).unwrap();
        let b = render_frame(&ss.project, 0.5, &Cache::new()).unwrap();
        assert_eq!(a.d, b.d);
        assert!(a.d.iter().all(|v| v.is_finite()));
        assert!(a.d.chunks_exact(4).any(|p| p[3] > 0.0));
    }
}
