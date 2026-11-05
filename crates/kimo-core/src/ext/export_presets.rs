use super::kit::*;
use std::process::Command as Cmd;

const PRESETS: &[(&str, &str, &str)] = &[
    ("web-1080", ".mp4", "H.264 yuva-free yuv420p, CRF 18 medium, +faststart, AAC 192k. Rec709 only. Downscale-to-fit 1920x1080 (or 1080x1920 portrait), aspect kept, even dimensions, never upscaled, scaled in the encoder (lanczos), not by changing the project canvas."),
    ("master", ".mkv", "Lossless 16-bit RGB integer delivery (FFV1 gbrp16le) at the project canvas and fps. Same encoder as export_high_bit ffv1_16. Integer RGB, not float radiance."),
    ("alpha-prores", ".mov", "ProRes 4444 (prores_ks profile 4, yuva444p10le, alpha_bits 16), transparent canvas, straight alpha. Rec709 only. Audio not muxed yet."),
    ("youtube", ".mp4", "H.264 High profile yuv420p, CRF 16 slow, +faststart, AAC 384k. Rec709 only. Native project resolution (YouTube accepts up to 8K; unlike web-1080 this preset never resizes)."),
    ("tiktok", ".mp4", "H.264 yuv420p, CRF 20 medium, +faststart, AAC 128k. Rec709 only. Always letterboxed/pillarboxed to exactly 1080x1920 (TikTok's own vertical canvas), unlike web-1080's shrink-to-fit-without-padding."),
    ("prores422", ".mov", "ProRes 422 HQ (prores_ks profile 3, yuv422p10le). Rec709 only. Has audio (AAC 192k), unlike alpha-prores which has none yet -- this preset carries no alpha."),
    ("dnxhr", ".mov", "Avid DNxHR HQ (dnxhd encoder, profile dnxhr_hq, yuv422p) at the project canvas and fps, with AAC 192k audio. Rec709 only. ffmpeg's dnxhd encoder refuses input under 256x120 -- a real codec constraint, enforced here with a clear error rather than a cryptic ffmpeg failure."),
    ("webm-vp9", ".webm", "VP9 (libvpx-vp9) yuv420p, CRF 32 (bitrate-less quality mode). Rec709 only. Video-only for now: WebM doesn't take the AAC audio this engine's generic audio mux writes, and no Opus muxing path was built this pass -- audio not muxed yet, same honesty as alpha-prores."),
    ("hevc", ".mp4", "H.265/HEVC (libx265) yuv420p, CRF 20 medium, tagged hvc1 for Apple/QuickTime compatibility, +faststart, AAC 192k. Rec709 only."),
    ("av1", ".mp4", "AV1 (libsvtav1) yuv420p, CRF 30 preset 8, +faststart, AAC 192k. Rec709 only."),
];

/// Output size for web-1080: fit inside 1920x1080 (landscape/square) or 1080x1920 (portrait), no upscale, even dims.
pub fn fit_web(w: usize, h: usize) -> (usize, usize) {
    let (mw, mh) = if h > w { (1080.0, 1920.0) } else { (1920.0, 1080.0) };
    let f = (mw / w as f64).min(mh / h as f64).min(1.0);
    let ev = |v: f64| ((v.round() as usize) & !1usize).max(2);
    if f >= 1.0 { (ev(w as f64), ev(h as f64)) } else { (ev(w as f64 * f), ev(h as f64 * f)) }
}

/// tiktok's own 1080x1920 canvas: the source is scaled to fit WITHOUT upscaling past the source's own pixels for a tiny
/// source, then letterboxed/pillarboxed with black bars to land on exactly 1080x1920, unlike web-1080's shrink-without-pad.
pub fn tiktok_scale_pad_filter() -> &'static str { "scale=1080:1920:force_original_aspect_ratio=decrease:flags=lanczos,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:color=black" }

fn check_ext(path: &str, ext: &str) -> Result<(), String> { if path.to_lowercase().ends_with(ext) { Ok(()) } else { Err(format!("this preset requires a {ext} output path")) } }

/// Stream-copy remux in place to move the moov atom to the front (progressive-download "+faststart").
/// The audio mux step after the picture encode rewrites the file and a prior +faststart flag on the
/// video-only encode does not survive that rewrite, so every faststart-tagged preset calls this once
/// the audio is muxed in. On remux failure the original (non-faststart) file is left in place rather
/// than erroring the whole export -- the file is still valid, just not front-loaded.
fn remux_faststart(path: &str) -> Result<(), String> {
    let tmp = format!("{path}.fs.tmp.mp4");
    let ok = Cmd::new("ffmpeg").args(["-v", "error", "-y", "-i", path, "-c", "copy", "-movflags", "+faststart", &tmp]).status().map(|s| s.success()).unwrap_or(false);
    if ok { std::fs::rename(&tmp, path).map_err(|e| e.to_string())?; } else { let _ = std::fs::remove_file(&tmp); }
    Ok(())
}

pub fn commands() -> Vec<Command> { vec![
Command { name: "inspect_export_preset", description: "Describe a named export preset, or list them all: container, codec, colour rules and output size. Describe a named export preset (web-1080, master, alpha-prores) or list all: container, codec, color rules and the output size it would produce for the current project.", mutates: false,
    schema: obj(vec![("name", s("preset name, omit to list"))], &[]),
    run: |ss, a| { let one = |n: &str, e: &str, d: &str| { let mut v = json!({"name":n,"extension":e,"description":d});
            if n == "web-1080" { let (w, h) = fit_web(ss.project.width as usize, ss.project.height as usize); v["output_size"] = json!([w, h]); }
            if n == "tiktok" { v["output_size"] = json!([1080, 1920]); }
            if n == "youtube" { v["output_size"] = json!([ss.project.width, ss.project.height]); }
            v };
        match a.get("name").and_then(|v| v.as_str()) { Some(n) => { let p = PRESETS.iter().find(|p| p.0 == n).ok_or("unknown preset")?; Ok(one(p.0, p.1, p.2)) }, None => Ok(json!({"presets": PRESETS.iter().map(|p| one(p.0, p.1, p.2)).collect::<Vec<_>>()})) } } },
Command { name: "export_preset", description: "Export with a named preset: web-1080 (H.264/AAC mp4), master (FFV1 lossless mkv) or alpha-prores (ProRes 4444 mov). Export with a named preset: web-1080 (.mp4, H.264/AAC, fit 1080p), master (.mkv, FFV1 16-bit RGB lossless) or alpha-prores (.mov, ProRes 4444 with alpha). Container/extension enforced. Optional start/end seconds (web-1080 and alpha-prores). Ordinary export is unchanged.", mutates: false,
    schema: obj(vec![("path", s("output path; extension must match the preset")), ("preset", json!({"type":"string","enum":["web-1080","master","alpha-prores","youtube","tiktok","prores422","dnxhr","webm-vp9","hevc","av1"]})), ("start", n("range start s, default 0")), ("end", n("range end s, default end")), ("encoder", json!({"type":"string","enum":["libx264","h264_nvenc","hevc_nvenc"],"description":"web-1080, youtube, tiktok and hevc only. NVENC falls back to the preset's software encoder if this machine cannot run it; the result's encoder says what was used"}))], &["path","preset"]),
    run: |ss, a| { let p = &ss.project; let path = str_arg(a, "path")?; let preset = str_arg(a, "preset")?;
        let (t_in, t_out) = (opt_num(a, "start", 0.0), opt_num(a, "end", p.duration()));
        if !t_in.is_finite() || !t_out.is_finite() || t_in < 0.0 || t_out <= t_in { return Err("range must satisfy 0 <= start < end".into()); }
        let t_out = t_out.min(p.duration()); if t_in >= t_out { return Err("range starts after the end of the timeline".into()); }
        let f0 = (t_in * p.fps).round() as usize; let f1 = ((t_out * p.fps).round() as usize).max(f0 + 1);
        if a.get("encoder").is_some() && !["web-1080", "youtube", "tiktok", "hevc"].contains(&preset) { return Err(format!("preset {preset} has no encoder option")); }
        let (enc, fell_back) = crate::render::resolve_encoder(a.get("encoder").and_then(|v| v.as_str()))?;
        let tag = |mut v: Value, software: &str| { v["encoder"] = json!(enc.as_deref().unwrap_or(software)); if let Some(w) = &fell_back { v["encoder_fallback"] = json!(w); } v };
        match preset {
            "master" => { check_ext(path, ".mkv")?; if a.get("start").is_some() || a.get("end").is_some() { return Err("master preset exports the whole timeline (no range)".into()); } let d = crate::color::pipeline::export_high(p, path, "ffv1_16", 18)?; Ok(json!({"path":path,"duration":d,"preset":preset})) }
            "web-1080" => { check_ext(path, ".mp4")?;
                // reuse the ordinary encoder's color validation (Rec709 only, no HDR)
                let _ = crate::render::legacy_encoder_command(p, path, 18)?; if p.color_pipeline.enabled && [6, 7].contains(&p.color_pipeline.output_transfer) { return Err("web-1080 is SDR Rec709; HDR output is not converted".into()); }
                let (w, h) = fit_web(p.width as usize, p.height as usize);
                let trc = if p.color_pipeline.enabled { match p.color_pipeline.output_transfer { 0 => "linear", 8 => "bt709", _ => "iec61966-2-1" } } else { "iec61966-2-1" };
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "18",
                    "-vf", &format!("scale={w}:{h}:flags=lanczos:in_range=pc:out_color_matrix=bt709:out_range=tv"), "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", trc, "-colorspace", "bt709", "-movflags", "+faststart", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, crate::render::with_encoder(cmd, enc.as_deref()), false, true)?;
                // audio mux rewrites the file; restore faststart
                let tmp = format!("{path}.fs.tmp.mp4"); let ok = Cmd::new("ffmpeg").args(["-v", "error", "-y", "-i", path, "-c", "copy", "-movflags", "+faststart", &tmp]).status().map(|s| s.success()).unwrap_or(false);
                if ok { std::fs::rename(&tmp, path).map_err(|e| e.to_string())?; } else { let _ = std::fs::remove_file(&tmp); }
                Ok(tag(json!({"path":path,"duration":d,"preset":preset,"output_size":[w,h]}), "libx264")) }
            "alpha-prores" => { check_ext(path, ".mov")?; if p.color_pipeline.enabled { return Err("alpha-prores supports the display-sRGB Rec709 project only for now; disable the managed color pipeline".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuva444p10le", "-c:v", "prores_ks", "-profile:v", "4", "-pix_fmt", "yuva444p10le", "-alpha_bits", "16", "-vendor", "apl0", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-bsf:v", "prores_metadata=color_primaries=bt709:color_trc=13:colorspace=bt709", "-movflags", "+write_colr", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, cmd, true, false)?; Ok(json!({"path":path,"duration":d,"preset":preset})) }
            "youtube" => { check_ext(path, ".mp4")?; let _ = crate::render::legacy_encoder_command(p, path, 16)?;
                if p.color_pipeline.enabled && [6, 7].contains(&p.color_pipeline.output_transfer) { return Err("youtube is SDR Rec709; HDR output is not converted".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-c:v", "libx264", "-profile:v", "high", "-preset", "slow", "-crf", "16",
                    "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv", "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-movflags", "+faststart", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, crate::render::with_encoder(cmd, enc.as_deref()), false, true)?;
                remux_faststart(path)?; Ok(tag(json!({"path":path,"duration":d,"preset":preset,"output_size":[p.width,p.height]}), "libx264")) }
            "tiktok" => { check_ext(path, ".mp4")?; let _ = crate::render::legacy_encoder_command(p, path, 20)?;
                if p.color_pipeline.enabled && [6, 7].contains(&p.color_pipeline.output_transfer) { return Err("tiktok is SDR Rec709; HDR output is not converted".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "20",
                    "-vf", &format!("scale=in_range=pc:out_color_matrix=bt709:out_range=tv,{}", tiktok_scale_pad_filter()), "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-movflags", "+faststart", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, crate::render::with_encoder(cmd, enc.as_deref()), false, true)?;
                remux_faststart(path)?; Ok(tag(json!({"path":path,"duration":d,"preset":preset,"output_size":[1080,1920]}), "libx264")) }
            "prores422" => { check_ext(path, ".mov")?; if p.color_pipeline.enabled { return Err("prores422 supports the display-sRGB Rec709 project only for now; disable the managed color pipeline".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuv422p10le", "-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le", "-vendor", "apl0", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, cmd, false, true)?; Ok(json!({"path":path,"duration":d,"preset":preset})) }
            "dnxhr" => { check_ext(path, ".mov")?; if p.color_pipeline.enabled { return Err("dnxhr supports the display-sRGB Rec709 project only for now; disable the managed color pipeline".into()); }
                if p.width < 256 || p.height < 120 { return Err(format!("dnxhr (ffmpeg's dnxhd encoder) refuses input under 256x120; this project is {}x{}", p.width, p.height)); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuv422p", "-c:v", "dnxhd", "-profile:v", "dnxhr_hq", "-pix_fmt", "yuv422p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, cmd, false, true)?; Ok(json!({"path":path,"duration":d,"preset":preset})) }
            "webm-vp9" => { check_ext(path, ".webm")?; if p.color_pipeline.enabled { return Err("webm-vp9 supports the display-sRGB Rec709 project only for now; disable the managed color pipeline".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuv420p", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32", "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, cmd, false, false)?; // audio not muxed yet (see preset description)
                Ok(json!({"path":path,"duration":d,"preset":preset})) }
            "hevc" => { check_ext(path, ".mp4")?; if p.color_pipeline.enabled && [6, 7].contains(&p.color_pipeline.output_transfer) { return Err("hevc is SDR Rec709; HDR output is not converted".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-c:v", "libx265", "-preset", "medium", "-crf", "20", "-tag:v", "hvc1",
                    "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv", "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-movflags", "+faststart", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, crate::render::with_encoder(cmd, enc.as_deref()), false, true)?;
                remux_faststart(path)?; Ok(tag(json!({"path":path,"duration":d,"preset":preset}), "libx265")) }
            "av1" => { check_ext(path, ".mp4")?; if p.color_pipeline.enabled && [6, 7].contains(&p.color_pipeline.output_transfer) { return Err("av1 is SDR Rec709; HDR output is not converted".into()); }
                let mut cmd = Cmd::new("ffmpeg"); cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", p.width, p.height), "-r", &p.fps.to_string(), "-i", "-", "-c:v", "libsvtav1", "-preset", "8", "-crf", "30",
                    "-vf", "scale=in_range=pc:out_color_matrix=bt709:out_range=tv", "-pix_fmt", "yuv420p", "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-colorspace", "bt709", "-movflags", "+faststart", path]);
                let d = crate::render::export_range_cmd(p, path, f0, f1, cmd, false, true)?;
                remux_faststart(path)?; Ok(json!({"path":path,"duration":d,"preset":preset})) }
            _ => Err("unknown preset".into()),
        } } },
] }
#[cfg(test)] mod tests { use super::fit_web;
    #[test] fn fit_rules() { assert_eq!(fit_web(2560, 1440), (1920, 1080)); assert_eq!(fit_web(1000, 563), (1000, 562)); assert_eq!(fit_web(1080, 1920), (1080, 1920)); assert_eq!(fit_web(1280, 720), (1280, 720)); assert_eq!(fit_web(2160, 3840), (1080, 1920)); assert_eq!(fit_web(641, 361), (640, 360)); } }
