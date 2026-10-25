//! retro-wall: a live desktop wallpaper. The binary is main.rs; everything else is here.

// output goes through outln!/errln!: println! and eprintln! panic when the reader has gone (EPIPE)
#![warn(clippy::print_stdout, clippy::print_stderr)]

/// As println!, ignoring a standard output that is closed or fails (`retro-wall --list | head -1`).
#[macro_export]
macro_rules! outln {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stdout(), $($arg)*);
    }};
}

/// As print!, ignoring a standard output that is closed or fails.
#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::write!(::std::io::stdout(), $($arg)*);
    }};
}

/// As eprintln!, ignoring a standard error that is closed or fails.
#[macro_export]
macro_rules! errln {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr(), $($arg)*);
    }};
}

pub mod cli;
pub mod config;
pub mod day;
pub mod elements;
pub mod files;
pub mod font;
pub mod gfx;
pub mod live;
pub mod path;
pub mod platform;
pub mod plugins;
pub mod rng;
pub mod scene;
pub mod sources;
pub mod still;
pub mod svg;
pub mod title;
pub mod tweaks;
pub mod util;
