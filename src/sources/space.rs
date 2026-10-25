//! used and free: the space of the drive holding `/`, or the path after a colon (`used:/home`).

use super::{Builtin, Source};
use crate::platform::{Native, Platform};

pub const USED: Builtin = Builtin { name: "used", path: true, every: 30.0, fixed: 64.0, make: used };
pub const FREE: Builtin = Builtin { name: "free", path: true, every: 30.0, fixed: 36.0, make: free };

fn used(path: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    make(path, demo, true)
}

fn free(path: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    make(path, demo, false)
}

fn make(path: Option<&str>, demo: bool, used: bool) -> Option<Box<dyn Source>> {
    let path = path.unwrap_or("/");
    if path.contains('\0') {
        return None;
    }
    Some(Box::new(Space { path: path.to_string(), used, demo }))
}

struct Space {
    path: String,
    used: bool,
    demo: bool,
}

impl Source for Space {
    fn read(&mut self) -> Option<(f64, bool)> {
        let (used, free) = if self.demo { (64.0, 36.0) } else { Native::drive_space(&self.path).unwrap_or((0.0, 0.0)) };
        Some((if self.used { used } else { free }, false))
    }
}
