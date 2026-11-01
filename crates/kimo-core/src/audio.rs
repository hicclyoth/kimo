//! Audio: per-clip decode through ffmpeg filters, keyframed volume, mix to 48k stereo f32, beat detection.
use crate::model::*;
use std::io::Read;
use std::process::{Command, Stdio};

pub const SR: usize = 48000;

pub fn filter_chain(c: &Clip) -> String {
    let a = &c.audio; let mut f: Vec<String> = vec![];
    // speed (atempo supports 0.5..2 per stage)
    let mut sp = c.speed; while sp > 2.0 { f.push("atempo=2.0".into()); sp /= 2.0; } while sp < 0.5 { f.push("atempo=0.5".into()); sp /= 0.5; } if (sp - 1.0).abs() > 1e-6 { f.push(format!("atempo={sp}")); }
    if a.pitch.abs() > 1e-6 { let r = 2f64.powf(a.pitch / 12.0); f.push(format!("asetrate={}", (SR as f64 * r) as i64)); f.push(format!("aresample={SR}")); f.push(format!("atempo={}", 1.0 / r)); }
    if a.highpass > 0.0 { f.push(format!("highpass=f={}", a.highpass)); }
    if a.lowpass > 0.0 { f.push(format!("lowpass=f={}", a.lowpass)); }
    if a.denoise > 0.0 { f.push(format!("afftdn=nr={}", a.denoise)); }
    // Placed BEFORE the hard-knee compressor below: speechnorm is a slow leveler (raise/fall time constants), so it narrows
    // the loudness swing WITHIN this clip first; acompressor (when also on) then only has to catch whatever peaks remain.
    if a.speech_level > 0.0 { let lvl = a.speech_level.clamp(1.0, 4.0); f.push(format!("speechnorm=e={lvl:.3}:c={lvl:.3}:p=0.95:t=0.0:r=0.002:f=0.002")); }
    // V7.9Q: compress_amount scales the ratio (default/unset 1.0 = legacy fixed 3.0, exactly as before this had
    // a knob; 0.0 = no compression, filter skipped entirely).
    if a.compress { let ratio = 1.0 + 2.0 * a.compress_amount.clamp(0.0, 1.0);
        if ratio > 1.001 { f.push(format!("acompressor=threshold=-18dB:ratio={ratio:.3}:attack=10:release=200")); } }
    if a.echo > 0.0 { f.push(format!("aecho=0.8:0.8:{}:0.4", (a.echo * 1000.0) as i64)); }
    if a.reverse { f.push("areverse".into()); }
    for effect in &a.effects { f.push(effect.filter()); }
    f.push(format!("aresample={SR}")); f.push("aformat=sample_fmts=flt:channel_layouts=stereo".into());
    f.join(",")
}

/// Time-remapped clips decode the source at 1x (speed ignored) for this many seconds, then `remap_pcm` resamples it along the curve.
fn remap_decode(c: &Clip) -> Option<(Clip, f64)> { c.time_remap.as_ref()?; let mut one = c.clone(); one.speed = 1.0; Some((one, c.remap_span(0.0, c.duration).1 + 0.05)) }
/// Varispeed playback of 1x source PCM along the clip's time remap (tape style: pitch follows speed, reverse plays backwards); silent where the remap holds.
fn remap_pcm(c: &Clip, src: &[f32]) -> Vec<f32> {
    let n = (c.duration * SR as f64).ceil().max(0.0) as usize; let m = src.len() / 2; let mut out = vec![0f32; n * 2]; let sr = SR as f64;
    let at = |i: isize, ch: usize| -> f64 { if i < 0 || i as usize >= m { 0.0 } else { src[i as usize * 2 + ch] as f64 } };
    for i in 0..n { let lt = i as f64 / sr; let (Some(s), Some(s1)) = (c.remap_source(lt), c.remap_source(lt + 1.0 / sr)) else { break };
        if (s1 - s).abs() * sr < 1e-3 { continue; }
        let x = s * sr; let i0 = x.floor(); let fr = x - i0; let i0 = i0 as isize;
        for ch in 0..2 { out[i * 2 + ch] = (at(i0, ch) * (1.0 - fr) + at(i0 + 1, ch) * fr) as f32; } }
    out
}

pub fn decode_clip(path: &str, c: &Clip) -> Vec<f32> {
    let remap = remap_decode(c); let t = match &remap { Some((_, span)) => format!("{span}"), None => format!("{}", c.duration * c.speed) };
    let mut ch = match Command::new("ffmpeg").args(["-v","error","-ss",&format!("{}", c.in_point),"-t",&t,"-i",path,"-vn","-af",&filter_chain(remap.as_ref().map_or(c, |(o, _)| o)),"-f","f32le","-ac","2","-ar",&SR.to_string(),"-"])
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn() { Ok(c) => c, Err(_) => return vec![] };
    let mut b = Vec::new(); ch.stdout.take().unwrap().read_to_end(&mut b).ok(); let _ = ch.wait();
    { let mut pcm = b.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect::<Vec<f32>>(); crate::editorial::process_pcm(&mut pcm, &c.audio.effects); if remap.is_some() { return remap_pcm(c, &pcm); } pcm }
}

/// Split edit (J/L cut): the clip whose audio is decoded — its span is [start - lead, start + duration + tail] from source
/// in_point - lead*speed, so it stays in sync with the video — and the key offset (effective lead) for the video-local
/// volume/duck keyframes. A straight cut borrows `c` unchanged.
pub(crate) fn audio_view(c: &Clip) -> (std::borrow::Cow<'_, Clip>, f64) {
    let (lead, tail) = (c.audio.lead, c.audio.tail);
    if lead == 0.0 && tail == 0.0 { return (std::borrow::Cow::Borrowed(c), 0.0); }
    let (mut lead, tail) = (if lead.is_finite() { lead } else { 0.0 }, if tail.is_finite() { tail } else { 0.0 });
    lead = lead.min(c.start.max(0.0)); if c.speed.is_finite() && c.speed > 0.0 { lead = lead.min(c.in_point.max(0.0) / c.speed); }
    let mut v = c.clone(); v.start = (c.start - lead).max(0.0); v.in_point = (c.in_point - lead * c.speed).max(0.0);
    v.duration = (c.duration + lead + tail).max(1.0 / SR as f64);
    (std::borrow::Cow::Owned(v), lead)
}

/// RV-28: gentle master-bus speech leveling (ffmpeg `speechnorm`), applied ONCE over the whole continuous mixed
/// buffer -- see Project::master_speech_level's doc for why this runs here and not per clip. Round-trips through
/// a WAV + ffmpeg subprocess, matching how every other audio analysis/transform in this file already shells out
/// (decode_clip, analyze); no new dependency. Returns the input unchanged (with a one-line stderr note swallowed,
/// same silent-degrade style `decode_clip` uses) if ffmpeg can't be run or the round-trip doesn't come back clean --
/// a master bus effect must never be the reason an export fails.
fn level_master(pcm: &[f32], strength: f64) -> Vec<f32> {
    let lvl = strength.clamp(1.0, 4.0);
    let dir = std::env::temp_dir();
    static NEXT_LVL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem = format!("kimo-master-level-{}-{}", std::process::id(), NEXT_LVL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let (inp, outp) = (dir.join(format!("{stem}-in.wav")), dir.join(format!("{stem}-out.wav")));
    let Ok(()) = write_wav(inp.to_str().unwrap_or_default(), pcm) else { return pcm.to_vec() };
    let af = format!("speechnorm=e={lvl:.3}:c={lvl:.3}:p=0.95:t=0.0:r=0.002:f=0.002");
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y", "-i", inp.to_str().unwrap_or_default(), "-af", &af, outp.to_str().unwrap_or_default()])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    let result = if ok {
        Command::new("ffmpeg").args(["-v", "error", "-i", outp.to_str().unwrap_or_default(), "-f", "f32le", "-ac", "2", "-ar", &SR.to_string(), "-"])
            .stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect::<Vec<f32>>())
    } else { None };
    let _ = std::fs::remove_file(&inp); let _ = std::fs::remove_file(&outp);
    match result {
        Some(v) if v.len() == pcm.len() => v,
        _ => pcm.to_vec(),
    }
}
/// `level_master` changes the SHAPE of the loudness over time (what narrows loudness range); it also shifts the
/// overall integrated level somewhat as a side effect of how speechnorm's peak-based expansion/compression works.
/// Wrapping it to hold integrated loudness constant (measure before, measure after, apply one uniform makeup gain
/// to land back where it started) is what keeps this a pure "narrow the swing" effect rather than a second,
/// uncoordinated loudness-target fight against finish_audio's own convergence loop, which measures the SAME mix
/// through this same function on every iteration: letting the leveler also drift the integrated level fed finish_audio
/// a moving target that its gain-matching loop then chased, round after round, undoing most of the leveler's own
/// effect by the time it converged (measured: identical loudness_range on vs off before this wrapper existed).
fn level_neutral(pcm: &[f32], strength: f64) -> Vec<f32> {
    let before = crate::editorial::loudness(pcm);
    let leveled = level_master(pcm, strength);
    match (before, crate::editorial::loudness(&leveled)) {
        (Some(b), Some(a)) if a.is_finite() && b.is_finite() => {
            let gain = 10f32.powf(((b - a) / 20.0) as f32);
            if gain.is_finite() { leveled.iter().map(|v| v * gain).collect() } else { leveled }
        }
        _ => leveled,
    }
}
/// V8.1Q: apply `master_speech_level` scoped to `master_speech_level_scope_end` (see its doc on Project) rather
/// than to the whole buffer. Shared by `mix` and `mix_strict` so finish_audio's own internal loudness
/// measurement (which goes through mix_strict via measure_loudness) and the real export (mix) always see the
/// same thing -- a mismatch there would mean finish_audio converges the mix to a target the export doesn't
/// actually hit. A 50ms crossfade at the scope boundary keeps the handoff from the leveled dialogue to the
/// untouched tail from being an audible seam.
fn apply_master_speech_level(p: &Project, out: Vec<f32>) -> Vec<f32> {
    if p.master_speech_level <= 0.0 { return out; }
    let scope_end = p.master_speech_level_scope_end;
    if scope_end <= 0.0 { return level_neutral(&out, p.master_speech_level); }
    let n_scope = (((scope_end * SR as f64).round() as usize) * 2).min(out.len());
    if n_scope == 0 { return out; }
    let leveled = level_neutral(&out[..n_scope], p.master_speech_level);
    let mut result = leveled;
    if n_scope < out.len() {
        let xfade_frames = ((SR as f64 * 0.05) as usize).min(n_scope / 2);
        let fade_start = n_scope - xfade_frames * 2;
        for k in 0..xfade_frames {
            let t = (k as f32 + 1.0) / xfade_frames.max(1) as f32;
            let idx = fade_start + k * 2;
            result[idx] = result[idx] * (1.0 - t) + out[idx] * t;
            result[idx + 1] = result[idx + 1] * (1.0 - t) + out[idx + 1] * t;
        }
        result.extend_from_slice(&out[n_scope..]);
    }
    result
}
/// Mix every media clip's audio (and audio-only assets) into one interleaved stereo buffer.
pub fn mix(p: &Project) -> Vec<f32> {
    let total = ((p.duration() * SR as f64).ceil() as usize) * 2; let mut out = vec![0f32; total];
    for t in &p.tracks { for c in &t.clips {
        let Source::Media { asset } = &c.source else { continue }; if crate::model::effective_mute(&p.tracks, t) || c.audio.mute { continue; }
        let Some(a) = p.assets.get(asset) else { continue };
        let (v, ko) = audio_view(c); let c = &*v;
        let s = decode_clip(&p.resolve(&a.path), c); let off = ((c.start * SR as f64) as usize) * 2; let n = ((c.duration * SR as f64) as usize * 2).min(s.len());
        for i in (0..n).step_by(2) {
            let lt = (i / 2) as f64 / SR as f64; let kt = if ko == 0.0 { lt } else { lt - ko };
            let mut g = (crate::keyframe::clamp_prop("volume", c.audio.volume.at(kt)) * c.audio.duck_gain.at(kt)) as f32;
            if c.audio.fade_in > 0.0 && lt < c.audio.fade_in { g *= (lt / c.audio.fade_in) as f32; }
            let rem = c.duration - lt; if c.audio.fade_out > 0.0 && rem < c.audio.fade_out { g *= (rem / c.audio.fade_out) as f32; }
            let pv = crate::keyframe::clamp_prop("pan", c.audio.pan.at(kt));
            if off + i + 1 < out.len() { out[off + i] += s[i] * g * (1.0-pv.max(0.0)) as f32; out[off + i + 1] += s[i + 1] * g * (1.0+pv.min(0.0)) as f32; }
        }
    }}
    out = apply_master_speech_level(p, out);
    limit(&mut out, -1.5);
    out
}

struct WavTemp{path:std::path::PathBuf}
impl Drop for WavTemp{fn drop(&mut self){let _=std::fs::remove_file(&self.path);}}
fn wav_size(samples:usize)->Result<u32,String>{let bytes=samples.checked_mul(2).ok_or("WAV size overflow")?;if bytes>u32::MAX as usize-36{return Err("WAV exceeds RIFF32 size limit".into())}Ok(bytes as u32)}
pub fn write_wav(path: &str, pcm: &[f32]) -> Result<(), String> {
    use std::io::Write;
    let data=wav_size(pcm.len())?;for(i,v)in pcm.iter().enumerate(){if i%32768==0{crate::guard::check()}if !v.is_finite(){return Err("WAV requires finite PCM".into())}}
    crate::guard::reserve(65536+44);
    let target=std::path::Path::new(path);let parent=target.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let mut temp=None;for _ in 0..100{let n=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);let q=parent.join(format!(".kimo-wav-{}-{n}.tmp",std::process::id()));match std::fs::OpenOptions::new().write(true).create_new(true).open(&q){Ok(file)=>{temp=Some((WavTemp{path:q},file));break},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e.to_string())}}
    let (temp,mut file)=temp.ok_or("cannot create unique WAV temporary file")?;
    let mut header=Vec::with_capacity(44);header.extend(b"RIFF");header.extend((36+data).to_le_bytes());header.extend(b"WAVEfmt ");header.extend(16u32.to_le_bytes());header.extend(1u16.to_le_bytes());header.extend(2u16.to_le_bytes());header.extend((SR as u32).to_le_bytes());header.extend((SR as u32*4).to_le_bytes());header.extend(4u16.to_le_bytes());header.extend(16u16.to_le_bytes());header.extend(b"data");header.extend(data.to_le_bytes());file.write_all(&header).map_err(|e|e.to_string())?;
    let mut bytes=Vec::with_capacity(65536);for chunk in pcm.chunks(32768){crate::guard::check();bytes.clear();for v in chunk{bytes.extend(((v*32767.) as i16).to_le_bytes());}file.write_all(&bytes).map_err(|e|e.to_string())?;}file.flush().map_err(|e|e.to_string())?;drop(file);crate::guard::check();std::fs::rename(&temp.path,target).map_err(|e|e.to_string())
}

/// Onset detector on a short-window RMS envelope. Returns (beat times in seconds, bpm).
/// Onsets are stamped at the centre of the window where the energy rises; min_gap suppression keeps the strongest hit in each gap.
pub fn analyze(path: &str, min_gap: f64) -> (Vec<f64>, f64) {
    let Ok(o) = Command::new("ffmpeg").args(["-v","error","-i",path,"-vn","-f","f32le","-ac","1","-ar","22050","-"]).output() else { return (vec![], 0.0) };
    let s: Vec<f32> = o.stdout.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect();
    let (hop, win, sr) = (256usize, 512usize, 22050.0f64); let mut env = vec![];
    let mut i = 0; while i + win < s.len() { env.push((s[i..i + win].iter().map(|v| v * v).sum::<f32>() / win as f32).sqrt()); i += hop; }
    let flux: Vec<f32> = env.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    let mut cand: Vec<(f64, f32)> = vec![]; let r = 16usize;
    for i in r..flux.len().saturating_sub(r) {
        let local = flux[i - r..i + r].iter().sum::<f32>() / (2 * r) as f32;
        if flux[i] > local * 1.8 + 0.01 && flux[i] >= flux[i - 1] && flux[i] >= flux[i + 1] { cand.push((((i + 1) * hop + win / 2) as f64 / sr - win as f64 / 2.0 / sr * 0.0, flux[i])); } }
    // strongest-wins suppression
    let mut order: Vec<usize> = (0..cand.len()).collect(); order.sort_by(|&x, &y| cand[y].1.partial_cmp(&cand[x].1).unwrap());
    let mut keep: Vec<f64> = vec![]; for &ix in &order { let t = cand[ix].0; if keep.iter().all(|&k| (k - t).abs() >= min_gap) { keep.push(t); } } keep.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // tempo from autocorrelation of the onset strength (80..180 BPM)
    let fps = sr / hop as f64; let n = flux.len(); let mut best = (0.0f64, 0usize);
    let (lo, hi) = ((fps * 60.0 / 180.0) as usize, (fps * 60.0 / 80.0) as usize);
    let mut ac = vec![0f64; hi + 2]; for lag in lo.max(1)..=hi { if lag >= n { break; } let mut sum = 0.0f64; for j in 0..n - lag { sum += flux[j] as f64 * flux[j + lag] as f64; } ac[lag] = sum / (n - lag) as f64; if ac[lag] > best.0 { best = (ac[lag], lag); } }
    let bpm = if best.1 > 1 && best.1 < hi { let l = best.1; let (a0, a1, a2) = (ac[l - 1], ac[l], ac[l + 1]); let d = a0 - 2.0 * a1 + a2; let off = if d.abs() > 1e-12 { 0.5 * (a0 - a2) / d } else { 0.0 }; 60.0 * fps / (l as f64 + off) } else { 0.0 };
    (keep, bpm)
}
pub fn detect_beats(path: &str, min_gap: f64) -> Vec<f64> { analyze(path, min_gap).0 }

/// Lookahead peak limiter (5 ms lookahead, 80 ms release) with a ceiling in dBFS; keeps AAC/true-peak overshoot below about -1 dBTP at the default.
pub fn limit(pcm: &mut [f32], ceiling_db: f64) {
    let ceil = 10f32.powf(ceiling_db as f32 / 20.0); let la = (SR as f32 * 0.005) as usize; let rel = (-1.0 / (SR as f32 * 0.08)).exp(); let n = pcm.len() / 2;
    let mut need = vec![1f32; n]; for i in 0..n { let pk = pcm[i * 2].abs().max(pcm[i * 2 + 1].abs()); if pk > ceil { need[i] = ceil / pk; } }
    // lookahead min over [i, i+la], then smooth release
    let mut g = vec![1f32; n]; let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in (0..n).rev() { while let Some(&b) = dq.back() { if need[b] >= need[i] { dq.pop_back(); } else { break; } } dq.push_back(i); while let Some(&f) = dq.front() { if f > i + la { dq.pop_front(); } else { break; } } g[i] = need[*dq.front().unwrap()]; }
    let att = 1.0 - (-3.0 / la.max(1) as f32).exp(); let mut cur = 1f32;
    for i in 0..n { let t = g[i]; if t < cur { cur += (t - cur) * att; } else { cur = t + (cur - t) * rel; } let gain = cur.min(1.0).max(0.0); pcm[i * 2] *= gain; pcm[i * 2 + 1] *= gain; }
    for v in pcm.iter_mut() { *v = v.clamp(-ceil, ceil); }
}
#[cfg(test)]mod wav_tests{use super::*;
fn prior(pcm:&[f32])->Vec<u8>{let data=(pcm.len()*2) as u32;let mut b=Vec::new();b.extend(b"RIFF");b.extend((36+data).to_le_bytes());b.extend(b"WAVEfmt ");b.extend(16u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(2u16.to_le_bytes());b.extend((SR as u32).to_le_bytes());b.extend((SR as u32*4).to_le_bytes());b.extend(4u16.to_le_bytes());b.extend(16u16.to_le_bytes());b.extend(b"data");b.extend(data.to_le_bytes());for v in pcm{b.extend(((v*32767.) as i16).to_le_bytes());}b}
#[test]fn wav_byte_identity_empty_single_and_chunks(){let path=std::env::temp_dir().join(format!("kimo-wav-proof-{}.wav",std::process::id()));for pcm in [vec![],vec![0.25],(0..70001).map(|i|i as f32/17000.-2.).collect()]{write_wav(path.to_str().unwrap(),&pcm).unwrap();assert_eq!(std::fs::read(&path).unwrap(),prior(&pcm));}std::fs::remove_file(path).unwrap();}
#[test]fn invalid_wav_leaves_target_untouched(){let path=std::env::temp_dir().join(format!("kimo-wav-invalid-{}.wav",std::process::id()));std::fs::write(&path,b"keep").unwrap();for v in [f32::NAN,f32::INFINITY]{assert!(write_wav(path.to_str().unwrap(),&[v]).is_err());assert_eq!(std::fs::read(&path).unwrap(),b"keep");}std::fs::remove_file(&path).unwrap();assert!(write_wav(path.to_str().unwrap(),&[f32::NAN]).is_err());assert!(!path.exists());assert!(wav_size(usize::MAX).is_err());assert!(wav_size((u32::MAX as usize-36)/2+1).is_err());}
}

/// Strict audio errors preserve track absence separately from failed decode.
#[derive(Debug,PartialEq)]pub enum AudioError{NoAudioTrack,DecodeFailed(String),InvalidInput(String),ResourceLimit(String)}
impl std::fmt::Display for AudioError{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self{Self::NoAudioTrack=>write!(f,"no audio track"),Self::DecodeFailed(e)=>write!(f,"audio decode failed: {e}"),Self::InvalidInput(e)=>write!(f,"invalid audio input: {e}"),Self::ResourceLimit(e)=>write!(f,"audio resource limit: {e}")}}}
fn strict_audio_cap()->usize{(crate::guard::limits().mem_bytes/16).min(64*1024*1024)}
fn strict_samples(seconds:f64)->Result<usize,AudioError>{if !seconds.is_finite()||seconds<0.{return Err(AudioError::InvalidInput("finite nonnegative duration required".into()))}let samples=(seconds*SR as f64).ceil()*2.;if !samples.is_finite()||samples>usize::MAX as f64{return Err(AudioError::ResourceLimit("sample count overflow".into()))}Ok(samples as usize)}
pub(crate) fn source_info(path:&str)->Result<serde_json::Value,AudioError>{let raw=crate::color::process::capture(Command::new("ffprobe").args(["-v","error","-select_streams","a:0","-show_entries","stream=sample_rate,channels","-of","json",path]),1024*1024,std::time::Duration::from_secs(30)).map_err(AudioError::DecodeFailed)?;let v:serde_json::Value=serde_json::from_slice(&raw).map_err(|e|AudioError::DecodeFailed(e.to_string()))?;let stream=v["streams"].as_array().and_then(|v|v.first()).ok_or(AudioError::NoAudioTrack)?;Ok(serde_json::json!({"native_rate":stream["sample_rate"].as_str().and_then(|v|v.parse::<u32>().ok()),"native_channels":stream["channels"]}))}
pub(crate) fn strict_decode(path:&str,c:&Clip)->Result<Vec<f32>,AudioError>{
 if !c.speed.is_finite()||c.speed<=0.||!c.in_point.is_finite()||c.in_point<0.||!c.duration.is_finite()||c.duration<0.||[c.audio.pitch,c.audio.highpass,c.audio.lowpass,c.audio.denoise,c.audio.echo].iter().any(|v|!v.is_finite()){return Err(AudioError::InvalidInput("finite clip/audio controls required".into()))}
 let probe=crate::color::process::capture(Command::new("ffprobe").args(["-v","error","-select_streams","a:0","-show_entries","stream=index","-of","json",path]),1024*1024,std::time::Duration::from_secs(30)).map_err(AudioError::DecodeFailed)?;
 let header:serde_json::Value=serde_json::from_slice(&probe).map_err(|e|AudioError::DecodeFailed(e.to_string()))?;let streams=header["streams"].as_array().ok_or_else(||AudioError::DecodeFailed("invalid stream probe".into()))?;if streams.is_empty(){return Err(AudioError::NoAudioTrack)}
 let remap=remap_decode(c);if remap.as_ref().is_some_and(|(_,s)|!s.is_finite()){return Err(AudioError::InvalidInput("finite time remap required".into()))}
 // ffmpeg's `-ss` seek before/after `-i` lands within a handful of samples of the
 // mathematically exact point for a nonzero seek target on some containers/codecs
 // (measured: a fixed small sample offset even with accurate_seek on, growing the
 // longer the seek). Seeking is only ever exact at the true start of the stream, so
 // we always decode from source time 0 and drop the unwanted lead-in ourselves;
 // this is what keeps ripple-deleted pieces of the same source sample-aligned with
 // each other and with an unedited decode of the same material.
 let span=remap.as_ref().map_or(c.duration*c.speed,|(_,s)|*s);
 let total=c.in_point+span;
 let samples=strict_samples(remap.as_ref().map_or(c.duration,|(_,s)|c.duration.max(*s))+c.in_point+c.audio.echo.max(0.)+2.)?;let bytes=samples.checked_mul(4).ok_or_else(||AudioError::ResourceLimit("decode bytes overflow".into()))?;if bytes>strict_audio_cap(){return Err(AudioError::ResourceLimit(format!("audio decode exceeds{}byte cap",strict_audio_cap())))}crate::guard::reserve(bytes.checked_mul(2).ok_or_else(||AudioError::ResourceLimit("audio allocation overflow".into()))?);
 let raw=crate::color::process::decode_capture(Command::new("ffmpeg").args(["-v","error","-ss","0","-t",&total.to_string(),"-i",path,"-vn","-af",&filter_chain(remap.as_ref().map_or(c,|(o,_)|o)),"-f","f32le","-ac","2","-ar",&SR.to_string(),"-"]),bytes,crate::color::process::encode_deadline((total*25.).ceil() as usize)).map_err(AudioError::DecodeFailed)?;
 if raw.len()%8!=0{return Err(AudioError::DecodeFailed("partial stereo float sample".into()))}let mut out=Vec::with_capacity(raw.len()/4);for(i,b)in raw.chunks_exact(4).enumerate(){if i%32768==0{crate::guard::check()}let v=f32::from_le_bytes(b.try_into().unwrap());if !v.is_finite(){return Err(AudioError::DecodeFailed("nonfinite decoded sample".into()))}out.push(v)}
 let skip=((c.in_point*SR as f64).round() as usize).saturating_mul(2);
 if skip>=out.len(){out.clear()}else if skip>0{out.drain(0..skip);}
 crate::editorial::process_pcm_checked(&mut out,&c.audio.effects).map_err(AudioError::InvalidInput)?;if remap.is_some(){return Ok(remap_pcm(c,&out))}Ok(out)
}
/// New strict path for high-bit export only. The legacy mix/decode API is unchanged.
pub fn mix_strict(p:&Project)->Result<Vec<f32>,AudioError>{
 let total=strict_samples(p.duration())?;let bytes=total.checked_mul(4).ok_or_else(||AudioError::ResourceLimit("mix bytes overflow".into()))?;if bytes>strict_audio_cap(){return Err(AudioError::ResourceLimit(format!("timeline PCM exceeds{}byte cap",strict_audio_cap())))}crate::guard::reserve(bytes);let mut out=vec![0f32;total];
 for track in &p.tracks{for c in &track.clips{crate::guard::check();let Source::Media{asset}=&c.source else{continue};if crate::model::effective_mute(&p.tracks, track) || c.audio.mute{continue}if !c.start.is_finite()||c.start<0.||!c.audio.fade_in.is_finite()||!c.audio.fade_out.is_finite(){return Err(AudioError::InvalidInput("invalid clip start/fades".into()))}let a=p.assets.get(asset).ok_or_else(||AudioError::InvalidInput("missing audio asset".into()))?;let(v,ko)=audio_view(c);let c=&*v;
 let s=match strict_decode(&p.resolve(&a.path),c){Ok(v)=>v,Err(AudioError::NoAudioTrack)=>continue,Err(e)=>return Err(e)};let off=((c.start*SR as f64) as usize).checked_mul(2).ok_or_else(||AudioError::ResourceLimit("audio offset overflow".into()))?;let n=((c.duration*SR as f64) as usize).checked_mul(2).ok_or_else(||AudioError::ResourceLimit("clip samples overflow".into()))?.min(s.len());
 for i in (0..n).step_by(2){if i%65536==0{crate::guard::check()}let lt=(i/2) as f64/SR as f64;let kt=if ko==0.{lt}else{lt-ko};let mut g=(crate::keyframe::clamp_prop("volume",c.audio.volume.at(kt))*c.audio.duck_gain.at(kt)) as f32;if !g.is_finite(){return Err(AudioError::InvalidInput("nonfinite volume".into()))}if c.audio.fade_in>0.&&lt<c.audio.fade_in{g*=(lt/c.audio.fade_in) as f32}let rem=c.duration-lt;if c.audio.fade_out>0.&&rem<c.audio.fade_out{g*=(rem/c.audio.fade_out) as f32}let pv=crate::keyframe::clamp_prop("pan",c.audio.pan.at(kt));if off.checked_add(i+1).filter(|j|*j<out.len()).is_some(){for j in 0..2{let v=out[off+i+j]+s[i+j]*g*if j==0{(1.-pv.max(0.)) as f32}else{(1.+pv.min(0.)) as f32};if !v.is_finite(){return Err(AudioError::InvalidInput("audio mix overflow".into()))}out[off+i+j]=v}}}
 }}
 out=apply_master_speech_level(p,out);
 strict_limit(&mut out,-1.5);Ok(out)
}

fn strict_limit(pcm: &mut [f32], ceiling_db: f64) {
    let ceil = 10f32.powf(ceiling_db as f32 / 20.0); let la = (SR as f32 * 0.005) as usize; let rel = (-1.0 / (SR as f32 * 0.08)).exp(); let n = pcm.len() / 2;
    crate::guard::reserve(n.checked_mul(8).and_then(|v|v.checked_add((la+2)*8)).expect("limiter allocation overflow")); let mut need = vec![1f32; n]; for i in 0..n {if i%32768==0{crate::guard::check()} let pk = pcm[i * 2].abs().max(pcm[i * 2 + 1].abs()); if pk > ceil { need[i] = ceil / pk; } }
    // lookahead min over [i, i+la], then smooth release
    let mut g = vec![1f32; n]; let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::with_capacity(la+2);
    for i in (0..n).rev() {if i%32768==0{crate::guard::check()} while let Some(&b) = dq.back() { if need[b] >= need[i] { dq.pop_back(); } else { break; } } dq.push_back(i); while let Some(&f) = dq.front() { if f > i + la { dq.pop_front(); } else { break; } } g[i] = need[*dq.front().unwrap()]; }
    let att = 1.0 - (-3.0 / la.max(1) as f32).exp(); let mut cur = 1f32;
    for i in 0..n {if i%32768==0{crate::guard::check()} let t = g[i]; if t < cur { cur += (t - cur) * att; } else { cur = t + (cur - t) * rel; } let gain = cur.min(1.0).max(0.0); pcm[i * 2] *= gain; pcm[i * 2 + 1] *= gain; }
    for(i,v) in pcm.iter_mut().enumerate() {if i%65536==0{crate::guard::check()} *v = v.clamp(-ceil, ceil); }
}#[cfg(test)]mod strict_tests{use super::*;
fn session(path:&std::path::Path)->crate::commands::Session{use crate::commands::{dispatch,Session};use serde_json::json;let mut s=Session::new();let a=dispatch(&mut s,"import_media",&json!({"path":path})).unwrap()["asset_id"].clone();let tr=dispatch(&mut s,"add_track",&json!({})).unwrap()["track_id"].clone();dispatch(&mut s,"add_clip",&json!({"track_id":tr,"source_type":"media","asset_id":a,"start":0.00001,"duration":0.2})).unwrap();s}
#[test]fn strict_mix_matches_legacy_pcm_and_limiter(){let path=std::env::temp_dir().join(format!("kimo-strict-audio-{}.wav",std::process::id()));assert!(Command::new("ffmpeg").args(["-v","error","-y","-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=0.3","-ac","2",path.to_str().unwrap()]).status().unwrap().success());let s=session(&path);assert_eq!(mix_strict(&s.project).unwrap(),mix(&s.project));let mut a:Vec<f32>=(0..20000).map(|i|(i as f32/30.).sin()*2.).collect();let mut b=a.clone();limit(&mut a,-1.5);strict_limit(&mut b,-1.5);assert_eq!(a,b);std::fs::remove_file(path).unwrap();}
#[test]fn absent_audio_and_decode_failure_are_typed(){let path=std::env::temp_dir().join(format!("kimo-strict-video-{}.mp4",std::process::id()));assert!(Command::new("ffmpeg").args(["-v","error","-y","-f","lavfi","-i","color=s=16x16:r=25:d=0.2","-c:v","libx264",path.to_str().unwrap()]).status().unwrap().success());let s=session(&path);let c=&s.project.tracks[0].clips[0];assert_eq!(strict_decode(path.to_str().unwrap(),c).unwrap_err(),AudioError::NoAudioTrack);assert!(mix_strict(&s.project).unwrap().iter().all(|v|*v==0.));std::fs::remove_file(&path).unwrap();assert!(matches!(strict_decode(path.to_str().unwrap(),c),Err(AudioError::DecodeFailed(_))));let mut c=(**c).clone();c.speed=0.;assert!(matches!(strict_decode("unused",&c),Err(AudioError::InvalidInput(_))));}
#[test]fn oversized_timeline_rejected_before_allocation(){assert!(strict_audio_cap()<=64*1024*1024);assert!(strict_audio_cap()<=crate::guard::limits().mem_bytes/16);assert!(strict_samples(f64::INFINITY).is_err());assert!(strict_samples(-1.).is_err());}
}
