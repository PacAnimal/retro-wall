//! The desktop environments that draw a wallpaper under ours, through their own command-line tools:
//! which one runs, showing a picture, and saving and restoring the user's wallpaper settings exactly.
//!
//! | Desktop | Settings | Tool |
//! |---|---|---|
//! | XFCE | xfconf's xfce4-desktop channel, under /backdrop, typed | xfconf-query, gdbus |
//! | GNOME, Cinnamon, MATE | their background schema: the keys the user set | gsettings, dconf |
//! | LXDE | pcmanfm's desktop-items files, whole | pcmanfm |
//! | KDE Plasma | each desktop's wallpaper plugin and picture settings | qdbus or dbus-send |

use crate::platform::{Desktop, Screen};
use std::collections::HashSet;
use std::process::{Command, Stdio};

/// (name, a word of XDG_CURRENT_DESKTOP, the program that draws its wallpaper)
const DESKTOPS: [(&str, &str, &str); 6] = [
    ("xfce", "XFCE", "xfdesktop"),
    ("kde", "KDE", "plasmashell"),
    ("gnome", "GNOME", "gnome-shell"),
    ("cinnamon", "X-CINNAMON", "cinnamon"),
    ("mate", "MATE", "caja"),
    ("lxde", "LXDE", "pcmanfm"),
];

/// The desktop that draws the wallpaper, from XDG_CURRENT_DESKTOP's words and the programs running:
/// the named one if its wallpaper program runs, else any whose program runs.
pub fn detect(current_desktop: &str, running: &HashSet<String>) -> Option<&'static str> {
    let words: HashSet<String> = current_desktop.split(':').map(|w| w.trim().to_uppercase()).collect();
    for (d, word, program) in DESKTOPS {
        if (words.contains(word) || (d == "cinnamon" && words.contains("CINNAMON"))) && running.contains(program) {
            return Some(d);
        }
    }
    DESKTOPS.iter().find(|(_, _, p)| running.contains(*p)).map(|(d, _, _)| *d)
}

pub fn named(name: &str) -> Option<Box<dyn Desktop>> {
    Some(match name {
        "xfce" => Box::new(Xfce),
        "gnome" => {
            Box::new(Gsettings { name: "gnome", schema: "org.gnome.desktop.background", path: "/org/gnome/desktop/background/", key: "picture-uri", uri: true })
        }
        "cinnamon" => Box::new(Gsettings {
            name: "cinnamon",
            schema: "org.cinnamon.desktop.background",
            path: "/org/cinnamon/desktop/background/",
            key: "picture-uri",
            uri: true,
        }),
        "mate" => {
            Box::new(Gsettings { name: "mate", schema: "org.mate.background", path: "/org/mate/desktop/background/", key: "picture-filename", uri: false })
        }
        "lxde" => Box::new(Lxde),
        "kde" => Box::new(Kde),
        _ => return None,
    })
}

/// A command's output, trimmed, if it ran and succeeded.
fn run(cmd: &[&str]) -> Option<String> {
    let o = Command::new(cmd[0]).args(&cmd[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// A command, or what went wrong with it.
fn must(cmd: &[&str]) -> Result<String, String> {
    let o = Command::new(cmd[0]).args(&cmd[1..]).stdin(Stdio::null()).output().map_err(|e| format!("{}: {e}", cmd[0]))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(format!("{}: {}", cmd.join(" "), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

fn found(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// (red, green, blue), 0 to 1, of #rrggbb.
fn rgb(colour: &str) -> (f64, f64, f64) {
    let c = |i: usize| colour.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(0) as f64 / 255.0;
    (c(1), c(3), c(5))
}

// --- XFCE -----------------------------------------------------------------------------------------

struct Xfce;

const XFCONF: [&str; 6] = ["gdbus", "call", "--session", "--dest", "org.xfce.Xfconf", "--object-path"];

fn xfconf(method: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd: Vec<&str> = XFCONF.to_vec();
    let method = format!("org.xfce.Xfconf.{method}");
    cmd.extend(["/org/xfce/Xfconf", "--method", &method, "xfce4-desktop"]);
    cmd.extend(args);
    must(&cmd)
}

impl Desktop for Xfce {
    fn name(&self) -> &'static str {
        "xfce"
    }

    fn per_monitor(&self) -> bool {
        true
    }

    fn show(&self, s: &Screen, path: &str, colour: &str) {
        let base = format!("/backdrop/screen0/monitor{}/workspace0", s.name);
        let q = |prop: &str, kind: &str, value: &str| run(&["xfconf-query", "-c", "xfce4-desktop", "-p", prop, "--create", "-t", kind, "-s", value]);
        if run(&["xfconf-query", "-c", "xfce4-desktop", "-p", &format!("{base}/last-image")]).as_deref() == Some(path) {
            return; // shown already: setting it again makes xfdesktop redraw
        }
        // one picture on every workspace: single-workspace mode shows workspace 0's
        q("/backdrop/single-workspace-mode", "bool", "true");
        q("/backdrop/single-workspace-number", "int", "0");
        let (r, g, b) = rgb(colour);
        let (r, g, b) = (format!("{r:.6}"), format!("{g:.6}"), format!("{b:.6}"));
        run(&[
            "xfconf-query",
            "-c",
            "xfce4-desktop",
            "-p",
            &format!("{base}/rgba1"),
            "--create",
            "-t",
            "double",
            "-s",
            &r,
            "-t",
            "double",
            "-s",
            &g,
            "-t",
            "double",
            "-s",
            &b,
            "-t",
            "double",
            "-s",
            "1.000000",
        ]);
        q(&format!("{base}/color-style"), "int", "0");
        q(&format!("{base}/last-image"), "string", path);
        q(&format!("{base}/image-style"), "int", "1");
    }

    /// Lines of property, tab, its value as GVariant text.
    fn save(&self) -> Result<String, String> {
        Ok(xfce_read()?.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect())
    }

    fn restore(&self, saved: &str) -> Result<(), String> {
        let wanted: Vec<(&str, &str)> = saved.lines().filter_map(|l| l.split_once('\t')).collect();
        for (prop, _) in xfce_read()? {
            if !wanted.iter().any(|(k, _)| *k == prop) {
                xfconf("ResetProperty", &[&prop, "false"])?;
            }
        }
        for (prop, value) in wanted {
            xfconf("SetProperty", &[prop, &format!("<{value}>")])?;
        }
        Ok(())
    }
}

/// Every property under /backdrop, sorted, with its value.
fn xfce_read() -> Result<Vec<(String, String)>, String> {
    match xfconf("GetAllProperties", &["/backdrop"]) {
        Ok(out) => properties(&out).ok_or_else(|| format!("xfconf's answer: {out}")),
        Err(e) if e.contains("does not exist") || e.contains("not found") => Ok(vec![]),
        Err(e) => Err(e),
    }
}

/// gdbus's answer to GetAllProperties, `({'/p': <value>, ...},)`, as (property, value) pairs, sorted.
pub fn properties(text: &str) -> Option<Vec<(String, String)>> {
    let t = text.trim();
    let inner = t.strip_prefix('(')?.strip_suffix(",)")?.trim();
    let inner = inner.strip_prefix("@a{sv}").unwrap_or(inner).trim();
    let body = inner.strip_prefix('{')?.strip_suffix('}')?;
    let mut out = vec![];
    for entry in split_top(body, ',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (key, value) = split_once_top(entry, ':')?;
        let key = unquote(key.trim())?;
        let value = value.trim().strip_prefix('<')?.strip_suffix('>')?.trim();
        out.push((key, value.to_string()));
    }
    out.sort();
    Some(out)
}

/// Pieces of GVariant text separated by `sep` outside quotes and brackets.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut out = vec![];
    let (mut depth, mut quote, mut escaped, mut start) = (0i32, None::<char>, false, 0);
    for (i, c) in text.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            _ if c == sep && depth == 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

fn split_once_top(text: &str, sep: char) -> Option<(&str, &str)> {
    let first = split_top(text, sep).into_iter().next()?;
    Some((first, &text[first.len() + sep.len_utf8()..]))
}

/// A GVariant string literal's text.
fn unquote(s: &str) -> Option<String> {
    let q = s.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let body = s.strip_prefix(q)?.strip_suffix(q)?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' { chars.next()? } else { c });
    }
    Some(out)
}

// --- GNOME, Cinnamon, MATE ------------------------------------------------------------------------

struct Gsettings {
    name: &'static str,
    schema: &'static str,
    path: &'static str,
    key: &'static str,
    uri: bool,
}

impl Desktop for Gsettings {
    fn name(&self) -> &'static str {
        self.name
    }

    fn show(&self, _s: &Screen, path: &str, colour: &str) {
        let value = if self.uri { format!("file://{path}") } else { path.to_string() };
        if run(&["gsettings", "get", self.schema, self.key]).is_some_and(|v| v.trim_matches('\'') == value) {
            return;
        }
        run(&["gsettings", "set", self.schema, "primary-color", colour]);
        run(&["gsettings", "set", self.schema, "color-shading-type", "solid"]);
        run(&["gsettings", "set", self.schema, "picture-options", "centered"]);
        run(&["gsettings", "set", self.schema, self.key, &value]);
        if self.name == "gnome" && run(&["gsettings", "list-keys", self.schema]).is_some_and(|k| k.lines().any(|l| l == "picture-uri-dark")) {
            run(&["gsettings", "set", self.schema, "picture-uri-dark", &value]);
        }
    }

    /// With dconf: the keys the user set, as `dconf dump` has them. Without: every key's value.
    fn save(&self) -> Result<String, String> {
        if found("dconf") {
            return Ok(format!("via\tdconf\n{}\n", must(&["dconf", "dump", self.path])?));
        }
        let all = must(&["gsettings", "list-recursively", self.schema])?;
        let mut out = "via\tgsettings\n".to_string();
        for line in all.lines() {
            if let Some((key, value)) = line.split_once(' ').and_then(|(_, rest)| rest.split_once(' ')) {
                out += &format!("{key}\t{value}\n");
            }
        }
        Ok(out)
    }

    fn restore(&self, saved: &str) -> Result<(), String> {
        let (via, body) = saved.split_once('\n').unwrap_or((saved, ""));
        match via {
            "via\tdconf" => {
                must(&["dconf", "reset", "-f", self.path])?;
                let mut c = Command::new("dconf").args(["load", self.path]).stdin(Stdio::piped()).spawn().map_err(|e| format!("dconf: {e}"))?;
                if let Some(mut i) = c.stdin.take() {
                    use std::io::Write;
                    i.write_all(body.as_bytes()).map_err(|e| format!("dconf: {e}"))?;
                }
                c.wait().ok().filter(|s| s.success()).map(|_| ()).ok_or("dconf load failed".to_string())
            }
            "via\tgsettings" => {
                for (key, value) in body.lines().filter_map(|l| l.split_once('\t')) {
                    must(&["gsettings", "set", self.schema, key, value])?;
                }
                Ok(())
            }
            _ => Err("not a saved GSettings background".into()),
        }
    }
}

// --- LXDE -----------------------------------------------------------------------------------------

struct Lxde;

fn pcmanfm_dir() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".config/pcmanfm/LXDE")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(2).then_some(())?;
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn desktop_files() -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(pcmanfm_dir())
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("desktop-items-") && n.ends_with(".conf")
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

impl Desktop for Lxde {
    fn name(&self) -> &'static str {
        "lxde"
    }

    fn show(&self, _s: &Screen, path: &str, _colour: &str) {
        run(&["pcmanfm", &format!("--set-wallpaper={path}"), "--wallpaper-mode=center"]);
    }

    /// Lines of file name, tab, its bytes in hex.
    fn save(&self) -> Result<String, String> {
        let mut out = String::new();
        for p in desktop_files() {
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out += &format!("{}\t{}\n", p.file_name().unwrap_or_default().to_string_lossy(), hex(&bytes));
        }
        Ok(out)
    }

    /// pcmanfm told the wallpaper again (it reads its files only at start), then its files put back as
    /// they were, over what it writes.
    fn restore(&self, saved: &str) -> Result<(), String> {
        let files: Vec<(String, Vec<u8>)> = saved
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(n, _)| !n.contains('/') && n.starts_with("desktop-items-"))
            .map(|(n, h)| unhex(h).map(|b| (n.to_string(), b)).ok_or(format!("{n}: not hex")))
            .collect::<Result<_, _>>()?;
        if let Some((_, first)) = files.iter().find(|(n, _)| n == "desktop-items-0.conf") {
            let text = String::from_utf8_lossy(first);
            let get = |k: &str| text.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()));
            if let Some(w) = get("wallpaper") {
                let mode = get("wallpaper_mode").unwrap_or("stretch".into());
                must(&["pcmanfm", &format!("--set-wallpaper={w}"), &format!("--wallpaper-mode={mode}")])?;
                std::thread::sleep(std::time::Duration::from_secs(1)); // its own write, first
            }
        }
        for p in desktop_files() {
            if !files.iter().any(|(n, _)| p.file_name().is_some_and(|f| f.to_string_lossy() == *n)) {
                let _ = std::fs::remove_file(&p);
            }
        }
        let dir = pcmanfm_dir();
        if !files.is_empty() {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        for (n, bytes) in files {
            std::fs::write(dir.join(&n), bytes).map_err(|e| format!("{n}: {e}"))?;
        }
        Ok(())
    }
}

// --- KDE Plasma -----------------------------------------------------------------------------------

struct Kde;

const KDE_KEYS: [&str; 3] = ["Image", "FillMode", "Color"];

/// Run a script in Plasma's shell: what it printed.
fn kde_script(script: &str) -> Result<String, String> {
    for q in ["qdbus6", "qdbus-qt6", "qdbus", "qdbus-qt5"] {
        if found(q) {
            return must(&[q, "org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell.evaluateScript", script]);
        }
    }
    let out = must(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.kde.plasmashell",
        "/PlasmaShell",
        "org.kde.PlasmaShell.evaluateScript",
        &format!("string:{script}"),
    ])?;
    Ok(dbus_send_string(&out).unwrap_or_default())
}

/// The string in dbus-send --print-reply's answer.
pub fn dbus_send_string(out: &str) -> Option<String> {
    let at = out.find("string \"")?;
    let rest = &out[at + 8..];
    Some(rest.strip_suffix('"').unwrap_or(rest).to_string())
}

/// A JavaScript string literal.
fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

impl Desktop for Kde {
    fn name(&self) -> &'static str {
        "kde"
    }

    fn per_monitor(&self) -> bool {
        true
    }

    /// The desktop on the screen at the monitor's place.
    fn show(&self, s: &Screen, path: &str, colour: &str) {
        let script = format!(
            "desktops().forEach(function (d) {{ var g = screenGeometry(d.screen); if (g.x != {} || g.y != {}) return; \
            d.wallpaperPlugin = 'org.kde.image'; d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; \
            d.writeConfig('Image', {}); d.writeConfig('FillMode', 6); d.writeConfig('Color', {}); }})",
            s.x,
            s.y,
            js(&format!("file://{path}")),
            js(colour)
        );
        if kde_script(&script).is_err() {
            run(&["plasma-apply-wallpaperimage", path]);
        }
    }

    /// Lines of desktop index, tab, its plugin and picture settings as JSON.
    fn save(&self) -> Result<String, String> {
        let keys = serde_json::to_string(&KDE_KEYS).unwrap_or_default();
        let text = kde_script(&format!(
            "var out = []; desktops().forEach(function (d) {{ var one = {{plugin: d.wallpaperPlugin}}; \
            d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; \
            {keys}.forEach(function (k) {{ one[k] = String(d.readConfig(k)); }}); out.push(one); }}); print(JSON.stringify(out));"
        ))?;
        let all: Vec<serde_json::Value> = serde_json::from_str(text.trim()).map_err(|e| format!("Plasma's answer: {e}"))?;
        Ok(all.iter().enumerate().map(|(i, one)| format!("{i}\t{one}\n")).collect())
    }

    fn restore(&self, saved: &str) -> Result<(), String> {
        for (i, one) in saved.lines().filter_map(|l| l.split_once('\t')) {
            let i: usize = i.parse().map_err(|_| format!("{i}: not a desktop"))?;
            let one: serde_json::Value = serde_json::from_str(one).map_err(|e| e.to_string())?;
            let sets: String =
                KDE_KEYS.iter().filter_map(|k| one.get(k).and_then(|v| v.as_str()).map(|v| format!("d.writeConfig({}, {}); ", js(k), js(v)))).collect();
            let plugin = one.get("plugin").and_then(|v| v.as_str()).unwrap_or("org.kde.image");
            kde_script(&format!(
                "var d = desktops()[{i}]; if (d) {{ d.wallpaperPlugin = 'org.kde.image'; \
                d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; {sets}d.wallpaperPlugin = {}; }}",
                js(plugin)
            ))?;
        }
        Ok(())
    }
}
