//! Tool definitions, part 1 of 5: a verbatim slice of the registry table (order is preserved, see `build_registry`).
#![allow(unused_imports)]
use super::*;

/// The body of `add_clip`, factored out so the `run` closure can choose whether to pay for a
/// project clone+restore (only when `mode` is given -- see the closure). Validates everything
/// (source_type/asset_id/color/gradient/fit) BEFORE performing the mode:"insert"/"overwrite"
/// ripple-or-lift, so a bad argument caught by that validation never leaves a partial mutation:
/// with mode none it cannot mutate before the final push either way (same as the original code).
fn add_clip_place(ss: &mut Session, a: &Value, mode: Option<&str>, ripple_tracks: &[String]) -> Result<Value, String> {
    let tid = str_arg(a, "track_id")?.to_string();
    let src = match str_arg(a, "source_type")? {
        "media" => { let id = str_arg(a, "asset_id")?; if !ss.project.assets.contains_key(id) { return Err(format!("no asset '{id}'")); } Source::Media { asset: id.into() } }
        "solid" => Source::Solid { color: color(a, "color", [0.0; 3])? },
        "gradient" => Source::Gradient { c1: color(a, "color", [0.0; 3])?, c2: color(a, "color2", [1.0; 3])? },
        o => return Err(format!("bad source_type {o}")) };
    let id = ss.project.gen_id("clip");
    let mut c = Clip { id: id.clone(), source: src, start: num_arg(a, "start")?, duration: num_arg(a, "duration")?, in_point: opt_num(a, "in_point", 0.0), speed: opt_num(a, "speed", 1.0), speed_curve: None, time_remap: None, speed_blend: Default::default(), props: default_props(), grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None };
    // A solid's colour alpha ("#rrggbbaa" / [r,g,b,a]) was dropped and the solid drew opaque; it is the clip's fill_alpha, as on add_shape (separate from layer opacity, composed with it).
    if matches!(c.source, Source::Solid { .. }) { let al = color_alpha(a, "color"); if al < 1.0 { c.props.insert("fill_alpha".into(), Prop::new(al)); } }
    // A gradient's end alphas were dropped too (a "#00000000" -> "#000000cc" scrim drew opaque black): kept as color_alpha / color2_alpha, only when < 1.
    if matches!(c.source, Source::Gradient { .. }) { if let Some(v) = a.get("angle").filter(|v| !v.is_null()) { let d = v.as_f64().filter(|d| d.is_finite()).ok_or("angle must be a finite number of degrees")?; c.props.insert("gradient_angle".into(), Prop::new(d)); }
        match a.get("gradient").and_then(|v| v.as_str()) { None | Some("linear") => {} Some("radial") => { c.props.insert("gradient_radial".into(), Prop::new(1.0)); } Some(o) => return Err(format!("bad gradient '{o}': use linear or radial")) } }
    else if a.get("angle").is_some_and(|v| !v.is_null()) || a.get("gradient").is_some_and(|v| !v.is_null()) { return Err("angle and gradient apply to source_type gradient only".into()); }
    if matches!(c.source, Source::Gradient { .. }) { for k in ["color", "color2"] { let al = color_alpha(a, k); if al < 1.0 { c.props.insert(format!("{k}_alpha"), Prop::new(al)); } } }
    match a.get("fit").and_then(|v| v.as_str()) { Some("cover") => { c.props.insert("fit".into(), Prop::new(1.0)); } Some("stretch") => { c.props.insert("fit".into(), Prop::new(2.0)); } Some("contain") | None => {} Some(o) => return Err(format!("bad fit {o}; use contain, cover or stretch")) }
    // Everything above is pure validation/construction (no project mutation); the ripple/lift below is the one and
    // only mutation before the final push, so an error at this point (track_id/ripple_tracks missing, locked, bad
    // duration) still leaves the project untouched for the mode:none case, and is caught by the caller's
    // clone+restore for the mode:Some case (do_insert_gap on a *later* ripple_tracks entry can fail after an
    // earlier one already mutated -- that's exactly what the outer wrapper undoes).
    match mode {
        Some("insert") => { let (st, d) = (num_arg(a, "start")?, num_arg(a, "duration")?); crate::edit::do_insert_gap(ss, &tid, st, d)?; for rt in ripple_tracks { crate::edit::do_insert_gap(ss, rt, st, d)?; } }
        Some("overwrite") => { let (st, d) = (num_arg(a, "start")?, num_arg(a, "duration")?); crate::edit::do_lift_range(ss, &tid, st, st + d, false)?; }
        _ => {}
    }
    let tr = ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or(format!("no track '{tid}'"))?; tr.clips.push(std::sync::Arc::new(c));
    Ok(json!({"clip_id":id}))
}

pub(super) fn tools(eff_kinds: &[&str]) -> Vec<Command> {
    vec![
    Command { name: "project_set", description: "Set project canvas size and frame rate (name, linear quality option).", mutates: true,
        schema: obj(vec![("width", n("pixels")), ("height", n("pixels")), ("fps", n("frames per second")), ("name", s("project name")),
            ("linear_blend", json!({"type":"boolean"})),
            ("x264_preset", json!({"type":"string"})),
            ("color_tag_sanity", json!({"type":"boolean"})),
            ("frame_blend_conform", json!({"type":"boolean"}))],
            &[]),
        run: |ss, a| { let p = &mut ss.project;
            if let Some(v) = a.get("width").and_then(|v| v.as_u64()) { if !(16..=16384).contains(&v) { return Err(format!("width {v} out of range 16..16384")); } p.width = v as u32; }
            if let Some(v) = a.get("height").and_then(|v| v.as_u64()) { if !(16..=16384).contains(&v) { return Err(format!("height {v} out of range 16..16384")); } p.height = v as u32; }
            if let Some(v) = a.get("fps").and_then(|v| v.as_f64()) { if !(1.0..=480.0).contains(&v) { return Err(format!("fps {v} out of range 1..480")); } p.fps = v; }
            if let Some(v) = a.get("name").and_then(|v| v.as_str()) { p.name = v.into(); }
            if let Some(v) = a.get("linear_blend").and_then(|v| v.as_bool()) { p.linear_blend = v; }
            if let Some(v) = a.get("x264_preset") { match v { Value::Null => p.x264_preset = None, Value::String(s) => { const OK: [&str; 9] = ["ultrafast","superfast","veryfast","faster","fast","medium","slow","slower","veryslow"]; if !OK.contains(&s.as_str()) { return Err(format!("x264_preset must be one of {OK:?}, got '{s}'")); } p.x264_preset = Some(s.clone()); }, _ => return Err("x264_preset must be a string or null".into()) } }
            if let Some(v) = a.get("color_tag_sanity").and_then(|v| v.as_bool()) { p.color_tag_sanity = v; }
            if let Some(v) = a.get("frame_blend_conform").and_then(|v| v.as_bool()) { p.frame_blend_conform = v; }
            Ok(json!({"width":p.width,"height":p.height,"fps":p.fps,"linear_blend":p.linear_blend,"x264_preset":p.x264_preset,"color_tag_sanity":p.color_tag_sanity,"frame_blend_conform":p.frame_blend_conform})) } },
    Command { name: "import_media", description: "Import a video file as an asset (probes size/duration). Returns asset id.", mutates: true,
        schema: obj(vec![("path", s("absolute path to media file"))], &["path"]),
        run: |ss, a| { let path = str_arg(a, "path")?; let (w, h, d, fps) = render::probe(path)?; render::check_importable(path)?;
            let id = ss.project.gen_id("asset"); ss.project.assets.insert(id.clone(), Asset { origin:None,hash:None,kind:None,id: id.clone(), path: path.into(), width: w, height: h, duration: d, fps, bin: None });
            Ok(json!({"asset_id":id,"width":w,"height":h,"duration":d,"fps":fps})) } },
    Command { name: "add_track", description: "Add a track on top of the stack. Higher tracks composite over lower ones.", mutates: true,
        schema: obj(vec![], &[]),
        run: |ss, _| { let id = ss.project.gen_id("track"); ss.project.tracks.push(Track { id: id.clone(), clips: vec![], locked: false, muted: false, solo: false, name: String::new() }); Ok(json!({"track_id":id})) } },
    Command { name: "add_clip", description: "Add a clip to a track. source_type: media (needs asset_id), solid (color [r,g,b] 0..1), gradient (color, color2; linear at angle, or radial).", mutates: true,
        schema: obj(vec![("track_id", s("target track")), ("source_type", json!({"type":"string","enum":["media","solid","gradient"]})), ("asset_id", s("for media")),
            ("color", col_schema("", false)), ("color2", col_schema("gradient end colour", false)), ("gradient", json!({"type":"string","enum":["linear","radial"],"description":"gradient shape: linear (default) or radial (color at the centre, color2 at the corners)"})), ("angle", n("linear gradient direction in degrees, color -> color2: 0 top->bottom (default), 90 left->right, 180 bottom->top; keyframe as gradient_angle")),
            ("start", n("timeline start seconds")), ("duration", n("seconds")), ("in_point", n("source offset seconds")), ("speed", n("playback speed, default 1")), ("fit", json!({"type":"string","enum":["contain","cover","stretch"],"description":"media framing: contain (letterbox, default), cover (fill + center crop), stretch"})),
            ("mode", json!({"type":"string","enum":["insert","overwrite"],"description":"omitted (default): plain placement, exactly as before. insert: ripple later clips on track_id (and ripple_tracks) later by `duration` first (FEATURES.md insert-edit). overwrite: clear [start, start+duration) on track_id first, trimming/removing what's there, nothing else moves (overwrite-edit)."})),
            ("ripple_tracks", json!({"type":"array","items":{"type":"string"},"description":"mode:\"insert\" only: also ripple these other tracks at the same point, to keep multi-track sync"}))], &["track_id","source_type","start","duration"]),
        run: |ss, a| {
            let mode = match a.get("mode") { None | Some(Value::Null) => None, Some(Value::String(s)) if s == "insert" || s == "overwrite" => Some(s.clone()), Some(Value::String(s)) => return Err(format!("mode must be insert or overwrite, got '{s}'")), _ => return Err("mode must be a string".into()) };
            let ripple_tracks: Vec<String> = match a.get("ripple_tracks") { None | Some(Value::Null) => vec![], Some(Value::Array(v)) => v.iter().map(|x| x.as_str().map(String::from).ok_or_else(|| "ripple_tracks must be an array of strings".to_string())).collect::<Result<_,_>>()?, _ => return Err("ripple_tracks must be an array of strings".into()) };
            if !ripple_tracks.is_empty() && mode.as_deref() != Some("insert") { return Err("ripple_tracks only applies to mode:\"insert\"".into()); }
            // mode is the only path that can mutate the project before every argument is validated (the insert/overwrite
            // ripple happens ahead of the source_type/asset/color/fit checks below, so a later validation error must not
            // leave a half-applied ripple or lift behind); the plain (mode: none) path is untouched and pays no clone.
            if mode.is_some() { let before = ss.project.clone(); match add_clip_place(ss, a, mode.as_deref(), &ripple_tracks) { Ok(v) => Ok(v), Err(e) => { ss.project = before; Err(e) } } }
            else { add_clip_place(ss, a, None, &[]) }
        } },
    Command { name: "add_audio_clip", description: "Add an audio-only clip (music, voice, sfx) from an imported asset. It never draws and never takes part in the video stack. Creates an audio track if track_id is omitted. Export applies a limiter (ceiling -1.5 dBFS) so peaks stay under about -1 dBTP.", mutates: true,
        schema: obj(vec![("asset_id", s("imported asset (wav/mp3/video with audio)")), ("track_id", s("existing track, optional")), ("start", n("timeline start seconds")), ("duration", n("seconds")), ("in_point", n("source offset seconds")), ("volume", n("gain, default 1"))], &["asset_id","start","duration"]),
        run: |ss, a| { let id = str_arg(a, "asset_id")?; if !ss.project.assets.contains_key(id) { return Err(format!("no asset '{id}'")); }
            let tid = match a.get("track_id").and_then(|v| v.as_str()) { Some(t) => t.to_string(), None => { let t = ss.project.gen_id("track"); ss.project.tracks.push(Track { id: t.clone(), clips: vec![], locked: false, muted: false, solo: false, name: "audio".into() }); t } };
            let cid = ss.project.gen_id("clip"); let mut props = default_props(); props.insert("audio_only".into(), Prop::new(1.0)); props.insert("opacity".into(), Prop::new(0.0));
            let mut au = Audio::default(); au.volume = Prop::new(opt_num(a, "volume", 1.0));
            let c = Clip { id: cid.clone(), source: Source::Media { asset: id.into() }, start: num_arg(a, "start")?, duration: num_arg(a, "duration")?, in_point: opt_num(a, "in_point", 0.0), speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props, grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: au, motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None };
            ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or(format!("no track '{tid}'"))?.clips.push(std::sync::Arc::new(c)); Ok(json!({"clip_id":cid,"track_id":tid})) } },
    Command { name: "add_text", description: "Add a text clip (rendered with a TTF/OTF font). Animate with set_keyframe on reveal (typewriter), opacity, scale, x, y, rotation. Default font is a system sans.", mutates: true,
        schema: obj(vec![("track_id", s("track")), ("text", s("text, \\n for newlines")), ("start", n("seconds")), ("duration", n("seconds")), ("font", s("font family name (e.g. Montserrat) or path to .ttf/.otf; both accepted, optional")), ("x", n("pixel offset of the anchor from canvas center, default 0")), ("anchor", json!({"type":"string","enum":["center","left","right"],"description":"what x/y position refers to: the text block center (default), its left edge or its right edge. The block is centered on the canvas at x=0 for every align; align only aligns lines inside the block."})), ("size", n("pixels, default 72")),
            ("color", col_schema("", false)), ("letter_spacing", n("pixels")), ("stroke_width", n("pixels")), ("stroke_color", col_schema("", false)), ("shadow", n("shadow offset pixels")), ("align", json!({"type":"string","enum":["left","center","right"]}))], &["track_id","text","start","duration"]),
        run: |ss, a| { let tid = str_arg(a, "track_id")?.to_string(); let id = ss.project.gen_id("clip");
            let src = Source::Text { text: str_arg(a, "text")?.replace("\\n", "\n"), font: a.get("font").and_then(|v| v.as_str()).unwrap_or("Montserrat").into(), size: opt_num(a, "size", 72.0), color: color(a, "color", [1.0; 3])?,
                letter_spacing: opt_num(a, "letter_spacing", 0.0), stroke_width: opt_num(a, "stroke_width", 0.0), stroke_color: color(a, "stroke_color", [0.0; 3])?, shadow: opt_num(a, "shadow", 0.0), align: a.get("align").and_then(|v| v.as_str()).unwrap_or("center").into() };
            let c = Clip { id: id.clone(), source: src, start: num_arg(a, "start")?, duration: num_arg(a, "duration")?, in_point: 0.0, speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props: default_props(), grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None };
            let mut c = c; { let an = a.get("anchor").and_then(|v| v.as_str()).unwrap_or("center"); if an != "center" { if let Source::Text { text, font, size, letter_spacing, align, .. } = &c.source { let cw = ss.project.width as f64; let bw = crate::text::measure(text, font, *size, *letter_spacing, 0.0, align, cw)?.0; let dx = if an == "left" { bw / 2.0 } else if an == "right" { -bw / 2.0 } else { return Err("anchor must be center, left or right".into()) }; if let Some(p) = c.props.get_mut("x") { p.base = dx + opt_num(a, "x", 0.0); } } } else if let Some(p) = c.props.get_mut("x") { p.base = opt_num(a, "x", 0.0); } }
            { let al = color_alpha(a, "color"); if al < 1.0 { if let Some(p) = c.props.get_mut("opacity") { p.base *= al; } } }
            ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or("no such track")?.clips.push(std::sync::Arc::new(c)); Ok(json!({"clip_id":id})) } },
    Command { name: "set_text", description: "Edit text content/style of a text clip.", mutates: true,
        schema: obj(vec![("clip_id", s("text clip")), ("text", s("new text")), ("size", n("pixels")), ("color", col_schema("", false))], &["clip_id"]),
        run: |ss, a| { let c = clip(&mut ss.project, a)?; if let Source::Text { text, size, color: col, .. } = &mut c.source {
            if let Some(t) = a.get("text").and_then(|v| v.as_str()) { *text = t.replace("\\n", "\n"); } if let Some(v) = a.get("size").and_then(|v| v.as_f64()) { *size = v; }
            if a.get("color").is_some() { *col = color(a, "color", *col)?; } Ok(json!({"ok":true})) } else { Err("not a text clip".into()) } } },
    Command { name: "set_shape_size", description: "Set a shape clip's own width and height in pixels. This is the shape's size field (rect/ellipse/polygon/line), independent of its animated scale -- not a Prop, so it has no keyframes of its own to fight. Use this to resize a box/pill that backs a text label (e.g. a subscribe_pop pill) after the text it hugs changes size, so the box keeps matching the rendered text bounds instead of staying the width it was first built for.", mutates: true,
        schema: obj(vec![("clip_id", s("shape clip")), ("width", n("pixels")), ("height", n("pixels"))], &["clip_id"]),
        run: |ss, a| { let c = clip(&mut ss.project, a)?; if let Source::Shape { width, height, .. } = &mut c.source {
            if let Some(v) = a.get("width").and_then(|v| v.as_f64()) { *width = v; } if let Some(v) = a.get("height").and_then(|v| v.as_f64()) { *height = v; }
            Ok(json!({"ok":true})) } else { Err("not a shape clip".into()) } } },
    Command { name: "set_mask", description: "Set a mask on a clip: shape rect|ellipse|linear, normalized cx,cy,w,h (0..1), feather (0..1), rotation (deg), invert. Mask params are keyframable via set_mask_keyframe.", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("shape", json!({"type":"string","enum":["rect","ellipse","linear"]})), ("cx", n("0..1")), ("cy", n("0..1")), ("w", n("0..1")), ("h", n("0..1")), ("feather", n("0..1")), ("rotation", n("degrees")), ("invert", json!({"type":"boolean"}))], &["clip_id","shape"]),
        run: |ss, a| { let shape = str_arg(a, "shape")?.to_string(); let mut props = std::collections::BTreeMap::new();
            for (k, d) in [("cx",0.5),("cy",0.5),("w",0.5),("h",0.5),("feather",0.02),("rotation",0.0)] { props.insert(k.to_string(), Prop::new(opt_num(a, k, d))); }
            let inv = a.get("invert").and_then(|v| v.as_bool()).unwrap_or(false); clip(&mut ss.project, a)?.mask = Some(Mask { shape, props, invert: inv, layers: clip(&mut ss.project, a)?.mask.take().map(|m| m.layers).unwrap_or_default(), matte: None }); Ok(json!({"ok":true})) } },
    Command { name: "set_mask_keyframe", description: "Keyframe a mask param (cx, cy, w, h, feather, rotation) at clip-local time.", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("property", s("mask param")), ("time", n("seconds")), ("value", n("value")), ("ease", json!({"type":"string","enum":crate::keyframe::EASE_NAMES}))], &["clip_id","property","time","value"]),
        run: |ss, a| { let k = str_arg(a, "property")?.to_string(); let t = num_arg(a, "time")?; let v = num_arg(a, "value")?; let e: Ease = serde_json::from_value(a.get("ease").cloned().unwrap_or(json!("linear"))).map_err(|e| e.to_string())?;
            let m = clip(&mut ss.project, a)?.mask.as_mut().ok_or("clip has no mask")?; m.props.entry(k).or_insert(Prop::new(v)).set_key(t, v, e); Ok(json!({"ok":true})) } },
    Command { name: "remove_mask", description: "Remove a clip's mask.", mutates: true,
        schema: obj(vec![("clip_id", s("clip"))], &["clip_id"]),
        run: |ss, a| { clip(&mut ss.project, a)?.mask = None; Ok(json!({"ok":true})) } },
    Command { name: "set_audio", description: "Audio settings for a media clip: mute, volume, fades, pitch, filters, denoise, compression, speech leveling, echo and reverse. Audio settings for a media clip: mute, volume (base), fade_in/fade_out seconds, pitch (semitones), highpass/lowpass Hz, denoise (dB, 0=off), compress, speech_level (gentle automatic speech leveling, 0=off), echo (seconds delay), reverse. Volume is keyframable via set_audio_volume_key.", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("mute", json!({"type":"boolean"})), ("volume", n("0..4")), ("fade_in", n("s")), ("fade_out", n("s")), ("pitch", n("semitones")), ("highpass", n("Hz")), ("lowpass", n("Hz")), ("denoise", n("dB")), ("compress", json!({"type":"boolean"})), ("compress_amount", n("0..1, scales compress ratio, default 1.0 (old fixed 3:1), 0=off")), ("speech_level", n("gentle speech leveling (speechnorm factor): 0=off, ~1.5-2=gentle, up to 4=stronger, narrows loudness WITHIN this clip over time")), ("echo", n("s")), ("reverse", json!({"type":"boolean"}))], &["clip_id"]),
        run: |ss, a| { let c = clip(&mut ss.project, a)?; let au = &mut c.audio;
            if let Some(v) = a.get("mute").and_then(|v| v.as_bool()) { au.mute = v; } if let Some(v) = a.get("volume").and_then(|v| v.as_f64()) { au.volume.base = v; }
            for (k, r) in [("fade_in", &mut au.fade_in), ("fade_out", &mut au.fade_out), ("pitch", &mut au.pitch), ("highpass", &mut au.highpass), ("lowpass", &mut au.lowpass), ("denoise", &mut au.denoise), ("echo", &mut au.echo), ("speech_level", &mut au.speech_level), ("compress_amount", &mut au.compress_amount)] { if let Some(v) = a.get(k).and_then(|v| v.as_f64()) { *r = v; } }
            if let Some(v) = a.get("compress").and_then(|v| v.as_bool()) { au.compress = v; } if let Some(v) = a.get("reverse").and_then(|v| v.as_bool()) { au.reverse = v; }
            Ok(serde_json::to_value(&*au).unwrap()) } },
    Command { name: "set_audio_volume_key", description: "Keyframe clip volume at clip-local time (ducking, swells).", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("time", n("s")), ("value", n("gain")), ("ease", json!({"type":"string","enum":crate::keyframe::EASE_NAMES}))], &["clip_id","time","value"]),
        run: |ss, a| { let e: Ease = serde_json::from_value(a.get("ease").cloned().unwrap_or(json!("linear"))).map_err(|e| e.to_string())?; let (t, v) = (num_arg(a, "time")?, num_arg(a, "value")?); clip(&mut ss.project, a)?.audio.volume.set_key(t, v, e); Ok(json!({"ok":true})) } },
    Command { name: "set_audio_pan_key", description: "Keyframe clip stereo pan at clip-local time (-1 left to +1 right; ease curves and expressions work the same as volume).", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("time", n("s")), ("value", n("-1..1")), ("ease", json!({"type":"string","enum":crate::keyframe::EASE_NAMES}))], &["clip_id","time","value"]),
        run: |ss, a| { let e: Ease = serde_json::from_value(a.get("ease").cloned().unwrap_or(json!("linear"))).map_err(|e| e.to_string())?; let (t, v) = (num_arg(a, "time")?, num_arg(a, "value")?); clip(&mut ss.project, a)?.audio.pan.set_key(t, v, e); Ok(json!({"ok":true})) } },
    Command { name: "detect_beats", description: "Detect beat/onset times (seconds) in an audio or video file, for cutting on the beat. Returns times plus tempo estimate.", mutates: false,
        schema: obj(vec![("path", s("media path")), ("min_gap", n("min seconds between beats, default 0.25"))], &["path"]),
        run: |_, a| { let (b, bpm) = crate::audio::analyze(str_arg(a, "path")?, opt_num(a, "min_gap", 0.25)); Ok(json!({"beats":b,"bpm":bpm})) } },
    Command { name: "export_audio", description: "Mix all clip audio to a 16-bit 48k stereo WAV.", mutates: false,
        schema: obj(vec![("path", s("output .wav"))], &["path"]),
        run: |ss, a| { let p = str_arg(a, "path")?; crate::audio::write_wav(p, &crate::audio::mix(&ss.project))?; Ok(json!({"path":p})) } },
    Command { name: "set_speed_ramp", description: "Speed ramp: keyed playback speed over clip time ([[t,speed]]), eased. frame_blend off|frame_mix|optical_flow smooths slow motion. [] clears.", mutates: true,
        schema: obj(vec![("clip_id", s("clip")), ("keys", json!({"type":"array","items":{"type":"array","items":{"type":"number"},"description":"[time, speed]"}})), ("ease", json!({"type":"string","enum":crate::keyframe::EASE_NAMES})),
            ("frame_blend", json!({"type":"string","enum":["off","frame_mix","optical_flow"],"description":"Time Interpolation between source frames: off = nearest frame (default), frame_mix = cross-blend, optical_flow = motion-interpolated in-betweens"}))], &["clip_id","keys"]),
        run: |ss, a| { let e: Ease = serde_json::from_value(a.get("ease").cloned().unwrap_or(json!("in_out"))).map_err(|e| e.to_string())?;
            // read before the clip is touched, so a bad mode changes nothing
            let fb = match a.get("frame_blend").filter(|v| !v.is_null()) { None => None, Some(v) => Some(serde_json::from_value::<FrameBlend>(v.clone()).map_err(|_| format!("unknown frame_blend {v}; use off|frame_mix|optical_flow"))?) };
            let ks: Vec<(f64, f64)> = a.get("keys").and_then(|v| v.as_array()).ok_or("keys required")?.iter().filter_map(|k| { let k = k.as_array()?; Some((k.get(0)?.as_f64()?, k.get(1)?.as_f64()?)) }).collect();
            let c = clip(&mut ss.project, a)?; if ks.is_empty() { c.speed_curve = None; c.speed_blend = FrameBlend::Off; return Ok(json!({"cleared":true})); }
            let mut p = Prop::new(ks[0].1); for (t, v) in &ks { if !(0.0..=100.0).contains(v) { return Err("speed must be 0..100".into()); } p.set_key(*t, *v, e); } c.speed_curve = Some(p);
            if let Some(fb) = fb { c.speed_blend = fb; }
            let mut r = json!({"source_seconds": c.source_time(c.duration)}); if !c.speed_blend.is_off() { r["frame_blend"] = serde_json::to_value(c.speed_blend).unwrap_or(Value::Null); }
            Ok(r) } },
    Command { name: "set_time_remap", description: "AE time remap: keyframe source time vs clip time ([[t,src,ease?]]); holds, reverse, ramps. frame_blend off|frame_mix|optical_flow. [] clears.", mutates: true,
        schema: obj(vec![("clip_id", s("media clip")), ("keys", json!({"type":"array","items":{"type":"array","items":{"type":["number","string"]},"description":"[clip_time, source_time, ease?]"}})), ("ease", json!({"type":"string","enum":crate::keyframe::EASE_NAMES})), ("frame_blend", json!({"type":"string","enum":["off","frame_mix","optical_flow"]}))], &["clip_id"]),
        run: |ss, a| { let de = a.get("ease").and_then(|v| v.as_str()).unwrap_or("linear"); let de = Ease::parse(de).ok_or(format!("unknown ease {de}"))?;
            let fb = match a.get("frame_blend").filter(|v| !v.is_null()) { None => None, Some(v) => Some(serde_json::from_value::<FrameBlend>(v.clone()).map_err(|_| format!("unknown frame_blend {v}; use off|frame_mix|optical_flow"))?) };
            let ks = match a.get("keys").filter(|v| !v.is_null()) { None => None, Some(v) => { let mut o = vec![]; for k in v.as_array().ok_or("keys must be an array of [clip_time, source_time, ease?]")? {
                let k = k.as_array().filter(|k| k.len() == 2 || k.len() == 3).ok_or("each key is [clip_time, source_time] or [clip_time, source_time, ease]")?;
                let (t, sv) = (k[0].as_f64().ok_or("key times must be numbers")?, k[1].as_f64().ok_or("source times must be numbers")?);
                if !t.is_finite() || !sv.is_finite() || t < 0.0 || sv < 0.0 { return Err(format!("key [{t}, {sv}]: clip and source times must be finite and >= 0")); }
                let e = match k.get(2) { None => de, Some(e) => { let n = e.as_str().ok_or("key ease must be a string")?; Ease::parse(n).ok_or(format!("unknown ease {n}"))? } }; o.push((t, sv, e)); } Some(o) } };
            let c = clip(&mut ss.project, a)?; if !matches!(c.source, Source::Media { .. }) { return Err("set_time_remap: media clips only".into()); }
            match ks { Some(ks) if ks.is_empty() => { c.time_remap = None; return Ok(json!({"cleared":true})); }
                Some(ks) => { let mut p = Prop::new(ks[0].1); for (t, v, e) in &ks { p.set_key(*t, *v, *e); } let blend = fb.or(c.time_remap.as_ref().map(|r| r.blend)).unwrap_or_default(); c.time_remap = Some(TimeRemap { curve: p, offset: 0.0, blend }); }
                None => { let fb = fb.ok_or("set_time_remap: pass keys and/or frame_blend")?; c.time_remap.as_mut().ok_or("set_time_remap: clip has no time remap; pass keys")?.blend = fb; } }
            let r = c.time_remap.as_ref().unwrap(); let (lo, hi) = c.remap_span(0.0, c.duration);
            Ok(json!({"keys": r.curve.keys.len(), "frame_blend": r.blend, "source_span": [lo, hi]})) } },
    Command { name: "track_long_gap", description: "Track an object's appearance across occlusions, read-only, with whole-frame search and reacquisition. Read-only appearance tracking across occlusions: immutable seed template, whole-canvas multiscale search, ambiguity rejection and consecutive-frame reacquisition. Returns visible/reacquired boxes in fitted canvas pixels; lost/tentative boxes are null. Not semantic re-ID. Normal media clips only, inclusive clip-local frame range <=10000. No guessed interpolation or target keyframes. Optional appearance_memory=true learns a bounded multi-anchor bank from stable trusted observations, keeping the seed; this narrows scale search to .8/1/1.2 and can drift, not semantic re-ID.", mutates: false,
        schema: obj(vec![("clip_id", s("media clip")), ("bbox", json!({"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"normalized [x,y,width,height] at start_frame"})), ("start_frame", n("default 0")), ("end_frame", n("inclusive, default last clip frame")), ("threshold", n("NCC .5.. .99, default .8")), ("margin", n("distinct candidate score margin .01.. .5, default .08")), ("confirm_frames", n("consecutive consistent frames after loss, 2..10 default 2")), ("appearance_memory", json!({"type":"boolean","description":"opt-in multi-anchor appearance memory; default false"}))], &["clip_id","bbox"]),
        run: |ss,a| crate::long_gap::run(&ss.project,a) },
    Command { name: "track_point", description: "Motion-track a surface point on a media clip and attach a target clip (text, graphic) so it sticks. rigid=true (default) tracks a ring of patches and also solves rotation and scale, so attached graphics rotate/scale with the surface; rigid=false tracks translation only. Writes x/y (+rotation, scale) keyframes on target_clip_id. Memory is windowed.", mutates: true,
        schema: obj(vec![("clip_id", s("media clip to track")), ("target_clip_id", s("clip to attach")), ("x", n("0..1 point x at start")), ("y", n("0..1 point y at start")), ("patch", n("patch size px, default 40")), ("search", n("search radius px per frame, default 14")), ("rigid", json!({"type":"boolean"})), ("radius", n("ring radius as fraction of min dimension, default 0.12")), ("step", n("keyframe every N frames, default 1"))], &["clip_id","target_clip_id","x","y"]),
        run: |ss, a| { let tid = str_arg(a, "target_clip_id")?.to_string(); let (nx, ny) = (num_arg(a, "x")?, num_arg(a, "y")?); let rigid = a.get("rigid").and_then(|v| v.as_bool()).unwrap_or(true);
            let tr = crate::tracking::track_point(&ss.project, str_arg(a, "clip_id")?, nx, ny, opt_num(a, "patch", 40.0) as i32, opt_num(a, "search", 14.0) as i32, rigid, opt_num(a, "radius", 0.12))?;
            let (w, h) = (ss.project.width as f64, ss.project.height as f64); let step = opt_num(a, "step", 1.0).max(1.0) as usize;
            let src = ss.project.find_clip(str_arg(a, "clip_id")?).unwrap().start; let t0 = ss.project.find_clip(&tid).ok_or("no target clip")?.start;
            let tc = ss.project.find_clip_mut(&tid).ok_or("no target clip")?; let (bx, by) = (tc.props["x"].base, tc.props["y"].base); let (br, bs) = (tc.props.get("rotation").map(|p| p.base).unwrap_or(0.0), tc.props.get("scale").map(|p| p.base).unwrap_or(1.0));
            let (mut kx, mut ky, mut kr, mut ks) = (Prop::new(bx), Prop::new(by), Prop::new(br), Prop::new(bs)); let n = tr.len();
            for (i, s_) in tr.iter().enumerate() { if i % step != 0 && i != n - 1 { continue; } let lt = s_.t + src - t0; kx.set_key(lt, bx + s_.x - nx * w, Ease::Linear); ky.set_key(lt, by + s_.y - ny * h, Ease::Linear); if rigid { kr.set_key(lt, br + s_.rot_deg, Ease::Linear); ks.set_key(lt, bs * s_.scale, Ease::Linear); } }
            tc.props.insert("x".into(), kx); tc.props.insert("y".into(), ky); if rigid { tc.props.insert("rotation".into(), kr); tc.props.insert("scale".into(), ks); } let e = &tr[n-1]; Ok(json!({"frames":n,"end_px":[e.x, e.y],"end_rotation_deg":e.rot_deg,"end_scale":e.scale})) } },
    Command { name: "stabilize", description: "Two-pass stabilizer: estimate the camera path, smooth it and zoom just enough to hide the borders. Warp-stabilize a media clip (AE/Premiere Warp Stabilizer): robust path, smooth or locked, position/similarity/perspective, crop, zoom or synth edges.", mutates: true,
        schema: obj(vec![("clip_id", s("media clip")),
            ("method", json!({"type":"string","enum":["position","position_scale_rotation","perspective"],"description":"warp engine: what the correction may move (AE Method); similarity = position_scale_rotation (default); perspective keys pin corners"})),
            ("mode", json!({"type":"string","enum":["smooth_motion","no_motion"],"description":"warp engine: smooth_motion (default) keeps the camera move, no_motion locks the framing of the first frame"})),
            ("smoothness", n("warp engine: 0..100 %, default 50; Gaussian sigma is that % of one second, the L1 crop box that % of 10% of the frame")),
            ("smoothing", json!({"type":"string","enum":["gaussian","l1"],"description":"warp engine path smoother: gaussian (default) or l1 (Grundmann 2011 L1-optimal camera path)"})),
            ("framing", json!({"type":"string","enum":["stabilize_only","crop","crop_auto_scale","synthesize_edges"],"description":"warp: stabilize_only, crop (stabilize_crop mask), crop_auto_scale (default, zoom to fill) or synthesize_edges (fill borders from nearby frames)"})),
            ("max_features", n("warp engine: tracked features per frame, 50..2000, default 400")),
            ("synthesis_range", n("warp engine, synthesize_edges: seconds of frames before and after to fill borders from, 0.03..2, default 0.3")),
            ("rolling_shutter", n("warp engine: rolling-shutter readout as a fraction of the frame time, 0..1 (0 off); undoes jello/skew with a 9x17 mesh")),
            ("smooth", n("classic engine: sigma in frames, default 10")), ("lock", json!({"type":"boolean","description":"classic engine: true holds the camera fixed (tripod)"})), ("rotation", json!({"type":"boolean","description":"classic engine: false corrects translation only"}))], &["clip_id"]),
        run: |ss, a| { let id = str_arg(a, "clip_id")?.to_string(); let has = |k: &str| a.get(k).is_some_and(|v| !v.is_null());
            if ["method", "mode", "smoothness", "smoothing", "framing", "max_features", "synthesis_range", "rolling_shutter"].iter().any(|k| has(*k)) {
                if ["smooth", "lock", "rotation"].iter().any(|k| has(*k)) { return Err("smooth/lock/rotation belong to the classic engine; use smoothness/mode/method".into()); }
                let o = crate::stabilize::opts_from_args(a)?; let (rows, rep) = crate::stabilize::stabilize_clip(&ss.project, &id, &o)?; crate::stabilize::write_keys(&mut ss.project, &id, &o, &rows)?; return Ok(rep); }
            let off = crate::tracking::stabilize(&ss.project, &id, opt_num(a, "smooth", 10.0).clamp(0.0, 300.0), a.get("lock").and_then(|v| v.as_bool()).unwrap_or(false), a.get("rotation").and_then(|v| v.as_bool()).unwrap_or(true))?;
            let c = ss.project.find_clip_mut(&id).unwrap(); let (mut kx, mut ky, mut kr, mut ks) = (Prop::new(0.0), Prop::new(0.0), Prop::new(0.0), Prop::new(1.0)); let (mut mx, mut mr, mut mz) = (0f64, 0f64, 1f64);
            for s_ in &off { kx.set_key(s_.t, s_.dx, Ease::Linear); ky.set_key(s_.t, s_.dy, Ease::Linear); kr.set_key(s_.t, s_.rot_deg, Ease::Linear); ks.set_key(s_.t, s_.scale, Ease::Linear); mx = mx.max(s_.dx.abs()).max(s_.dy.abs()); mr = mr.max(s_.rot_deg.abs()); mz = mz.max(s_.scale); }
            c.props.insert("x".into(), kx); c.props.insert("y".into(), ky); c.props.insert("rotation".into(), kr); c.props.insert("scale".into(), ks); Ok(json!({"frames":off.len(),"max_translation_px":mx,"max_rotation_deg":mr,"max_zoom":mz})) } },
    Command { name: "list_transitions", description: "List available transition kinds.", mutates: false, schema: obj(vec![], &[]),
        run: |_, _| Ok(json!(crate::transitions::KINDS)) },
    Command { name: "import_srt", description: "Create caption text clips from an SRT file on a track. Style args apply to every caption. Each cue becomes a text clip at its time range.", mutates: true,
        schema: obj(vec![("track_id", s("track")), ("path", s("SRT file path")), ("size", n("px, default 48")), ("color", col_schema("", false)), ("font", s("ttf path")), ("stroke_width", n("px")), ("y", n("vertical offset px, default 220 (lower third)"))], &["track_id","path"]),
        run: |ss, a| { let txt = std::fs::read_to_string(str_arg(a, "path")?).map_err(|e| e.to_string())?; let tid = str_arg(a, "track_id")?.to_string(); let mut made = vec![];
            for blk in txt.replace("\r", "").split("\n\n") { let ls: Vec<&str> = blk.lines().filter(|l| !l.trim().is_empty()).collect(); let ti = ls.iter().position(|l| l.contains("-->")); let Some(ti) = ti else { continue };
                let pt = |s: &str| -> f64 { let s = s.trim().replace(',', "."); let p: Vec<f64> = s.split(':').filter_map(|x| x.parse().ok()).collect(); if p.len() == 3 { p[0]*3600.0 + p[1]*60.0 + p[2] } else { 0.0 } };
                let mut it = ls[ti].split("-->"); let (st, en) = (pt(it.next().unwrap_or("")), pt(it.next().unwrap_or(""))); if en <= st { continue; }
                let body = ls[ti + 1..].join("\\n"); let id = ss.project.gen_id("clip");
                let src = Source::Text { text: body.replace("\\n", "\n"), font: a.get("font").and_then(|v| v.as_str()).unwrap_or("/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf").into(), size: opt_num(a, "size", 48.0), color: color(a, "color", [1.0; 3])?, letter_spacing: 0.0, stroke_width: opt_num(a, "stroke_width", 2.0), stroke_color: [0.0; 3], shadow: 2.0, align: "center".into() };
                let mut props = default_props(); props.get_mut("y").unwrap().base = opt_num(a, "y", 220.0);
                let c = Clip { id: id.clone(), source: src, start: st, duration: en - st, in_point: 0.0, speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props, grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None };
                ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or("no such track")?.clips.push(std::sync::Arc::new(c)); made.push(id); }
            Ok(json!({"captions":made.len(),"clip_ids":made})) } },
    Command { name: "get_scopes", description: "Video scopes at a time: histogram, luma stats, clipping, plus optional brightness/colourfulness percentiles and per-zone blown-out/crushed share. Histogram per channel (32 bins), average color and a coarse luma waveform, for AI color grading feedback. Optional percentiles (0..100, ascending) add luma, chroma and per-column waveform percentiles plus Cb/Cr means; zones [cols, rows] add per-zone crushed/clipped % at crush_below / clip_above. Raw numbers, no verdict.", mutates: false,
        schema: obj(vec![("time", n("seconds")), ("percentiles", json!({"type":"array","items":{"type":"number"},"minItems":1,"maxItems":32,"description":"0..100, strictly ascending; adds luma_percentiles, chroma_percentiles, waveform_percentiles, cb_mean, cr_mean, chroma_mean"})), ("waveform_columns", json!({"type":"integer","description":"1..256 columns for waveform_percentiles (default 16, or the frame width when narrower); needs percentiles"})), ("zones", json!({"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"[cols, rows] 1..32 each; adds per-zone crushed/clipped %"})), ("crush_below", json!({"type":"number","description":"0..1 luma, default 0.02; needs zones"})), ("clip_above", json!({"type":"number","description":"0..1 luma, default 0.98; needs zones"}))], &["time"]),
        run: |ss, a| { let numeric = crate::ext::scopes_numeric::parse(a)?; let tt = num_arg(a, "time")?; render::prepare_analysis_range(&ss.project, &mut ss.cache, tt, tt)?; let f = crate::framecache::render_cached(&mut ss.fcache, &ss.project, tt, &ss.cache)?;
            let n = f.w * f.h; let mut hist = [[0u32; 32]; 3]; let (mut sum, mut lo, mut hi) = ([0f64; 3], 0u32, 0u32); let mut lsum = 0f64; let mut lsq = 0f64;
            for i in 0..n { let mut l = 0f32; for j in 0..3 { let v = f.d[i*4+j].clamp(0.0, 1.0); hist[j][((v * 31.999) as usize).min(31)] += 1; sum[j] += v as f64; l += v * [0.2126, 0.7152, 0.0722][j]; } lsum += l as f64; lsq += (l * l) as f64; if l < 0.02 { lo += 1; } if l > 0.98 { hi += 1; } }
            let mean = lsum / n as f64; let cols = 16; let mut wf = vec![]; for cx in 0..cols { let (mut mn, mut mx) = (1f32, 0f32); for y in (0..f.h).step_by(4) { for x in (cx * f.w / cols..(cx + 1) * f.w / cols).step_by(4) { let i = y * f.w + x; let l = 0.2126*f.d[i*4] + 0.7152*f.d[i*4+1] + 0.0722*f.d[i*4+2]; mn = mn.min(l); mx = mx.max(l); } } wf.push(json!([mn, mx])); }
            let out = json!({"avg_rgb":[sum[0]/n as f64, sum[1]/n as f64, sum[2]/n as f64],"luma_mean":mean,"luma_std":(lsq/n as f64 - mean*mean).max(0.0).sqrt(),"crushed_blacks_pct":lo as f64*100.0/n as f64,"clipped_whites_pct":hi as f64*100.0/n as f64,"histogram_r":hist[0],"histogram_g":hist[1],"histogram_b":hist[2],"luma_waveform_min_max_per_column":wf});
            let mut out = out; if let Some(o) = numeric { crate::ext::scopes_numeric::extend(&o, &f.d, f.w, f.h, &mut out)?; } Ok(out) } },
    Command { name: "add_particles", description: "Add a procedural particle layer clip: kind sparks|embers|snow|rain|dust. Transparent background; blend with add/screen for glow. Combine with glow effect.", mutates: true,
        schema: obj(vec![("track_id", s("track")), ("kind", json!({"type":"string","enum":["sparks","embers","snow","rain","dust"]})), ("start", n("s")), ("duration", n("s")), ("count", n("particles, default 200")), ("color", col_schema("", false)), ("size", n("px radius, default 3")), ("seed", n("random seed"))], &["track_id","kind","start","duration"]),
        run: |ss, a| { let tid = str_arg(a, "track_id")?.to_string(); let id = ss.project.gen_id("clip");
            let src = Source::Particles { kind: str_arg(a, "kind")?.into(), count: opt_num(a, "count", 200.0) as u32, color: color(a, "color", [1.0, 0.7, 0.3])?, size: opt_num(a, "size", 3.0), seed: opt_num(a, "seed", 1.0) as u64 };
            let mut c = Clip { id: id.clone(), source: src, start: num_arg(a, "start")?, duration: num_arg(a, "duration")?, in_point: 0.0, speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props: default_props(), grade_graph: None, effects: vec![], blend: "add".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None }; c.blend = "add".into();
            ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or("no such track")?.clips.push(std::sync::Arc::new(c)); Ok(json!({"clip_id":id})) } },
    Command { name: "add_emitter", description: "Particle emitter layer (deterministic, scrub-safe). shape point|line|circle|rect|ring|layer (spawn on another clip's alpha silhouette: emit_clip + emit_edge for outline only + emit_follow to spawn on the silhouette at each particle's own birth time, 0.1 s bins) at x,y (0..1) sized width,height (frame fractions); rate/s, life (+life_var), speed (+speed_var px/s at 1080p), angle (270=up) +- spread, gravity, wind, drag, turbulence (+turb_scale), attractor (x,y,strength), floor+bounce (+friction), colliders [{kind: circle (x,y,r) | rect (x,y,w,h) | plane (x,y,angle of outward normal) | layer (clip_id: that clip's alpha silhouette, threshold 0.5; held at the current frame, or follow:true to use the silhouette at each particle's own time in 0.1 s bins)}] that particles bounce off, vortex (+vortex_x, vortex_y swirl force), size0->size1 over life (+size_var), color0->color1, alpha0->alpha1, trail (0..12), sprite soft|hard|streak|ring|star|spark|mote|puff|card, prewarm (already running at t=0), additive, seed. Use presets via `preset`: fire, sparks, snow, rain, smoke, confetti, fireflies, bokeh, magic_dust, explosion, bubbles, foam, card_dance (card_dance uses the `card` sprite kind: a flat rectangle whose width flips through a cosine phase of time+seed, the 2D stand-in for a 3D card flip).", mutates: true,
        schema: obj(vec![("track_id", s("track")), ("start", n("s")), ("duration", n("s")), ("preset", s("optional preset name")), ("config", json!({"type":"object","description":"Emitter fields overriding the preset"}))], &["track_id","start","duration"]),
        run: |ss, a| { let tid = str_arg(a, "track_id")?.to_string(); let mut cfg = match a.get("preset").and_then(|v| v.as_str()) { Some(p) => crate::particles::preset(p)?, None => json!({}) };
            if let Some(o) = a.get("config").and_then(|v| v.as_object()) { for (k, v) in o { cfg[k] = v.clone(); } }
            let e: crate::particles::Emitter = serde_json::from_value(cfg.clone()).map_err(|e| format!("bad emitter config: {e}"))?; let additive = e.additive; let id = ss.project.gen_id("clip");
            let mut c = Clip { id: id.clone(), source: Source::Emitter(e), start: num_arg(a, "start")?, duration: num_arg(a, "duration")?, in_point: 0.0, speed: 1.0, speed_curve: None, time_remap: None, speed_blend: Default::default(), props: default_props(), grade_graph: None, effects: vec![], blend: "normal".into(), mask: None, time_mode: TimeMode::Normal, corners: None, audio: Audio::default(), motion_blur: 0.0, echo: None, motion_path: None, posterize_fps: 0.0, text_fx: None, parent: None, inherit: Default::default(), name: None, role: None, tags: vec![], transition_in: None };
            if additive { c.blend = "add".into(); }
            ss.project.tracks.iter_mut().find(|t| t.id == tid).ok_or("no such track")?.clips.push(std::sync::Arc::new(c)); Ok(json!({"clip_id":id,"config":cfg})) } },
    ]
}
