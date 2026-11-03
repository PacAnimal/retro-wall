//! The live picture: on each monitor it covers, one window over the part that moves, drawn from the
//! scene's plan and presented only when it changes, all from one loop. Pauses on battery, with the
//! screen off or a VNC viewer connected (instruments fade, the scene slows to a stop, the loop
//! sleeps), skips drawing a monitor while it is covered, and follows monitors as they come and go.
//! Where the desktop draws over its wallpaper (its icons), that is laid over the live picture again, so
//! the wall shows under the icons as the still picture does (`Icons`).

use crate::cli::Reserve;
use crate::config::{Choice, Loaded, Monitors, Rule, Theme};
use crate::gfx::{Gfx, Tex, Uploads};
use crate::platform::{Desktop, Display, Edges, Native, Platform, Screen, WindowId};
use crate::scene::*;
use crate::sources::{self, Traffic};
use cairo::ImageSurface;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fixed inputs for a snapshot: canned readings and a fixed clock.
#[derive(Clone)]
pub struct Frozen {
    pub at: f64,
    pub mains: bool,
}

impl Frozen {
    pub fn value(&self, source: &str) -> f64 {
        sources::builtin(source).map_or(50.0, |(b, _)| b.fixed)
    }
}

pub struct Options {
    pub host: String,
    /// One monitor by name; None: the primary, else the first.
    pub monitor: Option<String>,
    pub all_monitors: bool,
    pub pause_screen_off: bool,
    pub pause_below: f64,
    pub pause_vnc: Option<u16>,
    /// The RDP server's port: paused while a client is connected to it.
    pub pause_rdp: Option<u16>,
    pub ip_url: String,
    pub software: bool,
    pub demo: bool,
    pub reserve: Reserve,
}

fn fail(what: &str) -> ! {
    crate::errln!("retro-wall: {what}");
    std::process::exit(1)
}

struct Monitor {
    screen: Screen,
    backdrop: Option<(WindowId, Gfx, Tex)>,
    geo: Geo,
    parts: Vec<Box<dyn Part>>,
    plan: Vec<Step>,
    gfx: Gfx,
    window: WindowId,
    live: Vec<usize>,
    state: Option<u64>,
    shown_at: f64,
    next: f64,
    next_frame: f64,
    clock: f64,
    rate: f64,
    presence: f64,
    shown: bool,
    faded_at: Option<f64>,
    hidden: bool,
    /// Its window covers the whole monitor: the desktop shows no wallpaper (Display::wallpaper_hidden).
    whole: bool,
    /// Its still picture as the desktop shows it, kept until the desktop's icons are followed.
    picture: Option<ImageSurface>,
    /// What the desktop draws over its wallpaper here (its icons), laid over the live picture.
    icons: Option<Icons>,
    /// The window's rectangle on the monitor: x, y, width, height.
    pane: (i32, i32, i32, i32),
}

const FADE: f64 = 1.0;
const SLOW: f64 = 1.5;

impl Monitor {
    /// Stop its parts and close its windows.
    fn close(mut self, d: &mut impl Display) {
        for p in self.parts.iter_mut() {
            p.stop();
        }
        if let Some((b, _, _)) = self.backdrop.take() {
            d.close(b);
        }
        d.close(self.window);
    }

    fn new(
        d: &mut impl Display,
        s: &Screen,
        th: Arc<Theme>,
        host: &str,
        header: Header,
        edges: Edges,
        w: &mut World,
        still: Option<&cairo::Context>,
    ) -> Result<Monitor, String> {
        let geo = Geo::new(s.w, s.h, host, edges, header, Some(s.name.clone()), th);
        let t0 = Instant::now();
        let mut scene = Scene::new(geo.clone());
        if let Some(c) = still {
            scene.compose(c);
        }
        let mut textures = Uploads::default();
        let plan = scene.compile(&mut textures);
        let mut shared = std::mem::take(&mut scene.shared);
        let mut parts = scene.parts;
        let live: Vec<usize> = (0..parts.len()).filter(|&i| moves(parts[i].as_ref())).collect();
        for &i in &live {
            parts[i].start(&geo, w, &mut shared);
        }
        for &i in &live {
            parts[i].textures(&geo, &shared, &mut textures);
        }
        drop(shared);
        crate::elements::built();
        let uploads = textures.len();
        // the window over the parts that move; over all of it where the still picture is not shown
        let whole = d.wallpaper_hidden();
        let head = if whole { 0 } else { live.iter().map(|&i| parts[i].top(&geo)).min().unwrap_or(geo.h - 1).min(geo.h - 1).max(0) };
        let rect = (0, head, geo.w, geo.h - head);
        let window = match d.window(s.x, s.y + head, rect.2, rect.3) {
            Ok(win) => win,
            Err(e) => {
                for p in parts.iter_mut() {
                    p.stop();
                }
                return Err(e);
            }
        };
        let core = d.core(window);
        let gfx = Gfx::new(rect, core, textures);
        crate::outln!(
            "{}: {} at {}x{}+{}+{}, showing {}; from row {head}, {} textures, made in {:.2} s, on {} ({})",
            s.name,
            geo.th.name,
            geo.w,
            geo.h,
            s.x,
            s.y,
            geo.header,
            uploads,
            t0.elapsed().as_secs_f64(),
            d.renderer(),
            if core { "OpenGL 3.3 core" } else { "OpenGL 2.1" }
        );
        Ok(Monitor {
            screen: s.clone(),
            backdrop: None,
            geo,
            parts,
            plan,
            gfx,
            window,
            live,
            state: None,
            shown_at: -1.0,
            next: 1.0 / 30.0,
            next_frame: 0.0,
            clock: 0.0,
            rate: 1.0,
            presence: 1.0,
            shown: true,
            faded_at: None,
            hidden: false,
            whole,
            picture: None,
            icons: None,
            pane: rect,
        })
    }

    fn fade_to(&mut self, t: f64) {
        let dt = self.faded_at.map_or(0.0, |f| (t - f).min(0.1));
        self.faded_at = Some(t);
        let step = dt / FADE;
        self.presence = if self.shown { (self.presence + step).min(1.0) } else { (self.presence - step).max(0.0) };
        let before = self.rate;
        self.rate = if self.shown { (before + dt / SLOW).min(1.0) } else { (before - dt / SLOW).max(0.0) };
        let ease = |r: f64| r * r * (3.0 - 2.0 * r);
        self.clock += dt * (ease(before) + ease(self.rate)) / 2.0;
    }

    fn faded(&self) -> bool {
        !self.shown && self.presence <= 0.0 && self.rate <= 0.0
    }

    /// What it shows at t, as one number: each live part's state and the instruments' strength.
    fn state_at(&mut self, t: f64, w: &mut World) -> u64 {
        if let Some(s) = self.state
            && t - self.shown_at < self.next - 0.004
        {
            return s;
        }
        self.shown_at = t;
        if let Some(f) = &w.frozen {
            self.clock = f.at;
            self.presence = 1.0;
        } else {
            self.fade_to(t);
        }
        w.clock = self.clock;
        w.clock_real = t;
        let mut h = 0xcbf2_9ce4_8422_2325;
        for &i in &self.live {
            h = (h ^ self.parts[i].state(&self.geo, w)).wrapping_mul(0x100000001b3);
        }
        hash_f(&mut h, ((self.presence) * 1000.0).round() / 1000.0);
        h
    }

    fn render(&mut self, w: &World) {
        draw_plan(&self.plan, &mut self.parts, &self.geo, &mut self.gfx, w, self.presence);
        if let Some(i) = self.icons.as_mut() {
            i.draw(&mut self.gfx, self.pane);
        }
        self.gfx.flush();
    }

    /// Draw and present at t if the picture changed, or if `force`. Returns seconds to the next frame.
    fn frame(&mut self, d: &impl Display, t: f64, w: &mut World, force: bool) -> f64 {
        let s = self.state_at(t, w);
        if Some(s) != self.state || force {
            self.state = Some(s);
            self.render(w);
            d.swap(self.window);
        }
        self.next = self.live.iter().map(|&i| self.parts[i].interval()).fold(1.0 / 30.0, f64::min);
        self.next
    }

    /// What the desktop has drawn anew in these rectangles (all of the monitor at first), taken into its
    /// icons. Whether a GL context was made current.
    fn desktop_drawn(&mut self, d: &mut impl Display, rects: &[crate::platform::Rect]) -> bool {
        let s = &self.screen;
        let whole = [crate::platform::Rect::new(s.x as i64, s.y as i64, (s.x + s.w) as i64, (s.y + s.h) as i64)];
        let first = self.icons.is_none();
        // the still picture, as the desktop shows it: drawn again if it was not kept
        let mut picture = match self.icons.as_mut() {
            Some(i) => std::mem::replace(&mut i.picture, crate::util::surface_rgb(1, 1)),
            None => self.picture.take().unwrap_or_else(|| crate::still::draw(&self.geo).unwrap_or_else(|_| crate::util::surface_rgb(1, 1))),
        };
        // what the desktop draws, starting as the still picture: what cannot be read shows nothing over the wall
        let mut drawn = self.icons.as_mut().map_or_else(|| copied(&picture), |i| std::mem::replace(&mut i.drawn, crate::util::surface_rgb(1, 1)));
        let mut changed = false;
        let (stride, ps) = (drawn.stride() as usize, picture.stride() as usize);
        let whole_size = |p: &cairo::ImageSurface| p.width() == s.w && p.height() == s.h;
        let sized = whole_size(&drawn) && whole_size(&picture);
        if sized && let (Some(mut px), Some(pp)) = (crate::util::view(&mut drawn), crate::util::view(&mut picture)) {
            for r in if first { &whole[..] } else { rects } {
                let (x0, y0) = (r.left.max(s.x as i64) as i32, r.top.max(s.y as i64) as i32);
                let (x1, y1) = (r.right.min((s.x + s.w) as i64) as i32, r.bottom.min((s.y + s.h) as i64) as i32);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
                let at = |row: usize, col: usize, stride: usize| (y0 - s.y) as usize * stride + row * stride + ((x0 - s.x) as usize + col) * 4;
                match d.desktop_pixels(x0, y0, x1 - x0, y1 - y0) {
                    Some(got) => {
                        for (i, p) in got.iter().enumerate() {
                            let a = at(i / w, i % w, stride);
                            px[a..a + 3].copy_from_slice(&[p[2], p[1], p[0]]);
                        }
                    }
                    // not shown by the desktop (gone, or this part out of its window): the still picture again
                    None => {
                        for row in 0..h {
                            let (a, b) = (at(row, 0, stride), at(row, 0, ps));
                            px[a..a + w * 4].copy_from_slice(&pp[b..b + w * 4]);
                        }
                    }
                }
                changed = true;
            }
        }
        if let Some(i) = self.icons.as_mut() {
            i.drawn = drawn;
            i.picture = picture;
            if !changed {
                return false;
            }
            for r in rects {
                i.mark(r.left - s.x as i64, r.top - s.y as i64, r.right - s.x as i64, r.bottom - s.y as i64);
            }
            d.make_current(self.window);
            self.gfx.replace(i.tex, &i.drawn, crate::gfx::Kind::Layer);
        } else {
            d.make_current(self.window);
            let still = self.gfx.upload(&picture, crate::gfx::Kind::Layer);
            let tex = self.gfx.upload(&drawn, crate::gfx::Kind::Layer);
            let (cols, rows) = ((s.w as usize).div_ceil(BLOCK), (s.h as usize).div_ceil(BLOCK));
            let mut i = Icons { drawn, picture, tex, still, blocks: vec![false; cols * rows], cols, program: None };
            i.mark(0, 0, s.w as i64, s.h as i64);
            self.icons = Some(i);
        }
        self.state = None;
        true
    }

    /// Whether it draws nothing now: covered, or paused and faded out.
    fn idle(&self, paused: bool) -> bool {
        self.hidden || (paused && self.faded())
    }
}

fn world(th: &Theme, frozen: Option<Frozen>, demo: bool) -> World {
    let span = th.tw.mountains[1];
    let tiers = th.tw.mountain_tiers.clone();
    World {
        clock: 0.0,
        clock_real: 0.0,
        demo: demo || frozen.is_some(),
        frozen,
        zoom: 1.0,
        traffic: Traffic::new(span, tiers),
        network: Net::default(),
        readings: Arc::new(Mutex::new(HashMap::new())),
    }
}

/// The header pieces monitor `name` shows. Alone (the one monitor driven)
/// it shows all of them that the theme does not turn off. Among `monitors`, as [monitors] chooses:
/// "rightmost" (by right edge, the upper on a tie; the default), "leftmost", "primary", "all", "none",
/// or a list of names, each piece by its own rule (the status line by the widgets' when it has none).
pub fn header(rules: &Monitors, name: &str, monitors: &[Screen], alone: bool) -> Header {
    let mut out = Header::NONE;
    for (part, choice) in rules.rules() {
        let shows = match choice {
            Choice::Rule(Rule::None) => false,
            _ if alone => true,
            Choice::Named(names) => names.iter().any(|n| n == name),
            Choice::Rule(Rule::All) => true,
            Choice::Rule(Rule::Primary) if monitors.iter().any(|m| m.primary) => monitors.iter().find(|m| m.primary).is_some_and(|m| m.name == name),
            Choice::Rule(Rule::Leftmost) => monitors.iter().min_by_key(|m| (m.x, m.y)).is_some_and(|m| m.name == name),
            Choice::Rule(_) => monitors.iter().max_by_key(|m| (m.x + m.w, -m.y)).is_some_and(|m| m.name == name),
        };
        if shows {
            out = out.with(part);
        }
    }
    out
}

/// The monitor a single wall drives: by name, else the primary, else the first.
pub fn pick(screens: &[Screen], name: Option<&str>) -> Option<Screen> {
    match name {
        Some(n) => screens.iter().find(|s| s.name == n).cloned(),
        None => screens.iter().find(|s| s.primary).or(screens.first()).cloned(),
    }
}

/// What each driven monitor should be: its screen, header and edges.
fn wanted(d: &impl Display, th: &Theme, o: &Options) -> Vec<(Screen, Header, Edges)> {
    let all = d.screens();
    let rules = &th.monitors;
    let chosen: Vec<Screen> = if o.all_monitors { all.clone() } else { pick(&all, o.monitor.as_deref()).into_iter().collect() };
    chosen
        .into_iter()
        .map(|s| {
            let h = header(rules, &s.name, &all, !o.all_monitors);
            let e = o.reserve.apply(&s, d.work_edges(&s));
            (s, h, e)
        })
        .collect()
}

/// The monitor whose picture a desktop of one picture shows: the first with the title, else the first.
fn lead(want: &[(Screen, Header, Edges)]) -> Option<String> {
    want.iter().find(|(_, h, _)| h.contains(Piece::Title)).or(want.first()).map(|(s, _, _)| s.name.clone())
}

/// What --snapshot draws: one still frame, or frames played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shot {
    /// One frame from fixed inputs: canned readings, the reels and needles at rest.
    Still,
    /// Frames of the live wall as it plays: made-up readings, everything moving, at `fps` a second.
    Frames { count: u32, fps: f64 },
}

/// Seconds a played run goes on before its first frame, so needles, reels and traffic have settled
/// as on a wall that has run for a while; less when --at is earlier.
pub const WARM: f64 = 20.0;

/// FILE with frame i's number before its extension: out.png, 7 → out-0007.png.
pub fn frame_file(path: &str, i: u32) -> String {
    let p = std::path::Path::new(path);
    let stem = p.file_stem().map_or_else(|| path.into(), |s| s.to_string_lossy());
    let ext = p.extension().map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
    let name = format!("{stem}-{i:04}{ext}");
    match p.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(d) => d.join(name).to_string_lossy().into_owned(),
        None => name,
    }
}

/// --snapshot: one frame from fixed inputs, or frames played on a clock of their own, so each is
/// what the wall shows at its time however long it takes to draw. Read back from GL, saved as PNGs.
pub fn snapshot(loaded: &Loaded, path: &str, host: &str, show_header: bool, at: f64, reserve: &Reserve, shot: Shot) {
    let first = if shot == Shot::Still { path.to_string() } else { frame_file(path, 0) };
    if let Err(e) = std::fs::File::create(&first) {
        crate::errln!("--snapshot {first}: {e}");
        std::process::exit(2)
    }
    let mut d = Native::open().unwrap_or_else(|e| fail(&e));
    let s = pick(&d.screens(), None).unwrap_or_else(|| fail("no monitor"));
    let th = loaded.look(crate::day::lighting_phase(loaded).as_deref());
    let header = if show_header { header(&th.monitors, &s.name, std::slice::from_ref(&s), true) } else { Header::NONE };
    let mut w = match shot {
        Shot::Still => {
            let mut w = world(&th, Some(Frozen { at, mains: true }), true);
            w.traffic.sample(at, true);
            w.traffic.held = true;
            w
        }
        Shot::Frames { .. } => world(&th, None, true),
    };
    w.network = sources::network_demo();
    let mut out = cairo::ImageSurface::create(cairo::Format::Rgb24, s.w, s.h).unwrap_or_else(|e| fail(&format!("a {}x{} picture: {e}", s.w, s.h)));
    let made = cairo::Context::new(&out).map_err(|e| e.to_string()).and_then(|c| {
        let edges = reserve.apply(&s, d.work_edges(&s));
        Monitor::new(&mut d, &s, th, host, header, edges, &mut w, Some(&c))
    });
    let mut m = made.unwrap_or_else(|e| fail(&e));
    let (count, fps) = match shot {
        Shot::Still => (1, 1.0),
        Shot::Frames { count, fps } => (count, fps),
    };
    // played: the scene's clock from `at` less the warm-up, every clock moved on a frame at a time
    let warm = if shot == Shot::Still { 0 } else { (WARM.min(at) * fps).round() as u32 };
    let mut sampler = sources::Sampler::new(true);
    let start = 1000.0;
    if shot != Shot::Still {
        m.clock = at - warm as f64 / fps;
    }
    let step = |m: &mut Monitor, w: &mut World, sampler: &mut sources::Sampler, i: u32| {
        let t = start + i as f64 / fps;
        if shot != Shot::Still {
            sources::hold_demo_time(t);
            sampler.step(&w.readings, t);
            w.traffic.sample(t, true);
            m.state = None; // every frame its own state, whatever the parts' pace
        }
        m.state_at(if shot == Shot::Still { at } else { t }, w);
    };
    for i in 0..warm {
        step(&mut m, &mut w, &mut sampler, i);
    }
    for f in 0..count {
        let i = warm + f;
        step(&mut m, &mut w, &mut sampler, i);
        // the faces' first answers, as live shows them; a face that does not answer fails at its deadline
        let until = Instant::now() + crate::plugins::FACE_DEADLINE + Duration::from_millis(500);
        while f == 0 && m.parts.iter().any(|p| p.waiting()) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
            m.state = None;
            step(&mut m, &mut w, &mut sampler, i);
        }
        m.render(&w);
        let px = m.gfx.read();
        let (_, y, pw, ph) = m.gfx.rect;
        {
            let stride = out.stride() as usize;
            let Ok(mut data) = out.data() else { fail("the picture's pixels") };
            for r in 0..ph as usize {
                let dst = (y as usize + r) * stride;
                data[dst..dst + pw as usize * 4].copy_from_slice(&px[r * pw as usize * 4..(r + 1) * pw as usize * 4]);
            }
        }
        let file = if shot == Shot::Still { path.to_string() } else { frame_file(path, f) };
        let written = std::fs::File::create(&file).map_err(|e| e.to_string()).and_then(|mut fh| out.write_to_png(&mut fh).map_err(|e| e.to_string()));
        if let Err(e) = written {
            crate::errln!("--snapshot {file}: {e}");
            std::process::exit(2)
        }
        if shot == Shot::Still {
            crate::outln!("{path}: written");
        }
    }
    if let Shot::Frames { count, fps } = shot {
        crate::outln!("{first}: {count} frames at {fps} a second, from {at} s on the scene's clock");
    }
}

fn now(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() + 1000.0
}

/// The loop's timed checks: pausing and monitors (watch), the look and coverage (look), restacking.
pub struct Timers {
    pub watch: f64,
    pub look: f64,
    pub restack: f64,
}

/// Which checks are due.
pub struct Due {
    pub watch: bool,
    pub look: bool,
    pub restack: bool,
}

impl Timers {
    pub const WATCH: f64 = 2.0;
    pub const LOOK: f64 = 1.0;
    pub const RESTACK: f64 = 10.0;
    /// The longest sleep: events wake the loop sooner.
    pub const NAP: f64 = 2.0;

    pub fn new() -> Timers {
        Timers { watch: 0.0, look: 0.0, restack: 0.0 }
    }

    /// The checks due at `now`, each moved on to its next time whether or not it then runs.
    pub fn tick(&mut self, now: f64) -> Due {
        let due = |at: &mut f64, every: f64| {
            let d = now >= *at;
            if d {
                *at = now + every;
            }
            d
        };
        Due { watch: due(&mut self.watch, Self::WATCH), look: due(&mut self.look, Self::LOOK), restack: due(&mut self.restack, Self::RESTACK) }
    }

    /// How long to sleep from `now`: to the soonest frame of a monitor that draws (None: idle) or the
    /// soonest check.
    pub fn sleep(&self, now: f64, frames: impl IntoIterator<Item = Option<f64>>) -> Duration {
        let wake = frames.into_iter().flatten().fold(self.watch.min(self.look).min(self.restack), f64::min);
        crate::util::seconds((wake - now).min(Self::NAP))
    }
}

impl Default for Timers {
    fn default() -> Self {
        Timers::new()
    }
}

/// Why the wall pauses now, if it does.
fn pause_reason(d: &impl Display, o: &Options) -> Option<&'static str> {
    if o.demo {
        return None;
    }
    let power = Native::power();
    if power.on_battery && power.charge <= o.pause_below {
        return Some("on battery");
    }
    if o.pause_screen_off && d.screen_off() {
        return Some("screen off");
    }
    if o.pause_vnc.is_some_and(Native::port_connected) {
        return Some("a VNC viewer is connected");
    }
    if o.pause_rdp.is_some_and(Native::port_connected) {
        return Some("a remote desktop client is connected");
    }
    None
}

pub fn run(loaded: &Loaded, o: Options) {
    let mut d = Native::open().unwrap_or_else(|e| fail(&e));
    #[cfg(target_os = "linux")]
    if !o.software {
        through_virtualgl(&mut d);
    }
    Native::name_process(&format!("retro-wall {}", o.host));
    let desktop = Native::desktop();
    let mut th = loaded.look(crate::day::lighting_phase(loaded).as_deref());
    let mut w = world(&th, None, o.demo);
    let running = Arc::new(AtomicBool::new(true));
    sources::spawn_sampler(w.readings.clone(), running.clone(), o.demo);
    if !o.demo {
        for spec in crate::plugins::source_specs(&th.sources) {
            crate::outln!("source {}: {}", spec.name, spec.command.join(" "));
            crate::plugins::spawn_source(spec, w.readings.clone(), running.clone());
        }
    }
    let net = Arc::new(Mutex::new(Net::default()));
    if o.demo {
        *net.lock().unwrap_or_else(|e| e.into_inner()) = sources::network_demo();
    } else {
        sources::spawn_network(net.clone(), o.ip_url.clone(), running.clone());
    }
    let t0 = Instant::now();
    let mut monitors: Vec<Monitor> = vec![];
    let mut paused: Option<&str> = None;
    let mut timers = Timers::new();
    let mut current: Option<WindowId> = None;
    let mut checked_renderer = false;
    loop {
        let t = now(t0);
        let due = timers.tick(t);
        let exposed = d.events();
        // what the desktop draws over its wallpaper (its icons), laid over the live windows
        // followed only where the desktop shows each monitor's own still picture: a desktop of a
        // picture per monitor, or one monitor; elsewhere its picture would be laid over the wall
        let follows = desktop.as_ref().is_some_and(|k| k.per_monitor()) || monitors.len() <= 1;
        // taken every time, so nothing piles up; laid over the wall only where it is followed
        let drawn = d.desktop_drawn().filter(|_| follows);
        if !follows {
            for m in monitors.iter_mut().filter(|m| m.icons.is_some() || m.picture.is_some()) {
                (m.icons, m.picture, m.state) = (None, None, None);
                current = None;
            }
        }
        if let Some(drawn) = &drawn {
            for m in monitors.iter_mut() {
                if (m.icons.is_none() || !drawn.is_empty()) && m.desktop_drawn(&mut d, drawn) {
                    current = None;
                }
            }
        }
        {
            let n = net.lock().unwrap_or_else(|e| e.into_inner());
            if *n != w.network {
                w.network = n.clone();
            }
        }
        if due.watch {
            let why = pause_reason(&d, &o);
            if why != paused {
                crate::outln!("{}", why.map_or("running".to_string(), |w| format!("paused: {w}")));
                paused = why;
                running.store(why.is_none(), Ordering::Relaxed);
                for m in monitors.iter_mut() {
                    m.shown = why.is_none();
                    m.next_frame = t;
                }
            }
            // paused or not: a remote desktop client that connects both pauses the wall and hides the
            // wallpaper, and a wall kept for the old desktop would leave its new one half covered and stale
            {
                let want = wanted(&d, &th, &o);
                let whole = d.wallpaper_hidden();
                // gone, moved, resized, a panel moved, the header now elsewhere, or the wallpaper shown or
                // hidden (a remote desktop reconnected): closed, and made again below
                let mut kept = vec![];
                for m in monitors.drain(..) {
                    if m.whole == whole && want.iter().any(|(s, h, e)| *s == m.screen && *h == m.geo.header && *e == m.geo.edges) {
                        kept.push(m);
                    } else {
                        crate::outln!("{}: changed or gone: closed", m.screen.name);
                        current = None;
                        m.close(&mut d);
                    }
                }
                monitors = kept;
                let lead = lead(&want);
                for (s, h, e) in want {
                    if monitors.iter().any(|m| m.screen == s) {
                        continue;
                    }
                    let is_lead = lead.as_deref() == Some(s.name.as_str());
                    match open_monitor(&mut d, &s, &th, h, e, &o, &mut w, desktop.as_deref(), is_lead, drawn.is_some()) {
                        Ok(mut m) => {
                            m.shown = paused.is_none();
                            current = Some(m.window);
                            monitors.push(m);
                        }
                        Err(e) => crate::errln!("{}: {e}", s.name),
                    }
                    if !checked_renderer && !monitors.is_empty() {
                        checked_renderer = true;
                        let r = d.renderer();
                        if software(&r) && !o.software {
                            crate::outln!("software OpenGL ({r}): not animating");
                            std::process::exit(0);
                        }
                    }
                }
            }
        }
        if due.look && paused.is_none() && !monitors.is_empty() {
            // a theme with day and night looks: the other one when the sun crosses
            let phase = crate::day::lighting_phase(loaded);
            if phase.is_some() && phase != th.phase {
                crate::outln!("the {}'s look", phase.as_deref().unwrap_or_default());
                th = loaded.look(phase.as_deref());
                for m in monitors.drain(..) {
                    m.close(&mut d);
                }
                current = None;
                timers.watch = t;
                continue;
            }
            let cover = d.covering();
            for m in monitors.iter_mut() {
                let (s, e) = (&m.screen, m.geo.edges);
                let area = crate::platform::Rect::new((s.x + e.left) as i64, (s.y + e.top) as i64, (s.x + s.w - e.right) as i64, (s.y + s.h - e.bottom) as i64);
                let covered = cover.as_ref().is_some_and(|r| crate::platform::covers(r, area));
                if covered != m.hidden {
                    crate::outln!("{}: {}", s.name, if covered { "covered: not drawing" } else { "showing again" });
                    m.hidden = covered;
                    m.faded_at = None;
                    m.next_frame = t;
                }
            }
        }
        if due.restack {
            for m in &monitors {
                if let Some((b, _, _)) = &m.backdrop {
                    d.raise(*b);
                }
                d.raise(m.window);
            }
        }
        if paused.is_none() {
            w.traffic.sample(t, w.demo);
        }
        for m in monitors.iter_mut() {
            if let Some((b, g, still)) = m.backdrop.as_mut()
                && exposed.contains(b)
            {
                d.make_current(*b);
                current = Some(*b);
                g.layer(*still, None, 1.0, crate::gfx::Blend::Opaque);
                g.flush();
                d.swap(*b);
            }
            let shown = exposed.contains(&m.window);
            let idle = m.idle(paused.is_some());
            if (!idle && t >= m.next_frame) || shown {
                if current != Some(m.window) {
                    d.make_current(m.window);
                    current = Some(m.window);
                }
                if idle {
                    m.render(&w);
                    d.swap(m.window);
                } else {
                    let interval = m.frame(&d, t, &mut w, shown);
                    m.next_frame = (m.next_frame + interval).max(t + 0.002);
                }
            }
        }
        let frames = monitors.iter().map(|m| (!m.idle(paused.is_some())).then_some(m.next_frame));
        let nap = timers.sleep(now(t0), frames);
        if monitors.iter().any(|m| !m.idle(paused.is_some())) {
            std::thread::sleep(nap); // drawing: the next frame is soon, and swaps wake the display's socket
        } else {
            d.wait(nap); // idle: long naps, an expose wakes it
        }
    }
}

/// What the desktop draws over its wallpaper on a monitor (its icons, their names, a selection), laid
/// over the live picture where it differs from the still picture: as the desktop draws it, and that
/// picture, each a texture of the window's pane, and the program that lays one over the other.
struct Icons {
    drawn: ImageSurface,
    /// The still picture: kept to tell where the desktop draws over it.
    picture: ImageSurface,
    tex: Tex,
    still: Tex,
    /// Which squares of BLOCK pixels the desktop draws over, by row: only those are drawn again.
    blocks: Vec<bool>,
    cols: usize,
    program: Option<Option<(gl::types::GLuint, [gl::types::GLint; 4])>>,
}

/// The side of a square the icons are followed by.
const BLOCK: usize = 64;

/// Where the desktop's pixel (tex0) is not the still picture's (tex1), it shows: over `square`, of the
/// window's `pane` (x, y, width, height on the monitor, both).
const ICONS: &str = "uniform vec4 square; uniform vec4 pane;
void main() {
    vec2 at = (square.xy + uv * square.zw - pane.xy) / pane.zw;
    vec3 d = texture2D(tex0, at).rgb;
    float a = smoothstep(0.03, 0.1, length(d - texture2D(tex1, at).rgb));
    gl_FragColor = vec4(d * a, a);
}";

impl Icons {
    /// Find again which squares the desktop draws over, in a rectangle of the monitor.
    fn mark(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        let (w, h) = (self.picture.width() as usize, self.picture.height() as usize);
        let rows = self.blocks.len() / self.cols.max(1);
        let (bx0, by0) = ((x0.max(0) as usize) / BLOCK, (y0.max(0) as usize) / BLOCK);
        let (bx1, by1) = ((x1.max(0) as usize).div_ceil(BLOCK).min(self.cols), (y1.max(0) as usize).div_ceil(BLOCK).min(rows));
        let (ds, ps) = (self.drawn.stride() as usize, self.picture.stride() as usize);
        let (Some(d), Some(p)) = (crate::util::view(&mut self.drawn), crate::util::view(&mut self.picture)) else { return };
        for by in by0..by1 {
            for bx in bx0..bx1 {
                let differs = (by * BLOCK..((by + 1) * BLOCK).min(h)).any(|y| {
                    (bx * BLOCK..((bx + 1) * BLOCK).min(w))
                        .any(|x| (0..3).map(|c| d[y * ds + x * 4 + c].abs_diff(p[y * ps + x * 4 + c]) as u32).sum::<u32>() > 24)
                });
                self.blocks[by * self.cols + bx] = differs;
            }
        }
    }

    /// Over the live picture, in the squares the desktop draws over: runs of them along each row.
    fn draw(&mut self, gfx: &mut Gfx, pane: (i32, i32, i32, i32)) {
        let program = self.program.get_or_insert_with(|| match gfx.compile(ICONS) {
            Ok(p) => Some((p, ["tex0", "tex1", "square", "pane"].map(|n| gfx.uniform(p, n)))),
            Err(e) => {
                crate::errln!("the desktop's icons over the wall: {e}");
                None
            }
        });
        let Some((prog, loc)) = program else { return };
        let (px, py, pw, ph) = pane;
        let p = [px as f32, py as f32, pw as f32, ph as f32];
        for (row, line) in self.blocks.chunks(self.cols).enumerate() {
            let mut x = 0;
            while x < line.len() {
                if !line[x] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < line.len() && line[x] {
                    x += 1;
                }
                // the run, within the pane
                let (x0, x1) = ((start * BLOCK) as i32, (x * BLOCK) as i32);
                let (y0, y1) = (((row * BLOCK) as i32).max(py), ((row + 1) * BLOCK) as i32);
                let y1 = y1.min(py + ph);
                if y1 <= y0 {
                    continue;
                }
                let square = [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32];
                let uniforms = [(loc[2], 4, square), (loc[3], 4, p)];
                gfx.custom(
                    *prog,
                    &uniforms,
                    &[(loc[0], self.tex), (loc[1], self.still)],
                    x0 as f64,
                    y0 as f64,
                    x1 as f64,
                    y1 as f64,
                    crate::gfx::Blend::Premultiplied,
                );
            }
        }
    }
}

/// A copy of a picture.
fn copied(s: &ImageSurface) -> ImageSurface {
    let out = crate::util::surface_rgb(s.width(), s.height());
    {
        let c = crate::util::ctx(&out);
        if c.set_source_surface(s, 0.0, 0.0).is_ok() {
            let _ = c.paint();
        }
    }
    out
}

/// A monitor's wall: its still picture, handed to the desktop (or shown in a window of our own), and
/// the live window over it. `lead`: the one whose picture a desktop of one picture shows.
fn open_monitor(
    d: &mut impl Display,
    s: &Screen,
    th: &Arc<Theme>,
    header: Header,
    edges: Edges,
    o: &Options,
    w: &mut World,
    desktop: Option<&dyn Desktop>,
    lead: bool,
    // the desktop's icons are followed: the still picture is kept for laying them over the live one
    keep: bool,
) -> Result<Monitor, String> {
    let mut still = crate::util::surface_rgb(s.w, s.h);
    let mut mon = {
        let c = cairo::Context::new(&still).map_err(|e| e.to_string())?;
        Monitor::new(d, s, th.clone(), &o.host, header, edges, w, Some(&c))?
    };
    if keep {
        mon.picture = Some(copied(&still));
    }
    match desktop {
        Some(desk) => {
            if desk.per_monitor() || lead {
                crate::still::set_wallpaper(th, desk, s, &mut still);
            }
        }
        None => match d.window(s.x, s.y, s.w, s.h) {
            Ok(win) => {
                let mut up = Uploads::default();
                let t = up.add(still, crate::gfx::Kind::Layer);
                let mut g = Gfx::new((0, 0, s.w, s.h), d.core(win), up);
                g.layer(t, None, 1.0, crate::gfx::Blend::Opaque);
                g.flush();
                d.swap(win);
                mon.backdrop = Some((win, g, t));
                d.make_current(mon.window);
                d.raise(mon.window);
            }
            Err(e) => crate::errln!("{}: the still picture's window: {e}", s.name),
        },
    }
    crate::platform::release_free_memory();
    Ok(mon)
}

/// Whether a GL renderer, by its name, draws on the CPU.
pub fn software(renderer: &str) -> bool {
    ["llvmpipe", "softpipe", "softrast", "Software Renderer"].iter().any(|s| renderer.contains(s))
}

/// Set in the environment of a run through VirtualGL, so it is not run through it again.
#[cfg(target_os = "linux")]
const THROUGH_VIRTUALGL: &str = "RETRO_VIRTUALGL";

/// On a display whose OpenGL is software only (Xephyr, Xvnc), run again through VirtualGL where it is
/// installed: the GPU draws (its first card, through EGL), and VirtualGL hands each frame to the display. Asked
/// of a window of one pixel before anything starts; without vglrun, nothing changes.
#[cfg(target_os = "linux")]
fn through_virtualgl(d: &mut impl Display) {
    use std::os::unix::process::CommandExt;
    if std::env::var_os(THROUGH_VIRTUALGL).is_some() {
        return;
    }
    let Ok(probe) = d.window(0, 0, 1, 1) else { return };
    d.make_current(probe);
    let r = d.renderer();
    d.close(probe);
    if !software(&r) {
        return;
    }
    let Some(vglrun) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("vglrun")).find(|f| f.is_file())) else { return };
    let Ok(exe) = std::env::current_exe() else { return };
    // VirtualGL's EGL back end takes a card's device, not its render node
    let mut cards: Vec<std::path::PathBuf> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("card")))
        .collect();
    cards.sort();
    let mut run = std::process::Command::new(&vglrun);
    match cards.first() {
        Some(node) => {
            crate::outln!("software OpenGL ({r}): again through VirtualGL, on {}", node.display());
            run.arg("-d").arg(node);
        }
        None => crate::outln!("software OpenGL ({r}): again through VirtualGL"),
    }
    let e = run.arg(exe).args(std::env::args_os().skip(1)).env(THROUGH_VIRTUALGL, "1").exec();
    crate::errln!("{}: {e}", vglrun.display());
}

/// Set the still picture of each monitor the wall drives, and exit.
pub fn wallpapers(loaded: &Loaded, o: &Options) {
    let d = Native::open().unwrap_or_else(|e| fail(&e));
    let Some(desktop) = Native::desktop() else {
        crate::outln!("no desktop shows a picture here: retro-wall shows it itself");
        return;
    };
    let th = loaded.look(crate::day::lighting_phase(loaded).as_deref());
    let want = wanted(&d, &th, o);
    let lead = lead(&want);
    for (s, header, edges) in want {
        if desktop.per_monitor() || lead.as_deref() == Some(s.name.as_str()) {
            let g = Geo::new(s.w, s.h, &o.host, edges, header, Some(s.name.clone()), th.clone());
            match crate::still::draw(&g) {
                Ok(mut pic) => crate::still::set_wallpaper(&th, desktop.as_ref(), &s, &mut pic),
                Err(e) => crate::errln!("{}: {e}", s.name),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_check_is_due_on_its_own_pace() {
        let mut t = Timers::new();
        let d = t.tick(0.0);
        assert!(d.watch && d.look && d.restack);
        let d = t.tick(Timers::LOOK);
        assert!(d.look && !d.watch && !d.restack);
        let d = t.tick(Timers::WATCH);
        assert!(d.watch && d.look && !d.restack);
        let d = t.tick(Timers::RESTACK);
        assert!(d.watch && d.look && d.restack);
        // a late tick moves each on from now, not from when it was due: no burst to catch up
        let d = t.tick(1000.0);
        assert!(d.watch && d.look && d.restack);
        assert!(!t.tick(1000.5).look);
    }

    #[test]
    fn it_sleeps_to_the_soonest_of_frames_and_checks() {
        let t = Timers { watch: 10.0, look: 10.5, restack: 20.0 };
        assert_eq!(t.sleep(9.0, [None]), Duration::from_secs(1));
        assert_eq!(t.sleep(9.0, [Some(9.25), Some(9.5), None]), Duration::from_millis(250));
        assert_eq!(t.sleep(11.0, []), Duration::ZERO);
        assert_eq!(t.sleep(0.0, []), crate::util::seconds(Timers::NAP));
        assert_eq!(t.sleep(9.0, [Some(f64::INFINITY)]), Duration::from_secs(1));
    }
}
