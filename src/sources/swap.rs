//! swap: swap space in use, of all there is.

use super::{Builtin, Source, demo_time, wave};
use crate::platform::{Native, Platform};

pub const SOURCE: Builtin = Builtin { name: "swap", path: false, every: 1.0, fixed: 17.0, make };

fn make(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Swap { demo }))
}

struct Swap {
    demo: bool,
}

impl Source for Swap {
    fn read(&mut self) -> Option<(f64, bool)> {
        if self.demo {
            return Some((17.0 + 2.0 * wave(demo_time(), &[(53.0, 1.0)]), false));
        }
        Some((Native::memory()?.1, false))
    }
}
