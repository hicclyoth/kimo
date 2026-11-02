//! Text engine: kerned layout with word wrap, per-unit (char/word/line) animators with range selectors (AE-style),
//! stroke, blurred shadow, glow, gradient fill and rounded background box.
use crate::effects::{box_blur3, Frame};
use crate::model::{TextAnimator, TextFx};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static FONTS: OnceLock<Mutex<HashMap<String, std::sync::Arc<FontVec>>>> = OnceLock::new();
/// 8.5.4: names of fonts we have already warned about falling back for -- `resolve_font_info` is called
/// on every glyph/frame touching a text clip, so without dedup a missing font would flood stderr with
/// the same line thousands of times over a render. Keyed by the requested name so a SECOND distinct
/// missing font still gets its own warning.
static WARNED_FONTS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
/// Returns the warning message the FIRST time `name` falls back to a substitute, None every time after
/// (for that same name) -- the dedup is a pure, testable decision separate from the eprintln side effect,
/// so a test can assert "exactly once" without capturing the process's real stderr.
fn font_fallback_warning(name: &str) -> Option<String> {
    let set = WARNED_FONTS.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut g = set.lock().unwrap();
    if g.insert(name.to_string()) {
        Some(format!("kimo: font '{name}' was not found, so text using it is drawn with a fallback system font -- set KIMO_FONTS to the folder holding the preset fonts, or run `bash fonts/fetch.sh` in the repo to fetch them"))
    } else {
        None
    }
}
fn norm(s: &str) -> String { let mut n: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect(); for suf in ["variablefont", "var", "regular"] { if n.ends_with(suf) && n.len() > suf.len() { n.truncate(n.len() - suf.len()); } } n }
pub fn font_dirs() -> Vec<std::path::PathBuf> { let mut v = vec![]; if let Ok(d) = std::env::var("KIMO_FONTS") { v.push(d.into()); }
    if let Ok(e) = std::env::current_exe() { let mut p = e.clone(); for _ in 0..4 { p.pop(); v.push(p.join("fonts")); } }
    v.push("fonts".into()); if let Ok(h) = std::env::var("HOME") { v.push(std::path::Path::new(&h).join("kimo/fonts")); } v.push("/usr/share/fonts".into()); v }
fn scan(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, depth: usize) { if depth > 4 { return; } if let Ok(rd) = std::fs::read_dir(dir) { for e in rd.flatten() { let p = e.path(); if p.is_dir() { scan(&p, out, depth + 1); } else if matches!(p.extension().and_then(|x| x.to_str()), Some("ttf") | Some("otf") | Some("TTF") | Some("OTF")) { out.push(p); } } } }
pub fn all_fonts() -> Vec<(String, std::path::PathBuf)> { let mut files = vec![]; for d in font_dirs() { scan(&d, &mut files, 0); } let mut seen = std::collections::HashSet::new(); let mut v = vec![];
    for p in files { let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(); let n = norm(&stem); if seen.insert(n.clone()) { v.push((stem, p)); } } v.sort_by(|a, b| a.0.cmp(&b.0)); v }
/// (path, substituted): substituted = the requested family was not found and a fallback was used.
pub fn resolve_font_info(name: &str) -> (String, bool) { if std::path::Path::new(name).is_file() { return (name.into(), false); } if crate::deps::font_is_path(name) { let r = crate::deps::resolve_str(name); if std::path::Path::new(&r).is_file() { return (r, false); } } let want = norm(name); let fonts = all_fonts();
    if let Some((_, p)) = fonts.iter().find(|(s, _)| norm(s) == want) { return (p.to_string_lossy().into(), false); }
    if let Some((_, p)) = fonts.iter().find(|(s, _)| norm(s).starts_with(&want)) { return (p.to_string_lossy().into(), false); }
    // 8.5.4: the bundled preset fonts (KIMO_FONTS) are missing, so we're about to fall back to Montserrat-if-
    // present or DejaVu/system -- this silently changed the whole look of every title using `name` with no
    // signal anywhere except lint_edit's own font_substituted check, which only runs if someone calls lint_edit.
    // One clear stderr line per distinct missing font name says so at the moment it actually happens.
    if let Some(msg) = font_fallback_warning(name) { eprintln!("{msg}"); }
    if let Some((_, p)) = fonts.iter().find(|(s, _)| norm(s) == "montserrat") { return (p.to_string_lossy().into(), true); }
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(), true) }
pub fn resolve_font(name: &str) -> String { resolve_font_info(name).0 }

fn font(name: &str, weight: f64) -> Result<std::sync::Arc<FontVec>, String> {
    let key = format!("{name}@{weight}"); let m = FONTS.get_or_init(|| Mutex::new(HashMap::new())); let mut g = m.lock().unwrap();
    if let Some(f) = g.get(&key) { return Ok(f.clone()); }
    let path = resolve_font(name); let data = std::fs::read(&path).map_err(|e| format!("font {path}: {e}"))?;
    let mut f = FontVec::try_from_vec(data).map_err(|e| format!("font parse {path}: {e}"))?;
    { use ab_glyph::VariableFont; f.set_variation(b"wght", if weight > 0.0 { weight as f32 } else { 700.0 }); }
    let f = std::sync::Arc::new(f); g.insert(key, f.clone()); Ok(f)
}

/// Characters of `text` the resolved font has no glyph for (they are skipped when drawing, never shown as a box).
pub fn missing_glyphs(text: &str, font_name: &str, weight: f64) -> Vec<char> {
    let Ok(f) = font(font_name, weight) else { return vec![] }; let mut v: Vec<char> = vec![];
    for ch in text.chars() { if !ch.is_whitespace() && f.glyph_id(ch).0 == 0 && !v.contains(&ch) { v.push(ch); } } v
}

struct G { c: char, x: f32, y: f32, line: usize, word: usize, adv: f32 }

/// Layout glyphs: kerning, tracking, word wrap to box_width (0 = no wrap), alignment, line spacing. Positions are relative to the text block origin (top-left of block, y = baseline).
fn layout(text: &str, f: &FontVec, size: f32, spacing: f32, align: &str, cw: f32, fx: Option<&TextFx>) -> (Vec<G>, f32, f32, usize, usize, usize) {
    let sf = f.as_scaled(PxScale::from(size)); let lh = sf.height() * fx.map(|x| x.line_spacing as f32).unwrap_or(1.15); let kern = fx.map(|x| x.kerning).unwrap_or(true);
    let maxw = fx.map(|x| x.box_width as f32).filter(|w| *w > 0.0);
    // split into logical lines, then wrap words
    let mut lines: Vec<Vec<(char, f32)>> = vec![]; // per line: (char, advance incl. kerning+spacing)
    for para in text.split('\n') { let mut cur: Vec<(char, f32)> = vec![]; let mut curw = 0f32; let mut prev: Option<char> = None; let words: Vec<&str> = para.split(' ').collect();
        for (wi, wd) in words.iter().enumerate() { let mut chars: Vec<(char, f32)> = vec![]; let mut p2 = prev; if wi > 0 { chars.push((' ', sf.h_advance(f.glyph_id(' ')) + spacing)); p2 = Some(' '); }
            for ch in wd.chars() { let gid = f.glyph_id(ch); if gid.0 == 0 && !ch.is_whitespace() { continue; } let k = if kern { p2.map(|p| sf.kern(f.glyph_id(p), gid)).unwrap_or(0.0) } else { 0.0 }; chars.push((ch, sf.h_advance(gid) + spacing + k)); p2 = Some(ch); }
            let ww: f32 = chars.iter().map(|c| c.1).sum();
            if let Some(mw) = maxw { if curw + ww > mw && !cur.is_empty() { if chars.first().map(|c| c.0 == ' ').unwrap_or(false) { chars.remove(0); } let ww2: f32 = chars.iter().map(|c| c.1).sum(); lines.push(std::mem::take(&mut cur)); cur = chars; curw = ww2; prev = p2; continue; } }
            curw += ww; cur.extend(chars); prev = p2; }
        lines.push(cur); }
    let widths: Vec<f32> = lines.iter().map(|l| l.iter().map(|c| c.1).sum::<f32>() - spacing.max(0.0).min(l.last().map(|c| c.1).unwrap_or(0.0))).collect();
    let bw = maxw.unwrap_or_else(|| widths.iter().cloned().fold(0.0, f32::max)); let mut out = vec![]; let mut word = 0usize; let mut nchars = 0usize;
    for (li, l) in lines.iter().enumerate() { let mut x = match align { "left" => 0.0, "right" => bw - widths[li], _ => (bw - widths[li]) / 2.0 }; let y = sf.ascent() + li as f32 * lh;
        for (c, adv) in l { if *c == ' ' { word += 1; x += adv; nchars += 1; continue; } out.push(G { c: *c, x, y, line: li, word, adv: *adv }); x += adv; nchars += 1; } word += 1; }
    let nwords = out.iter().map(|g| g.word).max().map(|m| m + 1).unwrap_or(0); let _ = (cw, nchars);
    (out, bw, lh * lines.len() as f32, lines.len(), nwords, 0)
}

fn shape_amount(shape: &str, x: f32, smooth: f32) -> f32 {
    // x: position of the unit relative to the selector window (0..1 inside)
    let soft = |v: f32| { if smooth <= 1e-4 { if (0.0..=1.0).contains(&v) { 1.0 } else { 0.0 } } else { let a = ((v + smooth) / smooth).clamp(0.0, 1.0); let b = ((1.0 - v + smooth) / smooth).clamp(0.0, 1.0); let (a, b) = (a * a * (3.0 - 2.0 * a), b * b * (3.0 - 2.0 * b)); a.min(b) } };
    match shape { "ramp_up" => if (0.0..=1.0).contains(&x) { x } else { 0.0 }, "ramp_down" => if (0.0..=1.0).contains(&x) { 1.0 - x } else { 0.0 },
        "triangle" => if (0.0..=1.0).contains(&x) { 1.0 - (2.0 * x - 1.0).abs() } else { 0.0 }, "round" => if (0.0..=1.0).contains(&x) { (1.0 - (2.0 * x - 1.0).powi(2)).max(0.0).sqrt() } else { 0.0 },
        "smooth" => if (0.0..=1.0).contains(&x) { let t = 1.0 - (2.0 * x - 1.0).abs(); t * t * (3.0 - 2.0 * t) } else { 0.0 }, _ => soft(x) }
}
/// Wiggly Selector value for unit `unit` at clip time `t`: smooth value noise (smoothstep between hashed knots, one knot per 1/freq s) mixed with
/// a shared stream by `correlation`, mapped from -1..1 onto min..max. Knots sit in -1..1, so the value never leaves min..max.
fn wiggle(w: &crate::model::Wiggly, unit: usize, t: f64) -> f32 {
    let knot = |stream: u64, k: i64| -> f64 { let mut x = (k as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ stream.wrapping_mul(0xD6E8FEB86659FD93) ^ w.seed.wrapping_mul(0xBF58476D1CE4E5B9).wrapping_add(0x632BE59BD9B4E019);
        x ^= x >> 30; x = x.wrapping_mul(0xBF58476D1CE4E5B9); x ^= x >> 27; x = x.wrapping_mul(0x94D049BB133111EB); x ^= x >> 31; (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0 };
    let noise = |stream: u64| -> f64 { let x = t * w.freq.max(0.0) + (stream as f64) * 0.618_034; let k = x.floor(); let f = x - k; let s = f * f * (3.0 - 2.0 * f); let (a, b) = (knot(stream, k as i64), knot(stream, k as i64 + 1)); a + (b - a) * s };
    let c = w.correlation.clamp(0.0, 1.0); let v = (noise(0) * c + noise(unit as u64 + 1) * (1.0 - c)).clamp(-1.0, 1.0);
    let (lo, hi) = (w.min.at(t), w.max.at(t)); (lo + (hi - lo) * (v + 1.0) * 0.5) as f32
}
#[derive(Clone, Copy)]
/// fs / ft: Fill Color animators composed as an affine pair, unit colour = base * fs + ft (identity 1 / 0 when no animator fills).
struct U { x: f32, y: f32, sc: f32, rot: f32, op: f32, track: f32, blur: f32, fs: f32, ft: [f32; 3] }
fn animate(an: &[TextAnimator], t: f64, g: &G, nchar: usize, nword: usize, nline: usize, ci: usize) -> U {
    let mut u = U { x: 0.0, y: 0.0, sc: 1.0, rot: 0.0, op: 1.0, track: 0.0, blur: 0.0, fs: 1.0, ft: [0.0; 3] };
    for a in an { let (n, i) = match a.unit.as_str() { "word" => (nword, g.word), "line" => (nline, g.line), _ => (nchar, ci) }; let unit = i; let i = if a.random { let hh = |k: usize| { let mut x = (k as u64).wrapping_add(a.seed.wrapping_mul(0x9E3779B97F4A7C15)).wrapping_mul(0xBF58476D1CE4E5B9); x ^= x >> 29; x = x.wrapping_mul(0x94D049BB133111EB); x ^ (x >> 32) }; let hi = hh(i); (0..n).filter(|&k| { let h = hh(k); h < hi || (h == hi && k < i) }).count() } else { i }; let pos = (i as f32 + 0.5) / n.max(1) as f32;
        let (s0, e0, off, sm) = (a.start.at(t) as f32, a.end.at(t) as f32, a.offset.at(t) as f32, a.smoothness.at(t) as f32); let (s, e) = (s0 + off, e0 + off); let x = (pos - s) / (e - s).max(1e-4); let mut k = shape_amount(&a.shape, x, sm); if let Some(e) = a.ease { k = e.apply(k as f64) as f32; }
        if k <= 0.0 { continue; }
        if let Some(wg) = &a.wiggly { k *= wiggle(wg, unit, t); if k == 0.0 { continue; } }
        if let Some(fc) = a.fill { let m = (k * a.fill_amount.as_ref().map_or(1.0, |p| p.at(t)) as f32).clamp(0.0, 1.0); u.fs *= 1.0 - m; for j in 0..3 { u.ft[j] = u.ft[j] * (1.0 - m) + fc[j] as f32 * m; } } u.x += a.x.at(t) as f32 * k; u.y += a.y.at(t) as f32 * k; u.sc *= 1.0 + (a.scale.at(t) as f32 - 1.0) * k; u.rot += a.rotation.at(t) as f32 * k; u.op *= 1.0 + (a.opacity.at(t) as f32 - 1.0) * k; u.track += a.tracking.at(t) as f32 * k; u.blur += a.blur.at(t) as f32 * k; }
    u
}

/// Rasterize all glyphs (with animator transforms) into a coverage mask; also returns the block bounds in canvas px.
/// Laid-out size (width, height) in canvas pixels of `text` for a font name or path; same layout the renderer uses.
pub fn measure(text: &str, font_name: &str, size: f64, spacing: f64, weight: f64, align: &str, cw: f64) -> Result<(f64, f64), String> { let m = measure_text(text, font_name, size, spacing, weight, align, cw, 0.0, None)?; Ok((m.width, m.height)) }
/// Where the laid-out block's top-left sits on a canvas: the renderer centres the block (RV-28 shares this with the measuring tool).
fn block_origin(cw: f32, ch: f32, bw: f32, bh: f32) -> (f32, f32) { ((cw - bw) / 2.0, (ch - bh) / 2.0) }
/// Everything a layout query needs, from the same `layout` and block origin the renderer uses (no animators; static pose).
pub struct LineMetrics { pub text: String, pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub baseline: f64 }
pub struct TextMetrics { pub width: f64, pub height: f64, pub lines: Vec<LineMetrics>, pub origin: (f64, f64), pub ink: Option<[f64; 4]>, pub font_file: String, pub substituted: bool }
/// `ch` = canvas height used to centre the block vertically (0 = height of the block, i.e. origin y 0).
pub fn measure_text(text: &str, font_name: &str, size: f64, spacing: f64, weight: f64, align: &str, cw: f64, ch: f64, fx: Option<&TextFx>) -> Result<TextMetrics, String> {
    let f = font(font_name, weight)?; let upper; let text = if fx.map(|x| x.uppercase).unwrap_or(false) { upper = text.to_uppercase(); upper.as_str() } else { text };
    let (glyphs, bw, bh, nl, _, _) = layout(text, &f, size as f32, spacing as f32, align, cw as f32, fx); let sf = f.as_scaled(PxScale::from(size as f32));
    let (bx, by) = block_origin(cw as f32, if ch > 0.0 { ch as f32 } else { bh }, bw, bh);
    let lh = bh / nl.max(1) as f32; let mut wpl: Vec<Vec<String>> = vec![vec![]; nl]; let mut ext: Vec<Option<(f32, f32)>> = vec![None; nl]; let mut last = (usize::MAX, usize::MAX);
    let mut ink: Option<[f32; 4]> = None;
    for g in &glyphs {
        if (g.line, g.word) != last { wpl[g.line].push(String::new()); last = (g.line, g.word); } wpl[g.line].last_mut().unwrap().push(g.c);
        let (x0, x1) = (bx + g.x, bx + g.x + g.adv); ext[g.line] = Some(match ext[g.line] { None => (x0, x1), Some((a, b)) => (a.min(x0), b.max(x1)) });
        if let Some(o) = f.outline_glyph(f.glyph_id(g.c).with_scale_and_position(PxScale::from(size as f32), ab_glyph::point(bx + g.x, by + g.y))) { let b = o.px_bounds(); let r = [b.min.x, b.min.y, b.max.x, b.max.y]; ink = Some(match ink { None => r, Some(i) => [i[0].min(r[0]), i[1].min(r[1]), i[2].max(r[2]), i[3].max(r[3])] }); }
    }
    let lines = (0..nl).map(|li| { let (x0, x1) = ext[li].unwrap_or((bx + bw / 2.0, bx + bw / 2.0)); LineMetrics { text: wpl[li].join(" "), x: x0 as f64, y: (by + li as f32 * lh) as f64, w: (x1 - x0) as f64, h: lh as f64, baseline: (by + sf.ascent() + li as f32 * lh) as f64 } }).collect();
    let (font_file, substituted) = resolve_font_info(font_name);
    Ok(TextMetrics { width: bw as f64, height: bh as f64, lines, origin: (bx as f64, by as f64), ink: ink.map(|i| [i[0] as f64, i[1] as f64, i[2] as f64, i[3] as f64]), font_file, substituted })
}
fn coverage(cw: usize, ch: usize, text: &str, f: &FontVec, size: f32, spacing: f32, align: &str, reveal: f64, fx: Option<&TextFx>, t: f64) -> (Vec<f32>, [f32; 4], Option<Vec<[f32; 4]>>) {
    let (glyphs, bw, bh, nl, nwords, _) = layout(text, f, size, spacing, align, cw as f32, fx); let sf = f.as_scaled(PxScale::from(size));
    let nchar = glyphs.len(); let shown = (reveal.clamp(0.0, 1.0) * nchar as f64).ceil() as usize;
    let (bx, by) = block_origin(cw as f32, ch as f32, bw, bh); let mut cov = vec![0f32; cw * ch];
    // The returned bounds feed the background-box style (render_text's "bg"): with a wrap width set (box_width,
    // for a caption/title that might run long), bw above is the WRAP LIMIT, not how wide this particular bit
    // of text actually is -- a two-word caption pill was drawing at the full configured wrap width every time,
    // regardless of how short the words were. ink_x0/ink_x1 is the tight horizontal extent of the glyphs as
    // laid out (pre-animation: a per-word pop/karaoke offset shouldn't make the box itself jitter), so the
    // background this function reports can hug the real content instead of the wrap box.
    let (mut ink_x0, mut ink_x1) = (f32::MAX, f32::MIN);
    for g in &glyphs { let (x0, x1) = (bx + g.x, bx + g.x + g.adv); ink_x0 = ink_x0.min(x0); ink_x1 = ink_x1.max(x1); }
    let (ink_bx, ink_bw) = if ink_x1 > ink_x0 { (ink_x0, ink_x1 - ink_x0) } else { (bx, bw) };
    let empty: Vec<TextAnimator> = vec![]; let an = fx.map(|x| &x.animators).unwrap_or(&empty);
    // per-pixel fill pair (ft.r, ft.g, ft.b, fs); allocated only when some animator has a Fill Color, identity elsewhere
    let mut tint: Option<Vec<[f32; 4]>> = if an.iter().any(|a| a.fill.is_some()) { Some(vec![[0.0, 0.0, 0.0, 1.0]; cw * ch]) } else { None };
    let plen = fx.and_then(|x| x.path.as_ref()).map(|pp| { let mut l = 0f64; let mut pv = crate::motion::sample(pp, 0.0, cw, ch); for k in 1..=200 { let c = crate::motion::sample(pp, k as f64 / 200.0, cw, ch); l += (c.0 - pv.0).hypot(c.1 - pv.1); pv = c; } l as f32 }).unwrap_or(1.0);
    let mut track_acc = vec![0f32; nl + 1]; let mut last_line = usize::MAX;
    let mut line_tr = vec![0f32; nl + 1]; for (ci, g) in glyphs.iter().enumerate().take(shown) { line_tr[g.line] += animate(an, t, g, nchar, nwords, nl, ci).track; }
    for (ci, g) in glyphs.iter().enumerate() { if ci >= shown { break; }
        let mut u = animate(an, t, g, nchar, nwords, nl, ci); if g.line != last_line { track_acc[g.line] = 0.0; last_line = g.line; } let tr = track_acc[g.line]; track_acc[g.line] += u.track;
        if u.op <= 0.0 { continue; }
        let pd = |tt: f64| -> (f32, f32, f32) { let Some(pp) = fx.and_then(|x| x.path.as_ref()) else { return (0.0, 0.0, 0.0) }; let gx0 = bx + g.x + tr; let sp = (gx0 + g.adv * 0.5 - bx) / plen.max(1.0); let s0 = fx.map(|x| x.path_start.at(tt) as f32).unwrap_or(0.0); let uu = (s0 + if fx.map(|x| x.path_flip).unwrap_or(false) { -sp } else { sp }).clamp(0.0, 1.0) as f64;
            let (dx, dy, ang) = crate::motion::sample(pp, uu, cw, ch); let (ox0, oy0) = (bx + g.x + tr, by + g.y); let (gcx0, gcy0) = (ox0 + g.adv * 0.5, oy0 - sf.ascent() * 0.35); (cw as f32 / 2.0 + dx as f32 - gcx0, ch as f32 / 2.0 + dy as f32 - gcy0, ang as f32) };
        let p1 = pd(t); u.x += p1.0; u.y += p1.1; u.rot += p1.2;
        let gid = f.glyph_id(g.c); let shift = match align { "left" => 0.0, "right" => -line_tr[g.line], _ => -line_tr[g.line] * 0.5 }; let (ox, oy) = (bx + g.x + tr + shift, by + g.y);
        let u1 = u; let mut u0 = animate(an, t - 0.5 / 30.0, g, nchar, nwords, nl, ci); { let p0 = pd(t - 0.5 / 30.0); u0.x += p0.0; u0.y += p0.1; u0.rot += p0.2; } let dist = ((u1.x - u0.x).powi(2) + (u1.y - u0.y).powi(2)).sqrt() + (u1.sc - u0.sc).abs() * size * 0.5 + (u1.rot - u0.rot).abs() * 0.3; let ns = if dist > 0.8 { ((dist / 1.5).ceil() as usize).clamp(2, 10) } else { 1 }; let wgt = 1.0 / ns as f32;
        for si in 0..ns { let ff = if ns > 1 { (si as f32 + 0.5) / ns as f32 } else { 1.0 }; let u = U { x: u0.x + (u1.x - u0.x) * ff, y: u0.y + (u1.y - u0.y) * ff, sc: u0.sc + (u1.sc - u0.sc) * ff, rot: u0.rot + (u1.rot - u0.rot) * ff, op: u1.op, track: u1.track, blur: u1.blur, fs: u1.fs, ft: u1.ft };
        let (gcx, gcy) = (ox + g.adv * 0.5, oy - sf.ascent() * 0.35);
        let band = if fx.map(|x| x.line_mask).unwrap_or(false) { let lh = sf.height() * fx.map(|x| x.line_spacing as f32).unwrap_or(1.15); Some((by + g.line as f32 * lh - lh * 0.02, by + (g.line as f32 + 1.0) * lh + lh * 0.02)) } else { None };
        // tint (only when an animator fills): the pixel takes this unit's fill pair where its coverage wins (max path) or last (accumulate path)
        let put = |cov: &mut Vec<f32>, tint: &mut Option<Vec<[f32; 4]>>, xx: i32, yy: i32, v: f32| { let wgt = wgt; if xx < 0 || yy < 0 || xx >= cw as i32 || yy >= ch as i32 { return; } if let Some((y0, y1)) = band { let yy = yy as f32 + 0.5; if yy < y0 || yy > y1 { return; } } let i = yy as usize * cw + xx as usize; let a = (v * u.op).min(1.0);
            if let Some(tb) = tint.as_mut() { if ns > 1 || a >= cov[i] { tb[i] = [u.ft[0], u.ft[1], u.ft[2], u.fs]; } }
            if ns > 1 { cov[i] = (cov[i] + a * wgt).min(1.0); } else if a > cov[i] { cov[i] = a; } };
        if u.rot.abs() < 0.01 {
            let s = size * u.sc.max(0.01); let (px, py) = (gcx + u.x - (gcx - ox) * u.sc + 0.0, gcy + u.y + (oy - gcy) * u.sc);
            let sk = fx.map(|x| x.italic_skew as f32).unwrap_or(0.0);
            let glyph = gid.with_scale_and_position(PxScale::from(s), ab_glyph::point(px, py));
            if let Some(o) = f.outline_glyph(glyph) { let b = o.px_bounds(); o.draw(|x, y, v| { let xx = b.min.x as i32 + x as i32; let yy = b.min.y as i32 + y as i32; let sh = (sk * (py - yy as f32)) as i32; put(&mut cov, &mut tint, xx + sh, yy, v); }); }
        } else {
            let glyph = gid.with_scale_and_position(PxScale::from(size), ab_glyph::point(0.0, 0.0));
            let Some(o) = f.outline_glyph(glyph) else { continue }; let b = o.px_bounds(); let (gw, gh) = ((b.max.x - b.min.x).ceil() as usize + 2, (b.max.y - b.min.y).ceil() as usize + 2); let mut loc = vec![0f32; gw * gh];
            o.draw(|px, py, v| { let i = (py as usize) * gw + px as usize; if i < loc.len() { loc[i] = v; } });
            let (s_, c_) = (u.rot.to_radians().sin(), u.rot.to_radians().cos()); let inv = 1.0 / u.sc.max(1e-3); let (lx0, ly0) = (b.min.x, b.min.y);
            let rad = (gw.max(gh) as f32 * u.sc.max(1.0) * 1.5 + 4.0) as i32;
            let (px0, py0) = ((gcx + u.x) as i32 - rad - (g.adv as i32), (gcy + u.y) as i32 - rad - size as i32); let (px1, py1) = ((gcx + u.x) as i32 + rad + g.adv as i32, (gcy + u.y) as i32 + rad + size as i32);
            for yy in py0.max(0)..py1.min(ch as i32) { for xx in px0.max(0)..px1.min(cw as i32) {
                let (dx, dy) = (xx as f32 + 0.5 - gcx - u.x, yy as f32 + 0.5 - gcy - u.y); let (rx, ry) = ((c_ * dx + s_ * dy) * inv, (-s_ * dx + c_ * dy) * inv);
                let (sx, sy) = (rx + gcx - ox - lx0 - 0.5, ry + gcy - oy - ly0 - 0.5); if sx < 0.0 || sy < 0.0 || sx > (gw - 1) as f32 || sy > (gh - 1) as f32 { continue; }
                let (x0, y0) = (sx.floor() as usize, sy.floor() as usize); let (x1, y1) = ((x0 + 1).min(gw - 1), (y0 + 1).min(gh - 1)); let (fx_, fy_) = (sx - x0 as f32, sy - y0 as f32);
                let v = (loc[y0 * gw + x0] * (1.0 - fx_) + loc[y0 * gw + x1] * fx_) * (1.0 - fy_) + (loc[y1 * gw + x0] * (1.0 - fx_) + loc[y1 * gw + x1] * fx_) * fy_; put(&mut cov, &mut tint, xx, yy, v); } }
        }
        }
        if u.blur > 0.5 { /* per-unit blur approximated below by whole-mask softening when any unit blurs */ }
    }
    let blur_any = glyphs.iter().enumerate().take(shown).map(|(ci, g)| animate(an, t, g, nchar, nwords, nl, ci).blur).fold(0.0, f32::max);
    if blur_any > 0.5 { let mut m = Frame { w: cw, h: ch, d: vec![1.0; cw * ch * 4] }; for i in 0..cw * ch { m.d[i * 4] = cov[i]; } box_blur3(&mut m, (blur_any * 2.0) as usize); for i in 0..cw * ch { cov[i] = cov[i] * (1.0 - (blur_any / (blur_any + 4.0))) + m.d[i * 4] * (blur_any / (blur_any + 4.0)); } }
    (cov, [ink_bx, by, ink_bw, bh], tint)
}

pub fn render_text(fr: &mut Frame, text: &str, fontpath: &str, size: f64, col: [f64; 3], spacing: f64, stroke_w: f64, stroke_col: [f64; 3], shadow: f64, align: &str, reveal: f64, fx: Option<&TextFx>, t: f64) -> Result<(), String> {
    let f = font(fontpath, fx.map(|x| x.weight).unwrap_or(0.0))?; let upper; let text = if fx.map(|x| x.uppercase).unwrap_or(false) { upper = text.to_uppercase(); upper.as_str() } else { text }; let (w, h) = (fr.w, fr.h);
    let (main, bounds, tint) = coverage(w, h, text, &f, size as f32, spacing as f32, align, reveal, fx, t);
    let over = |fr: &mut Frame, cov: &[f32], c: &dyn Fn(usize) -> [f32; 3], k: f32| { for i in 0..w * h { let a = cov[i] * k; if a <= 0.0 { continue; } let cc = c(i); let o = &mut fr.d[i * 4..i * 4 + 4]; let na = a + o[3] * (1.0 - a); for j in 0..3 { o[j] = (cc[j] * a + o[j] * o[3] * (1.0 - a)) / na.max(1e-6); } o[3] = na; } };
    // background box
    if let Some(fx) = fx { if let Some(bg) = fx.bg { let (x0, y0, x1, y1) = (bounds[0] - fx.bg_pad as f32, bounds[1] - fx.bg_pad as f32, bounds[0] + bounds[2] + fx.bg_pad as f32, bounds[1] + bounds[3] + fx.bg_pad as f32); let r = (fx.bg_radius as f32).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0); let (cx, cy, hw, hh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
        let mut m = vec![0f32; w * h]; for y in (y0.floor().max(0.0) as usize)..(y1.ceil() as usize + 1).min(h) { for x in (x0.floor().max(0.0) as usize)..(x1.ceil() as usize + 1).min(w) { let (px, py) = ((x as f32 + 0.5 - cx).abs() - hw + r, (y as f32 + 0.5 - cy).abs() - hh + r); let d = (px.max(0.0).powi(2) + py.max(0.0).powi(2)).sqrt() + px.max(py).min(0.0) - r; m[y * w + x] = (0.5 - d).clamp(0.0, 1.0); } }
        over(fr, &m, &|_| [bg[0] as f32, bg[1] as f32, bg[2] as f32], bg[3] as f32); } }
    // shadow (offset + optional blur)
    let (sh_off, sh_blur, sh_op, sh_col, sh_ang) = match fx { Some(x) => (shadow, x.shadow_blur, x.shadow_opacity, x.shadow_color, x.shadow_angle.to_radians()), None => (shadow, 0.0, 0.6, [0.0; 3], 0.785398) };
    if sh_off > 0.0 || sh_blur > 0.0 { let (dx, dy) = ((sh_off * sh_ang.cos()) as i32, (sh_off * sh_ang.sin()) as i32); let mut m = Frame { w, h, d: vec![1.0; w * h * 4] };
        for y in 0..h as i32 { for x in 0..w as i32 { let (sx, sy) = (x - dx, y - dy); if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 { m.d[(y as usize * w + x as usize) * 4] = main[sy as usize * w + sx as usize]; } else { m.d[(y as usize * w + x as usize) * 4] = 0.0; } } }
        if sh_blur > 0.5 { box_blur3(&mut m, (sh_blur * 2.0) as usize); } let sc: Vec<f32> = (0..w * h).map(|i| m.d[i * 4]).collect(); over(fr, &sc, &|_| [sh_col[0] as f32, sh_col[1] as f32, sh_col[2] as f32], sh_op as f32); }
    // glow
    if let Some(fx) = fx { if fx.glow > 0.0 { let mut m = Frame { w, h, d: vec![1.0; w * h * 4] }; for i in 0..w * h { m.d[i * 4] = main[i]; } box_blur3(&mut m, (fx.glow * 2.0) as usize); let gc: Vec<f32> = (0..w * h).map(|i| (m.d[i * 4] * 1.8).min(1.0)).collect(); over(fr, &gc, &|_| [fx.glow_color[0] as f32, fx.glow_color[1] as f32, fx.glow_color[2] as f32], 0.9); } }
    if stroke_w > 0.0 { let mut st = main.clone(); let r = stroke_w.ceil() as i32;
        for dy in -r..=r { for dx in -r..=r { if ((dx * dx + dy * dy) as f64).sqrt() > stroke_w { continue; } for y in 0..h as i32 { let sy = y - dy; if sy < 0 || sy >= h as i32 { continue; } for x in 0..w as i32 { let sx = x - dx; if sx < 0 || sx >= w as i32 { continue; } let v = main[sy as usize * w + sx as usize]; let i = y as usize * w + x as usize; if v > st[i] { st[i] = v; } } } } }
        over(fr, &st, &|_| [stroke_col[0] as f32, stroke_col[1] as f32, stroke_col[2] as f32], 1.0); }
    let grad = fx.and_then(|x| x.gradient); let (by0, bh) = (bounds[1], bounds[3].max(1.0));
    // Fill Color animators: base (solid or gradient, as below) * fs + ft per pixel; without a filling animator the match below runs as before
    if let Some(tb) = tint.as_ref() {
        let base = |i: usize| -> [f32; 3] { match grad { Some((a, b)) => { let y = (i / w) as f32; let t_ = ((y - by0) / bh).clamp(0.0, 1.0); [(a[0] + (b[0] - a[0]) * t_ as f64) as f32, (a[1] + (b[1] - a[1]) * t_ as f64) as f32, (a[2] + (b[2] - a[2]) * t_ as f64) as f32] } None => [col[0] as f32, col[1] as f32, col[2] as f32] } };
        over(fr, &main, &|i| { let (c, q) = (base(i), tb[i]); [c[0] * q[3] + q[0], c[1] * q[3] + q[1], c[2] * q[3] + q[2]] }, 1.0);
        return Ok(());
    }
    match grad { Some((a, b)) => over(fr, &main, &|i| { let y = (i / w) as f32; let t_ = ((y - by0) / bh).clamp(0.0, 1.0); [(a[0] + (b[0] - a[0]) * t_ as f64) as f32, (a[1] + (b[1] - a[1]) * t_ as f64) as f32, (a[2] + (b[2] - a[2]) * t_ as f64) as f32] }, 1.0),
        None => over(fr, &main, &|_| [col[0] as f32, col[1] as f32, col[2] as f32], 1.0) }
    Ok(())
}


#[cfg(test)]
mod font_fallback_warning_tests {
    use super::*;

    /// 8.5.4: a missing bundled font used to fall back to DejaVu/system with zero signal anywhere except
    /// lint_edit's own check (which only runs if someone calls it). The resolver itself must now say so,
    /// exactly once per distinct missing name -- not once per call (resolve_font_info runs per glyph/frame).
    #[test]
    fn warns_exactly_once_per_distinct_missing_font_name() {
        let name = "Totally_Not_A_Real_Font_8_5_4_xyz";
        let first = font_fallback_warning(name);
        assert!(first.is_some(), "the first fallback for a missing font should produce a warning");
        let msg = first.unwrap();
        assert!(msg.contains(name), "the warning must name the missing font: {msg}");
        assert!(msg.contains("KIMO_FONTS"), "the warning must say how to fix it (KIMO_FONTS): {msg}");
        assert!(msg.contains("fonts/fetch.sh"), "the warning must say how to fix it (fonts/fetch.sh): {msg}");

        let second = font_fallback_warning(name);
        assert!(second.is_none(), "a SECOND fallback for the SAME missing font must not warn again: {second:?}");

        // a DIFFERENT missing name still gets its own warning -- the dedup is per-name, not global
        let other = font_fallback_warning("Another_Missing_Font_8_5_4_xyz");
        assert!(other.is_some(), "a different missing font name must still warn");
    }

    /// The warning must be a pure side channel: resolve_font_info's actual return value (the resolved path
    /// and whether it substituted) is identical whether or not this is the first time that name fell back.
    #[test]
    fn the_warning_never_changes_what_resolve_font_info_returns() {
        let name = "Yet_Another_Missing_Font_8_5_4_xyz";
        let (path1, sub1) = resolve_font_info(name);
        let (path2, sub2) = resolve_font_info(name); // second call: dedup means no warning fires this time
        assert_eq!(path1, path2, "the resolved path must not depend on whether the warning already fired");
        assert_eq!(sub1, sub2);
        assert!(sub1, "a made-up font name should report substituted=true");
    }
}
