//! What moves over the baked picture without touching it: smoke from the fires and chimneys, fireflies
//! over the pond and the fields at night, a flock of birds crossing the sky now and then. Each a few
//! squares of pixel art, placed in the world, seen through the camera, a function of the clock.

use super::render::{Camera, V3, add, mul};
use super::sky::Moment;

/// A square on the screen: left, top, right, bottom and its opacity.
pub type Square = (f64, f64, f64, f64, f64);

fn hash(k: usize, salt: u64) -> f64 {
    let mut h = (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ salt.wrapping_mul(0xd1b5_4a32_d192_ed03);
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 31;
    (h >> 11) as f64 / (1u64 << 53) as f64
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

/// The flames of each fire of `fires` (those seen from the camera), lit at night only: square licks
/// rising from its logs and flickering, the outer orange and the inner yellow. A fire burns from dusk
/// to dawn; by day its logs lie cold.
pub fn flames(cam: &Camera, fires: &[V3], t: f64, m: &Moment) -> (Vec<Square>, Vec<Square>) {
    const LICKS: usize = 10;
    let lit = night(m);
    let (mut outer, mut inner) = (vec![], vec![]);
    if lit <= 0.0 {
        return (outer, inner);
    }
    for (f, &base) in fires.iter().enumerate() {
        for k in 0..LICKS {
            // each lick its own spot over the logs, jumping to another about ten times a second
            let id = f * 97 + k * 13 + (t * 10.0 + hash(f * 31 + k, 20)).floor() as usize;
            let (jx, jz, up) = (hash(id, 21) - 0.5, hash(id, 22) - 0.5, hash(id, 23));
            let p = add(base, [0.5 * jx, 0.1 + 0.5 * up, 0.5 * jz]);
            let size = 0.3 * (1.0 - 0.6 * up);
            outer.extend(square(cam, p, size, lit));
            if up < 0.5 {
                inner.extend(square(cam, add(p, [0.0, -0.02, 0.0]), size * 0.55, lit));
            }
        }
    }
    (outer, inner)
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
