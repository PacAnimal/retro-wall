//! The live picture: on each monitor it covers, one window over the part that moves, drawn from the
//! scene's plan and presented only when it changes, all from one loop. Pauses on battery, with the
//! screen off or a VNC viewer connected (instruments fade, the scene slows to a stop, the loop
//! sleeps), skips drawing a monitor while it is covered, and follows monitors as they come and go.

use crate::cli::Reserve;
use crate::config::{Choice, Loaded, Monitors, Rule, Theme};
use crate::gfx::{Gfx, Tex, Uploads};
use crate::platform::{Desktop, Display, Edges, Native, Platform, Screen, WindowId};
use crate::scene::*;
use crate::sources::{self, Traffic};
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
        let head = live.iter().map(|&i| parts[i].top(&geo)).min().unwrap_or(geo.h - 1).min(geo.h - 1).max(0);
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
            "{}: {} at {}x{} from row {head}, {} textures, made in {:.2} s, on {} ({})",
            s.name,
            geo.th.name,
            geo.w,
            geo.h,
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
/// or a list of names.
pub fn header(rules: &Monitors, name: &str, monitors: &[Screen], alone: bool) -> Header {
    let mut out = Header::NONE;
    for (part, choice) in [(Piece::Title, &rules.title), (Piece::Widgets, &rules.widgets), (Piece::Graph, &rules.graph)] {
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

/// One frame from fixed inputs, read back from GL, saved as a PNG.
pub fn snapshot(loaded: &Loaded, path: &str, host: &str, show_header: bool, at: f64, reserve: &Reserve) {
    let mut file = std::fs::File::create(path).unwrap_or_else(|e| {
        crate::errln!("--snapshot {path}: {e}");
        std::process::exit(2)
    });
    let mut d = Native::open().unwrap_or_else(|e| fail(&e));
    let s = pick(&d.screens(), None).unwrap_or_else(|| fail("no monitor"));
    let th = loaded.look(crate::day::lighting_phase(loaded).as_deref());
    let header = if show_header { header(&th.monitors, &s.name, std::slice::from_ref(&s), true) } else { Header::NONE };
    let mut w = world(&th, Some(Frozen { at, mains: true }), true);
    w.traffic.sample(at, true);
    w.traffic.held = true;
    w.network = sources::network_demo();
    let mut out = cairo::ImageSurface::create(cairo::Format::Rgb24, s.w, s.h).unwrap_or_else(|e| fail(&format!("a {}x{} picture: {e}", s.w, s.h)));
    let made = cairo::Context::new(&out).map_err(|e| e.to_string()).and_then(|c| {
        let edges = reserve.apply(&s, d.work_edges(&s));
        Monitor::new(&mut d, &s, th, host, header, edges, &mut w, Some(&c))
    });
    let mut m = made.unwrap_or_else(|e| fail(&e));
    m.state_at(at, &mut w);
    // the faces' first answers, as live shows them; a face that does not answer fails at its deadline
    let until = Instant::now() + crate::plugins::FACE_DEADLINE + Duration::from_millis(500);
    while m.parts.iter().any(|p| p.waiting()) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
        m.state_at(at, &mut w);
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
    if let Err(e) = out.write_to_png(&mut file) {
        crate::errln!("--snapshot {path}: {e}");
        std::process::exit(2)
    }
    crate::outln!("{path}: written");
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
    if o.pause_vnc.is_some_and(Native::vnc_connected) {
        return Some("a VNC viewer is connected");
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
            if paused.is_none() || monitors.is_empty() {
                let want = wanted(&d, &th, &o);
                // gone, moved, resized, a panel moved, or the header now elsewhere: closed, and made again below
                let mut kept = vec![];
                for m in monitors.drain(..) {
                    if want.iter().any(|(s, h, e)| *s == m.screen && *h == m.geo.header && *e == m.geo.edges) {
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
                    match open_monitor(&mut d, &s, &th, h, e, &o, &mut w, desktop.as_deref(), is_lead) {
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
) -> Result<Monitor, String> {
    let mut still = crate::util::surface_rgb(s.w, s.h);
    let mut mon = {
        let c = cairo::Context::new(&still).map_err(|e| e.to_string())?;
        Monitor::new(d, s, th.clone(), &o.host, header, edges, w, Some(&c))?
    };
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
