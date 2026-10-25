//! SVG drawing with cairo, for the subset the themes use: paths (M L H V C S Q T Z) and rects, filled
//! and stroked, in colours or linear gradients; opacity, fill rule, stroke width and join, crisp edges;
//! groups with transforms, clip paths and masks. Also the disk cache of pictures made from SVGs (`kept`),
//! in the user's cache directory.
//!
//! A theme's SVG is its data: a missing or broken file, or a part of one this cannot draw, is said once
//! and left out, never a crash.

use crate::path::PathData;
use crate::tweaks::Colour;
use cairo::{Antialias, Context, FillRule, ImageSurface, LineJoin, LinearGradient, Matrix, Pattern, SolidPattern};
use roxmltree::Node;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Say a problem with an SVG file once per run.
fn say_once(path: &Path, problem: &str) {
    static SAID: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap();
    if !said.iter().any(|p| p == path) {
        crate::errln!("{}: {problem}", path.display());
        said.push(path.to_path_buf());
    }
}

thread_local! {
    /// The first thing a drawing left out, said when it is done.
    static FAULT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn fault(what: String) {
    FAULT.with(|f| {
        f.borrow_mut().get_or_insert(what);
    });
}

/// The most of an SVG read, unzipped.
const MAX_SVG: u64 = 256 << 20;

/// An SVG file as text, gzipped (.svgz) or not; None (said) if it cannot be read.
pub fn svg_open(path: &Path) -> Option<String> {
    let read = || -> Result<String, String> {
        let raw = crate::files::read(path, crate::files::Kind::Svg).map_err(|e| e.to_string())?;
        if raw.starts_with(&[0x1f, 0x8b]) {
            let mut s = String::new();
            flate2::read::GzDecoder::new(&raw[..]).take(MAX_SVG).read_to_string(&mut s).map_err(|e| e.to_string())?;
            Ok(s)
        } else {
            String::from_utf8(raw).map_err(|e| e.to_string())
        }
    };
    read().map_err(|e| say_once(path, &format!("{e}: drawn as nothing"))).ok()
}

/// The first `chars` characters of an SVG file, gzipped or not ("" if it cannot be read).
pub fn svg_head(path: &Path, chars: usize) -> String {
    let open = || crate::files::open(path, crate::files::Kind::Svg);
    let Ok(mut f) = open() else { return String::new() };
    let mut magic = [0u8; 2];
    let zipped = f.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b];
    let Ok(f) = open() else { return String::new() };
    let mut buf = vec![];
    let limit = (chars * 4) as u64;
    // a partial read keeps what it got: a truncated head is still a head
    let _ = if zipped { flate2::read::GzDecoder::new(f).take(limit).read_to_end(&mut buf) } else { f.take(limit).read_to_end(&mut buf) };
    String::from_utf8_lossy(&buf).chars().take(chars).collect()
}

/// The attributes of the first `<svg`, `<g` or `<path` tag in `text`.
fn first_tag_attrs(text: &str) -> Vec<(String, String)> {
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(p) = text[i..].find('<') {
        let s = i + p + 1;
        let mut j = s;
        if j < b.len() && b[j] == b'/' {
            j += 1;
        }
        let name_end = (j..b.len()).find(|&k| !b[k].is_ascii_alphanumeric() && b[k] != b'_').unwrap_or(b.len());
        let name = &text[j..name_end];
        if matches!(name, "g" | "path" | "svg")
            && let Some(close) = text[name_end..].find('>')
        {
            let mut body = &text[name_end..name_end + close];
            if body.ends_with('/') {
                body = &body[..body.len() - 1];
            }
            return attrs(body);
        }
        i = s;
    }
    vec![]
}

/// The `name="value"` pairs in a tag body.
fn attrs(body: &str) -> Vec<(String, String)> {
    let b = body.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b':' || b[i] == b'-' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b':' || b[i] == b'-') {
                i += 1;
            }
            if i + 1 < b.len()
                && b[i] == b'='
                && b[i + 1] == b'"'
                && let Some(e) = body[i + 2..].find('"')
            {
                out.push((body[s..i].to_string(), body[i + 2..i + 2 + e].to_string()));
                i = i + 2 + e + 1;
                continue;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// An SVG's size in its own units: its viewBox's, or its width and height; None (said) without one.
pub fn svg_size(path: &Path) -> Option<(f64, f64)> {
    let head = svg_head(path, 4096);
    let a: HashMap<String, String> = first_tag_attrs(&head).into_iter().collect();
    let size = if let Some(vb) = a.get("viewBox") {
        let v: Vec<f64> = vb.replace(',', " ").split_whitespace().filter_map(|n| n.parse().ok()).collect();
        (v.len() == 4).then(|| (v[2], v[3]))
    } else {
        let f = |k: &str| a.get(k).and_then(|s| s.trim_end_matches(['p', 'x']).trim().parse::<f64>().ok());
        f("width").zip(f("height"))
    };
    let size = size.filter(|&(w, h)| (1e-3..=1e6).contains(&w) && (1e-3..=1e6).contains(&h) && (1e-4..=1e4).contains(&(w / h)));
    if size.is_none() {
        say_once(path, "no size (a viewBox, or width and height, of a sane shape): drawn as nothing");
    }
    size
}

/// An SVG number; a broken or endless one is left out as 0.
fn number(s: &str) -> f64 {
    match s.trim().parse::<f64>() {
        Ok(n) if n.is_finite() => n,
        _ => {
            fault(format!("number {s:?}"));
            0.0
        }
    }
}

/// An SVG colour as 0..1 RGB, or None for none: #rrggbb, #rgb or a few names.
pub fn svg_colour(value: Option<&str>) -> Option<(f64, f64, f64)> {
    let v = value?;
    let named = match v.trim().to_lowercase().as_str() {
        "white" => Some(Some("#ffffff")),
        "black" => Some(Some("#000000")),
        "red" => Some(Some("#ff0000")),
        "green" => Some(Some("#008000")),
        "blue" => Some(Some("#0000ff")),
        "grey" | "gray" => Some(Some("#808080")),
        "yellow" => Some(Some("#ffff00")),
        "transparent" | "none" => Some(None),
        _ => None,
    };
    let v: String = match named {
        Some(Some(h)) => h.to_string(),
        Some(None) => return None,
        None => v.trim().to_string(),
    };
    if v.is_empty() {
        return None;
    }
    let short: Vec<char> = v.chars().collect();
    let v = if short.len() == 4 && short[0] == '#' {
        short[1..].iter().fold(String::from("#"), |mut s, &c| {
            s.push(c);
            s.push(c);
            s
        })
    } else {
        v
    };
    match Colour::parse(&v) {
        Some(c) => Some(c.rgb()),
        None => {
            fault(format!("colour {v:?}"));
            None
        }
    }
}

/// Apply an SVG transform list to ctx: translate, scale, rotate, matrix.
pub fn svg_transform(c: &Context, value: Option<&str>) {
    let Some(value) = value else { return };
    let mut rest = value;
    // each "name(args)"
    while let Some(open) = rest.find('(') {
        let before = &rest[..open];
        let name_part = before.trim_end();
        let start = name_part.rfind(|ch: char| !(ch.is_alphanumeric() || ch == '_')).map_or(0, |k| k + 1);
        let name = &name_part[start..];
        let Some(close) = rest[open..].find(')') else { break };
        let args = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let v: Vec<f64> = args.trim().split(|ch: char| ch.is_whitespace() || ch == ',').filter(|n| !n.is_empty()).map(number).collect();
        if name.is_empty() || v.is_empty() || (name == "matrix" && v.len() < 6) {
            continue; // nothing to apply: a malformed transform is left out
        }
        match name {
            "translate" => c.translate(v[0], if v.len() > 1 { v[1] } else { 0.0 }),
            "scale" => c.scale(v[0], if v.len() > 1 { v[1] } else { v[0] }),
            "rotate" => {
                if v.len() == 3 {
                    c.translate(v[1], v[2]);
                }
                c.rotate(v[0].to_radians());
                if v.len() == 3 {
                    c.translate(-v[1], -v[2]);
                }
            }
            "matrix" => c.transform(Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5])),
            _ => fault(format!("transform {name:?} (only translate, scale, rotate and matrix are drawn)")),
        }
    }
}

/// A path traced once and cached in `traced`, so its points are cairo's fixed-point ones. A path that
/// cannot be read is traced as far as it goes, and said.
fn svg_traced<'a>(traced: &'a mut HashMap<String, Option<cairo::Path>>, d: &str) -> Option<&'a cairo::Path> {
    if !traced.contains_key(d) {
        let path = PathData::parse(d).unwrap_or_else(|e| {
            fault(format!("a path: {}", e.what));
            e.done
        });
        traced.insert(d.to_string(), path.traced());
    }
    traced[d].as_ref()
}

const INHERITED: &[&str] =
    &["fill", "stroke", "stroke-width", "stroke-linejoin", "fill-rule", "shape-rendering", "fill-opacity", "stroke-opacity", "stop-color"];

/// The id in url(#id); "" if it is not one.
fn url_id(v: &str) -> &str {
    match (v.find('#'), v.rfind(')')) {
        (Some(a), Some(b)) if a < b => &v[a + 1..b],
        _ => "",
    }
}

/// An element's attributes, without namespaced ones (none are drawn).
fn attrib<'a>(e: Node<'a, 'a>) -> impl Iterator<Item = (&'a str, &'a str)> {
    e.attributes().filter(|a| a.namespace().is_none()).map(|a| (a.name(), a.value()))
}

struct Drawer<'a, 'c> {
    c: &'c Context,
    defs: HashMap<&'a str, Node<'a, 'a>>,
    ids: Option<&'c HashSet<String>>,
    traced: HashMap<String, Option<cairo::Path>>,
    /// How deep in the tree the drawing is.
    depth: u32,
}

/// The deepest the drawing goes: deeper is left out.
const MAX_DEPTH: u32 = 200;

impl<'a, 'c> Drawer<'a, 'c> {
    /// A fill or stroke's source: a colour, or a gradient by url(#id); None for none.
    fn paint(&self, value: Option<&str>, alpha: f64) -> Option<Pattern> {
        if let Some(v) = value
            && v.starts_with("url(")
        {
            let g = *self.defs.get(url_id(v))?;
            if g.tag_name().name() != "linearGradient" {
                return None;
            }
            let f = |k: &str, d: f64| g.attribute(k).map_or(d, number);
            let lg = LinearGradient::new(f("x1", 0.0), f("y1", 0.0), f("x2", 1.0), f("y2", 0.0));
            for stop in g.children().filter(|n| n.is_element()) {
                if stop.tag_name().name() == "stop" {
                    let offset = stop.attribute("offset").unwrap_or("0");
                    let at = if let Some(p) = offset.strip_suffix('%') { number(p) / 100.0 } else { number(offset) };
                    let (r, gg, b) = svg_colour(Some(stop.attribute("stop-color").unwrap_or("black"))).unwrap_or((0.0, 0.0, 0.0));
                    lg.add_color_stop_rgba(at, r, gg, b, stop.attribute("stop-opacity").map_or(1.0, number) * alpha);
                }
            }
            let p: &Pattern = &lg;
            return Some(p.clone());
        }
        let (r, g, b) = svg_colour(value)?;
        let s = SolidPattern::from_rgba(r, g, b, alpha);
        let p: &Pattern = &s;
        Some(p.clone())
    }

    /// Trace a path or a rect.
    fn shape(&mut self, e: Node) {
        match e.tag_name().name() {
            "path" => {
                if let Some(p) = svg_traced(&mut self.traced, e.attribute("d").unwrap_or("")) {
                    self.c.append_path(p);
                }
            }
            "rect" => {
                let f = |k: &str| e.attribute(k).map_or(0.0, number);
                self.c.rectangle(f("x"), f("y"), f("width"), f("height"));
            }
            _ => {}
        }
    }

    fn draw(&mut self, e: Node<'a, 'a>, parent: &HashMap<&'a str, &'a str>, inside: bool) {
        if self.depth >= MAX_DEPTH {
            fault(format!("elements nested over {MAX_DEPTH} deep (a mask that holds itself?)"));
            return;
        }
        self.depth += 1;
        self.drawn(e, parent, inside);
        self.depth -= 1;
    }

    fn drawn(&mut self, e: Node<'a, 'a>, parent: &HashMap<&'a str, &'a str>, inside: bool) {
        let kind = e.tag_name().name();
        if matches!(kind, "defs" | "title" | "desc" | "clipPath" | "mask" | "linearGradient" | "metadata" | "style") {
            return;
        }
        let mut style: HashMap<&'a str, &'a str> = parent.iter().filter(|(k, _)| INHERITED.contains(k)).map(|(k, v)| (*k, *v)).collect();
        style.extend(attrib(e));
        let inside = inside || self.ids.is_none_or(|ids| e.attribute("id").is_some_and(|i| ids.contains(i)));
        let c = self.c;
        let _ = c.save();
        svg_transform(c, e.attribute("transform"));
        if let Some(clip) = e.attribute("clip-path").filter(|v| v.starts_with("url(")) {
            if let Some(cp) = self.defs.get(url_id(clip)).copied() {
                for ch in cp.children().filter(|n| n.is_element()) {
                    self.shape(ch);
                    c.set_fill_rule(if ch.attribute("clip-rule") == Some("evenodd") { FillRule::EvenOdd } else { FillRule::Winding });
                }
            }
            c.clip();
        }
        let mask = e.attribute("mask").filter(|v| v.starts_with("url("));
        let group = mask.is_some() || (kind == "g" && e.attribute("opacity").map_or(1.0, number) < 1.0);
        if group {
            c.push_group();
        }
        if kind == "svg" || kind == "g" {
            for ch in e.children().filter(|n| n.is_element()) {
                self.draw(ch, &style, inside);
            }
        } else if (kind == "path" || kind == "rect") && inside {
            c.new_path();
            self.shape(e);
            c.set_antialias(if style.get("shape-rendering") == Some(&"crispEdges") { Antialias::None } else { Antialias::Default });
            c.set_fill_rule(if style.get("fill-rule") == Some(&"evenodd") { FillRule::EvenOdd } else { FillRule::Winding });
            let alpha = e.attribute("opacity").map_or(1.0, number);
            let fill = self.paint(Some(style.get("fill").copied().unwrap_or("black")), alpha * style.get("fill-opacity").map_or(1.0, |v| number(v)));
            let stroke = self.paint(Some(style.get("stroke").copied().unwrap_or("none")), alpha * style.get("stroke-opacity").map_or(1.0, |v| number(v)));
            if let Some(f) = fill {
                let _ = c.set_source(&f);
                let _ = c.fill_preserve();
            }
            if let Some(s) = stroke {
                let _ = c.set_source(&s);
                c.set_line_width(style.get("stroke-width").map_or(1.0, |v| number(v)));
                c.set_line_join(match style.get("stroke-linejoin").copied() {
                    Some("round") => LineJoin::Round,
                    Some("bevel") => LineJoin::Bevel,
                    _ => LineJoin::Miter,
                });
                let _ = c.stroke_preserve();
            }
            c.new_path();
            c.set_antialias(Antialias::Default);
        }
        if group {
            let Ok(content) = c.pop_group() else {
                let _ = c.restore();
                return;
            };
            if let Some(m) = mask {
                c.push_group();
                if let Some(md) = self.defs.get(url_id(m)).copied() {
                    for ch in md.children().filter(|n| n.is_element()) {
                        self.draw(ch, &HashMap::new(), true);
                    }
                }
                if let Ok(shade) = c.pop_group() {
                    let _ = c.set_source(&content);
                    let _ = c.mask(&shade);
                }
            } else {
                let _ = c.set_source(&content);
                let _ = c.paint_with_alpha(e.attribute("opacity").map_or(1.0, number));
            }
        }
        let _ = c.restore();
    }
}

/// Draw an SVG onto ctx in its own units; with ids, only those elements and their contents.
pub fn draw_svg(c: &Context, path: &Path, ids: Option<&HashSet<String>>) {
    let Some(text) = svg_open(path) else { return };
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = match roxmltree::Document::parse_with_options(&text, opts) {
        Ok(d) => d,
        Err(e) => return say_once(path, &format!("{e}: drawn as nothing")),
    };
    let root = doc.root_element();
    let mut defs = HashMap::new();
    for e in root.descendants().filter(|n| n.is_element()) {
        if let Some(id) = e.attribute("id").filter(|i| !i.is_empty()) {
            defs.insert(id, e);
        }
    }
    FAULT.with(|f| f.borrow_mut().take());
    let mut d = Drawer { c, defs, ids, traced: HashMap::new(), depth: 0 };
    d.draw(root, &HashMap::new(), false);
    if let Some(what) = FAULT.with(|f| f.borrow_mut().take()) {
        say_once(path, &format!("{what}: left out"));
    }
}

/// An SVG drawn on a width x height surface, scaled by `scale` and moved to (x, y); with ids, only
/// those elements.
pub fn svg_layer(path: &Path, width: i32, height: i32, scale: f64, x: f64, y: f64, ids: Option<&[&str]>) -> ImageSurface {
    let s = crate::util::surface(width, height);
    {
        let c = crate::util::ctx(&s);
        c.translate(x, y);
        c.scale(scale, scale);
        let set: Option<HashSet<String>> = ids.map(|i| i.iter().map(|s| s.to_string()).collect());
        draw_svg(&c, path, set.as_ref());
    }
    s
}

// --- the cache -------------------------------------------------------------------------------------
//
// Pictures made from an SVG for a screen size, cached in $XDG_CACHE_HOME/retro-wall/THEME-PLACE/ (or
// ~/.cache/retro-wall/...) as STEM-HASH-WxH-KIND-TAG: HASH of the SVG file and of the program that
// drew it, the screen size, which picture, and a hash of its other inputs. A changed SVG or another
// build of the program makes another name, so a stale picture is never read. A new picture deletes
// others of its kind and size, and those of other hashes. Without a writable cache, pictures are just
// made.

pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

thread_local! {
    static FILE_HASH: RefCell<HashMap<PathBuf, u64>> = std::cell::RefCell::new(HashMap::new());
}

fn file_hash(path: &Path) -> u64 {
    FILE_HASH.with(|m| *m.borrow_mut().entry(path.to_path_buf()).or_insert_with(|| fnv(&crate::files::read(path, crate::files::Kind::Svg).unwrap_or_default())))
}

/// This build of the program: its version, and its executable's size and time. Another build draws
/// differently, perhaps, so its pictures are its own.
fn build() -> u64 {
    static BUILD: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BUILD.get_or_init(|| {
        let exe = std::env::current_exe().ok().and_then(|e| e.metadata().ok());
        let time = exe.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        fnv(format!("{} {} {time}", env!("CARGO_PKG_VERSION"), exe.map_or(0, |m| m.len())).as_bytes())
    })
}

/// The cache's root: $XDG_CACHE_HOME/retro-wall if that is set (and absolute), else
/// ~/.cache/retro-wall; None without either.
pub fn cache_root(xdg_cache_home: Option<&std::ffi::OsStr>, home: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let base = match xdg_cache_home.map(Path::new).filter(|p| p.is_absolute()) {
        Some(x) => x.to_path_buf(),
        None => Path::new(home.filter(|h| !h.is_empty())?).join(".cache"),
    };
    Some(base.join("retro-wall"))
}

/// retro-wall's cache directory: [`cache_root`] of the environment; on Windows
/// %LOCALAPPDATA%\retro-wall\cache.
pub fn cache_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
        return Some(PathBuf::from(local).join("retro-wall").join("cache"));
    }
    cache_root(std::env::var_os("XDG_CACHE_HOME").as_deref(), std::env::var_os("HOME").as_deref())
}

/// The cache directory for the SVG at `path` (its theme's) and the name prefix STEM-HASH-; None
/// without a cache.
fn cache_place(path: &Path) -> Option<(PathBuf, String)> {
    let root = cache_dir()?;
    let p = path.canonicalize().ok()?;
    let dir = p.parent()?;
    let theme = format!("{}-{:08x}", dir.file_name()?.to_string_lossy(), fnv(dir.as_os_str().as_encoded_bytes()) as u32);
    let stem = p.file_name()?.to_string_lossy().split('.').next()?.to_string();
    Some((root.join(theme), format!("{stem}-{:08x}-", (file_hash(path) ^ build()) as u32)))
}

/// Write to a per-process temporary name, then rename: readers never see a partial file.
fn write_whole(at: &Path, write: impl FnOnce(&Path) -> bool) -> bool {
    let part = at.with_extension(format!("{}.part", std::process::id()));
    if write(&part) && std::fs::rename(&part, at).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&part);
    false
}

/// The cache file of `path` at width x height as `kind` with `tag`, and the prefix it shares with
/// others of its kind and size.
fn kept_name(path: &Path, width: i32, height: i32, kind: &str, tag: &str, ext: &str) -> Option<(PathBuf, String)> {
    let (dir, start) = cache_place(path)?;
    let tag = fnv(tag.as_bytes()) & 0xffffff;
    let same = format!("{start}{width}x{height}-{kind}-");
    Some((dir.join(format!("{same}{tag:06x}.{ext}")), same))
}

/// Delete others of a kind and size, and anything of an older version of the SVG (another hash).
fn prune(dir: &Path, same: &str, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let (stem, hash) = {
        let parts: Vec<&str> = same.trim_end_matches('-').rsplitn(4, '-').collect(); // kind, WxH, hash, stem
        (parts[3].to_string(), parts[2].to_string())
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if p == keep || name.contains(".part") {
            continue;
        }
        let other_hash = name.strip_prefix(&format!("{stem}-")).and_then(|r| r.get(..8)).is_some_and(|h| h != hash && h.bytes().all(|b| b.is_ascii_hexdigit()));
        if name.starts_with(same) || other_hash {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// A picture made from the SVG `path`: read from the cache, else made and cached. Stored as deflated
/// premultiplied pixels, so it reads back exactly.
pub fn kept(path: &Path, width: i32, height: i32, kind: &str, tag: &str, make: impl FnOnce() -> ImageSurface) -> ImageSurface {
    let place = kept_name(path, width, height, kind, tag, "raw");
    if let Some((at, _)) = &place
        && let Some(s) = crate::util::read_raw(at)
    {
        return s;
    }
    let mut s = make();
    if let Some((at, same)) = place
        && std::fs::create_dir_all(at.parent().unwrap()).is_ok()
        && write_whole(&at, |part| crate::util::write_raw(&mut s, part))
    {
        prune(at.parent().unwrap(), &same, &at);
    }
    s
}

/// As `kept`, for a text value made from a picture.
pub fn kept_value(path: &Path, width: i32, height: i32, kind: &str, tag: &str, make: impl FnOnce() -> String) -> String {
    let place = kept_name(path, width, height, kind, tag, "txt");
    if let Some((at, _)) = &place
        && let Ok(v) = std::fs::read_to_string(at)
    {
        return v;
    }
    let v = make();
    if let Some((at, same)) = place
        && std::fs::create_dir_all(at.parent().unwrap()).is_ok()
        && write_whole(&at, |part| std::fs::write(part, &v).is_ok())
    {
        prune(at.parent().unwrap(), &same, &at);
    }
    v
}
