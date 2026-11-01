//! Tool groups: an access layer over the flat command registry. An AI client sees ~17 entry points (one per group plus
//! search/describe) instead of every tool. Every individual tool stays callable by its own name, so nothing is removed.
//! A tool's group comes from `group_of`: exact names first, then name rules, so most new tools land in the right group on
//! their own. The unit test fails if any registry tool falls through to "extras", which makes adding a tool without a
//! group a visible build failure rather than a silent drift.
use crate::commands::{registry_ref, Command};
fn reg() -> &'static [Command] { registry_ref() }
use serde_json::{json, Value};

pub struct Group { pub name: &'static str, pub about: &'static str }
pub const GROUPS: &[Group] = &[
    Group { name: "project", about: "Project settings, media import, save/load, undo/redo, inspect the project." },
    Group { name: "timeline", about: "Tracks and clips: add, move, trim, split, ripple/roll/slip/slide, speed, markers, multicam, silence cuts." },
    Group { name: "animate", about: "Properties, keyframes and curves, expressions, motion paths and presets, camera, blend, corner/mesh pin, physics, motion blur, beat binding." },
    Group { name: "effects", about: "Video effects, transitions, adjustment layers, effect masks and effect presets." },
    Group { name: "masks", about: "Masks, roto, track mattes, matte refinement and keying." },
    Group { name: "text", about: "Text, titles, text animators and paths, captions, subtitles, fonts, transcription." },
    Group { name: "graphics", about: "Shapes, bezier paths, shape modifiers, layer styles, HUD frames, counters, detection boxes." },
    Group { name: "vfx3d", about: "3D scenes, code clips (three.js / web pages), lights and materials, relight, particles, shard bursts and fracture glass." },
    Group { name: "tracking", about: "Point/planar/mesh/feature tracking, stabilize, camera solve, lens undistort, track validation and diagnostics." },
    Group { name: "color_grade", about: "Grading: primaries, curves, wheels, node graphs, LUTs, CDL, match grade, white balance, shared grades." },
    Group { name: "color_science", about: "Color pipeline: ACES, OCIO, HDR, float and high bit depth, color management, delivery checks and film response." },
    Group { name: "scopes", about: "Scopes and measurement: waveform/vectorscope, exposure analysis, false color, gamut checks, loudness." },
    Group { name: "audio", about: "Audio levels, effects, pan, ducking, loudness normalize, beat detection, audio export." },
    Group { name: "output", about: "Render a frame or export video, GIF, alpha, presets, EXR/TIFF, LUTs, camera solves." },
    Group { name: "extras", about: "Tools not yet assigned to a group (should stay empty)." },
];

pub fn group_of(n: &str) -> &'static str {
    if n.starts_with("proxy_") { return "project"; }
    const EXACT: &[(&str, &[&str])] = &[
        ("project", &["timeline_summary", "set_meta", "find", "project_diff", "project_merge", "batch", "recipe_list", "recipe_describe", "recipe_apply", "template_define", "template_apply", "template_list", "template_describe", "template_delete", "export_template_package", "import_template_package", "project_set", "import_media", "save_project", "load_project", "undo", "redo", "history_list", "checkpoint", "restore", "checkpoint_delete", "get_project", "ext_list", "kimo_info", "check_environment", "cache_stats", "cache_clear", "cache_set_budget", "list_dependencies", "collect_project", "relink_media", "set_limits", "get_limits", "import_pixels", "read_pixels", "create_bin", "list_bins", "rename_bin", "delete_bin", "move_asset_to_bin", "find_assets", "create_subclip", "list_subclips", "delete_subclip", "add_clip_from_subclip", "set_essential_properties", "list_essential_properties", "clear_essential_properties", "register_hook", "list_hooks", "remove_hook", "run_hooks", "import_edl", "export_edl", "list_project_templates", "create_project_from_template", "import_clips", "project_stats", "organize_bins", "batch_set_meta"]),
        ("tracking", &["auto_reframe", "fit_info", "camera_project_points", "camera_clip_triangle", "set_ground_plane", "place_at_track", "export_st_map"]),
        ("output", &["lint_edit", "validate_timeline", "self_review", "apply_review_fixes", "explain_cut", "review_export", "render_frame", "preview_range", "session_stats", "lint_frame", "contact_sheet", "diff_frames", "compare_ranges", "edit_check", "saliency_map", "frozen_frames", "black_frames", "focus_series", "detect_cuts", "match_events", "describe_frame", "pick_thumbnail", "score_windows", "export_manifest", "explain_invalidation", "export", "export_job_start", "job_status", "cancel_job", "resume_job", "join_export", "export_alpha", "export_gif", "export_audio", "export_preset", "inspect_export_preset", "export_with_subtitles", "burn_in", "export_camera_solve", "export_grade_lut", "export_asc_cdl", "export_color_pipeline_lut", "set_ocio_view_delivery", "export_ocio_view_video", "export_high_bit", "export_aces2_reference_video", "render_float_tiff", "render_float_exr", "render_high_bit_still", "render_ocio_display_view", "render_aces2_reference", "compute_text_bounds", "query_safe_margins", "solve_constraints", "mux_chapters", "auto_chapters", "gpu2d_status", "gpu_gaussian_blur_frame", "gpu_color_matrix_frame"]),
        ("effects", &["remove_transition", "add_morph_cut", "add_lens_blur", "add_depth_parallax", "add_push_wipe", "add_displacement_map", "add_channel_combiner", "add_set_channels", "add_match_grain", "add_blur_fill", "sample_field", "sample_noise", "list_effects", "add_effect", "set_effect_param", "remove_effect", "list_transitions", "add_transition", "set_effect_mask", "add_adjustment_layer", "set_echo", "set_posterize_time", "add_speed_lines", "add_glitch_confetti", "add_managed_linear_effect", "set_effect_encoding", "effect_describe", "add_droste_zoom", "add_portal_mirror", "add_vertigo_zoom", "add_subject_clone", "add_melt_drip"]),
        ("masks", &["set_mask", "set_mask_keyframe", "remove_mask", "add_mask", "update_mask", "remove_mask_layer", "reorder_mask", "list_masks", "set_mask_prop_key", "set_mask_motion_blur", "set_mask_path_key", "roto_propagate", "track_mask", "set_track_matte", "set_matte", "remove_track_matte", "track_matte", "render_mask", "refine_matte"]),
        ("text", &["add_text", "set_text", "set_text_style", "add_text_animator", "set_animator_keyframe", "add_title", "add_keyword_pops", "set_text_weight", "set_text_path", "list_fonts", "import_srt", "add_captions", "add_caption_cues", "list_captions", "set_caption_style", "style_captions_batch", "edit_caption", "transcribe", "transcript_import", "transcript_get", "transcript_edit_word", "transcript_set_take", "transcript_coverage", "transcript_to_captions", "validate_captions", "transcript_edit_preview", "auto_captions", "edit_caption_words", "export_captions", "caption_from_words"]),
        ("graphics", &["add_shape", "add_pip", "add_arrow", "add_path_shape", "set_path", "path_info", "add_path_key", "clear_path_keys", "add_shape_modifier", "add_path_operator", "list_shape_modifiers", "remove_shape_modifier", "add_modifier", "list_modifiers", "remove_modifier", "list_layer_styles", "set_layer_style", "remove_layer_style", "add_box_expand", "add_detection_box", "add_hud_frame", "add_counter", "add_prompt_box", "add_watermark", "burn_in_timecode"]),
        ("vfx3d", &["add_code_clip", "set_code_clip_param", "check_code_clip_determinism", "inspect_threed_backend", "inspect_mesh_buffers", "inspect_scene_matrices", "raycast_mesh_raw", "sample_sdf_raw", "sample_pbr_raw", "sample_diffuse_sh_raw", "render_mesh_cpu_reference", "import_mesh_raw", "add_scene3d", "add_shard_burst", "add_particles", "add_emitter", "add_particles3d", "set_particles3d", "shards_project", "set_light", "light_rig", "remove_light", "list_lights", "set_material", "list_materials", "remove_material", "set_relight", "preview_material", "simulate_physics", "generate_mesh_primitive", "inspect_mesh_primitive", "render_mesh_camera_pass", "ingest_vertex_stream", "inspect_vertex_stream", "derive_vertex_tangents", "export_vertex_stream_obj", "export_vertex_stream_gltf", "compile_scene3d_descriptor", "render_scene3d_cpu", "render_scene3d_wgpu", "sample_scene3d_animation", "render_scene3d_sequence", "sample_direct_lighting", "render_vertex_stream_pbr", "query_flat_sah", "inspect_sdf_graph", "sample_sdf_graph", "contour_sdf_graph", "transform_vertex_stream", "instantiate_gltf_scene", "render_offscreen_mesh_normal", "sample_texture_material", "import_gltf_surface", "render_textured_bvh", "render_wgpu_pbr", "sample_advection_field", "render_particle_flow", "render_particle_flow_motion", "mesh_lathe", "mesh_extrude_path", "mesh_subdivide", "mesh_boolean", "set_3d_compositor", "set_3d_layer_shadows"]),
        ("color_science", &["evaluate_hlg_reference_display"]),
        ("color_grade", &["preview_subject_match", "set_color_wheels", "pick_key_color", "auto_color", "match_shots", "auto_grade", "match_shot_look", "check_color_consistency"]),
        ("scopes", &["get_scopes", "render_scope", "render_display_scope", "analyze_exposure", "analyze_exposure_sequence", "render_exposure_false_color", "render_gamut_excursions", "measure_loudness", "find_by_loudness", "analyze_color", "inspect_grade_scopes", "set_grade_scopes", "flash_series", "contrast_ratio", "text_contrast_series"]),
        ("audio", &["finish_audio", "set_audio_effect_key", "read_samples", "import_samples", "audio_filter_response", "audio_effect_describe", "audio_speech_intervals", "audio_transients", "audio_probe", "audio_spectrum", "set_audio", "set_audio_volume_key", "set_audio_pan", "set_audio_pan_key", "add_audio_effect", "remove_audio_effect", "normalize_loudness", "batch_normalize_loudness", "auto_duck", "detect_beats", "av_offset", "add_audio_clip"]),
        ("animate", &["set_property", "set_keyframe", "list_animated_properties", "set_keyframe_bezier", "audio_to_keyframes", "add_punch_ins", "add_smart_punch", "set_expression", "link_property", "get_curve", "move_key", "remove_key", "copy_curve", "simplify_curve", "smooth_curve", "set_motion_path", "remove_motion_path", "apply_motion_preset", "set_motion_blur", "set_flow_blur", "render_flow", "add_camera_shake", "motion_extract", "motion_diff", "bind_beats", "track_beats", "set_corner_pin", "set_mesh_pin", "set_blend", "set_camera", "set_camera_keyframe", "set_keyframes", "sample_property", "bake_property", "add_null", "set_parent", "get_transform", "animate_from_data"]),
        ("timeline", &["auto_edit", "auto_rough_cut", "place_broll", "cut_to_beat", "cut_to_speech", "add_track", "add_clip", "move_clip", "trim_clip", "split_clip", "delete_clip", "paste_attributes", "copy_keyframes", "set_speed", "set_speed_ramp", "set_time_remap", "beat_cut", "set_time_mode", "freeze_frame", "ripple_delete", "close_gaps", "reframe_timeline", "drop_short_segments", "ripple_trim", "roll_edit", "slip_clip", "set_split_edit", "slide_clip", "replace_clip_media", "insert_gap", "lift_range", "duplicate_clip", "loop_clip", "distribute_clips", "swap_clips", "rate_stretch", "set_track", "delete_track", "split_all_at", "scene_split", "remove_silence_cuts", "trim_filler_words", "tighten_pauses", "fit_clip_duration", "group_clips", "ungroup_clip", "add_marker", "list_markers", "remove_marker", "mark_scene_cuts", "snap_time", "list_edit_points", "add_multicam", "switch_angle", "list_multicam", "flatten_multicam", "sync_clips", "mark_source", "clear_source_marks", "get_source_marks", "edit_points", "apply_responsive_time", "make_montage_from_folder", "best_segments", "beat_sync_montage", "montage_color_match"]),
    ];
    if n.starts_with("transcript_") { return "text"; }
    for (g, names) in EXACT { if names.contains(&n) { return g; } }
    // rules for tools added later: the first match wins
    let has = |p: &[&str]| p.iter().any(|x| n.contains(x));
    if has(&["track_", "_track", "stabiliz", "camera_solve", "solve_camera", "undistort", "homography", "jitter", "residual", "pin_layer", "planar", "corner_disagreement", "error_field", "lk_", "ecc_"]) { return "tracking"; }
    if has(&["scope", "exposure", "gamut", "loudness"]) { return "scopes"; }
    if has(&["ocio", "aces", "hdr", "float", "high_bit", "color_pipeline", "color_metadata", "color_delivery", "film_response", "linear", "color_output", "color_frame", "input_color", "delivery_contract"]) { return "color_science"; }
    if has(&["grade", "color", "cdl", "lut", "node", "white_balance", "curve"]) { return "color_grade"; }
    if has(&["audio", "beat", "duck"]) { return "audio"; }
    if has(&["caption", "text", "title", "subtitle", "font", "srt"]) { return "text"; }
    if has(&["mask", "matte", "roto", "key"]) { return "masks"; }
    if has(&["scene3d", "light", "material", "particle", "emitter", "shard", "fracture", "relight", "glass"]) { return "vfx3d"; }
    if has(&["shape", "path", "hud", "layer_style", "modifier"]) { return "graphics"; }
    if has(&["effect", "transition", "adjustment"]) { return "effects"; }
    if n.starts_with("export") || n.starts_with("render") { return "output"; }
    if has(&["keyframe", "expression", "motion", "camera", "blend"]) { return "animate"; }
    if has(&["clip", "marker", "multicam", "ripple", "speed"]) { return "timeline"; }
    "extras"
}

fn first_sentence(d: &str) -> String { let e = d.find(". ").map(|i| i + 1).unwrap_or(d.len()); let s = &d[..e]; if s.len() > 150 { format!("{}...", &s[..s.char_indices().map(|c| c.0).take_while(|&i| i <= 147).last().unwrap_or(0)]) } else { s.to_string() } }

pub fn members(group: &str) -> Vec<&'static Command> { reg().iter().filter(|c| group_of(c.name) == group).collect() }

/// Short text for the compact list: cut at a word boundary so the whole description stays within `max` chars.
fn short(d: &str, max: usize) -> String { if d.len() <= max { return d.to_string(); } let cut = d[..max].rfind(' ').unwrap_or(max); format!("{}...", d[..cut].trim_end_matches(|c| c == ',' || c == ';')) }

/// Compact grouped tools/list (RV-23): one entry per non-empty group with a short description and the member count, and no action names (they cost 2x
/// 300-700 bytes per group: once in the description, once as the schema enum). The AI finds actions with search_tools or kimo_describe(group).
pub fn grouped_list() -> Vec<Value> { profile_list("grouped", &std::collections::HashSet::new()) }

fn group_entry(g: &Group) -> Value { let n = members(g.name).len();
    json!({"name": g.name, "description": format!("{} [{} actions]", short(g.about, 100), n), "inputSchema": {"type":"object","properties":{"action":{"type":"string"},"args":{"type":"object"}},"required":["action"]}}) }
fn tool_entry(c: &Command) -> Value { json!({"name": c.name, "description": c.description, "inputSchema": c.schema}) }
fn meta_tools(with_enable: bool) -> Vec<Value> {
    let mut v = vec![json!({"name":"search_tools","description":"Find a tool, effect or transition by keywords. Returns ranked matches with required args and how to call each. Call any listed name directly.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"},"group":{"type":"string"}},"required":["query"]}}),
        json!({"name":"kimo_describe","description":"Full description and argument schema of one action (action) or the action list of a group (group).","inputSchema":{"type":"object","properties":{"action":{"type":"string"},"group":{"type":"string"}}}})];
    if with_enable { v.push(json!({"name":"kimo_enable_group","description":"List every tool of a group individually from now on (the server sends notifications/tools/list_changed).","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}})); }
    v }

/// The ~25 most used tools, listed with full schemas in the `core` profile.
pub const CORE: &[&str] = &["project_set", "import_media", "save_project", "load_project", "undo", "redo", "get_project", "add_track", "add_clip", "move_clip", "trim_clip", "split_clip",
    "delete_clip", "set_property", "set_keyframe", "add_effect", "set_effect_param", "add_transition", "add_text", "set_text_style", "add_shape", "add_mask", "set_blend", "set_audio", "render_frame", "export"];

/// The `director` profile (KIMO_TOOLS=director): the short list a small model needs to cut phone footage into a short, each
/// with a one-line description written for that job. Registry descriptions stay as they are; every other tool is still
/// callable by name and found with search_tools.
pub const DIRECTOR: &[(&str, &str)] = &[
    ("project_set", "Canvas and frame rate. A 9:16 short: {width: 720, height: 1280, fps: 30}. Call first."),
    ("import_media", "Import a video or audio file by absolute path. Returns asset_id, duration, width, height."),
    ("auto_rough_cut", "Talking-head clips back to back with dead air cut out. Leave out a false start with drop [{asset_id, index}]. Returns ids."),
    ("place_broll", "B-roll over the dialogue only while someone speaks, cut on pauses, b-roll muted. Returns ids."),
    ("cut_to_beat", "Music reel: lays the music and cuts the shots on its beats. Returns ids."),
    ("stabilize", "Steady a shaky media clip. Call after its in_point and duration are final."),
    ("auto_grade", "Fix exposure and colour cast per clip (dark, blown, blue or orange shots). Default: every clip."),
    ("add_title", "Title card: {track_id, style: \"impact\", text, start, duration}. Put it on its own top track."),
    ("add_text", "Text clip: {track_id, text, start, duration, size}. Move it with set_property y (pixels from centre, + is down)."),
    ("caption_from_words", "Captions from the words you give, timed to the speech, clear of the TikTok/Reels buttons. Best: by_asset {asset_id: words}."),
    ("auto_captions", "Captions from kimo's own speech recognition: {clip_ids: [...rough-cut segments...]}. Use with no trustworthy transcript. Flags low-confidence words."),
    ("edit_caption", "Change one caption cue's text: {clip_id, text}."),
    ("finish_audio", "Final mix in one call: clean dialogue, b-roll sound off, music under speech, -14 LUFS. Call before export."),
    ("remove_transition", "Take a transition off a clip: {clip_id}. lint_edit names it when a dissolve fades in from black."),
    ("lint_edit", "Check the edit before export. Lists each problem with the exact call that fixes it. Fix, then run it again."),
    ("timeline_summary", "The edit at a glance: tracks bottom to top, each clip's id, source, start and end."),
    ("add_track", "New track on top (higher tracks draw over lower ones). Returns track_id."),
    ("add_clip", "One media clip: {track_id, source_type: \"media\", asset_id, start, duration, in_point, fit: \"cover\"}."),
    ("trim_clip", "Change a clip's in_point and/or duration."),
    ("move_clip", "Move a clip to a new start time."),
    ("delete_clip", "Remove a clip."),
    ("set_property", "Set x, y, scale, opacity or rotation on a clip (x/y in pixels from centre)."),
    ("set_audio", "A clip's sound: mute, volume (1 = unchanged), fade_in, fade_out."),
    ("render_frame", "Render the edit at one time to a .png and look at it."),
    ("contact_sheet", "Several times of the edit in one .png grid you can look at."),
    ("export", "Render the edit to an .mp4 at path (H.264, AAC). Returns duration and bytes (check bytes against a size cap)."),
    ("review_export", "Look at any video file, your export or a source clip: a grid of its frames, duration, size, loudness."),
    ("save_project", "Save the project JSON to path."),
    ("undo", "Undo the last change. A recipe call (auto_*, place_broll, finish_audio) is one step."),
    ("add_smart_punch", "Automatic subject-follow punch-in: a continuous zoom that centres on wherever the saliency map says the eye goes. {clip_id, zoom}."),
    ("add_glow_text", "A title clip with a pulsing neon glow pre-applied: one call for the caption/hook look. {track_id, text, start, duration}."),
    ("auto_reframe", "Crop a clip to the canvas aspect (e.g. 16:9 into 9:16), following its subject. The fix lint_edit names for aspect_mismatch."),
    ("add_punch_ins", "Jump-cut punch-ins: alternate a track's clips between wide and zoomed around a focus point (talking-head style)."),
];

/// Profile names: grouped (default), flat, core, director, or a comma list of group names to expand (the other groups stay collapsed). Unknown text falls back to grouped.
pub fn parse_profile(p: &str) -> String { let p = p.trim().to_lowercase(); if matches!(p.as_str(), "" | "grouped" | "flat" | "core" | "director") { return if p.is_empty() { "grouped".into() } else { p }; }
    let ok: Vec<&str> = p.split(',').map(|x| x.trim()).filter(|x| GROUPS.iter().any(|g| g.name == *x && !members(g.name).is_empty())).collect(); if ok.is_empty() { "grouped".into() } else { ok.join(",") } }

/// tools/list for a profile. `enabled` holds groups switched on with kimo_enable_group (their tools are listed individually, their group entry is dropped).
pub fn profile_list(profile: &str, enabled: &std::collections::HashSet<String>) -> Vec<Value> {
    let p = parse_profile(profile);
    match p.as_str() {
        "flat" => { let mut v: Vec<Value> = reg().iter().map(tool_entry).collect(); v.extend(meta_tools(false)); v }
        "director" => { let mut v: Vec<Value> = DIRECTOR.iter().filter_map(|(n, d)| reg().iter().find(|c| c.name == *n).map(|c| json!({"name": c.name, "description": d, "inputSchema": c.schema}))).collect(); v.extend(meta_tools(false)); v }
        "core" => { let mut v: Vec<Value> = CORE.iter().filter_map(|n| reg().iter().find(|c| c.name == *n)).map(tool_entry).collect(); v.extend(meta_tools(true)); for g in GROUPS { if enabled.contains(g.name) { v.extend(members(g.name).into_iter().filter(|c| !CORE.contains(&c.name)).map(tool_entry)); } } v }
        _ => { let mut expand: std::collections::HashSet<String> = enabled.clone(); if p != "grouped" { for g in p.split(',') { expand.insert(g.to_string()); } }
            let mut v = vec![]; for g in GROUPS { let m = members(g.name); if m.is_empty() { continue; } if expand.contains(g.name) { v.extend(m.into_iter().map(tool_entry)); } else { v.push(group_entry(g)); } }
            v.extend(meta_tools(true)); v }
    }
}

fn required_args(c: &Command) -> Vec<String> { c.schema.get("required").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() }

/// search_tools: one call from a plain-language need to a callable tool. Ranked tool hits plus effect/transition catalog hits (those are reached through add_effect / add_transition).
pub fn search_tools(q: &str, limit: usize, group: Option<&str>, effects: &Value, transitions: &Value) -> Value {
    let limit = limit.clamp(1, 25); let r = search(q, 50); let total = r["total"].as_u64().unwrap_or(0);
    let mut tools: Vec<Value> = vec![];
    for m in r["matches"].as_array().into_iter().flatten() { let name = m["action"].as_str().unwrap_or(""); let Some(c) = reg().iter().find(|c| c.name == name) else { continue }; if let Some(g) = group { if group_of(name) != g { continue; } }
        let req = required_args(c); tools.push(json!({"type":"tool","name":name,"group":group_of(name),"summary":m["summary"],"required":req,"call_as":format!("{}({})", name, req.join(", "))})); }
    let words: Vec<String> = q.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(|w| w.to_lowercase()).collect();
    let mut cat: Vec<Value> = vec![]; if group.map_or(true, |g| g == "effects") { for h in search_catalog(q, effects, transitions, 8) { let ty = h["type"].as_str().unwrap_or(""); let kind = h["kind"].as_str().unwrap_or("");
        let (tool, extra) = if ty == "transition" { ("add_transition", ", duration") } else { ("add_effect", "") };
        cat.push(json!({"type":ty,"name":kind,"group":"effects","summary":h["summary"],"call_as":format!("{tool}(clip_id{extra}, kind=\"{kind}\")")})); } }
    let matches_all = |h: &Value| { let t = format!("{} {}", h["name"].as_str().unwrap_or(""), h["summary"].as_str().unwrap_or("")).to_lowercase(); !words.is_empty() && words.iter().all(|w| t.contains(w.as_str())) };
    let mut out: Vec<Value> = vec![]; let mut rest: Vec<Value> = vec![];
    for (i, h) in cat.into_iter().enumerate() { if i == 0 && matches_all(&h) { out.push(h); } else { rest.push(h); } }
    out.extend(tools); out.extend(rest); out.truncate(limit);
    json!({"matches": out, "total": total, "note": "call any listed name directly as a tool; effects and transitions are used through add_effect / add_transition"})
}

/// Resolve a grouped call to (tool name, args). Err carries a message for the client.
pub fn resolve_group_call(group: &str, args: &Value) -> Result<(String, Value), String> {
    let Some(action) = args.get("action").and_then(|v| v.as_str()) else { return Err(format!("group '{group}' needs 'action' (one of: {})", members(group).iter().map(|c| c.name).collect::<Vec<_>>().join(", "))); };
    let Some(c) = reg().iter().find(|c| c.name == action) else { return Err(format!("unknown action '{action}' in group '{group}'")); };
    if group_of(c.name) != group { return Err(format!("action '{action}' belongs to group '{}', not '{group}'", group_of(c.name))); }
    Ok((action.to_string(), args.get("args").cloned().unwrap_or(json!({}))))
}

pub fn is_group(n: &str) -> bool { GROUPS.iter().any(|g| g.name == n && !members(n).is_empty()) }

pub fn search(q: &str, limit: usize) -> Value {
    // OR search ranked by how many query words match: name (3), argument names (2), description (1). Words are lowercased and lightly stemmed.
    const STOP: &[&str] = &["a", "an", "the", "to", "of", "for", "and", "or", "in", "on", "with", "add", "set", "make", "create"];
    let stem = |w: &str| -> String { let w = w.to_lowercase(); for suf in ["ing", "es", "s"] { if w.len() > suf.len() + 3 && w.ends_with(suf) { return w[..w.len() - suf.len()].to_string(); } } w };
    let mut words: Vec<String> = q.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).map(|w| w.to_string()).collect();
    let kept: Vec<String> = words.iter().filter(|w| !STOP.contains(&w.to_lowercase().as_str())).cloned().collect(); if !kept.is_empty() { words = kept; }
    let words: Vec<String> = words.iter().map(|w| stem(w)).collect();
    // two-word phrases: "slow motion" is about speed, "green screen key" is about chroma keying; the second word would otherwise outrank the intent
    let has = |x: &[&str]| words.iter().any(|w| x.contains(&w.to_lowercase().as_str()));
    let words: Vec<String> = { let sp = has(&["slow", "slowmo", "fast"]); let gs = has(&["green", "greenscreen", "screen", "chroma"]); words.iter().filter(|w| !(sp && w.as_str() == "motion") && !(gs && w.as_str() == "key")).cloned().collect() };
    fn alts<'a>(w: &'a str) -> Vec<&'a str> { let mut v = vec![w]; match w { "slow" | "slowmo" | "fast" => v.push("speed"), "shatter" | "break" | "smash" => { v.push("fracture"); v.push("shard"); } "speech" | "spoken" | "voice" => { v.push("transcribe"); v.push("caption"); } "green" | "greenscreen" | "screen" => { v.push("chroma"); } "shake" | "shaky" => { v.push("stabilize"); v.push("shake"); } "fade" => { v.push("volume"); v.push("opacity"); } "subtitle" => v.push("caption"), "cover" => v.push("thumbnail"), "cut" => { v.push("split"); v.push("trim"); } "um" | "uh" | "ums" | "uhs" | "stutter" | "disfluency" => v.push("filler"), "tiny" | "stray" | "false" => v.push("short"), "point" | "pointing" | "callout" | "annotate" | "annotation" => v.push("arrow"), _ => {} } v }
    let mut hits: Vec<(usize, usize, &Command)> = reg().iter().filter_map(|c| {
        let nm = c.name.to_lowercase(); let ds = c.description.to_lowercase();
        let args: String = c.schema.get("properties").and_then(|p| p.as_object()).map(|o| o.iter().map(|(k, v)| format!("{} {}", k, v.get("description").and_then(|d| d.as_str()).unwrap_or(""))).collect::<Vec<_>>().join(" ").to_lowercase()).unwrap_or_default();
        let arg_names: String = c.schema.get("properties").and_then(|p| p.as_object()).map(|o| o.keys().cloned().collect::<Vec<_>>().join(" ").to_lowercase()).unwrap_or_default();
        let (mut sc, mut matched) = (0usize, 0usize);
        let toks: Vec<&str> = nm.split('_').collect();
        for w in &words { let mut m = 0; for a in alts(w.as_str()) { let v = if toks.iter().any(|t| *t == a || stem(t) == a) { 4 } else if nm.contains(a) { 3 } else if arg_names.contains(a) { 2 } else if ds.contains(a) || args.contains(a) { 1 } else { 0 }; m = m.max(v); } if m > 0 { sc += m; matched += 1; } }
        if matched > 0 { Some((matched, sc, c)) } else { None } }).collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)).then(a.2.name.cmp(b.2.name)));
    json!({"matches": hits.iter().take(limit.clamp(1, 50)).map(|(_, _, c)| json!({"group": group_of(c.name), "action": c.name, "summary": first_sentence(c.description)})).collect::<Vec<_>>(), "total": hits.len()})
}

pub fn describe(args: &Value) -> Result<Value, String> {
    if let Some(a) = args.get("action").and_then(|v| v.as_str()) {
        let c = reg().iter().find(|c| c.name == a).ok_or_else(|| format!("unknown action '{a}'; try kimo_search"))?;
        return Ok(json!({"group": group_of(c.name), "action": c.name, "description": c.description, "mutates": c.mutates, "args_schema": enrich(c.name, c.schema.clone()), "call": {"tool": group_of(c.name), "arguments": {"action": c.name, "args": "{...per args_schema}"}}}));
    }
    if let Some(g) = args.get("group").and_then(|v| v.as_str()) {
        let gi = GROUPS.iter().find(|x| x.name == g).ok_or_else(|| format!("unknown group '{g}'; groups: {}", GROUPS.iter().map(|x| x.name).collect::<Vec<_>>().join(", ")))?;
        return Ok(json!({"group": g, "about": gi.about, "actions": members(g).iter().map(|c| json!({"action": c.name, "summary": first_sentence(c.description)})).collect::<Vec<_>>()}));
    }
    Err("pass 'action' or 'group'".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn every_tool_has_a_real_group() { let bad: Vec<_> = reg().iter().filter(|c| group_of(c.name) == "extras").map(|c| c.name).collect(); assert!(bad.is_empty(), "tools without a group, add them to toolgroups.rs: {bad:?}"); }
    #[test] fn groups_partition_the_reg() { let n: usize = GROUPS.iter().map(|g| members(g.name).len()).sum(); assert_eq!(n, reg().len()); }
    #[test] fn group_call_resolves_and_rejects_wrong_group() {
        let (n, a) = resolve_group_call("project", &json!({"action":"project_set","args":{"width":640}})).unwrap(); assert_eq!(n, "project_set"); assert_eq!(a["width"], 640);
        assert!(resolve_group_call("audio", &json!({"action":"project_set"})).is_err()); assert!(resolve_group_call("audio", &json!({})).is_err()); }
    #[test] fn grouped_list_is_small() { let l = grouped_list(); assert!(l.len() <= 20, "{}", l.len()); let bytes = serde_json::to_string(&l).unwrap().len(); assert!(bytes <= 6144, "default tools/list is {bytes} bytes, ceiling 6144"); }
    #[test] fn core_tools_exist() { for n in CORE { assert!(reg().iter().any(|c| c.name == *n), "core tool {n} missing"); } assert!(CORE.len() <= 26); }
}

/// Effects and transitions are catalogs reached through add_effect / add_transition, so tool search alone cannot find them.
/// This matches a query against those catalogs (kind, description, parameter names) and says how to use each hit.
pub fn search_catalog(q: &str, effects: &Value, transitions: &Value, limit: usize) -> Vec<Value> {
    let words: Vec<String> = q.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(|w| { let w = w.to_lowercase(); if w.len() > 4 && w.ends_with('s') { w[..w.len() - 1].to_string() } else { w } }).collect();
    let syn = |w: &str| -> Vec<&'static str> { match w { "green" | "greenscreen" | "screen" => vec!["chroma", "key"], "shatter" | "break" => vec!["fracture", "shard"], "slow" => vec!["speed"], _ => vec![] } };
    let mut out: Vec<(usize, Value)> = vec![];
    for e in effects.as_array().into_iter().flatten() {
        let kind = e["kind"].as_str().unwrap_or("").to_lowercase(); let d = e["description"].as_str().unwrap_or("").to_lowercase();
        let params: String = e["params"].as_array().into_iter().flatten().filter_map(|p| p["name"].as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
        let mut sc = 0; for w in &words { let mut ws = vec![w.as_str()]; ws.extend(syn(w)); if ws.iter().any(|x| kind.contains(x)) { sc += 3; } else if ws.iter().any(|x| d.contains(x)) { sc += 2; } else if ws.iter().any(|x| params.contains(x)) { sc += 1; } }
        if sc > 0 { out.push((sc, json!({"type": "effect", "kind": e["kind"], "summary": e["description"], "use": format!("effects/add_effect with kind=\"{}\"", kind)}))); } }
    for t in transitions.as_array().into_iter().flatten() { let n = t.as_str().unwrap_or("").to_lowercase(); if words.iter().any(|w| n.contains(w.as_str())) { out.push((3, json!({"type": "transition", "kind": n, "use": format!("effects/add_transition with kind=\"{}\"", n)}))); } }
    out.sort_by(|a, b| b.0.cmp(&a.0)); out.into_iter().take(limit).map(|x| x.1).collect()
}

/// Fills in the shape of colour arguments, which many tools take as plain arrays with no description. Only facts verified in the
/// code are added (colours are [r,g,b] or [r,g,b,a] floats in 0..1); other untyped arguments are left as they are.
fn enrich(tool: &str, mut schema: Value) -> Value {
    if let Some(props) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for (k, v) in props.iter_mut() {
            let is_color = k.contains("color") || matches!(k.as_str(), "accent" | "albedo" | "emissive");
            let note = match (tool, k.as_str()) {
                ("group_clips", "clip_ids") => Some("array of clip id strings; must not be empty"),
                ("add_mask" | "update_mask", "ngon") => Some("polygon or star for kind=path: {sides, inner, r, cx, cy, rot, aspect}; all optional (defaults sides 5, inner 0, r 0.3, cx 0.5, cy 0.5, rot 0, aspect 16/9)"),
                ("track_mesh", "corners") => Some("exactly 8 numbers: x0,y0,x1,y1,x2,y2,x3,y3 (an error is returned for any other length)"),
                ("match_grade", "source_region" | "ref_region") => Some("normalized sample rectangle [x, y, w, h], each 0..1 with x+w<=1 and y+h<=1; w,h > 0; default whole frame"),
                ("export_color_pipeline_lut" | "generate_lut_node", "domain_min" | "domain_max") => Some("3 numbers [r,g,b], the .cube DOMAIN_MIN / DOMAIN_MAX written to the LUT"),
                ("export_color_pipeline_lut" | "generate_lut_node", "steps") => Some("ordered step objects {kind, params:{name:number}, strings:{name:string}}; kind is one of the color effect names (e.g. cdl, lut, color_space, tonemap, exposure, saturation); unknown keys or kinds are rejected"),
                ("evaluate_hlg_reference_display", "scene_rgb") => Some("exactly 3 numbers [r,g,b], scene-linear"),
                ("add_multicam" | "sync_clips", "angles" | "clip_ids") => Some("array of clip id strings"),
                _ => None };
            if let Some(n) = note { if v.get("description").is_none() { v["description"] = json!(n); } }
            if is_color && v.get("type").and_then(|t| t.as_str()) == Some("array") && v.get("description").is_none() { v["description"] = json!("color [r,g,b] or [r,g,b,a], floats 0..1"); }
        }
    }
    schema
}
