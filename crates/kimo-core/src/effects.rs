//! Per-pixel and spatial effects on f32 RGBA frames (premultiplied-free, 0..1).
use crate::keyframe::Prop;
use std::collections::BTreeMap;

pub struct Frame { pub w: usize, pub h: usize, pub d: Vec<f32> }
impl Frame {
    pub fn new(w: usize, h: usize) -> Self { crate::guard::reserve(w.saturating_mul(h).saturating_mul(16)); match Self::try_new(w, h) { Ok(f) => f, Err(e) => panic!("{e}") } }
    /// Fallible allocation: `Err` (never an abort) when the pixels do not fit the budget, the hard ceiling, or real memory.
    pub fn try_new(w: usize, h: usize) -> Result<Self, String> {
        let n = w.checked_mul(h).and_then(|v| v.checked_mul(4)).ok_or_else(|| format!("frame {w}x{h} overflows"))?;
        let b = n.saturating_mul(4); let (budget, live) = (crate::guard::BUDGET.load(std::sync::atomic::Ordering::Relaxed), crate::guard::LIVE.load(std::sync::atomic::Ordering::Relaxed));
        if budget != usize::MAX && live.saturating_add(b) > budget { return Err(format!("memory budget exceeded: a {w}x{h} frame needs {} MB, {} MB of the {} MB budget is in use", b >> 20, live >> 20, budget >> 20)); }
        let mut d: Vec<f32> = Vec::new(); d.try_reserve_exact(n).map_err(|e| format!("cannot allocate a {w}x{h} frame ({} MB): {e}", b >> 20))?; d.resize(n, 0.0); Ok(Frame { w, h, d })
    }
}

pub type Params = BTreeMap<String, Prop>;
pub(crate) fn p(m: &Params, k: &str, def: f64, t: f64) -> f32 { m.get(k).map(|x| x.at(t)).unwrap_or(def) as f32 }

/// Every `strings` key some effect (or its mask / OCIO / wrapper / LUT reader) reads. add_effect refuses any other key, which would be
/// stored and never read. Collected from every `strings.get/contains_key/insert/remove("..")` in the crate plus the OCIO keys read by name.
pub const STRING_KEYS: &[&str] = &["a", "aux_path", "b", "bound_linear_gamut", "cct_space", "cdl_payload", "config_path", "core_mask", "cube_text", "d", "dir", "edge", "expr", "field", "fill_rule", "fps", "g", "garbage_mask", "height_source", "hits", "interp", "kernel", "keys", "kind", "lights", "linear_space", "mask_layer", "material", "mode", "ocio_config", "ocio_display_space", "ocio_domain", "ocio_linear_space", "ocio_version", "ocio_view_delivery", "ocio_working_space", "occluder", "path", "position", "profile", "r", "ref_clip", "seed", "shape", "solve_id", "source_clip", "source_light", "source_mask", "wrapper_gamut", "wrapper_reference_white", "wrapper_transfer", "wrapper_working_gamut"];

/// (kind, description, [(param, default, description)]) - drives list_effects and tool schemas.
pub const CATALOG: &[(&str, &str, &[(&str, f64, &str)])] = &[
    ("field_operator", "Raw selected 2D operator. strings.field is JSON with op worley|fbm|warp|curl and explicit config matching sample_field. RGB is selected scalar/component*gain+bias; alpha unchanged. Point sampled. No implicit bundled fractal/warp.", &[("frequency",8.,"coordinate scale per frame height"),("offset_x",0.,"field units"),("offset_y",0.,"field units"),("rate_x",0.,"field units/s"),("rate_y",0.,"field units/s"),("gain",1.,"display/output gain"),("bias",0.,"display/output bias"),("amount",1.,"0..1 mix")]),
    ("coherent_noise", "Raw signed 2D coherent field; strings.kind perlin|simplex, strings.seed u64 decimal. RGB mixes with field*gain+bias, alpha unchanged. Coordinates pixel-center/frame-height * frequency + offset + rate*t. Point-sampled, not antialiased. No FBM/warp/curl bundled.", &[("frequency",8.,"cycles per frame height"),("offset_x",0.,"field units"),("offset_y",0.,"field units"),("rate_x",0.,"field units/s"),("rate_y",0.,"field units/s"),("amount",1.,"0..1 mix"),("gain",1.,"raw signed gain"),("bias",0.,"raw bias")]),
    ("displace", "Bilinear UV offset by aux_path pixel asset or strings.expr field; edge clamp|wrap|transparent", &[("amount_x",0.,"UV scale"),("amount_y",0.,"UV scale"),("neutral",0.5,"zero offset field value")]),
    ("convolution", "Float convolution strings.kernel odd square flat JSON array (default [1] identity); no creative clamps", &[("divisor",1.,"nonzero"),("bias",0.,"RGB bias")]),
    ("grade", "Raw gain/lift primitive in working (linear) space: RGB_out=RGB*gain+lift; alpha preserved; separate from grade-graph nodes", &[("gain_r",1.,"red gain"),("gain_g",1.,"green gain"),("gain_b",1.,"blue gain"),("lift_r",0.,"red lift"),("lift_g",0.,"green lift"),("lift_b",0.,"blue lift")]),
    ("pixel_expr", "Pure per-pixel expression in strings.expr (vector) or strings.r/g/b/a; x,y,u,v,w,h,t,src,p0..p7; sample(u,v), aux1(u,v). No I/O or loops. Resource caps via set_limits.", &[("p0",0.,"animatable"),("p1",0.,"animatable"),("p2",0.,"animatable"),("p3",0.,"animatable"),("p4",0.,"animatable"),("p5",0.,"animatable"),("p6",0.,"animatable"),("p7",0.,"animatable")]),
    ("exposure", "Exposure in stops", &[("stops", 0.0, "-5..5")]),
    ("contrast", "Contrast around mid grey", &[("amount", 1.0, "0..3")]),
    ("saturation", "Color saturation", &[("amount", 1.0, "0=mono, 1=unchanged, 2=vivid")]),
    ("temperature", "White balance warm/cool", &[("amount", 0.0, "-1 cool..1 warm")]),
    ("tint", "Green/magenta tint", &[("amount", 0.0, "-1..1")]),
    ("color_wheels", "Lift/gamma/gain per channel (shadows/mids/highlights)", &[
        ("lift_r",0.0,"shadows R"),("lift_g",0.0,"shadows G"),("lift_b",0.0,"shadows B"),
        ("gamma_r",1.0,"mids R"),("gamma_g",1.0,"mids G"),("gamma_b",1.0,"mids B"),
        ("gain_r",1.0,"highlights R"),("gain_g",1.0,"highlights G"),("gain_b",1.0,"highlights B")]),
    ("curves", "Luma S-curve", &[("amount", 0.0, "-1..1 (positive = more contrast S-curve)")]),
    ("teal_orange", "Hollywood teal/orange split tone", &[("amount", 0.5, "0..1")]),
    ("vignette", "Darkened (or tinted) edges; shape it (roundness, squareness, size, aspect, feather) and centre it, or follow a tracked clip (strings.source_clip) or tracked mask (strings.source_mask)", &[("amount", 0.5, "0..1"), ("softness", 0.5, "0..1"), ("center_x", 0.5, "centre 0..1 of width"), ("center_y", 0.5, "centre 0..1 of height"), ("roundness", 0.0, "0 frame-shaped .. 1 true circle"), ("squareness", 0.0, "0 ellipse .. 1 rounded rectangle"), ("size", 1.0, "reach, 1 = frame corner"), ("aspect", 1.0, "width/height stretch"), ("feather", 0.5, "edge softness 0.05..1 (overrides softness)"), ("color_r", 0.0, "edge colour 0..1"), ("color_g", 0.0, "edge colour 0..1"), ("color_b", 0.0, "edge colour 0..1")]),
    ("blur", "Box-approx gaussian blur", &[("radius", 4.0, "pixels")]),
    ("glow", "Bloom on highlights. mode 1 = deep glow: soft-knee bright pass, exact Gaussian sum or exponential falloff, A/B tint, linear light", &[("amount", 0.5, "0..2"), ("radius", 12.0, "pixels (mode 1: widest Gaussian sigma / exponential R)"), ("threshold", 0.7, "0..1"),
        ("mode", 0.0, "0 classic, 1 deep glow"), ("falloff", 0.0, "mode 1: 0 sum of Gaussians, 1 exponential exp(-r/radius)"), ("iterations", 5.0, "mode 1 falloff 0: 1..8 Gaussians, sigma_k = radius*2^k/2^(n-1)"), ("exposure", 0.0, "mode 1: -8..8 stops"), ("knee", 0.2, "mode 1: 0..1 soft knee (as bloom)"),
        ("color_a_r", 1.0, "mode 1: core tint R"), ("color_a_g", 1.0, "mode 1: core tint G"), ("color_a_b", 1.0, "mode 1: core tint B"), ("color_b_r", 1.0, "mode 1: outer tint R"), ("color_b_g", 1.0, "mode 1: outer tint G"), ("color_b_b", 1.0, "mode 1: outer tint B"), ("linear", 1.0, "mode 1: 1 = glow in linear light (sRGB decode/encode)"),
        ("half_res", 0.0, "mode 1 fast path (FEATURES.md L558): 1 = blur at half resolution (box-down, bilinear-up) for a faster, slightly softer glow; 0 (default) is the exact full-res path, byte-identical to before this param existed")]),
    ("bloom", "Light-intensity bloom: soft-knee bright-pass, multi-scale blur, energy-conserving by default", &[("amount", 0.6, "0..1 (conserve) or more"), ("threshold", 0.8, "luma where bloom starts"), ("knee", 0.2, "0..1 soft knee"), ("radius", 8.0, "base px, levels double it"), ("levels", 5.0, "1..7 scales"), ("conserve", 1.0, "1 = frame energy preserved, 0 = additive"), ("tint_r", 1.0, "bloom tint R"), ("tint_g", 1.0, "bloom tint G"), ("tint_b", 1.0, "bloom tint B")]),
    ("grain", "Film grain: size, softness, colour, per-channel strength, shadows/mids/highlights response (Film Look Creator style)", &[("amount", 0.08, "0..0.5"),
        ("size", 1.0, "grain size in px, 1..32 (1 = per pixel)"), ("softness", 0.0, "0..1 rounds the grains off"), ("color", 0.0, "0 mono .. 1 independent per channel"),
        ("shadows", 1.0, "grain in shadows, 0..2"), ("mids", 1.0, "grain in midtones, 0..2"), ("highlights", 1.0, "grain in highlights, 0..2"),
        ("red", 1.0, "red channel strength, 0..2"), ("green", 1.0, "green channel strength, 0..2"), ("blue", 1.0, "blue channel strength, 0..2")]),
    ("rgb_split", "Formula: channel(x,y)=src(x+offset_channel*cos(angle),y+offset_channel*sin(angle)); offset truncated toward zero before adding integer position, clamp edges", &[("amount",6.,"legacy +/- red/blue px"),("r_offset",6.,"red px"),("g_offset",0.,"green px"),("b_offset",-6.,"blue px"),("angle",0.,"degrees")]),
    ("invert", "Invert colors", &[]),
    ("chroma_key", "Remove a key color to alpha (space 1: YCbCr chroma distance)", &[("r",0.0,"key R"),("g",1.0,"key G"),("b",0.0,"key B"),("tolerance",0.35,"0..1"),("softness",0.1,"0..1"),("space",0.0,"0 RGB distance (default), 1 YCbCr chroma distance (ignores screen shading)")]),
    ("lut", "3D LUT (.cube) via strings.path", &[("intensity", 1.0, "0..1")]),
    ("hsl", "Hue/Sat/Lum, global + per color band (red,yellow,green,cyan,blue,magenta _hue/_sat/_lum)", &[("hue",0.0,"-1..1 (x60deg)"),("sat",0.0,"-1..1"),("lum",0.0,"-1..1"),("red_sat",0.0,"band"),("blue_hue",0.0,"band")]),
    ("sharpen", "Unsharp mask", &[("amount", 0.8, "0..3"), ("radius", 2.0, "px")]),
    ("pixelate", "Mosaic blocks", &[("size", 12.0, "px")]),
    ("posterize", "Reduce color levels", &[("levels", 5.0, "2..32")]),
    ("sepia", "Sepia tone", &[("amount", 1.0, "0..1")]),
    ("color_isolate", "Black & white except one hue (keep gun/scope color)", &[("hue", 190.0, "degrees 0..360"), ("range", 40.0, "degrees"), ("softness", 25.0, "degrees"), ("bw_amount", 1.0, "0..1")]),
    ("silhouette_fill", "Flat-color silhouette of the clip's alpha (segmentation-mask look): replaces RGB with a solid color, keeps alpha, optional outline ring", &[("r",1.0,"fill R"),("g",0.85,"fill G"),("b",0.1,"fill B"),("amount",1.0,"0..1 mix toward the flat color"),("outline",0.0,"outline px @720p, 0=none"),("or",1.0,"outline R"),("og",1.0,"outline G"),("ob",1.0,"outline B")]),
    ("shake", "Formula: step=floor(t*speed), n(k)=2*signed_fract(sin(k*noise_frequency)*noise_gain)-1; dx=trunc(n(step)*amount), dy=trunc(n(step+y_seed_offset)*amount_y); absent amount_y follows amount. Sample source at (x-dx,y-dy), clamp edges", &[("amount",8.,"x px"),("amount_y",8.,"y px"),("speed",24.,"Hz"),("noise_frequency",12.9898,"seed multiplier"),("noise_gain",43758.545,"hash gain"),("y_seed_offset",7.,"y seed offset")]),
    ("impact_shake", "Beat/impact camera shake: damped spring kicks at hit times (strings.hits=\"t1,t2,..\" clip-local seconds), x/y/rotation/zoom; measured from the SCUMBLE reference", &[("amount", 29.0, "px @720p, x kick"), ("amount_y", 9.0, "px @720p, y kick"), ("decay", 27.0, "1/s, exp decay (0.34/frame at 25fps)"), ("frequency", 12.5, "Hz, 12.5 flips sign every frame at 25fps"), ("rotation", 0.0, "deg kick"), ("zoom", 0.0, "scale kick, e.g. 0.03")]),
    ("mirror", "Mirror half of frame", &[("axis", 0.0, "0=horizontal(left->right), 1=vertical(top->bottom)")]),
    ("scanlines", "CRT scanlines", &[("amount", 0.3, "0..1"), ("spacing", 3.0, "px")]),
    ("wave", "Sine wave distortion", &[("amplitude", 10.0, "px"), ("wavelength", 80.0, "px"), ("speed", 3.0, "cycles/s")]),
    ("light_leak", "Animated light leak, warm by default: colour, place, size and sway are editable", &[("amount", 0.6, "0..1"), ("speed", 0.5, "cycles/s"), ("x", 0.5, "centre 0..1 of width"), ("y", 0.1, "centre 0..1 of height"), ("size", 1.0, "reach, 1 = default"), ("travel", 0.5, "sideways sway 0..1 of width"), ("color_r", 1.0, "0..1"), ("color_g", 0.45, "0..1"), ("color_b", 0.12, "0..1")]),
    ("zoom_blur", "Radial zoom blur", &[("amount", 0.06, "0..0.3")]),
    ("luma_key", "Key out by brightness (keep=0 removes darks, keep=1 removes brights)", &[("threshold", 0.2, "0..1"), ("softness", 0.1, "0..1"), ("keep_bright", 1.0, "1 keeps brights, 0 keeps darks")]),
    ("ripple", "Concentric water ripple displacement", &[("amplitude", 8.0, "px"), ("wavelength", 60.0, "px"), ("speed", 2.0, "cycles/s"), ("cx", 0.5, "0..1"), ("cy", 0.5, "0..1")]),
    ("lens_distortion", "Barrel/pincushion lens distortion", &[("amount", 0.3, "-1..1 (positive = barrel)")]),
    ("lens_undistort", "Formula: Brown-Conrady (OpenCV) k1,k2 radial, p1,p2 tangential; output pixel samples the source at distort(pixel), bilinear, outside transparent. strings.solve_id: lens of that camera solve", &[("k1",0.,"radial k1"),("k2",0.,"radial k2"),("p1",0.,"tangential p1"),("p2",0.,"tangential p2"),("focal_px",0.,"px; 0 = latest camera solve"),("cx",0.5,"principal point x, 0..1"),("cy",0.5,"principal point y, 0..1")]),
    ("lens_redistort", "Formula: inverse of lens_undistort; output pixel samples the source at undistort(pixel) (exact iterative inverse), bilinear, outside transparent. strings.solve_id: lens of that camera solve", &[("k1",0.,"radial k1"),("k2",0.,"radial k2"),("p1",0.,"tangential p1"),("p2",0.,"tangential p2"),("focal_px",0.,"px; 0 = latest camera solve"),("cx",0.5,"principal point x, 0..1"),("cy",0.5,"principal point y, 0..1")]),
    ("glitch_blocks", "Formula: sh=max(1,floor(h/max(1,slices))) when slice_height=0, otherwise max(1,floor(slice_height)); sl=floor(y/sh), step=floor(t*rate), rnd(k)=abs(signed_fract(sin(k*noise_frequency)*noise_gain)); off=trunc((2*rnd(sl*slice_seed_scale+step)-1)*amount) if rnd(sl+step*step_seed_scale)>slice_threshold else 0; RGB/A sample x+off modulo w, green adds sign(off)*trunc(green_offset) modulo w", &[("amount",30.,"px"),("slices",12.,"count"),("rate",12.,"Hz"),("noise_frequency",12.9898,"hash multiplier"),("noise_gain",43758.545,"hash gain"),("step_seed_scale",17.,"time seed multiplier"),("slice_threshold",0.55,"activation threshold"),("slice_seed_scale",3.1,"slice seed multiplier"),("green_offset",4.,"green shift px"),("slice_height",0.,"override rows per slice; absent uses h/slices")]),
    ("light_rays", "God rays: radial blur of highlights from a point", &[("amount", 0.8, "0..2"), ("threshold", 0.6, "0..1"), ("cx", 0.5, "0..1"), ("cy", 0.2, "0..1"), ("length", 0.4, "0..1")]),
    ("drop_shadow", "Drop shadow from alpha (use on text/shapes/stickers)", &[("dx", 8.0, "px"), ("dy", 8.0, "px"), ("blur", 8.0, "px"), ("opacity", 0.6, "0..1")]),
    ("chromatic_aberration", "Formula: src(x+(x/w-cx)*amount*radial_scale*channel_scale,y+(y/h-cy)*amount*radial_scale*channel_scale); clamp float coordinate then truncate toward zero, green/alpha unchanged", &[("amount",4.,"px"),("cx",0.5,"center U"),("cy",0.5,"center V"),("radial_scale",2.,"radial multiplier"),("r_scale",1.,"red scale"),("b_scale",-1.,"blue scale")]),
    ("flicker", "Brightness flicker", &[("amount", 0.15, "0..1"), ("rate", 18.0, "Hz")]),
    ("halftone", "Halftone dots", &[("size", 8.0, "px")]),
    ("edge_glow", "Neon edge detect glow", &[("amount", 1.0, "0..3"), ("r", 0.0, "R"), ("g", 0.9, "G"), ("b", 1.0, "B")]),
    ("letterbox", "Cinematic bars", &[("ratio", 2.39, "target aspect")]),
];

pub fn apply(kind: &str, params: &Params, strings: &BTreeMap<String, String>, f: &mut Frame, t: f64, seed: u64) -> Result<(), String> {
    if kind == "field_operator" { return crate::noise_fields::apply(params,strings,f,t); }
    if kind == "coherent_noise" { return crate::noise::apply(params,strings,f,t); }
    if ["displace","convolution","grade"].contains(&kind){return crate::shader_ops::apply(kind,params,strings,f,t);}
    if kind == "vfx_graph" { return crate::vfx_graph::apply_effect(params, strings, f, t, seed); }
    if kind == "pixel_expr" {
        let source=strings.get("expr").cloned().unwrap_or_else(||format!("vec4({},{},{},{})",strings.get("r").map(String::as_str).unwrap_or("r"),strings.get("g").map(String::as_str).unwrap_or("g"),strings.get("b").map(String::as_str).unwrap_or("b"),strings.get("a").map(String::as_str).unwrap_or("a")));
        let cap=crate::resource_limits::current().expr_max_ops.unwrap_or(u64::MAX).min(usize::MAX as u64) as usize;
        let program=crate::pixel_expr::compile(&source,cap)?;
        let mut knobs=[0f32;8];for (i,v) in knobs.iter_mut().enumerate(){*v=p(params,&format!("p{i}"),0.,t);}
        let aux=strings.get("aux_path").map(|path|crate::pixels::read(&crate::deps::resolve_str(path))).transpose()?;
        if source.contains("aux1") && aux.is_none(){return Err("aux1 requires strings.aux_path (native pixel asset path)".into())}
        let out=crate::pixel_expr::apply(&program,f,aux.as_ref(),knobs,t as f32)?;f.d=out.d;return Ok(());
    }
    if kind == "relight" { return crate::lighting::apply_relight(params, strings, f, t); }
    if kind.starts_with("ls_") { return crate::layerstyles::apply(kind, params, strings, f, t); }
    if kind.starts_with("shape_") { return Ok(()); } // shape modifiers act on the vector source (shapemods.rs), not on pixels
    if kind=="asc_cdl"{if strings.contains_key("wrapper_working_gamut")||strings.contains_key("ocio_domain"){return Err("ASC CDL owns declareddomain; noextra wrapper".into())}return crate::color::cdl_file::apply(strings,f)}
    if kind=="acescct_primaries" {if strings.contains_key("wrapper_working_gamut")||strings.contains_key("ocio_domain"){return Err("ACEScctprimaries owns its native domain sandwich; no extra wrapper".into())}return crate::color::cct_grade::apply(params,strings,f,t)}
    if strings.contains_key("ocio_domain") {return crate::color::effect_domain::apply(kind,params,strings,f,t,seed)}
    if strings.contains_key("wrapper_working_gamut") {return crate::color::wrapper::apply(kind,params,strings,f,t,seed)}
    match kind {
        "exposure" => { let g = 2f32.powf(p(params, "stops", 0.0, t)); px(f, |c| [c[0]*g, c[1]*g, c[2]*g]); }
        "contrast" => { let a = p(params, "amount", 1.0, t); px(f, |c| [(c[0]-0.5)*a+0.5, (c[1]-0.5)*a+0.5, (c[2]-0.5)*a+0.5]); }
        "saturation" => { let a = p(params, "amount", 1.0, t); px(f, |c| { let l = lum(c); [l+(c[0]-l)*a, l+(c[1]-l)*a, l+(c[2]-l)*a] }); }
        "temperature" => { let a = p(params, "amount", 0.0, t)*0.2; px(f, |c| [c[0]+a, c[1], c[2]-a]); }
        "tint" => { let a = p(params, "amount", 0.0, t)*0.2; px(f, |c| [c[0]+a*0.5, c[1]-a, c[2]+a*0.5]); }
        "color_wheels" => {
            let g = |k: &str, d: f64| p(params, k, d, t);
            let (lr,lg,lb) = (g("lift_r",0.0),g("lift_g",0.0),g("lift_b",0.0));
            let (mr,mg,mb) = (g("gamma_r",1.0),g("gamma_g",1.0),g("gamma_b",1.0));
            let (hr,hg,hb) = (g("gain_r",1.0),g("gain_g",1.0),g("gain_b",1.0));
            px(f, |c| {
                let o = |v: f32, l: f32, m: f32, h: f32| { let v = (v*h + l*(1.0-v*h)).max(0.0); v.powf(1.0/m.max(0.01)) };
                [o(c[0],lr,mr,hr), o(c[1],lg,mg,hg), o(c[2],lb,mb,hb)]
            });
        }
        "curves" => { let a = p(params, "amount", 0.0, t); px(f, |c| { let s = |v: f32| { let v = v.clamp(0.0,1.0); let sc = v*v*(3.0-2.0*v); v + (sc - v)*a }; [s(c[0]), s(c[1]), s(c[2])] }); }
        "teal_orange" => {
            let a = p(params, "amount", 0.5, t);
            px(f, |c| { let l = lum(c).clamp(0.0, 1.0);
                // shadows -> teal, highlights -> orange
                let sh = (1.0 - l).powi(2) * a * 0.12; let hi = l.powi(2) * a * 0.14;
                [c[0] - sh*1.0 + hi*1.0, c[1] + sh*0.35 + hi*0.25, c[2] + sh*0.9 - hi*1.0] });
        }
        "vignette" if crate::vignette::extended(params) => crate::vignette::apply(params, f, t),
        "vignette" => {
            let a = p(params, "amount", 0.5, t); let s = p(params, "softness", 0.5, t).max(0.05);
            let (w, h) = (f.w as f32, f.h as f32);
            for y in 0..f.h { for x in 0..f.w {
                let dx = (x as f32 / w - 0.5) * 2.0; let dy = (y as f32 / h - 0.5) * 2.0;
                let r = (dx*dx + dy*dy).sqrt() / 1.414;
                let v = 1.0 - a * ((r - (1.0 - s)).max(0.0) / s).powi(2).min(1.0);
                let i = (y*f.w+x)*4; f.d[i]*=v; f.d[i+1]*=v; f.d[i+2]*=v; } }
        }
        "blur" => { let r = p(params, "radius", 4.0, t) as usize; box_blur3(f, r); }
        "glow" if crate::glow::mode(params, t) => crate::glow::apply_deep(params, f, t),
        "glow" => {
            let a = p(params, "amount", 0.5, t); let r = p(params, "radius", 12.0, t) as usize; let th = p(params, "threshold", 0.7, t);
            let mut b = Frame { w: f.w, h: f.h, d: f.d.clone() };
            for i in 0..f.w*f.h { let c = [b.d[i*4],b.d[i*4+1],b.d[i*4+2]]; let l = lum(c);
                let k = ((l - th)/(1.0-th).max(0.01)).clamp(0.0,1.0); for j in 0..3 { b.d[i*4+j] *= k; } }
            box_blur3(&mut b, r);
            for i in 0..f.w*f.h { for j in 0..3 { f.d[i*4+j] += b.d[i*4+j]*a; } }
        }
        "bloom" => {
            // Light-intensity-responsive bloom: bright-pass with a soft knee that keeps the pixel's colour and scales with its excess over the
            // threshold (so a 4x brighter light blooms 4x more, even above 1.0), then a multi-scale blur (radius, 2r, 4r, ...) averaged with
            // equal weights. mode 1 (default) conserves energy: what the bloom adds is taken out of the bright pixels, so the frame sum is unchanged.
            let conserve = p(params, "conserve", 1.0, t) >= 0.5;
            let a = p(params, "amount", 0.6, t).max(0.0); let a = if conserve { a.min(1.0) } else { a };
            let th = p(params, "threshold", 0.8, t).max(0.0); let knee = p(params, "knee", 0.2, t).clamp(0.0, 1.0);
            let r0 = p(params, "radius", 8.0, t).clamp(1.0, 256.0) as usize; let levels = (p(params, "levels", 5.0, t).round() as usize).clamp(1, 7);
            let tint = [p(params, "tint_r", 1.0, t), p(params, "tint_g", 1.0, t), p(params, "tint_b", 1.0, t)];
            let n = f.w * f.h; let mut ex = vec![0f32; n * 3];
            for i in 0..n { let c = [f.d[i*4], f.d[i*4+1], f.d[i*4+2]]; let l = lum(c); if l <= 1e-6 { continue; }
                // soft knee: quadratic ramp over [th - knee*th, th + knee*th], linear above
                let e = crate::glow::knee_excess(l, th, knee);
                let k = e / l; for j in 0..3 { ex[i*3+j] = c[j] * k; } }
            let mut acc = vec![0f32; n * 3];
            for lv in 0..levels {
                let mut b = Frame { w: f.w, h: f.h, d: vec![0.0; n * 4] };
                for i in 0..n { for j in 0..3 { b.d[i*4+j] = ex[i*3+j]; } b.d[i*4+3] = 1.0; }
                box_blur3(&mut b, (r0 << lv).min(512));
                for i in 0..n { for j in 0..3 { acc[i*3+j] += b.d[i*4+j] / levels as f32; } }
            }
            for i in 0..n { let a0 = f.d[i*4+3]; let mut extra = [0f32; 3]; let mut mx = 0f32;
                for j in 0..3 { let rm = if conserve { ex[i*3+j] } else { 0.0 }; let add = a * acc[i*3+j] * tint[j];
                    f.d[i*4+j] = (f.d[i*4+j] - a * rm + add).max(0.0); extra[j] = add; mx = mx.max(add); }
                // light that spills past the layer's alpha: grow alpha so the glow shows outside the shape (straight alpha, premultiplied sum)
                if a0 < 1.0 && mx > 0.0 { let k = 1.0 - a0; let an = a0 + k * mx.min(1.0);
                    if an > 1e-6 { for j in 0..3 { let p = (f.d[i*4+j] - extra[j]) * a0 + k * extra[j]; f.d[i*4+j] = p / an; } f.d[i*4+3] = an; } } }
        }
        "grain" => {
            let a = p(params, "amount", 0.08, t);
            // Film Look Creator-style shaping; with every shaping param at its default the legacy per-pixel mono grain below runs unchanged.
            let q = |k: &str, d: f64| params.get(k).map(|x| x.at(t)).unwrap_or(d);
            let sh = [("size", 1.0, 1.0, 32.0), ("softness", 0.0, 0.0, 1.0), ("color", 0.0, 0.0, 1.0), ("shadows", 1.0, 0.0, 2.0), ("mids", 1.0, 0.0, 2.0), ("highlights", 1.0, 0.0, 2.0), ("red", 1.0, 0.0, 2.0), ("green", 1.0, 0.0, 2.0), ("blue", 1.0, 0.0, 2.0)];
            let mut v = [0.0f64; 9];
            for (i, (k, d, lo, hi)) in sh.iter().enumerate() { v[i] = q(k, *d); if !v[i].is_finite() || v[i] < *lo || v[i] > *hi { return Err(format!("grain: {k} must be in {lo}..{hi}, got {}", v[i])); } }
            if sh.iter().zip(&v).any(|(s, x)| *x != s.1) { crate::color::grain_look::apply(a as f64, v[0], v[1], v[2], [v[3], v[4], v[5]], [v[6], v[7], v[8]], f, seed); return Ok(()); }
            let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            for i in 0..f.w*f.h { s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let n = ((s >> 40) as f32 / 16777216.0 - 0.5) * a; for j in 0..3 { f.d[i*4+j] += n; } }
        }
        "rgb_split" => {
            let amount=p(params,"amount",6.,t);let offsets=[p(params,"r_offset",amount as f64,t),p(params,"g_offset",0.,t),p(params,"b_offset",-amount as f64,t)];let angle=p(params,"angle",0.,t).to_radians();let src=f.d.clone();let(w,h)=(f.w as isize,f.h as isize);
            for y in 0..f.h{crate::guard::check();for x in 0..f.w{for j in 0..3{let dx=(offsets[j]*angle.cos()) as isize;let dy=(offsets[j]*angle.sin()) as isize;let sx=(x as isize).saturating_add(dx).clamp(0,w-1) as usize;let sy=(y as isize).saturating_add(dy).clamp(0,h-1) as usize;f.d[(y*f.w+x)*4+j]=src[(sy*f.w+sx)*4+j];}}}
        }
        "invert" => px(f, |c| [1.0-c[0], 1.0-c[1], 1.0-c[2]]),
        "chroma_key" => {
            let k = [p(params,"r",0.0,t), p(params,"g",1.0,t), p(params,"b",0.0,t)];
            let tol = p(params,"tolerance",0.35,t); let soft = p(params,"softness",0.1,t).max(0.001);
            let space = p(params,"space",0.0,t); if space != 0.0 && space != 1.0 { return Err("chroma_key: space must be 0 (RGB) or 1 (YCbCr)".into()); }
            // Rec.709 Cb/Cr: space 1 ignores luma so a shaded screen still keys
            let cbcr = |c: &[f32]| { let y = 0.2126*c[0]+0.7152*c[1]+0.0722*c[2]; [(c[2]-y)/1.8556, (c[0]-y)/1.5748] }; let kc = cbcr(&k[..]);
            for i in 0..f.w*f.h { let c = &mut f.d[i*4..i*4+4];
                let dist = if space == 0.0 { ((c[0]-k[0]).powi(2)+(c[1]-k[1]).powi(2)+(c[2]-k[2]).powi(2)).sqrt()/1.732 }
                    else { let q = cbcr(&*c); ((q[0]-kc[0]).powi(2)+(q[1]-kc[1]).powi(2)).sqrt()/std::f32::consts::SQRT_2 };
                let al = ((dist - tol)/soft).clamp(0.0,1.0); c[3] *= al;
                if al < 1.0 && c[1] > c[0].max(c[2]) { c[1] = c[0].max(c[2]); } } // despill
        }
        "letterbox" => {
            let ratio = p(params, "ratio", 2.39, t); let vis_h = ((f.w as f32 / ratio) as usize).min(f.h);
            let bar = (f.h - vis_h)/2;
            for y in 0..f.h { if y < bar || y >= f.h - bar { for x in 0..f.w { let i=(y*f.w+x)*4; f.d[i]=0.0; f.d[i+1]=0.0; f.d[i+2]=0.0; f.d[i+3]=1.0; } } }
        }
        "lut" => return crate::color::lut::apply(params, strings, f, t),
        "hsl" => {
            let g = |k: &str| p(params, k, 0.0, t);
            const BANDS: [(&str, f32); 6] = [("red",0.0),("yellow",60.0),("green",120.0),("cyan",180.0),("blue",240.0),("magenta",300.0)];
            px(f, |c| { let (mut h, mut s, mut l) = rgb2hsl(c);
                let (mut dh, mut ds, mut dl) = (g("hue"), g("sat"), g("lum"));
                for (n, c0) in BANDS { let mut d = (h - c0).abs(); if d > 180.0 { d = 360.0 - d; } let w = (1.0 - d/60.0).max(0.0);
                    if w > 0.0 { dh += g(&format!("{n}_hue"))*w; ds += g(&format!("{n}_sat"))*w; dl += g(&format!("{n}_lum"))*w; } }
                h = (h + dh*60.0).rem_euclid(360.0); s = (s*(1.0+ds)).clamp(0.0,1.0); l = (l + dl*0.5).clamp(0.0,1.0); hsl2rgb(h, s, l) });
        }
        "sharpen" => { let a = p(params, "amount", 0.8, t); let r = p(params, "radius", 2.0, t) as usize; let mut b = Frame { w: f.w, h: f.h, d: f.d.clone() }; box_blur3(&mut b, r);
            for i in 0..f.w*f.h { for j in 0..3 { f.d[i*4+j] += (f.d[i*4+j] - b.d[i*4+j]) * a; } } }
        "pixelate" => { let s = (p(params, "size", 12.0, t) as usize).max(1); let src = f.d.clone();
            for y in 0..f.h { for x in 0..f.w { let (sx, sy) = ((x / s * s + s / 2).min(f.w-1), (y / s * s + s / 2).min(f.h-1)); for j in 0..4 { f.d[(y*f.w+x)*4+j] = src[(sy*f.w+sx)*4+j]; } } } }
        "posterize" => { let l = p(params, "levels", 5.0, t).max(2.0); px(f, |c| [(c[0].clamp(0.0,1.0)*l).floor()/(l-1.0), (c[1].clamp(0.0,1.0)*l).floor()/(l-1.0), (c[2].clamp(0.0,1.0)*l).floor()/(l-1.0)]); }
        "sepia" => { let a = p(params, "amount", 1.0, t); px(f, |c| { let l = lum(c); let s = [l*1.07, l*0.74, l*0.43]; [c[0]+(s[0]-c[0])*a, c[1]+(s[1]-c[1])*a, c[2]+(s[2]-c[2])*a] }); }
        "color_isolate" => { let (h0, rg, so, bw) = (p(params, "hue", 190.0, t), p(params, "range", 40.0, t), p(params, "softness", 25.0, t).max(1.0), p(params, "bw_amount", 1.0, t));
            px(f, |c| { let (h, s, _) = rgb2hsl(c); let mut d = (h - h0).abs(); if d > 180.0 { d = 360.0 - d; } let keep = (1.0 - ((d - rg) / so).clamp(0.0, 1.0)) * (s * 2.0).min(1.0); let l = lum(c); let k = keep.max(1.0 - bw);
                [l + (c[0]-l)*k, l + (c[1]-l)*k, l + (c[2]-l)*k] }); }
        "shake" => { let a = p(params, "amount", 8.0, t); let sp = p(params, "speed", 24.0, t); let n = |k: f32| (((k * p(params,"noise_frequency",12.9898,t)).sin() * p(params,"noise_gain",43758.545,t)).fract()) * 2.0 - 1.0; let step = (t as f32 * sp).floor();
            let (dx, dy) = ((n(step) * a) as isize, (n(step + p(params,"y_seed_offset",7.,t)) * p(params,"amount_y",a as f64,t)) as isize); let src = f.d.clone(); let (w, h) = (f.w as isize, f.h as isize);
            for y in 0..h { for x in 0..w { let (sx, sy) = ((x - dx).clamp(0, w-1), (y - dy).clamp(0, h-1)); for j in 0..4 { f.d[((y*w+x)*4) as usize+j] = src[((sy*w+sx)*4) as usize+j]; } } } }
        "impact_shake" => {
            let hits: Vec<f64> = strings.get("hits").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
            let k = f.h as f32 / 720.0; let (ax, ay) = (p(params, "amount", 29.0, t) * k, p(params, "amount_y", 9.0, t) * k); let (dec, fr) = (p(params, "decay", 27.0, t), p(params, "frequency", 12.5, t));
            let (rot, zm) = (p(params, "rotation", 0.0, t), p(params, "zoom", 0.0, t));
            let (mut dx, mut dy, mut dr, mut dz) = (0f32, 0f32, 0f32, 0f32);
            if let Some(pr) = strings.get("profile") { // measured per-frame kick table "dx,dy;dx,dy;..." (pixels @720p), discrete frames like the reference
                let tab: Vec<(f32, f32)> = pr.split(';').filter_map(|s| { let mut it = s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()); Some((it.next()?, it.next()?)) }).collect();
                let fps: f64 = strings.get("fps").and_then(|s| s.parse().ok()).unwrap_or(25.0); let sc = p(params, "scale", 1.0, t);
                for h in hits.iter() { let fi = ((t - h) * fps + 1e-4).floor(); if fi >= 0.0 && (fi as usize) < tab.len() { let (a, b) = tab[fi as usize]; dx += a * k * sc; dy += b * k * sc; } }
            } else {
            for (n, h) in hits.iter().enumerate() { let dt = (t - h) as f32; if dt < 0.0 || dt > 1.5 { continue; }
                let env = (-dec * dt).exp(); let osc = (std::f32::consts::TAU * fr * dt).cos(); let sg: f32 = if n % 2 == 0 { 1.0 } else { -1.0 } * if (n / 2) % 2 == 0 { 1.0 } else { 0.7 };
                dx += ax * env * osc; dy -= ay * env; dr += rot * env * osc * sg; dz += zm * env; }
            }
            if dx.abs() < 1e-3 && dy.abs() < 1e-3 && dr.abs() < 1e-4 && dz.abs() < 1e-5 { return Ok(()); }
            let src = f.d.clone(); let (w, h) = (f.w, f.h); let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0); let (s_, c_) = (dr.to_radians().sin(), dr.to_radians().cos()); let inv = 1.0 / (1.0 + dz);
            for y in 0..h { for x in 0..w { let (px, py) = (x as f32 - cx - dx, y as f32 - cy - dy); let sx = ((px * c_ + py * s_) * inv + cx).clamp(0.0, w as f32 - 1.0); let sy = ((-px * s_ + py * c_) * inv + cy).clamp(0.0, h as f32 - 1.0);
                let (x0, y0) = (sx.floor() as usize, sy.floor() as usize); let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1)); let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
                for j in 0..4 { let g = |xx: usize, yy: usize| src[(yy * w + xx) * 4 + j]; f.d[(y * w + x) * 4 + j] = g(x0, y0) * (1.0 - fx) * (1.0 - fy) + g(x1, y0) * fx * (1.0 - fy) + g(x0, y1) * (1.0 - fx) * fy + g(x1, y1) * fx * fy; } } } }
        "mirror" => { let v = p(params, "axis", 0.0, t) > 0.5; for y in 0..f.h { for x in 0..f.w { let (sx, sy) = if v { (x, if y >= f.h/2 { f.h-1-y } else { y }) } else { (if x >= f.w/2 { f.w-1-x } else { x }, y) };
            for j in 0..4 { f.d[(y*f.w+x)*4+j] = f.d[(sy*f.w+sx)*4+j]; } } } }
        "scanlines" => { let (a, s) = (p(params, "amount", 0.3, t), (p(params, "spacing", 3.0, t) as usize).max(2)); for y in 0..f.h { if y % s == 0 { for x in 0..f.w { for j in 0..3 { f.d[(y*f.w+x)*4+j] *= 1.0 - a; } } } } }
        "wave" => { let (a, wl, sp) = (p(params, "amplitude", 10.0, t), p(params, "wavelength", 80.0, t), p(params, "speed", 3.0, t)); let src = f.d.clone();
            for y in 0..f.h { let off = (a * ((y as f32 / wl + t as f32 * sp) * 6.2831853).sin()) as isize; for x in 0..f.w { let sx = (x as isize + off).clamp(0, f.w as isize - 1) as usize; for j in 0..4 { f.d[(y*f.w+x)*4+j] = src[(y*f.w+sx)*4+j]; } } } }
        "light_leak" => { let (a, sp) = (p(params, "amount", 0.6, t), p(params, "speed", 0.5, t)); let ph = t as f32 * sp * 6.2831853; let (w, h) = (f.w as f32, f.h as f32);
            // editable: centre (x, y, 0..1), sideways travel (sway amplitude), size (reach), light colour; the defaults are the original warm leak, bit for bit
            let (x0, y0, sz, tv) = (p(params, "x", 0.5, t), p(params, "y", 0.1, t), p(params, "size", 1.0, t).max(0.01), p(params, "travel", 0.5, t)); let col = [p(params, "color_r", 1.0, t), p(params, "color_g", 0.45, t), p(params, "color_b", 0.12, t)];
            for y in 0..f.h { for x in 0..f.w { let (u, v) = (x as f32 / w, y as f32 / h); let cx = x0 + tv * ph.sin(); let d = ((u - cx).powi(2) * 2.0 + (v - y0).powi(2)).sqrt(); let k = (1.0 - d * 1.8 / sz).max(0.0).powi(2) * a * (0.7 + 0.3 * (ph * 2.0).sin());
                let i = (y*f.w+x)*4; f.d[i] += k * col[0]; f.d[i+1] += k * col[1]; f.d[i+2] += k * col[2]; } } }
        "zoom_blur" => { let a = p(params, "amount", 0.06, t); let src = f.d.clone(); let (w, h) = (f.w as f32, f.h as f32); let n = 8;
            for y in 0..f.h { for x in 0..f.w { let mut acc = [0f32; 4]; for k in 0..n { let s = 1.0 - a * k as f32 / n as f32; let sx = ((x as f32 - w/2.0) * s + w/2.0).clamp(0.0, w-1.0) as usize; let sy = ((y as f32 - h/2.0) * s + h/2.0).clamp(0.0, h-1.0) as usize; for j in 0..4 { acc[j] += src[(sy*f.w+sx)*4+j]; } }
                for j in 0..4 { f.d[(y*f.w+x)*4+j] = acc[j] / n as f32; } } } }
        "silhouette_fill" => { let (w, h) = (f.w, f.h); let (r, g, b, am) = (p(params, "r", 1.0, t), p(params, "g", 0.85, t), p(params, "b", 0.1, t), p(params, "amount", 1.0, t).clamp(0.0, 1.0));
            let ow = (p(params, "outline", 0.0, t) * h as f32 / 720.0).round().max(0.0) as isize; let (or_, og, ob) = (p(params, "or", 1.0, t), p(params, "og", 1.0, t), p(params, "ob", 1.0, t));
            if ow > 0 { // separable max filter of alpha = dilated mask; ring = dilated minus original
                let a0: Vec<f32> = (0..w*h).map(|i| f.d[i*4+3]).collect(); let mut tmp = vec![0.0f32; w*h]; let mut dil = vec![0.0f32; w*h];
                for y in 0..h { for x in 0..w { let mut m = 0.0f32; for k in -ow..=ow { let xx = x as isize + k; if xx >= 0 && (xx as usize) < w { m = m.max(a0[y*w+xx as usize]); } } tmp[y*w+x] = m; } }
                for y in 0..h { for x in 0..w { let mut m = 0.0f32; for k in -ow..=ow { let yy = y as isize + k; if yy >= 0 && (yy as usize) < h { m = m.max(tmp[yy as usize*w+x]); } } dil[y*w+x] = m; } }
                for i in 0..w*h { let a = a0[i]; let ring = (dil[i] - a).max(0.0);
                    if a > 0.0 { for (j, c) in [r, g, b].iter().enumerate() { f.d[i*4+j] = f.d[i*4+j] * (1.0 - am) + c * am; } }
                    if ring > 0.0 { let na = a + ring * (1.0 - a); for (j, c) in [or_, og, ob].iter().enumerate() { f.d[i*4+j] = (f.d[i*4+j] * a + c * ring * (1.0 - a)) / na.max(1e-6); } f.d[i*4+3] = na; } }
            } else { for i in 0..w*h { if f.d[i*4+3] > 0.0 { for (j, c) in [r, g, b].iter().enumerate() { f.d[i*4+j] = f.d[i*4+j] * (1.0 - am) + c * am; } } } } }
        "luma_key" => { let (th, so, kb) = (p(params, "threshold", 0.2, t), p(params, "softness", 0.1, t).max(0.001), p(params, "keep_bright", 1.0, t) > 0.5);
            for i in 0..f.w*f.h { let l = lum([f.d[i*4], f.d[i*4+1], f.d[i*4+2]]); let k = ((l - th) / so).clamp(0.0, 1.0); f.d[i*4+3] *= if kb { k } else { 1.0 - k }; } }
        "ripple" => { let (a, wl, sp, cx, cy) = (p(params, "amplitude", 8.0, t), p(params, "wavelength", 60.0, t), p(params, "speed", 2.0, t), p(params, "cx", 0.5, t) * f.w as f32, p(params, "cy", 0.5, t) * f.h as f32); let src = f.d.clone(); let (w, h) = (f.w, f.h);
            for y in 0..h { for x in 0..w { let (dx, dy) = (x as f32 - cx, y as f32 - cy); let d = (dx*dx + dy*dy).sqrt().max(1.0); let o = a * ((d / wl - t as f32 * sp) * 6.2831853).sin() * (-d / (w as f32)).exp();
                let sx = (x as f32 + dx / d * o).clamp(0.0, w as f32 - 1.0) as usize; let sy = (y as f32 + dy / d * o).clamp(0.0, h as f32 - 1.0) as usize; for j in 0..4 { f.d[(y*w+x)*4+j] = src[(sy*w+sx)*4+j]; } } } }
        "lens_distortion" => { let a = p(params, "amount", 0.3, t); let src = f.d.clone(); let (w, h) = (f.w as f32, f.h as f32);
            for y in 0..f.h { for x in 0..f.w { let (nx, ny) = ((x as f32 / w - 0.5) * 2.0, (y as f32 / h - 0.5) * 2.0); let r2 = nx*nx + ny*ny; let k = 1.0 + a * r2; let (sx, sy) = (((nx * k) * 0.5 + 0.5) * w, ((ny * k) * 0.5 + 0.5) * h);
                let o = (y*f.w+x)*4; if sx < 0.0 || sy < 0.0 || sx >= w || sy >= h { f.d[o..o+4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]); continue; } let s = (sy as usize * f.w + sx as usize) * 4; for j in 0..4 { f.d[o+j] = src[s+j]; } } } }
        "glitch_blocks" => { let (a, n, rate) = (p(params, "amount", 30.0, t), p(params, "slices", 12.0, t).max(1.0) as usize, p(params, "rate", 12.0, t)); let step = (t as f32 * rate).floor(); let src = f.d.clone(); let (w, h) = (f.w, f.h); let sh=if params.get("slice_height").map(|q|q.at(t)).unwrap_or(0.)==0.{(h/n).max(1)}else{p(params,"slice_height",1.,t).max(1.) as usize};
            let rnd = |k: f32| (((k * p(params,"noise_frequency",12.9898,t)).sin() * p(params,"noise_gain",43758.545,t)).fract()).abs();
            for y in 0..h { let sl = (y / sh) as f32; let r = rnd(sl + step * p(params,"step_seed_scale",17.,t)); let off = if r > p(params,"slice_threshold",0.55,t) { ((rnd(sl * p(params,"slice_seed_scale",3.1,t) + step) * 2.0 - 1.0) * a) as isize } else { 0 };
                for x in 0..w { let sx = (x as isize + off).rem_euclid(w as isize) as usize; let o = (y*w+x)*4; f.d[o] = src[(y*w+sx)*4]; f.d[o+1] = src[(y*w+((sx as isize + off.signum().saturating_mul(p(params,"green_offset",4.,t) as isize)).rem_euclid(w as isize) as usize))*4+1]; f.d[o+2] = src[(y*w+sx)*4+2]; f.d[o+3] = src[(y*w+sx)*4+3]; } } }
        "light_rays" => { let (a, th, cx, cy, len) = (p(params, "amount", 0.8, t), p(params, "threshold", 0.6, t), p(params, "cx", 0.5, t), p(params, "cy", 0.2, t), p(params, "length", 0.4, t)); let (w, h) = (f.w as f32, f.h as f32); let n = 24; let src = f.d.clone();
            for y in 0..f.h { for x in 0..f.w { let mut acc = [0f32; 3]; for k in 0..n { let s = 1.0 - len * k as f32 / n as f32; let sx = ((x as f32 - cx*w) * s + cx*w).clamp(0.0, w - 1.0) as usize; let sy = ((y as f32 - cy*h) * s + cy*h).clamp(0.0, h - 1.0) as usize; let i = (sy*f.w+sx)*4;
                let l = lum([src[i], src[i+1], src[i+2]]); let m = ((l - th) / (1.0 - th).max(0.01)).clamp(0.0, 1.0) * (1.0 - k as f32 / n as f32); for j in 0..3 { acc[j] += src[i+j] * m; } }
                let o = (y*f.w+x)*4; for j in 0..3 { f.d[o+j] += acc[j] / n as f32 * a * 2.0; } } } }
        "drop_shadow" => { let (dx, dy, bl, op) = (p(params, "dx", 8.0, t) as isize, p(params, "dy", 8.0, t) as isize, p(params, "blur", 8.0, t) as usize, p(params, "opacity", 0.6, t)); let mut sh = Frame::new(f.w, f.h); let (w, h) = (f.w as isize, f.h as isize);
            for y in 0..h { for x in 0..w { let (sx, sy) = (x - dx, y - dy); if sx >= 0 && sy >= 0 && sx < w && sy < h { sh.d[((y*w+x)*4+3) as usize] = f.d[((sy*w+sx)*4+3) as usize]; } } } box_blur3(&mut sh, bl);
            for i in 0..f.w*f.h { let a = sh.d[i*4+3] * op; let o = &mut f.d[i*4..i*4+4]; let na = o[3] + a * (1.0 - o[3]); if na > 0.0 { for j in 0..3 { o[j] = (o[j] * o[3]) / na; } } o[3] = na; } }
        "chromatic_aberration" => { let a = p(params, "amount", 4.0, t); let src = f.d.clone(); let (w, h) = (f.w as f32, f.h as f32);
            for y in 0..f.h { for x in 0..f.w { let (nx, ny) = (x as f32 / w - p(params,"cx",0.5,t), y as f32 / h - p(params,"cy",0.5,t)); let o = (y*f.w+x)*4;
                for (j, s) in [(0usize, p(params,"r_scale",1.,t)), (2usize, p(params,"b_scale",-1.,t))] { let sx = (x as f32 + nx * a * p(params,"radial_scale",2.,t) * s).clamp(0.0, w - 1.0) as usize; let sy = (y as f32 + ny * a * p(params,"radial_scale",2.,t) * s).clamp(0.0, h - 1.0) as usize; f.d[o+j] = src[(sy*f.w+sx)*4+j]; } } } }
        "flicker" => { let (a, r) = (p(params, "amount", 0.15, t), p(params, "rate", 18.0, t)); let k = 1.0 - a * (((t as f32 * r).floor() * 78.233).sin() * 43758.545).fract().abs(); px(f, |c| [c[0]*k, c[1]*k, c[2]*k]); }
        "halftone" => { let s = p(params, "size", 8.0, t).max(2.0); let src = f.d.clone();
            for y in 0..f.h { for x in 0..f.w { let (cx, cy) = (((x as f32 / s).floor() + 0.5) * s, ((y as f32 / s).floor() + 0.5) * s); let (sx, sy) = ((cx as usize).min(f.w-1), (cy as usize).min(f.h-1)); let i = (sy*f.w+sx)*4; let l = lum([src[i], src[i+1], src[i+2]]).clamp(0.0, 1.0);
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt(); let r = s * 0.7 * l.sqrt(); let k = (r - d + 0.5).clamp(0.0, 1.0); let o = (y*f.w+x)*4; for j in 0..3 { f.d[o+j] = src[i+j] * k; } } } }
        "edge_glow" => { let (a, col) = (p(params, "amount", 1.0, t), [p(params, "r", 0.0, t), p(params, "g", 0.9, t), p(params, "b", 1.0, t)]); let src = f.d.clone(); let (w, h) = (f.w, f.h);
            let mut ed = Frame::new(w, h); for y in 1..h-1 { for x in 1..w-1 { let g = |xx: usize, yy: usize| { let i = (yy*w+xx)*4; lum([src[i], src[i+1], src[i+2]]) };
                let gx = g(x+1, y) - g(x-1, y); let gy = g(x, y+1) - g(x, y-1); let m = ((gx*gx + gy*gy).sqrt() * 3.0).min(1.0); let o = (y*w+x)*4; ed.d[o] = col[0]*m; ed.d[o+1] = col[1]*m; ed.d[o+2] = col[2]*m; ed.d[o+3] = 1.0; } }
            let mut bl = Frame { w, h, d: ed.d.clone() }; box_blur3(&mut bl, 6); for i in 0..w*h { for j in 0..3 { f.d[i*4+j] = f.d[i*4+j] * 0.35 + (ed.d[i*4+j] + bl.d[i*4+j]) * a; } } }
        "lens_undistort" | "lens_redistort" => return crate::ext::camera_tracker::lens_effect(kind, params, strings, f, t),
        other => return crate::effects2::apply2(other, params, f, t, seed),
    }
    Ok(())
}

pub(crate) fn lum(c: [f32; 3]) -> f32 { 0.2126*c[0] + 0.7152*c[1] + 0.0722*c[2] }
pub(crate) fn px(f: &mut Frame, op: impl Fn([f32; 3]) -> [f32; 3] + Sync) {
    let ptr=crate::par::SyncPtr(f.d.as_mut_ptr());crate::par::ranges(f.w*f.h,32768,|a,b|{let ptr=ptr;for i in a..b{let px=unsafe{std::slice::from_raw_parts_mut(ptr.0.add(i*4),4)};let o=op([px[0],px[1],px[2]]);px[0]=o[0];px[1]=o[1];px[2]=o[2];}});
}
pub fn box_blur3(f: &mut Frame, r: usize) {
    if r == 0 || f.w == 0 || f.h == 0 { return; }
    let r = (r / 2).max(1);
    let mut tmp = vec![0f32; f.d.len()];
    for _ in 0..3 { blur_axis(&f.d, &mut tmp, f.w, f.h, r, true); blur_axis(&tmp, &mut f.d, f.w, f.h, r, false); }
}
/// One clamped box pass of 4 channels, src -> dst. Bit-identical to the old per-line
/// running sum; the vertical pass walks column strips row by row (cache friendly).
fn blur_axis(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horiz: bool) {
    let win = (2*r+1) as f32;
    if horiz {
        crate::par::rows_mut(dst, w*4, |y, out| {
            let row = &src[y*w*4..(y+1)*w*4];
            let mut acc = [0f32; 4];
            for i in 0..=r.min(w-1) { for j in 0..4 { acc[j] += row[i*4+j]; } }
            for j in 0..4 { acc[j] += row[j] * r as f32; }
            for i in 0..w {
                for j in 0..4 { out[i*4+j] = acc[j]/win; }
                let a = (i+r+1).min(w-1)*4; let s = i.saturating_sub(r)*4;
                for j in 0..4 { acc[j] += row[a+j] - row[s+j]; }
            }
        });
    } else {
        const C: usize = 32;
        let strips = (w + C - 1) / C; let p = crate::par::SyncPtr(dst.as_mut_ptr());
        crate::par::ranges(strips, 1, |a, b| { let p = p; for st in a..b {
            let c0 = st*C; let c1 = (c0+C).min(w); let n = c1-c0; let mut acc = [[0f32; 4]; C];
            for i in 0..=r.min(h-1) { for c in 0..n { let o = (i*w+c0+c)*4; for j in 0..4 { acc[c][j] += src[o+j]; } } }
            for c in 0..n { let o = (c0+c)*4; for j in 0..4 { acc[c][j] += src[o+j] * r as f32; } }
            for i in 0..h {
                let ra = (i+r+1).min(h-1); let rs = i.saturating_sub(r);
                for c in 0..n { let o = (i*w+c0+c)*4; for j in 0..4 { unsafe { *p.0.add(o+j) = acc[c][j]/win; } }
                    let (oa, os) = ((ra*w+c0+c)*4, (rs*w+c0+c)*4); for j in 0..4 { acc[c][j] += src[oa+j] - src[os+j]; } }
            }
        } });
    }
}

pub(crate) fn rgb2hsl(c: [f32; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (c[0].clamp(0.0,1.0), c[1].clamp(0.0,1.0), c[2].clamp(0.0,1.0)); let mx = r.max(g).max(b); let mn = r.min(g).min(b); let l = (mx+mn)/2.0; let d = mx-mn;
    if d < 1e-6 { return (0.0, 0.0, l); } let s = d / (1.0 - (2.0*l-1.0).abs()).max(1e-6);
    let h = if mx == r { ((g-b)/d).rem_euclid(6.0) } else if mx == g { (b-r)/d + 2.0 } else { (r-g)/d + 4.0 }; (h*60.0, s, l)
}
pub(crate) fn hsl2rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0*l-1.0).abs()) * s; let x = c * (1.0 - ((h/60.0).rem_euclid(2.0) - 1.0).abs()); let m = l - c/2.0;
    let (r, g, b) = match (h/60.0) as u32 { 0 => (c,x,0.0), 1 => (x,c,0.0), 2 => (0.0,c,x), 3 => (0.0,x,c), 4 => (x,0.0,c), _ => (c,0.0,x) }; [r+m, g+m, b+m]
}
