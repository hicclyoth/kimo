//! RV-7: `Project::external_refs` is the single source of truth for file paths held by a project.
//! 1 fixture using every path-bearing feature -> exact expected table; 2 reflection guard: every "/refs/..." string in the serialized project is reported;
//! 3 set_ref round-trips every locator; 4 source-scan guard: a new path-ish serde field / effect string key that is not registered fails this test;
//! 5 resolve_path rules.
use kimo_core::deps::{asset_kind, resolve_path, How, RefKind};
use kimo_core::model::Project;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn fixture() -> Project {
    let mut p = Project::default();
    let clip = |id: &str, source: Value| json!({"id": id, "source": source, "start": 0.0, "duration": 1.0, "in_point": 0.0, "speed": 1.0, "props": {}, "effects": [], "blend": "normal"});
    let mut v = serde_json::to_value(&p).unwrap();
    v["assets"] = json!({
        "v1": {"id": "v1", "path": "/refs/media/v1.mp4", "width": 64, "height": 36, "duration": 1.0, "fps": 25.0},
        "v2": {"id": "v2", "path": "/refs/media/sub dir/v2 \u{00e9}.mov", "width": 64, "height": 36, "duration": 1.0, "fps": 25.0},
        "i1": {"id": "i1", "path": "/refs/img/i1.png", "width": 64, "height": 36, "duration": 0.0, "fps": 0.0},
        "i2": {"id": "i2", "path": "refs_rel/i2.JPG", "width": 64, "height": 36, "duration": 0.0, "fps": 0.0},
        "a1": {"id": "a1", "path": "/refs/audio/a1.wav", "width": 0, "height": 0, "duration": 2.0, "fps": 0.0}});
    let mut shape = clip("shape1", json!({"type": "shape", "shape": "rect", "width": 10.0, "height": 10.0, "fill": [1.0, 1.0, 1.0], "stroke_width": 0.0, "stroke_color": [0.0, 0.0, 0.0], "sides": 4, "corner_radius": 0.0}));
    shape["effects"] = json!([
        {"id": "e1", "kind": "lut", "params": {}, "strings": {"path": "/refs/luts/look.cube"}},
        {"id": "e2", "kind": "lut", "params": {}, "strings": {"cube_text": "TITLE x"}},
        {"id": "e3", "kind": "ocio_transform", "params": {}, "strings": {"config_path": "/refs/ocio/config.ocio"}},
        {"id": "e4", "kind": "blur", "params": {}, "strings": {"path": "/not/a/ref/because/not/lut"}}]);
    shape["mask"] = json!({"shape": "stack", "props": {}, "invert": false, "layers": [{"id": "m1", "kind": "matte", "strings": {"dir": "/refs/matte/shot1"}}]});
    shape["grade_graph"] = json!({"nodes": {"n1": {"id": "n1", "kind": "effect", "effect": {"id": "ge", "kind": "lut", "params": {}, "strings": {"path": "/refs/luts/node.cube"}}, "mask": {"id": "nm", "kind": "matte", "strings": {"dir": "/refs/matte/node"}}}}, "output": "n1"});
    let text_file = clip("text1", json!({"type": "text", "text": "hi", "font": "/refs/fonts/Custom.ttf", "size": 40.0, "color": [1.0, 1.0, 1.0], "letter_spacing": 0.0, "stroke_width": 0.0, "stroke_color": [0.0, 0.0, 0.0], "shadow": 0.0, "align": "center"}));
    let text_family = clip("text2", json!({"type": "text", "text": "hi", "font": "Inter", "size": 40.0, "color": [1.0, 1.0, 1.0], "letter_spacing": 0.0, "stroke_width": 0.0, "stroke_color": [0.0, 0.0, 0.0], "shadow": 0.0, "align": "center"}));
    let nested_text = clip("text3", json!({"type": "text", "text": "in", "font": "/refs/fonts/Nested.otf", "size": 40.0, "color": [1.0, 1.0, 1.0], "letter_spacing": 0.0, "stroke_width": 0.0, "stroke_color": [0.0, 0.0, 0.0], "shadow": 0.0, "align": "center"}));
    let precomp = clip("pre1", json!({"type": "precomp", "tracks": [{"id": "ptrack", "clips": [nested_text]}]}));
    let shards = clip("shards1", json!({"type": "shard_burst", "plane": {"occlusion": "/refs/occ/occ_%04d.png", "texture": "/refs/img/tex.png"}}));
    let code = clip("code1", json!({"type": "code", "dir": "/refs/code/orbit", "entry": "index.html", "width": 64, "height": 36}));
    v["tracks"] = json!([{"id": "t1", "clips": [shape, text_file, text_family, precomp, shards, code]}]);
    v["occluders"] = json!({"occ1": {"src": "shape1", "src_start": 0.0, "path": "/refs/media/v1.mp4", "src_time": 0.0, "q0": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], "soft": 0.0, "chroma": 0.0, "w": 4, "h": 4}});
    v["ocio_view_delivery"] = json!({"config_path": "/refs/ocio/delivery.ocio", "source": "a", "display": "b", "view": "c", "input_gamut": 0, "output_gamut": 0, "output_transfer": 0});
    p = serde_json::from_value(v).unwrap_or_else(|e| panic!("fixture does not deserialize: {e}"));
    p
}

#[test]
fn table_is_exact() {
    let p = fixture(); let got: Vec<(String, String, String)> = p.external_refs().into_iter().map(|r| (format!("{:?}", r.kind), r.field, r.path)).collect();
    let want: Vec<(&str, &str, &str)> = vec![
        ("Audio", "assets[a1].path", "/refs/audio/a1.wav"), ("Image", "assets[i1].path", "/refs/img/i1.png"), ("Image", "assets[i2].path", "refs_rel/i2.JPG"),
        ("Media", "assets[v1].path", "/refs/media/v1.mp4"), ("Media", "assets[v2].path", "/refs/media/sub dir/v2 \u{00e9}.mov"),
        ("Media", "occluders[occ1].path", "/refs/media/v1.mp4"), ("Other", "ocio_view_delivery.config_path", "/refs/ocio/delivery.ocio"),
        ("Lut", "clip[shape1].effects[0].strings.path", "/refs/luts/look.cube"), ("Other", "clip[shape1].effects[2].strings.config_path", "/refs/ocio/config.ocio"),
        ("Other", "clip[shape1].mask.layers[0].strings.dir", "/refs/matte/shot1"),
        ("Lut", "clip[shape1].grade_graph.nodes[n1].effect.strings.path", "/refs/luts/node.cube"), ("Other", "clip[shape1].grade_graph.nodes[n1].mask.strings.dir", "/refs/matte/node"),
        ("Font", "clip[text1].source.font", "/refs/fonts/Custom.ttf"), ("Font", "clip[text2].source.font", "Inter"), ("Font", "clip[text3].source.font", "/refs/fonts/Nested.otf"),
        ("Occlusion", "clip[shards1].source.plane.occlusion", "/refs/occ/occ_%04d.png"), ("Image", "clip[shards1].source.plane.texture", "/refs/img/tex.png"),
        ("Other", "clip[code1].source.dir", "/refs/code/orbit")];
    let want: Vec<(String, String, String)> = want.into_iter().map(|(a, b, c)| (a.into(), b.into(), c.into())).collect();
    assert_eq!(got, want);
    let fam: Vec<_> = p.external_refs().into_iter().filter(|r| r.family).map(|r| r.path).collect(); assert_eq!(fam, vec!["Inter"]);
}

#[test]
fn reflection_every_ref_string_is_reported() {
    let p = fixture(); let v = serde_json::to_value(&p).unwrap();
    fn walk(v: &Value, out: &mut BTreeSet<String>) { match v { Value::String(s) if s.starts_with("/refs/") || s.starts_with("refs_rel/") => { out.insert(s.clone()); } Value::Array(a) => a.iter().for_each(|x| walk(x, out)), Value::Object(o) => o.values().for_each(|x| walk(x, out)), _ => {} } }
    let mut in_json = BTreeSet::new(); walk(&v, &mut in_json);
    let reported: BTreeSet<String> = p.external_refs().into_iter().filter(|r| !r.family).map(|r| r.path).collect();
    // the only path-looking string that is deliberately NOT a reference is the `path` key of a non-lut effect
    in_json.remove("/not/a/ref/because/not/lut");
    assert_eq!(in_json, reported, "a path-bearing field is serialized but not reported by external_refs (register it in deps.rs)");
}

#[test]
fn set_ref_round_trips_every_locator_and_rejects_unknown() {
    let mut p = fixture(); let refs = p.external_refs();
    for (i, r) in refs.iter().enumerate() {
        let np = format!("/moved/{i}/{}", r.path.rsplit('/').next().unwrap()); p.set_ref(&r.field, &np).unwrap_or_else(|e| panic!("{}: {e}", r.field));
        assert_eq!(p.external_refs()[i].path, np, "{}", r.field); assert_eq!(p.external_refs()[i].field, r.field);
        p.set_ref(&r.field, &r.path).unwrap();
    }
    assert_eq!(p.external_refs(), refs);
    for bad in ["", "assets[nope].path", "assets[v1].width", "clip[ghost].source.font", "clip[shape1].effects[3].strings.path", "clip[shape1].effects[1].strings.path", "clip[shape1].source.font", "a..b", "x[unclosed"] { assert!(p.set_ref(bad, "/x").is_err(), "{bad} should be rejected"); }
}

#[test]
fn resolve_path_rules() {
    let d = std::path::Path::new("/proj/dir");
    let r = resolve_path(Some(d), "/abs/a.mp4"); assert_eq!((r.path.to_str().unwrap(), r.how), ("/abs/a.mp4", How::Abs));
    let r = resolve_path(Some(d), "media/a.mp4"); assert_eq!((r.path.to_str().unwrap(), r.how), ("/proj/dir/media/a.mp4", How::Rel));
    let r = resolve_path(Some(d), "../x/a.mp4"); assert_eq!(r.path.to_str().unwrap(), "/proj/dir/../x/a.mp4");
    let r = resolve_path(None, "a.mp4"); assert_eq!((r.path.to_str().unwrap(), r.how), ("a.mp4", How::Rel));
    for w in ["C:\\Users\\me\\a.mp4", "media\\a.mp4", "D:/x/a.mp4"] { let r = resolve_path(Some(d), w); assert_eq!((r.path.to_str().unwrap(), r.how), (w, How::WindowsStyle)); }
    assert_eq!(asset_kind("x.WAV"), RefKind::Audio); assert_eq!(asset_kind("x.exr"), RefKind::Image); assert_eq!(asset_kind("x.mkv"), RefKind::Media);
}

/// Source-scan guard. Any struct field or effect-string key whose NAME looks like a file reference must be listed here, either as a reference
/// (handled by deps.rs) or as a documented non-reference. A new `path`/`file`/`src`/`lut`/`font`/`texture`/`occlusion`/`*_path`/`*_file`/`*_dir` field fails this test
/// until it is registered, which forces it through external_refs / set_ref / collect / relink.
#[test]
fn unregistered_path_fields_fail() {
    // (file, field) -> why
    let registered: &[(&str, &str, &str)] = &[
        ("model.rs", "path", "REF Asset.path and OccSpec.path"), ("model.rs", "font", "REF Source::Text.font"),
        ("shards.rs", "occlusion", "REF Plane.occlusion"), ("shards.rs", "texture", "REF Plane.texture"),
        ("color/view_video.rs", "config_path", "REF Project.ocio_view_delivery.config_path"),
        ("proxy.rs", "path", "REF Project.proxies.entries[].path; optional preview dependency"), ("deps.rs", "path", "OUTPUT: ExtRef.path / Resolved.path"), ("ext/auto_captions.rs", "path", "OUTPUT: Src.path, a clip's media path already resolved through deps, held for one call"), ("deps.rs", "rel_path", "OUTPUT: DepRecord.rel_path, informational"), ("text.rs", "font_file", "OUTPUT: TextMetrics.font_file, the font file a measurement resolved to, informational"), ("model.rs", "src", "NOT A FILE: OccSpec.src is a clip id"), ("codeclip.rs", "dir", "REF Source::Code dir (code clip project folder, a directory)"), ("planar.rs", "src", "NOT A FILE: Anchor.src is a clip id (runtime, not serialized in the project)"),
        ("color/bake.rs", "path", "NOT A PROJECT FIELD: bake step input, per call"), ("color/bake.rs", "config_path", "NOT A PROJECT FIELD: bake step input, per call"),
        ("color/mux.rs", "dir", "NOT A PROJECT FIELD: temp transaction dir"), ("audio.rs", "path", "NOT A PROJECT FIELD: temp wav"), ("commands.rs", "path", "OUTPUT: Media.path, an image a tool call produced (cleared every dispatch), not stored in the project"),
        ("analysis_proxy.rs", "path", "OUTPUT: AnalysisProxy.path, a cached short-GOP analysis proxy file path (keyed by source path+size+mtime+fps), not stored in the project; analysis decode only, never export"),
        ("ext/export_edl.rs", "path", "NOT A PROJECT FIELD: ExEvent.path, a clip's media path already resolved through deps, held for one export call, never stored"),
    ];
    let reg_keys: &[(&str, &str)] = &[("aux_path", "REF pixel_expr/displace map resolved through deps"), ("path", "REF lut effect"), ("config_path", "REF OCIO config on any effect"), ("dir", "REF matte store on mask layers"), ("cube_text", "inline, not a file"), ("d", "SVG path data, not a file"), ("profile", "inline table"), ("mask_layer", "layer id")];
    let looks = |n: &str| { n == "path" || n == "file" || n == "src" || n == "lut" || n == "font" || n == "texture" || n == "occlusion" || n == "font_file" || n.ends_with("_path") || n.ends_with("_file") || n.ends_with("_dir") || n == "dir" };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    fn files(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) { for e in std::fs::read_dir(d).unwrap().flatten() { let p = e.path(); if p.is_dir() { files(&p, out) } else if p.extension().map_or(false, |x| x == "rs") { out.push(p) } } }
    let mut fs = vec![]; files(&root, &mut fs); let mut bad = vec![];
    for f in fs {
        let rel = f.strip_prefix(&root).unwrap().to_str().unwrap().replace('\\', "/"); let txt = std::fs::read_to_string(&f).unwrap();
        // field declarations: `name: String` / `name: Option<String>` (struct fields and enum variant fields)
        let b = txt.as_bytes(); let mut i = 0;
        while i + 1 < b.len() {
            if b[i] == b':' && b[i + 1] != b':' && (i == 0 || b[i - 1] != b':') {
                let mut s = i; while s > 0 && (b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_') { s -= 1; } let name = &txt[s..i];
                let rest = txt[i + 1..].trim_start(); let ty_ok = rest.starts_with("String") && !rest[6..].starts_with(|c: char| c.is_alphanumeric()) || rest.starts_with("Option<String>");
                let before_ok = s == 0 || !(b[s - 1] == b'.' || b[s - 1] == b'\"');
                if ty_ok && before_ok && !name.is_empty() && looks(name) && !registered.iter().any(|(rf, rn, _)| *rf == rel && *rn == name) {
                    let line = txt[..i].matches('\n').count() + 1; let ln = txt.lines().nth(line - 1).unwrap_or("");
                    if !ln.trim_start().starts_with("fn ") && !ln.contains("let ") && !ln.contains("fn ") && !ln.trim_start().starts_with("//") { bad.push(format!("{rel}:{line} field '{name}'")); }
                }
            }
            i += 1;
        }
        // effect/mask string keys: strings.get("key") / strings.contains_key("key") / strings.insert("key"
        for pat in ["strings.get(\"", "strings.contains_key(\"", "strings.insert(\"", "strings[\""] { for (pos, _) in txt.match_indices(pat) { let k: String = txt[pos + pat.len()..].chars().take_while(|c| *c != '"').collect(); if looks(&k) && !reg_keys.iter().any(|(rk, _)| *rk == k) { bad.push(format!("{rel} strings key '{k}'")); } } }
    }
    bad.sort(); bad.dedup();
    assert!(bad.is_empty(), "path-looking fields not registered in deps.rs / tests/external_refs.rs: {bad:#?}");
}

/// Open-site gate. Every file that opens a file (ffmpeg `-i`, File::open, fs::read, read_to_string, fs::metadata, image::open, Config::from_file) is listed with its
/// number of sites and whether those sites read PROJECT-DERIVED strings (then they must go through deps::resolve / resolve_str) or caller-supplied
/// paths. A new or removed site changes the count and fails this test until the new site is classified here: add the resolve call or a reason.
#[test]
fn open_sites_are_classified() {
    const SITES: &[(&str, usize, &str)] = &[
        ("proxy.rs", 3, "PROJECT-DERIVED: original and proxy paths resolved by Project::resolve before fingerprint/probe/transcode; output path resolved against project base"),
        ("analysis_proxy.rs", 3, "PROJECT-DERIVED: the source asset path resolved by Project::resolve before ffprobe keyframe-gap check / ffmpeg transcode / cache-sidecar read; output proxy path is cache-local, never stored in the project"),
        ("audio.rs", 9, "7 PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site; 2 temp: level_master's own round-trip of the mixed PCM through ffmpeg speechnorm (temp in/out wav, not stored in the project)"),
        ("codeclip.rs", 7, "3 PROJECT-DERIVED, under resolve_dir (Source::Code.dir resolved through deps): kimo.json, the folder walk and hash; 4 temp: the frame cache, the renderer's output frames and logs"),
        ("color/adjust_v2_proof.rs", 2, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/bake.rs", 2, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/cct_grade.rs", 1, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("color/cdl_file.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/cdl_xml.rs", 2, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/delivery.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/effect_domain.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/film_fast_proof.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/interchange.rs", 4, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/lut.rs", 2, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("color/mux.rs", 9, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/ocio.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/ocio_config.rs", 3, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/pipeline.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/process.rs", 8, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("color/view_video.rs", 1, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("colortools.rs", 5, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("commands/tools_0.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("commands/tools_1.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("commands/tools_4.rs", 4, "3 PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site; 1 caller path: export reads the size of the output file it just wrote"),
        ("deps.rs", 3, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("editorial.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/auto_captions.rs", 3, "2 PROJECT-DERIVED: a clip's media for its audio and sample frames, resolved through deps (Project::resolve); 1 temp: the recognizer's JSON output"),
        ("ext/add_keyword_pops.rs", 1, "no project path: synth_pop_sfx's own fixed temp WAV path for the synthesized pop sound (same bytes every call, reused, never stored in the project -- media is never committed, and this is not read from a file that was)"),
        ("ext/cdl_files.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/data_driven_v7.rs", 2, "caller path (csv_path / json_path arguments of animate_from_data), not stored in the project"),
        ("ext/delivery.rs", 5, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("ext/delivery_extras.rs", 2, "caller path (an already-exported file argument of mux_chapters, plus its own temp ffmetadata file), not stored in the project"),
        ("ext/check_environment.rs", 1, "no project path: the size of the whisper binary / model files check_environment reports (KIMO_WHISPER_BIN, KIMO_WHISPER_MODEL or the models dir); nothing is stored in the project"),
        ("ext/director_tools.rs", 5, "2 PROJECT-DERIVED (asset paths: the shot look of auto_rough_cut, the frame auto_grade measures), resolved through deps::resolve_str at the call site; 2 caller path (review_export's path argument: frame sheet and loudness), not stored in the project; 1 no project path (V7.9Q): synth_whoosh_sfx's own fixed temp WAV path for the synthesized whoosh (same bytes every call, reused, never stored in the project -- media is never committed, same pattern as add_keyword_pops.rs's synth_pop_sfx)"),
        ("ext/editorial_tools.rs", 3, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("ext/export_job.rs", 9, "caller path under the job directory or fixed OS probes, nothing stored in the project: 2 fsync opens (sync_dir, sync_file of files this module just wrote), 2 /proc reads for the job.lock owner check (hostname, pid start ticks), 1 read_json of index.json / manifest.json / job.lock in the job dir, 2 metadata calls on chunk files (reuse_check size verify, index entry bytes), 2 ffmpeg -i inputs of the join (concat list + optional audio wav, both inside the job dir)"),
        ("ext/export_presets.rs", 11, "caller path (tool argument / output / temp file / test), not stored in the project: each preset's own rawvideo stdin pipe (-i -), plus remux_faststart's stream-copy remux (-i <output path>)"),
        ("ext/ocio_tools.rs", 2, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/ocio_view.rs", 2, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/refine_matte.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/subtitle_export.rs", 5, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/templates.rs", 2, "caller path, not stored in the project: library_path argument of template_define (the library is copied into the project); v7 import_template_package reads its own package path the same way"),
        ("ext/transcript_tools.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("ext/undistort.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("pixels.rs", 2, "PROJECT-DERIVED, resolved by pixel media/decode callers; caller file reads by tools"),
        ("resource_limits.rs", 1, "fixed /proc/meminfo physical RAM source"),
        ("ext/preview_range.rs", 2, "no project path: an ffmpeg stdin pipe (-i -) fed with rendered frames, and a size read of the output file this tool just wrote (caller output path)"),
        ("ext/project_diff.rs", 1, "caller path (file:<path.kimo> revision argument), not stored in the project"),
        ("ext/export_manifest.rs", 1, "caller path (previous manifest .json), resolved through deps::resolve_str; asset hashing goes through deps::hash_file"),
        ("ext/export_variants.rs", 1, "caller path: a size read of each variant's output file it just wrote (bytes in the per-variant result), same shape as commands/tools_4.rs's export; not stored in the project"),
        ("ext/audio_samples.rs", 3, "caller raw path resolved with project.resolve; engine-owned hash asset verification and output"),
        ("ext/pixel_tools.rs", 3, "caller input path or private hash-addressed asset verification"),
        ("framecache.rs", 1, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("guard.rs", 3, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("matte.rs", 1, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("motion.rs", 1, "caller path (tool argument / output / temp file / test), not stored in the project"),
        ("planar.rs", 2, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("reframe.rs", 1, "PROJECT-DERIVED, resolved through deps (Project::resolve -> deps::resolve_path) at the call site: the clip's media decoded by auto_reframe's subject analysis"),
        ("render.rs", 17, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site; 1 caller path: check_still_path reads the first bytes of a still's output path so a still never overwrites a video"),
        ("shards.rs", 3, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("text.rs", 1, "PROJECT-DERIVED, resolved through deps (resolve / resolve_str) at the call site"),
        ("threed/csm.rs", 1, "no project path: an ffmpeg stdin pipe (-i -) fed with raw RGBA frames inside a #[cfg(test)] #[ignore] frame-dump harness; output goes to a test-supplied directory (KIMO_*_FRAMES env), nothing is read from or stored in the project (here: KIMO_CSM_FRAMES)"),
        ("threed/ingest.rs", 1, "no project path: an ffmpeg stdin pipe (-i -) fed with raw RGBA frames inside a #[cfg(test)] #[ignore] frame-dump harness; output goes to a test-supplied directory (KIMO_*_FRAMES env), nothing is read from or stored in the project (here: KIMO_PBR_FRAMES)"),
        ("threed/lit_raster.rs", 6, "no project path: an ffmpeg stdin pipe (-i -) fed with raw RGBA frames inside a #[cfg(test)] #[ignore] frame-dump harness; output goes to a test-supplied directory (KIMO_*_FRAMES env), nothing is read from or stored in the project (here: KIMO_PBR_FRAMES)"),
        ("threed/perspective.rs", 1, "no project path: an ffmpeg stdin pipe (-i -) fed with raw RGBA frames inside a #[cfg(test)] #[ignore] frame-dump harness; output goes to a test-supplied directory (KIMO_*_FRAMES env), nothing is read from or stored in the project (here: KIMO_PRIMITIVE_FRAMES)"),
        ("threed/textured_fixture.rs", 1, "no project path: an ffmpeg stdin pipe (-i -) fed with raw RGBA frames inside a #[cfg(test)] #[ignore] frame-dump harness; output goes to a test-supplied directory (KIMO_*_FRAMES env), nothing is read from or stored in the project (here: KIMO_TEXTURE_FRAMES)"),
        ("ext/import_edl.rs", 1, "caller path (the EDL/OTIO file named by import_edl's own `path` argument), not resolved through Project::resolve and not stored in the project"),
    ];
    let pat = ["\"-i\"", "File::open", "fs::read(", "read_to_string", "fs::metadata", "image::open", "from_file("];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    fn files(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) { for e in std::fs::read_dir(d).unwrap().flatten() { let p = e.path(); if p.is_dir() { files(&p, out) } else if p.extension().map_or(false, |x| x == "rs") { out.push(p) } } }
    let mut fs = vec![]; files(&root, &mut fs); let mut bad = vec![];
    for f in fs { let rel = f.strip_prefix(&root).unwrap().to_str().unwrap().replace('\\', "/"); let txt = std::fs::read_to_string(&f).unwrap(); let n: usize = pat.iter().map(|p| txt.matches(p).count()).sum();
        let want = SITES.iter().find(|s| s.0 == rel).map(|s| s.1).unwrap_or(0); if n != want { bad.push(format!("{rel}: {n} sites, classified {want}")); } }
    assert!(bad.is_empty(), "file-open sites changed; classify them in tests/external_refs.rs (resolve project-derived paths):\n{}", bad.join("\n"));
}
