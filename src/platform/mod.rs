//! What differs between operating systems, behind one interface: the display (monitors, windows with
//! their GL contexts, events, what covers the desktop, whether the screen is off), the machine (power,
//! the built-in readings' counters, a VNC viewer) and the desktop's own picture. `Native` is this
//! system's implementation; nothing outside this module names it.

use std::time::Duration;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::Linux as Native;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::Mac as Native;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use windows::Windows as Native;

/// Make the font file at `path` available to cairo's text, by the family name inside it, for this
/// process only and before the first lookup of that family. Each system implements it with its own
/// font machinery: fontconfig on Linux, Core Text on macOS, FreeType faces of its own on Windows.
#[cfg(target_os = "linux")]
pub use linux::register_font;
#[cfg(target_os = "macos")]
pub use macos::register_font;
#[cfg(windows)]
pub use windows::register_font;

/// Give the memory the allocator holds free back to the system, as after a scene is built: its
/// screen-sized pictures are freed by then.
#[cfg(target_os = "linux")]
pub use linux::release_free_memory;
#[cfg(target_os = "macos")]
pub use macos::release_free_memory;
#[cfg(windows)]
pub use windows::release_free_memory;

/// A monitor, by the name the system gives it, and where it lies on the desktop, in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// Pixels along a monitor's sides kept by the desktop (panels, a taskbar).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Edges {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Edges {
    /// Nothing kept.
    pub const NONE: Edges = Edges { top: 0, right: 0, bottom: 0, left: 0 };
}

/// A rectangle on the desktop, in pixels: its left and top edges, and past its right and bottom ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl Rect {
    pub const fn new(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
        Rect { left, top, right, bottom }
    }
}

/// A window of ours, as the display knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// The machine's power. A machine without a battery: no charge, on mains.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Power {
    /// The battery's charge, in percent.
    pub charge: f64,
    pub on_battery: bool,
    /// Whether there is a battery at all.
    pub battery: bool,
}

/// The display: one connection, many windows, each with a GL context (all sharing one another's
/// objects), made current one at a time.
pub trait Display {
    /// The monitors.
    fn screens(&self) -> Vec<Screen>;
    /// What the desktop keeps along each side of a monitor.
    fn work_edges(&self, s: &Screen) -> Edges;
    /// A wallpaper window at (x, y), w by h: below every other window, on every workspace, off the
    /// taskbar, letting clicks through. Its GL context is current on return.
    fn window(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<WindowId, String>;
    /// Whether the window's context is OpenGL 3.3 core (else 2.1).
    fn core(&self, w: WindowId) -> bool;
    fn make_current(&mut self, w: WindowId);
    fn swap(&self, w: WindowId);
    /// To the top of the desktop's windows (still below every other).
    fn raise(&self, w: WindowId);
    fn close(&mut self, w: WindowId);
    /// The current context's renderer, as the driver names it.
    fn renderer(&self) -> String;
    /// Events since the last call: the windows that need drawing again.
    fn events(&mut self) -> Vec<WindowId>;
    /// Sleep up to `timeout`, waking early when events arrive.
    fn wait(&self, timeout: Duration);
    /// The rectangles of other windows over the desktop; None when it cannot be told.
    fn covering(&self) -> Option<Vec<Rect>>;
    /// Whether nobody can see the screens: powered down, or a laptop's lid closed on its own panel.
    fn screen_off(&self) -> bool;
}

/// The machine and its desktop.
pub trait Platform {
    type Display: Display;
    fn open() -> Result<Self::Display, String>;
    fn power() -> Power;
    /// Whether a viewer is connected to a VNC server on this TCP port.
    fn vnc_connected(port: u16) -> bool;
    /// (idle, total) CPU time so far, in any unit.
    fn cpu_times() -> Option<(u64, u64)>;
    /// (memory used, swap used), in percent.
    fn memory() -> Option<(f64, f64)>;
    /// Disk reads and writes completed so far, over the physical disks.
    fn disk_ios() -> u64;
    /// (used, free) of the drive holding `path`, in percent.
    fn drive_space(path: &str) -> Option<(f64, f64)>;
    /// {interface: (received, sent)} bytes so far: of the physical interfaces, or of `only`.
    fn net_counts(only: &[String]) -> std::collections::HashMap<String, (u64, u64)>;
    /// (this machine's address, its default router's).
    fn addresses() -> (Option<String>, Option<String>);
    /// The machine's name, short.
    fn hostname() -> String;
    /// The desktop that shows a picture under ours, if one does; None: we show the still picture.
    fn desktop() -> Option<Box<dyn Desktop>>;
    /// A desktop by the name its `Desktop::name` gives, running or not.
    fn desktop_named(name: &str) -> Option<Box<dyn Desktop>>;
    /// Name the process, for ps and top.
    fn name_process(name: &str);
}

/// A desktop environment's wallpaper settings.
pub trait Desktop {
    fn name(&self) -> &'static str;
    /// Whether each monitor has a picture of its own; else one picture serves them all.
    fn per_monitor(&self) -> bool {
        false
    }
    /// Show the picture at `path` on the monitor, centred on `colour` (#rrggbb).
    fn show(&self, s: &Screen, path: &str, colour: &str);
    /// The wallpaper settings as they are, as text that `restore` takes.
    fn save(&self) -> Result<String, String>;
    /// Make the settings what `saved` says.
    fn restore(&self, saved: &str) -> Result<(), String>;
}

/// A default route: its next hop (none through a point-to-point tunnel) and its metric.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub gateway: Option<String>,
    pub metric: i64,
}

/// The router the status line shows: the next hop of the best (lowest-metric) default route; none
/// when that route has none (a VPN tunnel).
pub fn gateway(routes: &[Route]) -> Option<String> {
    routes.iter().min_by_key(|r| r.metric)?.gateway.clone()
}

/// Whether the rectangles cover every pixel of `area`.
pub fn covers(rects: &[Rect], area: Rect) -> bool {
    let Rect { left: x0, top: y0, right: x1, bottom: y1 } = area;
    if x1 <= x0 || y1 <= y0 {
        return true;
    }
    let clipped: Vec<(f64, f64, f64, f64)> = rects
        .iter()
        .map(|r| (r.left.max(x0) as f64, r.top.max(y0) as f64, r.right.min(x1) as f64, r.bottom.min(y1) as f64))
        .filter(|r| r.2 > r.0 && r.3 > r.1)
        .collect();
    let covered: f64 = crate::scene::disjoint(&clipped).iter().map(|r| (r.2 - r.0) * (r.3 - r.1)).sum();
    covered >= ((x1 - x0) * (y1 - y0)) as f64
}
