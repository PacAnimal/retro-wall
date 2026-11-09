//! What moves over the baked picture without touching it: smoke from the fires and chimneys, fireflies
//! over the pond and the fields at night, a flock of birds crossing the sky now and then. Each a few
//! squares of pixel art, placed in the world, seen through the camera, a function of the clock.

use super::render::{Camera, V3, add, mul};
use super::sky::Moment;

/// A square on the screen: left, top, right, bottom and its opacity.
pub type Square = (f64, f64, f64, f64, f64);

fn hash(k: usize, salt: u64) -> f64 {
    crate::rng::keyed(0, k as i64, salt)
}

/// A square `size` blocks across about a point of the world, on whole pixels; None off the screen.
fn square(cam: &Camera, p: V3, size: f64, a: f64) -> Option<Square> {
    sized(cam, p, size, f64::INFINITY, a)
}

/// The same, at most `most` pixels across, however near.
fn sized(cam: &Camera, p: V3, size: f64, most: f64, a: f64) -> Option<Square> {
    let (x, y, k) = cam.project(p)?;
    let s = (size * k).clamp(1.0, most.max(1.0)).round();
    let (x0, y0) = ((x - s / 2.0).round(), (y - s / 2.0).round());
    (x0 > -s && y0 > -s && x0 < cam.w as f64 && y0 < cam.h as f64).then_some((x0, y0, x0 + s, y0 + s, a))
}

/// Whether a chimney is lit at `t`: each in its own spells of a few minutes, lit or not at random;
/// most of the spells at night, a few by day (`day`, 0 to 1).
fn lit(i: usize, t: f64, day: f64) -> bool {
    const SPELL: f64 = 240.0;
    let spell = (t / SPELL + hash(i, 6)).floor() as usize;
    hash(i * 1013 + spell, 7) < 0.6 - 0.4 * day
}

/// How dark it is for what shows only at night (fires, fireflies): 0 by day, 1 at night, rising
/// through dusk.
fn night(m: &Moment) -> f64 {
    ((1.0 - m.day) * 1.4 - 0.2).clamp(0.0, 1.0)
}

/// The pig's puffs of smoke (each where it went up and how long ago, `pig::PUFF` at most): a burst of
/// square puffs flung out every way, slowing, growing and thinning as they go; in two greys, light by
/// day and by night. The puffs in each grey, and each's colour.
pub fn puffs(cam: &Camera, from: &[(V3, f64)], m: &Moment) -> [(Vec<Square>, (f64, f64, f64)); 2] {
    let (mut dark, mut light) = (vec![], vec![]);
    let k = 0.55 + 0.45 * m.day;
    for &(at, age) in from {
        // each its own, by where it is, however many are in the air
        let id = (at[0].to_bits() ^ at[1].to_bits().rotate_left(21) ^ at[2].to_bits().rotate_left(42)) as usize;
        let life = age / super::pig::PUFF;
        let out = 1.0 - (1.0 - life).powi(3);
        for p in 0..18 {
            let n = id.wrapping_add(p * 131);
            let (a, b) = (hash(n, 11) * std::f64::consts::TAU, hash(n, 12) * 2.0 - 1.0);
            let across = (1.0 - b * b).sqrt();
            let dir = [across * a.cos(), b * 0.7 + 0.3, across * a.sin()];
            let reach = 0.6 + hash(n, 13) * 0.9;
            let q = add(at, mul(dir, reach * out));
            let q = add(q, [0.0, life * 0.6, 0.0]);
            let size = 0.25 + hash(n, 14) * 0.25 + life * 0.35;
            let alpha = 0.95 * (1.0 - life).powf(1.5);
            if let Some(s) = square(cam, q, size, alpha) {
                if hash(n, 15) < 0.45 { dark.push(s) } else { light.push(s) }
            }
        }
    }
    [(dark, (0.72 * k, 0.72 * k, 0.75 * k)), (light, (0.95 * k, 0.95 * k, 0.97 * k))]
}

/// Smoke from each of `from` (a chimney only while it is lit, a fire only at night): square puffs rising, drifting downwind,
/// growing and thinning as they go, each on a way of its own; a puff already let out goes on when its
/// chimney is put out. In two greys, a puff one or the other, a chimney's mostly the darker, a fire's
/// the lighter; grey by day, lighter than the dark at night, so it is seen: the puffs in each, and
/// each's colour.
pub fn smoke(cam: &Camera, from: &[(V3, bool)], t: f64, m: &Moment) -> [(Vec<Square>, (f64, f64, f64)); 2] {
    let (mut dark, mut light) = (vec![], vec![]);
    let lit_fire = night(m);
    for (i, &(e, chimney)) in from.iter().enumerate() {
        if !chimney && lit_fire <= 0.0 {
            continue;
        }
        // a chimney puffs thickly; a fire's column is slower and taller, its puffs larger and lighter
        let (puffs, rise, period, grow, darker) = if chimney { (14, 7.5, 7.0, 1.0, 0.7) } else { (12, 8.0, 10.0, 1.3, 0.2) };
        for k in 0..puffs {
            let run = t / period + k as f64 / puffs as f64 + hash(i, 1);
            let age = run.fract();
            if chimney && !lit(i, t - age * period, m.day) {
                continue;
            }
            // which puff this is, for its own way, size and grey
            let id = i * 7919 + k * 131 + run.floor() as usize;
            let (jx, jz, ph) = (hash(id, 14) - 0.5, hash(id, 15) - 0.5, 6.3 * hash(id, 16));
            let sway = 0.3 * (t * 0.7 + ph).sin() * age.sqrt();
            let p = add(e, [(1.6 + 0.6 * jx) * age + sway + 0.12 * jx, rise * age * (0.9 + 0.2 * hash(id, 17)), (0.8 + 0.6 * jz) * age + 0.12 * jz]);
            let a = 0.75 * (age * 6.0).min(1.0) * (1.0 - age).powf(1.3) * if chimney { 1.0 } else { lit_fire };
            let size = (0.3 + 0.9 * age) * grow * (0.85 + 0.3 * hash(id, 18));
            let to = if hash(id, 19) < darker { &mut dark } else { &mut light };
            to.extend(square(cam, p, size, a));
        }
    }
    let mix = |night: f64, day: f64| night + (day - night) * m.day;
    [(dark, (mix(0.36, 0.5), mix(0.37, 0.5), mix(0.42, 0.53))), (light, (mix(0.46, 0.68), mix(0.48, 0.68), mix(0.54, 0.7)))]
}

/// Fireflies about each glade at night, drifting low and blinking now and then: their glow and their
/// bright cores. Specks: a pixel or two at the core, however near, so they do not read as lamps.
pub fn fireflies(cam: &Camera, glades: &[(V3, f64)], t: f64, m: &Moment) -> (Vec<Square>, Vec<Square>) {
    const EACH: usize = 3;
    let night = night(m);
    let (mut glow, mut core) = (vec![], vec![]);
    if night <= 0.0 {
        return (glow, core);
    }
    // a pixel or two on a 1080-line screen, as many more as the screen is taller
    let px = (cam.h as f64 / 1080.0).max(1.0);
    for (g, &(c, reach)) in glades.iter().enumerate() {
        for k in 0..EACH {
            let i = g * EACH + k;
            let (a, b, ph) = (0.08 + 0.06 * hash(i, 2), 0.07 + 0.05 * hash(i, 3), 6.3 * hash(i, 4));
            let p = add(c, [reach * 0.8 * (t * a + ph).sin(), 0.5 + 1.4 * (0.5 + 0.5 * (t * 0.37 + ph * 2.0).sin()), reach * 0.8 * (t * b + ph * 1.7).cos()]);
            // lit for a moment, then dark for longer
            let blink = (t * (0.5 + 0.4 * hash(i, 5)) + ph * 3.0).sin().max(0.0).powi(8) * night;
            if blink < 0.02 {
                continue;
            }
            glow.extend(sized(cam, p, 0.12, 4.0 * px, 0.18 * blink));
            core.extend(sized(cam, p, 0.04, 2.0 * px, blink));
        }
    }
    (glow, core)
}

/// Smooth noise over the plane, 0 to 1: a value at each whole point, eased between them.
fn noise(x: f64, y: f64, salt: u64) -> f64 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |dx: f64, dy: f64| {
        let (a, b) = ((xi + dx) as i64, (yi + dy) as i64);
        hash((a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b)) as usize, salt)
    };
    let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * sx;
    let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

/// A campfire's flames, in sixteenths of a block: how many across each of its two pictures and how many
/// up, and how high over the cell's floor they start (on the logs, among the embers).
const FLAME_W: usize = 10;
const FLAME_H: usize = 12;
const FLAME_FOOT: f64 = 0.22;
/// The flames' colours, from their tips to their hearts: deep red, orange, yellow, nearly white.
pub const FLAME: [(f64, f64, f64); 4] = [(0.78, 0.2, 0.05), (0.98, 0.46, 0.08), (1.0, 0.76, 0.18), (1.0, 0.94, 0.6)];

/// How hot a campfire's flame is at a pixel of one of its pictures (`u` across, `v` up, in sixteenths)
/// at `t`: hottest low in the middle, cooling up and out, and torn into tongues by noise that rises
/// through it, so the tongues lick upwards and change a little at a time, never jumping.
pub fn flame_heat(u: usize, v: usize, t: f64, salt: u64) -> f64 {
    let x = (u as f64 + 0.5) / FLAME_W as f64 * 2.0 - 1.0;
    let h = (v as f64 + 0.5) / FLAME_H as f64;
    let body = (1.0 - x.abs().powf(1.6)).max(0.0) * (1.0 - h).max(0.0).powf(0.7);
    let rise = v as f64 - t * 7.0;
    let tongues = 0.65 * noise(u as f64 * 0.45, rise * 0.3, salt) + 0.35 * noise(u as f64 * 0.9, rise * 0.6, salt ^ 0x55);
    body * 1.35 - tongues * 1.05 * (0.2 + h)
}

/// The flames of each fire of `fires` (those seen from the camera, each the middle of its cell's floor),
/// lit at night only: two pictures of flame crossed in its cell as its logs are, standing on them, a
/// square for each of their pixels: in each of `FLAME`'s colours, coolest first. A fire burns from dusk
/// to dawn; by day its logs lie cold.
pub fn flames(cam: &Camera, fires: &[V3], t: f64, m: &Moment) -> [Vec<Square>; 4] {
    let mut out: [Vec<Square>; 4] = Default::default();
    let lit = night(m);
    if lit <= 0.0 {
        return out;
    }
    const SIXTEENTH: f64 = 1.0 / 16.0;
    for (f, &base) in fires.iter().enumerate() {
        for (k, across) in {
            use std::f64::consts::FRAC_1_SQRT_2 as D;
            [[D, 0.0, D], [D, 0.0, -D]]
        }
        .into_iter()
        .enumerate()
        {
            let salt = 40 + (f * 2 + k) as u64;
            for v in 0..FLAME_H {
                for u in 0..FLAME_W {
                    let heat = flame_heat(u, v, t, salt);
                    let Some(c) = [0.14, 0.34, 0.62, 0.92].iter().rposition(|&edge| heat > edge) else { continue };
                    let along = (u as f64 + 0.5 - FLAME_W as f64 / 2.0) * SIXTEENTH;
                    let p = add(base, add(mul(across, along), [0.0, FLAME_FOOT + (v as f64 + 0.5) * SIXTEENTH, 0.0]));
                    // a hair over a pixel of the picture, so its squares close up; while they fade in
                    // at dusk, exactly a pixel, so no overlap shows twice as bright
                    out[c].extend(square(cam, p, SIXTEENTH * if lit < 1.0 { 1.0 } else { 1.15 }, lit));
                }
            }
        }
    }
    out
}

/// Now and then, rarely, a bird crossing the sky high over the land, or a few in a V, flapping; by day
/// only. Each spell of a few minutes has one crossing or none, its way across, its height, how far off
/// and how many each its own.
pub fn birds(cam: &Camera, t: f64, m: &Moment) -> Vec<Square> {
    const SPELL: f64 = 300.0;
    const CROSSING: f64 = 45.0;
    let spell = (t / SPELL).floor().max(0.0) as usize;
    let chance = hash(spell, 8);
    if chance < 0.7 || m.day < 0.3 {
        return vec![];
    }
    let count = if chance < 0.85 { 1 } else { 3 + (hash(spell, 9) * 3.0) as usize };
    let u = (t - spell as f64 * SPELL - hash(spell, 10) * (SPELL - CROSSING)) / CROSSING;
    if !(0.0..1.0).contains(&u) {
        return vec![];
    }
    let way = if hash(spell, 11) < 0.5 { 1.0 } else { -1.0 };
    let (far, high) = (140.0 + 60.0 * hash(spell, 12), 35.0 + 25.0 * hash(spell, 13));
    let f = [cam.forward[0], 0.0, cam.forward[2]];
    let left = mul(cam.right, -1.0);
    let at = |s: f64, rise: f64| add(add(cam.eye, mul(f, far)), add(mul(left, s), [0.0, rise, 0.0]));
    let (from, to) = (at(150.0 * way, high), at(-150.0 * way, high + 8.0));
    let centre = add(from, mul(add(to, mul(from, -1.0)), u));
    let along = mul(cam.right, way);
    let mut out = vec![];
    for k in 0..count {
        let rank = k as f64 - (count / 2) as f64;
        // the V: those to either side of the leader further back
        let p = add(add(add(centre, mul(along, -2.6 * rank.abs())), mul(f, 2.2 * rank)), [0.0, 0.4 * (t * 0.8 + rank).sin(), 0.0]);
        let Some((x, y, px)) = cam.project(p) else { continue };
        let s = (0.9 * px).max(1.0).round();
        let flap = (t * 9.0 + k as f64 * 0.9).sin() * s * 0.8;
        let (x, y) = (x.round(), y.round());
        out.push((x - s / 2.0, y - s / 2.0, x + s / 2.0, y + s / 2.0, m.day));
        out.push((x - s * 2.0, y - s / 2.0 + flap.round(), x - s / 2.0, y + flap.round(), m.day));
        out.push((x + s / 2.0, y - s / 2.0 + flap.round(), x + s * 2.0, y + flap.round(), m.day));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fireflies_are_specks_and_few() {
        let cam = Camera::new([0.0, 74.0, 0.0], 0.6, -0.2, 78.0, 1920, 1080);
        let m = Moment::at(23.5 / 24.0, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0));
        // one glade just in front of the eye, where a firefly would look largest
        let near = [(add(cam.eye, mul(cam.forward, 3.0)), 1.0)];
        let mut most = 0;
        for s in 0..2000 {
            let (glow, core) = fireflies(&cam, &near, s as f64 * 0.1, &m);
            assert!(glow.iter().all(|q| q.2 - q.0 <= 4.0) && core.iter().all(|q| q.2 - q.0 <= 2.0), "{glow:?} {core:?}");
            most = most.max(core.len());
        }
        assert!((1..=3).contains(&most), "at most a few lit at once: {most}");
    }

    #[test]
    fn birds_are_rare_and_some_fly_alone() {
        let cam = Camera::new([0.0, 74.0, 0.0], 0.6, -0.2, 78.0, 1920, 1080);
        let m = Moment::at(13.0 / 24.0, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0));
        let seen: Vec<usize> = (0..40_000).map(|s| birds(&cam, s as f64, &m).len() / 3).collect();
        let share = seen.iter().filter(|&&n| n > 0).count() as f64 / seen.len() as f64;
        assert!((0.01..0.1).contains(&share), "in the sky {:.1}% of the time", share * 100.0);
        assert!(seen.contains(&1), "a bird alone, now and then");
        assert!(seen.iter().any(|&n| n >= 3), "a few together, now and then");
        let night = Moment::at(23.5 / 24.0, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0));
        assert!((0..40_000).all(|s| birds(&cam, s as f64, &night).is_empty()), "none by night");
    }
}
