use crate::keyframe::Prop;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// Media file decoded through ffmpeg.
    Media { asset: String },
    /// Flat color generator.
    Solid { color: [f64; 3] },
    /// Procedural gradient generator (c1 -> c2 vertical).
    Gradient { c1: [f64; 3], c2: [f64; 3] },
    /// Rendered text (TTF/OTF via ab_glyph). Animate with the `reveal` prop (typewriter) plus transform props.
    /// Procedural particles: kind sparks|snow|dust|embers|rain. Deterministic per seed.
    Emitter(crate::particles::Emitter),
    /// 3D glass shard burst (see shards.rs).
    ShardBurst(crate::shards::ShardBurst),
    SpeedLines(crate::speedlines::SpeedLines),
    GlitchConfetti(crate::confetti::GlitchConfetti),
    Scene3D(crate::scene3d::Scene3D),
    /// 3D particle layer (Trapcode Particular class) seen through the project camera; closed-form physics, scrub-exact (see particles3d.rs).
    #[serde(rename = "particles3d")]
    Particles3D(crate::particles3d::Particles3D),
    Particles { kind: String, count: u32, color: [f64; 3], size: f64, seed: u64 },
    /// Adjustment layer: its effects/mask/opacity apply to everything composited beneath it.
    Adjustment,
    /// Nested composition: its own tracks on a local clock (clip start = 0). Transform, effects, masks and opacity apply to the whole group.
    Precomp { tracks: Vec<Track> },
    /// Vector shape: rect | ellipse | polygon (sides) | line. Sizes in pixels, centered on canvas (offset with x/y props).
    Shape { shape: String, width: f64, height: f64, fill: [f64; 3], stroke_width: f64, stroke_color: [f64; 3], sides: u32, corner_radius: f64 },
    Text { text: String, font: String, size: f64, color: [f64; 3], letter_spacing: f64, stroke_width: f64, stroke_color: [f64; 3], shadow: f64, align: String },
    /// Code clip: a web project (index.html + JS, e.g. three.js) drawing each frame through window.renderFrame(t, params) in a sandboxed headless browser (see codeclip.rs).
    Code(crate::codeclip::CodeClip),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mask {
    /// rect | ellipse | linear (left-to-right gradient edge) ; coordinates normalized 0..1 of canvas.
    pub shape: String,
    pub props: BTreeMap<String, Prop>, // cx, cy, w, h, feather, rotation
    pub invert: bool,
    /// Full mask stack (see masks.rs). The legacy single shape above applies first when shape != "stack".
    #[serde(default)] pub layers: Vec<crate::masks::MaskLayer>,
    /// Track matte: another clip's alpha/luma gates this clip.
    #[serde(default)] pub matte: Option<crate::masks::Matte>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    pub kind: String,
    pub params: BTreeMap<String, Prop>,
    #[serde(default)]
    pub strings: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub source: Source,
    /// Position on the timeline, seconds.
    pub start: f64,
    pub duration: f64,
    /// Offset into the source media, seconds.
    pub in_point: f64,
    pub speed: f64,
    /// Optional speed ramp (multiplier over clip-local time); overrides `speed` for video.
    #[serde(default)]
    pub speed_curve: Option<Prop>,
    /// Optional time remap (source time vs clip time); overrides speed, speed ramp, reverse and freeze for video and audio.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub time_remap: Option<TimeRemap>,
    /// Premiere Time Interpolation for a speed ramp: how source times between frames are shown (off = nearest frame). Unused without speed_curve.
    #[serde(default, skip_serializing_if = "FrameBlend::is_off")] pub speed_blend: FrameBlend,
    pub props: BTreeMap<String, Prop>, // opacity, x, y, scale, rotation
    pub effects: Vec<Effect>,
    #[serde(default)] pub grade_graph: Option<crate::color::node_graph::Graph>,
    pub blend: String,
    #[serde(default)]
    pub mask: Option<Mask>,
    #[serde(default)]
    pub time_mode: TimeMode,
    #[serde(default)]
    pub corners: Option<[[f64; 2]; 4]>,
    #[serde(default)]
    pub audio: Audio,
    /// Motion blur shutter angle in degrees (0 = off, 180 = film standard).
    #[serde(default)]
    pub motion_blur: f64,
    /// Echo / temporal smear (AE Echo, CC Wide Time, frame blending).
    #[serde(default)]
    pub echo: Option<Echo>,
    #[serde(default)] pub motion_path: Option<crate::motion::MotionPath>,
    /// Posterize time: quantize this clip's local time to N fps (0 = off).
    #[serde(default)]
    pub posterize_fps: f64,
    /// Rich text styling and per-character animators (text clips only).
    #[serde(default)]
    pub text_fx: Option<TextFx>,
    /// Transition INTO this clip from the previous clip on the track.
    pub transition_in: Option<Transition>,
    /// A5: transform parent (clip id). The child's x/y/scale/rotation/opacity compose with the parent's, per `inherit`.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Inherit::is_default")] pub inherit: Inherit,    /// U1: optional project-unique handle for `@name` references, a free-form role label, and tags. All skipped when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub tags: Vec<String>,
}
/// Which parts of the parent's transform a child inherits (all on by default).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Inherit { #[serde(default = "yes")] pub position: bool, #[serde(default = "yes")] pub scale: bool, #[serde(default = "yes")] pub rotation: bool, #[serde(default = "yes")] pub opacity: bool }
fn yes() -> bool { true }
impl Default for Inherit { fn default() -> Self { Inherit { position: true, scale: true, rotation: true, opacity: true } } }
impl Inherit { pub fn is_default(&self) -> bool { *self == Inherit::default() } }

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TimeMode { #[default] Normal, Reverse, /// hold the frame at clip-local time `at`
    Freeze { at: f64 } }

/// How a time remap shows source times that fall between frames.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FrameBlend { #[default] Off, FrameMix, OpticalFlow }
impl FrameBlend { pub fn is_off(&self) -> bool { *self == FrameBlend::Off } }
/// AE-style time remap: `curve` gives source seconds after in_point as a function of clip-local seconds (+ `offset`, set by split_clip so the right half continues the curve).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeRemap { pub curve: Prop, #[serde(default)] pub offset: f64, #[serde(default)] pub blend: FrameBlend }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Audio {
    /// Split edit: seconds the audio starts BEFORE the video (J cut; negative = after, the incoming side of an L cut).
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub lead: f64,
    /// Split edit: seconds the audio runs on AFTER the video ends (L cut; negative = ends before, the outgoing side of a J cut).
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub tail: f64,
    #[serde(default)] pub effects: Vec<crate::editorial::AudioEffect>,
    /// Stereo balance, -1 left to +1 right. A `Prop` (like `volume`) so it is keyframeable via `set_audio_pan_key`;
    /// `set_audio_pan` still sets the static base. Serialization is opt-in on actually keyframing it: with no keys
    /// and no expression ops it writes back out as the old bare number (byte-identical to pre-v7 project JSON --
    /// every golden_project fixture has an un-keyframed pan, so none of them move), and only becomes the full Prop
    /// object once a key is set. Older saved projects wrote this as a bare number -- accepted on load via `de_pan`
    /// (`tests/data/v1_project.kimo`, a pre-migration fixture, proves the bare-number form still loads).
    #[serde(default = "zero_prop", deserialize_with = "de_pan", serialize_with = "ser_pan")] pub pan: Prop,
    #[serde(default = "unity_prop")] pub duck_gain: Prop, pub mute: bool, pub volume: Prop, pub fade_in: f64, pub fade_out: f64, pub pitch: f64, pub highpass: f64, pub lowpass: f64, pub denoise: f64, pub compress: bool, pub echo: f64, pub reverse: bool,
    /// Gentle automatic speech leveling (ffmpeg `speechnorm`): continuously raises quiet passages and eases down loud ones over
    /// time, narrowing loudness swings WITHIN a clip instead of a hard compressor's threshold/ratio knee -- the thing finish_audio's
    /// per-clip average-matching step never touched (it only matched each clip's average level to the loudest, leaving whatever
    /// variance already lived inside each clip exactly as it was). 0 = off (default); else the expansion/compression factor passed
    /// straight to `speechnorm` (e=c=this value), so ~1.5-2 is gentle and higher is stronger. Default-serialized away so every
    /// existing project/golden byte stays identical until something actually turns this on.
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub speech_level: f64,
    /// V7.9Q: scales the `compress` acompressor's ratio: ratio = 1.0 + 2.0*this (clamped 0..1), so 0.0 is no
    /// compression at all (filter skipped) and 1.0 reproduces the exact old fixed ratio=3 threshold=-18dB.
    /// Default (and what every pre-7.9 saved project loads as, via `one_f64` -- no "compress_amount" key in old
    /// JSON) is 1.0, so nothing changes until a caller asks for less: measured on real rough-cut interview
    /// footage, that fixed ratio=3 compressor -- not the per-clip average-matching loop -- was the dominant force
    /// holding a finished mix's loudness_range near 1.2-1.4 LU regardless of level_dynamics/speech_level. Set by
    /// finish_audio's compress_strength arg; never by hand-authored projects.
    #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")] pub compress_amount: f64 }
fn unity_prop() -> Prop { Prop::new(1.0) }
fn zero_prop() -> Prop { Prop::new(0.0) }
/// Accepts either the legacy bare-number `pan` (pre-v7 project format) or a full `Prop` object.
fn de_pan<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Prop, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PanRepr { Num(f64), Full(Prop) }
    Ok(match PanRepr::deserialize(d)? { PanRepr::Num(n) => Prop::new(n), PanRepr::Full(p) => p })
}
/// Writes the old bare-number shape while unkeyframed (opt-in migration: existing project JSON stays byte-identical
/// until pan is actually animated), the full Prop object once a key or expression op is present.
fn ser_pan<S: serde::Serializer>(p: &Prop, s: S) -> Result<S::Ok, S::Error> {
    if p.keys.is_empty() && p.expr.is_empty() { s.serialize_f64(p.base) } else { p.serialize(s) }
}
fn is_zero_f64(v: &f64) -> bool { *v == 0.0 }
fn one_f64() -> f64 { 1.0 }
fn is_one_f64(v: &f64) -> bool { *v == 1.0 }
fn is_false(v: &bool) -> bool { !*v }
impl Default for Audio { fn default() -> Self { Audio { lead: 0.0, tail: 0.0, effects: vec![], pan: Prop::new(0.0), duck_gain: Prop::new(1.0), mute: false, volume: Prop::new(1.0), fade_in: 0.0, fade_out: 0.0, pitch: 0.0, highpass: 0.0, lowpass: 0.0, denoise: 0.0, compress: false, echo: 0.0, reverse: false, speech_level: 0.0, compress_amount: 1.0 } } }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition { pub kind: String, pub duration: f64,
    #[serde(default = "d_ease")] pub ease: String, #[serde(default = "d_soft")] pub softness: f64, #[serde(default)] pub angle: f64,
    #[serde(default)] pub color: [f64; 3], #[serde(default = "d_one")] pub intensity: f64, #[serde(default = "d_count")] pub count: f64, #[serde(default)] pub seed: u32,
    /// add_push_wipe's extra settings (push direction; gradient wipe shape, centre, luma clip, invert). None for add_transition kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub spec: Option<TransitionSpec> }
/// Settings of the push and gradient_wipe transitions (add_push_wipe, pushwipe.rs).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TransitionSpec { #[serde(default, skip_serializing_if = "String::is_empty")] pub direction: String, #[serde(default, skip_serializing_if = "String::is_empty")] pub gradient: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub luma_clip: Option<String>, #[serde(default = "d_center")] pub center: [f64; 2], #[serde(default)] pub invert: bool }
fn d_center() -> [f64; 2] { [0.5, 0.5] }
fn d_ease() -> String { "in_out".into() } fn d_soft() -> f64 { 0.08 } fn d_one() -> f64 { 1.0 } fn d_count() -> f64 { 10.0 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track { pub id: String, pub clips: Vec<std::sync::Arc<Clip>>, #[serde(default)] pub locked: bool, #[serde(default)] pub muted: bool, #[serde(default)] pub solo: bool, #[serde(default)] pub name: String }
impl Track { pub fn cm(&mut self, i: usize) -> &mut Clip { std::sync::Arc::make_mut(&mut self.clips[i]) } }

/// v7.4: solo overrides mute project-wide (standard NLE behaviour -- Premiere/Resolve/Reaper all do this):
/// when any track in the project is soloed, every non-soloed track is effectively muted regardless of its
/// own `muted` flag, and a soloed track plays even if its own `muted` happens to be true (solo wins). With
/// no track soloed, a track's own `muted` flag is the whole answer. Centralised here so every call site
/// that used to read `t.muted` directly (audio render, auto_captions' speech gather, director_tools'
/// broll/pacing/hook scanners, lint_frame's visual checks, export_manifest's audit) applies solo the same
/// way instead of five subtly different reimplementations.
pub fn effective_mute(tracks: &[Track], t: &Track) -> bool {
    let any_solo = tracks.iter().any(|x| x.solo);
    if any_solo { !t.solo } else { t.muted }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset { #[serde(default,skip_serializing_if="Option::is_none")] pub origin:Option<String>, #[serde(default,skip_serializing_if="Option::is_none")] pub hash:Option<String>, #[serde(default,skip_serializing_if="Option::is_none")] pub kind:Option<String>, pub id: String, pub path: String, pub width: u32, pub height: u32, pub duration: f64, pub fps: f64,
    /// Media-pool organisation (FEATURES.md L85): the bin this asset is filed under, by name. None = unfiled (the
    /// pool root). Absent from old projects, so they stay byte-stable; set/cleared via `move_asset_to_bin`.
    #[serde(default, skip_serializing_if="Option::is_none")] pub bin: Option<String> }

/// A named, saved sub-range of a media asset (FEATURES.md L85 "subclips"): bookmarks `[in_point, out_point)`
/// source seconds under a name, so it can be found and placed on the timeline (`add_clip_from_subclip`) without
/// re-remembering the exact numbers each time. Does not itself appear on any track.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subclip { pub id: String, pub asset_id: String, #[serde(default, skip_serializing_if="Option::is_none")] pub name: Option<String>, pub in_point: f64, pub out_point: f64 }

/// Persisted auto-occluder anchor recipe: lets a reloaded project rebuild the surface reference frame from the source media.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OccSpec { pub src: String, pub src_start: f64, pub path: String, pub src_time: f64, pub q0: [[f64; 2]; 4], pub soft: f32, pub chroma: f32, pub w: usize, pub h: usize, #[serde(default)] pub adaptive: bool, #[serde(default)] pub f0: usize, #[serde(default = "one_speed")] pub speed: f64, #[serde(default = "one_tol")] pub tol: f32, #[serde(default = "def_min_area")] pub min_area: f32, #[serde(default)] pub temporal: usize, #[serde(default)] pub thin: f32, #[serde(default)] pub close: f32, #[serde(default)] pub ramp: Vec<f64>, #[serde(default)] pub lgate: f32, #[serde(default)] pub expand: f32 }
fn def_min_area() -> f32 { 0.0 }
fn one_tol() -> f32 { 1.0 }
fn one_speed() -> f64 { 1.0 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// link_property's live links: (target clip/property/effect) <- (driver clip/property/effect) * multiplier + offset,
    /// delayed. Absent/empty for every project that never called link_property, so old files stay byte-stable. Resolved
    /// fresh from the current driver at every render and sample_property read (crate::ext::link_property::resolve_links)
    /// -- the persisted keys below are last-bake snapshot, kept for tools/files that are not link-aware.
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub links: Vec<crate::ext::link_property::Link>,
    #[serde(default, skip_serializing_if="crate::proxy::Proxies::is_empty")] pub proxies: crate::proxy::Proxies,
    #[serde(default)] pub grade_scopes: crate::color::grade_scopes::Scopes,
    #[serde(default)] pub editorial: crate::editorial::Editorial,
    #[serde(default)] pub lights: BTreeMap<String, crate::lighting::LightDef>,
    #[serde(default)] pub materials: BTreeMap<String, crate::lighting::Material>,
    #[serde(default)] pub occluders: BTreeMap<String, OccSpec>,
    #[serde(default)] pub ocio_view_delivery: Option<crate::color::view_video::Selection>,
    #[serde(default)] pub color_pipeline: crate::color::pipeline::Pipeline,
    /// V8.7.0: lightweight linear-light compositing option, independent of the full managed color_pipeline
    /// above (ACEScg working space, 16-bit decode, HDR) -- that path measured ~3.4x slower on a real
    /// auto_edit export (144.7s vs 42.4s at 720p, see CHANGELOG v8.7.0), far past the "doesn't tank speed"
    /// bar, because it reroutes decode/effects/output through 16-bit float scene-linear end to end. This
    /// flag instead linearizes only the actual alpha-composite step (render.rs's blend/blend_region, via a
    /// small interpolated sRGB EOTF/OETF LUT) and nothing else -- decode, grading effects (auto_grade,
    /// match_shot_look) and encode stay exactly as they are today, so this is both fast and leaves existing
    /// grading math undisturbed. Also gates the lanczos clip-to-canvas fit and TPDF dithered output (the
    /// audit's #4 and #6), the same switch for all three. Absent/false for every existing project file, so
    /// old files and golden_frames.txt / recipe_frames.txt stay byte-identical.
    #[serde(default, skip_serializing_if = "is_false")] pub linear_blend: bool,
    /// V8.8.0: explicit x264 encoder preset for the legacy H.264 export (legacy_encoder_command). Absent
    /// (None) keeps ffmpeg's own default ("medium"), so every existing project and golden stays byte-
    /// identical -- this only changes final encoder settings, never a rendered pixel, so it does not
    /// touch golden_frames.txt/recipe_frames.txt either way. Set by auto_edit as its own default (see
    /// ext/auto_edit.rs), the same opt-in-for-auto_edit pattern as linear_blend above.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub x264_preset: Option<String>,
    /// V8.8.1: when true, decode treats HD (>=720 lines) source footage that is mistagged or incompletely
    /// tagged (an SD matrix such as bt470bg/smpte170m with no primaries/transfer at all) as bt709 instead
    /// of trusting the lone incomplete tag -- the convention most NLEs use, and the audit's measured case
    /// (talkinghead_real_final.mp4: bt470bg, no primaries/transfer, at 1280x720). False (default, every
    /// existing project) decodes exactly as before: ffmpeg's swscale honours whatever is tagged. See
    /// render.rs::color_tag_override and decode().
    #[serde(default, skip_serializing_if = "is_false")] pub color_tag_sanity: bool,
    /// V8.9.3 (quality audit #5): when true and a clip's native media fps is lower than the project fps
    /// (e.g. a 24fps source in a 30fps project), decode uses ffmpeg's `minterpolate=mi_mode=blend` (a
    /// cheap cross-dissolve of the two nearest source frames, no motion search) instead of the plain
    /// `fps=` filter's frame duplication. Off by default: existing projects/goldens render byte-identical.
    #[serde(default, skip_serializing_if = "is_false")] pub frame_blend_conform: bool,
    #[serde(default)] pub camera: BTreeMap<String, crate::keyframe::Prop>,
    /// 3D compositor (comp3d.rs, set_3d_compositor): depth sorting, intersections, shadows, depth of field. None (absent from the
    /// file) or enabled = false renders exactly as before.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub comp3d: Option<crate::comp3d::Comp3d>,
    /// Scripting hooks (FEATURES.md L239): named lifecycle events, each a list of template_apply calls to run when
    /// run_hooks(event) is invoked. Absent/empty for every project that never called register_hook, so old project
    /// files stay byte-stable. Explicitly triggered (run_hooks), not auto-fired by render/export -- see data_driven_v7.rs.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")] pub hooks: BTreeMap<String, Vec<crate::ext::data_driven_v7::HookEntry>>,
    /// Free-form expression scripts (FEATURES.md L210/L220 gap, ext/expr_interp_v7.rs): Project.links'
    /// sibling record for a real small expression language instead of a fixed multiplier/offset. Absent/
    /// empty for every project that never called set_expression_script, so old project files stay byte-
    /// stable. Resolved fresh (expr_interp_v7::resolve_live, which also resolves links) at every render
    /// and sample_property read; the persisted keys are the last-bake snapshot for non-live readers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub expr_scripts: Vec<crate::ext::expr_interp_v7::ExprScript>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub assets: BTreeMap<String, Asset>,
    /// Media-pool bin names that exist (FEATURES.md L85), including empty ones; an asset's `bin` field may also
    /// name a bin not listed here if it predates explicit bin creation. Absent/empty for every project that never
    /// called create_bin, so old files stay byte-stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub bins: Vec<String>,
    /// Saved named sub-ranges of media assets (FEATURES.md L85 "subclips"). Absent/empty for every project that
    /// never called create_subclip, so old files stay byte-stable.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")] pub subclips: BTreeMap<String, Subclip>,
    /// Bottom to top.
    pub tracks: Vec<Track>,
    #[serde(skip)]
    pub next_id: u64,
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// Dependency records (size, sha256, ...) written by collect_project; absent in files that never recorded them (old files stay byte-stable).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub deps: Option<crate::deps::DepTable>,
    /// U2.4: user-authored batch templates (none ship with Kimo); absent when empty so old files stay byte-stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub templates: Vec<crate::ext::templates::Template>,
    /// Directory of the project file (canonical), set by load_project / save_project / collect. Never serialized. Relative stored paths resolve against it.
    #[serde(skip)] pub base_dir: Option<std::path::PathBuf>,
    /// RV-28: a gentle master-bus speech leveler (ffmpeg `speechnorm`), applied to the mixed timeline buffer
    /// in `audio::mix`/`audio::mix_strict` right before each one's own peak limiter -- not per clip. A per-clip version was tried first and made
    /// loudness range WORSE on real multi-take footage (11.1 -> 13.7 LU): speechnorm is a slow raise/fall leveler, and
    /// restarting its internal state at every short rough-cut clip boundary introduced a new transient at every cut
    /// instead of smoothing across them. Running it once over the continuous mixed signal (same shape as finish_audio's
    /// own final-mix measurement) is what actually narrows loudness range end to end. 0 = off (default, and every
    /// existing project/golden stays byte-identical and mix()-identical); else the speechnorm expansion/compression
    /// factor. Set by finish_audio's level_dynamics/level_strength args, never by hand-authored projects.
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub master_speech_level: f64,
    /// V8.1Q: the scope `master_speech_level` actually runs over, as a timeline end time in seconds. 0 = unscoped,
    /// the old whole-buffer behaviour (back-compat for any caller that sets master_speech_level without this).
    /// When > 0, `audio::mix`/`mix_strict` apply speechnorm only to `[0, this)` and leave the tail (an end card,
    /// a music-only outro) untouched, with a short crossfade at the boundary so the handoff isn't audible.
    /// Root cause this fixes (V8.0Q): speechnorm run over the WHOLE mix reacts to a sudden louder segment
    /// ANYWHERE in it -- an end-card music swell as small as 5dB collapsed whisper word recall from ~90% to
    /// 56.7% on the real benchmark even though the dialogue audio itself never changed (confirmed via
    /// --no-level-dynamics recovering recall on the same render). Scoping speechnorm to the dialogue span
    /// (set by finish_audio from its own classified dialogue clips' latest end) removes the end card from
    /// speechnorm's view entirely, so a swell there can no longer touch it. Set by finish_audio, never by
    /// hand-authored projects.
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub master_speech_level_scope_end: f64,
}

impl Default for Project {
    fn default() -> Self {
        Project { links: vec![], proxies: Default::default(), grade_scopes: Default::default(), lights: BTreeMap::new(), materials: BTreeMap::new(), editorial: Default::default(), ocio_view_delivery: None, occluders: BTreeMap::new(), color_pipeline: Default::default(), linear_blend: false, x264_preset: None, color_tag_sanity: false, frame_blend_conform: false, camera: BTreeMap::new(), comp3d: None, hooks: BTreeMap::new(), expr_scripts: vec![], name: "untitled".into(), width: 1280, height: 720, fps: 30.0, assets: BTreeMap::new(), bins: vec![], subclips: BTreeMap::new(), tracks: vec![], next_id: 1, markers: vec![], deps: None, templates: vec![], base_dir: None, master_speech_level: 0.0, master_speech_level_scope_end: 0.0 }
    }
}

impl Clip {
    /// Map clip-local time to the local time whose frame should be shown (reverse/freeze).
    pub fn display_time(&self, lt: f64) -> f64 { if self.time_remap.is_some() { return lt; } match self.time_mode { TimeMode::Normal => lt, TimeMode::Reverse => (self.duration - lt).max(0.0), TimeMode::Freeze { at } => at } }
    /// Source-media time consumed after `lt` seconds of clip time.
    /// Source frame index shown at clip time `lt`; the 1e-6 frame epsilon absorbs quadrature rounding (0.36*25 -> 8.9999999) so boundaries pick the intended frame.
    pub fn source_index(&self, lt: f64, fps: f64) -> usize { if let Some((a, _, _)) = self.remap_pos(lt.max(0.0), fps) { return a; } ((self.source_time(lt.max(0.0)) * fps) + 1e-6).floor() as usize }
    /// Remapped source seconds (after in_point) at clip time `lt`, when the clip has a time remap.
    pub fn remap_source(&self, lt: f64) -> Option<f64> { self.time_remap.as_ref().map(|r| r.curve.at(lt + r.offset).max(0.0)) }
    /// True when video is indexed by source frame (speed ramp or time remap) rather than by constant speed.
    pub fn retimed(&self) -> bool { self.speed_curve.is_some() || self.time_remap.is_some() }
    /// Remapped source position at clip time `lt`: (frame a, frame b, weight of b). Positions within 1e-6 frame of a whole frame snap to it (0.7*10 -> 6.9999999).
    pub fn remap_pos(&self, lt: f64, fps: f64) -> Option<(usize, usize, f64)> {
        let x = self.remap_source(lt)? * fps; let r = x.round();
        if (x - r).abs() < 1e-6 { return Some((r as usize, r as usize, 0.0)); }
        let a = x.floor(); Some((a as usize, a as usize + 1, x - a))
    }
    /// Speed-ramp source position at clip time `lt` when the ramp blends frames (speed_curve + speed_blend, no time remap): (frame a, frame b, weight of b),
    /// with frame a = source_index (same 1e-6 snap), so frame sampling and blending agree on the frame shown.
    pub fn ramp_pos(&self, lt: f64, fps: f64) -> Option<(usize, usize, f64)> {
        if self.time_remap.is_some() || self.speed_curve.is_none() || self.speed_blend.is_off() { return None; }
        let x = self.source_time(lt.max(0.0)) * fps; let a = (x + 1e-6).floor(); let w = x - a;
        if w < 1e-6 { return Some((a as usize, a as usize, 0.0)); } Some((a as usize, a as usize + 1, w))
    }
    /// Min/max remapped source seconds over clip time [lt0, lt1] (curves may reverse or overshoot): both ends, every key inside and a <= 1/240 s grid (<= 4096 steps).
    pub fn remap_span(&self, lt0: f64, lt1: f64) -> (f64, f64) {
        let Some(r) = &self.time_remap else { return (0.0, 0.0) }; let (lt0, lt1) = (lt0.min(lt1), lt0.max(lt1));
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY); let mut f = |t: f64| { let v = r.curve.at(t + r.offset).max(0.0); lo = lo.min(v); hi = hi.max(v); };
        f(lt0); f(lt1); for k in &r.curve.keys { let t = k.t - r.offset; if t > lt0 && t < lt1 { f(t); } }
        let n = ((lt1 - lt0) * 240.0).ceil().clamp(1.0, 4096.0) as usize; for i in 1..n { f(lt0 + (lt1 - lt0) * i as f64 / n as f64); }
        (lo, hi)
    }
    pub fn source_time(&self, lt: f64) -> f64 {
        match &self.speed_curve { None => lt * self.speed, Some(p) => { let n = (lt * 240.0).ceil().max(1.0) as usize; let dt = lt / n as f64; (0..n).map(|i| p.at((i as f64 + 0.5) * dt).max(0.0) * dt).sum() } }
    }
}

impl Project {
    /// Resolves a stored path against this project's directory (see deps::resolve_path). Absolute paths resolve to themselves.
    pub fn resolve(&self, stored: &str) -> String { crate::deps::resolve_path(self.base_dir.as_deref(), stored).path.to_string_lossy().into_owned() }
    /// Largest trailing number in any id-like string (keys and string values) of this project; 0 when there is none.
    pub fn max_id_suffix(&self) -> u64 {
        fn walk(v: &serde_json::Value, m: &mut u64) {
            let tail = |s: &str| { let d: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect(); if d.is_empty() || d.len() > 12 || s.len() > 64 { 0 } else { d.parse::<u64>().unwrap_or(0) } };
            match v { serde_json::Value::String(s) => *m = (*m).max(tail(s)), serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, m)), serde_json::Value::Object(o) => for (k, x) in o { *m = (*m).max(tail(k)); walk(x, m); }, _ => {} }
        }
        let mut m = 0u64; if let Ok(v) = serde_json::to_value(self) { walk(&v, &mut m); } m
    }
    /// Collision-safe id counter after undo/redo/restore: one past the largest trailing number in any id-like string, never below 1000.
    pub fn recompute_next_id(&mut self) { self.next_id = self.max_id_suffix().max(999) + 1; }
    /// Id counter after load_project (F1): one past the largest id number in the project and in every history snapshot (`others`), no 1000 floor, so ids
    /// continue exactly where an in-memory session would have continued. Unique-id guarantee: the counter is above every id that can come back through undo/redo.
    pub fn continue_ids_after_load(&mut self, others: &[&Project]) { let m = others.iter().map(|p| p.max_id_suffix()).fold(self.max_id_suffix(), u64::max); self.next_id = m + 1; }
    pub fn gen_id(&mut self, p: &str) -> String { let i = self.next_id; self.next_id += 1; format!("{p}{i}") }
    pub fn duration(&self) -> f64 {
        self.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.start + c.duration).fold(0.0, f64::max)
    }
    pub fn find_clip_mut(&mut self, id: &str) -> Option<&mut Clip> {
        self.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).find(|c| c.id == id).map(|c| std::sync::Arc::make_mut(c))
    }
    pub fn find_clip(&self, id: &str) -> Option<&Clip> {
        self.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| c.id == id).map(|c| &**c)
    }
}


/// A clip-id reference that was cleared because its target clip was removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClearedRef { pub clip_id: String, pub field: String, pub removed_id: String }

/// Every clip-id reference held by `c` (not recursing into precomps): (field path, referenced id). ONE place that knows the reference kinds.
/// Known kinds are cleared on delete. Unknown `strings` values that equal a clip id are reported by dangling_refs only (field prefix "unknown:").
pub fn clip_refs(c: &Clip) -> Vec<(String, String)> {
    let mut v = vec![];
    if let Some(m) = &c.mask {
        if let Some(t) = &m.matte { v.push(("mask.matte.source".to_string(), t.source.clone())); }
        for l in &m.layers { if let Some(r) = l.strings.get("ref_clip") { v.push((format!("mask.layers.{}.ref_clip", l.id), r.clone())); } }
    }
    for e in &c.effects { if let Some(k) = crate::layerfx::source_key(&e.kind) { if let Some(r) = e.strings.get(k) { v.push((format!("effects.{}.strings.{k}", e.id), r.clone())); } } } // displacement_map / set_matte driving clip
    if let Some(l) = c.transition_in.as_ref().and_then(|t| t.spec.as_ref()).and_then(|s| s.luma_clip.as_ref()) { v.push(("transition_in.spec.luma_clip".to_string(), l.clone())); } // add_push_wipe's luma clip
    match &c.source {
        Source::ShardBurst(e) => { if let Some(t) = e.plane.as_ref().and_then(|p| p.occlusion_target.as_ref()) { v.push(("source.plane.occlusion_target".to_string(), t.clone())); } }
        Source::Emitter(e) => {
            if !e.emit_clip.is_empty() { v.push(("source.emit_clip".to_string(), e.emit_clip.clone())); }
            for (i, k) in e.colliders.iter().enumerate() { if k.kind == "layer" && !k.clip_id.is_empty() { v.push((format!("source.colliders.{i}.clip_id"), k.clip_id.clone())); } }
        }
        Source::Particles3D(e) => { if !e.sprite.clip_id.is_empty() { v.push(("source.sprite.clip_id".to_string(), e.sprite.clip_id.clone())); } }
        _ => {}
    }
    v
}
/// Unknown string-map values (effect strings, mask layer strings other than ref_clip) that look like clip ids of existing-or-removed clips; reported, never cleared.
fn unknown_string_refs(c: &Clip, ids: &std::collections::HashSet<&str>, max_id: u64) -> Vec<(String, String)> {
    let looks = |s: &str| { let d: String = s.chars().rev().take_while(|x| x.is_ascii_digit()).collect(); s.starts_with("clip") && !d.is_empty() && d.parse::<u64>().map(|n| n < max_id).unwrap_or(false) && !ids.contains(s) };
    let mut v = vec![];
    for e in &c.effects { for (k, val) in &e.strings { if crate::layerfx::source_key(&e.kind) != Some(k.as_str()) && looks(val) { v.push((format!("unknown:effects.{}.strings.{k}", e.id), val.clone())); } } }
    if let Some(m) = &c.mask { for l in &m.layers { for (k, val) in &l.strings { if k != "ref_clip" && looks(val) { v.push((format!("unknown:mask.layers.{}.strings.{k}", l.id), val.clone())); } } } }
    v
}
fn clear_in_tracks(tracks: &mut Vec<Track>, ids: &[String], out: &mut Vec<ClearedRef>) {
    for t in tracks.iter_mut() { for ac in t.clips.iter_mut() {
        let hits: Vec<(String, String)> = clip_refs(ac).into_iter().filter(|(_, r)| ids.contains(r)).collect();
        let nested = matches!(ac.source, Source::Precomp { .. });
        if hits.is_empty() && !nested { continue; }
        let c = std::sync::Arc::make_mut(ac);
        for (field, rid) in hits {
            if field == "mask.matte.source" { if let Some(m) = c.mask.as_mut() { m.matte = None; } }
            else if field == "source.plane.occlusion_target" { if let Source::ShardBurst(e) = &mut c.source { if let Some(p) = e.plane.as_mut() { p.occlusion_target = None; } } }
            else if let Some((eid, key)) = field.strip_prefix("effects.").and_then(|x| x.split_once(".strings.")) { for e in c.effects.iter_mut().filter(|e| e.id == eid) { e.strings.remove(key); } } // the effect then does nothing
            else if field == "transition_in.spec.luma_clip" { if let Some(s) = c.transition_in.as_mut().and_then(|t| t.spec.as_mut()) { s.luma_clip = None; } } // the wipe falls back to its gradient
            else if field == "source.emit_clip" { if let Source::Emitter(e) = &mut c.source { e.emit_clip.clear(); } }
            else if field == "source.sprite.clip_id" { if let Source::Particles3D(e) = &mut c.source { e.sprite.clip_id.clear(); } }
            else if let Some(i) = field.strip_prefix("source.colliders.").and_then(|x| x.strip_suffix(".clip_id")).and_then(|x| x.parse::<usize>().ok()) { if let Source::Emitter(e) = &mut c.source { if i < e.colliders.len() { e.colliders[i].clip_id.clear(); } } }
            else if let Some(lid) = field.strip_prefix("mask.layers.").and_then(|x| x.strip_suffix(".ref_clip")) { if let Some(m) = c.mask.as_mut() { for l in m.layers.iter_mut().filter(|l| l.id == lid) { l.strings.remove("ref_clip"); } } }
            out.push(ClearedRef { clip_id: c.id.clone(), field, removed_id: rid });
        }
        if let Source::Precomp { tracks } = &mut c.source { clear_in_tracks(tracks, ids, out); }
    } }
}
fn collect_ids<'a>(tracks: &'a [Track], out: &mut std::collections::HashSet<&'a str>) { for t in tracks { for c in &t.clips { out.insert(c.id.as_str()); if let Source::Precomp { tracks } = &c.source { collect_ids(tracks, out); } } } }
fn dangling_in<'a>(tracks: &[Track], ids: &std::collections::HashSet<&'a str>, max_id: u64, out: &mut Vec<ClearedRef>) {
    for t in tracks { for c in &t.clips {
        for (f, r) in clip_refs(c).into_iter().filter(|(_, r)| !ids.contains(r.as_str())).chain(unknown_string_refs(c, ids, max_id)) { out.push(ClearedRef { clip_id: c.id.clone(), field: f, removed_id: r }); }
        if let Source::Precomp { tracks } = &c.source { dangling_in(tracks, ids, max_id, out); }
    } }
}

impl Project {
    /// Clear every reference to the given (removed) clip ids: known kinds are cleared (matte, mask ref_clip, shard occlusion target, emitter emit_clip and layer colliders, occluder entry keyed by the id, caption group membership), recursing into precomps. Ids are never reused, so a cleared reference can only come back through undo.
    /// Process-global planar state for a removed target is dropped too.
    pub fn clear_refs_to(&mut self, ids: &[String]) -> Vec<ClearedRef> {
        let mut out = vec![];
        if ids.is_empty() { return out; }
        clear_in_tracks(&mut self.tracks, ids, &mut out);
        for id in ids {
            if self.occluders.remove(id).is_some() { out.push(ClearedRef { clip_id: id.clone(), field: "occluders".into(), removed_id: id.clone() }); }
            crate::planar::clear_seq(id); crate::planar::remove_anchor(id);
        }
        for (f, rid) in self.grade_scopes.remove_clips(ids) { out.push(ClearedRef { clip_id: String::new(), field: f, removed_id: rid }); }
        for (gid, g) in self.editorial.captions.iter_mut() { for id in ids { if g.clips.contains(id) { g.clips.retain(|x| x != id); out.push(ClearedRef { clip_id: gid.clone(), field: "editorial.captions.clips".into(), removed_id: id.clone() }); } } }
        out
    }
    /// Validator: references whose target clip does not exist (recurses into precomps). Empty on a consistent project.
    pub fn dangling_refs(&self) -> Vec<ClearedRef> {
        let mut ids = std::collections::HashSet::new(); collect_ids(&self.tracks, &mut ids);
        let mut out = vec![]; dangling_in(&self.tracks, &ids, self.next_id, &mut out);
        for k in self.occluders.keys() { if !ids.contains(k.as_str()) { out.push(ClearedRef { clip_id: k.clone(), field: "occluders".into(), removed_id: k.clone() }); } }
        for c in self.grade_scopes.clip_bindings.keys() { if !ids.contains(c.as_str()) { out.push(ClearedRef { clip_id: String::new(), field: "grade_scopes.clip_bindings".into(), removed_id: c.clone() }); } }
        for (i, g) in self.grade_scopes.groups.iter().enumerate() { for c in &g.clips { if !ids.contains(c.as_str()) { out.push(ClearedRef { clip_id: g.id.clone(), field: format!("grade_scopes.groups[{i}].clips"), removed_id: c.clone() }); } } }
        for (gid, g) in &self.editorial.captions { for x in &g.clips { if !ids.contains(x.as_str()) { out.push(ClearedRef { clip_id: gid.clone(), field: "editorial.captions.clips".into(), removed_id: x.clone() }); } } }
        out
    }
}

pub fn default_props() -> BTreeMap<String, Prop> {
    let mut m = BTreeMap::new();
    m.insert("opacity".into(), Prop::new(1.0));
    m.insert("x".into(), Prop::new(0.0));      // pixels offset
    m.insert("y".into(), Prop::new(0.0));
    m.insert("scale".into(), Prop::new(1.0));
    m.insert("rotation".into(), Prop::new(0.0)); // degrees
    m.insert("reveal".into(), Prop::new(1.0)); // text typewriter 0..1
    m
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker { pub id: String, pub time: f64, pub duration: f64, pub name: String, pub color: String }

/// Rich text style + After Effects-style per-unit animators for text clips.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextFx {
    pub weight: f64, pub uppercase: bool, pub line_mask: bool, pub italic_skew: f64, pub box_width: f64, pub line_spacing: f64, pub kerning: bool,
    pub bg: Option<[f64; 4]>, pub bg_pad: f64, pub bg_radius: f64,
    pub shadow_blur: f64, pub shadow_opacity: f64, pub shadow_color: [f64; 3], pub shadow_angle: f64,
    pub gradient: Option<([f64; 3], [f64; 3])>, pub glow: f64, pub glow_color: [f64; 3],
    pub animators: Vec<TextAnimator>,
    #[serde(default)] pub path: Option<crate::motion::MotionPath>, #[serde(default = "zp")] pub path_start: Prop, #[serde(default)] pub path_flip: bool,
}
fn zp() -> Prop { Prop::new(0.0) }
impl Default for TextFx { fn default() -> Self { TextFx { weight: 0.0, uppercase: false, line_mask: false, italic_skew: 0.0, box_width: 0.0, line_spacing: 1.15, kerning: true, bg: None, bg_pad: 24.0, bg_radius: 12.0, shadow_blur: 0.0, shadow_opacity: 0.6, shadow_color: [0.0; 3], shadow_angle: 45.0, gradient: None, glow: 0.0, glow_color: [1.0, 1.0, 1.0], animators: vec![], path: None, path_start: Prop::new(0.0), path_flip: false } } }
/// Range selector + properties applied to each unit (char/word/line) by selector amount. All values keyframable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextAnimator {
    pub id: String, pub unit: String, pub shape: String,
    pub start: Prop, pub end: Prop, pub offset: Prop, pub smoothness: Prop,
    pub x: Prop, pub y: Prop, pub scale: Prop, pub rotation: Prop, pub opacity: Prop, pub tracking: Prop, pub blur: Prop,
    /// Selector order: false = reading order, true = pseudo-random (AE 'Randomize Order'), stable per `seed`.
    #[serde(default)] pub random: bool, #[serde(default)] pub seed: u64,
    /// AE Fill Color: selected units are tinted toward this colour by selector amount x `fill_amount` (None = 1.0). None = no tint.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub fill: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub fill_amount: Option<Prop>,
    /// Reshapes the selector amount k -> ease(k) with the shared easing primitive. None = k untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub ease: Option<crate::keyframe::Ease>,
    /// AE Wiggly Selector: the selector amount is multiplied per unit by smooth noise between min and max (text.rs). None = no wiggle.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub wiggly: Option<Wiggly>,
}
/// Wiggly Selector (After Effects): per unit a smooth random value in min..max (each -1..1, keyframable), changing `freq` times a second;
/// `correlation` 0 = every unit wanders on its own, 1 = all units share one wiggle; stable per `seed`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wiggly { pub min: Prop, pub max: Prop, pub freq: f64, pub correlation: f64, pub seed: u64 }

/// Echo ghosts: `count` earlier (or later when spacing<0) copies of the layer at `spacing` seconds apart.
/// mode: composite (ghosts fade by decay^k), add / screen (light trails), average (equal-weight frame blending = temporal smear).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Echo { pub count: u32, pub spacing: f64, pub decay: f64, pub mode: String }
