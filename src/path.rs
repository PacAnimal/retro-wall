//! SVG path data: M L H V C S Q T Z, absolute and relative. Parsed once into absolute segments, with
//! quadratic curves raised to cubics; a path that cannot be read is an error that says where.

use cairo::{Context, Format, ImageSurface};

/// One segment, in absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

/// A parsed path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathData(pub Vec<Seg>);

/// Why a path could not be read, and the segments read before that point.
#[derive(Clone, Debug)]
pub struct PathError {
    pub what: String,
    pub done: PathData,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.what)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Command(u8),
    Number(f64),
}

/// The path's tokens: command letters and numbers. Anything else separates them.
fn tokens(d: &str) -> Result<Vec<Token>, String> {
    let b = d.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() {
            out.push(Token::Command(b[i]));
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        if b[j] == b'+' || b[j] == b'-' {
            j += 1;
        }
        let end = if j < b.len() && b[j].is_ascii_digit() {
            let mut k = digits(j);
            if k < b.len() && b[k] == b'.' {
                k = digits(k + 1);
            }
            Some(k)
        } else if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            Some(digits(j + 1))
        } else {
            None
        };
        match end {
            Some(mut k) => {
                if k < b.len() && (b[k] == b'e' || b[k] == b'E') {
                    let mut e = k + 1;
                    if e < b.len() && (b[e] == b'-' || b[e] == b'+') {
                        e += 1;
                    }
                    if e < b.len() && b[e].is_ascii_digit() {
                        k = digits(e);
                    }
                }
                let n: f64 = d[start..k].parse().map_err(|_| format!("{:?} is not a number", &d[start..k]))?;
                if !n.is_finite() {
                    return Err(format!("{:?} is out of range", &d[start..k]));
                }
                out.push(Token::Number(n));
                i = k;
            }
            None => i += 1,
        }
    }
    Ok(out)
}

impl PathData {
    /// Parse path data. It starts with a move; every command has all its numbers.
    pub fn parse(d: &str) -> Result<PathData, PathError> {
        let mut segs = vec![];
        match Self::read(d, &mut segs) {
            Ok(()) => Ok(PathData(segs)),
            Err(what) => Err(PathError { what, done: PathData(segs) }),
        }
    }

    fn read(d: &str, segs: &mut Vec<Seg>) -> Result<(), String> {
        let tokens = tokens(d)?;
        let (mut x, mut y, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        // the last curve's second control point, for S and T
        let (mut cc, mut qc): (Option<(f64, f64)>, Option<(f64, f64)>) = (None, None);
        let mut i = 0;
        let mut cmd = 0u8;
        let num = |i: &mut usize, cmd: u8| -> Result<f64, String> {
            *i += 1;
            match tokens.get(*i - 1) {
                Some(Token::Number(n)) => Ok(*n),
                Some(Token::Command(c)) => Err(format!("{} where {} needs a number", *c as char, cmd as char)),
                None => Err(format!("it ends where {} needs a number", cmd as char)),
            }
        };
        while i < tokens.len() {
            if let Token::Command(c) = tokens[i] {
                if !b"MmLlHhVvCcSsQqTtZz".contains(&c) {
                    return Err(format!("command {} (only M L H V C S Q T Z are drawn)", c as char));
                }
                if cmd == 0 && !matches!(c, b'M' | b'm') {
                    return Err(format!("it starts with {}, not a move (M)", c as char));
                }
                cmd = c;
                i += 1;
                if cmd == b'Z' || cmd == b'z' {
                    segs.push(Seg::Close);
                    (x, y, cc, qc) = (sx, sy, None, None);
                    continue;
                }
            } else if cmd == 0 {
                return Err("it starts with a number, not a move (M)".into());
            } else if cmd == b'Z' || cmd == b'z' {
                return Err("a number after Z".into());
            }
            let rel = cmd.is_ascii_lowercase();
            let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
            match cmd.to_ascii_uppercase() {
                b'M' => {
                    x = ox + num(&mut i, cmd)?;
                    y = oy + num(&mut i, cmd)?;
                    (sx, sy, cc, qc) = (x, y, None, None);
                    segs.push(Seg::Move(x, y));
                    cmd = if rel { b'l' } else { b'L' };
                }
                u @ (b'L' | b'H' | b'V') => {
                    if u == b'L' {
                        x = ox + num(&mut i, cmd)?;
                        y = oy + num(&mut i, cmd)?;
                    } else if u == b'H' {
                        x = ox + num(&mut i, cmd)?;
                    } else {
                        y = oy + num(&mut i, cmd)?;
                    }
                    segs.push(Seg::Line(x, y));
                    (cc, qc) = (None, None);
                }
                u @ (b'Q' | b'T') => {
                    let (x1, y1) = if u == b'Q' {
                        let a = ox + num(&mut i, cmd)?;
                        (a, oy + num(&mut i, cmd)?)
                    } else {
                        qc.map_or((x, y), |(qx, qy)| (2.0 * x - qx, 2.0 * y - qy))
                    };
                    let (x0, y0) = (x, y);
                    x = ox + num(&mut i, cmd)?;
                    y = oy + num(&mut i, cmd)?;
                    let t = 2.0 / 3.0;
                    segs.push(Seg::Curve(x0 + t * (x1 - x0), y0 + t * (y1 - y0), x + t * (x1 - x), y + t * (y1 - y), x, y));
                    (qc, cc) = (Some((x1, y1)), None);
                }
                u => {
                    let (x1, y1) = if u == b'C' {
                        let a = ox + num(&mut i, cmd)?;
                        (a, oy + num(&mut i, cmd)?)
                    } else {
                        cc.map_or((x, y), |(px, py)| (2.0 * x - px, 2.0 * y - py))
                    };
                    let x2 = ox + num(&mut i, cmd)?;
                    let y2 = oy + num(&mut i, cmd)?;
                    x = ox + num(&mut i, cmd)?;
                    y = oy + num(&mut i, cmd)?;
                    segs.push(Seg::Curve(x1, y1, x2, y2, x, y));
                    (cc, qc) = (Some((x2, y2)), None);
                }
            }
        }
        Ok(())
    }

    /// Trace it on `c`.
    pub fn trace(&self, c: &Context) {
        for s in &self.0 {
            match *s {
                Seg::Move(x, y) => c.move_to(x, y),
                Seg::Line(x, y) => c.line_to(x, y),
                Seg::Curve(a, b, cx, cy, x, y) => c.curve_to(a, b, cx, cy, x, y),
                Seg::Close => c.close_path(),
            }
        }
    }

    /// It traced on a scratch context, so its points are cairo's fixed-point ones. None only if cairo
    /// cannot make a 1x1 surface.
    pub fn traced(&self) -> Option<cairo::Path> {
        let scratch = Context::new(ImageSurface::create(Format::A8, 1, 1).ok()?).ok()?;
        self.trace(&scratch);
        scratch.copy_path().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Seg::*;

    fn segs(d: &str) -> Vec<Seg> {
        PathData::parse(d).unwrap_or_else(|e| panic!("{d}: {e}")).0
    }

    #[test]
    fn relative_commands_are_made_absolute() {
        assert_eq!(segs("M10 20 l5 5 h-5 v10 z"), [Move(10.0, 20.0), Line(15.0, 25.0), Line(10.0, 25.0), Line(10.0, 35.0), Close]);
        // after a close, a relative move starts from the subpath's start
        assert_eq!(segs("m1,1 l1,0 z m2,2"), [Move(1.0, 1.0), Line(2.0, 1.0), Close, Move(3.0, 3.0)]);
    }

    #[test]
    fn numbers_run_together_as_svg_writes_them() {
        // implicit lines after a move, signs and dots as separators, exponents
        assert_eq!(segs("M0-1.5.5.5L1e1,2E0"), [Move(0.0, -1.5), Line(0.5, 0.5), Line(10.0, 2.0)]);
    }

    #[test]
    fn quadratics_are_raised_to_cubics() {
        let Curve(x1, y1, x2, y2, x, y) = segs("M0 0 Q3 3 6 0")[1] else { panic!("not a curve") };
        assert_eq!((x1, y1, x2, y2, x, y), (2.0, 2.0, 4.0, 2.0, 6.0, 0.0));
        // a smooth curve reflects the last control point
        let Curve(x1, y1, ..) = segs("M0 0 C1 1 2 1 3 0 S5 -1 6 0")[2] else { panic!("not a curve") };
        assert_eq!((x1, y1), (4.0, -1.0));
    }

    #[test]
    fn what_cannot_be_read_says_where_and_keeps_what_came_before() {
        for (d, says) in [("L1 1", "not a move"), ("M0 0 A1 1 0 0 1 2 2", "command A"), ("M0 0 L1", "needs a number"), ("M0 0 L1 Z", "Z where L")] {
            let e = PathData::parse(d).expect_err(d);
            assert!(e.what.contains(says), "{d}: {}", e.what);
        }
        assert_eq!(PathData::parse("M0 0 L1 1 L2").unwrap_err().done.0.len(), 2);
    }
}
