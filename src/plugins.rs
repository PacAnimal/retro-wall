//! Plugins are programs in any language, talking in lines of JSON. A broken one is reported and left
//! out (or keeps its last value, dimmed); nothing a plugin does can stop or stall the wall.
//!
//! - **Sources** (measurements): an executable in `~/.config/retro/sources/` (its file name is the
//!   source's name), or `[source.NAME] command = [...]` in retro.toml. Run every `every` seconds
//!   (default 2), it prints a number 0..100, or `{"value": 37, "indicator": true}`, or `{"text": "..."}`.
//!   A first line `{"stream": true}` (or `stream = true`) keeps it running, a line per new reading.
//! - **Faces** (instruments): an executable in `~/.config/retro/faces/` (its file name is a widget
//!   type). It keeps running; for each picture it reads a request line and answers a list of drawing
//!   operations (`face_ops`), within two seconds.

use crate::sources::Reading;
use serde_json::{Value as J, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub fn sources_dir() -> PathBuf {
    crate::config::config_dir().join("sources")
}

pub fn faces_dir() -> PathBuf {
    crate::config::config_dir().join("faces")
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file() && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Windows has no execute bit: a program is a file the shell runs by its extension.
#[cfg(windows)]
fn executable(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    p.is_file() && ext.is_some_and(|e| ["exe", "com", "bat", "cmd"].contains(&e.as_str()))
}

/// A shell line as a command.
fn shell(line: &str) -> Vec<String> {
    if cfg!(windows) { vec!["cmd".into(), "/C".into(), line.into()] } else { vec!["/bin/sh".into(), "-c".into(), line.into()] }
}

/// How to run a plugin source.
#[derive(Clone, Debug)]
pub struct SourceSpec {
    pub name: String,
    pub command: Vec<String>,
    pub every: f64,
    pub stream: bool,
}

fn expand(s: &str) -> String {
    match s.strip_prefix("~/") {
        Some(rest) => crate::config::home().join(rest).to_string_lossy().into_owned(),
        None => s.to_string(),
    }
}

/// The longest a source's `every` may be: a day.
const MAX_EVERY: f64 = 86_400.0;

/// A `[source.NAME]` table as a spec, or what is wrong with it.
fn spec_of(name: &str, t: &toml::Value) -> Result<SourceSpec, String> {
    let t = t.as_table().ok_or("a table: command, every, stream")?;
    if let Some(k) = t.keys().find(|k| !["command", "every", "stream"].contains(&k.as_str())) {
        return Err(format!("unknown {k} (it has command, every, stream)"));
    }
    let command = match t.get("command") {
        Some(toml::Value::String(s)) if !s.trim().is_empty() => shell(s),
        Some(toml::Value::Array(a)) if !a.is_empty() && a.iter().all(|v| v.is_str()) => a.iter().map(|v| expand(v.as_str().unwrap())).collect(),
        _ => return Err("command is a program and its arguments, [\"prog\", \"arg\"], or a shell line".into()),
    };
    let every = match t.get("every") {
        None => 2.0,
        Some(v) => match v.as_float().or(v.as_integer().map(|i| i as f64)) {
            Some(e) if (0.1..=MAX_EVERY).contains(&e) => e,
            Some(e) => return Err(format!("every is {}: seconds, 0.1 to {MAX_EVERY}", crate::util::num(e))),
            None => return Err(format!("every is {v}: seconds, 0.1 to {MAX_EVERY}")),
        },
    };
    let stream = match t.get("stream") {
        None => false,
        Some(toml::Value::Boolean(b)) => *b,
        Some(v) => return Err(format!("stream is {v}: true or false")),
    };
    Ok(SourceSpec { name: name.to_string(), command, every, stream })
}

/// Check a `[source.NAME]` table, for the config loader.
pub fn check_source(name: &str, t: &toml::Value) -> Result<(), String> {
    spec_of(name, t).map(|_| ())
}

/// Every plugin source: the executables in sources/, and retro.toml's tables (checked when it loaded).
pub fn source_specs(config: &toml::Table) -> Vec<SourceSpec> {
    let mut out: Vec<SourceSpec> = vec![];
    if let Ok(dir) = std::fs::read_dir(sources_dir()) {
        let mut files: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| executable(p)).collect();
        files.sort();
        for p in files {
            let name = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            out.push(SourceSpec { name, command: vec![p.to_string_lossy().into_owned()], every: 2.0, stream: false });
        }
    }
    for (name, t) in config {
        if let Ok(spec) = spec_of(name, t) {
            out.retain(|s| s.name != *name);
            out.push(spec);
        }
    }
    out
}

/// A line of a source's output as a reading: a number, or JSON.
pub fn parse_reading(line: &str) -> Option<Reading> {
    let line = line.trim();
    if let Ok(v) = line.parse::<f64>() {
        return v.is_finite().then(|| Reading { value: v.clamp(0.0, 100.0), ..Default::default() });
    }
    match serde_json::from_str::<J>(line).ok()? {
        J::Number(n) => Some(Reading { value: n.as_f64()?.clamp(0.0, 100.0), ..Default::default() }),
        J::Object(o) => Some(Reading {
            value: o.get("value").and_then(J::as_f64).unwrap_or(0.0).clamp(0.0, 100.0),
            indicator: o.get("indicator").and_then(J::as_bool).unwrap_or(false),
            text: o.get("text").and_then(J::as_str).map(|s| Arc::from(crate::util::printable(s))),
            stale: false,
        }),
        J::String(s) => Some(Reading { text: Some(Arc::from(crate::util::printable(&s))), ..Default::default() }),
        _ => None,
    }
}

fn is_stream_marker(line: &str) -> bool {
    serde_json::from_str::<J>(line.trim()).ok().and_then(|j| j.get("stream").and_then(J::as_bool)).unwrap_or(false)
}

/// Lines from a child's stdout, each waited for no longer than a deadline.
struct Lines {
    #[cfg(unix)]
    out: std::process::ChildStdout,
    /// Windows pipes cannot be polled: a thread reads them, a chunk at a time.
    #[cfg(windows)]
    out: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
}

impl Lines {
    #[cfg(unix)]
    fn new(out: std::process::ChildStdout) -> Lines {
        Lines { out, buf: vec![] }
    }

    #[cfg(windows)]
    fn new(mut out: std::process::ChildStdout) -> Lines {
        // bounded: a paused wall stops reading, and the child then blocks on its pipe
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let _ = std::thread::Builder::new().name("plugin output".into()).spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n @ 1..) = out.read(&mut chunk) {
                if tx.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Lines { out: rx, buf: vec![] }
    }

    /// The next chunk of output, waiting `wait` ms at most (-1: for ever); None at the end, on an
    /// error, or on time.
    #[cfg(unix)]
    fn chunk(&mut self, wait: i32) -> Option<Vec<u8>> {
        use std::os::fd::AsRawFd;
        let mut pfd = libc::pollfd { fd: self.out.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one pollfd on our own pipe's descriptor, open while `self.out` is; count 1.
        if unsafe { libc::poll(&mut pfd, 1, wait) } <= 0 {
            return None;
        }
        let mut chunk = [0u8; 4096];
        match self.out.read(&mut chunk) {
            Ok(0) | Err(_) => None,
            Ok(n) => Some(chunk[..n].to_vec()),
        }
    }

    #[cfg(windows)]
    fn chunk(&mut self, wait: i32) -> Option<Vec<u8>> {
        match u64::try_from(wait) {
            Ok(ms) => self.out.recv_timeout(Duration::from_millis(ms)).ok(),
            Err(_) => self.out.recv().ok(),
        }
    }

    /// The next line, or None at the end, on an error, or when `deadline` passes first.
    fn next(&mut self, deadline: Option<Instant>) -> Option<String> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=i).collect();
                return Some(String::from_utf8_lossy(&line[..i]).trim_end_matches('\r').to_string());
            }
            let wait = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    left.as_millis().min(i32::MAX as u128) as i32
                }
                None => -1,
            };
            let chunk = self.chunk(wait)?;
            self.buf.extend_from_slice(&chunk);
            if self.buf.len() > 1 << 20 {
                return None; // a megabyte and no line
            }
        }
    }
}

fn mark_stale(readings: &Mutex<HashMap<String, Reading>>, name: &str) {
    readings.lock().unwrap().entry(name.to_string()).or_default().stale = true;
}

/// A plugin in a process group of its own, so that what it starts is stopped with it. On Windows,
/// without a console window (retro-wall has none to lend it); its job object is made in `spawn`.
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    c
}

/// Start a plugin; on Windows in a job object of its own, the process group's counterpart.
fn spawn(c: &mut Command) -> std::io::Result<Child> {
    let child = c.spawn()?;
    #[cfg(windows)]
    jobs::adopt(&child);
    Ok(child)
}

/// Each plugin's job object, by its process ID: closed, it ends the plugin and all it started.
#[cfg(windows)]
mod jobs {
    use std::os::windows::io::AsRawHandle;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    /// Handles kept as numbers: a HANDLE is a pointer, and not Send.
    static JOBS: Mutex<Vec<(u32, usize)>> = Mutex::new(Vec::new());

    pub fn adopt(child: &std::process::Child) {
        // SAFETY: a new unnamed job, checked for null; the limits are a zeroed (valid) structure with one
        // flag set, passed with its own size; the process handle is the child's, open while `child` is
        // borrowed. A job that is not made or taken is closed here, else its handle is kept in JOBS.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const limits).cast(), size) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle()) == 0
            {
                CloseHandle(job);
                return;
            }
            JOBS.lock().unwrap_or_else(|e| e.into_inner()).push((child.id(), job as usize));
        }
    }

    /// End the plugin `id` and everything it started.
    pub fn end(id: u32) {
        let job = {
            let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
            jobs.iter().position(|j| j.0 == id).map(|i| jobs.swap_remove(i).1)
        };
        if let Some(job) = job {
            // SAFETY: a job handle made in `adopt`; taken out of the table under its lock, so closed once.
            unsafe { CloseHandle(job as _) };
        }
    }
}

/// Signal a plugin and everything it started.
#[cfg(unix)]
fn signal(child: &Child, sig: i32) {
    // SAFETY: kill touches no memory. The child leads its own group (process_group(0)), and it is not
    // reaped before `finish` signals it, so its id is still its group's and no one else's.
    unsafe { libc::kill(-(child.id() as i32), sig) };
}

fn finish(mut child: Child) {
    #[cfg(unix)]
    signal(&child, libc::SIGKILL);
    #[cfg(windows)]
    jobs::end(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// Run a plugin source for as long as the wall runs. A run that fails, or prints nothing within its
/// deadline, is killed; the last value is kept, dimmed, and it is tried again after 10 s, doubling up
/// to a minute. Said once.
pub fn spawn_source(spec: SourceSpec, readings: Arc<Mutex<HashMap<String, Reading>>>, running: Arc<AtomicBool>) {
    let name = spec.name.clone();
    let spawned = std::thread::Builder::new().name(format!("source {name}")).spawn(move || {
        let mut said = false;
        let mut retry = 10.0f64;
        let deadline = crate::util::seconds(spec.every.clamp(5.0, 60.0));
        loop {
            while !running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
            let fail = |said: &mut bool, retry: &mut f64, what: String| {
                if !*said {
                    crate::errln!("source {}: {what}: its last value kept", spec.name);
                    *said = true;
                }
                mark_stale(&readings, &spec.name);
                std::thread::sleep(crate::util::seconds(*retry));
                *retry = (*retry * 2.0).min(60.0);
            };
            let spawned = spawn(command(&spec.command[0]).args(&spec.command[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()));
            let mut child = match spawned {
                Ok(c) => c,
                Err(e) => {
                    fail(&mut said, &mut retry, e.to_string());
                    continue;
                }
            };
            let mut lines = Lines::new(child.stdout.take().unwrap());
            let (mut stream, mut got) = (spec.stream, false);
            while let Some(line) = lines.next(if stream { None } else { Some(Instant::now() + deadline) }) {
                if !got && is_stream_marker(&line) {
                    stream = true;
                    continue;
                }
                if let Some(r) = parse_reading(&line) {
                    readings.lock().unwrap().insert(spec.name.clone(), r);
                    got = true;
                    retry = 10.0;
                }
                if !stream && got {
                    break;
                }
                if stream && !running.load(Ordering::Relaxed) {
                    // paused: stopped where it is (on Windows, held by its full pipe), and on again with the wall
                    #[cfg(unix)]
                    signal(&child, libc::SIGSTOP);
                    while !running.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    #[cfg(unix)]
                    signal(&child, libc::SIGCONT);
                }
            }
            finish(child);
            if !got {
                fail(&mut said, &mut retry, format!("no reading within {} s", deadline.as_secs()));
                continue;
            }
            std::thread::sleep(crate::util::seconds(if stream { 5.0 } else { spec.every }));
        }
    });
    if let Err(e) = spawned {
        crate::errln!("source {name}: {e}");
    }
}

// --- faces ---------------------------------------------------------------------------------------------

/// The face plugins there are: {type: executable}.
pub fn face_plugins() -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for p in std::fs::read_dir(faces_dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| executable(p)) {
        out.insert(p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p);
    }
    out
}

/// A face's picture, drawn on its thread and handed over whole: premultiplied ARGB rows.
pub struct Picture {
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    /// The surface, held by nothing else, so it may move between threads.
    pub data: cairo::ImageSurfaceDataOwned,
}

impl Picture {
    pub fn surface(self) -> cairo::ImageSurface {
        self.data.into_inner()
    }
}

/// How long a face has to answer a request.
pub const FACE_DEADLINE: Duration = Duration::from_secs(2);

/// One face's mailbox: the newest request, the newest answer.
#[derive(Default)]
struct Exchange {
    request: Option<(u64, J)>,
    answer: Option<(u64, Result<Picture, String>)>,
    stop: bool,
}

/// A face plugin, run on a thread of its own: the render thread asks and moves on; the answer (or the
/// failure) is picked up on a later frame.
pub struct Face {
    shared: Arc<(Mutex<Exchange>, Condvar)>,
    asked: u64,
}

impl Face {
    pub fn new(path: PathBuf, look: FaceLook) -> Face {
        let shared: Arc<(Mutex<Exchange>, Condvar)> = Arc::default();
        let worker = shared.clone();
        let name = format!("face {}", path.display());
        if let Err(e) = std::thread::Builder::new().name(name.clone()).spawn(move || face_loop(path, look, worker)) {
            crate::errln!("{name}: {e}");
        }
        Face { shared, asked: 0 }
    }

    /// Ask for a picture; a newer request replaces one not yet taken. Its number.
    pub fn ask(&mut self, request: J) -> u64 {
        self.asked += 1;
        let (m, cv) = &*self.shared;
        m.lock().unwrap().request = Some((self.asked, request));
        cv.notify_one();
        self.asked
    }

    /// The newest answer, once: (the request it answers, its picture or why it failed).
    pub fn answer(&self) -> Option<(u64, Result<Picture, String>)> {
        self.shared.0.lock().unwrap().answer.take()
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        let (m, cv) = &*self.shared;
        m.lock().unwrap().stop = true;
        cv.notify_one();
    }
}

fn face_loop(path: PathBuf, look: FaceLook, shared: Arc<(Mutex<Exchange>, Condvar)>) {
    let (m, cv) = &*shared;
    let mut child: Option<(Child, std::process::ChildStdin, Lines)> = None;
    let mut failures = 0u32;
    loop {
        let (id, request) = {
            let mut x = m.lock().unwrap();
            while x.request.is_none() && !x.stop {
                x = cv.wait(x).unwrap();
            }
            if x.stop {
                break;
            }
            x.request.take().unwrap()
        };
        let size = |k: &str| request.get(k).and_then(J::as_f64).unwrap_or(1.0);
        let (w, h, scale) = (size("width").clamp(1.0, 4096.0) as i32, size("height").clamp(1.0, 4096.0) as i32, size("scale"));
        let result = (|| -> Result<Picture, String> {
            if child.is_none() {
                let mut c = spawn(command(&path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())).map_err(|e| e.to_string())?;
                let (i, o) = (c.stdin.take().unwrap(), c.stdout.take().unwrap());
                child = Some((c, i, Lines::new(o)));
            }
            let (_, stdin, lines) = child.as_mut().unwrap();
            writeln!(stdin, "{request}").and_then(|_| stdin.flush()).map_err(|_| "it stopped reading".to_string())?;
            let line = lines.next(Some(Instant::now() + FACE_DEADLINE)).ok_or_else(|| format!("no answer within {} s", FACE_DEADLINE.as_secs()))?;
            let ops = match serde_json::from_str::<J>(&line) {
                Ok(J::Array(ops)) => ops,
                _ => return Err(format!("not a list of operations: {}", line.chars().take(80).collect::<String>())),
            };
            let pic = face_ops(&ops, w, h, scale, &look)?;
            let stride = pic.stride();
            let data = pic.take_data().map_err(|e| e.to_string())?;
            Ok(Picture { w, h, stride, data })
        })();
        match &result {
            Err(e) => {
                if failures == 0 {
                    crate::errln!("face {}: {e}", path.display());
                }
                failures += 1;
                if let Some((c, _, _)) = child.take() {
                    finish(c);
                }
            }
            Ok(_) => failures = 0,
        }
        m.lock().unwrap().answer = Some((id, result));
        if failures > 0 {
            std::thread::sleep(Duration::from_secs(10u64 << failures.min(3)));
        }
    }
    if let Some((c, _, _)) = child.take() {
        finish(c);
    }
}

/// What a face is told of the theme: its palette and colours, and how its lettering and glow look.
#[derive(Clone)]
pub struct FaceLook {
    palette: J,
    colours: J,
    lettering: crate::tweaks::Colour,
    glow: crate::tweaks::Glow,
}

impl FaceLook {
    pub fn of(th: &crate::config::Theme) -> FaceLook {
        let (p, t) = (&th.palette, &th.tw);
        let palette = json!({
            "base": p.base.hex(), "shadow": p.shadow.hex(), "deep": p.deep.hex(), "ramp": p.ramp.map(|c| c.hex()),
            "sun_top": p.sun_top.hex(), "sun_bottom": p.sun_bottom.hex(), "title_top": p.title_top.hex(),
            "title_light": p.title_light.hex(), "title_bottom": p.title_bottom.hex(), "glow": p.glow.hex(),
            "caption": p.caption.hex(), "bracket": p.bracket.hex(), "ridge": p.ridge.hex(),
        });
        let colours = json!({ "needle": t.needle_colour.hex(), "lines": t.deck_colour.hex(), "caption": t.caption.hex(), "fill": t.deck_fill.0.hex() });
        FaceLook { palette, colours, lettering: t.deck_colour, glow: t.deck_glow.clone() }
    }
}

/// What a face is asked: the picture's size, what it shows, and the theme's colours.
pub fn face_request(look: &FaceLook, w: i32, h: i32, scale: f64, r: &Reading, settings: &toml::Table) -> J {
    json!({"width": w, "height": h, "scale": scale, "value": r.value, "indicator": r.indicator, "text": r.text.as_deref(),
        "stale": r.stale, "settings": serde_json::to_value(settings).unwrap_or(J::Null), "palette": look.palette, "colours": look.colours})
}

fn colour(v: &J) -> Option<(f64, f64, f64, f64)> {
    match v {
        J::String(s) => {
            let c = crate::tweaks::Colour::parse(s.get(..7)?)?;
            let a = match s.len() {
                7 => 1.0,
                9 => u8::from_str_radix(&s[7..9], 16).ok()? as f64 / 255.0,
                _ => return None,
            };
            Some((c.0, c.1, c.2, a))
        }
        J::Array(a) if a.len() >= 3 => Some((a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?, a.get(3).and_then(J::as_f64).unwrap_or(1.0))),
        _ => None,
    }
}

/// A face's drawing operations run with cairo onto a picture w by h, or why they could not be:
/// rect (x, y, w, h), circle (x, y, r), arc (x, y, r, from, to: degrees), move, line (x, y), close,
/// fill / stroke (colour, width), text (text, x, y, size, colour, bold, align), glow, save, restore.
pub fn face_ops(ops: &[J], w: i32, h: i32, scale: f64, look: &FaceLook) -> Result<cairo::ImageSurface, String> {
    use crate::util::*;
    let out = surface(w, h);
    let c = ctx(&out);
    let e = |e: cairo::Error| e.to_string();
    // a number of a face's, in reach of the picture: a hostile one cannot make cairo work for ever
    let n = |o: &J, k: &str| o.get(k).and_then(J::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-1e5, 1e5);
    let mut depth = 0u32;
    for o in ops.iter().take(10_000) {
        match o.get("op").and_then(J::as_str).unwrap_or("") {
            "rect" => c.rectangle(n(o, "x"), n(o, "y"), n(o, "w"), n(o, "h")),
            "circle" => {
                c.new_sub_path();
                c.arc(n(o, "x"), n(o, "y"), n(o, "r").abs(), 0.0, std::f64::consts::TAU);
            }
            "arc" => {
                c.new_sub_path();
                let from = n(o, "from").to_radians();
                let sweep = (n(o, "to").to_radians() - from).clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                c.arc(n(o, "x"), n(o, "y"), n(o, "r").abs(), from, from + sweep);
            }
            "move" => c.move_to(n(o, "x"), n(o, "y")),
            "line" => c.line_to(n(o, "x"), n(o, "y")),
            "close" => c.close_path(),
            op @ ("fill" | "stroke") => {
                let (r, g, b, a) = o.get("colour").and_then(colour).unwrap_or((1.0, 1.0, 1.0, 1.0));
                c.set_source_rgba(r, g, b, a);
                if op == "fill" {
                    c.fill().map_err(e)?;
                } else {
                    c.set_line_width(o.get("width").and_then(J::as_f64).unwrap_or(1.0).clamp(0.0, 100.0));
                    c.stroke().map_err(e)?;
                }
            }
            "text" => {
                let text = printable(o.get("text").and_then(J::as_str).unwrap_or(""));
                let bold = o.get("bold").and_then(J::as_bool).unwrap_or(false);
                let size = o.get("size").and_then(J::as_f64).unwrap_or((13.0 * scale).max(7.0).round()).clamp(1.0, 500.0);
                crate::util::select_mono(&c, bold);
                c.set_font_size(size);
                let adv = text_extents(&c, &text).map(|x| x.x_advance()).unwrap_or(0.0);
                let x = match o.get("align").and_then(J::as_str).unwrap_or("left") {
                    "centre" | "center" => n(o, "x") - adv / 2.0,
                    "right" => n(o, "x") - adv,
                    _ => n(o, "x"),
                };
                let (r, g, b, a) = o.get("colour").and_then(colour).unwrap_or_else(|| {
                    let (r, g, b) = look.lettering.rgb();
                    (r, g, b, 0.95)
                });
                c.set_source_rgba(r, g, b, a);
                c.move_to(x.round(), n(o, "y").round());
                show_text(&c, &text).map_err(e)?;
                c.new_path();
            }
            "glow" => {
                out.flush();
                let copy = surface(w, h);
                {
                    let cc = ctx(&copy);
                    cc.set_source_surface(&out, 0.0, 0.0).map_err(e)?;
                    cc.paint().map_err(e)?;
                }
                c.save().map_err(e)?;
                c.set_operator(cairo::Operator::Clear);
                c.paint().map_err(e)?;
                c.restore().map_err(e)?;
                glowing(&c, &copy, &look.glow);
            }
            "save" => {
                c.save().map_err(e)?;
                depth += 1;
            }
            "restore" if depth > 0 => {
                c.restore().map_err(e)?;
                depth -= 1;
            }
            _ => {}
        }
    }
    c.status().map_err(e)?;
    drop(c);
    out.flush();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Value {
        text.parse::<toml::Table>().unwrap()["s"].clone()
    }

    #[test]
    fn every_is_a_finite_number_of_seconds_within_a_day() {
        let every = |v: &str| spec_of("s", &table(&format!("s = {{ command = [\"x\"], every = {v} }}"))).map(|s| s.every);
        assert_eq!(every("0.1"), Ok(0.1));
        assert_eq!(every("86400"), Ok(86_400.0));
        for bad in ["0", "0.05", "-1", "86400.5", "1e30", "inf", "-inf", "nan", "\"5\""] {
            assert!(every(bad).is_err(), "every = {bad}");
        }
        assert_eq!(spec_of("s", &table("s = { command = \"echo 1\" }")).unwrap().every, 2.0);
    }

    #[test]
    fn a_source_table_has_a_command() {
        let spec = spec_of("s", &table("s = { command = \"echo 1\", stream = true }")).unwrap();
        assert!(spec.stream && spec.command.last().map(String::as_str) == Some("echo 1"));
        assert!(spec_of("s", &table("s = { command = \"  \" }")).is_err());
        assert!(spec_of("s", &table("s = { command = [1] }")).is_err());
        assert!(spec_of("s", &table("s = { command = [\"x\"], stream = 1 }")).is_err());
        assert!(spec_of("s", &table("s = { command = [\"x\"], colour = 1 }")).is_err());
    }

    #[test]
    fn readings_are_numbers_or_json() {
        let r = |line: &str| parse_reading(line);
        assert_eq!(r(" 42 ").map(|x| x.value), Some(42.0));
        assert_eq!(r("250").map(|x| x.value), Some(100.0));
        assert_eq!(r("-3").map(|x| x.value), Some(0.0));
        assert!(r("inf").is_none() && r("NaN").is_none() && r("").is_none() && r("[1]").is_none());
        let x = r(r#"{"value": 87, "indicator": true, "text": "a\u0000b\nc"}"#).unwrap();
        assert_eq!((x.value, x.indicator, x.text.as_deref()), (87.0, true, Some("abc")));
        assert_eq!(r(r#""\u0007hi""#).and_then(|x| x.text).as_deref(), Some("hi"));
        assert!(is_stream_marker(r#" {"stream": true} "#) && !is_stream_marker(r#"{"stream": 1}"#) && !is_stream_marker("5"));
    }

    #[test]
    fn a_face_colour_is_hex_or_a_list() {
        assert_eq!(colour(&json!("#ff0000")), Some((1.0, 0.0, 0.0, 1.0)));
        assert_eq!(colour(&json!("#ff000080")), Some((1.0, 0.0, 0.0, 128.0 / 255.0)));
        assert_eq!(colour(&json!([0.5, 0.5, 0.5])), Some((0.5, 0.5, 0.5, 1.0)));
        for bad in [json!("#ff00"), json!("#ff0000zz"), json!("red"), json!([1, 2]), json!(5), json!("#\u{e9}\u{e9}\u{e9}"), json!("#ff000\u{e9}x")] {
            assert_eq!(colour(&bad), None, "{bad}");
        }
    }
}
