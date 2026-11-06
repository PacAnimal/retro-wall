//! What moves over a picture traced once: animals, carts, birds, anything a theme traces into the
//! scene each frame. The picture under them is kept (`Backdrop`) with how far off each of its pixels
//! is, so what moves is traced before what is behind it and behind what is before it. Each frame only
//! what has changed is painted again (`Changes`): where something has moved, the pixels it left and
//! the ones it is in now. A pixel is the backdrop's and whatever is traced over it, so painting a
//! region again makes what painting all of it would.

use super::camera::{Px, overlap};

/// A rectangle of a traced picture kept as traced, under what moves: pictures of four bytes a pixel
/// (B, G, R, A rows, or whatever the theme keeps), and how far off each pixel is.
pub struct Backdrop {
    pub at: Px,
    pub planes: Vec<Vec<u8>>,
    pub depth: Vec<f32>,
}

impl Backdrop {
    /// The rectangle `at` taken from whole pictures `w` pixels across, with the depth of each pixel, asked
    /// for row by row.
    pub fn take(at: Px, w: usize, pictures: &[&[u8]], mut depth: impl FnMut(i32, i32) -> f32) -> Backdrop {
        let (bw, bh) = ((at.2 - at.0) as usize, (at.3 - at.1) as usize);
        let planes = pictures
            .iter()
            .map(|pic| {
                let mut out = Vec::with_capacity(bw * bh * 4);
                for r in 0..bh {
                    let from = ((at.1 as usize + r) * w + at.0 as usize) * 4;
                    out.extend_from_slice(&pic[from..from + bw * 4]);
                }
                out
            })
            .collect();
        let depth = (at.1..at.3).flat_map(|y| (at.0..at.2).map(move |x| (x, y))).map(|(x, y)| depth(x, y)).collect();
        Backdrop { at, planes, depth }
    }

    fn width(&self) -> usize {
        (self.at.2 - self.at.0) as usize
    }

    /// Where a pixel of the screen is in its pictures, counted in pixels.
    pub fn index(&self, x: i32, y: i32) -> usize {
        (y - self.at.1) as usize * self.width() + (x - self.at.0) as usize
    }

    /// Whether everything in `px` that it holds is nearer than `near`: something no nearer than that is
    /// hidden behind it there.
    pub fn hides(&self, px: Px, near: f32) -> bool {
        let Some(r) = overlap(px, self.at) else { return true };
        (r.1..r.3).all(|y| {
            let i = self.index(r.0, y);
            self.depth[i..i + (r.2 - r.0) as usize].iter().all(|&d| d <= near)
        })
    }

    /// A copy of its pictures and depths over `clip` (within it), to trace into.
    pub fn crop(&self, clip: Px) -> (Vec<Vec<u8>>, Vec<f32>) {
        let (cw, ch) = ((clip.2 - clip.0) as usize, (clip.3 - clip.1) as usize);
        let rows = |from: &[u8]| {
            let mut out = Vec::with_capacity(cw * ch * 4);
            for y in clip.1..clip.3 {
                let i = self.index(clip.0, y) * 4;
                out.extend_from_slice(&from[i..i + cw * 4]);
            }
            out
        };
        let mut depth = Vec::with_capacity(cw * ch);
        for y in clip.1..clip.3 {
            let i = self.index(clip.0, y);
            depth.extend_from_slice(&self.depth[i..i + cw]);
        }
        (self.planes.iter().map(|p| rows(p)).collect(), depth)
    }
}

/// Which pixels to paint again: what each moving thing was when last painted (`K`, all its picture
/// depends on, compared exactly) and the pixels it could be in then.
pub struct Changes<K> {
    shown: Vec<Option<(K, Option<Px>)>>,
}

impl<K: PartialEq + Copy> Changes<K> {
    /// For `n` things, none painted yet.
    pub fn new(n: usize) -> Changes<K> {
        Changes { shown: (0..n).map(|_| None).collect() }
    }

    /// The things as they are now, each its key and the pixels it can be in (None off the screen): the
    /// regions to paint again, each with everything over it, none overlapping another.
    pub fn since(&mut self, now: impl IntoIterator<Item = (K, Option<Px>)>) -> Vec<Px> {
        let mut dirty = vec![];
        for (shown, now) in self.shown.iter_mut().zip(now) {
            if *shown != Some(now) {
                dirty.extend(shown.and_then(|s| s.1));
                dirty.extend(now.1);
                *shown = Some(now);
            }
        }
        merge(&mut dirty);
        dirty
    }
}

/// Rectangles that overlap made one, until none do.
pub fn merge(rects: &mut Vec<Px>) {
    let mut merged = true;
    while merged {
        merged = false;
        'pairs: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                if overlap(rects[i], rects[j]).is_some() {
                    let (a, b) = (rects[i], rects[j]);
                    rects[i] = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
                    rects.swap_remove(j);
                    merged = true;
                    break 'pairs;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_moved_is_painted_again_where_it_was_and_where_it_is() {
        let mut c: Changes<u32> = Changes::new(3);
        let first = c.since([(1, Some((0, 0, 10, 10))), (1, Some((50, 0, 60, 10))), (1, None)]);
        assert_eq!(first.len(), 2, "everything at first: {first:?}");
        assert!(c.since([(1, Some((0, 0, 10, 10))), (1, Some((50, 0, 60, 10))), (1, None)]).is_empty());
        // the first moves a little: where it was and where it is, one region
        assert_eq!(c.since([(2, Some((5, 0, 15, 10))), (1, Some((50, 0, 60, 10))), (1, None)]), [(0, 0, 15, 10)]);
        // the second goes off the screen: where it was
        assert_eq!(c.since([(2, Some((5, 0, 15, 10))), (2, None), (1, None)]), [(50, 0, 60, 10)]);
    }

    #[test]
    fn a_backdrop_is_cropped_and_hides_what_is_behind_it() {
        let (w, h) = (8usize, 6usize);
        let pic: Vec<u8> = (0..w * h * 4).map(|i| (i / 4) as u8).collect();
        let b = Backdrop::take((2, 1, 6, 5), w, &[&pic], |x, _| if x < 4 { 1.0 } else { 10.0 });
        let (planes, depth) = b.crop((3, 2, 5, 4));
        assert_eq!(planes[0].chunks(4).map(|p| p[0]).collect::<Vec<_>>(), [19, 20, 27, 28]);
        assert_eq!(depth, [1.0, 10.0, 1.0, 10.0]);
        assert!(b.hides((2, 1, 4, 5), 5.0), "all nearer there");
        assert!(!b.hides((2, 1, 5, 5), 5.0), "not where it is far");
        assert!(b.hides((20, 20, 30, 30), 5.0), "nothing of it shows outside it");
    }
}
