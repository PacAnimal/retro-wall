//! Scenes traced on the CPU, for themes that draw a world of their own in 3D: the camera, rays and
//! boxes, models made of boxes that turn on hinges, and things that move over a picture traced once.
//! A theme says what its world and its models are and how they are painted; these do the geometry,
//! and keep the work each frame to what has changed.
//!
//! - `camera`: the eye, the ray through a pixel, where a point falls on the screen.
//! - `boxes`: a ray and a box, exactly and quickly.
//! - `rig`: a model of boxes, each part turned on its hinge, placed and turned in the world; posed once
//!   a frame, then traced pixel by pixel.
//! - `sprites`: what moves over a traced picture: which pixels to paint again when something has
//!   moved, and the traced picture under them.

pub mod boxes;
pub mod camera;
pub mod rig;
pub mod sprites;

pub use camera::{Camera, Px, V3, add, cross, dot, mul, norm};

/// Pictures filled row by row on every core: `fill` is given each row's number and that row of each
/// picture (`stride` bytes a row), in no particular order. A traced picture's rows are each their own.
pub fn rows<const N: usize>(pictures: [&mut [u8]; N], stride: usize, fill: impl Fn(usize, [&mut [u8]; N]) + Sync) {
    let mut chunks = pictures.map(|p| p.chunks_mut(stride));
    let mut all = vec![];
    while let Some(row) = chunks.iter_mut().map(|c| c.next()).collect::<Option<Vec<&mut [u8]>>>() {
        let Ok(row) = <[&mut [u8]; N]>::try_from(row) else { break };
        all.push((all.len(), row));
    }
    let queue = std::sync::Mutex::new(all.into_iter());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(32);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
                    let Some((y, row)) = next else { break };
                    fill(y, row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn rows_are_each_filled_once() {
        let (mut a, mut b) = (vec![0u8; 12 * 100], vec![0u8; 12 * 100]);
        super::rows([&mut a, &mut b], 12, |y, [ra, rb]| {
            ra.iter_mut().for_each(|v| *v += y as u8);
            rb.iter_mut().for_each(|v| *v += 1);
        });
        assert!(a.chunks(12).enumerate().all(|(y, r)| r.iter().all(|&v| v == y as u8)));
        assert!(b.iter().all(|&v| v == 1));
    }
}
