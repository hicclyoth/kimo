//! Transition engine: eased progress, soft-edged angled wipes, iris/clock/split/blinds/noise reveals, slides with overshoot,
//! zoom/spin, dips to a colour, flash, light leak, film burn, glitch, whip pan and zoom blur.
use crate::effects::Frame;
use crate::model::Transition;

pub const KINDS: &[&str] = &["crossfade", "dissolve", "wipe", "wipe_left", "wipe_right", "wipe_up", "wipe_down", "diagonal", "blinds", "checker", "iris", "clock", "split_h", "split_v", "noise_dissolve", "block_dissolve", "card_wipe", "slide_left", "slide_right", "slide_up", "slide_down", "zoom", "zoom_blur", "spin", "flash", "dip_black", "dip_white", "dip_color", "light_leak", "film_burn", "glitch", "whip_left", "whip_right", "shatter", "shatter_cells", "ripple_dissolve", "barn_doors", "page_peel"];
pub const EASES: &[&str] = &["linear", "in", "out", "in_out", "in_cubic", "out_cubic", "in_out_cubic", "in_out_expo", "out_back", "out_bounce", "out_quart", "out_expo", "in_expo", "in_out_quint", "in_back", "in_out_back", "in_elastic", "out_elastic", "in_out_elastic", "in_bounce", "in_out_bounce"];

pub fn ease(name: &str, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match name { "linear" => t, "in" => t * t, "out" => 1.0 - (1.0 - t).powi(2), "in_cubic" => t.powi(3), "out_cubic" => 1.0 - (1.0 - t).powi(3),
        "in_out_cubic" => if t < 0.5 { 4.0 * t.powi(3) } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 },
        "in_out_expo" => if t <= 0.0 { 0.0 } else if t >= 1.0 { 1.0 } else if t < 0.5 { 2f64.powf(20.0 * t - 10.0) / 2.0 } else { (2.0 - 2f64.powf(-20.0 * t + 10.0)) / 2.0 },
        "out_back" => { let (c1, c3) = (1.70158, 2.70158); 1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2) }
        "out_bounce" => { let (n, d) = (7.5625, 2.75); if t < 1.0 / d { n * t * t } else if t < 2.0 / d { let t = t - 1.5 / d; n * t * t + 0.75 } else if t < 2.5 / d { let t = t - 2.25 / d; n * t * t + 0.9375 } else { let t = t - 2.625 / d; n * t * t + 0.984375 } }
        n if n != "in_out" && n != "hold" && crate::keyframe::Ease::parse(n).is_some() => crate::keyframe::Ease::parse(n).unwrap().apply(t), // the shared keyframe easings
        _ => t * t * (3.0 - 2.0 * t) }
}
fn sstep(x: f32) -> f32 { let t = x.clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) }
fn hash(x: i32, y: i32, s: u32) -> f32 { let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ s.wrapping_mul(2246822519); h = (h ^ (h >> 13)).wrapping_mul(1274126177); ((h ^ (h >> 16)) & 0xffffff) as f32 / 16777215.0 }
fn vnoise(x: f32, y: f32, s: u32) -> f32 { let (xi, yi) = (x.floor(), y.floor()); let (fx, fy) = (sstep(x - xi), sstep(y - yi)); let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi, s) * (1.0 - fx) + hash(xi + 1, yi, s) * fx; let b = hash(xi, yi + 1, s) * (1.0 - fx) + hash(xi + 1, yi + 1, s) * fx; a * (1.0 - fy) + b * fy }


fn voro(x: f32, y: f32, cs: f32, seed: u32) -> (f32, f32, f32, f32, f32) { // (cell hash, d1, d2, cx, cy)
    let (gx, gy) = ((x / cs).floor() as i32, (y / cs).floor() as i32); let (mut d1, mut d2, mut best, mut bc) = (1e9f32, 1e9f32, (0, 0), (0.0, 0.0));
    for j in -1..=1 { for i in -1..=1 { let (cx, cy) = (gx + i, gy + j); let px = (cx as f32 + 0.15 + 0.7 * hash(cx, cy, seed)) * cs; let py = (cy as f32 + 0.15 + 0.7 * hash(cx, cy, seed ^ 0x9e37)) * cs; let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
        if d < d1 { d2 = d1; d1 = d; best = (cx, cy); bc = (px, py); } else if d < d2 { d2 = d; } } }
    (hash(best.0, best.1, seed ^ 0x51ed), d1, d2, bc.0, bc.1) }
pub type Mask = Box<dyn Fn(usize, usize) -> f32>;
pub struct Plan { pub op: f32, pub mask: Option<Mask>, pub dx: f64, pub dy: f64, pub scale: f64, pub rot: f64,
    pub dip: Option<([f32; 3], f32)>, pub post: Option<Box<dyn Fn(&mut Frame)>>, pub overlay: Option<Box<dyn Fn(&mut Frame)>>, pub compose: Option<Box<dyn Fn(&Frame, &Frame) -> Frame>>,
    /// push / gradient_wipe (pushwipe.rs): runs on the canvas under the incoming clip before it is drawn, given the outgoing abutting clip's last frame when there is one.
    pub under: Option<Box<dyn Fn(&mut Frame, Option<&Frame>)>>,
    /// morph_cut: optical-flow weight from the previous abutting clip's last frame (0) to this clip (1).
    pub morph: Option<f32> }

fn dir_blur(f: &mut Frame, dx: f32, dy: f32, n: usize) { if dx.abs() + dy.abs() < 0.5 { return; } let (w, h) = (f.w, f.h); let src = f.d.clone();
    for y in 0..h { for x in 0..w { let mut acc = [0f32; 4]; for k in 0..n { let o = k as f32 / (n - 1) as f32 - 0.5; let sx = ((x as f32 + dx * o).round() as i32).clamp(0, w as i32 - 1) as usize; let sy = ((y as f32 + dy * o).round() as i32).clamp(0, h as i32 - 1) as usize; let i = (sy * w + sx) * 4; let a = src[i + 3]; for j in 0..3 { acc[j] += src[i + j] * a; } acc[3] += a; }
        let o = (y * w + x) * 4; if acc[3] > 0.0 { for j in 0..3 { f.d[o + j] = acc[j] / acc[3]; } } f.d[o + 3] = acc[3] / n as f32; } } }
fn radial_blur(f: &mut Frame, amount: f32, n: usize) { if amount < 0.002 { return; } let (w, h) = (f.w, f.h); let src = f.d.clone(); let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    for y in 0..h { for x in 0..w { let mut acc = [0f32; 4]; for k in 0..n { let s = 1.0 - amount * k as f32 / (n - 1) as f32; let sx = ((cx + (x as f32 - cx) * s).round() as i32).clamp(0, w as i32 - 1) as usize; let sy = ((cy + (y as f32 - cy) * s).round() as i32).clamp(0, h as i32 - 1) as usize; let i = (sy * w + sx) * 4; let a = src[i + 3]; for j in 0..3 { acc[j] += src[i + j] * a; } acc[3] += a; }
        let o = (y * w + x) * 4; if acc[3] > 0.0 { for j in 0..3 { f.d[o + j] = acc[j] / acc[3]; } } f.d[o + 3] = acc[3] / n as f32; } } }

pub fn plan(tr: &Transition, lt: f64, w: usize, h: usize) -> Plan {
    let kr = (lt / tr.duration).clamp(0.0, 1.0); let k = ease(&tr.ease, kr) as f32; let (wf, hf) = (w as f32, h as f32); let soft = tr.softness.max(0.0) as f32; let seed = tr.seed;
    let mut p = Plan { op: 1.0, mask: None, dx: 0.0, dy: 0.0, scale: 1.0, rot: 0.0, dip: None, post: None, overlay: None, compose: None, morph: None, under: None };
    let wipe = |ang_deg: f32, k: f32| -> Mask { let (c, s) = (ang_deg.to_radians().cos(), ang_deg.to_radians().sin()); let half = (wf * c.abs() + hf * s.abs()) / 2.0; let sw = (soft * half * 2.0).max(1.0); let edge = -half + k * (2.0 * half + sw);
        Box::new(move |x, y| { let pr = (x as f32 - wf / 2.0) * c + (y as f32 - hf / 2.0) * s; sstep((edge - pr) / sw) }) };
    match tr.kind.as_str() {
        "crossfade" | "dissolve" => p.op = k,
        "morph_cut" => p.morph = Some(k),
        "push" => crate::pushwipe::push_plan(&mut p, tr, k, w, h),
        "gradient_wipe" => crate::pushwipe::gradient_plan(&mut p, tr, k, w, h),
        "wipe" => p.mask = Some(wipe(tr.angle as f32, k)), "wipe_left" => p.mask = Some(wipe(0.0, k)), "wipe_right" => p.mask = Some(wipe(180.0, k)), "wipe_up" => p.mask = Some(wipe(90.0, k)), "wipe_down" => p.mask = Some(wipe(270.0, k)),
        "diagonal" => p.mask = Some(wipe(if tr.angle != 0.0 { tr.angle as f32 } else { 45.0 }, k)),
        "iris" => { let r = k * (wf * wf + hf * hf).sqrt() * 0.5 * (1.0 + soft); let sw = (soft * wf * 0.5).max(1.5); p.mask = Some(Box::new(move |x, y| { let d = ((x as f32 - wf / 2.0).powi(2) + (y as f32 - hf / 2.0).powi(2)).sqrt(); sstep((r - d) / sw) })); }
        "clock" => { let sw = (soft * 0.5).max(0.004); let a0 = (tr.angle as f32 - 90.0).to_radians(); p.mask = Some(Box::new(move |x, y| { let mut a = ((y as f32 - hf / 2.0).atan2(x as f32 - wf / 2.0) - a0).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU; if a.is_nan() { a = 0.0 } sstep((k * (1.0 + sw) - a) / sw) })); }
        "split_h" => { let sw = (soft * wf * 0.5).max(1.5); p.mask = Some(Box::new(move |x, _| sstep((k * wf * 0.5 - (x as f32 - wf / 2.0).abs()) / sw + 0.0))); }
        "split_v" => { let sw = (soft * hf * 0.5).max(1.5); p.mask = Some(Box::new(move |_, y| sstep((k * hf * 0.5 - (y as f32 - hf / 2.0).abs()) / sw))); }
        "blinds" => { let n = tr.count.max(2.0) as f32; let hh = hf / n; let sw = (soft * 0.5).max(0.02); p.mask = Some(Box::new(move |_, y| { let v = (y as f32 % hh) / hh; sstep((k * (1.0 + sw) - v) / sw) })); }
        "checker" => { let s = (wf / tr.count.max(2.0) as f32).max(1.0); let sw = (soft * 0.5).max(0.03); p.mask = Some(Box::new(move |x, y| { let (cx, cy) = ((x as f32 / s) as i32, (y as f32 / s) as i32); let off = if (cx + cy) % 2 == 0 { 0.0 } else { 0.5 }; sstep((k * (1.0 + 0.5 + sw) - off - ((x as f32 / s).fract() * 0.0)) / sw.max(0.2) ) })); }
        "noise_dissolve" => { let sc = (tr.count.max(1.0) as f32 * 4.0) / wf; let sw = soft.max(0.05); p.mask = Some(Box::new(move |x, y| { let n = vnoise(x as f32 * sc, y as f32 * sc, seed) * 0.65 + vnoise(x as f32 * sc * 2.3, y as f32 * sc * 2.3, seed + 7) * 0.35; sstep((k * (1.0 + sw) - n) / sw) })); }
        "block_dissolve" => { // After Effects' Block Dissolve: discrete rectangular blocks pop to the incoming frame in a random order, not the smooth per-pixel noise_dissolve.
            let n = tr.count.clamp(2.0, 64.0) as f32; let bs = (wf.max(hf) / n).max(2.0); let sw = soft.max(0.04);
            p.mask = Some(Box::new(move |x, y| { let (bx, by) = ((x as f32 / bs) as i32, (y as f32 / bs) as i32); let th = hash(bx, by, seed); sstep((k * (1.0 + sw) - th) / sw) })); }
        "card_wipe" => { // After Effects' Card Wipe, "Left-Right" rows-and-columns deal preset: the frame is cut into columns that flip/reveal one after another left to right, rather than one smooth edge.
            let n = tr.count.clamp(2.0, 32.0) as f32; let cw = wf / n; let stagger = 0.65; // fraction of the whole transition each column's own reveal takes, so columns overlap like a dealt hand rather than firing all at once
            let sw = soft.max(0.03);
            p.mask = Some(Box::new(move |x, _| { let col = (x as f32 / cw).floor().min(n - 1.0); let start = (col / n) * (1.0 - stagger); let local = ((k - start) / stagger).clamp(0.0, 1.0); sstep((local - 0.5 + sw) / sw.max(0.05)) }));
            p.post = Some(Box::new(move |f| { let (w, h) = (f.w, f.h); let src = f.d.clone(); // a brief horizontal squeeze per column near its own flip moment, standing in for the 3D card turn
                for x in 0..w { let col = (x as f32 / cw).floor().min(n - 1.0); let start = (col / n) * (1.0 - stagger); let local = ((k - start) / stagger).clamp(0.0, 1.0);
                    let squeeze = (1.0 - (local * 2.0 - 1.0).abs()).max(0.0); if squeeze < 0.01 { continue; } let scale = 1.0 - squeeze * 0.7; let cx = (col + 0.5) * cw; let sx = (cx + (x as f32 - cx) / scale.max(0.05)).round() as i32;
                    if sx < 0 || sx >= w as i32 { for y in 0..h { f.d[(y * w + x) * 4 + 3] *= 1.0 - squeeze; } continue; }
                    for y in 0..h { let si = (y * w + sx as usize) * 4; let di = (y * w + x) * 4; for j in 0..4 { f.d[di + j] = src[si + j]; } f.d[di + 3] *= 1.0 - squeeze * 0.4; } } })); }
        "slide_left" => p.dx = (1.0 - k as f64) * w as f64, "slide_right" => p.dx = -(1.0 - k as f64) * w as f64, "slide_up" => p.dy = (1.0 - k as f64) * h as f64, "slide_down" => p.dy = -(1.0 - k as f64) * h as f64,
        "zoom" => { p.op = sstep(k * 1.6); p.scale = 0.6 + 0.4 * k as f64; }
        "zoom_blur" => { p.op = sstep(k * 2.0); p.scale = 1.0 + (1.0 - k as f64) * 0.8 * tr.intensity; let amt = (1.0 - k).powi(2) * 0.35 * tr.intensity as f32; p.post = Some(Box::new(move |f| radial_blur(f, amt, 14))); }
        "spin" => { p.op = sstep(k * 1.5); p.rot = (1.0 - k as f64) * 180.0 * tr.intensity; p.scale = 0.5 + 0.5 * k as f64; }
        "flash" => { p.op = if k < 0.5 { 0.0 } else { 1.0 }; let a = (1.0 - (2.0 * k - 1.0).abs()).powi(2) * tr.intensity as f32; let c = [tr.color[0] as f32, tr.color[1] as f32, tr.color[2] as f32]; let c = if tr.color == [0.0; 3] { [1.0; 3] } else { c };
            p.overlay = Some(Box::new(move |f| for i in 0..f.w * f.h { for j in 0..3 { f.d[i * 4 + j] = f.d[i * 4 + j] * (1.0 - a) + c[j] * a; } })); }
        "dip_black" | "dip_white" | "dip_color" => { let c = match tr.kind.as_str() { "dip_black" => [0.0; 3], "dip_white" => [1.0; 3], _ => [tr.color[0] as f32, tr.color[1] as f32, tr.color[2] as f32] };
            if k < 0.5 { p.op = 0.0; p.dip = Some((c, sstep(k * 2.0))); } else { p.dip = Some((c, 1.0)); p.op = sstep((k - 0.5) * 2.0); } }
        "light_leak" => { p.op = sstep((k - 0.3) / 0.4); let a = (1.0 - (2.0 * k - 1.0).abs()).powf(1.5) * tr.intensity as f32; let (ox, oy) = (hash(1, 2, seed) * 0.6 + 0.2, hash(3, 4, seed) * 0.6 + 0.2);
            p.overlay = Some(Box::new(move |f| { let (w, h) = (f.w as f32, f.h as f32); for y in 0..f.h { for x in 0..f.w { let d = (((x as f32 / w - ox) * 1.2).powi(2) + ((y as f32 / h - oy)).powi(2)).sqrt(); let g = (1.0 - d * 1.1).max(0.0).powi(2) * a; let i = (y * f.w + x) * 4; f.d[i] += g * 1.6; f.d[i + 1] += g * 0.75; f.d[i + 2] += g * 0.25; } } })); }
        "film_burn" => { // a hot film-burn bloom from a seeded point: noise-edged heat spreads out, blows the frame to orange/white at the cut (k 0.5), then burns off
            let it = tr.intensity as f32; let env = (1.0 - (2.0 * k - 1.0).abs()).max(0.0); p.op = sstep((k - 0.42) / 0.16);
            let (ox, oy) = (wf * (0.15 + 0.7 * hash(11, 12, seed)), hf * (0.15 + 0.7 * hash(13, 14, seed))); let maxd = (wf * wf + hf * hf).sqrt();
            let flick = 0.85 + 0.3 * hash((kr * 24.0) as i32, 77, seed); let reach = env * 1.3; let ns = 5.0 / wf.max(hf); let drift = k * 1.7;
            if it > 0.0 && env > 0.0 { p.overlay = Some(Box::new(move |f| { for y in 0..f.h { for x in 0..f.w { let (xf, yf) = (x as f32, y as f32);
                let d = ((xf - ox).powi(2) + (yf - oy).powi(2)).sqrt() / maxd; let n = 0.65 * vnoise(xf * ns + drift, yf * ns, seed) + 0.35 * vnoise(xf * ns * 2.7, yf * ns * 2.7 - drift, seed ^ 0x5bd1);
                let h = sstep((reach - d + (n - 0.5) * 0.45) / 0.22) * env.powf(0.3) * it * flick; if h <= 0.0 { continue; }
                let core = ((h - 0.55) / 0.45).max(0.0); let i = (y * f.w + x) * 4;
                f.d[i] += h * 1.5 + core * 0.6; f.d[i + 1] += h * h * 0.75 + core * 0.8; f.d[i + 2] += h * h * h * 0.25 + core * 0.9; } } })); } }
        "glitch" => { let bands = 18usize; let sev = (1.0 - k) * tr.intensity as f32; let bh = (hf / bands as f32).ceil().max(1.0); let step = (kr * 14.0) as u32;
            p.mask = Some(Box::new(move |_, y| { let b = (y as f32 / bh) as i32; if hash(b, step as i32, seed) < k * 1.4 - 0.1 { 1.0 } else { 0.0 } }));
            p.post = Some(Box::new(move |f| { let (w, h) = (f.w, f.h); let src = f.d.clone(); for y in 0..h { let b = (y as f32 / bh) as i32; let off = ((hash(b, step as i32 + 5, seed) - 0.5) * 2.0 * sev * w as f32 * 0.25) as i32; let rs = (sev * 14.0) as i32;
                for x in 0..w { let o = (y * w + x) * 4; for (j, d) in [(0usize, -rs), (1, 0), (2, rs)] { let sx = (x as i32 + off + d).rem_euclid(w as i32) as usize; f.d[o + j] = src[(y * w + sx) * 4 + j]; } } } })); }
        "shatter" => { let (uu, sd, dn, it) = (kr as f32, seed, (tr.count / 9.0).max(0.4) as f32, tr.intensity as f32); p.compose = Some(Box::new(move |below, inc| crate::glass::compose(below, inc, uu, sd, dn, it))); }
        "shatter_cells" => { // incoming frame revealed through Voronoi shards, shockwave from an impact point; cracks glow ahead of the wave
            let cs = (wf / tr.count.clamp(3.0, 40.0) as f32).max(8.0); let (ix, iy) = (wf * (0.3 + 0.4 * hash(5, 6, seed)), hf * (0.3 + 0.4 * hash(7, 8, seed))); let maxd = (wf * wf + hf * hf).sqrt();
            let lag = move |cx: f32, cy: f32, hs: f32| -> f32 { (((cx - ix).powi(2) + (cy - iy).powi(2)).sqrt() / maxd) * 0.55 + hs * 0.2 };
            p.mask = Some(Box::new(move |x, y| { let (hs, _d1, _d2, cx, cy) = voro(x as f32, y as f32, cs, seed); sstep((k - 0.18 - lag(cx, cy, hs)) / 0.12) }));
            let it = tr.intensity as f32;
            p.overlay = Some(Box::new(move |f| { for y in 0..f.h { for x in 0..f.w { let (hs, d1, d2, cx, cy) = voro(x as f32, y as f32, cs, seed); let edge = d2 - d1; let lg = lag(cx, cy, hs);
                let crack = (1.0 - edge / (1.0 + cs * 0.018)).clamp(0.0, 1.0).powi(2); let born = sstep((k - 0.02 - lg * 0.9) / 0.06); let gone = 1.0 - sstep((k - 0.3 - lg) / 0.2);
                let glow = (1.0 - edge / (cs * 0.12)).clamp(0.0, 1.0).powi(2) * 0.25; let a = (crack + glow) * born * gone * it; if a <= 0.0 { continue; } let i = (y * f.w + x) * 4; f.d[i] += a * 1.0; f.d[i + 1] += a * 0.97; f.d[i + 2] += a * 0.92; } } }));
            let kk = k as f64; p.scale = 1.0 + 0.04 * (1.0 - kk); }
        "whip_left" | "whip_right" => {
            // V8.5.7: a pro whip pan is short and strong, concentrated on only the middle
            // frames, with the outgoing AND incoming shots staying sharp right up to the peak.
            // The old shape here (plain envelope 4k(1-k), one box blur up to ~0.35*frame-width
            // wide on only 16 taps, plus a second `overlay` blur that render.rs re-applies to
            // the whole ALREADY COMPOSITED frame -- incoming shot included) held visible blur
            // over most of the transition and read as ghosting/mush rather than a streak (16
            // taps spread across ~450px at full intensity is ~30px between samples, i.e.
            // distinct echoes, not a blur). Fixed: the envelope is raised to a power so it only
            // really fires near k=0.5; the peak width is tightened; the tap count scales with
            // the blur width so samples never land more than ~3px apart (no more combing); and
            // there is no longer any `overlay` pass -- only the outgoing clip's own frame
            // (`post`) is ever touched, so the incoming shot is never blurred by this transition.
            //
            // V8.5.7 follow-up: that first fix only ever touched `post`, which render.rs runs
            // on the INCOMING clip's own frame before it is blended on top -- it never touches
            // `out`, the already-composited backdrop the incoming clip is drawn over (the
            // outgoing talking-head layer, still playing underneath on its own track/time).
            // So the outgoing layer stayed perfectly sharp and static for the whole transition
            // while only the incoming b-roll ever got any blur: a cheap slide/wipe, not a
            // whip-pan. A real whip blurs BOTH layers along the motion direction with the SAME
            // envelope, peaking together at the midpoint. `under` is the hook render.rs runs on
            // `out` right before this clip is blended on top (it is how push/gradient_wipe move
            // the outgoing picture already); using the identical vel/n here gives the backdrop
            // the matching streak -- sharp at k=0, streaked near the middle, sharp again well
            // before k=1 -- with no extra taps and no change to the incoming side at all. With
            // no abutting same-track outgoing clip (the normal auto_edit b-roll-over-talking-
            // head case), `prev` is None and the blur runs on `out` in place; with one (a push-
            // style abutting cut), `prev` is that clip's own frame, blurred the same way and
            // drawn back over `out` unshifted (its position is already correct).
            let s = if tr.kind == "whip_left" { 1.0 } else { -1.0 };
            p.dx = s * (1.0 - k as f64) * w as f64;
            let base = ((1.0 - k) * 4.0 * k).clamp(0.0, 1.0);
            let env = base.powf(2.4);
            let vel = env * wf * 0.22 * tr.intensity as f32;
            let n = ((vel.abs() / 3.0).ceil() as usize).clamp(16, 64);
            p.post = Some(Box::new(move |f| dir_blur(f, vel, 0.0, n)));
            p.under = Some(Box::new(move |out, prev| match prev {
                Some(a) => { let mut bf = Frame { w: a.w, h: a.h, d: a.d.clone() }; dir_blur(&mut bf, vel, 0.0, n); crate::pushwipe::over_shifted(out, &bf, 0.0, 0.0); }
                None => dir_blur(out, vel, 0.0, n),
            }));
            p.op = 1.0;
        }
        "ripple_dissolve" => { // concentric rings expanding from centre, a sine-perturbed radial threshold (same
            // sstep((k*(1+sw)-n)/sw) shape as noise_dissolve, n clamped to 0..1 so k=0/1 are always pure)
            let (cx, cy) = (wf / 2.0, hf / 2.0); let maxd = (cx * cx + cy * cy).sqrt().max(1.0);
            let freq = tr.count.max(1.0) as f32 * 2.0; let sw = soft.max(0.06);
            p.mask = Some(Box::new(move |x, y| { let (dx, dy) = (x as f32 - cx, y as f32 - cy); let d = (dx * dx + dy * dy).sqrt() / maxd;
                let n = (d + 0.05 * (d * freq * std::f32::consts::TAU).sin()).clamp(0.0, 1.0); sstep((k * (1.0 + sw) - n) / sw) }));
        }
        "barn_doors" => { // the outgoing picture's two halves physically slide apart (like double doors opening) instead of
            // being wiped away; a true geometric slide (like push), not a reveal mask, so it runs in `compose` on raw pixels.
            let half = w as isize / 2; let shift = (k * (w as f32 / 2.0 + 2.0)) as isize;
            p.compose = Some(Box::new(move |below, inc| { let (bw, bh) = (below.w, below.h); let mut out = Frame { w: inc.w, h: inc.h, d: inc.d.clone() };
                for y in 0..bh { for x in 0..bw { let xi = x as isize;
                    let src_x = if xi < half - shift { Some(xi + shift) } else if xi >= half + shift { Some(xi - shift) } else { None };
                    if let Some(sx) = src_x { if sx >= 0 && (sx as usize) < bw { let so = (y * bw + sx as usize) * 4; let a = below.d[so + 3];
                        if a > 0.0 { let doo = (y * bw + x) * 4;
                            for j in 0..3 { out.d[doo + j] = below.d[so + j] * a + out.d[doo + j] * out.d[doo + 3] * (1.0 - a); }
                            out.d[doo + 3] = a + out.d[doo + 3] * (1.0 - a); } } } } }
                out })); }
        "page_peel" => { // a simplified 2D corner peel (diagonal reveal from a corner with a curl-shadow band along the
            // lifting edge and a bright highlight just behind it) -- an honest stand-in for a true 3D page curl, which
            // this pass does not attempt; cheap, but reads as a page lifting rather than a plain diagonal wipe.
            let ang = if tr.angle != 0.0 { tr.angle as f32 } else { 135.0 }; let (c, sn) = (ang.to_radians().cos(), ang.to_radians().sin());
            let half = (wf * c.abs() + hf * sn.abs()) / 2.0; let sw = (soft * half * 2.0).max(1.0); let edge = -half + k * (2.0 * half + sw);
            p.mask = Some(Box::new(move |x, y| { let pr = (x as f32 - wf / 2.0) * c + (y as f32 - hf / 2.0) * sn; sstep((edge - pr) / sw) }));
            let it = tr.intensity as f32;
            p.overlay = Some(Box::new(move |f| { for y in 0..f.h { for x in 0..f.w { let pr = (x as f32 - wf / 2.0) * c + (y as f32 - hf / 2.0) * sn;
                let d = edge - pr; // distance behind the lifting edge, on the still-covered (outgoing) side
                if d < 0.0 || d > sw * 6.0 { continue; }
                let band = d / (sw * 6.0); let shadow = (1.0 - band).powi(2) * 0.5 * it; let hl = (1.0 - (band - 0.12).abs() * 6.0).clamp(0.0, 1.0) * 0.35 * it;
                let i = (y * f.w + x) * 4; let a = f.d[i + 3]; if a <= 0.0 { continue; }
                for j in 0..3 { f.d[i + j] = (f.d[i + j] * (1.0 - shadow) + hl).clamp(0.0, 1.0); } } } })); }
        _ => p.op = k,
    }
    p
}

/// morph_cut's outgoing shot: the media clip on `c`'s track that ends where `c` starts (within 1.5 frames, no overlap).
pub fn morph_prev<'a>(p: &'a crate::model::Project, c: &crate::model::Clip) -> Option<&'a crate::model::Clip> {
    let tol = 1.5 / p.fps.max(1.0);
    p.tracks.iter().find(|t| t.clips.iter().any(|x| x.id == c.id))?.clips.iter().map(|x| &**x)
        .filter(|x| x.id != c.id && matches!(x.source, crate::model::Source::Media { .. }) && x.start + x.duration <= c.start + 1e-6 && c.start - (x.start + x.duration) < tol)
        .max_by(|a, b| (a.start + a.duration).total_cmp(&(b.start + b.duration)))
}
