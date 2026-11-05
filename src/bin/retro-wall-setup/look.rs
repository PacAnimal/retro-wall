//! The setup program's look, after Windows 11's own apps: its window and card colours, light or dark
//! as Windows is set, the user's accent colour, Segoe UI Variable, rounded cards, toggle switches and an
//! accent button. Shapes are drawn with cairo (smooth corners) into a bitmap and put on the screen;
//! text is drawn by Windows (ClearType). The controls stay Windows' own (check boxes, radio buttons,
//! buttons, lists), drawn here through custom draw, so screen readers and keyboards work as anywhere.

use retro_wall::platform::windows::wide;
use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
    DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, GetTextFaceW, HBRUSH, HDC,
    HFONT, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetDIBitsToDevice, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

/// An RGB colour, 0 to 1 each.
pub type Rgb = (f64, f64, f64);

pub fn hex(c: u32) -> Rgb {
    (((c >> 16) & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, (c & 255) as f64 / 255.0)
}

/// A colour as GDI takes it.
pub fn colorref(c: Rgb) -> COLORREF {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c.0) | b(c.1) << 8 | b(c.2) << 16
}

/// The colours of a light or dark window, Windows 11's.
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// The window behind the cards.
    pub window: Rgb,
    pub card: Rgb,
    pub card_line: Rgb,
    pub text: Rgb,
    pub secondary: Rgb,
    pub disabled: Rgb,
    pub accent: Rgb,
    /// Text on the accent colour.
    pub on_accent: Rgb,
    /// An edit box's, a list's inside.
    pub field: Rgb,
    pub field_line: Rgb,
    /// A neutral button's face.
    pub button: Rgb,
    /// The navigation's selected item, hovered things.
    pub subtle: Rgb,
    pub info: Rgb,
    pub danger: Rgb,
}

impl Palette {
    pub fn new(dark: bool, accent: Rgb) -> Palette {
        if dark {
            Palette {
                dark,
                window: hex(0x202020),
                card: hex(0x2b2b2b),
                card_line: hex(0x1d1d1d),
                text: hex(0xffffff),
                secondary: hex(0xc5c5c5),
                disabled: hex(0x787878),
                accent: lighten(accent, 0.35),
                on_accent: hex(0x000000),
                field: hex(0x2d2d2d),
                field_line: hex(0x3a3a3a),
                button: hex(0x373737),
                subtle: hex(0x353535),
                info: hex(0x2c3a46),
                danger: hex(0xff99a4),
            }
        } else {
            Palette {
                dark,
                window: hex(0xf3f3f3),
                card: hex(0xfbfbfb),
                card_line: hex(0xe5e5e5),
                text: hex(0x1a1a1a),
                secondary: hex(0x5d5d5d),
                disabled: hex(0xa0a0a0),
                accent,
                // white on the accent, or black on a light one (as Windows does for a yellow accent)
                on_accent: if 0.2126 * accent.0 + 0.7152 * accent.1 + 0.0722 * accent.2 > 0.6 { hex(0x000000) } else { hex(0xffffff) },
                field: hex(0xffffff),
                field_line: hex(0xd1d1d1),
                button: hex(0xfdfdfd),
                subtle: hex(0xeaeaea),
                info: hex(0xe8f1fb),
                danger: hex(0xc42b1c),
            }
        }
    }
}

fn lighten(c: Rgb, k: f64) -> Rgb {
    (c.0 + (1.0 - c.0) * k, c.1 + (1.0 - c.1) * k, c.2 + (1.0 - c.2) * k)
}

/// A colour a little lighter (dark) or darker (light): a hovered face.
pub fn hover(c: Rgb, dark: bool) -> Rgb {
    if dark { lighten(c, 0.08) } else { (c.0 * 0.96, c.1 * 0.96, c.2 * 0.96) }
}

fn dword(key: &str, value: &str) -> Option<u32> {
    let (k, v) = (wide(key), wide(value));
    let mut out = 0u32;
    let mut len = 4u32;
    // SAFETY: NUL-terminated names; a DWORD we own.
    let rc =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut out as *mut u32).cast(), &mut len) };
    (rc == 0).then_some(out)
}

/// Whether Windows is set to dark for apps.
pub fn system_dark() -> bool {
    dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") == Some(0)
}

/// The user's accent colour (Windows keeps it as 0xAABBGGRR); Windows 11's blue if none is set.
pub fn accent() -> Rgb {
    dword(r"Software\Microsoft\Windows\DWM", "AccentColor")
        .map_or(hex(0x0067c0), |c| ((c & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, ((c >> 16) & 255) as f64 / 255.0))
}

/// The fonts, at a dpi: Segoe UI Variable where Windows has it (11), else Segoe UI (10); the icons'
/// Segoe Fluent Icons, else Segoe MDL2 Assets.
pub struct Fonts {
    pub body: HFONT,
    pub strong: HFONT,
    pub small: HFONT,
    pub subtitle: HFONT,
    pub title: HFONT,
    pub icon: HFONT,
}

fn font(px: i32, weight: i32, faces: &[&str]) -> HFONT {
    for (i, face) in faces.iter().enumerate() {
        let f = wide(face);
        // SAFETY: a font made from a NUL-terminated face name; checked by the face the system gives.
        unsafe {
            let h = CreateFontW(
                -px,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                f.as_ptr(),
            );
            if i + 1 == faces.len() || has_face(h, face) {
                return h;
            }
            DeleteObject(h);
        }
    }
    std::ptr::null_mut()
}

/// Whether a font is the face asked for, not a stand-in.
fn has_face(h: HFONT, face: &str) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC};
    // SAFETY: a memory dc of our own, the font selected out again before it goes.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let old = SelectObject(dc, h);
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(dc, buf.len() as i32, buf.as_mut_ptr());
        SelectObject(dc, old);
        DeleteDC(dc);
        String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]).eq_ignore_ascii_case(face)
    }
}

impl Fonts {
    pub fn new(dpi: i32) -> Fonts {
        let px = |v: f64| (v * dpi as f64 / 96.0).round() as i32;
        let text = ["Segoe UI Variable Text", "Segoe UI"];
        let display = ["Segoe UI Variable Display", "Segoe UI"];
        Fonts {
            body: font(px(14.0), 400, &text),
            strong: font(px(14.0), 600, &text),
            small: font(px(12.0), 400, &text),
            subtitle: font(px(20.0), 600, &display),
            title: font(px(28.0), 600, &display),
            icon: font(px(16.0), 400, &["Segoe Fluent Icons", "Segoe MDL2 Assets"]),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in [self.body, self.strong, self.small, self.subtitle, self.title, self.icon] {
            // SAFETY: our own fonts, no longer selected anywhere once replaced.
            unsafe { DeleteObject(f) };
        }
    }
}

/// A solid brush of a colour, deleted with it.
pub struct Brush(pub HBRUSH);

impl Brush {
    pub fn new(c: Rgb) -> Brush {
        // SAFETY: a brush of our own.
        Brush(unsafe { CreateSolidBrush(colorref(c)) })
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        // SAFETY: our own brush.
        unsafe { DeleteObject(self.0) };
    }
}

/// Draw shapes with cairo onto `rect` of a dc: `f` draws in the rectangle's own coordinates, on what
/// `under` fills it with first.
pub fn shapes(hdc: HDC, rect: RECT, under: Rgb, f: impl FnOnce(&cairo::Context)) {
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let Ok(mut surf) = cairo::ImageSurface::create(cairo::Format::Rgb24, w, h) else { return };
    if let Ok(cr) = cairo::Context::new(&surf) {
        cr.set_source_rgb(under.0, under.1, under.2);
        let _ = cr.paint();
        f(&cr);
    }
    surf.flush();
    let stride = surf.stride();
    let Ok(data) = surf.data() else { return };
    // SAFETY: a top-down 32-bit DIB of the surface's own rows (cairo's RGB24 is BGRX in memory, as GDI's).
    unsafe {
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: stride / 4,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        SetDIBitsToDevice(hdc, rect.left, rect.top, w as u32, h as u32, 0, 0, 0, h as u32, data.as_ptr().cast(), &bi, DIB_RGB_COLORS);
    }
}

/// A rounded rectangle's path.
pub fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

pub fn fill(cr: &cairo::Context, c: Rgb) {
    cr.set_source_rgb(c.0, c.1, c.2);
    let _ = cr.fill();
}

/// A card: a rounded face with a hairline edge.
pub fn card(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, k: f64, p: &Palette) {
    rounded(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 6.0 * k);
    cr.set_source_rgb(p.card.0, p.card.1, p.card.2);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(p.card_line.0, p.card_line.1, p.card_line.2);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

/// Text in a rectangle, in a font and colour.
pub fn text(hdc: HDC, f: HFONT, c: Rgb, mut r: RECT, s: &str, flags: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: the text and rectangle are ours for the call; the font is selected back out after.
    unsafe {
        let old = SelectObject(hdc, f);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, colorref(c));
        DrawTextW(hdc, w.as_mut_ptr(), w.len() as i32, &mut r, flags | DT_NOPREFIX);
        SelectObject(hdc, old);
    }
}

pub const LINE: DRAW_TEXT_FORMAT = DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS;

/// What a custom-drawn button is drawn as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A check box drawn as a toggle switch, "On" or "Off" beside it.
    Toggle,
    Radio,
    /// A button in the window's colours.
    Push,
    /// The button that does the page's work, in the accent colour.
    Primary,
    /// A button that takes something away: its text in red.
    Danger,
}

/// A button's states, from custom draw.
pub struct State {
    pub on: bool,
    pub enabled: bool,
    pub hot: bool,
    pub pressed: bool,
    pub focus: bool,
}

/// Draw a button as its kind, on a surface of colour `under`, its text in `font`.
pub fn button(hdc: HDC, rect: RECT, kind: Kind, s: &State, label: &str, under: Rgb, font: HFONT, k: f64, p: &Palette) {
    let (w, h) = ((rect.right - rect.left) as f64, (rect.bottom - rect.top) as f64);
    let ink = if s.enabled { p.text } else { p.disabled };
    match kind {
        Kind::Toggle => {
            // Windows 11's: a 40 x 20 pill, the knob right and the pill filled when on
            let (sw, sh) = (40.0 * k, 20.0 * k);
            let (sx, sy) = (w - sw - 1.0, (h - sh) / 2.0);
            shapes(hdc, rect, under, |cr| {
                rounded(cr, sx + 0.5, sy + 0.5, sw - 1.0, sh - 1.0, sh / 2.0);
                if s.on {
                    let c = if s.enabled { if s.hot { hover(p.accent, !p.dark) } else { p.accent } } else { p.disabled };
                    fill(cr, c);
                } else {
                    cr.set_source_rgb(
                        if s.hot { p.subtle.0 } else { under.0 },
                        if s.hot { p.subtle.1 } else { under.1 },
                        if s.hot { p.subtle.2 } else { under.2 },
                    );
                    let _ = cr.fill_preserve();
                    cr.set_source_rgb(
                        if s.enabled { p.secondary.0 } else { p.disabled.0 },
                        if s.enabled { p.secondary.1 } else { p.disabled.1 },
                        if s.enabled { p.secondary.2 } else { p.disabled.2 },
                    );
                    cr.set_line_width(1.0 * k.max(1.0));
                    let _ = cr.stroke();
                }
                let r = if s.pressed {
                    7.0
                } else if s.hot {
                    6.5
                } else {
                    6.0
                } * k;
                let cx = if s.on { sx + sw - sh / 2.0 } else { sx + sh / 2.0 };
                cr.arc(cx, sy + sh / 2.0, r, 0.0, std::f64::consts::TAU);
                fill(
                    cr,
                    if s.on {
                        p.on_accent
                    } else if s.enabled {
                        p.secondary
                    } else {
                        p.disabled
                    },
                );
                if s.focus {
                    rounded(cr, sx - 3.0 * k, sy - 3.0 * k, sw + 6.0 * k, sh + 6.0 * k, (sh + 6.0 * k) / 2.0);
                    cr.set_source_rgb(p.text.0, p.text.1, p.text.2);
                    cr.set_line_width(2.0 * k.max(1.0) / 1.5);
                    let _ = cr.stroke();
                }
            });
            let mut r = rect;
            r.right = rect.left + (sx - 12.0 * k) as i32;
            text(hdc, font, ink, r, if s.on { "On" } else { "Off" }, LINE | windows_sys::Win32::Graphics::Gdi::DT_RIGHT);
        }
        Kind::Radio => {
            let d = 20.0 * k;
            let (cx, cy) = (d / 2.0 + 1.0, h / 2.0);
            shapes(hdc, rect, under, |cr| {
                cr.arc(cx, cy, d / 2.0 - 0.5, 0.0, std::f64::consts::TAU);
                if s.on {
                    fill(cr, if s.enabled { p.accent } else { p.disabled });
                    cr.arc(cx, cy, if s.hot { 5.0 } else { 4.0 } * k, 0.0, std::f64::consts::TAU);
                    fill(cr, p.on_accent);
                } else {
                    cr.set_source_rgb(under.0, under.1, under.2);
                    let _ = cr.fill_preserve();
                    let c = if s.enabled { p.secondary } else { p.disabled };
                    cr.set_source_rgb(c.0, c.1, c.2);
                    cr.set_line_width(1.0 * k.max(1.0));
                    let _ = cr.stroke();
                }
                if s.focus {
                    rounded(cr, 1.0, 1.0, w - 2.0, h - 2.0, 4.0 * k);
                    cr.set_source_rgb(p.text.0, p.text.1, p.text.2);
                    cr.set_line_width(1.5);
                    let _ = cr.stroke();
                }
            });
            let mut r = rect;
            r.left += (d + 10.0 * k) as i32;
            text(hdc, font, ink, r, label, LINE);
        }
        Kind::Push | Kind::Primary | Kind::Danger => {
            let primary = kind == Kind::Primary && s.enabled;
            let face = if primary {
                if s.pressed {
                    hover(hover(p.accent, !p.dark), !p.dark)
                } else if s.hot {
                    hover(p.accent, !p.dark)
                } else {
                    p.accent
                }
            } else if s.pressed {
                p.subtle
            } else if s.hot {
                hover(p.button, p.dark)
            } else {
                p.button
            };
            shapes(hdc, rect, under, |cr| {
                rounded(cr, 0.5, 0.5, w - 1.0, h - 1.0, 4.0 * k);
                cr.set_source_rgb(face.0, face.1, face.2);
                let _ = cr.fill_preserve();
                if !primary {
                    cr.set_source_rgb(p.field_line.0, p.field_line.1, p.field_line.2);
                    cr.set_line_width(1.0);
                    let _ = cr.stroke();
                } else {
                    cr.new_path();
                }
                if s.focus {
                    rounded(cr, 1.5, 1.5, w - 3.0, h - 3.0, 4.0 * k);
                    cr.set_source_rgb(p.text.0, p.text.1, p.text.2);
                    cr.set_line_width(2.0);
                    let _ = cr.stroke();
                }
            });
            let ink = if primary {
                p.on_accent
            } else if !s.enabled {
                p.disabled
            } else if kind == Kind::Danger {
                p.danger
            } else {
                p.text
            };
            text(hdc, font, ink, rect, label, LINE | windows_sys::Win32::Graphics::Gdi::DT_CENTER);
        }
    }
}
