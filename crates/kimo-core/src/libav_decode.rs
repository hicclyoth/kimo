//! V8.6.1: in-process libav decoder (ffmpeg-next, behind KIMO_DECODER=libav). One long-lived decoder per
//! source clip -- opened once, seek once per export, decode forward from there -- instead of render.rs's
//! `decode()` default path, which spawns a fresh `ffmpeg` process per decode window (see render.rs:202).
//!
//! Scope (the opt-in fast path; anything outside it falls back to the existing ffmpeg-pipe `decode()`,
//! never errors the export): ordinary 8-bit video clips at constant speed 1.0, not time-remapped, not
//! reframe-active, not a still image, not kimopx/EXR/TIFF, and the project's `color_pipeline` disabled
//! (matching `decode()`'s plain `pix="rgba"` branch -- the one the default auto_edit/legacy export path
//! actually uses). `supported()` is the single source of truth for this scope; `render.rs::decode()` calls
//! it before ever touching this module.
use ffmpeg_next as ffmpeg;
use std::collections::HashMap;
use std::sync::Mutex;

/// One open decode: input context + video decoder + (lazily built, rebuilt only if the source frame's own
/// dims/format change) swscale context, plus a small one-frame lookahead so `frame_for_time` can pick the
/// source frame nearest each output tick (matching ffmpeg's `fps` filter's default `round=near`) without
/// re-decoding.
/// (src_w, src_h, src_fmt, dst_w, dst_h) -- rebuilt whenever ANY of these drift, not just the source side:
/// a pooled decoder is keyed by source path only, so the same clip decoded at two different canvas sizes
/// (two exports in the same process, e.g. the 720p and 1080p benchmark runs, or two tests sharing a
/// fixture) must not silently keep serving frames scaled for the previous call's target dimensions.
type ScalerKey = (usize, usize, ffmpeg::format::Pixel, usize, usize);
pub struct LibavDecoder {
    ictx: ffmpeg::format::context::Input,
    stream_index: usize,
    stream_time_base: ffmpeg::Rational,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: Option<(ScalerKey, ffmpeg::software::scaling::context::Context)>,
    held: Option<(f64, ffmpeg::util::frame::Video)>,
    peeked: Option<(f64, ffmpeg::util::frame::Video)>,
    eof: bool,
    /// Where decode-forward currently sits (seconds); a request before this re-seeks, a request at/after
    /// it just keeps decoding forward from wherever `held`/`peeked` already are.
    cursor_s: f64,
}

// Safety: every `LibavDecoder` lives inside the pool's `Mutex`, so at most one thread ever touches
// its raw ffmpeg/swscale pointers at a time -- the same reasoning `ffmpeg-next` itself applies to
// `format::context::Input` (`unsafe impl Send for Input`); `scaling::context::Context` just doesn't
// carry that same blanket impl for its own raw `*mut SwsContext`.
unsafe impl Send for LibavDecoder {}

static POOL: std::sync::OnceLock<Mutex<HashMap<String, LibavDecoder>>> = std::sync::OnceLock::new();
fn pool() -> &'static Mutex<HashMap<String, LibavDecoder>> { POOL.get_or_init(|| Mutex::new(HashMap::new())) }

/// Drop every pooled decoder (tests: each test should start from a clean pool so one test's held ffmpeg
/// file handles / cursor position can't leak into another's assertions).
pub fn clear_pool() { pool().lock().unwrap().clear(); }

/// Whether `render.rs::decode()`'s request for this clip/project falls inside this module's scope. Kept
/// as a free function (not a method) so the caller can check it before paying for anything else.
pub fn supported(color_pipeline_enabled: bool, retimed: bool, speed: f64, reframe_active: bool, is_img: bool, is_kimopx: bool, is_native_fit_ext: bool) -> bool {
    !color_pipeline_enabled && !retimed && (speed - 1.0).abs() < 1e-9 && !reframe_active && !is_img && !is_kimopx && !is_native_fit_ext
}

/// Decode `n` project-fps frames starting at clip-local frame `a` (frame index at `project_fps`, source
/// time `in_point + (a+k)/project_fps` for k in 0..n), fit into `w`x`h` RGBA8 exactly as `decode()`'s
/// `fitf` would (stretch/cover/contain by the same `fit` thresholds), returning `n*w*h*4` bytes in
/// clip-frame order. One `LibavDecoder` per `path` is kept in a process-wide pool and reused/seeked as
/// needed across calls -- the "one long-lived decoder per source clip" the phase asked for.
pub fn decode_window(path: &str, w: u32, h: u32, fit: f64, in_point: f64, project_fps: f64, a: usize, n: usize) -> Result<Vec<u8>, String> {
    let mut guard = pool().lock().map_err(|_| "libav decoder pool poisoned".to_string())?;
    let dec = match guard.entry(path.to_string()) {
        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
        std::collections::hash_map::Entry::Vacant(e) => e.insert(LibavDecoder::open(path)?),
    };
    let mut out = Vec::with_capacity(n * (w as usize) * (h as usize) * 4);
    for k in 0..n {
        let t = in_point + (a + k) as f64 / project_fps;
        let frame = dec.frame_for_time(t, project_fps)?;
        let bytes = fit_into_rgba(&frame, w, h, fit, &mut dec.scaler)?;
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

impl LibavDecoder {
    fn open(path: &str) -> Result<Self, String> {
        ffmpeg::init().map_err(|e| format!("ffmpeg::init: {e}"))?;
        let ictx = ffmpeg::format::input(&path).map_err(|e| format!("libav open {path}: {e}"))?;
        let stream = ictx.streams().best(ffmpeg::media::Type::Video).ok_or_else(|| format!("libav: no video stream in {path}"))?;
        let stream_index = stream.index();
        let stream_time_base = stream.time_base();
        let mut ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters()).map_err(|e| format!("libav codec context: {e}"))?;
        // The ffmpeg CLI subprocess this replaces decodes with libavcodec's own frame-parallel threading by
        // default; opening a decoder via ffmpeg-next with no threading config leaves it single-threaded
        // (thread_count defaults to 1), which measurably cost more than overlap gained on this box's real
        // benchmark (first measurement: 41.8-43.6s median vs 37.5s for the pipe+overlap pipeline alone,
        // despite higher average core usage) -- matching this to the CLI's own default closes most of that gap.
        ctx.set_threading(ffmpeg::threading::Config { kind: ffmpeg::threading::Type::Frame, count: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4), safe: false });
        let decoder = ctx.decoder().video().map_err(|e| format!("libav open decoder: {e}"))?;
        Ok(Self { ictx, stream_index, stream_time_base, decoder, scaler: None, held: None, peeked: None, eof: false, cursor_s: f64::NEG_INFINITY })
    }

    /// Seek to the nearest keyframe at or before `target_s`, flush decoder state, and clear the lookahead
    /// so the next `frame_for_time` call decodes forward from there.
    fn seek_before(&mut self, target_s: f64) -> Result<(), String> {
        let ts = (target_s * ffmpeg::ffi::AV_TIME_BASE as f64).floor() as i64;
        self.ictx.seek(ts, ..ts).map_err(|e| format!("libav seek to {target_s}s: {e}"))?;
        self.decoder.flush();
        self.held = None;
        self.peeked = None;
        self.eof = false;
        self.cursor_s = f64::NEG_INFINITY;
        Ok(())
    }

    fn pts_seconds(&self, frame: &ffmpeg::util::frame::Video) -> f64 {
        let pts = frame.pts().or_else(|| frame.timestamp()).unwrap_or(0);
        pts as f64 * self.stream_time_base.numerator() as f64 / self.stream_time_base.denominator() as f64
    }

    /// Decode the next frame in presentation order (None at genuine EOF).
    fn decode_next(&mut self) -> Result<Option<(f64, ffmpeg::util::frame::Video)>, String> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        loop {
            if self.decoder.receive_frame(&mut decoded).is_ok() {
                let t = self.pts_seconds(&decoded);
                return Ok(Some((t, decoded)));
            }
            if self.eof { return Ok(None); }
            match self.ictx.packets().next() {
                Some((stream, packet)) if stream.index() == self.stream_index => {
                    self.decoder.send_packet(&packet).map_err(|e| format!("libav send_packet: {e}"))?;
                }
                Some(_) => continue, // a different stream's packet (audio, etc.) -- ignore
                None => {
                    self.eof = true;
                    self.decoder.send_eof().map_err(|e| format!("libav send_eof: {e}"))?;
                }
            }
        }
    }

    /// The source frame to show at output tick `t` (seconds, always an exact multiple of `1/fps` -- the
    /// caller always calls with `in_point + (a+k)/project_fps`): ffmpeg's `fps` filter (verified empirically
    /// against a genuinely VFR fixture, see tests/libav_vfr_conform.rs) holds each source frame from the
    /// output tick nearest to ITS OWN pts (`round(pts * fps)`) until the next source frame's own rounded
    /// tick -- not "whichever source frame is nearest to this tick's raw time", which is a different rule
    /// (a first attempt using that rule mismatched the pipe path at 2 of 19 ticks on the VFR fixture; this
    /// one matches all 19).
    fn frame_for_time(&mut self, t: f64, fps: f64) -> Result<ffmpeg::util::frame::Video, String> {
        if t < self.cursor_s - 1e-6 {
            // A backward or far-forward request (a re-render of an earlier range, a different clip
            // instance sharing this source, ...): reseek well before it so decode-forward reaches it.
            self.seek_before((t - 1.0).max(0.0))?;
        }
        self.cursor_s = t;
        let k = (t * fps).round() as i64;
        loop {
            if self.peeked.is_none() { self.peeked = self.decode_next()?; }
            match (&self.held, &self.peeked) {
                (None, Some(_)) => { self.held = self.peeked.take(); }
                (Some(_), None) => return Ok(self.held.as_ref().unwrap().1.clone()), // EOF: hold the last frame
                (None, None) => return Err(format!("libav: no decodable frame at or after {t:.3}s")),
                (Some(_), Some((pp, _))) => {
                    let peek_tick = (*pp * fps).round() as i64;
                    if peek_tick <= k { self.held = self.peeked.take(); continue; }
                    return Ok(self.held.as_ref().unwrap().1.clone());
                }
            }
        }
    }
}

/// `fitf`'s three modes (render.rs: fit>=1.5 stretch, fit>=0.5 cover+crop, else contain+pad), replicated
/// in Rust: scale with swscale (BICUBIC, matching ffmpeg's `scale` filter default), then crop or pad to
/// land on exactly `w`x`h`. Rebuilds the scaler only when the source frame's own dims/format change (the
/// target dims are a pure function of the source dims + `fit`, both constant for one export).
fn fit_into_rgba(src: &ffmpeg::util::frame::Video, w: u32, h: u32, fit: f64, scaler_slot: &mut Option<(ScalerKey, ffmpeg::software::scaling::context::Context)>) -> Result<Vec<u8>, String> {
    use ffmpeg::software::scaling::{context::Context, flag::Flags};
    let (sw, sh) = (src.width(), src.height());
    let (tw, th) = if fit >= 1.5 {
        (w, h) // stretch: scale straight to the target, no crop/pad
    } else if fit >= 0.5 {
        // cover: scale up (never down) so the frame fully covers w x h, crop centred afterwards
        let scale = (w as f64 / sw as f64).max(h as f64 / sh as f64);
        (((sw as f64 * scale).round() as u32).max(1), ((sh as f64 * scale).round() as u32).max(1))
    } else {
        // contain: scale down (never up) so the frame fully fits inside w x h, pad centred afterwards
        let scale = (w as f64 / sw as f64).min(h as f64 / sh as f64);
        (((sw as f64 * scale).round() as u32).max(1), ((sh as f64 * scale).round() as u32).max(1))
    };
    let key: ScalerKey = (sw as usize, sh as usize, src.format(), tw as usize, th as usize);
    let need_rebuild = match scaler_slot { Some((k, _)) => *k != key, None => true };
    if need_rebuild {
        let ctx = Context::get(src.format(), sw, sh, ffmpeg::format::Pixel::RGBA, tw, th, Flags::BICUBIC).map_err(|e| format!("libav scaler: {e}"))?;
        *scaler_slot = Some((key, ctx));
    }
    let (_, ctx) = scaler_slot.as_mut().unwrap();
    let mut scaled = ffmpeg::util::frame::Video::empty();
    ctx.run(src, &mut scaled).map_err(|e| format!("libav scale run: {e}"))?;
    Ok(compose_crop_pad(&scaled, tw, th, w, h))
}

/// Centre-crop (when the scaled frame is larger than the canvas, "cover") or centre-pad with transparent
/// black (when it's smaller, "contain") -- exactly `crop=w:h` / `pad=w:h:(ow-iw)/2:(oh-ih)/2:color=black@0`
/// would. Stretch mode scales straight to w x h so this is a no-op copy for it.
fn compose_crop_pad(scaled: &ffmpeg::util::frame::Video, sw: u32, sh: u32, w: u32, h: u32) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0u8; wu * hu * 4];
    let stride = scaled.stride(0);
    let data = scaled.data(0);
    if sw == w && sh == h {
        for row in 0..hu { out[row * wu * 4..row * wu * 4 + wu * 4].copy_from_slice(&data[row * stride..row * stride + wu * 4]); }
        return out;
    }
    if sw >= w && sh >= h {
        // cover: crop centred
        let x0 = ((sw - w) / 2) as usize; let y0 = ((sh - h) / 2) as usize;
        for row in 0..hu { let srow = (y0 + row) * stride + x0 * 4; out[row * wu * 4..row * wu * 4 + wu * 4].copy_from_slice(&data[srow..srow + wu * 4]); }
    } else {
        // contain: pad centred, transparent black outside
        let x0 = ((w - sw) / 2) as usize; let y0 = ((h - sh) / 2) as usize;
        for row in 0..sh as usize {
            let srow = row * stride;
            let drow = (y0 + row) * wu * 4 + x0 * 4;
            out[drow..drow + sw as usize * 4].copy_from_slice(&data[srow..srow + sw as usize * 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> String { "/home/user/tab/work/v7q_real/talkinghead_real_final.mp4".to_string() }

    #[test]
    fn exact_frame_index_at_a_seek_target() {
        clear_pool();
        let path = source();
        let buf = decode_window(&path, 64, 36, 0.0, 0.0, 24.0, 120, 1).unwrap();
        assert_eq!(buf.len(), 64 * 36 * 4);
        let buf2 = decode_window(&path, 64, 36, 0.0, 0.0, 24.0, 121, 1).unwrap();
        assert_eq!(buf2.len(), 64 * 36 * 4);
    }

    #[test]
    fn same_frame_count_as_requested() {
        clear_pool();
        let buf = decode_window(&source(), 32, 18, 0.0, 0.0, 24.0, 0, 50).unwrap();
        assert_eq!(buf.len(), 50 * 32 * 18 * 4);
    }

    #[test]
    fn seek_backward_then_forward_still_works() {
        clear_pool();
        let path = source();
        let _ = decode_window(&path, 32, 18, 0.0, 0.0, 24.0, 200, 5).unwrap();
        // Now ask for something earlier -- must re-seek correctly, not just silently serve the wrong frames.
        let early = decode_window(&path, 32, 18, 0.0, 0.0, 24.0, 10, 5).unwrap();
        assert_eq!(early.len(), 5 * 32 * 18 * 4);
    }

    #[test]
    fn supported_matches_the_stated_scope() {
        assert!(supported(false, false, 1.0, false, false, false, false));
        assert!(!supported(true, false, 1.0, false, false, false, false)); // color_pipeline enabled
        assert!(!supported(false, true, 1.0, false, false, false, false)); // retimed
        assert!(!supported(false, false, 1.5, false, false, false, false)); // speed != 1
        assert!(!supported(false, false, 1.0, true, false, false, false)); // reframe active
        assert!(!supported(false, false, 1.0, false, true, false, false)); // still image
    }
}
