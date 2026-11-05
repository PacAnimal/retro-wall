//! retro-wall-setup: installs retro-wall on Windows for the user who runs it (no administrator), shows
//! its settings, and uninstalls it. One download: the wall's program and themes are carried in this
//! program (`retro_wall::setup::payload`).
//!
//!   retro-wall-setup                      the window: Install on a first run, the settings after
//!   retro-wall-setup --install [KEY=VALUE]...       install or update, no window; settings for a first install
//!   retro-wall-setup --set KEY=VALUE...   change the settings, and restart the wall
//!   retro-wall-setup --get                every setting, KEY=VALUE
//!   retro-wall-setup --status             installed or not, its version, the wall running or not
//!   retro-wall-setup --uninstall [--quiet] [--keep-settings]
//!                                         remove it all (asked first, unless --quiet)
//!
//! The keys: `retro_wall::setup::settings::KEYS`. Exit 0 done, 1 not done (why on standard error), 2 a
//! wrong command line.

// on Windows a GUI program: no console window of its own
#![cfg_attr(windows, windows_subsystem = "windows")]
#![warn(clippy::print_stdout, clippy::print_stderr)]

#[cfg(windows)]
mod gui;
#[cfg(windows)]
mod look;
#[cfg(windows)]
mod ops;

#[cfg(not(windows))]
fn main() {
    retro_wall::errln!("retro-wall-setup installs retro-wall on Windows; elsewhere, use linux/install or macos/install");
    std::process::exit(1);
}

#[cfg(windows)]
const USAGE: &str = "usage: retro-wall-setup [--install [KEY=VALUE]... | --set KEY=VALUE... | --get | --status | --uninstall [--quiet] [--keep-settings]]";

#[cfg(windows)]
fn main() {
    use retro_wall::setup::settings::{KEYS, Settings};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |e: String| -> ! {
        retro_wall::errln!("retro-wall-setup: {e}");
        std::process::exit(1)
    };
    let usage = |e: &str| -> ! {
        retro_wall::errln!("retro-wall-setup: {e}\n{USAGE}\nkeys: {}", KEYS.join(", "));
        std::process::exit(2)
    };
    let mut say = |t: &str| retro_wall::outln!("retro-wall-setup: {t}");
    let pairs = |rest: &[String]| -> Settings {
        let mut s = Settings::read(&ops::settings_dir());
        for kv in rest {
            let Some((k, v)) = kv.split_once('=') else { usage(&format!("{kv:?}: KEY=VALUE")) };
            s.set(k, v).unwrap_or_else(|e| usage(&e));
        }
        s
    };
    match args.first().map(String::as_str) {
        None => {
            // a download run over an install: update it (the window, with Update), or its settings
            let installing = ops::installed_version().is_none() || !ops::running_installed() && wants_update();
            if let Err(e) = gui::run(installing) {
                fail(e)
            }
        }
        Some("--install") => {
            let first = (args.len() > 1).then(|| pairs(&args[1..]));
            ops::install(first.as_ref(), &mut say).unwrap_or_else(|e| fail(e));
        }
        Some("--set") => {
            if ops::installed_version().is_none() {
                fail("retro-wall is not installed: --install first".into());
            }
            if args.len() < 2 {
                usage("--set KEY=VALUE...");
            }
            let s = pairs(&args[1..]);
            ops::apply(&s, &mut say).unwrap_or_else(|e| fail(e));
        }
        Some("--get") => {
            for (k, v) in Settings::read(&ops::settings_dir()).pairs() {
                retro_wall::outln!("{k}={v}");
            }
        }
        Some("--status") => {
            retro_wall::outln!("installed={}", ops::installed_version().unwrap_or_else(|| "no".into()));
            retro_wall::outln!("running={}", if ops::wall_running() { "yes" } else { "no" });
            retro_wall::outln!("install={}", ops::install_dir().display());
            retro_wall::outln!("settings={}", ops::settings_dir().display());
        }
        Some("--uninstall") => {
            let quiet = args.iter().any(|a| a == "--quiet");
            let keep = if quiet {
                args.iter().any(|a| a == "--keep-settings")
            } else {
                match gui::confirm_uninstall(std::ptr::null_mut()) {
                    Some(k) => k,
                    None => return,
                }
            };
            match ops::uninstall(keep, &mut say) {
                Ok(()) if !quiet => gui_note("retro-wall is uninstalled, and your own wallpaper is back."),
                Ok(()) => {}
                Err(e) => {
                    if !quiet {
                        gui_note(&format!("retro-wall could not be uninstalled: {e}"));
                    }
                    fail(e)
                }
            }
        }
        // an uninstall's helper (ops::spawn_remover), not for users
        Some("--remove-after") => {
            let (Some(pid), Some(dir)) = (args.get(1).and_then(|p| p.parse::<u32>().ok()), args.get(2)) else { usage("--remove-after PID DIR") };
            ops::remove_after(pid, std::path::Path::new(dir));
        }
        Some("--help" | "-h") => retro_wall::outln!("{USAGE}\nkeys: {}", KEYS.join(", ")),
        Some("--version") => retro_wall::outln!("retro-wall-setup {}", env!("CARGO_PKG_VERSION")),
        Some(other) => usage(&format!("{other:?}?")),
    }
}

/// A download run over an install: update it? Yes opens the window to update with (its settings kept,
/// its progress shown); No opens the settings.
#[cfg(windows)]
fn wants_update() -> bool {
    let installed = ops::installed_version().unwrap_or_default();
    let mine = env!("CARGO_PKG_VERSION");
    let text = if installed == mine {
        format!("retro-wall {mine} is installed. Install it again? Your settings are kept. No opens its settings.")
    } else {
        format!("retro-wall {installed} is installed. Update it to {mine}? Your settings are kept. No opens its settings.")
    };
    let (t, m) = (retro_wall::platform::windows::wide("retro-wall"), retro_wall::platform::windows::wide(&text));
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
    // SAFETY: NUL-terminated strings.
    unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_ICONQUESTION | MB_YESNO) == IDYES }
}

#[cfg(windows)]
fn gui_note(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
    let (t, m) = (retro_wall::platform::windows::wide("retro-wall"), retro_wall::platform::windows::wide(text));
    // SAFETY: NUL-terminated strings.
    unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_ICONINFORMATION | MB_OK) };
}
