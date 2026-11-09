//! The setup program's window, after Windows 11's Settings: a navigation pane (theme, screens,
//! instruments, pausing, advanced), each page's settings on rounded cards with a title, a line saying
//! what it does and its control, and Install (a first run) or Save, Apply and Close. Light or dark as
//! Windows is set, in the user's accent colour (`look`).
//!
//! The controls are Windows' own (lists, combo boxes, edit boxes, check boxes drawn as toggle switches,
//! radio buttons, buttons), each with a fixed id: keyboards, screen readers and the tests reach them as
//! any program's. Each control's place is kept in 96-dpi units and laid out again at the window's dpi,
//! so it is sharp at any scale and when moved to a monitor of another. Work that takes a moment (a
//! preview, an install, applying the settings) runs on a thread and reports back with a posted message,
//! so the window never stops answering.

use crate::look::{self, Kind, Palette, Rgb};
use crate::ops;
use retro_wall::platform::windows::wide;
use retro_wall::setup::settings::{OnBattery, Place, Screens, Settings};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, DT_CENTER, DT_SINGLELINE, DT_VCENTER, EndPaint, HBRUSH, HDC, HFONT, InvalidateRect, PAINTSTRUCT, SetBkColor, SetTextColor,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::{SS_ENDELLIPSIS, SS_LEFT, SS_LEFTNOWORDWRAP, SS_NOPREFIX};
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, CDDS_PREPAINT, CDIS_DISABLED, CDIS_FOCUS, CDIS_HOT, CDIS_SELECTED, CDRF_DODEFAULT, CDRF_SKIPDEFAULT, DRAWITEMSTRUCT,
    ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, MEASUREITEMSTRUCT, NM_CUSTOMDRAW, NMCUSTOMDRAW, NMHDR, ODS_SELECTED, SetWindowTheme,
};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// the controls' ids: the tests find the controls by them
pub const NAV: i32 = 100;
pub const THEMES: i32 = 201;
pub const ABOUT: i32 = 203;
pub const UI: i32 = 204;
pub const HAND: i32 = 205;
pub const SCREENS_MAIN: i32 = 211;
pub const SCREENS_ALL: i32 = 212;
pub const SCREENS_ONE: i32 = 213;
pub const MONITOR: i32 = 214;
pub const PLACE_TITLE: i32 = 215;
pub const PLACE_WIDGETS: i32 = 216;
pub const PLACE_GRAPH: i32 = 217;
pub const PLACE_STATUS: i32 = 218;
pub const SHOW_CPU: i32 = 221;
pub const SHOW_RAM: i32 = 222;
pub const SHOW_SWAP: i32 = 223;
pub const SHOW_DECK: i32 = 224;
pub const SHOW_STATUS: i32 = 225;
pub const BATTERY: i32 = 226;
pub const HOST_MACHINE: i32 = 227;
pub const HOST_OWN: i32 = 228;
pub const HOST_NAME: i32 = 229;
pub const OWN_LAYOUT: i32 = 230;
pub const ON_BATTERY: i32 = 231;
pub const BELOW: i32 = 232;
pub const SCREEN_OFF: i32 = 233;
pub const RDP: i32 = 234;
pub const RDP_PORT: i32 = 235;
pub const VNC: i32 = 236;
pub const VNC_PORT: i32 = 237;
pub const SOFTWARE: i32 = 241;
pub const INTERFACES: i32 = 242;
pub const IP_URL: i32 = 243;
pub const OPEN_FOLDER: i32 = 244;
pub const OPEN_LOG: i32 = 245;
pub const UNINSTALL: i32 = 246;
pub const STATUS: i32 = 301;
pub const OK: i32 = 302;
pub const APPLY: i32 = 303;
pub const CLOSE: i32 = 304;
pub const PAGE_TITLE: i32 = 310;
pub const MODE: i32 = 311;

const WM_PREVIEW: u32 = WM_APP + 1;
const WM_SAY: u32 = WM_APP + 2;
const WM_DONE: u32 = WM_APP + 3;
const WM_UNPACKED: u32 = WM_APP + 4;

/// The client area, in 96-dpi units; the navigation's width; the content's left and width.
const W: i32 = 1000;
const H: i32 = 680;
const CX: i32 = 272;
const CW: i32 = W - CX - 28;
/// The preview's place, on the theme page.
const PREVIEW: (i32, i32, i32, i32) = (604, 100, 352, 198);

/// The pages: title and icon (Segoe Fluent Icons).
const PAGES: [(&str, char); 5] =
    [("Theme", '\u{E771}'), ("Screens", '\u{E7F4}'), ("Instruments", '\u{E9D9}'), ("Pausing", '\u{E769}'), ("Advanced", '\u{E713}')];

/// What a label or control stands on, and so is painted over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Window,
    Card,
    Info,
}

/// A label's font and colour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Body,
    Secondary,
    Subtitle,
    Title,
}

/// A control: its window, its page (None: on every page) and its place at 96 dpi; how it is drawn.
struct Control {
    hwnd: HWND,
    page: Option<usize>,
    rect: (i32, i32, i32, i32),
    role: Role,
    surface: Surface,
    kind: Option<Kind>,
    /// An edit box, in a field the window draws.
    field: bool,
    /// On blocky's extras card: shown with it.
    extras: bool,
}

/// A card on a page; `extras`: only while the theme has blocky's extras.
struct Card {
    page: usize,
    rect: (i32, i32, i32, i32),
    extras: bool,
}

/// A theme as the list shows it.
#[derive(Clone)]
struct ThemeItem {
    name: String,
    shown: String,
    about: String,
    extras: bool,
}

struct State {
    hwnd: HWND,
    controls: Vec<(i32, Control)>,
    cards: Vec<Card>,
    palette: Palette,
    fonts: Option<look::Fonts>,
    /// The window's, a card's, the info bar's and a field's brush.
    brushes: Vec<look::Brush>,
    dpi: i32,
    preview: Option<cairo::ImageSurface>,
    status: String,
    page: usize,
    /// A first run: Install, not Save and Apply.
    installing: bool,
    /// The install is an update of one there.
    update: bool,
    busy: bool,
    /// A first run's themes being made ready (Cancel works then).
    unpacking: bool,
    extras_shown: bool,
    themes: Vec<ThemeItem>,
    monitors: Vec<String>,
    settings: Settings,
    /// Where the wall's program and themes are, for the previews: the install, or a first run's copy.
    wall: PathBuf,
    themes_dir: PathBuf,
    preview_asked: u32,
    /// What the window started with: Close asks only if this changed.
    shown: Settings,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // a control may call back while the state is in use (a message sent, not posted): it then gets None
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Show the window: the settings, or on a first run the install. Returns when it is closed.
pub fn run(installing: bool) -> Result<(), String> {
    // SAFETY: plain Win32 calls on this thread's window, its controls and its message loop.
    unsafe {
        // one window at a time: a second brings the first forward
        let name = wide("Local\\retro-wall-setup-window");
        let _mutex = windows_sys::Win32::System::Threading::CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if windows_sys::Win32::Foundation::GetLastError() == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS {
            let class = wide("retro-wall-setup");
            let other = FindWindowW(class.as_ptr(), std::ptr::null());
            if !other.is_null() {
                ShowWindow(other, SW_RESTORE);
                SetForegroundWindow(other);
            }
            if installing {
                let (t, m) = (wide("retro-wall"), wide("retro-wall's settings are open. Close them, then open this again to install."));
                MessageBoxW(other, m.as_ptr(), t.as_ptr(), MB_ICONINFORMATION | MB_OK);
            }
            return Ok(());
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_STANDARD_CLASSES };
        InitCommonControlsEx(&icc);
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("retro-wall-setup");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: instance,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            hIcon: LoadIconW(instance, 1 as _),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let title = wide(match (installing, ops::installed_version().is_some()) {
            (false, _) => "retro-wall settings",
            (true, false) => "Install retro-wall",
            (true, true) => "Update retro-wall",
        });
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            W,
            H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("the window could not be made".into());
        }
        // a first run's program and themes, for the previews and the list, are unpacked on a thread
        // (tens of megabytes): the window and its controls are there at once
        let (wall, themes_dir) = if installing {
            (preview_copy_dir().join("retro-wall.exe"), preview_copy_dir().join("themes"))
        } else {
            (ops::wall_exe(), ops::install_dir().join("themes"))
        };
        // the settings there are (an install's, or kept from one uninstalled); a first run's defaults else
        let settings = Settings::read(&ops::settings_dir());
        let mut themes = themes(&[themes_dir.clone(), ops::settings_dir().join("themes")]);
        if let Some(t) = settings.theme.as_ref().filter(|t| !themes.iter().any(|x| &x.name == *t)) {
            themes.push(ThemeItem { name: t.clone(), shown: format!("{t} (not found)"), about: String::new(), extras: false });
        }
        let palette = Palette::new(look::system_dark(), look::accent());
        let state = State {
            hwnd,
            controls: vec![],
            cards: vec![],
            palette,
            fonts: None,
            brushes: vec![],
            dpi: 96,
            preview: None,
            status: String::new(),
            page: 0,
            installing,
            update: installing && ops::installed_version().is_some(),
            busy: false,
            unpacking: false,
            extras_shown: false,
            themes,
            monitors: monitors(),
            shown: settings.clone(),
            settings,
            wall,
            themes_dir,
            preview_asked: 0,
        };
        STATE.with(|s| *s.borrow_mut() = Some(state));
        build(hwnd);
        colours();
        fill();
        // what Close compares with: the settings as the window shows them
        if let Ok(shown) = read_back() {
            with(|s| s.shown = shown);
        }
        relayout();
        show_page(0);
        ShowWindow(hwnd, SW_SHOW);
        if installing {
            with(|s| {
                s.busy = true;
                s.unpacking = true;
            });
            busy(true);
            // Cancel still works while the themes are made ready: nothing is done yet
            EnableWindow(hwnd_of(CLOSE), 1);
            say("Getting the themes ready…");
            let h = hwnd as usize;
            std::thread::spawn(move || {
                let r = Box::into_raw(Box::new(ops::unpack_to(&preview_copy_dir())));
                // SAFETY: the window frees the box when it takes the message.
                PostMessageW(h as HWND, WM_UNPACKED, 0, r as LPARAM);
            });
        } else {
            ask_preview();
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    if installing {
        let _ = std::fs::remove_dir_all(preview_copy_dir());
    }
    Ok(())
}

fn preview_copy_dir() -> PathBuf {
    std::env::temp_dir().join(format!("retro-wall-setup-{}", std::process::id()))
}

/// What a theme is, in a line: the first sentence of its theme.toml's opening comment
/// ("# blocky: a block game's world as ... farm:").
fn about(text: &str) -> String {
    let first = text.lines().take_while(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim()).collect::<Vec<_>>().join(" ");
    let body = first.split_once(':').map_or(first.as_str(), |(_, rest)| rest).trim();
    let line: String = body.chars().take_while(|c| !matches!(c, ':' | ';' | '.')).collect();
    let mut c = line.trim().chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// The themes there are, bundled and the user's own. The default first; the setup program's own themes
/// (its settings for a theme's extras) left out.
fn themes(dirs: &[PathBuf]) -> Vec<ThemeItem> {
    let mut out: Vec<ThemeItem> = vec![];
    for e in dirs.iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(retro_wall::setup::settings::OWN_THEME) || out.iter().any(|t| t.name == name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path().join("theme.toml")) else { continue };
        let shown = text.lines().find_map(|l| l.strip_prefix("name = \"").and_then(|r| r.strip_suffix('"')).map(String::from)).unwrap_or_else(|| name.clone());
        out.push(ThemeItem { about: about(&text), extras: retro_wall::elements::blocks::in_theme(&text), name, shown });
    }
    out.sort_by_key(|t| (t.name != "phosphor", t.shown.to_lowercase()));
    if let Some(p) = out.iter_mut().find(|t| t.name == "phosphor") {
        p.shown = format!("{} (the default)", p.shown);
    }
    out
}

/// The monitors, by the names the wall knows them by (DISPLAY1), the main one first.
fn monitors() -> Vec<String> {
    use retro_wall::platform::{Display, Native, Platform};
    let mut screens = Native::open().map(|d| d.screens()).unwrap_or_default();
    screens.sort_by_key(|s| (!s.primary, s.x, s.y));
    screens.into_iter().map(|s| s.name).collect()
}

// --- building ------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn add(
    id: i32,
    class: &str,
    text: &str,
    style: u32,
    page: Option<usize>,
    rect: (i32, i32, i32, i32),
    role: Role,
    surface: Surface,
    kind: Option<Kind>,
) -> HWND {
    let Some(parent) = with(|s| s.hwnd) else { return std::ptr::null_mut() };
    let (c, t) = (wide(class), wide(text));
    // on every page: shown from the start (the info bar once it has something to say)
    let visible = if page.is_none() && id != STATUS { WS_VISIBLE } else { 0 };
    // SAFETY: a child window of our window, with our id; destroyed with it.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            c.as_ptr(),
            t.as_ptr(),
            WS_CHILD | visible | style,
            0,
            0,
            10,
            10,
            parent,
            id as usize as _,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    let field = class == "EDIT";
    with(|s| s.controls.push((id, Control { hwnd, page, rect, role, surface, kind, field, extras: false })));
    hwnd
}

fn label(id: i32, text: &str, page: Option<usize>, rect: (i32, i32, i32, i32), role: Role, surface: Surface) {
    let wrap = if role == Role::Secondary && rect.3 > 22 { 0 } else { SS_LEFTNOWORDWRAP | SS_ENDELLIPSIS };
    add(id, "STATIC", text, SS_LEFT | SS_NOPREFIX | wrap, page, rect, role, surface, None);
}

/// A label on blocky's extras card, shown with it.
fn extras_label(text: &str, rect: (i32, i32, i32, i32), role: Role, surface: Surface) {
    label(0, text, Some(0), rect, role, surface);
    with(|s| {
        if let Some((_, c)) = s.controls.last_mut() {
            c.extras = true;
        }
    });
}

fn card(page: usize, rect: (i32, i32, i32, i32), extras: bool) {
    with(|s| s.cards.push(Card { page, rect, extras }));
}

/// A setting's card: its title and what it does on the left; the caller puts its control on the right.
fn row(page: usize, y: i32, h: i32, title: &str, about: &str) {
    card(page, (CX, y, CW, h), false);
    let top = y + (h - 40) / 2;
    label(0, title, Some(page), (CX + 16, top, CW - 360, 20), Role::Body, Surface::Card);
    label(0, about, Some(page), (CX + 16, top + 20, CW - 360, 18), Role::Secondary, Surface::Card);
}

/// A toggle switch at the right of a card at `y`, `h` tall.
fn toggle(id: i32, name: &str, page: usize, y: i32, h: i32) {
    add(
        id,
        "BUTTON",
        name,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        Some(page),
        (CX + CW - 16 - 110, y + (h - 40) / 2, 110, 40),
        Role::Body,
        Surface::Card,
        Some(Kind::Toggle),
    );
}

const COMBO: u32 = WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32;
/// An edit box: no border of its own, the window draws its field around it (`FIELD_INSET`).
const EDITBOX: u32 = WS_TABSTOP | ES_AUTOHSCROLL as u32;
/// An edit box inside its field: left, top, right, bottom, at 96 dpi.
const FIELD_INSET: (i32, i32, i32, i32) = (9, 5, 8, 4);
const RADIO: u32 = BS_AUTORADIOBUTTON as u32;

/// Every control and card, at its place at 96 dpi.
fn build(_w: HWND) {
    let list = WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32;
    // the navigation
    label(0, "retro-wall", None, (24, 22, 210, 30), Role::Subtitle, Surface::Window);
    label(MODE, "", None, (24, 54, 210, 20), Role::Secondary, Surface::Window);
    add(NAV, "LISTBOX", "Pages", list, None, (8, 92, 232, 5 * 40), Role::Body, Surface::Window, None);
    label(0, &format!("Version {}", env!("CARGO_PKG_VERSION")), None, (24, H - 36, 210, 20), Role::Secondary, Surface::Window);
    // the page's title
    label(PAGE_TITLE, "", None, (CX, 20, CW, 42), Role::Title, Surface::Window);

    // theme: the list, the preview with what the theme is, and blocky's extras
    card(0, (CX, 84, 300, 500), false);
    add(THEMES, "LISTBOX", "Themes", list, Some(0), (CX + 6, 90, 288, 488), Role::Body, Surface::Card, None);
    card(0, (588, 84, 384, 300), false);
    label(ABOUT, "", Some(0), (604, 308, 352, 60), Role::Secondary, Surface::Card);
    card(0, (588, 396, 384, 128), true);
    extras_label("Game interface", (604, 410, 230, 20), Role::Body, Surface::Card);
    extras_label("Hotbar, hearts and food", (604, 430, 230, 18), Role::Secondary, Surface::Card);
    add(
        UI,
        "BUTTON",
        "Game interface",
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        Some(0),
        (588 + 384 - 16 - 110, 404, 110, 40),
        Role::Body,
        Surface::Card,
        Some(Kind::Toggle),
    );
    extras_label("Player's hand", (604, 470, 230, 20), Role::Body, Surface::Card);
    extras_label("At the lower right", (604, 490, 230, 18), Role::Secondary, Surface::Card);
    add(
        HAND,
        "BUTTON",
        "Player's hand",
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        Some(0),
        (588 + 384 - 16 - 110, 464, 110, 40),
        Role::Body,
        Surface::Card,
        Some(Kind::Toggle),
    );

    // screens
    card(1, (CX, 84, CW, 148), false);
    label(0, "Show the wallpaper on", Some(1), (CX + 16, 96, 500, 20), Role::Body, Surface::Card);
    label(0, "Each screen gets a wallpaper of its own size", Some(1), (CX + 16, 116, 500, 18), Role::Secondary, Surface::Card);
    add(
        SCREENS_MAIN,
        "BUTTON",
        "The main screen only",
        RADIO | WS_GROUP | WS_TABSTOP,
        Some(1),
        (CX + 16, 142, 320, 26),
        Role::Body,
        Surface::Card,
        Some(Kind::Radio),
    );
    add(SCREENS_ALL, "BUTTON", "Every screen, as they come and go", RADIO, Some(1), (CX + 16, 168, 320, 26), Role::Body, Surface::Card, Some(Kind::Radio));
    add(SCREENS_ONE, "BUTTON", "One screen:", RADIO, Some(1), (CX + 16, 194, 150, 26), Role::Body, Surface::Card, Some(Kind::Radio));
    add(MONITOR, "COMBOBOX", "Screen", COMBO | WS_GROUP, Some(1), (CX + 170, 194, 240, 300), Role::Body, Surface::Card, None);
    label(0, "On several screens", Some(1), (CX, 252, CW, 28), Role::Subtitle, Surface::Window);
    label(0, "Where each part shows. One screen alone shows them all.", Some(1), (CX, 282, CW, 20), Role::Secondary, Surface::Window);
    for (i, (id, name)) in
        [(PLACE_TITLE, "Hostname"), (PLACE_WIDGETS, "Instruments"), (PLACE_GRAPH, "Traffic graph"), (PLACE_STATUS, "Status line")].into_iter().enumerate()
    {
        let y = 310 + i as i32 * 56;
        card(1, (CX, y, CW, 52), false);
        label(0, name, Some(1), (CX + 16, y + 16, 300, 20), Role::Body, Surface::Card);
        add(id, "COMBOBOX", name, COMBO, Some(1), (CX + CW - 16 - 260, y + 12, 260, 300), Role::Body, Surface::Card, None);
    }

    // instruments
    label(OWN_LAYOUT, "", Some(2), (CX, 62, CW, 20), Role::Secondary, Surface::Window);
    for (i, (id, name, what)) in [
        (SHOW_CPU, "CPU", "How busy the processor is"),
        (SHOW_RAM, "Memory", "How much of it is in use"),
        (SHOW_SWAP, "Swap", "How much of the page file is in use"),
        (SHOW_DECK, "Disk", "The tape deck: its reels turn as the disk works"),
        (SHOW_STATUS, "Network status line", "This computer's addresses and its connection"),
        (BATTERY, "Battery meter", "The battery's charge, for a laptop"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 88 + i as i32 * 58;
        row(2, y, 54, name, what);
        toggle(id, name, 2, y, 54);
    }
    card(2, (CX, 436, CW, 112), false);
    label(0, "Name shown", Some(2), (CX + 16, 448, 300, 20), Role::Body, Surface::Card);
    label(0, "The big lettering", Some(2), (CX + 16, 468, 300, 18), Role::Secondary, Surface::Card);
    add(
        HOST_MACHINE,
        "BUTTON",
        "This computer's name",
        RADIO | WS_GROUP | WS_TABSTOP,
        Some(2),
        (CX + 16, 494, 260, 26),
        Role::Body,
        Surface::Card,
        Some(Kind::Radio),
    );
    add(HOST_OWN, "BUTTON", "Another name:", RADIO, Some(2), (CX + 16, 520, 150, 26), Role::Body, Surface::Card, Some(Kind::Radio));
    add(HOST_NAME, "EDIT", "Name shown", EDITBOX | WS_GROUP, Some(2), (CX + 170, 519, 300, 28), Role::Body, Surface::Card, None);

    // pausing
    row(3, 84, 64, "On battery power", "Paused, the wallpaper stands still and costs nothing");
    add(ON_BATTERY, "COMBOBOX", "On battery power", COMBO, Some(3), (CX + CW - 16 - 300, 101, 220, 200), Role::Body, Surface::Card, None);
    add(BELOW, "EDIT", "Charge", EDITBOX | ES_NUMBER as u32, Some(3), (CX + CW - 16 - 72, 101, 48, 28), Role::Body, Surface::Card, None);
    label(0, "%", Some(3), (CX + CW - 16 - 20, 105, 20, 20), Role::Body, Surface::Card);
    row(3, 152, 64, "Pause while the screen is off", "Or while a laptop's lid is closed");
    toggle(SCREEN_OFF, "Pause while the screen is off", 3, 152, 64);
    row(3, 220, 64, "Pause during a Remote Desktop connection", "A viewer is sent no animation. Port: 3389 for Windows' own");
    add(RDP_PORT, "EDIT", "Remote Desktop port", EDITBOX | ES_NUMBER as u32, Some(3), (CX + CW - 16 - 110 - 96, 238, 80, 28), Role::Body, Surface::Card, None);
    toggle(RDP, "Pause during a Remote Desktop connection", 3, 220, 64);
    row(3, 288, 64, "Pause while a VNC viewer is connected", "The port of this computer's VNC server");
    add(VNC_PORT, "EDIT", "VNC port", EDITBOX | ES_NUMBER as u32, Some(3), (CX + CW - 16 - 110 - 96, 306, 80, 28), Role::Body, Surface::Card, None);
    toggle(VNC, "Pause while a VNC viewer is connected", 3, 288, 64);

    // advanced
    row(4, 84, 64, "Animate without a graphics driver", "Software drawing: slow. For a computer without one");
    toggle(SOFTWARE, "Animate without a graphics driver", 4, 84, 64);
    row(4, 152, 64, "Network adapters to count", "Commas between. None: the physical ones");
    add(INTERFACES, "EDIT", "Network adapters to count", EDITBOX, Some(4), (CX + CW - 16 - 310, 170, 310, 28), Role::Body, Surface::Card, None);
    row(4, 220, 64, "Public address from", "Where the wall asks for it. None: the default");
    add(IP_URL, "EDIT", "Public address from", EDITBOX, Some(4), (CX + CW - 16 - 310, 238, 310, 28), Role::Body, Surface::Card, None);
    row(4, 288, 64, "Files", "Your settings, and the wallpaper's log");
    add(
        OPEN_FOLDER,
        "BUTTON",
        "Settings folder",
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        Some(4),
        (CX + CW - 16 - 140 - 8 - 160, 304, 160, 32),
        Role::Body,
        Surface::Card,
        Some(Kind::Push),
    );
    add(OPEN_LOG, "BUTTON", "Log", WS_TABSTOP | BS_PUSHBUTTON as u32, Some(4), (CX + CW - 16 - 140, 304, 140, 32), Role::Body, Surface::Card, Some(Kind::Push));
    row(4, 356, 64, "Uninstall retro-wall", "Your own wallpaper comes back");
    add(
        UNINSTALL,
        "BUTTON",
        "Uninstall…",
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        Some(4),
        (CX + CW - 16 - 140, 372, 140, 32),
        Role::Body,
        Surface::Card,
        Some(Kind::Danger),
    );

    // on every page: the info bar and the buttons
    label(STATUS, "", None, (CX + 40, H - 50, CW - 340 - 48, 28), Role::Body, Surface::Info);
    add(OK, "BUTTON", "Save", WS_TABSTOP | BS_PUSHBUTTON as u32, None, (W - 28 - 316, H - 52, 100, 32), Role::Body, Surface::Window, Some(Kind::Primary));
    add(APPLY, "BUTTON", "Apply", WS_TABSTOP | BS_PUSHBUTTON as u32, None, (W - 28 - 208, H - 52, 100, 32), Role::Body, Surface::Window, Some(Kind::Push));
    add(CLOSE, "BUTTON", "Close", WS_TABSTOP | BS_PUSHBUTTON as u32, None, (W - 28 - 100, H - 52, 100, 32), Role::Body, Surface::Window, Some(Kind::Push));
    // what to type, shown in an empty field
    for (id, hint) in [(INTERFACES, "Ethernet, Wi-Fi"), (IP_URL, "https://ip.c-net.org/")] {
        let t = wide(hint);
        // SAFETY: a string that outlives the call, to our edit box (EM_SETCUEBANNER; shown with the focus too).
        unsafe { SendMessageW(hwnd_of(id), 0x1501, 1, t.as_ptr() as LPARAM) };
    }
    let nav = hwnd_of(NAV);
    for (name, _) in PAGES {
        let t = wide(name);
        // SAFETY: a string that outlives the call, to our list.
        unsafe { SendMessageW(nav, LB_ADDSTRING, 0, t.as_ptr() as LPARAM) };
    }
}

fn hwnd_of(id: i32) -> HWND {
    with(|s| s.controls.iter().find(|c| c.0 == id).map(|c| c.1.hwnd)).flatten().unwrap_or(std::ptr::null_mut())
}

/// Light or dark, as Windows is set now: the title bar, the brushes, the controls' own drawing.
/// (Here and below, the controls are told outside the state's borrow: a control may redraw at once and
/// ask the window how.)
fn colours() {
    let Some((hwnd, dark, window, controls)) = with(|s| {
        s.palette = Palette::new(look::system_dark(), look::accent());
        let p = s.palette;
        s.brushes = vec![look::Brush::new(p.window), look::Brush::new(p.card), look::Brush::new(p.info), look::Brush::new(p.field)];
        (s.hwnd, p.dark, p.window, s.controls.iter().map(|(_, c)| c.hwnd).collect::<Vec<_>>())
    }) else {
        return;
    };
    // SAFETY: attributes of our own window and themes of our own controls.
    unsafe {
        let d: i32 = dark as i32;
        DwmSetWindowAttribute(hwnd, 20, (&d as *const i32).cast(), 4); // DWMWA_USE_IMMERSIVE_DARK_MODE
        let caption = look::colorref(window);
        DwmSetWindowAttribute(hwnd, 35, (&caption as *const u32).cast(), 4); // DWMWA_CAPTION_COLOR: one with the window
        for h in controls {
            let mut class = [0u16; 32];
            let n = GetClassNameW(h, class.as_mut_ptr(), class.len() as i32);
            let class = String::from_utf16_lossy(&class[..n.max(0) as usize]).to_lowercase();
            let theme = match (dark, class.as_str()) {
                (true, "combobox" | "edit") => Some("DarkMode_CFD"),
                (true, "listbox") => Some("DarkMode_Explorer"),
                (false, "combobox" | "edit" | "listbox") => Some("Explorer"),
                _ => None,
            };
            if let Some(t) = theme {
                let t = wide(t);
                SetWindowTheme(h, t.as_ptr(), std::ptr::null());
            }
        }
        InvalidateRect(hwnd, std::ptr::null(), 1);
    }
}

/// The controls placed for the window's dpi, each in its font.
fn relayout() {
    let Some((hwnd, dpi, old, places)) = with(|s| {
        // SAFETY: our window.
        let dpi = unsafe { GetDpiForWindow(s.hwnd) }.max(96) as i32;
        s.dpi = dpi;
        let fonts = look::Fonts::new(dpi);
        let places: Vec<(i32, HWND, (i32, i32, i32, i32), HFONT)> = s
            .controls
            .iter()
            .map(|(id, c)| {
                let f = match c.role {
                    Role::Body => fonts.body,
                    Role::Secondary => fonts.small,
                    Role::Subtitle => fonts.subtitle,
                    Role::Title => fonts.title,
                };
                let (x, y, w, h) = c.rect;
                let rect = if c.field {
                    let (l, t, r, b) = FIELD_INSET;
                    (x + l, y + t, w - l - r, h - t - b)
                } else {
                    c.rect
                };
                (*id, c.hwnd, rect, f)
            })
            .collect();
        // the last fonts kept until the controls have the new ones
        (s.hwnd, dpi, s.fonts.replace(fonts), places)
    }) else {
        return;
    };
    let px = |v: i32| v * dpi / 96;
    // SAFETY: our windows and the fonts the state owns.
    unsafe {
        for (id, h, (x, y, w, hh), f) in places {
            SetWindowPos(h, std::ptr::null_mut(), px(x), px(y), px(w), px(hh), SWP_NOZORDER | SWP_NOACTIVATE);
            SendMessageW(h, WM_SETFONT, f as WPARAM, 1);
            if id == NAV || id == THEMES {
                SendMessageW(h, LB_SETITEMHEIGHT, 0, px(if id == NAV { 40 } else { 56 }) as LPARAM);
            }
        }
        // the window's client area W x H at this dpi
        let mut r = RECT { left: 0, top: 0, right: px(W), bottom: px(H) };
        AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, 0, WS_EX_CONTROLPARENT, dpi as u32);
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOMOVE | SWP_NOACTIVATE);
        InvalidateRect(hwnd, std::ptr::null(), 1);
    }
    drop(old);
}

fn show_page(page: usize) {
    // what to show, worked out first; the controls told outside the state's borrow, since a button or
    // list redraws at once and asks the window how (custom draw, an owner-drawn item)
    let Some((shown, installing, update, ok_at)) = with(|s| {
        s.page = page;
        let shown: Vec<(HWND, bool)> = s.controls.iter().filter_map(|(_, c)| c.page.map(|p| (c.hwnd, p == page))).collect();
        let ok_at = s.controls.iter_mut().find(|c| c.0 == OK).map(|(_, c)| {
            c.rect.0 = if s.installing { W - 28 - 208 } else { W - 28 - 316 };
            let px = |v: i32| v * s.dpi / 96;
            (c.hwnd, px(c.rect.0), px(c.rect.1), px(c.rect.2), px(c.rect.3))
        });
        (shown, s.installing, s.update, ok_at)
    }) else {
        return;
    };
    // SAFETY: our child windows.
    unsafe {
        for (h, on) in shown {
            ShowWindow(h, if on { SW_SHOW } else { SW_HIDE });
        }
        SendMessageW(hwnd_of(NAV), LB_SETCURSEL, page, 0);
        // a first run: Install (or Update) where Apply is, no Apply; Uninstall only once installed
        ShowWindow(hwnd_of(APPLY), if installing { SW_HIDE } else { SW_SHOW });
        let unin = !installing && page == 4;
        ShowWindow(hwnd_of(UNINSTALL), if unin { SW_SHOW } else { SW_HIDE });
        ShowWindow(hwnd_of(OPEN_LOG), if unin { SW_SHOW } else { SW_HIDE });
        if let Some((h, x, y, w, hh)) = ok_at {
            SetWindowPos(h, std::ptr::null_mut(), x, y, w, hh, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    set_text(hwnd_of(PAGE_TITLE), PAGES[page].0);
    set_text(
        hwnd_of(OK),
        match (installing, update) {
            (false, _) => "Save",
            (true, true) => "Update",
            (true, false) => "Install",
        },
    );
    set_text(hwnd_of(CLOSE), if installing { "Cancel" } else { "Close" });
    set_text(
        hwnd_of(MODE),
        match (installing, update) {
            (false, _) => "Settings",
            (true, true) => "Update",
            (true, false) => "Install",
        },
    );
    extras_shown();
    if let Some(h) = with(|s| s.hwnd) {
        // SAFETY: our window.
        unsafe { InvalidateRect(h, std::ptr::null(), 1) };
    }
}

fn hwnd_of_in(s: &State, id: i32) -> HWND {
    s.controls.iter().find(|c| c.0 == id).map_or(std::ptr::null_mut(), |c| c.1.hwnd)
}

/// Blocky's extras, shown only for a theme that has them, on its page; and what the theme is.
fn extras_shown() {
    let (has, about) = with(|s| {
        let t = selected_theme_in(s).map(|i| s.themes[i].clone());
        (s.page == 0 && t.as_ref().is_some_and(|t| t.extras), t.map(|t| t.about).unwrap_or_default())
    })
    .unwrap_or((false, String::new()));
    for id in [UI, HAND] {
        // SAFETY: our child windows.
        unsafe { ShowWindow(hwnd_of(id), if has { SW_SHOW } else { SW_HIDE }) };
    }
    set_text(hwnd_of(ABOUT), &about);
    with(|s| {
        s.extras_shown = has;
        // SAFETY: our window.
        unsafe { InvalidateRect(s.hwnd, std::ptr::null(), 0) };
    });
    // labels of the extras' card, shown with it
    let labels: Vec<HWND> = with(|s| s.controls.iter().filter(|(_, c)| c.extras).map(|(_, c)| c.hwnd).collect()).unwrap_or_default();
    for h in labels {
        // SAFETY: our child windows.
        unsafe { ShowWindow(h, if has { SW_SHOW } else { SW_HIDE }) };
    }
}

// --- painting ------------------------------------------------------------------------------------

/// The window: its colour, the page's cards, the preview, the info bar, the selected navigation item's
/// mark. The controls paint themselves over it.
fn paint(hwnd: HWND) {
    with(|s| {
        // SAFETY: a paint cycle of our own window.
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut client);
            let (p, k, page, extras) = (s.palette, s.dpi as f64 / 96.0, s.page, s.extras_shown);
            let px = |v: i32| v as f64 * k;
            let cards: Vec<(i32, i32, i32, i32)> = s.cards.iter().filter(|c| c.page == page && (!c.extras || extras)).map(|c| c.rect).collect();
            let status = !s.status.is_empty();
            let preview = s.preview.clone();
            // the edit boxes' fields: rounded, a line at the bottom, the accent's while typing in it
            let focus = GetFocus();
            let fields: Vec<((i32, i32, i32, i32), bool, bool)> = s
                .controls
                .iter()
                .filter(|(_, c)| c.field && c.page == Some(page))
                .map(|(_, c)| (c.rect, IsWindowEnabled(c.hwnd) != 0, c.hwnd == focus))
                .collect();
            look::shapes(dc, client, p.window, |cr| {
                for (x, y, w, h) in &cards {
                    look::card(cr, px(*x), px(*y), px(*w), px(*h), k, &p);
                }
                for ((x, y, w, h), enabled, focused) in &fields {
                    let (x, y, w, h) = (px(*x), px(*y), px(*w), px(*h));
                    look::rounded(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 4.0 * k);
                    look::fill(cr, if *enabled { p.field } else { p.card });
                    look::rounded(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 4.0 * k);
                    let (r, g, b) = p.field_line;
                    cr.set_source_rgb(r, g, b);
                    cr.set_line_width(1.0);
                    let _ = cr.stroke();
                    if *enabled {
                        // the bottom line, inside the rounding
                        let _ = cr.save();
                        look::rounded(cr, x, y, w, h, 4.0 * k);
                        cr.clip();
                        let (line, t) = if *focused { (p.accent, 2.0 * k) } else { (p.secondary, 1.0) };
                        cr.rectangle(x, y + h - t, w, t);
                        look::fill(cr, line);
                        let _ = cr.restore();
                    }
                }
                if status {
                    look::rounded(cr, px(CX), px(H - 54), px(CW - 340), px(36), 4.0 * k);
                    look::fill(cr, p.info);
                }
                if page == 0 {
                    let (x, y, w, h) = PREVIEW;
                    look::rounded(cr, px(x), px(y), px(w), px(h), 4.0 * k);
                    match &preview {
                        Some(img) => {
                            let _ = cr.save();
                            cr.clip();
                            let (iw, ih) = (img.width() as f64, img.height() as f64);
                            cr.translate(px(x), px(y));
                            cr.scale(px(w) / iw, px(h) / ih);
                            let _ = cr.set_source_surface(img, 0.0, 0.0);
                            let _ = cr.paint();
                            let _ = cr.restore();
                        }
                        None => look::fill(cr, p.subtle),
                    }
                }
            });
            if status && let Some(f) = s.fonts.as_ref() {
                let r = RECT { left: (px(CX) + 12.0 * k) as i32, top: (px(H - 54)) as i32, right: (px(CX) + 36.0 * k) as i32, bottom: px(H - 18) as i32 };
                look::text(dc, f.icon, p.accent, r, "\u{E946}", look::LINE);
            }
            if page == 0
                && preview.is_none()
                && let Some(f) = s.fonts.as_ref()
            {
                let (x, y, w, h) = PREVIEW;
                let r = RECT { left: px(x) as i32, top: px(y) as i32, right: px(x + w) as i32, bottom: px(y + h) as i32 };
                look::text(dc, f.body, p.secondary, r, "Drawing the preview…", DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            }
            EndPaint(hwnd, &ps);
        }
    });
}

/// A list's item: the navigation's (icon and page) or a theme (name and what it is).
fn draw_item(d: &DRAWITEMSTRUCT) {
    with(|s| {
        let (p, k) = (s.palette, s.dpi as f64 / 96.0);
        let Some(f) = s.fonts.as_ref() else { return };
        let selected = d.itemState & ODS_SELECTED != 0 && d.itemID != u32::MAX;
        let nav = d.CtlID == NAV as u32;
        let under = if nav { p.window } else { p.card };
        let r = d.rcItem;
        let (w, h) = ((r.right - r.left) as f64, (r.bottom - r.top) as f64);
        look::shapes(d.hDC, r, under, |cr| {
            if selected {
                look::rounded(cr, 4.0 * k, 2.0 * k, w - 8.0 * k, h - 4.0 * k, 4.0 * k);
                look::fill(cr, p.subtle);
                look::rounded(cr, 4.0 * k, h / 2.0 - 8.0 * k, 3.0 * k, 16.0 * k, 1.5 * k);
                look::fill(cr, p.accent);
            }
        });
        if d.itemID == u32::MAX {
            return;
        }
        let i = d.itemID as usize;
        let at = |l: f64, t: f64, rr: f64, b: f64| RECT {
            left: r.left + (l * k) as i32,
            top: r.top + (t * k) as i32,
            right: r.left + (rr * k) as i32,
            bottom: r.top + (b * k) as i32,
        };
        if nav {
            if let Some((name, icon)) = PAGES.get(i) {
                look::text(d.hDC, f.icon, p.text, at(16.0, 0.0, 44.0, h / k), &icon.to_string(), look::LINE);
                look::text(d.hDC, f.body, p.text, at(52.0, 0.0, w / k - 8.0, h / k), name, look::LINE);
            }
        } else if let Some(t) = s.themes.get(i) {
            look::text(d.hDC, f.strong, p.text, at(18.0, 8.0, w / k - 8.0, 30.0), &t.shown, look::LINE);
            look::text(d.hDC, f.small, p.secondary, at(18.0, 29.0, w / k - 8.0, 50.0), &t.about, look::LINE);
        }
    });
}

/// A button through custom draw: a toggle switch, a radio button, a push button, as `look` draws them.
fn custom_draw(cd: &NMCUSTOMDRAW) -> LRESULT {
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as LRESULT;
    }
    with(|s| {
        let Some((_, c)) = s.controls.iter().find(|c| c.1.hwnd == cd.hdr.hwndFrom) else { return CDRF_DODEFAULT as LRESULT };
        let (Some(kind), Some(f)) = (c.kind, s.fonts.as_ref()) else { return CDRF_DODEFAULT as LRESULT };
        let p = s.palette;
        let under = match c.surface {
            Surface::Window => p.window,
            Surface::Card => p.card,
            Surface::Info => p.info,
        };
        // SAFETY: messages to the control being drawn.
        let (on, hide_focus) = unsafe {
            (SendMessageW(c.hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT, SendMessageW(c.hwnd, WM_QUERYUISTATE, 0, 0) as u32 & UISF_HIDEFOCUS != 0)
        };
        let st = look::State {
            on,
            enabled: cd.uItemState & CDIS_DISABLED == 0,
            hot: cd.uItemState & CDIS_HOT != 0,
            pressed: cd.uItemState & CDIS_SELECTED != 0,
            focus: cd.uItemState & CDIS_FOCUS != 0 && !hide_focus,
        };
        let label = text_of(c.hwnd);
        look::button(cd.hdc, cd.rc, kind, &st, &label, under, f.body, s.dpi as f64 / 96.0, &p);
        CDRF_SKIPDEFAULT as LRESULT
    })
    .unwrap_or(CDRF_DODEFAULT as LRESULT)
}

fn text_of(h: HWND) -> String {
    // SAFETY: a buffer of the length the control reports, plus its NUL.
    unsafe {
        let n = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

/// A label's or field's colours: its surface's, and its role's text.
fn control_colours(dc: HDC, control: HWND, field: bool) -> LRESULT {
    with(|s| {
        let p = s.palette;
        let (surface, role) = s.controls.iter().find(|c| c.1.hwnd == control).map_or((Surface::Window, Role::Body), |c| (c.1.surface, c.1.role));
        let (bg, brush) = if field {
            (p.field, s.brushes.get(3))
        } else {
            match surface {
                Surface::Window => (p.window, s.brushes.first()),
                Surface::Card => (p.card, s.brushes.get(1)),
                Surface::Info => (p.info, s.brushes.get(2)),
            }
        };
        let ink: Rgb = match role {
            Role::Secondary => p.secondary,
            _ => p.text,
        };
        // SAFETY: the dc the control paints with.
        unsafe {
            SetTextColor(dc, look::colorref(ink));
            SetBkColor(dc, look::colorref(bg));
        }
        brush.map_or(0, |b| b.0 as HBRUSH as LRESULT)
    })
    .unwrap_or(0)
}

// --- values in and out ---------------------------------------------------------------------------

fn set_text(h: HWND, text: &str) {
    let t = wide(text);
    // SAFETY: a NUL-terminated string to our control.
    unsafe { SetWindowTextW(h, t.as_ptr()) };
}

fn text(id: i32) -> String {
    let h = hwnd_of(id);
    // SAFETY: a buffer of the length the control reports, plus its NUL.
    unsafe {
        let n = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize]).trim().to_string()
    }
}

fn check(id: i32, on: bool) {
    // SAFETY: a button message to our control.
    unsafe { SendMessageW(hwnd_of(id), BM_SETCHECK, if on { BST_CHECKED } else { BST_UNCHECKED } as WPARAM, 0) };
}

fn checked(id: i32) -> bool {
    // SAFETY: as check.
    unsafe { SendMessageW(hwnd_of(id), BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT }
}

fn combo(id: i32, items: &[String], selected: usize) {
    let h = hwnd_of(id);
    // SAFETY: strings that outlive each call, to our combo box.
    unsafe {
        SendMessageW(h, CB_RESETCONTENT, 0, 0);
        for it in items {
            let t = wide(it);
            SendMessageW(h, CB_ADDSTRING, 0, t.as_ptr() as LPARAM);
        }
        SendMessageW(h, CB_SETCURSEL, selected, 0);
    }
}

fn combo_at(id: i32) -> usize {
    // SAFETY: as combo.
    unsafe { SendMessageW(hwnd_of(id), CB_GETCURSEL, 0, 0).max(0) as usize }
}

fn selected_theme_in(s: &State) -> Option<usize> {
    // SAFETY: a list box message to our control.
    let i = unsafe { SendMessageW(hwnd_of_in(s, THEMES), LB_GETCURSEL, 0, 0) };
    (i >= 0 && (i as usize) < s.themes.len()).then_some(i as usize)
}

/// The monitors to choose one of: those here, and the one chosen before if it is not here now.
fn monitor_choices(monitors: &[String], screens: &Screens) -> Vec<String> {
    let mut out: Vec<String> = if monitors.is_empty() { vec!["DISPLAY1".into()] } else { monitors.to_vec() };
    if let Screens::One(m) = screens
        && !out.contains(m)
    {
        out.push(m.clone());
    }
    out
}

const PLACES: [&str; 6] = ["As the theme has it", "The rightmost screen", "The leftmost screen", "The main screen", "Every screen", "Nowhere"];

fn place_items(monitors: &[String]) -> Vec<String> {
    PLACES.iter().map(|s| s.to_string()).chain(monitors.iter().map(|m| format!("Only {m}"))).collect()
}

/// A place the list cannot show (several monitors, or one not here now), kept as a last item.
fn place_extra(p: &Place, monitors: &[String]) -> Option<String> {
    match p {
        Place::Named(n) if n.len() != 1 || !monitors.contains(&n[0]) => Some(format!("As set: {}", n.join(", "))),
        _ => None,
    }
}

fn place_index(p: &Place, monitors: &[String]) -> usize {
    match p {
        Place::Theme => 0,
        Place::Rightmost => 1,
        Place::Leftmost => 2,
        Place::Primary => 3,
        Place::All => 4,
        Place::None => 5,
        Place::Named(n) => n.first().and_then(|m| monitors.iter().position(|x| x == m)).map_or(0, |i| 6 + i),
    }
}

fn place_of(i: usize, monitors: &[String], was: &Place) -> Place {
    if i == 6 + monitors.len() && place_extra(was, monitors).is_some() {
        return was.clone();
    }
    match i {
        1 => Place::Rightmost,
        2 => Place::Leftmost,
        3 => Place::Primary,
        4 => Place::All,
        5 => Place::None,
        i if i >= 6 && i - 6 < monitors.len() => Place::Named(vec![monitors[i - 6].clone()]),
        _ => Place::Theme,
    }
}

/// The settings into the controls.
/// The themes into their list, the settings' one selected.
fn fill_themes() {
    let Some((current, themes)) = with(|st| (st.settings.theme.clone().unwrap_or_else(|| "phosphor".into()), st.themes.clone())) else { return };
    let list = hwnd_of(THEMES);
    // SAFETY: strings that outlive each call, to our list box.
    unsafe {
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        for t in &themes {
            let t = wide(&t.shown);
            SendMessageW(list, LB_ADDSTRING, 0, t.as_ptr() as LPARAM);
        }
        SendMessageW(list, LB_SETCURSEL, themes.iter().position(|t| t.name == current).unwrap_or(0), 0);
    }
    extras_shown();
}

/// The settings into the controls.
fn fill() {
    let Some((s, monitors)) = with(|st| (st.settings.clone(), st.monitors.clone())) else { return };
    fill_themes();
    check(UI, s.ui.unwrap_or(true));
    check(HAND, s.hand.unwrap_or(false));
    let one = match &s.screens {
        Screens::Main => {
            check(SCREENS_MAIN, true);
            None
        }
        Screens::All => {
            check(SCREENS_ALL, true);
            None
        }
        Screens::One(m) => {
            check(SCREENS_ONE, true);
            Some(m.clone())
        }
    };
    let mons = monitor_choices(&monitors, &s.screens);
    combo(MONITOR, &mons, one.and_then(|m| mons.iter().position(|x| *x == m)).unwrap_or(0));
    for (id, p) in [(PLACE_TITLE, &s.title), (PLACE_WIDGETS, &s.widgets), (PLACE_GRAPH, &s.graph), (PLACE_STATUS, &s.status)] {
        let mut items = place_items(&monitors);
        let at = match place_extra(p, &monitors) {
            Some(extra) => {
                items.push(extra);
                items.len() - 1
            }
            None => place_index(p, &monitors),
        };
        combo(id, &items, at);
    }
    for (id, name) in [(SHOW_CPU, "cpu"), (SHOW_RAM, "ram"), (SHOW_SWAP, "swap"), (SHOW_DECK, "deck"), (SHOW_STATUS, "status")] {
        check(id, !s.hidden.iter().any(|h| h == name));
        // SAFETY: our child window.
        unsafe { EnableWindow(hwnd_of(id), (!s.custom_layout) as i32) };
    }
    if s.custom_layout {
        set_text(hwnd_of(OWN_LAYOUT), "Your retro.toml lays out the instruments itself, with widgets of your own: change them there.");
    }
    check(BATTERY, s.battery);
    check(HOST_MACHINE, s.host.is_none());
    check(HOST_OWN, s.host.is_some());
    set_text(hwnd_of(HOST_NAME), s.host.as_deref().unwrap_or(""));
    let (i, below) = match s.on_battery {
        OnBattery::Pause => (0, 25),
        OnBattery::Below(n) => (1, n),
        OnBattery::Keep => (2, 25),
    };
    combo(ON_BATTERY, &["Pause the animation".into(), "Pause below a charge of".into(), "Keep animating".into()], i);
    set_text(hwnd_of(BELOW), &below.to_string());
    check(SCREEN_OFF, s.pause_screen_off);
    check(RDP, s.pause_rdp.is_some());
    set_text(hwnd_of(RDP_PORT), &s.pause_rdp.unwrap_or(3389).to_string());
    check(VNC, s.pause_vnc.is_some());
    set_text(hwnd_of(VNC_PORT), &s.pause_vnc.unwrap_or(5900).to_string());
    check(SOFTWARE, s.software);
    set_text(hwnd_of(INTERFACES), &s.interfaces.join(", "));
    set_text(hwnd_of(IP_URL), s.ip_url.as_deref().unwrap_or(""));
    enable_dependents();
}

/// The window's own drawing (cards, fields) again.
fn redraw() {
    if let Some(h) = with(|s| s.hwnd) {
        // SAFETY: our window.
        unsafe { InvalidateRect(h, std::ptr::null(), 0) };
    }
}

/// What only means something when another is on: greyed out otherwise.
fn enable_dependents() {
    let pairs = [
        (MONITOR, checked(SCREENS_ONE)),
        (HOST_NAME, checked(HOST_OWN)),
        (BELOW, combo_at(ON_BATTERY) == 1),
        (RDP_PORT, checked(RDP)),
        (VNC_PORT, checked(VNC)),
    ];
    for (id, on) in pairs {
        // SAFETY: our child window.
        unsafe { EnableWindow(hwnd_of(id), on as i32) };
    }
    redraw();
}

/// The controls into settings; an error names what is wrong, and the page it is on.
fn read_back() -> Result<Settings, String> {
    let Some((mut s, themes, monitors)) = with(|st| (st.settings.clone(), st.themes.clone(), st.monitors.clone())) else { return Err("no window".into()) };
    let ti = with(|st| selected_theme_in(st)).flatten().unwrap_or(0);
    let (name, extras) = themes.get(ti).map_or(("phosphor".to_string(), false), |t| (t.name.clone(), t.extras));
    s.theme = (name != "phosphor").then_some(name);
    (s.ui, s.hand) = if extras { ((!checked(UI)).then_some(false), checked(HAND).then_some(true)) } else { (None, None) };
    s.screens = if checked(SCREENS_ALL) {
        Screens::All
    } else if checked(SCREENS_ONE) {
        let mons = monitor_choices(&monitors, &s.screens);
        Screens::One(mons.get(combo_at(MONITOR)).cloned().unwrap_or_default())
    } else {
        Screens::Main
    };
    s.title = place_of(combo_at(PLACE_TITLE), &monitors, &s.title);
    s.widgets = place_of(combo_at(PLACE_WIDGETS), &monitors, &s.widgets);
    s.graph = place_of(combo_at(PLACE_GRAPH), &monitors, &s.graph);
    s.status = place_of(combo_at(PLACE_STATUS), &monitors, &s.status);
    s.hidden = [(SHOW_CPU, "cpu"), (SHOW_RAM, "ram"), (SHOW_SWAP, "swap"), (SHOW_DECK, "deck"), (SHOW_STATUS, "status")]
        .into_iter()
        .filter(|(id, _)| !checked(*id))
        .map(|(_, n)| n.to_string())
        .collect();
    s.battery = checked(BATTERY);
    s.host = if checked(HOST_OWN) {
        let h = text(HOST_NAME);
        if h.is_empty() {
            return Err("Instruments: type the name to show, or choose this computer's".into());
        }
        if h.contains('"') {
            return Err("Instruments: the name shown cannot have a double quote in it".into());
        }
        Some(h)
    } else {
        None
    };
    s.on_battery = match combo_at(ON_BATTERY) {
        1 => OnBattery::Below(text(BELOW).parse::<u8>().ok().filter(|n| (1..=100).contains(n)).ok_or("Pausing: the charge is a number from 1 to 100")?),
        2 => OnBattery::Keep,
        _ => OnBattery::Pause,
    };
    s.pause_screen_off = checked(SCREEN_OFF);
    let port = |on: i32, id: i32, what: &str| -> Result<Option<u16>, String> {
        if !checked(on) {
            return Ok(None);
        }
        text(id).parse::<u16>().ok().filter(|p| *p > 0).map(Some).ok_or(format!("Pausing: the {what} port is a number from 1 to 65535"))
    };
    s.pause_rdp = port(RDP, RDP_PORT, "Remote Desktop")?;
    s.pause_vnc = port(VNC, VNC_PORT, "VNC")?;
    s.software = checked(SOFTWARE);
    s.interfaces = text(INTERFACES).split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect();
    s.ip_url = Some(text(IP_URL)).filter(|u| !u.is_empty());
    if let Some(u) = &s.ip_url
        && !(u.starts_with("http://") || u.starts_with("https://"))
    {
        return Err("Advanced: the address to ask begins with http:// or https://".into());
    }
    Ok(s)
}

/// The info bar's text; the bar is shown only while there is one.
fn say(text: &str) {
    let Some(hwnd) = with(|s| {
        s.status = text.to_string();
        s.hwnd
    }) else {
        return;
    };
    let h = hwnd_of(STATUS);
    set_text(h, text);
    // SAFETY: our child window and our window.
    unsafe {
        ShowWindow(h, if text.is_empty() { SW_HIDE } else { SW_SHOW });
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

// --- previews ------------------------------------------------------------------------------------

/// Draw the selected theme's preview on a thread: the wall's still picture at the preview's size.
fn ask_preview() {
    let Some((wall, themes_dir, name, n, hwnd, (w, h))) = with(|s| {
        s.preview_asked += 1;
        let name = selected_theme_in(s).map(|i| s.themes[i].name.clone()).unwrap_or_else(|| "phosphor".into());
        (s.wall.clone(), s.themes_dir.clone(), name, s.preview_asked, s.hwnd as usize, (PREVIEW.2 * s.dpi / 96, PREVIEW.3 * s.dpi / 96))
    }) else {
        return;
    };
    std::thread::spawn(move || {
        let out = std::env::temp_dir().join(format!("retro-wall-preview-{}-{n}.png", std::process::id()));
        // the theme where it is: the install's, or the user's own
        let theme = [themes_dir.join(&name), ops::settings_dir().join("themes").join(&name)]
            .into_iter()
            .find(|d| d.join("theme.toml").is_file())
            .unwrap_or_else(|| themes_dir.join(&name));
        let ok = std::process::Command::new(&wall)
            .args(["--preview", &out.to_string_lossy(), &format!("{w}x{h}"), "--theme", &theme.to_string_lossy(), "--demo", "13:37"])
            .creation_flags_no_window()
            .status()
            .is_ok_and(|s| s.success());
        let path = ok.then(|| out.to_string_lossy().into_owned());
        let boxed = Box::into_raw(Box::new(path));
        // SAFETY: the window owns the box once posted; it frees it on WM_PREVIEW.
        unsafe { PostMessageW(hwnd as HWND, WM_PREVIEW, n as WPARAM, boxed as LPARAM) };
    });
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
    }
}

// --- doing ---------------------------------------------------------------------------------------

/// Work on a thread, its reports in the status line, then WM_DONE with its outcome.
fn work(f: impl FnOnce(ops::Say) -> Result<String, String> + Send + 'static) {
    let Some(hwnd) = with(|s| {
        s.busy = true;
        s.hwnd as usize
    }) else {
        return;
    };
    busy(true);
    std::thread::spawn(move || {
        let post = |m: u32, text: String| {
            let b = Box::into_raw(Box::new(text));
            // SAFETY: the window frees the box when it takes the message.
            unsafe { PostMessageW(hwnd as HWND, m, 0, b as LPARAM) };
        };
        let mut say = |t: &str| post(WM_SAY, t.to_string());
        let r = f(&mut say);
        post(
            WM_DONE,
            match r {
                Ok(t) => format!("ok:{t}"),
                Err(e) => format!("error:{e}"),
            },
        );
    });
}

fn busy(on: bool) {
    for id in [OK, APPLY, CLOSE, UNINSTALL] {
        // SAFETY: our child windows.
        unsafe { EnableWindow(hwnd_of(id), (!on) as i32) };
    }
}

fn message(title: &str, text: &str, flags: MESSAGEBOX_STYLE) -> i32 {
    let hwnd = with(|s| s.hwnd).unwrap_or(std::ptr::null_mut());
    let (t, m) = (wide(title), wide(text));
    // SAFETY: NUL-terminated strings, our window as the owner.
    unsafe { MessageBoxW(hwnd, m.as_ptr(), t.as_ptr(), flags) }
}

/// OK, Install or Apply: keep the settings (install first on a first run); OK closes after.
fn commit(close_after: bool) {
    // Enter reaches here even while OK is greyed: one piece of work at a time
    if with(|s| s.busy).unwrap_or(false) {
        return;
    }
    let s = match read_back() {
        Ok(s) => s,
        Err(e) => {
            message("retro-wall", &e, MB_ICONWARNING | MB_OK);
            return;
        }
    };
    let installing = with(|st| st.installing).unwrap_or(false);
    with(|st| st.settings = s.clone());
    work(move |say| {
        if installing {
            ops::install(Some(&s), say)?;
            Ok(if close_after { "installed:close".into() } else { "installed".into() })
        } else {
            ops::apply(&s, say)?;
            Ok(if close_after { "applied:close".into() } else { "applied".into() })
        }
    });
}

fn done(outcome: &str) {
    with(|s| s.busy = false);
    busy(false);
    if let Some(e) = outcome.strip_prefix("error:") {
        say("Not done.");
        message("retro-wall", e, MB_ICONERROR | MB_OK);
        return;
    }
    let what = outcome.trim_start_matches("ok:");
    if what.starts_with("installed") {
        if let Some(h) = with(|s| {
            s.installing = false;
            s.wall = ops::wall_exe();
            s.themes_dir = ops::install_dir().join("themes");
            s.hwnd
        }) {
            set_text(h, "retro-wall settings");
        }
        let page = with(|s| s.page).unwrap_or(0);
        show_page(page);
        say("Installed: retro-wall is your wallpaper now. Find these settings in the Start menu: retro-wall settings.");
    } else if what == "uninstalled" {
        message("retro-wall", "retro-wall is uninstalled, and your own wallpaper is back.", MB_ICONINFORMATION | MB_OK);
        close();
        return;
    } else {
        say("Saved: retro-wall restarted with these settings.");
    }
    with(|s| s.shown = s.settings.clone());
    if what.ends_with(":close") {
        close();
    }
}

fn close() {
    // destroyed outside the state's borrow: destroying sends messages back to the window
    if let Some(h) = with(|s| s.hwnd) {
        // SAFETY: our window.
        unsafe { DestroyWindow(h) };
    }
}

/// Close, asking first if there are changes not kept.
fn close_asking() {
    // a first run still making the themes ready: nothing done yet, so nothing to ask
    if with(|s| s.unpacking).unwrap_or(false) {
        close();
        return;
    }
    let changed = with(|s| (s.busy, s.installing))
        .is_some_and(|(busy, installing)| !busy && !installing && read_back().is_ok_and(|now| with(|s| now != s.shown).unwrap_or(false)));
    if changed {
        match message("retro-wall", "Keep your changes?", MB_ICONQUESTION | MB_YESNOCANCEL) {
            IDYES => commit(true),
            IDNO => close(),
            _ => {}
        }
        return;
    }
    if with(|s| s.busy).unwrap_or(false) {
        return;
    }
    close();
}

/// Ask, then uninstall: keep the settings or not.
pub fn confirm_uninstall(owner: HWND) -> Option<bool> {
    let ask = |text: &str, flags| {
        let (t, m) = (wide("Uninstall retro-wall"), wide(text));
        // SAFETY: NUL-terminated strings.
        unsafe { MessageBoxW(owner, m.as_ptr(), t.as_ptr(), flags) }
    };
    if ask("Remove retro-wall? Your own wallpaper comes back.", MB_ICONQUESTION | MB_OKCANCEL) != IDOK {
        return None;
    }
    Some(ask("Keep your retro-wall settings, for if you install it again?", MB_ICONQUESTION | MB_YESNO) == IDYES)
}

fn open(path: &Path) {
    let (verb, p) = (wide("open"), wide(path));
    // SAFETY: NUL-terminated strings; the shell opens the folder or file.
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), p.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

fn command(id: i32, code: u32) {
    match (id, code) {
        // an edit box's field shows where the typing goes
        (_, c) if c == EN_SETFOCUS || c == EN_KILLFOCUS => redraw(),
        // Enter and Esc, as IsDialogMessage sends them
        (1, _) | (OK, _) => commit(!with(|s| s.installing).unwrap_or(false)),
        (2, _) => close_asking(),
        (APPLY, _) => commit(false),
        (CLOSE, _) => close_asking(),
        (NAV, c) if c == LBN_SELCHANGE => {
            // SAFETY: our list.
            let page = unsafe { SendMessageW(hwnd_of(NAV), LB_GETCURSEL, 0, 0) };
            if (0..PAGES.len() as isize).contains(&page) {
                show_page(page as usize);
            }
        }
        (THEMES, c) if c == LBN_SELCHANGE => {
            extras_shown();
            ask_preview();
        }
        (SCREENS_MAIN | SCREENS_ALL | SCREENS_ONE | HOST_MACHINE | HOST_OWN | RDP | VNC, _) => enable_dependents(),
        (ON_BATTERY, c) if c == CBN_SELCHANGE => enable_dependents(),
        (OPEN_FOLDER, _) => {
            let d = ops::settings_dir();
            let _ = std::fs::create_dir_all(&d);
            open(&d);
        }
        (OPEN_LOG, _) => open(&ops::install_dir().join("retro-wall.log")),
        (UNINSTALL, _) if !with(|s| s.busy).unwrap_or(false) => {
            let owner = with(|s| s.hwnd).unwrap_or(std::ptr::null_mut());
            if let Some(keep) = confirm_uninstall(owner) {
                work(move |say| ops::uninstall(keep, say).map(|_| "uninstalled".to_string()));
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            command((wp & 0xffff) as i32, ((wp >> 16) & 0xffff) as u32);
            0
        }
        WM_NOTIFY => {
            // SAFETY: WM_NOTIFY's lparam is an NMHDR; NM_CUSTOMDRAW's an NMCUSTOMDRAW.
            let n = unsafe { &*(lp as *const NMHDR) };
            if n.code == NM_CUSTOMDRAW {
                // SAFETY: as above.
                return custom_draw(unsafe { &*(lp as *const NMCUSTOMDRAW) });
            }
            0
        }
        WM_MEASUREITEM => {
            // SAFETY: WM_MEASUREITEM's lparam.
            let m = unsafe { &mut *(lp as *mut MEASUREITEMSTRUCT) };
            let dpi = with(|s| s.dpi).unwrap_or(96);
            m.itemHeight = (if m.CtlID == NAV as u32 { 40 } else { 56 } * dpi / 96) as u32;
            1
        }
        WM_DRAWITEM => {
            // SAFETY: WM_DRAWITEM's lparam.
            draw_item(unsafe { &*(lp as *const DRAWITEMSTRUCT) });
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => control_colours(wp as HDC, lp as HWND, false),
        WM_CTLCOLOREDIT => control_colours(wp as HDC, lp as HWND, true),
        // a list on what it stands on: the window, a card
        WM_CTLCOLORLISTBOX => control_colours(wp as HDC, lp as HWND, false),
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            if with(|_| ()).is_none() {
                // SAFETY: as Windows does it, while the state is in use
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            }
            paint(hwnd);
            0
        }
        WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED => {
            // light or dark, or the accent colour, may have changed
            colours();
            0
        }
        WM_PREVIEW => {
            // SAFETY: the box posted by ask_preview's thread, ours to free now.
            let path = unsafe { Box::from_raw(lp as *mut Option<String>) };
            let current = with(|s| s.preview_asked).unwrap_or(0) == wp as u32;
            if let Some(p) = path.as_deref() {
                if current && let Some(img) = std::fs::File::open(p).ok().and_then(|mut f| cairo::ImageSurface::create_from_png(&mut f).ok()) {
                    with(|s| {
                        s.preview = Some(img);
                        // SAFETY: our window.
                        unsafe { InvalidateRect(s.hwnd, std::ptr::null(), 0) };
                    });
                }
                let _ = std::fs::remove_file(p);
            } else if current {
                with(|s| s.preview = None);
                say("The preview could not be drawn.");
            }
            0
        }
        WM_UNPACKED => {
            // SAFETY: the box posted by run's unpacking thread.
            let r = unsafe { Box::from_raw(lp as *mut Result<(), String>) };
            with(|s| {
                s.busy = false;
                s.unpacking = false;
            });
            busy(false);
            match *r {
                Ok(()) => {
                    say("");
                    with(|s| {
                        let mut list = themes(&[s.themes_dir.clone(), ops::settings_dir().join("themes")]);
                        if let Some(t) = s.settings.theme.as_ref().filter(|t| !list.iter().any(|x| &x.name == *t)) {
                            list.push(ThemeItem { name: t.clone(), shown: format!("{t} (not found)"), about: String::new(), extras: false });
                        }
                        s.themes = list;
                    });
                    fill_themes();
                    if let Ok(shown) = read_back() {
                        with(|s| s.shown = shown);
                    }
                    ask_preview();
                }
                Err(e) => {
                    say("The themes could not be read.");
                    message("retro-wall", &e, MB_ICONERROR | MB_OK);
                }
            }
            0
        }
        WM_SAY => {
            // SAFETY: the box posted by work's thread.
            let t = unsafe { Box::from_raw(lp as *mut String) };
            say(&t);
            0
        }
        WM_DONE => {
            // SAFETY: as WM_SAY.
            let t = unsafe { Box::from_raw(lp as *mut String) };
            done(&t);
            0
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED's lparam is the suggested rectangle.
            let r = unsafe { &*(lp as *const RECT) };
            // SAFETY: our window.
            unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE) };
            relayout();
            ask_preview();
            0
        }
        WM_CLOSE => {
            close_asking();
            0
        }
        WM_DESTROY => {
            // SAFETY: the window's own message loop ends.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: everything else as Windows does it.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
