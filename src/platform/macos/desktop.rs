//! The desktop picture, per screen, through NSWorkspace: shown, saved and restored with its options
//! (scaling, clipping, fill colour). A managed Mac (a profile forces the picture) takes the setting and
//! shows no change, so there retro-wall shows the still picture itself.

use crate::platform::{Desktop, Screen};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSColor, NSColorSpace, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageFillColorKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_core_foundation::{CFPreferencesAppValueIsForced, CFString};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// NSImageScaleProportionallyUpOrDown.
const FIT: isize = 3;

/// Whether a configuration profile sets the desktop picture.
pub fn managed() -> bool {
    CFPreferencesAppValueIsForced(&CFString::from_static_str("override-picture-path"), &CFString::from_static_str("com.apple.desktop"))
}

pub struct Workspace;

fn screens() -> Vec<(String, Retained<NSScreen>)> {
    let Some(mtm) = MainThreadMarker::new() else { return vec![] };
    NSScreen::screens(mtm).iter().map(|s| (s.localizedName().to_string(), s)).collect()
}

/// (red, green, blue), 0 to 1, of #rrggbb.
fn rgb(colour: &str) -> (f64, f64, f64) {
    let c = |i: usize| colour.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(0) as f64 / 255.0;
    (c(1), c(3), c(5))
}

/// Set a screen's picture, with its scaling, clipping and fill colour.
fn set(screen: &NSScreen, path: &str, scaling: isize, clipping: bool, fill: Option<(f64, f64, f64, f64)>) -> Result<(), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let scaling = NSNumber::new_isize(scaling);
    let clipping = NSNumber::new_bool(clipping);
    let colour = fill.map(|(r, g, b, a)| NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a));
    // SAFETY: the keys are AppKit's constant strings.
    let (k_scale, k_clip, k_fill) =
        unsafe { (NSWorkspaceDesktopImageScalingKey, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageFillColorKey) };
    let mut keys = vec![k_scale, k_clip];
    let mut values: Vec<&AnyObject> = vec![&scaling, &clipping];
    if let Some(c) = &colour {
        keys.push(k_fill);
        values.push(c);
    }
    let options = NSDictionary::from_slices(&keys, &values);
    // SAFETY: the options are the documented keys with values of their documented types.
    unsafe { NSWorkspace::sharedWorkspace().setDesktopImageURL_forScreen_options_error(&url, screen, &options) }
        .map_err(|e| e.localizedDescription().to_string())
}

impl Desktop for Workspace {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn per_monitor(&self) -> bool {
        true
    }

    fn show(&self, s: &Screen, path: &str, colour: &str) {
        let Some((_, screen)) = screens().into_iter().find(|(n, _)| *n == s.name) else { return };
        let (r, g, b) = rgb(colour);
        if let Err(e) = set(&screen, path, FIT, true, Some((r, g, b, 1.0))) {
            crate::errln!("{}: the desktop picture: {e}", s.name);
        }
    }

    /// A line per screen: its name, the picture's path, scaling, clipping, and fill colour (r,g,b,a or
    /// empty), separated by tabs.
    fn save(&self) -> Result<String, String> {
        let ws = NSWorkspace::sharedWorkspace();
        let mut out = String::new();
        // SAFETY: AppKit's constant strings.
        let (k_scale, k_clip, k_fill) =
            unsafe { (NSWorkspaceDesktopImageScalingKey, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageFillColorKey) };
        for (name, screen) in screens() {
            let Some(path) = ws.desktopImageURLForScreen(&screen).and_then(|u| u.path()) else { continue };
            let options = ws.desktopImageOptionsForScreen(&screen);
            let get = |k: &NSString| options.as_ref().and_then(|o| o.objectForKey(k));
            let scaling = get(k_scale).and_then(|v| v.downcast::<NSNumber>().ok()).map_or(FIT, |n| n.integerValue());
            let clipping = get(k_clip).and_then(|v| v.downcast::<NSNumber>().ok()).is_some_and(|n| n.boolValue());
            let fill = get(k_fill)
                .and_then(|v| v.downcast::<NSColor>().ok())
                .and_then(|c| c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()))
                .map_or(String::new(), |c| format!("{},{},{},{}", c.redComponent(), c.greenComponent(), c.blueComponent(), c.alphaComponent()));
            out.push_str(&format!("{name}\t{path}\t{scaling}\t{}\t{fill}\n", u8::from(clipping)));
        }
        Ok(out)
    }

    fn restore(&self, saved: &str) -> Result<(), String> {
        let now = screens();
        for line in saved.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 5 {
                continue;
            }
            let Some((_, screen)) = now.iter().find(|(n, _)| n == f[0]) else { continue };
            let fill: Vec<f64> = f[4].split(',').filter_map(|v| v.parse().ok()).collect();
            let fill = (fill.len() == 4).then(|| (fill[0], fill[1], fill[2], fill[3]));
            set(screen, f[1], f[2].parse().unwrap_or(FIT), f[3] == "1", fill).map_err(|e| format!("{}: {e}", f[0]))?;
        }
        Ok(())
    }
}
