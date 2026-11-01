//! After Effects-style effect library, part 2: generate, distort, stylize, matte, color, blur.
use crate::effects::{box_blur3, lum, p, Frame, Params};

type C = (&'static str, f64, &'static str);
pub const CATALOG2: &[(&str, &str, &[C])] = &[
    ("saber", "Saber: a neon light stroke along the layer's alpha outline (text, logo, shape): hot core + coloured glow, write-on reveal, flicker, wobble", &[("mode", 0.0, "0 outline (the alpha edge), 1 fill (the whole shape burns)"), ("core_size", 2.0, "core half-width px"), ("core_r", 1.0, "0..1"), ("core_g", 1.0, "0..1"), ("core_b", 1.0, "0..1"), ("glow_r", 0.15, "0..1"), ("glow_g", 0.55, "0..1"), ("glow_b", 1.0, "0..1"), ("glow_intensity", 1.5, "glow strength, 0..10"), ("glow_spread", 10.0, "glow reach px"), ("start", 0.0, "reveal start 0..1 along reveal_angle"), ("end", 1.0, "reveal end 0..1 (key 0 -> 1 = write-on)"), ("reveal_angle", 0.0, "reveal direction deg (0 left->right, 90 top->bottom)"), ("flicker", 0.0, "0..1 random brightness flicker"), ("flicker_speed", 12.0, "flickers/s"), ("distortion", 0.0, "stroke wobble px"), ("distortion_speed", 1.0, "wobble speed"), ("composite", 0.0, "0 over the layer, 1 the light alone (layer hidden)")]),
    ("light_sweep", "CC Light Sweep: a soft band of light crossing the layer (shine on text, logos, products), edge light on alpha edges", &[("position", 0.5, "0..1 along the sweep (0, 1 = just off frame)"), ("speed", 0.0, "sweeps/s on top of position, 0 = still"), ("angle", 0.0, "band tilt deg from vertical (90 = horizontal, moves down)"), ("width", 0.12, "band width, fraction of the travel"), ("shape", 1.0, "0 linear, 1 smooth, 2 sharp"), ("intensity", 1.0, "sweep light"), ("edge_intensity", 0.0, "light on the layer's alpha edges"), ("edge_width", 3.0, "edge px"), ("color_r", 1.0, "0..1"), ("color_g", 1.0, "0..1"), ("color_b", 1.0, "0..1"), ("reception", 0.0, "0 add, 1 composite (also past the layer), 2 cutout (light only)")]),
    ("twirl", "Twirl distortion around a point", &[("angle", 120.0, "degrees at center"), ("radius", 0.5, "fraction of min dimension"), ("cx", 0.5, "0..1"), ("cy", 0.5, "0..1")]),
    ("bulge", "Bulge/pinch (positive bulges out)", &[("amount", 0.6, "-1..1"), ("radius", 0.5, "fraction"), ("cx", 0.5, "0..1"), ("cy", 0.5, "0..1")]),
    ("polar", "Polar coordinates (rect to polar, or back with inverse=1)", &[("inverse", 0.0, "0/1")]),
    ("kaleidoscope", "Kaleidoscope mirror segments", &[("segments", 6.0, "count"), ("angle", 0.0, "degrees"), ("cx", 0.5, "0..1"), ("cy", 0.5, "0..1")]),
    ("turbulent_displace", "Noise-driven displacement (heat haze, liquid, organic warp)", &[("amount", 20.0, "px"), ("size", 80.0, "noise px"), ("speed", 0.5, "evolution/s")]),
    ("droste_zoom", "Droste/Escher infinite recursive zoom: the frame repeats into itself in a self-similar spiral tunnel (log-polar radius wrap, not a simple crop-zoom). mix fades the whole effect in/out (0 = untouched frame); drift spirals it inward over time; keyframe mix for a clean start/end window.", &[("cx", 0.5, "0..1"), ("cy", 0.5, "0..1"), ("zoom", 1.6, "recursion ratio per ring, >1.02"), ("twist", 12.0, "deg rotation added per ring, for a spiral rather than a plain tunnel"), ("drift", 0.12, "spiral inward speed, rings/s"), ("mix", 1.0, "0..1 blend with the untouched frame, 0 = off")]),
    ("portal_mirror", "Portal into an inverted world: a soft-edged circular window shows the same frame flipped and colour-inverted, like a doorway into upside-down reality. Combine with set_effect_mask for a non-circular portal shape, or with another clip's mask for a subject-shaped portal.", &[("cx", 0.5, "0..1"), ("cy", 0.5, "0..1"), ("radius", 0.28, "portal radius, fraction of the short side"), ("feather", 0.08, "edge softness, fraction of the short side"), ("invert_color", 1.0, "0/1, invert colour inside the portal"), ("flip_vertical", 1.0, "0/1, flip the world vertically inside the portal"), ("mix", 1.0, "0..1 blend with the untouched frame, 0 = off")]),
    ("melt_drip", "Directional dripping/melting warp: the image streaks downward (or any angle) with smear growing toward the far edge, driven by per-column noise -- a one-way melt, distinct from turbulent_displace's symmetric organic wobble.", &[("amount", 18.0, "px of maximum drip at the far edge"), ("size", 40.0, "noise feature width px, controls how wide each drip streak reads"), ("speed", 0.3, "drip animation speed"), ("angle", 90.0, "drip direction deg, 90 = straight down"), ("mix", 1.0, "0..1 blend with the untouched frame, 0 = off")]),
    ("vfx_graph", "KIMO 8.5 VFX graph: a small JSON node DAG (strings.graph) combining sources, warps, stylize, colour and composite -- including the whole existing effect catalog as one generic node -- into one effect, with an optional driven mask/depth clip (strings.mask_clip_id). See vfx_graph_describe for the node catalog.", &[("mix", 1.0, "0..1 blend with the untouched frame, 0 = off")]),
    ("offset", "Offset with wrap-around", &[("dx", 0.0, "px"), ("dy", 0.0, "px")]),
    ("motion_tile", "Tile the frame (optionally mirrored)", &[("tiles", 2.0, "per axis"), ("mirror", 0.0, "0/1")]),
    ("spin_blur", "Rotational blur", &[("angle", 10.0, "degrees"), ("cx", 0.5, "0..1"), ("cy", 0.5, "0..1")]),
    ("directional_blur", "Motion blur along an angle", &[("length", 24.0, "px"), ("angle", 0.0, "degrees")]),
    ("flip", "Flip frame", &[("horizontal", 1.0, "0/1"), ("vertical", 0.0, "0/1")]),
    ("tilt_shift", "Miniature tilt-shift blur outside a focus band", &[("center", 0.5, "0..1 y"), ("band", 0.15, "half-height fraction"), ("radius", 10.0, "blur px")]),
    ("channel_blur", "Blur R, G and B independently by different amounts (chromatic-fringe / prism look); alpha is untouched", &[("r_radius", 4.0, "px"), ("g_radius", 4.0, "px"), ("b_radius", 4.0, "px")]),
    ("smart_blur", "Smooths flat/noisy areas only; real edges (luma change above threshold) are left exactly sharp", &[("radius", 4.0, "px"), ("threshold", 0.1, "0..1 luma change that counts as a real edge")]),
    ("reduce_interlace_flicker", "Vertical-only low-pass (3-tap 1-2-1) that suppresses thin horizontal detail prone to interlace flicker", &[("amount", 0.5, "0..1 blend strength")]),
    ("fractal_noise", "Animated fractal (fbm) noise: clouds, smoke, texture. mode 0 replace, 1 overlay, 2 add, 3 multiply, 4 alpha-matte", &[("scale", 160.0, "px"), ("complexity", 4.0, "octaves 1..8"), ("speed", 0.3, "evolution/s"), ("contrast", 1.4, "0..4"), ("brightness", 0.0, "-1..1"), ("opacity", 1.0, "0..1"), ("mode", 1.0, "0..4")]),
    ("gradient_ramp", "Linear/radial color ramp. mode 0 replace, 1 overlay, 2 multiply, 3 add", &[("r0", 0.0, "start R"), ("g0", 0.0, "start G"), ("b0", 0.0, "start B"), ("r1", 1.0, "end R"), ("g1", 0.5, "end G"), ("b1", 0.0, "end B"), ("x0", 0.5, "0..1"), ("y0", 0.0, "0..1"), ("x1", 0.5, "0..1"), ("y1", 1.0, "0..1"), ("radial", 0.0, "0/1"), ("opacity", 1.0, "0..1"), ("mode", 0.0, "0..3")]),
    ("checkerboard", "Checkerboard generator", &[("size", 40.0, "px"), ("r", 1.0, "R"), ("g", 1.0, "G"), ("b", 1.0, "B"), ("opacity", 0.5, "0..1")]),
    ("grid", "Grid lines generator", &[("size", 60.0, "px"), ("width", 2.0, "px"), ("r", 1.0, "R"), ("g", 1.0, "G"), ("b", 1.0, "B"), ("opacity", 0.5, "0..1")]),
    ("lightning", "Animated electric arc / energy beam between two points with branches and glow. strike=1 for discrete instantaneous strikes (gaps between flashes) instead of a continuous flickering arc; branch_decay controls how much dimmer/thinner each branch generation is vs its parent", &[("x0", 0.1, "0..1"), ("y0", 0.5, "0..1"), ("x1", 0.9, "0..1"), ("y1", 0.5, "0..1"), ("jitter", 40.0, "px chaos (0 = straight beam)"), ("width", 2.5, "core px"), ("glow", 14.0, "glow px"), ("branches", 3.0, "count"), ("branch_decay", 0.5, "0..1 branch width/glow/intensity falloff vs the parent bolt (0.5 = original look)"), ("rate", 18.0, "re-roll Hz"), ("r", 0.55, "R"), ("g", 0.75, "G"), ("b", 1.0, "B"), ("intensity", 1.0, "0..3"), ("strike", 0.0, "0/1: 1 = discrete instantaneous strike flashes with gaps between, 0 = continuous flickering arc"), ("strike_duty", 0.12, "0..1 fraction of each rate-cycle visible when strike=1")]),
    ("lens_flare", "Lens flare with ghosts, ring and anamorphic streak. mode 1: axis elements, strings source_clip/source_light/occluder", &[("x", 0.75, "0..1"), ("y", 0.25, "0..1"), ("brightness", 1.0, "0..3"), ("streak", 1.0, "0..2"),
        ("mode", 0.0, "0 classic, 1 v2 (elements on the source->centre axis)"), ("halo", 1.0, "mode 1: halo + core intensity, 0 = off"), ("halo_size", 0.12, "mode 1: fraction of min dimension"), ("ring", 0.25, "mode 1: ring intensity, 0 = off"), ("ring_size", 0.25, "mode 1: ring radius, fraction of min dimension"),
        ("ghosts", 0.35, "mode 1: ghost intensity, 0 = off"), ("ghost_count", 5.0, "mode 1: 0..8"), ("ghost_spacing", 0.4, "mode 1: axis step (1 = frame centre)"), ("ghost_size", 0.03, "mode 1: fraction of min dimension"),
        ("streak_length", 0.45, "mode 1: fraction of width"), ("streak_width", 1.5, "mode 1: px sigma"), ("starburst", 0.5, "mode 1: ray intensity, 0 = off"), ("rays", 8.0, "mode 1: ray count"), ("ray_length", 0.2, "mode 1: fraction of min dimension"), ("ray_angle", 0.0, "mode 1: degrees"),
        ("source_r", 8.0, "mode 1: px radius of the disc strings.occluder alpha is averaged over")]),
    ("anamorphic_streak", "Horizontal anamorphic light streaks from highlights", &[("threshold", 0.75, "0..1"), ("length", 120.0, "px"), ("amount", 0.8, "0..3"), ("r", 0.5, "tint R"), ("g", 0.7, "tint G"), ("b", 1.0, "tint B")]),
    ("strobe", "Strobe flash (white or black)", &[("rate", 8.0, "Hz"), ("duty", 0.2, "0..1"), ("black", 0.0, "0/1"), ("amount", 1.0, "0..1")]),
    ("gradient_map", "Map luminance to 3-color gradient (duotone/tritone)", &[("sr", 0.02, "shadow R"), ("sg", 0.05, "shadow G"), ("sb", 0.2, "shadow B"), ("mr", 0.8, "mid R"), ("mg", 0.2, "mid G"), ("mb", 0.4, "mid B"), ("hr", 1.0, "high R"), ("hg", 0.9, "high G"), ("hb", 0.6, "high B"), ("amount", 1.0, "0..1")]),
    ("levels", "Levels (input black/white, gamma, output black/white)", &[("in_black", 0.0, "0..1"), ("in_white", 1.0, "0..1"), ("gamma", 1.0, "0.1..4"), ("out_black", 0.0, "0..1"), ("out_white", 1.0, "0..1")]),
    ("brightness_contrast", "Brightness and contrast", &[("brightness", 0.0, "-1..1"), ("contrast", 0.0, "-1..1")]),
    ("channel_mixer", "3x3 RGB channel mixer", &[("rr", 1.0, "R from R"), ("rg", 0.0, "R from G"), ("rb", 0.0, "R from B"), ("gr", 0.0, "G from R"), ("gg", 1.0, "G from G"), ("gb", 0.0, "G from B"), ("br", 0.0, "B from R"), ("bg", 0.0, "B from G"), ("bb", 1.0, "B from B")]),
    ("threshold", "Black/white threshold", &[("level", 0.5, "0..1")]),
    ("colorize", "Tint luminance with a color", &[("r", 1.0, "R"), ("g", 0.7, "G"), ("b", 0.4, "B"), ("amount", 1.0, "0..1")]),
    ("shadow_highlight", "Lift shadows / recover highlights", &[("shadows", 0.3, "-1..1"), ("highlights", 0.3, "-1..1 (positive recovers)")]),
    ("emboss", "Emboss relief", &[("strength", 1.0, "0..4"), ("angle", 135.0, "degrees")]),
    ("find_edges", "Sobel edge detection", &[("invert", 0.0, "0/1 (1 = dark lines on white)"), ("amount", 1.5, "0..5")]),
    ("pixel_sort", "Glitch art pixel sorting by brightness", &[("threshold", 0.35, "0..1 start of sorted runs"), ("max_run", 400.0, "px")]),
    ("vhs", "Analog VHS: line wobble, chroma bleed, noise, tracking bands", &[("wobble", 3.0, "px"), ("bleed", 5.0, "px"), ("noise", 0.12, "0..1"), ("bands", 1.0, "0..1")]),
    ("film_damage", "Scratches, dust specks and gate flicker", &[("amount", 0.5, "0..1")]),
    ("choker", "Matte choker: positive shrinks the alpha edge, negative expands", &[("amount", 2.0, "px")]),
    ("shift_channels", "Reassign each output channel (R/G/B/A) from any input channel, a constant, or an inverted channel -- AE's Shift Channels. Each take_* picks the source: 0 R, 1 G, 2 B, 3 A, 4 constant 0, 5 constant 1, 6 inverted R (1-R), 7 inverted G, 8 inverted B, 9 inverted A", &[("take_r", 0.0, "source for output R"), ("take_g", 1.0, "source for output G"), ("take_b", 2.0, "source for output B"), ("take_a", 3.0, "source for output A")]),
    ("solid_composite", "AE's Solid Composite: fills behind or in front of the layer with a flat colour at the given opacity, inside the effect itself (no separate layer needed)", &[("r", 0.0, "fill R"), ("g", 0.0, "fill G"), ("b", 0.0, "fill B"), ("opacity", 1.0, "0..1 fill opacity"), ("behind", 1.0, "1 = fill is composited behind the layer (classic use: fill transparent gaps); 0 = the fill is composited in front, with the layer showing through by its own alpha")]),
    ("remove_color_matting", "AE's Remove Color Matting: undoes a straight-alpha layer's fringe from having been composited against a solid background color before it reached you (the common case: keyed/alpha footage re-exported over a colour instead of saved with clean alpha). Divides the background's contribution back out: original = (observed - bg*(1-alpha)) / alpha. Wrong bg or already-clean footage will visibly shift colours, so it only helps on footage that actually has the fringe", &[("r", 1.0, "matting background R"), ("g", 1.0, "matting background G"), ("b", 1.0, "matting background B")]),
    ("median", "Median filter: each pixel's RGB becomes the per-channel median of its own square neighbourhood -- removes isolated speckle noise while keeping hard edges sharper than a blur would (classic cleanup/denoise tool). Alpha is untouched", &[("radius", 1.0, "1..4 px, window is (2*radius+1) square")]),
    ("dust_scratches", "AE's Dust & Scratches: despeckle, not a general blur. A pixel is replaced by its neighbourhood's per-channel median ONLY where it differs from that median by more than threshold (an isolated bright/dark speck, the signature of film dust and scratches); pixels close to their local median are left exactly alone, so clean footage is untouched", &[("radius", 2.0, "1..6 px, window is (2*radius+1) square"), ("threshold", 0.15, "0..1 per-channel distance from the local median that counts as a defect; lower catches more (and risks softening real detail)")]),
    ("feather_alpha", "Blur only the alpha channel (soft edges)", &[("radius", 8.0, "px")]),
    ("spill_suppress", "Remove green/blue spill after keying", &[("amount", 1.0, "0..1; only changes pixels where the spill channel exceeds the other two, so a plate with no spill is unchanged"), ("channel", 1.0, "1 green, 2 blue")]),
    ("outline", "Stroke around alpha shape (text/shapes/stickers)", &[("width", 3.0, "px"), ("r", 1.0, "R"), ("g", 1.0, "G"), ("b", 1.0, "B")]),
    ("outer_glow", "Colored glow around alpha shape", &[("radius", 16.0, "px"), ("amount", 1.2, "0..4"), ("r", 0.3, "R"), ("g", 0.8, "G"), ("b", 1.0, "B")]),
    ("long_shadow", "Flat-design long shadow from alpha", &[("length", 80.0, "px"), ("angle", 45.0, "degrees"), ("opacity", 0.5, "0..1")]),
    ("cast_shadow", "Light-driven soft shadow of the layer onto what is below: the layer's alpha is projected onto the receiving plane from a point light (first scene light, or light_x/y/z) with a penumbra from a finite-size light (radius = light_size * H / (Lz - H); one constant height, no per-point contact hardening). Runs after the layer transform; written under the layer.", &[("height", 0.08, "caster height above the receiving plane in frame heights"), ("light_x", 0.2, "light x, fraction of width (ignored when a scene light is used)"), ("light_y", 0.1, "light y, fraction of height"), ("light_z", 0.8, "light height in frame heights"), ("use_scene_light", 1.0, "1 = use the first non-directional scene light if one exists"), ("opacity", 0.6, "0..1 shadow darkness"), ("light_size", 0.05, "light size as a fraction of frame height: bigger = softer shadow, 0 = hard edge"), ("r", 0.0, "shadow R"), ("g", 0.0, "shadow G"), ("b", 0.0, "shadow B")]),
    ("light_wrap", "Light wrap: backdrop light bleeds over the layer's edges, wrap = blur(backdrop) * blur(1 - alpha) * alpha. After the transform; needs a layer below", &[("width", 24.0, "px, how far the wrap reaches inward from the edge"), ("intensity", 1.0, "0..4 strength"), ("blur", 20.0, "px blur of the backdrop colour that wraps"), ("desaturate", 0.0, "0..1 pull the wrap toward grey"), ("linear", 0.0, "1 = wrap in linear light (sRGB decode, add, re-encode); 0 = as before")]),
    ("frosted_glass", "Frosted/refractive glass pane: the layer's alpha is the pane; inside it shows the composite below, blurred, tinted, saturated, refracted at a bevel with RGB dispersion and a rim light. Needs a layer below; runs after the layer's own position/scale/rotation, so the pane frosts what is actually under it (also on 3D-tilted layers, where the pane is the projected silhouette).", &[("blur", 24.0, "px"), ("refraction", 14.0, "bevel offset strength"), ("bevel", 10.0, "px"), ("dispersion", 0.25, "0..1.5"), ("tint_amount", 0.12, "0..1"), ("tint_r", 1.0, "R"), ("tint_g", 1.0, "G"), ("tint_b", 1.0, "B"), ("saturation", 1.15, "0..3"), ("edge_light", 0.5, "0..2"), ("layer_mix", 0.0, "0..1 keep layer own colour"), ("magnify", 1.0, "0.25..8, 1 = off; >1 magnifies what is under the pane about its centre (set blur 0, tint_amount 0, saturation 1 for a clear lens)"), ("magnify_cx", -1.0, "lens centre as a fraction of canvas width, -1 = pane centroid"), ("magnify_cy", -1.0, "lens centre as a fraction of canvas height, -1 = pane centroid"), ("fresnel", 0.0, "0..1, 0 = off: Schlick reflection of a procedural environment (sky gradient + a softbox) by view angle; works flat and tilted, stronger at grazing angles"), ("f0", 0.04, "reflectance at normal incidence (glass 0.04)"), ("env_gain", 1.0, "sky brightness for fresnel"), ("box_gain", 3.0, "softbox brightness for fresnel"), ("box_x", -0.4, "softbox direction x (screen right +)"), ("box_y", -0.6, "softbox direction y (screen down +, so negative = up)"), ("box_size", 0.02, "softbox size (angular falloff, 0.001..1)"), ("reflect_scene", 0.0, "0..1, 0 = off: with fresnel > 0 the reflection also shows the backdrop, looked up along the reflected ray in screen space"), ("reflect_dist", 300.0, "px the reflected lookup travels per unit of reflected direction"), ("env_blur", 6.0, "px roughness blur of the reflected backdrop"), ("slab_thickness", 0.0, "world px, 0 = off; on a 3D-tilted pane, per-pixel Snell refraction through a glass slab of this thickness (zero at normal incidence, grows with the view angle)"), ("ior", 1.5, "index of refraction for slab_thickness, 1..3; dispersion splits it per channel"), ("tilt_refraction", 0.0, "px per unit of 3D surface normal; a tilted pane shifts the backdrop along its normal, 0 = off")]),
    ("fracture_glass", "Procedural laminated-glass shatter on the clip's live canvas: irregular radial/ring Voronoi cracks from one impact (t0, impact_x/y) with branching and fine debris, cracks grow outward, facets refract with RGB dispersion, cells release in a radial stagger and fall with 3D tumble, slab thickness and motion blur carrying the live source texture; holes dimmed and transparent so lower layers show. Deterministic per seed. Single impact.", &[("t0", 0.0, "impact time, clip seconds"), ("impact_x", 0.5, "0..1"), ("impact_y", 0.5, "0..1"), ("radials", 14.0, "base radial crack count"), ("ring_growth", 1.3, "radius ratio per ring (smaller = more, finer cells)"), ("r0", 0.012, "first ring radius, frame heights"), ("debris", 70.0, "extra small fragments near impact"), ("max_cells", 700.0, "cell budget; ring_growth is coarsened automatically so rings always reach the frame edge (cost grows ~N^2)"), ("crack_speed", 12.0, "front speed, heights/s"), ("release_delay", 0.1, "s after impact for central cells"), ("release_spread", 0.12, "extra s to outermost releasing cells"), ("release_radius", 0.6, "frame heights; cells beyond stay in the pane"), ("gravity", 2.5, "heights/s^2"), ("push", 0.35, "outward speed, heights/s"), ("tumble", 6.0, "3D tumble rate, rad/s"), ("toward_camera", 0.1, "z speed, heights/s"), ("thickness", 5.0, "slab thickness px"), ("blur_samples", 5.0, "motion blur sub-samples"), ("shutter_deg", 180.0, "motion blur shutter"), ("fps", 25.0, "frame rate for the shutter"), ("fade_start", 0.4, "s after impact (absolute) debris starts fading"), ("clear_time", 0.6, "s after impact (absolute) debris fully gone"), ("hole_dim", 0.2, "0..1 black overlay in holes (lower layer shows through)"), ("edge_r", 0.62, "slab edge tint R"), ("edge_g", 0.68, "slab edge tint G"), ("edge_b", 0.66, "slab edge tint B"), ("vel_noise", 0.6, "per-shard velocity decorrelation (direction, speed, swirl, release lead/lag)"), ("release_jitter", 0.08, "s of random release lead/lag"), ("face_refraction", 10.0, "px shard-face lens/tilt distortion of the sampled texture"), ("transmission", 0.0, "0..1 how much of the real composite below (refracted, dispersed) shows through the shard faces; needs a layer underneath"), ("face_opacity", 0.85, "shard face opacity (lower = more see-through to the layer below)"), ("fresnel", 0.6, "grazing-angle reflection + edge sheen"), ("sheen", 0.5, "moving specular streak across faces"), ("crack_clear", 0.0, "absolute s after impact when the remaining pane cracks are gone (0 = keep)"), ("refraction", 7.0, "px facet displacement"), ("dispersion", 0.35, "0..1.5 RGB split"), ("crack_width", 1.1, "px"), ("glint", 0.7, "0..3"), ("bevel", 3.0, "px edge highlight"), ("scene_lights", 0.0, "1 = shade faces with the shared project lights (set_light) instead of the built-in fake sheen: GGX specular on each shard's real 3D normal, per-shard roughness"), ("light_gain", 1.0, "0..16 scale of the scene-light specular; needs scene_lights=1 and a light in the scene (set_light), otherwise no effect"), ("hole_fill", 0.0, "0..1, default off: fill the hole where glass left"), ("hole_source", 0.0, "0 defocused copy of the pane, 1 layers below; only used when hole_fill > 0"), ("hole_blur", 40.0, "px; only used when hole_fill > 0"), ("hole_dim_color", 0.45, "0..1 brightness of the defocused fill; only used when hole_fill > 0")]),
    ("vr_rotate_sphere", "VR Rotate Sphere: re-orient a full 360 equirectangular frame by yaw/pitch/roll, as if the camera rig itself turned (vr.rs, FEATURES.md L477)", &[("yaw", 0.0, "degrees, left/right"), ("pitch", 0.0, "degrees, up/down"), ("roll", 0.0, "degrees, tilt")]),
    ("vr_convert_projection", "VR Convert Projection: equirectangular <-> fisheye (vr.rs, FEATURES.md L477)", &[("mode", 0.0, "0 equirect source -> fisheye output, 1 fisheye source -> equirect output"), ("fov", 180.0, "degrees, fisheye field of view (180 = full hemisphere)")]),
    ("vr_plane_to_sphere", "VR Plane to Sphere: place this clip's own flat footage onto its patch of a full equirectangular sphere at a given yaw/pitch/fov; everywhere else is transparent (vr.rs, FEATURES.md L477)", &[("yaw", 0.0, "degrees, centre of the placed patch"), ("pitch", 0.0, "degrees, centre of the placed patch"), ("fov_h", 90.0, "degrees, horizontal field of view of the flat footage")]),
    ("vr_blur", "VR Blur: box blur that wraps around the 360 seam instead of clamping (vr.rs, FEATURES.md L477)", &[("radius", 8.0, "px")]),
    ("vr_sharpen", "VR Sharpen: unsharp mask using a seam-wrapping blur as the low-pass (vr.rs, FEATURES.md L477)", &[("amount", 0.8, "0..3"), ("radius", 2.0, "px")]),
    ("vr_denoise", "VR Denoise: seam-wrapping spatial smoothing mixed in by amount; simpler than the main `denoise` (no luma/chroma split, no temporal) -- built for the wrap property (vr.rs, FEATURES.md L477)", &[("amount", 0.5, "0..1"), ("radius", 2.0, "px")]),
    ("vr_glow", "VR Glow: threshold + seam-wrapping blur + add, same recipe as `glow` but the bloom itself wraps the 360 seam (vr.rs, FEATURES.md L477)", &[("amount", 0.5, "0..2"), ("radius", 12.0, "px"), ("threshold", 0.7, "0..1")]),
    ("vr_chromatic_aberration", "VR Chromatic Aberrations: radial R/B channel split, sampled with horizontal wrap so the fringe runs across the seam instead of smearing into the last column (vr.rs, FEATURES.md L477)", &[("amount", 4.0, "px"), ("cx", 0.5, "center U"), ("cy", 0.5, "center V"), ("radial_scale", 2.0, "radial multiplier"), ("r_scale", 1.0, "red scale"), ("b_scale", -1.0, "blue scale")]),

    ("cartoon", "Cartoon/toon shading (FEATURES.md L485): posterize colour into flat bands and darken wherever the local luma gradient crosses a threshold (a real edge line, not a blur)", &[("levels", 5.0, "colour bands, 2..16"), ("edge_strength", 1.0, "0..2, how dark the edge lines are"), ("edge_threshold", 0.15, "0..1 gradient magnitude that counts as an edge")]),
    ("color_emboss", "Colour-preserving emboss (FEATURES.md L485): the classic emboss relief (original minus a directionally-offset sample) added back onto the original colour, so colour survives (plain `emboss` elsewhere goes to grey)", &[("angle", 135.0, "deg"), ("strength", 1.0, "0..4"), ("depth", 1.0, "offset px, >0")]),
    ("roughen_edges", "Roughen Edges (FEATURES.md L485): a jagged noise-driven warp confined to the alpha edge band (an interior solid fill barely moves; only the boundary gets ragged)", &[("amount", 6.0, "px, 0..200"), ("size", 20.0, "noise feature size px, >=1")]),
    ("scatter", "Scatter (FEATURES.md L485): every pixel resampled from a random nearby position, independent per pixel (not a blur -- edges go ragged, flat fields stay flat on average)", &[("amount", 6.0, "px, 0..200")]),
    ("texturize", "Texturize (FEATURES.md L485), procedural variant: multiplies the image by a synthetic bump pattern. Honest scope note: AE's own Texturize can also pull its bump from ANOTHER layer -- that two-clip-reference mode was not built this pass (see FEATURES.md).", &[("scale", 0.1, "noise frequency, >0"), ("amount", 0.5, "0..2"), ("octaves", 3.0, "1..6")]),
    ("cc_glass", "CC Glass (FEATURES.md L485): refracts the image through a procedural bump field (the bump's own gradient bends the sample position), a real lens-like distortion, not a blur", &[("amount", 20.0, "px, 0..400"), ("scale", 0.02, "bump noise frequency, >0")]),
    ("cc_kaleida", "CC Kaleida (FEATURES.md L485): mirrors the image into N radial wedges around a centre point for a true kaleidoscope repeat", &[("segments", 6.0, "2..64"), ("center_x", 0.5, "0..1"), ("center_y", 0.5, "0..1"), ("angle_offset", 0.0, "deg"), ("radius_scale", 1.0, "zoom, >0")]),
    ("cc_threshold_rgb", "CC Threshold RGB (FEATURES.md L485): each colour channel independently hard-clipped to 0 or 1 at its own threshold (not a single combined luma threshold)", &[("r_threshold", 0.5, "0..1"), ("g_threshold", 0.5, "0..1"), ("b_threshold", 0.5, "0..1")]),
    ("dither", "Ordered (Bayer 4x4) dithering down to a small number of levels per channel (FEATURES.md L485): a real spatial dither pattern, not a plain posterize", &[("levels", 4.0, "2..16"), ("strength", 1.0, "0..1 dither pattern amount")]),
    ("tile_mosaic", "Mosaic/pixelate (FEATURES.md L485): the image rebuilt from flat NxN colour-average blocks", &[("size", 16.0, "block size px, 1..256")]),
    ("sketch_pencil", "Pencil sketch (FEATURES.md L485, the raster look half of 'sketch/pencil' -- see trace/vector note on the honest-gaps list): the classic colour-dodge-of-an-inverted-blur technique over the image's own luma", &[("blur_radius", 6.0, "px, 1..64"), ("strength", 1.0, "0..1 blend back to the original"), ("color", 0.0, "0 pure grey sketch .. 1 tinted with the original colour")]),
    ("oil_paint", "Oil paint (FEATURES.md L485): a real windowed most-common-luma-bucket filter (not a blur) -- each pixel becomes the average colour of whichever luma bucket is most common in its neighbourhood", &[("radius", 3.0, "neighbourhood radius px, 1..8"), ("levels", 20.0, "luma buckets, 2..64")]),
    ("anaglyph", "Anaglyph red/cyan stereo (FEATURES.md L485): red channel sampled from one horizontal shift, green+blue from the opposite shift, approximating stereo separation from a single 2D image (no real depth map -- an honest approximation, not true stereo)", &[("shift", 4.0, "px, 0..100")]),
    ("cc_burn_film", "CC Burn Film (FEATURES.md L485): an animated noise-edged film-burn eating the layer away as `progress` advances, with an ember glow band right at the burn front", &[("progress", 0.0, "0..1, 0 untouched, 1 fully burned away"), ("edge_width", 0.1, "0..1, softness/jaggedness of the burn front"), ("burn_r", 1.0, "ember colour R"), ("burn_g", 0.45, "ember colour G"), ("burn_b", 0.05, "ember colour B")]),
];

// ---- helpers ----
fn hash(x: i32, y: i32, s: u32) -> f32 { let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ s.wrapping_mul(2246822519); h = (h ^ (h >> 13)).wrapping_mul(1274126177); ((h ^ (h >> 16)) & 0xffffff) as f32 / 16777215.0 }
fn vnoise(x: f32, y: f32, s: u32) -> f32 { let (xi, yi) = (x.floor(), y.floor()); let (fx, fy) = (x - xi, y - yi); let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy)); let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi, s); let b = hash(xi + 1, yi, s); let c = hash(xi, yi + 1, s); let d = hash(xi + 1, yi + 1, s); (a + (b - a) * u) * (1.0 - v) + (c + (d - c) * u) * v }
fn fbm(x: f32, y: f32, oct: usize, s: u32) -> f32 { let (mut a, mut f, mut sum, mut norm) = (1.0, 1.0, 0.0, 0.0); for o in 0..oct { sum += a * vnoise(x * f, y * f, s + o as u32 * 17); norm += a; a *= 0.5; f *= 2.0; } sum / norm }
pub(crate) fn sample(src: &[f32], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, w as f32 - 1.001); let y = y.clamp(0.0, h as f32 - 1.001); let (x0, y0) = (x as usize, y as usize); let (fx, fy) = (x - x0 as f32, y - y0 as f32); let mut o = [0f32; 4];
    for j in 0..4 { let a = src[(y0 * w + x0) * 4 + j]; let b = src[(y0 * w + x0 + 1) * 4 + j]; let c = src[((y0 + 1) * w + x0) * 4 + j]; let d = src[((y0 + 1) * w + x0 + 1) * 4 + j]; o[j] = (a + (b - a) * fx) * (1.0 - fy) + (c + (d - c) * fx) * fy; } o }
fn warp(f: &mut Frame, m: impl Fn(f32, f32) -> (f32, f32)) { let src = f.d.clone(); let (w, h) = (f.w, f.h); for y in 0..h { for x in 0..w { let (sx, sy) = m(x as f32, y as f32); let o = sample(&src, w, h, sx, sy); f.d[(y * w + x) * 4..][..4].copy_from_slice(&o); } } }
fn blend(f: &mut Frame, i: usize, c: [f32; 3], op: f32, mode: u32) { for j in 0..3 { let b = f.d[i * 4 + j]; let r = match mode { 0 => c[j], 1 => if b < 0.5 { 2.0 * b * c[j] } else { 1.0 - 2.0 * (1.0 - b) * (1.0 - c[j]) }, 2 => b * c[j], _ => b + c[j] }; f.d[i * 4 + j] = b + (r - b) * op; } }
fn mat3(c: [f32; 3], m: [f32; 9]) -> [f32; 3] { [c[0] * m[0] + c[1] * m[1] + c[2] * m[2], c[0] * m[3] + c[1] * m[4] + c[2] * m[5], c[0] * m[6] + c[1] * m[7] + c[2] * m[8]] }
fn disc(f: &mut Frame, cx: f32, cy: f32, r: f32, col: [f32; 3], k: f32, ring: bool) { let (w, h) = (f.w as i32, f.h as i32); let rr = (r * 1.8) as i32 + 2; for y in (cy as i32 - rr).max(0)..(cy as i32 + rr).min(h) { for x in (cx as i32 - rr).max(0)..(cx as i32 + rr).min(w) {
    let d = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt()) / r; let v = if ring { (-((d - 1.0) * 6.0).powi(2)).exp() } else { (-d * d * 2.0).exp() } * k; let i = (y as usize * f.w + x as usize) * 4; for j in 0..3 { f.d[i + j] += col[j] * v; } } } }
fn glow_line(o: &mut [f32], w: usize, h: usize, a: (f32, f32), b: (f32, f32), core: f32, glow: f32, k: f32) {
    let (wi, hi) = (w as i32, h as i32); let pad = (glow * 3.0 + core * 2.0) as i32 + 1; let (x0, x1) = (a.0.min(b.0) as i32 - pad, a.0.max(b.0) as i32 + pad); let (y0, y1) = (a.1.min(b.1) as i32 - pad, a.1.max(b.1) as i32 + pad);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1); let l2 = (dx * dx + dy * dy).max(1e-3);
    for y in y0.max(0)..y1.min(hi) { for x in x0.max(0)..x1.min(wi) { let t = (((x as f32 - a.0) * dx + (y as f32 - a.1) * dy) / l2).clamp(0.0, 1.0); let d = ((x as f32 - a.0 - t * dx).powi(2) + (y as f32 - a.1 - t * dy).powi(2)).sqrt();
        let v = ((-(d / core).powi(2)).exp() + (-(d / glow)).exp() * 0.3) * k; let i = y as usize * w + x as usize; if v > o[i] { o[i] = v; } } } }
fn bolt(a: (f32, f32), b: (f32, f32), jit: f32, seed: u32, depth: u32, out: &mut Vec<(f32, f32)>) { if depth == 0 || jit < 0.5 { out.push(b); return; }
    let m = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5); let (dx, dy) = (b.0 - a.0, b.1 - a.1); let l = (dx * dx + dy * dy).sqrt().max(1e-3); let (nx, ny) = (-dy / l, dx / l); let o = (hash(seed as i32, depth as i32, seed ^ 77) - 0.5) * 2.0 * jit * (l / (l + 40.0)).max(0.3);
    let mid = (m.0 + nx * o, m.1 + ny * o); bolt(a, mid, jit * 0.55, seed.wrapping_mul(31).wrapping_add(1), depth - 1, out); bolt(mid, b, jit * 0.55, seed.wrapping_mul(31).wrapping_add(7), depth - 1, out); }

/// Effects that read the composite below their layer (see render::backdrop()). The renderer only builds that copy for clips that carry one of these.
/// Effects that run after the layer transform and read the composite below (frosted_glass, light_wrap).
pub fn is_post(kind: &str) -> bool { matches!(kind, "frosted_glass" | "light_wrap" | "cast_shadow") }
pub fn needs_backdrop(kind: &str) -> bool { matches!(kind, "fracture_glass") }
pub fn apply2(kind: &str, pr: &Params, f: &mut Frame, t: f64, seed: u64) -> Result<(), String> {
    let g = |k: &str, d: f64| p(pr, k, d, t);
    let (w, h) = (f.w, f.h); let (wf, hf) = (w as f32, h as f32); let md = wf.min(hf);
    match kind {
        "twirl" => { let (ang, r, cx, cy) = (g("angle", 120.0).to_radians(), g("radius", 0.5) * md, g("cx", 0.5) * wf, g("cy", 0.5) * hf); warp(f, |x, y| { let (dx, dy) = (x - cx, y - cy); let d = (dx * dx + dy * dy).sqrt(); if d >= r { return (x, y); } let th = ang * (1.0 - d / r).powi(2); let (s, c) = th.sin_cos(); (cx + dx * c - dy * s, cy + dx * s + dy * c) }); }
        "bulge" => { let (a, r, cx, cy) = (g("amount", 0.6), g("radius", 0.5) * md, g("cx", 0.5) * wf, g("cy", 0.5) * hf); warp(f, |x, y| { let (dx, dy) = (x - cx, y - cy); let d = (dx * dx + dy * dy).sqrt(); if d >= r || d < 1e-3 { return (x, y); } let n = d / r; let k = 1.0 - a * (1.0 - n * n); (cx + dx * k, cy + dy * k) }); }
        "polar" => { let inv = g("inverse", 0.0) > 0.5; let (cx, cy) = (wf * 0.5, hf * 0.5); let mr = (cx * cx + cy * cy).sqrt(); warp(f, |x, y| if !inv { let (dx, dy) = (x - cx, y - cy); let a = dy.atan2(dx); ((a / 6.2831853 + 0.5) * wf, (dx * dx + dy * dy).sqrt() / mr * hf) } else { let a = (x / wf - 0.5) * 6.2831853; let r = y / hf * mr; (cx + r * a.cos(), cy + r * a.sin()) }); }
        "kaleidoscope" => { let (n, ang, cx, cy) = (g("segments", 6.0).max(1.0), g("angle", 0.0).to_radians(), g("cx", 0.5) * wf, g("cy", 0.5) * hf); let seg = 6.2831853 / n; warp(f, |x, y| { let (dx, dy) = (x - cx, y - cy); let r = (dx * dx + dy * dy).sqrt(); let th = (dy.atan2(dx) - ang).rem_euclid(seg); let th = (th - seg * 0.5).abs() + ang; (cx + r * th.cos(), cy + r * th.sin()) }); }
        "turbulent_displace" => { let (a, s, sp) = (g("amount", 20.0), g("size", 80.0).max(4.0), g("speed", 0.5)); let tt = t as f32 * sp * 3.0; let sd = seed as u32; warp(f, |x, y| (x + (fbm(x / s + tt, y / s, 3, sd) - 0.5) * 2.0 * a, y + (fbm(x / s, y / s + tt, 3, sd + 99) - 0.5) * 2.0 * a)); }
        "offset" => { let (dx, dy) = (g("dx", 0.0), g("dy", 0.0)); warp(f, |x, y| ((x - dx).rem_euclid(wf - 1.0), (y - dy).rem_euclid(hf - 1.0))); }
        "motion_tile" => { let n = g("tiles", 2.0).max(1.0); let mir = g("mirror", 0.0) > 0.5; warp(f, |x, y| { let tx = x * n / wf; let ty = y * n / hf; let (fx, fy) = (tx.fract(), ty.fract()); let (fx, fy) = if mir { (if (tx as i32) % 2 == 1 { 1.0 - fx } else { fx }, if (ty as i32) % 2 == 1 { 1.0 - fy } else { fy }) } else { (fx, fy) }; (fx * wf, fy * hf) }); }
        "spin_blur" => { let (a, cx, cy) = (g("angle", 10.0).to_radians(), g("cx", 0.5) * wf, g("cy", 0.5) * hf); let src = f.d.clone(); let n = 12; for y in 0..h { for x in 0..w { let (dx, dy) = (x as f32 - cx, y as f32 - cy); let mut acc = [0f32; 4]; for k in 0..n { let th = a * (k as f32 / (n - 1) as f32 - 0.5); let (s, c) = th.sin_cos(); let v = sample(&src, w, h, cx + dx * c - dy * s, cy + dx * s + dy * c); for j in 0..4 { acc[j] += v[j]; } } for j in 0..4 { f.d[(y * w + x) * 4 + j] = acc[j] / n as f32; } } } }
        "directional_blur" => { let (l, a) = (g("length", 24.0), g("angle", 0.0).to_radians()); let src = f.d.clone(); let n = (l as usize).clamp(2, 48); let (ux, uy) = (a.cos(), a.sin()); for y in 0..h { for x in 0..w { let mut acc = [0f32; 4]; for k in 0..n { let o = (k as f32 / (n - 1) as f32 - 0.5) * l; let v = sample(&src, w, h, x as f32 + ux * o, y as f32 + uy * o); for j in 0..4 { acc[j] += v[j]; } } for j in 0..4 { f.d[(y * w + x) * 4 + j] = acc[j] / n as f32; } } } }
        "flip" => { let (hz, vt) = (g("horizontal", 1.0) > 0.5, g("vertical", 0.0) > 0.5); warp(f, |x, y| (if hz { wf - 1.0 - x } else { x }, if vt { hf - 1.0 - y } else { y })); }
        "tilt_shift" => { let (c, bnd, r) = (g("center", 0.5), g("band", 0.15), g("radius", 10.0)); let mut bl = Frame { w, h, d: f.d.clone() }; box_blur3(&mut bl, (r * 2.0) as usize); for y in 0..h { let d = ((y as f32 / hf - c).abs() - bnd).max(0.0) / 0.25; let k = d.clamp(0.0, 1.0); for x in 0..w { for j in 0..4 { let i = (y * w + x) * 4 + j; f.d[i] += (bl.d[i] - f.d[i]) * k; } } } }
        "channel_blur" => { let (rr, rg, rb) = (g("r_radius", 4.0) as usize, g("g_radius", 4.0) as usize, g("b_radius", 4.0) as usize);
            let alpha: Vec<f32> = (0..w * h).map(|i| f.d[i * 4 + 3]).collect();
            for (ch, r) in [(0usize, rr), (1, rg), (2, rb)] { let mut tmp = Frame::new(w, h);
                for i in 0..w * h { let v = f.d[i * 4 + ch]; tmp.d[i * 4] = v; tmp.d[i * 4 + 1] = v; tmp.d[i * 4 + 2] = v; tmp.d[i * 4 + 3] = v; }
                box_blur3(&mut tmp, r); for i in 0..w * h { f.d[i * 4 + ch] = tmp.d[i * 4]; } }
            for i in 0..w * h { f.d[i * 4 + 3] = alpha[i]; } }
        "smart_blur" => { let r = g("radius", 4.0) as usize; let thr = g("threshold", 0.1).clamp(0.0, 1.0) as f32;
            let orig = f.d.clone(); box_blur3(f, r);
            for i in 0..w * h { let lo = 0.299 * orig[i * 4] + 0.587 * orig[i * 4 + 1] + 0.114 * orig[i * 4 + 2]; let lb = 0.299 * f.d[i * 4] + 0.587 * f.d[i * 4 + 1] + 0.114 * f.d[i * 4 + 2];
                if (lo - lb).abs() > thr { for j in 0..3 { f.d[i * 4 + j] = orig[i * 4 + j]; } } } }
        "reduce_interlace_flicker" => { let amt = g("amount", 0.5).clamp(0.0, 1.0) as f32; let src = f.d.clone();
            for y in 0..h { let y0 = y.saturating_sub(1); let y1 = (y + 1).min(h - 1);
                for x in 0..w { let i = (y * w + x) * 4;
                    for j in 0..4 { let v = src[(y0 * w + x) * 4 + j] * 0.25 + src[i + j] * 0.5 + src[(y1 * w + x) * 4 + j] * 0.25; f.d[i + j] = src[i + j] * (1.0 - amt) + v * amt; } } } }
        "fractal_noise" => { let (sc, oct, sp, ct, br, op, mode) = (g("scale", 160.0).max(4.0), g("complexity", 4.0).clamp(1.0, 8.0) as usize, g("speed", 0.3), g("contrast", 1.4), g("brightness", 0.0), g("opacity", 1.0), g("mode", 1.0) as u32); let tt = t as f32 * sp * 2.0; let sd = seed as u32;
            for y in 0..h { for x in 0..w { let n = ((fbm(x as f32 / sc + tt * 0.3, y as f32 / sc - tt, oct, sd) - 0.5) * ct + 0.5 + br).clamp(0.0, 1.0); let i = y * w + x; match mode { 4 => { f.d[i * 4 + 3] *= 1.0 + (n - 1.0) * op; } m => blend(f, i, [n, n, n], op, m) } } } }
        "gradient_ramp" => { let (c0, c1) = ([g("r0", 0.0), g("g0", 0.0), g("b0", 0.0)], [g("r1", 1.0), g("g1", 0.5), g("b1", 0.0)]); let (x0, y0, x1, y1) = (g("x0", 0.5) * wf, g("y0", 0.0) * hf, g("x1", 0.5) * wf, g("y1", 1.0) * hf); let rad = g("radial", 0.0) > 0.5; let (op, mode) = (g("opacity", 1.0), g("mode", 0.0) as u32);
            let (dx, dy) = (x1 - x0, y1 - y0); let l2 = (dx * dx + dy * dy).max(1e-3); for y in 0..h { for x in 0..w { let t_ = if rad { (((x as f32 - x0).powi(2) + (y as f32 - y0).powi(2)) / l2).sqrt() } else { ((x as f32 - x0) * dx + (y as f32 - y0) * dy) / l2 }.clamp(0.0, 1.0); blend(f, y * w + x, [c0[0] + (c1[0] - c0[0]) * t_, c0[1] + (c1[1] - c0[1]) * t_, c0[2] + (c1[2] - c0[2]) * t_], op, mode); } } }
        "checkerboard" => { let (s, c, op) = (g("size", 40.0).max(2.0), [g("r", 1.0), g("g", 1.0), g("b", 1.0)], g("opacity", 0.5)); for y in 0..h { for x in 0..w { if ((x as f32 / s) as i32 + (y as f32 / s) as i32) % 2 == 0 { blend(f, y * w + x, c, op, 0); } } } }
        "grid" => { let (s, lw, c, op) = (g("size", 60.0).max(4.0), g("width", 2.0), [g("r", 1.0), g("g", 1.0), g("b", 1.0)], g("opacity", 0.5)); for y in 0..h { for x in 0..w { if (x as f32 % s) < lw || (y as f32 % s) < lw { blend(f, y * w + x, c, op, 0); } } } }
        "lightning" => { let rate = g("rate", 18.0); let step = (t as f32 * rate).floor() as u32; let sd = (seed as u32).wrapping_add(step.wrapping_mul(2654435761)); let (a, b) = ((g("x0", 0.1) * wf, g("y0", 0.5) * hf), (g("x1", 0.9) * wf, g("y1", 0.5) * hf));
            let strike = g("strike", 0.0) > 0.5; let duty = g("strike_duty", 0.12).clamp(0.001, 1.0); let phase = (t as f32 * rate).fract();
            if !strike || phase < duty {
            let bd = g("branch_decay", 0.5).clamp(0.0, 1.0); let bd_k = (bd * 1.4).min(3.0);
            let (jit, core, gl, col, k) = (g("jitter", 40.0), g("width", 2.5), g("glow", 14.0).max(1.0), [g("r", 0.55), g("g", 0.75), g("b", 1.0)], g("intensity", 1.0)); let mut pts = vec![a]; bolt(a, b, jit, sd, 6, &mut pts); let mut ov = vec![0f32; w * h];
            for s in pts.windows(2) { glow_line(&mut ov, w, h, s[0], s[1], core, gl, k); }
            let nb = g("branches", 3.0) as u32; for i in 0..nb { if pts.len() < 4 { break; } let idx = 1 + (hash(i as i32, 3, sd) * (pts.len() - 2) as f32) as usize; let o = pts[idx]; let ang = (hash(i as i32, 9, sd) - 0.5) * 2.2; let (dx, dy) = (b.0 - a.0, b.1 - a.1); let l = (dx * dx + dy * dy).sqrt() * (0.12 + 0.2 * hash(i as i32, 5, sd)); let base = dy.atan2(dx) + ang; let e = (o.0 + l * base.cos(), o.1 + l * base.sin()); let mut bp = vec![o]; bolt(o, e, jit * 0.6, sd ^ (i + 1), 4, &mut bp); for s in bp.windows(2) { glow_line(&mut ov, w, h, s[0], s[1], core * bd, gl * bd, k * bd_k); } }
            for i in 0..w * h { let v = ov[i]; if v > 0.0 { let wh = (v - 0.6).max(0.0).min(1.0); for j in 0..3 { f.d[i * 4 + j] += col[j] * v.min(1.0) * 1.1 + wh * (1.0 - col[j]); } } }
            disc(f, a.0, a.1, gl * 2.0, col, k * 0.8, false); disc(f, b.0, b.1, gl * 2.5, col, k, false);
            } }
        "lens_flare" if g("mode", 0.0) >= 0.5 => crate::flare::apply_v2(pr, f, t),
        "lens_flare" => { let (x, y, br, st) = (g("x", 0.75) * wf, g("y", 0.25) * hf, g("brightness", 1.0), g("streak", 1.0)); let (cx, cy) = (wf * 0.5, hf * 0.5); disc(f, x, y, md * 0.12, [1.0, 0.9, 0.7], br, false); disc(f, x, y, md * 0.04, [1.0, 1.0, 1.0], br * 1.2, false); disc(f, x, y, md * 0.25, [1.0, 0.8, 0.5], br * 0.25, true);
            let cols = [[1.0, 0.4, 0.2], [0.3, 1.0, 0.5], [0.3, 0.5, 1.0], [1.0, 0.8, 0.3], [0.8, 0.3, 1.0]]; for (i, c) in cols.iter().enumerate() { let tt = 0.3 + i as f32 * 0.45; let (gx, gy) = (x + (cx - x) * tt * 1.6, y + (cy - y) * tt * 1.6); disc(f, gx, gy, md * (0.03 + 0.025 * (i % 3) as f32), *c, br * 0.35, i % 2 == 0); }
            if st > 0.0 { for xx in 0..w { let d = (xx as f32 - x).abs() / (wf * 0.45); let v = (-d * d * 3.0).exp() * st * br * 0.5; for dy in -1i32..=1 { let yy = y as i32 + dy; if yy >= 0 && (yy as usize) < h { let i = (yy as usize * w + xx) * 4; let kk = if dy == 0 { v } else { v * 0.35 }; f.d[i] += 0.5 * kk; f.d[i + 1] += 0.7 * kk; f.d[i + 2] += kk; } } } } }
        "anamorphic_streak" => { let (th, l, a, c) = (g("threshold", 0.75), g("length", 120.0), g("amount", 0.8), [g("r", 0.5), g("g", 0.7), g("b", 1.0)]); let mut m = Frame { w, h, d: f.d.clone() }; for i in 0..w * h { let v = ((lum([m.d[i * 4], m.d[i * 4 + 1], m.d[i * 4 + 2]]) - th) / (1.0 - th).max(0.01)).clamp(0.0, 1.0); for j in 0..3 { m.d[i * 4 + j] = v * c[j]; } }
            let r = (l / 2.0) as usize; let src = m.d.clone(); for y in 0..h { let mut acc = [0f32; 3]; let mut cnt = 0f32; for x in 0..r.min(w) { for j in 0..3 { acc[j] += src[(y * w + x) * 4 + j]; } cnt += 1.0; } for x in 0..w { if x + r < w { for j in 0..3 { acc[j] += src[(y * w + x + r) * 4 + j]; } cnt += 1.0; } if x > r { for j in 0..3 { acc[j] -= src[(y * w + x - r - 1) * 4 + j]; } cnt -= 1.0; } for j in 0..3 { f.d[(y * w + x) * 4 + j] += acc[j] / cnt.max(1.0) * a * 2.0; } } } }
        "strobe" => { let (rate, duty, blk, a) = (g("rate", 8.0), g("duty", 0.2), g("black", 0.0) > 0.5, g("amount", 1.0)); if (t as f32 * rate).fract() < duty { let v = if blk { 0.0 } else { 1.0 }; for i in 0..w * h { for j in 0..3 { f.d[i * 4 + j] += (v - f.d[i * 4 + j]) * a; } } } }
        "gradient_map" => { let (s, m, hh, a) = ([g("sr", 0.02), g("sg", 0.05), g("sb", 0.2)], [g("mr", 0.8), g("mg", 0.2), g("mb", 0.4)], [g("hr", 1.0), g("hg", 0.9), g("hb", 0.6)], g("amount", 1.0)); for i in 0..w * h { let l = lum([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]]).clamp(0.0, 1.0); for j in 0..3 { let v = if l < 0.5 { s[j] + (m[j] - s[j]) * l * 2.0 } else { m[j] + (hh[j] - m[j]) * (l - 0.5) * 2.0 }; f.d[i * 4 + j] += (v - f.d[i * 4 + j]) * a; } } }
        "levels" => { let (ib, iw, gm, ob, ow) = (g("in_black", 0.0), g("in_white", 1.0), g("gamma", 1.0).max(0.05), g("out_black", 0.0), g("out_white", 1.0)); for i in 0..w * h { for j in 0..3 { let v = ((f.d[i * 4 + j] - ib) / (iw - ib).max(1e-3)).clamp(0.0, 1.0).powf(1.0 / gm); f.d[i * 4 + j] = ob + (ow - ob) * v; } } }
        "brightness_contrast" => { let (b, c) = (g("brightness", 0.0), g("contrast", 0.0)); let k = if c >= 0.0 { 1.0 / (1.0 - c * 0.99) } else { 1.0 + c }; for i in 0..w * h { for j in 0..3 { f.d[i * 4 + j] = (f.d[i * 4 + j] - 0.5) * k + 0.5 + b; } } }
        "channel_mixer" => { let m = [g("rr", 1.0), g("rg", 0.0), g("rb", 0.0), g("gr", 0.0), g("gg", 1.0), g("gb", 0.0), g("br", 0.0), g("bg", 0.0), g("bb", 1.0)]; for i in 0..w * h { let o = mat3([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]], m); f.d[i * 4..i * 4 + 3].copy_from_slice(&o); } }
        "threshold" => { let l_ = g("level", 0.5); for i in 0..w * h { let v = if lum([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]]) > l_ { 1.0 } else { 0.0 }; for j in 0..3 { f.d[i * 4 + j] = v; } } }
        "colorize" => { let (c, a) = ([g("r", 1.0), g("g", 0.7), g("b", 0.4)], g("amount", 1.0)); for i in 0..w * h { let l = lum([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]]); for j in 0..3 { let v = if l < 0.5 { c[j] * l * 2.0 } else { c[j] + (1.0 - c[j]) * (l - 0.5) * 2.0 }; f.d[i * 4 + j] += (v - f.d[i * 4 + j]) * a; } } }
        "shadow_highlight" => { let (s, hh) = (g("shadows", 0.3), g("highlights", 0.3)); for i in 0..w * h { let l = lum([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]]).clamp(0.0, 1.0); let add = s * (1.0 - l).powi(2) * 0.5 - hh * l.powi(2) * 0.5; for j in 0..3 { f.d[i * 4 + j] += add; } } }
        "emboss" => { let (s, a) = (g("strength", 1.0), g("angle", 135.0).to_radians()); let src = f.d.clone(); let (ox, oy) = (a.cos().round() as i32, a.sin().round() as i32); for y in 1..h - 1 { for x in 1..w - 1 { let i1 = ((y as i32 + oy) as usize * w + (x as i32 + ox) as usize) * 4; let i2 = ((y as i32 - oy) as usize * w + (x as i32 - ox) as usize) * 4; let l1 = lum([src[i1], src[i1 + 1], src[i1 + 2]]); let l2 = lum([src[i2], src[i2 + 1], src[i2 + 2]]); let v = 0.5 + (l1 - l2) * s; for j in 0..3 { f.d[(y * w + x) * 4 + j] = v; } } } }
        "find_edges" => { let (inv, a) = (g("invert", 0.0) > 0.5, g("amount", 1.5)); let src = f.d.clone(); let l = |x: usize, y: usize| lum([src[(y * w + x) * 4], src[(y * w + x) * 4 + 1], src[(y * w + x) * 4 + 2]]); for y in 1..h - 1 { for x in 1..w - 1 { let gx = -l(x - 1, y - 1) - 2.0 * l(x - 1, y) - l(x - 1, y + 1) + l(x + 1, y - 1) + 2.0 * l(x + 1, y) + l(x + 1, y + 1); let gy = -l(x - 1, y - 1) - 2.0 * l(x, y - 1) - l(x + 1, y - 1) + l(x - 1, y + 1) + 2.0 * l(x, y + 1) + l(x + 1, y + 1); let mut v = ((gx * gx + gy * gy).sqrt() * a).clamp(0.0, 1.0); if inv { v = 1.0 - v; } for j in 0..3 { f.d[(y * w + x) * 4 + j] = v; } } } }
        "pixel_sort" => { let (th, mx) = (g("threshold", 0.35), g("max_run", 400.0) as usize); for y in 0..h { let mut x = 0; while x < w { let l0 = lum([f.d[(y * w + x) * 4], f.d[(y * w + x) * 4 + 1], f.d[(y * w + x) * 4 + 2]]); if l0 < th { x += 1; continue; } let mut e = x; while e < w && e - x < mx && lum([f.d[(y * w + e) * 4], f.d[(y * w + e) * 4 + 1], f.d[(y * w + e) * 4 + 2]]) >= th { e += 1; }
            let mut run: Vec<[f32; 4]> = (x..e).map(|k| { let i = (y * w + k) * 4; [f.d[i], f.d[i + 1], f.d[i + 2], f.d[i + 3]] }).collect(); run.sort_by(|a, b| lum([a[0], a[1], a[2]]).partial_cmp(&lum([b[0], b[1], b[2]])).unwrap()); for (k, px) in run.iter().enumerate() { f.d[(y * w + x + k) * 4..][..4].copy_from_slice(px); } x = e.max(x + 1); } } }
        "vhs" => { let (wb, bl, nz, bd) = (g("wobble", 3.0), g("bleed", 5.0), g("noise", 0.12), g("bands", 1.0)); let step = (t * 30.0).floor() as i32; let src = f.d.clone(); let sd = seed as u32;
            for y in 0..h { let band = { let by = ((t as f32 * 0.35) % 1.0) * hf; let d = (y as f32 - by).abs(); if d < hf * 0.04 { (1.0 - d / (hf * 0.04)) * bd * 18.0 } else { 0.0 } }; let off = (hash(y as i32 / 2, step, sd) - 0.5) * 2.0 * wb + band * (hash(y as i32, step, sd + 5) - 0.3);
                for x in 0..w { let sx = x as f32 - off; let r = sample(&src, w, h, sx + bl, y as f32); let gg = sample(&src, w, h, sx, y as f32); let b = sample(&src, w, h, sx - bl, y as f32); let n = (hash(x as i32, y as i32 + step * 7, sd + 9) - 0.5) * nz; let sl = if y % 2 == 0 { 0.94 } else { 1.0 }; let o = (y * w + x) * 4; let l = lum([gg[0], gg[1], gg[2]]); f.d[o] = (r[0] * 0.9 + l * 0.1 + n) * sl; f.d[o + 1] = (gg[1] * 0.9 + l * 0.1 + n) * sl; f.d[o + 2] = (b[2] * 0.9 + l * 0.1 + n) * sl; } } }
        "film_damage" => { let a = g("amount", 0.5); let step = (t * 24.0).floor() as i32; let sd = seed as u32; let fl = 1.0 + (hash(step, 1, sd) - 0.5) * 0.12 * a; for i in 0..w * h { for j in 0..3 { f.d[i * 4 + j] *= fl; } }
            let ns = (3.0 * a) as i32; for k in 0..ns { if hash(step, k, sd + 3) > 0.5 { let x = (hash(step, k + 10, sd) * wf) as usize; let len = (hash(step, k + 20, sd) * hf) as usize; let y0 = (hash(step, k + 30, sd) * (hf - len as f32)) as usize; for y in y0..(y0 + len).min(h) { let i = (y * w + x.min(w - 1)) * 4; for j in 0..3 { f.d[i + j] = f.d[i + j] * 0.3 + 0.7; } } } }
            let nd = (30.0 * a) as i32; for k in 0..nd { let x = (hash(step, k + 100, sd) * wf) as i32; let y = (hash(step, k + 200, sd) * hf) as i32; let r = 1 + (hash(step, k + 300, sd) * 2.5) as i32; for dy in -r..=r { for dx in -r..=r { if dx * dx + dy * dy <= r * r { let (xx, yy) = (x + dx, y + dy); if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h { let i = (yy as usize * w + xx as usize) * 4; for j in 0..3 { f.d[i + j] = f.d[i + j] * 0.2 + 0.05; } } } } } } }
        "choker" => { let r = g("amount", 2.0); let rr = (r.abs() as i32).max(0); if rr > 0 { let src: Vec<f32> = (0..w * h).map(|i| f.d[i * 4 + 3]).collect(); let shrink = r > 0.0; let mut tmp = src.clone(); for y in 0..h { for x in 0..w { let mut v = src[y * w + x]; for d in -rr..=rr { let xx = (x as i32 + d).clamp(0, w as i32 - 1) as usize; let s_ = src[y * w + xx]; v = if shrink { v.min(s_) } else { v.max(s_) }; } tmp[y * w + x] = v; } } for y in 0..h { for x in 0..w { let mut v = tmp[y * w + x]; for d in -rr..=rr { let yy = (y as i32 + d).clamp(0, h as i32 - 1) as usize; let s_ = tmp[yy * w + x]; v = if shrink { v.min(s_) } else { v.max(s_) }; } f.d[(y * w + x) * 4 + 3] = v; } } } }
        "feather_alpha" => { let r = g("radius", 8.0) as usize; let mut a = Frame { w, h, d: vec![0.0; w * h * 4] }; for i in 0..w * h { a.d[i * 4] = f.d[i * 4 + 3]; a.d[i * 4 + 3] = 1.0; } box_blur3(&mut a, r * 2); for i in 0..w * h { f.d[i * 4 + 3] = a.d[i * 4]; } }
        "spill_suppress" => { let (a, ch) = (g("amount", 1.0), g("channel", 1.0) as usize); for i in 0..w * h { let o = i * 4; let lim = if ch == 2 { (f.d[o] + f.d[o + 1]) * 0.5 } else { (f.d[o] + f.d[o + 2]) * 0.5 }; let c = if ch == 2 { 2 } else { 1 }; if f.d[o + c] > lim { f.d[o + c] += (lim - f.d[o + c]) * a; } } }
        "shift_channels" => { let takes = [g("take_r", 0.0) as i32, g("take_g", 1.0) as i32, g("take_b", 2.0) as i32, g("take_a", 3.0) as i32];
            for t in takes { if !(0..=9).contains(&t) { return Err(format!("shift_channels: take_* must be 0..9 (0-3 R/G/B/A, 4/5 constant 0/1, 6-9 inverted R/G/B/A), got {t}")); } }
            let pick = |src: &[f32; 4], k: i32| -> f32 { match k { 0 => src[0], 1 => src[1], 2 => src[2], 3 => src[3], 4 => 0.0, 5 => 1.0, 6 => 1.0 - src[0], 7 => 1.0 - src[1], 8 => 1.0 - src[2], _ => 1.0 - src[3] } };
            for i in 0..w * h { let o = i * 4; let src = [f.d[o], f.d[o + 1], f.d[o + 2], f.d[o + 3]]; for (j, &k) in takes.iter().enumerate() { f.d[o + j] = pick(&src, k); } } }
        "solid_composite" => { let (col, op, behind) = ([g("r", 0.0), g("g", 0.0), g("b", 0.0)], g("opacity", 1.0).clamp(0.0, 1.0), g("behind", 1.0) >= 0.5);
            for i in 0..w * h { let o = i * 4; let a = f.d[o + 3];
                if behind { let under = 1.0 - a; let k = under * op; for j in 0..3 { f.d[o + j] = f.d[o + j] * a + col[j] * k; } f.d[o + 3] = a + under * op; }
                else { for j in 0..3 { f.d[o + j] = f.d[o + j] * (1.0 - op) + col[j] * op; } f.d[o + 3] = f.d[o + 3].max(op); } } }
        "remove_color_matting" => { let bg = [g("r", 1.0), g("g", 1.0), g("b", 1.0)];
            for i in 0..w * h { let o = i * 4; let a = f.d[o + 3]; if a <= 0.0005 { continue; } for j in 0..3 { f.d[o + j] = ((f.d[o + j] - bg[j] * (1.0 - a)) / a).clamp(0.0, 1.0); } } }
        "median" | "dust_scratches" => { let r = if kind == "median" { g("radius", 1.0).round().clamp(1.0, 4.0) as i32 } else { g("radius", 2.0).round().clamp(1.0, 6.0) as i32 };
            let thr = g("threshold", 0.15).clamp(0.0, 1.0); let despeckle_only = kind == "dust_scratches";
            let src = f.d.clone(); let (wi, hi) = (w as i32, h as i32);
            for y in 0..h { crate::guard::check(); for x in 0..w { let (xi, yi) = (x as i32, y as i32); let o = (y * w + x) * 4;
                let mut chans: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
                for dy in -r..=r { for dx in -r..=r { let sx = (xi + dx).clamp(0, wi - 1) as usize; let sy = (yi + dy).clamp(0, hi - 1) as usize; let si = (sy * w + sx) * 4;
                    for j in 0..3 { chans[j].push(src[si + j]); } } }
                for j in 0..3 { chans[j].sort_by(|a, b| a.total_cmp(b)); let med = chans[j][chans[j].len() / 2];
                    if !despeckle_only || (src[o + j] - med).abs() > thr { f.d[o + j] = med; } } } } }
        "outline" => { let (wd, c) = (g("width", 3.0).max(1.0), [g("r", 1.0), g("g", 1.0), g("b", 1.0)]); let r = wd as i32; let src: Vec<f32> = (0..w * h).map(|i| f.d[i * 4 + 3]).collect(); for y in 0..h { for x in 0..w { let o = (y * w + x) * 4; if src[y * w + x] > 0.99 { continue; } let mut m = 0f32; for dy in -r..=r { for dx in -r..=r { if dx * dx + dy * dy <= r * r { let (xx, yy) = (x as i32 + dx, y as i32 + dy); if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h { m = m.max(src[yy as usize * w + xx as usize]); } } } } let k = m * (1.0 - src[y * w + x]); for j in 0..3 { f.d[o + j] = f.d[o + j] * (1.0 - k) + c[j] * k; } f.d[o + 3] = f.d[o + 3].max(k); } } }
        "outer_glow" => { let (r, a, c) = (g("radius", 16.0) as usize, g("amount", 1.2), [g("r", 0.3), g("g", 0.8), g("b", 1.0)]); let mut m = Frame { w, h, d: vec![0.0; w * h * 4] }; for i in 0..w * h { m.d[i * 4] = f.d[i * 4 + 3]; m.d[i * 4 + 3] = 1.0; } box_blur3(&mut m, r * 2); for i in 0..w * h { let k = (m.d[i * 4] * a).min(1.0) * (1.0 - f.d[i * 4 + 3]); for j in 0..3 { f.d[i * 4 + j] = f.d[i * 4 + j] * (1.0 - k) + c[j] * k; } f.d[i * 4 + 3] = f.d[i * 4 + 3].max(k); } }
        "long_shadow" => { let (l, a, op) = (g("length", 80.0), g("angle", 45.0).to_radians(), g("opacity", 0.5)); let src = f.d.clone(); let (ux, uy) = (a.cos(), a.sin()); let n = (l as usize).clamp(1, 200); for y in 0..h { for x in 0..w { let o = (y * w + x) * 4; if src[o + 3] > 0.99 { continue; } let mut m = 0f32; for k in (1..=n).step_by(2) { let (sx, sy) = (x as f32 - ux * k as f32, y as f32 - uy * k as f32); if sx >= 0.0 && sy >= 0.0 && sx < wf && sy < hf { m = m.max(src[(sy as usize * w + sx as usize) * 4 + 3]); } } let k = m * op * (1.0 - src[o + 3]); for j in 0..3 { f.d[o + j] *= 1.0 - k; } f.d[o + 3] = f.d[o + 3].max(k); } } }
        "fracture_glass" => return crate::fracture::apply(pr, f, t, seed),
        "cast_shadow" => return if crate::frosted::in_post() { crate::castshadow::apply(pr, f, t) } else { Ok(()) },
        "light_wrap" => return if crate::frosted::in_post() { crate::lightwrap::apply(pr, f, t) } else { Ok(()) },
        "frosted_glass" => return if crate::frosted::in_post() { crate::frosted::apply(pr, f, t) } else { Ok(()) },
        "lens_blur" => crate::lens::apply_params(pr, f, t),
        "displacement_map" | "set_matte" | "depth_parallax" | "channel_combiner" | "set_channels" | "match_grain" => {} // need another clip: applied in render.rs apply_effects_masked (layerfx.rs)
        "light_sweep" => light_sweep(pr, f, t),
        "saber" => saber(pr, f, t),
        "vr_rotate_sphere" => crate::vr::rotate_sphere(f, g("yaw", 0.0), g("pitch", 0.0), g("roll", 0.0)),
        "vr_convert_projection" => crate::vr::convert_projection(f, g("mode", 0.0), g("fov", 180.0)),
        "vr_plane_to_sphere" => crate::vr::plane_to_sphere(f, g("yaw", 0.0), g("pitch", 0.0), g("fov_h", 90.0)),
        "vr_blur" => crate::vr::vr_blur(f, pr, t),
        "vr_sharpen" => crate::vr::vr_sharpen(f, pr, t),
        "vr_denoise" => crate::vr::vr_denoise(f, pr, t),
        "vr_glow" => crate::vr::vr_glow(f, pr, t),
        "vr_chromatic_aberration" => crate::vr::vr_chromatic_aberration(f, pr, t),
        "cartoon" => { let (levels, estr, ethr) = (g("levels", 5.0).clamp(2.0, 16.0), g("edge_strength", 1.0).clamp(0.0, 2.0), g("edge_threshold", 0.15).clamp(0.0, 1.0));
            let gray: Vec<f32> = (0..w * h).map(|i| lum([f.d[i * 4], f.d[i * 4 + 1], f.d[i * 4 + 2]])).collect();
            let src = f.d.clone();
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4;
                let gx = gray[y * w + (x + 1).min(w - 1)] - gray[y * w + x.saturating_sub(1)];
                let gy = gray[(y + 1).min(h - 1) * w + x] - gray[y.saturating_sub(1) * w + x];
                let grad = (gx * gx + gy * gy).sqrt();
                let edge = if grad > ethr { (1.0 - estr).max(0.0) } else { 1.0 };
                for j in 0..3 { f.d[i + j] = ((src[i + j] * levels).round() / levels) * edge; }
            } }
        }
        "color_emboss" => { let (ang, strength, depth) = (g("angle", 135.0).to_radians(), g("strength", 1.0).clamp(0.0, 4.0), g("depth", 1.0).max(0.1));
            let (dx, dy) = (ang.cos() * depth, ang.sin() * depth); let src = f.d.clone();
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let o = sample(&src, w, h, x as f32 - dx, y as f32 - dy);
                for j in 0..3 { f.d[i + j] = (src[i + j] + (src[i + j] - o[j]) * strength).clamp(0.0, 1.0); } } }
        }
        "roughen_edges" => { let (amount, size) = (g("amount", 6.0).clamp(0.0, 200.0), g("size", 20.0).max(1.0));
            let alpha: Vec<f32> = (0..w * h).map(|i| f.d[i * 4 + 3]).collect(); let src = f.d.clone();
            let ga = |x: usize, y: usize| -> f32 { let gx = alpha[y * w + (x + 1).min(w - 1)] - alpha[y * w + x.saturating_sub(1)]; let gy = alpha[(y + 1).min(h - 1) * w + x] - alpha[y.saturating_sub(1) * w + x]; (gx * gx + gy * gy).sqrt().min(1.0) };
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let band = ga(x, y);
                if band < 1e-4 { continue; }
                let (nx, ny) = (fbm(x as f32 / size, y as f32 / size, 2, seed as u32) - 0.5, fbm(x as f32 / size + 100.0, y as f32 / size, 2, seed as u32) - 0.5);
                let o = sample(&src, w, h, x as f32 + nx * amount * band * 2.0, y as f32 + ny * amount * band * 2.0);
                f.d[i..i + 4].copy_from_slice(&o);
            } }
        }
        "scatter" => { let amount = g("amount", 6.0).clamp(0.0, 200.0); let s = seed as u32;
            warp(f, |x, y| (x + (hash(x as i32, y as i32, s) - 0.5) * 2.0 * amount, y + (hash(x as i32, y as i32, s.wrapping_add(1)) - 0.5) * 2.0 * amount));
        }
        "texturize" => { let (scale, amount, oct) = (g("scale", 0.1).max(1e-4), g("amount", 0.5).clamp(0.0, 2.0), g("octaves", 3.0).clamp(1.0, 6.0) as usize); let s = seed as u32;
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let b = fbm(x as f32 * scale, y as f32 * scale, oct, s);
                let k = 1.0 + (b - 0.5) * amount * 2.0; for j in 0..3 { f.d[i + j] = (f.d[i + j] * k).clamp(0.0, 4.0); } } }
        }
        "cc_glass" => { let (amount, scale) = (g("amount", 20.0).clamp(0.0, 400.0), g("scale", 0.02).max(1e-5)); let s = seed as u32; let src = f.d.clone();
            let bump = |x: f32, y: f32| fbm(x * scale, y * scale, 4, s);
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4;
                let gx = bump(x as f32 + 1.0, y as f32) - bump(x as f32 - 1.0, y as f32); let gy = bump(x as f32, y as f32 + 1.0) - bump(x as f32, y as f32 - 1.0);
                let o = sample(&src, w, h, x as f32 + gx * amount, y as f32 + gy * amount); f.d[i..i + 4].copy_from_slice(&o);
            } }
        }
        "cc_kaleida" => { let (seg, cx, cy, ang0, rscale) = (g("segments", 6.0).clamp(2.0, 64.0) as i32, g("center_x", 0.5) * wf, g("center_y", 0.5) * hf, g("angle_offset", 0.0).to_radians(), g("radius_scale", 1.0).max(0.01));
            let wedge = std::f32::consts::TAU / seg as f32; let src = f.d.clone();
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let r = (dx * dx + dy * dy).sqrt() / rscale; let mut a = dy.atan2(dx) - ang0; a = a.rem_euclid(wedge); if a > wedge / 2.0 { a = wedge - a; }
                let (sx, sy) = (cx + r * a.cos(), cy + r * a.sin()); let o = sample(&src, w, h, sx, sy); f.d[i..i + 4].copy_from_slice(&o);
            } }
        }
        "cc_threshold_rgb" => { let t3 = [g("r_threshold", 0.5), g("g_threshold", 0.5), g("b_threshold", 0.5)];
            for i in 0..w * h { for j in 0..3 { f.d[i * 4 + j] = if f.d[i * 4 + j] >= t3[j] { 1.0 } else { 0.0 }; } }
        }
        "dither" => { const BAYER: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
            let (levels, strength) = (g("levels", 4.0).clamp(2.0, 16.0), g("strength", 1.0).clamp(0.0, 1.0)); let span = (levels - 1.0).max(1.0);
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let th = (BAYER[(y % 4) * 4 + (x % 4)] + 0.5) / 16.0 - 0.5;
                for j in 0..3 { let v = f.d[i + j]; f.d[i + j] = ((v * span + th * strength).round() / span).clamp(0.0, 1.0); } } }
        }
        "tile_mosaic" => { let size = (g("size", 16.0).clamp(1.0, 256.0)) as usize; let src = f.d.clone();
            let mut by = 0; while by < h { let mut bx = 0; while bx < w { let (bw, bh) = ((size).min(w - bx), (size).min(h - by));
                let mut acc = [0f32; 4]; for yy in by..by + bh { for xx in bx..bx + bw { for j in 0..4 { acc[j] += src[(yy * w + xx) * 4 + j]; } } }
                let n = (bw * bh).max(1) as f32; for j in 0..4 { acc[j] /= n; }
                for yy in by..by + bh { for xx in bx..bx + bw { f.d[(yy * w + xx) * 4..][..4].copy_from_slice(&acc); } }
                bx += size; } by += size; }
        }
        "sketch_pencil" => { let (radius, strength, color_mix) = (g("blur_radius", 6.0).clamp(1.0, 64.0) as usize, g("strength", 1.0).clamp(0.0, 1.0), g("color", 0.0).clamp(0.0, 1.0));
            let src = f.d.clone(); let mut inv = Frame { w, h, d: (0..w * h).flat_map(|i| { let l = 1.0 - lum([src[i * 4], src[i * 4 + 1], src[i * 4 + 2]]); [l, l, l, 1.0] }).collect() };
            box_blur3(&mut inv, radius);
            for i in 0..w * h { let g_ = lum([src[i * 4], src[i * 4 + 1], src[i * 4 + 2]]); let b = inv.d[i * 4].min(0.995);
                let sk = (g_ / (1.0 - b).max(1e-3)).clamp(0.0, 1.0);
                for j in 0..3 { let tinted = sk * (1.0 - color_mix) + sk * src[i * 4 + j] * color_mix; f.d[i * 4 + j] = src[i * 4 + j] + (tinted - src[i * 4 + j]) * strength; } }
        }
        "oil_paint" => { let (radius, levels) = (g("radius", 3.0).clamp(1.0, 8.0) as i32, g("levels", 20.0).clamp(2.0, 64.0) as usize);
            let src = f.d.clone(); let gray: Vec<f32> = (0..w * h).map(|i| lum([src[i * 4], src[i * 4 + 1], src[i * 4 + 2]])).collect();
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4;
                let mut counts = vec![0u32; levels]; let mut sums = vec![[0f32; 3]; levels];
                for dy in -radius..=radius { for dx in -radius..=radius { let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize; let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let bucket = ((gray[yy * w + xx] * (levels - 1) as f32).round() as usize).min(levels - 1); counts[bucket] += 1;
                    for j in 0..3 { sums[bucket][j] += src[(yy * w + xx) * 4 + j]; } } }
                let best = (0..levels).max_by_key(|&k| counts[k]).unwrap(); let n = counts[best].max(1) as f32;
                for j in 0..3 { f.d[i + j] = sums[best][j] / n; }
            } }
        }
        "anaglyph" => { let shift = g("shift", 4.0).clamp(0.0, 100.0); let src = f.d.clone();
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4;
                let left = sample(&src, w, h, x as f32 - shift, y as f32); let right = sample(&src, w, h, x as f32 + shift, y as f32);
                f.d[i] = left[0]; f.d[i + 1] = right[1]; f.d[i + 2] = right[2];
            } }
        }
        "cc_burn_film" => { let (progress, edge_w) = (g("progress", 0.0).clamp(0.0, 1.0), g("edge_width", 0.1).clamp(0.001, 1.0));
            let col = [g("burn_r", 1.0), g("burn_g", 0.45), g("burn_b", 0.05)]; let s = seed as u32;
            let sstep = |a: f32, b: f32, x: f32| { let t = ((x - a) / (b - a).max(1e-6)).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) };
            for y in 0..h { for x in 0..w { let i = (y * w + x) * 4; let n = fbm(x as f32 * 0.01, y as f32 * 0.01, 4, s);
                // a_front: 0 fully burned away (noise below the progress threshold), 1 untouched; a_front*(1-a_front) peaks exactly
                // at the burn edge itself and falls to 0 on both the ash side and the untouched side -- the ember glow band
                let a_front = sstep(progress, progress + edge_w, n);
                let ember = a_front * (1.0 - a_front) * 4.0;
                for j in 0..3 { f.d[i + j] = f.d[i + j] * a_front + col[j] * ember; } f.d[i + 3] *= a_front;
            } }
        }
        "droste_zoom" => { let (cx, cy) = (g("cx", 0.5) * wf, g("cy", 0.5) * hf); let zoom = g("zoom", 1.6).max(1.02); let twist = g("twist", 12.0).to_radians(); let drift = g("drift", 0.12); let mix = g("mix", 1.0).clamp(0.0, 1.0);
            if mix > 0.001 {
                // rmax anchors the "home ring": the radius from the centre whose wrapped copy always lands back on
                // real picture content. BUG FIXED HERE: the original version wrapped the raw pixel radius r (which
                // is tens to hundreds of px) straight into [0, ln(zoom)) and un-logged it, so r2 = exp(wrapped) was
                // ALWAYS within about 1..zoom px of the centre -- every output pixel sampled the same ~1px speck,
                // which is exactly the flat brown/orange wash with no scene content that was reported. Wrapping the
                // *ratio* r/rmax instead, centred on 0, keeps every sampled radius within rmax/sqrt(zoom)..rmax*sqrt(zoom)
                // of the centre -- real image content at every ring, nested self-similarly as zoom/twist repeat it.
                let rmax = md * 0.42; let period = zoom.ln(); let half = period * 0.5; let prog = t as f32 * drift * period; let src = f.d.clone();
                for y in 0..h { for x in 0..w { let (dx, dy) = (x as f32 - cx, y as f32 - cy); let r = (dx * dx + dy * dy).sqrt().max(0.5); let th = dy.atan2(dx);
                    let lr = (r / rmax).ln() + prog; let wrapped = (lr + half).rem_euclid(period) - half; let k = ((lr - wrapped) / period).round();
                    let th2 = th + k * twist; let r2 = rmax * wrapped.exp();
                    let v = sample(&src, w, h, cx + r2 * th2.cos(), cy + r2 * th2.sin()); let i = (y * w + x) * 4; for j in 0..4 { f.d[i + j] = src[i + j] * (1.0 - mix) + v[j] * mix; } } } } }
        "portal_mirror" => { let (cx, cy) = (g("cx", 0.5) * wf, g("cy", 0.5) * hf); let radius = g("radius", 0.28).max(0.01) * md; let feather = g("feather", 0.08).max(0.001) * md; let invert_color = g("invert_color", 1.0) > 0.5; let flip_v = g("flip_vertical", 1.0) > 0.5; let mix = g("mix", 1.0).clamp(0.0, 1.0);
            if mix > 0.001 { let src = f.d.clone();
                for y in 0..h { for x in 0..w { let (dx, dy) = (x as f32 - cx, y as f32 - cy); let d = (dx * dx + dy * dy).sqrt(); let inside = ((radius + feather - d) / feather).clamp(0.0, 1.0); if inside <= 0.0 { continue; }
                    let (sx, sy) = if flip_v { (x as f32, cy - dy) } else { (x as f32, y as f32) }; let mut v = sample(&src, w, h, sx, sy); if invert_color { for j in 0..3 { v[j] = 1.0 - v[j]; } }
                    let i = (y * w + x) * 4; let k = inside * mix; for j in 0..4 { f.d[i + j] = f.d[i + j] * (1.0 - k) + v[j] * k; } } } } }
        "melt_drip" => { let amount = g("amount", 18.0); let size = g("size", 40.0).max(4.0); let speed = g("speed", 0.3); let angle = g("angle", 90.0).to_radians(); let mix = g("mix", 1.0).clamp(0.0, 1.0);
            if mix > 0.001 { let (ux, uy) = (angle.cos(), angle.sin()); let tt = t as f32 * speed; let sd = seed as u32; let src = f.d.clone(); let span = (wf * ux.abs() + hf * uy.abs()).max(1.0);
                for y in 0..h { for x in 0..w {
                    // perp picks which noise "drip lane" this pixel falls under; along grows from 0 (the edge the drip flows from) to 1 (the
                    // far edge). FIX: 3 octaves of fbm here read as a jagged sawtooth/zigzag (high-frequency wobble stacked on top of the base
                    // lane pattern, reported as "a jagged wave, nothing flowing") instead of a smooth melt -- single-octave noise is already a
                    // smooth bilinear field (see vnoise), so dropping to 1 octave gives broad, softly-varying lanes with no fine jitter. A
                    // smoothstep on `along` (instead of a raw square) also keeps the acceleration gentle near the far edge instead of lurching.
                    let perp = (x as f32 * -uy + y as f32 * ux) / size; let along = ((x as f32 * ux + y as f32 * uy) / span).clamp(0.0, 1.0);
                    let smooth_along = along * along * (3.0 - 2.0 * along);
                    let n = fbm(perp, tt, 1, sd); let drip = amount * smooth_along * n;
                    let v = sample(&src, w, h, x as f32 - ux * drip, y as f32 - uy * drip); let i = (y * w + x) * 4; for j in 0..4 { f.d[i + j] = src[i + j] * (1.0 - mix) + v[j] * mix; } } } } }
        other => return crate::color::apply3(other, pr, f, t, seed),
    }
    Ok(())
}

/// Exact Euclidean distance (px) from every pixel to the nearest pixel where `m` is set (Felzenszwalb-Huttenlocher, separable);
/// 1e9 where no pixel is set.
fn edt(m: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f64 = 1e18;
    fn pass(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
        let n = f.len(); let Some(p0) = (0..n).find(|&q| f[q] < INF) else { for q in 0..n { d[q] = INF; } return; };
        let mut k = 0usize; v[0] = p0; z[0] = -INF; z[1] = INF;
        for q in p0 + 1..n { if f[q] >= INF { continue; }
            let mut s; loop { let p = v[k]; s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64); if s <= z[k] { k -= 1; } else { break; } }
            k += 1; v[k] = q; z[k] = s; z[k + 1] = INF; }
        k = 0; for q in 0..n { while z[k + 1] < q as f64 { k += 1; } let p = v[k]; d[q] = (q as f64 - p as f64).powi(2) + f[p]; }
    }
    let n = w.max(h); let (mut f, mut d, mut v, mut z) = (vec![0f64; n], vec![0f64; n], vec![0usize; n], vec![0f64; n + 1]);
    let mut g = vec![INF; w * h];
    for x in 0..w { for y in 0..h { f[y] = if m[y * w + x] { 0.0 } else { INF }; } pass(&f[..h], &mut d[..h], &mut v[..h], &mut z[..h + 1]); for y in 0..h { g[y * w + x] = d[y]; } }
    for y in 0..h { f[..w].copy_from_slice(&g[y * w..y * w + w]); pass(&f[..w], &mut d[..w], &mut v[..w], &mut z[..w + 1]); for x in 0..w { g[y * w + x] = d[x]; } }
    g.iter().map(|&q| if q >= INF * 0.5 { 1e9 } else { q.sqrt() as f32 }).collect()
}
/// Smooth value noise in 0..1 over (x, y) on an integer lattice, interpolated in z (time) too.
fn saber_noise(x: f32, y: f32, z: f32, s: u32) -> f32 {
    let (xi, yi, zi) = (x.floor(), y.floor(), z.floor()); let (fx, fy, fz) = (x - xi, y - yi, z - zi); let sm = |u: f32| u * u * (3.0 - 2.0 * u); let (ux, uy, uz) = (sm(fx), sm(fy), sm(fz));
    let at = |dz: i32| { let ss = s.wrapping_add(((zi as i32 + dz) as u32).wrapping_mul(7919)); let (a, b, c, d) = (hash(xi as i32, yi as i32, ss), hash(xi as i32 + 1, yi as i32, ss), hash(xi as i32, yi as i32 + 1, ss), hash(xi as i32 + 1, yi as i32 + 1, ss)); (a + (b - a) * ux) + ((c + (d - c) * ux) - (a + (b - a) * ux)) * uy };
    at(0) + (at(1) - at(0)) * uz
}
/// Saber (Video Copilot Saber) / a neon light stroke. The layer's alpha (>= 0.5 is inside) gives an exact distance field: in mode 0
/// the distance to the outline (both sides), in mode 1 the distance to the shape (0 inside). The core is 1 within core_size px
/// (0.75 px soft edge); the glow is glow_intensity * (0.6 e^(-x/spread) + 0.4 e^(-(x/0.35 spread)^2)) with x the distance past
/// the core, tapered to 0 at 5 x spread; inside the outline (mode 0) only the gaussian part, so the layer's middle stays as it was.
/// The light is reveal-masked along reveal_angle across the layer's alpha bounds (start..end, 1.5 px soft), times a flicker
/// (1 - flicker * value noise at flicker_speed), and wobbled by distortion px of animated value noise. It is added as emitted
/// light: premultiplied colour + light, coverage = its brightest channel (composite 1: the light alone). Pixels with no light
/// keep their exact value (composite 0), so end <= start leaves the layer bit-exact.
fn saber(pr: &Params, f: &mut Frame, t: f64) {
    let g = |k: &str, d: f64| p(pr, k, d, t);
    let (w, h) = (f.w, f.h); if w == 0 || h == 0 { return; }
    let (start, end) = (g("start", 0.0).clamp(0.0, 1.0), g("end", 1.0).clamp(0.0, 1.0)); let comp = g("composite", 0.0) >= 0.5;
    if end <= start && !comp { return; }
    let fill = g("mode", 0.0) >= 0.5; let core = g("core_size", 2.0).max(0.0); let cc = [g("core_r", 1.0), g("core_g", 1.0), g("core_b", 1.0)]; let gc = [g("glow_r", 0.15), g("glow_g", 0.55), g("glow_b", 1.0)];
    let gi = g("glow_intensity", 1.5).max(0.0); let sp = g("glow_spread", 10.0).max(0.5); let fl = g("flicker", 0.0).clamp(0.0, 1.0); let fs = g("flicker_speed", 12.0).max(0.0);
    let dist = g("distortion", 0.0).max(0.0); let ds = g("distortion_speed", 1.0);
    let inside: Vec<bool> = (0..w * h).map(|i| f.d[i * 4 + 3] >= 0.5).collect();
    if !inside.iter().any(|&b| b) { if comp { for v in f.d.iter_mut() { *v = 0.0; } } return; }
    let to_in = edt(&inside, w, h); let outside: Vec<bool> = inside.iter().map(|b| !b).collect(); let to_out = edt(&outside, w, h);
    // e: distance to the stroke (outline or shape), negative-free; ins: the pixel is inside the outline (mode 0 inner glow)
    let field: Vec<f32> = (0..w * h).map(|i| if inside[i] { if fill { 0.0 } else { -(to_out[i] - 0.5).max(0.0) } } else { (to_in[i] - 0.5).max(0.0) }).collect();
    let sample = |x: f32, y: f32| -> f32 { let (x, y) = (x.clamp(0.0, (w - 1) as f32), y.clamp(0.0, (h - 1) as f32)); let (x0, y0) = (x.floor() as usize, y.floor() as usize); let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1)); let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let a = field[y0 * w + x0] + (field[y0 * w + x1] - field[y0 * w + x0]) * fx; let b = field[y1 * w + x0] + (field[y1 * w + x1] - field[y1 * w + x0]) * fx; a + (b - a) * fy };
    // reveal coordinate: projection on the reveal direction, 0..1 across the layer's alpha bounds
    let ang = g("reveal_angle", 0.0).to_radians(); let (dx, dy) = (ang.cos(), ang.sin());
    let (mut lo, mut hi) = (f32::MAX, f32::MIN); for y in 0..h { for x in 0..w { if f.d[(y * w + x) * 4 + 3] > 0.01 { let u = (x as f32 + 0.5) * dx + (y as f32 + 0.5) * dy; lo = lo.min(u); hi = hi.max(u); } } }
    let span = (hi - lo).max(1.0); let soft = 1.5 / span;
    let ss = |a: f32, b: f32, x: f32| { let u = ((x - a) / (b - a)).clamp(0.0, 1.0); u * u * (3.0 - 2.0 * u) };
    let flick = if fl > 0.0 { let z = t as f32 * fs; let k = z.floor(); let u = z - k; let n = hash(k as i32, 77, 991) + (hash(k as i32 + 1, 77, 991) - hash(k as i32, 77, 991)) * (u * u * (3.0 - 2.0 * u)); 1.0 - fl * n } else { 1.0 };
    let tz = t as f32 * ds;
    for y in 0..h { for x in 0..w {
        let i = y * w + x; let o = i * 4;
        let u = ((((x as f32 + 0.5) * dx + (y as f32 + 0.5) * dy) - lo) / span).clamp(0.0, 1.0);
        let m = if end <= start { 0.0 } else { let a = if start <= 0.0 { 1.0 } else { ss(start - soft, start, u) }; let b = if end >= 1.0 { 1.0 } else { 1.0 - ss(end, end + soft, u) }; a * b };
        let mut light = [0f32; 3];
        if m > 0.0 {
            let sd = if dist > 0.0 { let (fx, fy) = (x as f32 / 20.0, y as f32 / 20.0); sample(x as f32 + dist * (2.0 * saber_noise(fx, fy, tz, 17) - 1.0), y as f32 + dist * (2.0 * saber_noise(fx, fy, tz, 29) - 1.0)) } else { field[i] };
            let (e, ins) = (sd.abs(), sd < 0.0);
            let k_core = 1.0 - ss(core - 0.75, core + 0.75, e);
            let xg = (e - core).max(0.0); let taper = (1.0 - (xg / (5.0 * sp)).powi(2)).max(0.0).powi(2);
            let k_glow = gi * taper * if ins { 0.5 * (-(xg / (0.35 * sp)).powi(2)).exp() } else { 0.6 * (-xg / sp).exp() + 0.4 * (-(xg / (0.35 * sp)).powi(2)).exp() };
            let k = m * flick; for j in 0..3 { light[j] = k * (k_core * cc[j] + k_glow * gc[j]); }
        }
        let ka = light[0].max(light[1]).max(light[2]).clamp(0.0, 1.0);
        if comp { if ka > 1e-6 { for j in 0..3 { f.d[o + j] = light[j] / ka; } } else { for j in 0..3 { f.d[o + j] = 0.0; } } f.d[o + 3] = ka; continue; }
        if ka <= 0.0 { continue; }
        let a = f.d[o + 3].clamp(0.0, 1.0); let na = (a + ka * (1.0 - a)).min(1.0);
        for j in 0..3 { f.d[o + j] = (f.d[o + j] * a + light[j]) / na; } f.d[o + 3] = na;
    } }
}

/// CC Light Sweep. The band is tilted `angle` degrees from vertical and travels along its normal (left to right at 0, top to
/// bottom at 90); position 0 and 1 park it just off the frame, speed adds sweeps per second (wrapping). width is the band's
/// width as a fraction of the travel span; shape 0 linear, 1 smooth (cosine), 2 sharp (flat core). Light is added in the
/// layer's straight colour: reception 0 on the layer only, 1 also over its transparent part, 2 the light alone cut to the layer.
/// edge_intensity lights the layer's alpha border (pixels within edge_width px of transparency) inside the band.
fn light_sweep(pr: &Params, f: &mut Frame, t: f64) {
    let g = |k: &str, d: f64| p(pr, k, d, t);
    let (w, h) = (f.w, f.h); let (wf, hf) = (w as f32, h as f32); if w == 0 || h == 0 { return; }
    let ang = g("angle", 0.0).to_radians(); let (nx, ny) = (ang.cos(), ang.sin());
    let half = (wf * nx.abs() + hf * ny.abs()) / 2.0; let bw = (g("width", 0.12).max(0.0) * 2.0 * half).max(0.5); let hw = bw / 2.0;
    let (pos, sp) = (g("position", 0.5), g("speed", 0.0)); let pos = if sp != 0.0 { (pos + t as f32 * sp).rem_euclid(1.0) } else { pos };
    let s0 = -half - hw + pos * (2.0 * half + bw);
    let shape = g("shape", 1.0).round() as i32;
    let prof = |d: f32| -> f32 { let u = d.abs() / hw; if u >= 1.0 { return 0.0; } match shape { 0 => 1.0 - u, 2 => { let v = ((1.0 - u) / 0.35).min(1.0); v * v * (3.0 - 2.0 * v) } _ => 0.5 + 0.5 * (std::f32::consts::PI * u).cos() } };
    let (si, ei, ew) = (g("intensity", 1.0), g("edge_intensity", 0.0), g("edge_width", 3.0).max(0.0)); let col = [g("color_r", 1.0), g("color_g", 1.0), g("color_b", 1.0)]; let rec = g("reception", 0.0).round() as i32;
    // alpha border: alpha minus its minimum over a (2r+1)^2 box (separable); the frame's own border is not an edge
    let edge: Option<Vec<f32>> = (ei != 0.0 && ew > 0.0).then(|| { let r = ew.ceil() as usize; let a: Vec<f32> = (0..w * h).map(|i| f.d[i * 4 + 3]).collect(); let mut tmp = vec![0f32; w * h];
        for y in 0..h { for x in 0..w { tmp[y * w + x] = (x.saturating_sub(r)..=(x + r).min(w - 1)).map(|xx| a[y * w + xx]).fold(1.0, f32::min); } }
        (0..w * h).map(|i| { let (x, y) = (i % w, i / w); (a[i] - (y.saturating_sub(r)..=(y + r).min(h - 1)).map(|yy| tmp[yy * w + x]).fold(1.0, f32::min)).max(0.0) }).collect() });
    let (cx, cy) = (wf / 2.0, hf / 2.0);
    for y in 0..h { for x in 0..w {
        let i = (y * w + x) * 4; let b = prof((x as f32 + 0.5 - cx) * nx + (y as f32 + 0.5 - cy) * ny - s0);
        let k = b * (si + ei * edge.as_ref().map_or(0.0, |e| e[y * w + x])); let a = f.d[i + 3];
        match rec {
            1 => { let na = (a + k.clamp(0.0, 1.0) * (1.0 - a)).min(1.0); if na > 1e-6 { for j in 0..3 { f.d[i + j] = (f.d[i + j] * a + k * col[j]) / na; } } f.d[i + 3] = na; }
            2 => { let ka = k.clamp(0.0, 1.0); if ka > 1e-6 { for j in 0..3 { f.d[i + j] = col[j] * k / ka; } } else { for j in 0..3 { f.d[i + j] = 0.0; } } f.d[i + 3] = ka * a; }
            _ => { if k != 0.0 { for j in 0..3 { f.d[i + j] += k * col[j]; } } }
        }
    } }
}
