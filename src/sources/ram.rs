//! ram: memory in use, of all there is.

use super::{Builtin, Source, demo_time, wave};
use crate::platform::{Native, Platform};

pub const SOURCE: Builtin = Builtin { name: "ram", path: false, every: 1.0, fixed: 61.0, make };

fn make(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Ram { demo }))
}

struct Ram {
    demo: bool,
}

impl Source for Ram {
    fn read(&mut self) -> Option<(f64, bool)> {
        if self.demo {
            return Some((58.0 + 6.0 * wave(demo_time(), &[(37.0, 0.0), (11.0, 2.0)]), false));
        }
        Some((Native::memory()?.0, false))
    }
}
