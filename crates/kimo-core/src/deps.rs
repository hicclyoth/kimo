//! External file references of a project: ONE walker (`Project::external_refs`) that knows every field holding a file path, a setter that writes
//! through the same locators (`Project::set_ref`), and the path resolution rule (`resolve_path`). Same discipline as `clear_refs_to`: nothing else
//! may enumerate or rewrite project file paths. The guard test (tests/external_refs.rs) fails when a new path-bearing field is not registered here.
use crate::masks::MaskLayer;
use crate::model::{Clip, Effect, Project, Source, Track};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RefKind { Media, Image, Audio, Font, Lut, Srt, Occlusion, Project, Other }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Owner {
    #[serde(skip_serializing_if = "Option::is_none")] pub clip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub occluder_id: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")] pub project: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtRef {
    pub kind: RefKind,
    pub owner: Owner,
    /// Locator understood by `Project::set_ref`, e.g. `assets[a1].path`, `clip[clip7].effects[0].strings.path`.
    pub field: String,
    pub path: String,
    /// True for a font given by family name (resolved through KIMO_FONTS / bundled / OS fonts) rather than by file path.
    #[serde(skip_serializing_if = "std::ops::Not::not")] pub family: bool,
}

const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "kimopx", "exr", "tga", "dpx", "heic"];
const AUDIO_EXT: &[&str] = &["wav", "mp3", "aac", "m4a", "flac", "ogg", "opus", "aif", "aiff"];
const FONT_EXT: &[&str] = &["ttf", "otf", "ttc", "woff", "woff2"];
fn ext(p: &str) -> String { p.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default() }
/// Media kind of an asset from its extension (images and audio-only files are assets too).
pub fn asset_kind(path: &str) -> RefKind { let e = ext(path); if IMAGE_EXT.contains(&e.as_str()) { RefKind::Image } else if AUDIO_EXT.contains(&e.as_str()) { RefKind::Audio } else { RefKind::Media } }
/// A font value is a file path when it has a directory separator or a font-file extension; otherwise it is a family name.
pub fn font_is_path(f: &str) -> bool { f.contains('/') || f.contains('\\') || FONT_EXT.contains(&ext(f).as_str()) }

/// Effect string keys that hold files: (key, kind). `path` is a file only on `lut` effects (`cube_text` is inline).
fn effect_refs(e: &Effect, base: &str, owner: &Owner, out: &mut Vec<ExtRef>) {
    for (key, kind) in [("path", RefKind::Lut), ("config_path", RefKind::Other), ("aux_path", RefKind::Image)] {
        if key == "path" && e.kind != "lut" { continue; }
        if let Some(v) = e.strings.get(key) { out.push(ExtRef { kind, owner: owner.clone(), field: format!("{base}.strings.{key}"), path: v.clone(), family: false }); }
    }
}
fn mask_layer_refs(l: &MaskLayer, base: &str, owner: &Owner, out: &mut Vec<ExtRef>) {
    if let Some(v) = l.strings.get("dir") { out.push(ExtRef { kind: RefKind::Other, owner: owner.clone(), field: format!("{base}.strings.dir"), path: v.clone(), family: false }); }
}
fn clip_refs(c: &Clip, out: &mut Vec<ExtRef>) {
    let owner = Owner { clip_id: Some(c.id.clone()), ..Default::default() };
    let b = format!("clip[{}]", c.id);
    match &c.source {
        Source::Text { font, .. } => out.push(ExtRef { kind: RefKind::Font, owner: owner.clone(), field: format!("{b}.source.font"), path: font.clone(), family: !font_is_path(font) }),
        Source::Code(cc) => out.push(ExtRef { kind: RefKind::Other, owner: owner.clone(), field: format!("{b}.source.dir"), path: cc.dir.clone(), family: false }),
        Source::ShardBurst(sb) => if let Some(pl) = &sb.plane {
            if let Some(o) = &pl.occlusion { out.push(ExtRef { kind: RefKind::Occlusion, owner: owner.clone(), field: format!("{b}.source.plane.occlusion"), path: o.clone(), family: false }); }
            if let Some(t) = &pl.texture { out.push(ExtRef { kind: RefKind::Image, owner: owner.clone(), field: format!("{b}.source.plane.texture"), path: t.clone(), family: false }); }
        },
        Source::Precomp { tracks } => tracks_refs(tracks, out),
        _ => {}
    }
    for (i, e) in c.effects.iter().enumerate() { effect_refs(e, &format!("{b}.effects[{i}]"), &owner, out); }
    if let Some(m) = &c.mask { for (i, l) in m.layers.iter().enumerate() { mask_layer_refs(l, &format!("{b}.mask.layers[{i}]"), &owner, out); } }
    if let Some(g) = &c.grade_graph { for (nid, n) in &g.nodes {
        let no = Owner { clip_id: Some(c.id.clone()), node_id: Some(nid.clone()), ..Default::default() };
        if let Some(e) = &n.effect { effect_refs(e, &format!("{b}.grade_graph.nodes[{nid}].effect"), &no, out); }
        if let Some(l) = &n.mask { mask_layer_refs(l, &format!("{b}.grade_graph.nodes[{nid}].mask"), &no, out); }
    } }
}
fn tracks_refs(tracks: &[Track], out: &mut Vec<ExtRef>) { for t in tracks { for c in &t.clips { clip_refs(c, out); } } }

impl Project {
    /// Every file path the project refers to, in a deterministic order (assets, occluders, delivery config, then clips depth-first). THE single source of truth.
    pub fn external_refs(&self) -> Vec<ExtRef> {
        let mut out = vec![];
        for (id, a) in &self.assets { out.push(ExtRef { kind: asset_kind(&a.path), owner: Owner { asset_id: Some(id.clone()), ..Default::default() }, field: format!("assets[{id}].path"), path: a.path.clone(), family: false }); }
        for (id, q) in &self.proxies.entries { out.push(ExtRef { kind: RefKind::Media, owner: Owner { asset_id: Some(id.clone()), ..Default::default() }, field: format!("proxies[{id}].path"), path: q.path.clone(), family: false }); }
        for (id, o) in &self.occluders { out.push(ExtRef { kind: asset_kind(&o.path), owner: Owner { occluder_id: Some(id.clone()), ..Default::default() }, field: format!("occluders[{id}].path"), path: o.path.clone(), family: false }); }
        if let Some(d) = &self.ocio_view_delivery { out.push(ExtRef { kind: RefKind::Other, owner: Owner { project: true, ..Default::default() }, field: "ocio_view_delivery.config_path".into(), path: d.config_path.clone(), family: false }); }
        tracks_refs(&self.tracks, &mut out);
        out
    }

    /// Rewrites one reference found by `external_refs` (by its `field` locator). Errors when the locator does not name a path field of this project.
    pub fn set_ref(&mut self, field: &str, new_path: &str) -> Result<(), String> {
        let toks = parse_locator(field).ok_or_else(|| format!("bad reference locator '{field}'"))?;
        let slot = slot_mut(self, &toks).ok_or_else(|| format!("no such reference '{field}'"))?;
        *slot = new_path.to_string(); Ok(())
    }
}

type Tok = (String, Option<String>);
/// `a.b[key].c` -> [(a,None),(b,Some(key)),(c,None)]. Keys may contain dots; they end at the first `]`.
fn parse_locator(s: &str) -> Option<Vec<Tok>> {
    let (mut out, mut i) = (vec![], 0); let b = s.as_bytes();
    while i < b.len() {
        let st = i; while i < b.len() && b[i] != b'.' && b[i] != b'[' { i += 1; }
        let name = s[st..i].to_string(); if name.is_empty() { return None; }
        let mut key = None;
        if i < b.len() && b[i] == b'[' { let e = s[i + 1..].find(']')? + i + 1; key = Some(s[i + 1..e].to_string()); i = e + 1; }
        out.push((name, key));
        if i < b.len() { if b[i] != b'.' { return None; } i += 1; }
    }
    if out.is_empty() { None } else { Some(out) }
}
fn clip_deep<'a>(tracks: &'a mut [Track], id: &str) -> Option<&'a mut Clip> {
    for t in tracks { for c in t.clips.iter_mut() {
        if c.id == id { return Some(std::sync::Arc::make_mut(c)); }
        if matches!(c.source, Source::Precomp { .. }) { if let Source::Precomp { tracks } = &mut std::sync::Arc::make_mut(c).source { if let Some(x) = clip_deep(tracks, id) { return Some(x); } } }
    } }
    None
}
fn str_key<'a>(m: &'a mut std::collections::BTreeMap<String, String>, k: &str) -> Option<&'a mut String> { m.get_mut(k) }
fn effect_slot<'a>(e: &'a mut Effect, t: &[Tok]) -> Option<&'a mut String> {
    let lut = e.kind == "lut"; match t { [(s, None), (k, None)] if s == "strings" && ((k == "path" && lut) || k == "config_path") => str_key(&mut e.strings, k), _ => None }
}
fn mask_slot<'a>(l: &'a mut MaskLayer, t: &[Tok]) -> Option<&'a mut String> {
    match t { [(s, None), (k, None)] if s == "strings" && k == "dir" => str_key(&mut l.strings, k), _ => None }
}
fn slot_mut<'a>(p: &'a mut Project, t: &[Tok]) -> Option<&'a mut String> {
    match t {
        [(a, Some(id)), (f, None)] if a == "assets" && f == "path" => p.assets.get_mut(id).map(|x| &mut x.path),
        [(a, Some(id)), (f, None)] if a == "proxies" && f == "path" => p.proxies.entries.get_mut(id).map(|x| &mut x.path),
        [(a, Some(id)), (f, None)] if a == "occluders" && f == "path" => p.occluders.get_mut(id).map(|x| &mut x.path),
        [(a, None), (f, None)] if a == "ocio_view_delivery" && f == "config_path" => p.ocio_view_delivery.as_mut().map(|x| &mut x.config_path),
        [(a, Some(cid)), rest @ ..] if a == "clip" => {
            let c = clip_deep(&mut p.tracks, cid)?;
            match rest {
                [(s, None), (f, None)] if s == "source" && f == "font" => if let Source::Text { font, .. } = &mut c.source { Some(font) } else { None },
                [(s, None), (f, None)] if s == "source" && f == "dir" => if let Source::Code(cc) = &mut c.source { Some(&mut cc.dir) } else { None },
                [(s, None), (pl, None), (f, None)] if s == "source" && pl == "plane" => if let Source::ShardBurst(sb) = &mut c.source { let pl = sb.plane.as_mut()?; if f == "occlusion" { pl.occlusion.as_mut() } else if f == "texture" { pl.texture.as_mut() } else { None } } else { None },
                [(e, Some(i)), tail @ ..] if e == "effects" => effect_slot(c.effects.get_mut(i.parse::<usize>().ok()?)?, tail),
                [(m, None), (l, Some(i)), tail @ ..] if m == "mask" && l == "layers" => mask_slot(c.mask.as_mut()?.layers.get_mut(i.parse::<usize>().ok()?)?, tail),
                [(g, None), (n, Some(nid)), (w, None), tail @ ..] if g == "grade_graph" && n == "nodes" => { let node = c.grade_graph.as_mut()?.nodes.get_mut(nid)?; if w == "effect" { effect_slot(node.effect.as_mut()?, tail) } else if w == "mask" { mask_slot(node.mask.as_mut()?, tail) } else { None } }
                _ => None,
            }
        }
        _ => None,
    }
}

/// How a stored path was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum How { Abs, Rel, EnvFont, WindowsStyle }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved { pub path: PathBuf, pub how: How }

/// THE path resolution rule: absolute stays; relative resolves against the project file's directory (`project_dir`, None = current directory).
/// A Windows-style stored path (drive letter or backslashes) is never interpreted as a POSIX path: it is returned as-is with `How::WindowsStyle`,
/// so a reader reports a missing file with a clear hint instead of guessing.
pub fn resolve_path(project_dir: Option<&Path>, stored: &str) -> Resolved {
    let b = stored.as_bytes();
    if stored.contains('\\') || (b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) { return Resolved { path: PathBuf::from(stored), how: How::WindowsStyle }; }
    let p = Path::new(stored);
    if p.is_absolute() { Resolved { path: p.to_path_buf(), how: How::Abs } } else { Resolved { path: match project_dir { Some(d) => d.join(p), None => p.to_path_buf() }, how: How::Rel } }
}

/// Process-wide project directory, for readers deep in render/par worker threads that do not hold the project (a thread_local would be invisible to
/// those fresh threads). Set by load_project / save_project / collect together with `Project.base_dir`; the project itself is the source for readers that hold it.
static BASE_DIR: std::sync::RwLock<Option<std::sync::Arc<PathBuf>>> = std::sync::RwLock::new(None);
pub fn set_base_dir(d: Option<PathBuf>) { *BASE_DIR.write().unwrap_or_else(|e| e.into_inner()) = d.map(std::sync::Arc::new); }
pub fn base_dir() -> Option<std::sync::Arc<PathBuf>> { BASE_DIR.read().unwrap_or_else(|e| e.into_inner()).clone() }
/// Resolve a stored path for a reader without a `&Project` (see BASE_DIR). Absolute paths and sequence patterns stay themselves.
pub fn resolve_str(stored: &str) -> String { let d = base_dir(); resolve_path(d.as_deref().map(|p| p.as_path()), stored).path.to_string_lossy().into_owned() }
/// Canonical directory of a project file path (None when it cannot be resolved, e.g. the directory does not exist yet).
pub fn dir_of(project_file: &str) -> Option<PathBuf> { let p = Path::new(project_file); let d = match p.parent() { Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(), _ => PathBuf::from(".") }; std::fs::canonicalize(d).ok().map(plain_path) }
/// canonicalize gives `\\?\W:\x` on Windows, and ffmpeg cannot open that form. No-op elsewhere.
pub fn plain_path(p: PathBuf) -> PathBuf { let s = p.to_string_lossy(); if let Some(r) = s.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{r}")); } if let Some(r) = s.strip_prefix(r"\\?\") { return PathBuf::from(r); } p }

/// One recorded dependency (written by collect_project, read by list_dependencies to report `changed`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DepRecord {
    pub kind: String, pub rel_path: String, #[serde(default)] pub abs_hint: String, pub size: u64, pub sha256: String,
    #[serde(default)] pub hash_mode: String, #[serde(default)] pub mtime: i64, #[serde(default)] pub status: String,
}
/// stored-path key -> record
pub type DepTable = std::collections::BTreeMap<String, DepRecord>;

// ---------------------------------------------------------------------------------------------------------------------------------------------
// SHA-256 (no external crate), file hashing with an (path, size, mtime) cache, dependency listing, font license read, dependency signature.
// ---------------------------------------------------------------------------------------------------------------------------------------------
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2];
/// Streaming SHA-256.
pub struct Sha256 { h: [u32; 8], buf: [u8; 64], n: usize, len: u64 }
impl Sha256 {
    pub fn new() -> Self { Sha256 { h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], buf: [0; 64], n: 0, len: 0 } }
    fn block(&mut self, b: &[u8]) {
        let mut w = [0u32; 64]; for i in 0..16 { w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]); }
        for i in 16..64 { let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3); let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10); w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1); }
        let mut v = self.h;
        for i in 0..64 { let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25); let ch = (v[4] & v[5]) ^ (!v[4] & v[6]); let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22); let mj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]); let t2 = s0.wrapping_add(mj);
            v[7] = v[6]; v[6] = v[5]; v[5] = v[4]; v[4] = v[3].wrapping_add(t1); v[3] = v[2]; v[2] = v[1]; v[1] = v[0]; v[0] = t1.wrapping_add(t2); }
        for i in 0..8 { self.h[i] = self.h[i].wrapping_add(v[i]); }
    }
    pub fn update(&mut self, mut d: &[u8]) {
        self.len += d.len() as u64;
        if self.n > 0 { let t = (64 - self.n).min(d.len()); self.buf[self.n..self.n + t].copy_from_slice(&d[..t]); self.n += t; d = &d[t..]; if self.n == 64 { let b = self.buf; self.block(&b); self.n = 0; } }
        while d.len() >= 64 { self.block(&d[..64]); d = &d[64..]; }
        if !d.is_empty() { self.buf[..d.len()].copy_from_slice(d); self.n = d.len(); }
    }
    pub fn finish_hex(mut self) -> String {
        let bits = self.len * 8; let mut pad = vec![0x80u8]; while (self.n + pad.len()) % 64 != 56 { pad.push(0); } pad.extend_from_slice(&bits.to_be_bytes());
        let l = self.len; self.update(&pad); self.len = l;
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}
pub fn sha256_hex(d: &[u8]) -> String { let mut s = Sha256::new(); s.update(d); s.finish_hex() }

/// Files above this size are hashed in `head_tail` mode (first and last MiB plus the size) unless the caller asks for a full hash.
pub const HEAD_TAIL_ABOVE: u64 = 256 << 20;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat { pub size: u64, pub mtime_ns: i128 }
pub fn stat(p: &Path) -> Option<FileStat> { let m = std::fs::metadata(p).ok()?; if !m.is_file() { return None; } let mt = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0); Some(FileStat { size: m.len(), mtime_ns: mt }) }
type HashCache = std::sync::Mutex<std::collections::HashMap<(PathBuf, u64, i128, bool), String>>;
fn hash_cache() -> &'static HashCache { static C: std::sync::OnceLock<HashCache> = std::sync::OnceLock::new(); C.get_or_init(Default::default) }
pub static HASH_CACHE_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// sha256 of a file (hex) and the mode used: "full" or "head_tail". Cached by (path, size, mtime, mode); cooperative cancel between 1 MiB chunks.
pub fn hash_file(p: &Path, force_full: bool) -> Result<(String, &'static str), String> {
    use std::io::{Read, Seek, SeekFrom};
    let st = stat(p).ok_or_else(|| format!("cannot read '{}'", p.display()))?; let head_tail = !force_full && st.size > HEAD_TAIL_ABOVE;
    let key = (p.to_path_buf(), st.size, st.mtime_ns, head_tail);
    if let Some(h) = hash_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) { HASH_CACHE_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed); return Ok((h.clone(), if head_tail { "head_tail" } else { "full" })); }
    let mut f = std::fs::File::open(p).map_err(|e| format!("cannot read '{}': {e}", p.display()))?; let mut s = Sha256::new(); let mut buf = vec![0u8; 1 << 20];
    if head_tail { s.update(&st.size.to_le_bytes()); let n = f.read(&mut buf).map_err(|e| e.to_string())?; s.update(&buf[..n]); f.seek(SeekFrom::End(-(1 << 20))).map_err(|e| e.to_string())?; let n = f.read(&mut buf).map_err(|e| e.to_string())?; s.update(&buf[..n]); }
    else { loop { crate::guard::check(); let n = f.read(&mut buf).map_err(|e| e.to_string())?; if n == 0 { break; } s.update(&buf[..n]); } }
    let h = s.finish_hex(); hash_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, h.clone()); Ok((h, if head_tail { "head_tail" } else { "full" }))
}

/// `%d` / `%04d` frame-sequence pattern -> the path of frame `i`. None when the string has no pattern.
pub fn seq_frame(pat: &str, i: i64) -> Option<String> {
    let at = pat.find('%')?; let rest = &pat[at + 1..]; let digs: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect(); if !rest[digs.len()..].starts_with('d') { return None; }
    let w: usize = digs.parse().unwrap_or(0); Some(format!("{}{:0w$}{}", &pat[..at], i, &rest[digs.len() + 1..], w = w))
}

/// Does the stored reference exist on disk? (A frame sequence exists when frame 0 or 1 does; a directory reference when the directory does.)
pub fn ref_exists(r: &ExtRef, resolved: &str) -> bool {
    if r.family { return family_file(&r.path).is_some(); }
    if let Some(f0) = seq_frame(resolved, 0) { return Path::new(&f0).is_file() || seq_frame(resolved, 1).map_or(false, |f| Path::new(&f).is_file()); }
    let p = Path::new(resolved); if is_dir_field(&r.field) { p.is_dir() } else { p.is_file() }
}

/// Fields whose reference is a directory, not a file: a matte store (`strings.dir`) and a code clip folder (`source.dir`).
pub fn is_dir_field(field: &str) -> bool { field.ends_with("strings.dir") || field.ends_with("source.dir") }

/// Embedding permission of a font file from OS/2 fsType and the name-table license description (nameID 13). None when the file is not a readable sfnt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FontLicense { pub fs_type: u16, pub license: String, pub embeddable: bool }
pub fn font_license(bytes: &[u8]) -> Option<FontLicense> {
    let u16at = |o: usize| bytes.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let u32at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let base = if bytes.get(0..4) == Some(b"ttcf") { u32at(12)? as usize } else { 0 };
    let n = u16at(base + 4)? as usize; let (mut os2, mut name) = (None, None);
    for i in 0..n { let r = base + 12 + 16 * i; let tag = bytes.get(r..r + 4)?; let off = u32at(r + 8)? as usize; if tag == b"OS/2" { os2 = Some(off); } if tag == b"name" { name = Some(off); } }
    let fs_type = u16at(os2? + 8)?;
    let mut license = String::new();
    if let Some(no) = name { if let (Some(cnt), Some(so)) = (u16at(no + 2), u16at(no + 4)) { for i in 0..cnt as usize { let r = no + 6 + 12 * i; let (pid, nid, len, off) = (u16at(r)?, u16at(r + 6)?, u16at(r + 8)? as usize, u16at(r + 10)? as usize); if nid != 13 { continue; }
        let s = bytes.get(no + so as usize + off..no + so as usize + off + len)?; license = if pid == 3 || pid == 0 { String::from_utf16_lossy(&s.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()) } else { s.iter().map(|b| *b as char).collect() }; if !license.is_empty() { break; } } } }
    // fsType: 0 installable, 8 editable embedding allow copying with the project; 2 restricted (and 4 preview/print) do not.
    Some(FontLicense { fs_type, license, embeddable: matches!(fs_type & 0x000F, 0 | 8) })
}

fn norm_font(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect() }
/// File of an installed/bundled font family WITHOUT the renderer's silent fallbacks (text::resolve_font falls back to Montserrat/DejaVu).
pub fn family_file(name: &str) -> Option<PathBuf> {
    let want = norm_font(name); let fonts = crate::text::all_fonts();
    fonts.iter().find(|(s, _)| norm_font(s) == want).or_else(|| fonts.iter().find(|(s, _)| norm_font(s).starts_with(&want))).map(|(_, p)| p.clone())
}
/// The file a font reference reads: a font path resolves against the project dir, a family name through the font dirs.
pub fn font_file(project_dir: Option<&Path>, r: &ExtRef) -> Option<PathBuf> { if r.family { family_file(&r.path) } else { let p = resolve_path(project_dir, &r.path).path; if p.is_file() { Some(p) } else { None } } }

/// Source-time range [lo, hi] each asset is used over (clip in_point .. in_point + source time of its duration), recursing into precomps.
pub fn used_ranges(p: &Project) -> std::collections::BTreeMap<String, (f64, f64)> {
    fn walk(tracks: &[Track], out: &mut std::collections::BTreeMap<String, (f64, f64)>) {
        for t in tracks { for c in &t.clips { match &c.source {
            Source::Media { asset } => { let (lo, hi) = (c.in_point, c.in_point + c.source_time(c.duration)); let e = out.entry(asset.clone()).or_insert((lo, hi)); e.0 = e.0.min(lo); e.1 = e.1.max(hi); }
            Source::Precomp { tracks } => walk(tracks, out), _ => {} } } }
    }
    let mut m = Default::default(); walk(&p.tracks, &mut m); m
}

/// One dependency as reported by list_dependencies.
#[derive(Clone, Debug, Serialize)]
pub struct DepItem {
    pub kind: RefKind, pub owner: Owner, pub field: String, pub path: String, pub resolved: String, pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")] pub size: Option<u64>, #[serde(skip_serializing_if = "Option::is_none")] pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub hash_mode: Option<&'static str>, #[serde(skip_serializing_if = "Option::is_none")] pub used_range: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "std::ops::Not::not")] pub family: bool,
}
/// Status of every reference: ok | missing | changed (size differs from the recorded dependency, or the hash when `check_hash`). Never fails for a missing file.
pub fn list_dependencies(p: &Project, check_hash: bool) -> Vec<DepItem> {
    let ranges = used_ranges(p); let dir = p.base_dir.as_deref();
    p.external_refs().into_iter().map(|r| {
        let resolved = if r.family { family_file(&r.path).map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| r.path.clone()) } else { resolve_path(dir, &r.path).path.to_string_lossy().into_owned() };
        let exists = ref_exists(&r, &resolved); let file = Path::new(&resolved);
        let (mut size, mut sha, mut mode, mut status) = (None, None, None, if exists { "ok" } else { "missing" });
        if exists && file.is_file() {
            size = std::fs::metadata(file).ok().map(|m| m.len());
            let rec = p.deps.as_ref().and_then(|d| d.get(&r.path));
            if check_hash || rec.map_or(false, |x| !x.sha256.is_empty() && false) { if let Ok((h, m)) = hash_file(file, false) { sha = Some(h); mode = Some(m); } }
            if let Some(rec) = rec { if rec.size != 0 && size.map_or(false, |s| s != rec.size) { status = "changed"; } else if let (Some(h), true) = (&sha, !rec.sha256.is_empty() && rec.hash_mode == mode.unwrap_or("full")) { if *h != rec.sha256 { status = "changed"; } } }
        }
        let used_range = r.owner.asset_id.as_ref().and_then(|a| ranges.get(a).copied());
        DepItem { kind: r.kind, owner: r.owner, field: r.field, path: r.path, resolved, status, size, sha256: sha, hash_mode: mode, used_range, family: r.family }
    }).collect()
}

/// Dependency signature shared by delivery checks (RV-5/RV-6 reuse it): sha256 over every reference (kind, stored path, size, content hash or "missing") that matters
/// for `range` (timeline seconds; None = whole project), plus one signature per owner (clip id / asset id / project) so a cache can invalidate per object.
pub fn dependency_signature(p: &Project, range: Option<(f64, f64)>) -> (String, std::collections::BTreeMap<String, String>) {
    let mut keep_clips: Option<std::collections::BTreeSet<String>> = None;
    if let Some((t0, t1)) = range { let mut s = std::collections::BTreeSet::new(); fn walk(tracks: &[Track], t0: f64, t1: f64, s: &mut std::collections::BTreeSet<String>) { for t in tracks { for c in &t.clips { if c.start < t1 && c.start + c.duration > t0 { s.insert(c.id.clone()); if let Source::Precomp { tracks } = &c.source { walk(tracks, 0.0, f64::MAX, s); } } } } } walk(&p.tracks, t0, t1, &mut s); keep_clips = Some(s); }
    let items = list_dependencies(p, true); let mut per: std::collections::BTreeMap<String, Sha256> = Default::default(); let mut all = Sha256::new();
    let mut rows: Vec<(String, String)> = vec![];
    for it in &items {
        let owner = it.owner.clip_id.clone().or(it.owner.asset_id.clone()).or(it.owner.occluder_id.clone()).unwrap_or_else(|| "project".into());
        if let (Some(k), Some(c)) = (&keep_clips, &it.owner.clip_id) { if !k.contains(c) { continue; } }
        let line = format!("{:?}|{}|{}|{}|{}\n", it.kind, it.path, it.size.map_or("-".into(), |s| s.to_string()), it.sha256.clone().unwrap_or_else(|| "missing".into()), it.status);
        rows.push((owner, line));
    }
    rows.sort();
    for (o, l) in &rows { all.update(l.as_bytes()); per.entry(o.clone()).or_insert_with(Sha256::new).update(l.as_bytes()); }
    (all.finish_hex(), per.into_iter().map(|(k, v)| (k, v.finish_hex())).collect())
}
