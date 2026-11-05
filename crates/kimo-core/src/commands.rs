//! Command registry: single source of truth for every editor operation.
//! The MCP server lists and dispatches straight from `registry()`.
use crate::effects::CATALOG;
use crate::effects2::CATALOG2;
use crate::color::catalog3;
use crate::keyframe::{Ease, Prop};
use crate::model::*;
use crate::render;
use serde_json::{json, Value};
mod tools_0;
mod tools_1;
mod tools_2;
mod tools_3;
mod tools_4;

pub fn fnv64(b: &[u8]) -> u64 { let mut h = 0xcbf29ce484222325u64; for x in b { h ^= *x as u64; h = h.wrapping_mul(0x100000001b3); } h }
struct FnvWriter<W: std::io::Write> { inner: W, h: u64 }
impl<W: std::io::Write> std::io::Write for FnvWriter<W> { fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { let n = self.inner.write(b)?; for x in &b[..n] { self.h ^= *x as u64; self.h = self.h.wrapping_mul(0x100000001b3); } Ok(n) } fn flush(&mut self) -> std::io::Result<()> { self.inner.flush() } }
/// Image produced by a tool call: path on disk plus size. Never bytes: only the protocol layer encodes it. Cleared at the start of every dispatch.
#[derive(Clone, Debug)]
pub struct Media { pub path: String, pub w: u32, pub h: u32, pub mime: &'static str }
pub struct Session { pub resource_limits: crate::resource_limits::Limits, pub media: Vec<Media>, pub project: Project, pub undo: Vec<Project>, pub redo: Vec<Project>, pub cache: render::Cache, pub fcache: crate::framecache::FrameCache, pub stats: crate::ext::session_stats::Stats,
    /// One label per undo / redo entry (tool name, plus clip id when given), parallel to `undo` / `redo`.
    pub undo_labels: Vec<String>, pub redo_labels: Vec<String>,
    /// Named snapshots of the whole project: (name, project, label of the history position when taken).
    pub checkpoints: Vec<(String, Project, usize)>,
    /// Snapshot-size estimate for the history cap, refreshed every 16 mutations instead of serialising the project on every call.
    pub snap_sz: usize, pub snap_n: u32, pub snap_clips: usize,
    /// Build that first saved the loaded/current project (kept across re-saves); None until a save or a load sets it.
    pub created_with: Option<serde_json::Value>,
    /// KR2: decoded-PCM memo for audio_speech_intervals (ext/speech_intervals.rs), keyed by (target
    /// JSON, a whole-project content fingerprint). Conservative on purpose: any project edit at all
    /// changes the fingerprint and misses, so it is only ever a cache HIT when the project is
    /// byte-identical to the call that filled it (e.g. lint_edit's own dialogue-track probe-then-
    /// final pair, which runs back to back with no mutation between them) - never a stale read.
    /// Bounded to a handful of entries; cleared rather than LRU-evicted past that, since a miss only
    /// costs exactly what every call cost before this cache existed.
    pub audio_decode_cache: std::collections::HashMap<(String, u64), std::sync::Arc<Vec<f32>>>,
    /// Ephemeral source in/out marks per asset (FEATURES.md "source in/out marks"), set by `mark_source` and read
    /// by `edit_points`'s three-point edit when source_in/source_out are omitted. Session-only, like a real NLE's
    /// source-viewer marks: never serialized into the saved project (Session is not Serialize at all), cleared by
    /// `clear_source_marks`, and simply absent (so unused) for a project that never touches this feature.
    pub source_marks: std::collections::BTreeMap<String, (Option<f64>, Option<f64>)> }
impl Session { pub fn new() -> Self { Session { resource_limits: crate::resource_limits::Limits::startup(), media: vec![], project: Project::default(), undo: vec![], redo: vec![], cache: render::Cache::new(), fcache: crate::framecache::FrameCache::new(crate::framecache::default_cap()), undo_labels: vec![], redo_labels: vec![], checkpoints: vec![], snap_sz: 1 << 20, snap_n: 0, snap_clips: 0, created_with: None, stats: Default::default(), audio_decode_cache: Default::default(), source_marks: Default::default() } } }
const MAX_CHECKPOINTS: usize = 24;

#[derive(Clone)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub mutates: bool,
    pub run: fn(&mut Session, &Value) -> Result<Value, String>,
}

fn s(desc: &str) -> Value { json!({"type":"string","description":desc}) }
fn n(desc: &str) -> Value { json!({"type":"number","description":desc}) }
/// Schema of a colour argument, saying what `color` / `color_opt` take: [r,g,b] or [r,g,b,a] with values 0..1, or a hex string.
/// It said array-only before, so a client validating against the schema could refuse "#ff0000" before kimo saw it (task brief item c).
/// `clear` adds the [] that set_text_style reads as "clear".
pub(crate) fn col_schema(desc: &str, clear: bool) -> Value {
    let arr = if clear { json!({"type":"array","items":{"type":"number"},"maxItems":4}) } else { json!({"type":"array","items":{"type":"number"},"minItems":3,"maxItems":4}) };
    let what = if clear { "[r,g,b] or [r,g,b,a] with values 0..1, hex \"#rrggbb\" (also #rgb, #rgba, #rrggbbaa), or [] to clear" } else { "[r,g,b] or [r,g,b,a] with values 0..1, or hex \"#rrggbb\" (also #rgb, #rgba, #rrggbbaa)" };
    json!({"description": if desc.is_empty() { what.to_string() } else { format!("{what}; {desc}") }, "oneOf": [arr, {"type":"string"}]})
}
fn obj(props: Vec<(&str, Value)>, req: &[&str]) -> Value {
    let mut m = serde_json::Map::new(); for (k, v) in props { m.insert(k.into(), v); }
    json!({"type":"object","properties":m,"required":req})
}
fn str_arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> { a.get(k).and_then(|v| v.as_str()).ok_or_else(|| crate::errors::arg_err(a, k, "string")) }
fn num_arg(a: &Value, k: &str) -> Result<f64, String> { a.get(k).and_then(|v| v.as_f64()).ok_or_else(|| crate::errors::arg_err(a, k, "number")) }
fn opt_num(a: &Value, k: &str, d: f64) -> f64 { a.get(k).and_then(|v| v.as_f64()).unwrap_or(d) }
/// Alpha of a 4-value RGBA colour argument (1.0 when absent or RGB only).
fn color_alpha(a: &Value, k: &str) -> f64 { if let Some(h) = a.get(k).and_then(|v| v.as_str()).and_then(hex_rgba) { return h[3]; } a.get(k).and_then(|v| v.as_array()).filter(|x| x.len() == 4).and_then(|x| x[3].as_f64()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0)).unwrap_or(1.0) }
/// sRGB hex "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa" (case-insensitive) as 0..1 values (x/255); None for anything else.
/// Leading/trailing whitespace is ignored (" #ffffff" is plainly a colour); spaces inside the value are not.
pub(crate) fn hex_rgba(s: &str) -> Option<[f64; 4]> {
    let h = s.trim().strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let n: Vec<u8> = match h.len() {
        3 | 4 => h.chars().map(|c| { let v = c.to_digit(16).unwrap() as u8; v * 17 }).collect(),
        6 | 8 => (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect(),
        _ => return None,
    };
    Some([n[0] as f64 / 255.0, n[1] as f64 / 255.0, n[2] as f64 / 255.0, n.get(3).map_or(1.0, |a| *a as f64 / 255.0)])
}
/// A colour argument: absent or null -> the default `d`; an [r,g,b] / [r,g,b,a] array (0..1) as before; a hex string
/// "#rrggbb" (also "#rgb", "#rgba", "#rrggbbaa"). Anything else is an error naming the argument. Before this, a hex
/// string (or any non-array) silently fell back to the default, so "#ffffff" on a solid rendered black.
pub(crate) fn color(a: &Value, k: &str, d: [f64; 3]) -> Result<[f64; 3], String> { Ok(color_opt(a, k)?.map_or(d, |c| [c[0], c[1], c[2]])) }
/// The same reading with alpha kept and no default: absent or null -> None, [r,g,b] -> alpha 1. Also used by the readers that
/// once parsed arrays by hand and read a bad element as 0 (set_text_style bg / shadow_color / glow_color / gradient_*,
/// add_transition color, preview_material color; task brief item d).
pub(crate) fn color_opt(a: &Value, k: &str) -> Result<Option<[f64; 4]>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(x)) => {
            // A wrong-length array ([1,1], five values) used to fall back to the default silently, and a non-number
            // element read as 0. Both are now errors naming the argument (Naman m:48b79971).
            if x.len() != 3 && x.len() != 4 { return Err(format!("{k}: expected 3 or 4 numbers, got {}", x.len())); }
            let mut c = [0.0, 0.0, 0.0, 1.0];
            for (i, v) in x.iter().enumerate() { c[i] = v.as_f64().ok_or_else(|| format!("{k}: expected 3 or 4 numbers, element {i} is {v}"))?; }
            Ok(Some(c))
        }
        Some(Value::String(s)) => hex_rgba(s).map(Some).ok_or_else(|| format!("{k}: \"{s}\" is not a colour; argument '{k}' takes [r,g,b] with values 0..1, or hex \"#rrggbb\" (also \"#rgb\", \"#rgba\", \"#rrggbbaa\")")),
        Some(v) => Err(format!("{k}: {v} is not a colour; argument '{k}' takes [r,g,b] with values 0..1, or hex \"#rrggbb\"")),
    }
}

fn mask_stack<'a>(p: &'a mut Project, a: &Value) -> Result<&'a mut Mask, String> {
    let c = clip(p, a)?; Ok(c.mask.get_or_insert_with(|| Mask { shape: "stack".into(), props: Default::default(), invert: false, layers: vec![], matte: None }))
}
fn parse_pts(v: &Value) -> Result<Vec<crate::masks::PathPt>, String> { serde_json::from_value(v.clone()).map_err(|e| format!("bad points: {e}")) }
fn pt_arr(v: &Value) -> Vec<[f64; 2]> { v.as_array().map(|a| a.iter().filter_map(|p| { let q = p.as_array()?; Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]) }).collect()).unwrap_or_default() }
fn apply_layer_args(l: &mut crate::masks::MaskLayer, a: &Value) -> Result<(), String> {
    if let Some(o) = a.get("params").and_then(|v| v.as_object()) { for (k, v) in o { if let Some(x) = v.as_f64() { crate::masks::check_extra(&l.kind, k, x)?; } } }   // validate before any mutation
    if let Some(m) = a.get("mode").and_then(|v| v.as_str()) { if !crate::masks::MODES.contains(&m) { return Err(format!("bad mode '{m}', one of {:?}", crate::masks::MODES)); } l.mode = m.into(); }
    if let Some(b) = a.get("invert").and_then(|v| v.as_bool()) { l.invert = b; } if let Some(b) = a.get("enabled").and_then(|v| v.as_bool()) { l.enabled = b; } if let Some(b) = a.get("closed").and_then(|v| v.as_bool()) { l.closed = b; }
    if let Some(n) = a.get("name").and_then(|v| v.as_str()) { l.name = n.into(); } if let Some(n) = a.get("interp").and_then(|v| v.as_str()) { l.strings.insert("interp".into(), n.into()); } if let Some(n) = a.get("ref_clip").and_then(|v| v.as_str()) { l.strings.insert("ref_clip".into(), n.into()); }
    for k in ["feather", "expand", "opacity"] { if let Some(x) = a.get(k).and_then(|v| v.as_f64()) { l.props.insert(k.into(), Prop::new(x)); } }
    if let Some(o) = a.get("params").and_then(|v| v.as_object()) { for (k, v) in o { if let Some(x) = v.as_f64() { match l.props.get_mut(k) { Some(pr) => { pr.base = x; pr.keys.clear(); } None => { l.props.insert(k.clone(), Prop::new(x)); } } } } }
    if let Some(v) = a.get("points") { l.points = parse_pts(v)?; l.path_keys.clear(); }
    if let Some(v) = a.get("fg") { l.fg = pt_arr(v); } if let Some(v) = a.get("bg") { l.bg = pt_arr(v); }
    if let Some(g) = a.get("ngon") { let gf = |k: &str, d: f64| g.get(k).and_then(|v| v.as_f64()).unwrap_or(d); l.points = crate::masks::ngon(gf("cx", 0.5), gf("cy", 0.5), gf("r", 0.3), gf("sides", 5.0) as usize, gf("inner", 0.0), gf("rot", 0.0), gf("aspect", 16.0 / 9.0)); l.closed = true; }
    Ok(())
}

fn clip<'a>(p: &'a mut Project, a: &Value) -> Result<&'a mut Clip, String> {
    let id = str_arg(a, "clip_id")?; p.find_clip_mut(id).ok_or(format!("no clip '{id}'"))
}

pub fn registry() -> Vec<Command> { registry_ref().to_vec() }
/// The tool table, built once per process (it used to be rebuilt, schemas and all, on every call), with a name index.
fn reg_tables() -> &'static (Vec<Command>, std::collections::HashMap<&'static str, usize>) { static REG: std::sync::OnceLock<(Vec<Command>, std::collections::HashMap<&'static str, usize>)> = std::sync::OnceLock::new(); REG.get_or_init(|| { let v = build_registry(); let mut m = std::collections::HashMap::new(); for (i, c) in v.iter().enumerate() { m.entry(c.name).or_insert(i); } (v, m) }) }
pub fn registry_ref() -> &'static [Command] { &reg_tables().0 }
/// Look a tool up by name without copying the table.
pub fn find_command(name: &str) -> Option<&'static Command> { let t = reg_tables(); t.1.get(name).map(|&i| &t.0[i]) }
fn build_registry() -> Vec<Command> {
    let eff_kinds: Vec<&str> = CATALOG.iter().chain(CATALOG2.iter()).chain(catalog3().iter()).map(|c| c.0).collect();
    let mut v: Vec<Command> = Vec::new();
    v.extend(tools_0::tools(&eff_kinds));
    v.extend(tools_1::tools(&eff_kinds));
    v.extend(tools_2::tools(&eff_kinds));
    v.extend(tools_3::tools(&eff_kinds));
    v.extend(tools_4::tools(&eff_kinds));
    v.extend(crate::edit::commands()); v.extend(crate::colortools::commands()); v.push(crate::color::scope_command::command()); v.extend(crate::ext::commands()); v
}

/// Keyframe tools whose `time` is clip-local seconds and cannot be negative.
const KEYFRAME_TOOLS: [&str; 9] = ["set_keyframe", "set_keyframe_bezier", "set_mask_keyframe", "set_mask_prop_key", "set_mask_path_key", "set_animator_keyframe", "set_camera_keyframe", "set_audio_volume_key", "set_audio_pan_key"];

/// Public entry: runs the tool and unifies the wording of not-found and OS errors (unit 5 commit B).
/// U1: "@name" accepted wherever a clip or track id argument exists. Only id-shaped keys are touched (clip_id, parent_id, track_id, clip_ids and the like), never free text.
/// A plain id passes through unchanged; a bad or ambiguous name fails before anything runs (name_not_found / ambiguous_name).
fn resolve_names(ss: &Session, name: &str, args: &Value) -> Result<Option<Value>, String> {
    if name == "set_meta" || name == "find" { return Ok(None); }
    let Some(o) = args.as_object() else { return Ok(None) };
    let kind_of = |k: &str| -> Option<&'static str> { if k == "track_id" || k.ends_with("_track_id") { Some("track") } else if k == "clip_id" || k == "parent_id" || k.ends_with("_clip_id") { Some("clip") } else { None } };
    let mut out: Option<Value> = None;
    for (k, v) in o {
        let list = k == "clip_ids" || k == "track_ids";
        let kind = if list { Some(if k == "clip_ids" { "clip" } else { "track" }) } else { kind_of(k) }; let Some(kind) = kind else { continue };
        match v {
            Value::String(s) if !list && s.starts_with('@') => { let id = crate::ext::names::resolve_ref(&ss.project, kind, s)?; out.get_or_insert_with(|| args.clone())[k.as_str()] = json!(id); }
            Value::Array(a) if list && a.iter().any(|x| x.as_str().is_some_and(|s| s.starts_with('@'))) => {
                let mut ids = vec![]; for x in a { ids.push(match x.as_str() { Some(s) => json!(crate::ext::names::resolve_ref(&ss.project, kind, s)?), None => x.clone() }); } out.get_or_insert_with(|| args.clone())[k.as_str()] = Value::Array(ids); }
            _ => {}
        }
    }
    Ok(out)
}

/// Top-level call: counted by `session_stats` (calls and bytes returned per tool); nested calls made by batch are not counted separately.
pub fn dispatch(ss: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    let r = dispatch_core(ss, name, args);
    let bytes = match &r { Ok(v) => crate::ext::session_stats::json_len(v), Err(e) => e.len() };
    ss.stats.record(name, bytes, r.is_err()); r
}
fn dispatch_core(ss: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    let _resource_context=crate::resource_limits::install(ss.resource_limits);
    ss.media.clear();
    let unknown = unknown_args(name, args);
    if !unknown.is_empty() {
        if let Ok(p) = std::env::var("KIMO_AUDIT_ARGS") { use std::io::Write; if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) { for k in &unknown { let _ = writeln!(f, "{name} {k}"); } } }
        if std::env::var("KIMO_STRICT_ARGS").map(|v| v == "1").unwrap_or(false) { return Err(unknown_arg_text(name, &unknown[0])); }
    }
    if KEYFRAME_TOOLS.contains(&name) { if let Some(t) = args.get("time").and_then(|v| v.as_f64()) { if t < 0.0 { return Err(format!("time must be >= 0 (clip-local seconds), got {t}")); } } }
    // RV-22: these inputs used to fall back silently on a bad value (the renderer's default arm). Rejected at tool input only; project load and render keep the old fallback.
    let named; let args = match resolve_names(ss, name, args)? { Some(a) => { named = a; &named } None => args };
    let normalized; let args = match guard_enums(name, args)? { Some(a) => { normalized = a; &normalized } None => args };
    // a failed call also names the arguments it dropped: a misspelt or guessed name ('at' for 'start') is the usual cause,
    // and the success-only warning never reached the caller
    let mut r = dispatch_inner(ss, name, args).map_err(|m| polish_error(ss, args, m)).map_err(|m| {
        let dropped: Vec<String> = unknown.iter().filter(|k| !m.split(|ch: char| !ch.is_alphanumeric() && ch != '_').any(|w| w == k.as_str())).map(|k| unknown_arg_text(name, k)).collect();
        if dropped.is_empty() { m } else { format!("{m}; ignored {}", dropped.join("; ")) } })?;
    if !unknown.is_empty() { if let Some(o) = r.as_object_mut() { let mut w: Vec<Value> = o.get("warnings").and_then(|v| v.as_array()).cloned().unwrap_or_default(); w.extend(unknown.iter().map(|k| json!(format!("ignored {}", unknown_arg_text(name, k))))); o.insert("warnings".into(), json!(w)); } }
    Ok(r)
}

/// (tool, property, [(alias, canonical)]). The allowed set is the schema's own enum; anything the renderer accepts beyond it is in the schema or an alias here.
const ENUM_GUARDS: &[(&str, &str, &[(&str, &str)])] = &[("add_text", "align", &[("centre", "center")]), ("set_mask", "shape", &[]), ("add_particles", "kind", &[]), ("add_text_animator", "unit", &[]), ("add_text_animator", "ease", &[]),
    ("add_transition", "ease", &[]), ("add_transition", "kind", &[]), ("set_blend", "mode", &[]),
    ("stabilize", "method", &[("similarity", "position_scale_rotation")]), ("stabilize", "mode", &[]), ("stabilize", "smoothing", &[]), ("stabilize", "framing", &[]), ("animate_from_data", "ease", &[]), ("export", "encoder", &[]), ("export_preset", "encoder", &[])];
fn guard_enums(name: &str, args: &Value) -> Result<Option<Value>, String> {
    let mut out: Option<Value> = None;
    for (tool, prop, aliases) in ENUM_GUARDS { if *tool != name { continue; } let Some(v) = args.get(*prop).and_then(|v| v.as_str()) else { continue };
        let Some(c) = registry_ref().iter().find(|c| c.name == name) else { continue }; let Some(en) = c.schema.pointer(&format!("/properties/{prop}/enum")).and_then(|e| e.as_array()) else { continue };
        let vals: Vec<&str> = en.iter().filter_map(|x| x.as_str()).collect(); let lv = v.trim().to_lowercase(); let canon = aliases.iter().find(|(a, _)| *a == lv).map(|(_, c)| c.to_string()).unwrap_or(lv);
        if !vals.contains(&canon.as_str()) { return Err(format!("unknown {prop} '{v}'. Valid: {}", vals.join(", "))); }
        if canon != v { out.get_or_insert_with(|| args.clone())[*prop] = json!(canon); } }
    Ok(out)
}

/// Argument keys that the tool's schema does not declare (empty when the schema is free-form).
pub fn unknown_args(name: &str, args: &Value) -> Vec<String> {
    let (Some(obj), Some(c)) = (args.as_object(), registry_ref().iter().find(|c| c.name == name)) else { return vec![] };
    let Some(props) = c.schema.get("properties").and_then(|p| p.as_object()) else { return vec![] };
    if c.schema.get("additionalProperties").map_or(false, |v| v != &json!(false)) { return vec![]; }
    obj.keys().filter(|k| !props.contains_key(*k)).cloned().collect()
}

fn unknown_arg_text(name: &str, key: &str) -> String {
    let valid: Vec<String> = registry_ref().iter().find(|c| c.name == name).and_then(|c| c.schema.get("properties").and_then(|p| p.as_object()).map(|p| p.keys().cloned().collect())).unwrap_or_default();
    let near = crate::errors::nearest(key, &valid, 1);
    let mut t = format!("unknown argument '{key}'");
    // A clip property given as a call argument (y on add_text): name the call that sets it, not a near-miss spelling ('y' is not 'x').
    const PROPS: &[&str] = &["opacity", "x", "y", "scale", "rotation", "rot_x", "rot_y", "pos_z"];
    if PROPS.contains(&key) && name != "set_property" {
        let hint = if key == "y" { " (pixels from the canvas centre, + is down)" } else { "" };
        t.push_str(&format!(": '{key}' is a clip property, not an argument of {name}; set it after with set_property {{clip_id, property: \"{key}\", value}}{hint}"));
    } else if let Some(n) = near.first() { if crate::errors::lev_pub(key, n) <= 3 { t.push_str(&format!(" (did you mean '{n}'?)")); } }
    let shown: Vec<&str> = valid.iter().take(12).map(|x| x.as_str()).collect();
    t.push_str(&format!(" (valid: {}{})", if shown.is_empty() { "none".to_string() } else { shown.join(", ") }, if valid.len() > 12 { ", ..." } else { "" }));
    t
}

fn polish_error(ss: &Session, args: &Value, m: String) -> String {
    let bad = |suffix: &str, exists: &dyn Fn(&str) -> bool| -> Option<String> { args.as_object()?.iter().find(|(k, v)| (k.ends_with(suffix) || (suffix == "clip_id" && (*k == "clip_a" || *k == "clip_b"))) && v.as_str().map(|s| !exists(s)).unwrap_or(false)).and_then(|(_, v)| v.as_str().map(String::from)) };
    let (clip_ok, track_ok) = (|s: &str| ss.project.tracks.iter().flat_map(|t| t.clips.iter()).any(|c| c.id == s), |s: &str| ss.project.tracks.iter().any(|t| t.id == s));
    if m == "no such clip" { if let Some(id) = bad("clip_id", &clip_ok) { return format!("no such clip '{id}'"); } }
    if m == "no such track" { if let Some(id) = bad("track_id", &track_ok) { return format!("no such track '{id}'"); } }
    if let Some(id) = m.strip_prefix("no clip '").and_then(|r| r.strip_suffix('\'')) { return format!("no such clip '{id}'"); }
    if m.contains("(os error") { if let Some(p) = args.get("path").and_then(|v| v.as_str()) { if !m.contains(p) { return format!("cannot read '{p}': {m}"); } } }
    m
}

fn dispatch_inner(ss: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    let (mut r, rec) = run_core(ss, name, args)?;
    if let Some(before) = rec { record_undo(ss, before, name, args);
        // a tool may name its own history entry (`batch`): the field is consumed here and never reaches the caller
        if let Some(l) = r.as_object_mut().and_then(|o| o.remove("history_label")).and_then(|v| v.as_str().map(String::from)) { if let Some(last) = ss.undo_labels.last_mut() { *last = l; } } }
    Ok(r)
}
/// Pushes ONE undo snapshot (`before`) with its label, clears redo and enforces the history cap. The only part of `dispatch_inner` a nested call skips.
fn record_undo(ss: &mut Session, before: Project, name: &str, args: &Value) {
    let nclips: usize = ss.project.tracks.iter().map(|t| t.clips.len()).sum(); let moved = nclips.abs_diff(ss.snap_clips) * 4 > ss.snap_clips.max(4); if ss.snap_n % 16 == 0 || ss.undo.is_empty() || moved { ss.snap_clips = nclips; ss.snap_sz = serde_json::to_vec(&before).map(|v| v.len()).unwrap_or(1 << 20).max(1); } ss.snap_n = ss.snap_n.wrapping_add(1); let sz = ss.snap_sz; ss.undo.push(before); ss.redo.clear(); ss.redo_labels.clear(); ss.undo_labels.push(match args.get("clip_id").and_then(|v| v.as_str()) { Some(id) => format!("{name} {id}"), None => name.to_string() });
        // bounded history: at most 100 steps and roughly 256 MB of snapshots, so a 200-layer timeline cannot grow until the OS kills the engine
        let cap = ((256usize << 20) / sz).clamp(8, 100); if ss.undo.len() > cap { let drop = ss.undo.len() - cap; ss.undo.drain(0..drop); ss.undo_labels.drain(0..drop.min(ss.undo_labels.len())); }
}
/// Everything `dispatch_inner` does except the history record: tool lookup, sandbox limits, the (panic-safe) run, track locks, dangling-ref clearing and the
/// project size check. Returns the tool result and, when a history entry is due (mutating, ok, not no_history), the `before` snapshot for `record_undo`.
fn run_core(ss: &mut Session, name: &str, args: &Value) -> Result<(Value, Option<Project>), String> {
    let t = reg_tables(); let c = t.1.get(name).map(|&i| &t.0[i]).ok_or(format!("unknown tool '{name}'"))?;
    let l = crate::guard::limits(); let limit_args; let sanitize_limits = if name=="set_limits" {limit_args={let mut q=l.clone();q.max_num=u64::MAX as f64;q}; &limit_args} else {l}; crate::guard::sanitize(args, sanitize_limits, 0).map_err(|e| format!("rejected by sandbox: {e}"))?;
    // read-only tools skip the whole-project snapshot (it cost O(project) on every call); mutating tools still snapshot for undo and rollback
    // KIMO_AUDIT_READONLY=1: a tool marked read-only must leave the project unchanged (undo, redo, checkpoint and restore manage history themselves and are allowed).
    static AUDIT: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); let audit = !c.mutates && *AUDIT.get_or_init(|| std::env::var("KIMO_AUDIT_READONLY").map(|v| v == "1").unwrap_or(false)) && !matches!(name, "undo" | "redo" | "checkpoint" | "checkpoint_delete" | "restore");
    let fp = if audit { serde_json::to_vec(&ss.project).ok() } else { None }; let audit_before = if audit { Some(ss.project.clone()) } else { None };
    let before = if c.mutates { ss.project.clone() } else { Project::default() };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (c.run)(ss, args)));
    let mut rec: Option<Project> = None;
    let mut r = match r { Ok(r) => r, Err(e) => { if c.mutates { ss.project = before; } let m = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "internal error".into()); return Err(format!("tool '{name}' failed safely (project rolled back): {m}")); } };
    // a mutating tool whose result carries "no_history": true changed nothing (a dry run); it pushes no undo snapshot and keeps the redo stack
    let no_history = r.as_ref().ok().and_then(|v| v.get("no_history")).and_then(|b| b.as_bool()).unwrap_or(false);
    if c.mutates && r.is_ok() && !no_history { if !matches!(name, "undo" | "redo" | "restore" | "load_project" | "checkpoint") && !name.starts_with("history_") { crate::ext::blurfill_tools::sync_fills(&mut ss.project); } // linked blur fills follow their clips
        if !matches!(name, "set_track" | "undo" | "redo" | "restore" | "load_project" | "checkpoint" | "project_set" | "batch" | "recipe_apply" | "template_apply") && !name.starts_with("history_") && before.tracks.iter().any(|t| t.locked) {
        let hit = before.tracks.iter().filter(|t| t.locked).find(|bt| match ss.project.tracks.iter().find(|t| t.id == bt.id) { None => true, Some(at) => at.clips.len() != bt.clips.len() || bt.clips.iter().zip(at.clips.iter()).any(|(x, y)| !std::sync::Arc::ptr_eq(x, y) && serde_json::to_value(&**x).ok() != serde_json::to_value(&**y).ok()) }).map(|t| t.id.clone());
        if let Some(tid) = hit { ss.project = before; return Err(format!("track '{tid}' is locked: tool '{name}' would modify it; unlock with set_track locked=false")); } } let removed: Vec<String> = if before.tracks.iter().map(|t| t.clips.len()).sum::<usize>() > ss.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>() || matches!(name, "lift_range" | "remove_track") { let now: std::collections::HashSet<&str> = ss.project.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.id.as_str()).collect(); before.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.id.clone()).filter(|i| !now.contains(i.as_str())).collect() } else { vec![] }; let cleared = ss.project.clear_refs_to(&removed); if !cleared.is_empty() { if let Ok(v) = r.as_mut() { if let Some(o) = v.as_object_mut() { o.insert("cleared_refs".into(), serde_json::to_value(&cleared).unwrap_or_default()); } } } if let Err(e) = crate::guard::check_project(&ss.project, l) { ss.project = before; return Err(format!("rejected by sandbox (project rolled back): {e}")); } rec = Some(before); }
    if let (Some(fp), Some(pre)) = (fp, audit_before) { if serde_json::to_vec(&ss.project).ok().as_ref() != Some(&fp) { ss.project = pre; return Err(format!("AUDIT: read-only tool '{name}' modified the project (rolled back); mark it mutating or fix it")); } }
    r.map(|v| (v, rec))
}

/// RV-11: runs one tool inside a batch. Same validation, sanitising, enum guard, unknown-arg warnings, panic safety, track-lock check, ref clearing and size
/// check as `dispatch` (one shared code path), but NO undo snapshot, label or history-cap work: the batch records ONE entry for the whole group.
pub fn dispatch_nested(ss: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    // deliberately no `ss.media.clear()`: only the outermost dispatch resets the per-call media list, a nested call must not wipe what the outer tool pushed
    let unknown = unknown_args(name, args);
    if std::env::var("KIMO_STRICT_ARGS").map(|v| v == "1").unwrap_or(false) && !unknown.is_empty() { return Err(unknown_arg_text(name, &unknown[0])); }
    if KEYFRAME_TOOLS.contains(&name) { if let Some(t) = args.get("time").and_then(|v| v.as_f64()) { if t < 0.0 { return Err(format!("time must be >= 0 (clip-local seconds), got {t}")); } } }
    let named; let args = match resolve_names(ss, name, args)? { Some(a) => { named = a; &named } None => args };
    let normalized; let args = match guard_enums(name, args)? { Some(a) => { normalized = a; &normalized } None => args };
    let (mut r, _rec) = run_core(ss, name, args).map_err(|m| polish_error(ss, args, m))?;
    if !unknown.is_empty() { if let Some(o) = r.as_object_mut() { let mut w: Vec<Value> = o.get("warnings").and_then(|v| v.as_array()).cloned().unwrap_or_default(); w.extend(unknown.iter().map(|k| json!(format!("ignored {}", unknown_arg_text(name, k))))); o.insert("warnings".into(), json!(w)); } }
    Ok(r)
}

/// `dispatch` under a per-call cancellation token: cancelling `tok` stops this call's work (and any worker threads it spawned through `guard`) and nothing else.
pub fn dispatch_with(ss: &mut Session, name: &str, args: &Value, tok: &crate::guard::Cancel) -> Result<Value, String> { crate::guard::with_token(tok, || dispatch(ss, name, args)) }
pub fn dispatch_ex_with(ss: &mut Session, name: &str, args: &Value, tok: &crate::guard::Cancel) -> Result<Value, crate::errors::ToolError> { crate::guard::with_token(tok, || dispatch_ex(ss, name, args)) }

/// Structured variant of `dispatch`: same behaviour, same error text, plus a code, the offending parameter and a hint (unit 5 step 1).
pub fn dispatch_ex(ss: &mut Session, name: &str, args: &Value) -> Result<Value, crate::errors::ToolError> {
    dispatch(ss, name, args).map_err(|m| {
        let mut e = crate::errors::ToolError::from(m);
        match e.code {
            "unknown_tool" => { let names: Vec<String> = registry_ref().iter().map(|c| c.name.to_string()).collect(); e.hint = Some(format!("nearest tools: {}", crate::errors::nearest(name, &names, 3).join(", "))); }
            "missing_arg" => { if let Some(c) = registry_ref().iter().find(|c| c.name == name) { let req: Vec<String> = c.schema.get("required").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(); let mut h = format!("required arguments: {}", req.join(", ")); for k in unknown_args(name, args) { let r = crate::errors::nearest(&k, &req, 1); if let Some(n) = r.first() { if crate::errors::lev_pub(&k, n) <= 3 && e.param.as_deref() == Some(n.as_str()) { h.push_str(&format!("; did you mean '{n}' instead of '{k}'?")); } } } e.hint = Some(h); } }
            "not_found" => {
                let lower = e.message.to_lowercase(); let (kind, ids): (&str, Vec<String>) = if lower.contains("track") { ("track", ss.project.tracks.iter().map(|t| t.id.clone()).collect()) } else if lower.contains("clip") { ("clip", ss.project.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.id.clone()).collect()) } else { ("", vec![]) };
                if !kind.is_empty() { let key = args.as_object().and_then(|o| o.iter().find(|(k, v)| k.ends_with(&format!("{kind}_id")) && v.as_str().map(|s| !ids.iter().any(|i| i == s)).unwrap_or(false)).map(|(k, _)| k.clone())); e.param = key; e.hint = Some(format!("valid {kind} ids (up to 10): {}", ids.iter().take(10).cloned().collect::<Vec<_>>().join(", "))); }
            }
            "invalid_value" => {
                if let Some(r) = e.message.split("Valid: ").nth(1) { e.param = Some(if e.message.starts_with("unknown shape") { "shape".into() } else if e.message.starts_with("unknown clip property") || e.message.starts_with("unknown property") { "property".into() } else { "value".into() }); e.hint = Some(format!("valid values: {}", r.trim_end_matches('.'))); }
                else if let Some(k) = e.message.strip_prefix("unknown effect '").and_then(|r| r.split('\'').next()).map(String::from) {
                    let kinds: Vec<String> = dispatch(ss, "list_effects", &json!({})).ok().and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.get("kind").and_then(|k| k.as_str()).map(String::from)).collect())).unwrap_or_default(); e.param = Some("kind".into()); e.allowed_total = Some(kinds.len()); e.hint = Some(format!("nearest effect kinds: {}. list_effects returns all {} kinds", crate::errors::nearest(&k, &kinds, 3).join(", "), kinds.len())); }
            }
            _ => {}
        }
        // RV-22: when a string argument is outside the schema's own enum, name the allowed values (generated from the same enum the schema advertises, never a hand list), with a did-you-mean. Message text is unchanged.
        if matches!(e.code, "invalid_value" | "other" | "out_of_range" | "invalid_method" | "invalid_metric" | "invalid_profile") && e.allowed.is_none() { if let (Some(c), Some(obj)) = (registry_ref().iter().find(|c| c.name == name), args.as_object()) {
            if let Some(props) = c.schema.get("properties").and_then(|p| p.as_object()) { for (pn, p) in props { let (Some(en), Some(v)) = (p.get("enum").and_then(|x| x.as_array()), obj.get(pn).and_then(|v| v.as_str())) else { continue };
                let vals: Vec<String> = en.iter().filter_map(|x| x.as_str().map(String::from)).collect(); if vals.is_empty() || vals.iter().any(|x| x == v) { continue; }
                let total = vals.len(); let near = crate::errors::nearest(v, &vals, 1); let mut hint = format!("valid values for '{pn}' ({total}): {}", vals.iter().take(40).cloned().collect::<Vec<_>>().join(", ")); if total > 40 { hint.push_str(", ... (first 40 shown)"); }
                if let Some(n) = near.first() { if crate::errors::lev_pub(v, n) <= (v.len() / 2).max(2) { hint.push_str(&format!(". Did you mean '{n}'?")); } }
                let ours = ENUM_GUARDS.iter().any(|(t, p, _)| *t == name && *p == pn.as_str()) && e.message.starts_with(&format!("unknown {pn} '")); if ours || e.param.is_none() { e.param = Some(pn.clone()); } if ours || e.hint.is_none() { e.hint = Some(hint); } e.allowed = Some(vals.iter().take(40).cloned().collect()); e.allowed_total = Some(total); break; } } } }
        e
    })
}

#[cfg(test)]
mod color_arg_tests {
    use super::{color, color_alpha};
    use serde_json::json;
    #[test] fn hex_forms_read_as_srgb_bytes_over_255() {
        assert_eq!(color(&json!({"c": "#ffffff"}), "c", [0.0; 3]).unwrap(), [1.0, 1.0, 1.0]);
        assert_eq!(color(&json!({"c": "#FFF"}), "c", [0.0; 3]).unwrap(), [1.0, 1.0, 1.0]);
        assert_eq!(color(&json!({"c": "#336699"}), "c", [0.0; 3]).unwrap(), [51.0 / 255.0, 102.0 / 255.0, 153.0 / 255.0]);
        assert_eq!(color(&json!({"c": "#33669980"}), "c", [0.0; 3]).unwrap(), [51.0 / 255.0, 102.0 / 255.0, 153.0 / 255.0]);
        assert_eq!(color_alpha(&json!({"c": "#33669980"}), "c"), 128.0 / 255.0);
        assert_eq!(color_alpha(&json!({"c": "#336699"}), "c"), 1.0);
    }
    #[test] fn short_hex_expands_and_hex_alpha_is_the_4th_value_like_the_list_form() {
        assert_eq!(color(&json!({"c": "#369"}), "c", [0.0; 3]).unwrap(), color(&json!({"c": "#336699"}), "c", [0.0; 3]).unwrap());
        let list = json!({"c": [51.0 / 255.0, 102.0 / 255.0, 153.0 / 255.0, 128.0 / 255.0]});
        assert_eq!(color(&json!({"c": "#33669980"}), "c", [0.0; 3]).unwrap(), color(&list, "c", [0.0; 3]).unwrap());
        assert_eq!(color_alpha(&json!({"c": "#33669980"}), "c"), color_alpha(&list, "c"));
        assert_eq!(color_alpha(&json!({"c": "#000000ff"}), "c"), 1.0); assert_eq!(color_alpha(&json!({"c": "#00000000"}), "c"), 0.0);
    }
    #[test] fn arrays_absent_and_null_read_exactly_as_before() {
        assert_eq!(color(&json!({"c": [1, 0.5, 0]}), "c", [0.0; 3]).unwrap(), [1.0, 0.5, 0.0]);
        assert_eq!(color(&json!({"c": [1, 0.5, 0, 0.25]}), "c", [0.0; 3]).unwrap(), [1.0, 0.5, 0.0]);
        assert_eq!(color_alpha(&json!({"c": [1, 0.5, 0, 0.25]}), "c"), 0.25);
        assert_eq!(color(&json!({}), "c", [0.2; 3]).unwrap(), [0.2; 3]);
        assert_eq!(color(&json!({"c": null}), "c", [0.2; 3]).unwrap(), [0.2; 3]);
        // v6-all merge: a wrong-length array is an error naming the key (v6 colour-argument rule, tests/color_array_length.rs)
        assert!(color(&json!({"c": [1, 2]}), "c", [0.2; 3]).unwrap_err().contains("c: expected 3 or 4 numbers, got 2"));
    }
    #[test] fn a_string_that_is_not_hex_is_refused_by_name() {
        for s in ["white", "#fffff", "#gggggg", "ffffff", ""] {   // "#ffff" is the v6 "#rgba" form, so 5 digits stands in for a bad length
            let e = color(&json!({"color": s}), "color", [0.0; 3]).unwrap_err();
            assert!(e.contains("'color'") && e.contains("#rrggbb"), "{s}: {e}");
        }
    }
}
