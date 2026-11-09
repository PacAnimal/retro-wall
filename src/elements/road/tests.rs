use super::*;

#[test]
fn the_car_keeps_to_the_middle_of_its_lane() {
    for i in 0..40000 {
        let lane = lane_at(7, i as f64 * 0.1);
        // its wheels, a third of the lane out from its middle, stay well within the lane (0 to 1)
        assert!((0.38..=0.62).contains(&lane), "lane {lane}");
    }
    let spread = (0..40000).map(|i| (lane_at(7, i as f64 * 0.1) - VIEW).abs()).fold(0.0, f64::max);
    assert!(spread > 0.03, "it wanders, by up to {spread}");
}

/// A stretch of road the course and the hills leave straight and level for `before` ahead of the
/// car, then bending: where the bend begins.
fn bend_after_straight(c: &mut Course, before: f64) -> f64 {
    (2..4000)
        .map(|i| i as f64 * PIECE)
        .find(|&at| {
            c.piece((at / PIECE) as usize).0 != 0.0
                && c.piece((at / PIECE) as usize - 1).0 == 0.0
                && c.piece((at / PIECE) as usize - 2).0 == 0.0
                && (0..200).all(|k| height(7, at - before - 10.0 + k as f64).0 == 0.0)
        })
        .expect("a bend after a level straight")
}

#[test]
fn a_bend_is_seen_ahead_before_the_car_reaches_it() {
    let mut c = Course::new(7);
    let start = bend_after_straight(&mut c, 20.0);
    let zc = Z_CAR;
    // the car 20 units before the bend begins: straight up to it, bending beyond it
    let a = walk(&mut c, 7, start - 20.0);
    let (_, near, _) = a.at(zc + 15.0).unwrap();
    let (_, far, _) = a.at(100.0).unwrap();
    assert!(near.abs() < 1e-9, "straight before the bend: {near}");
    assert!(far.abs() > 0.5, "the bend seen in the distance: {far}");
    assert_eq!(c.at(start - 20.0).0, 0.0, "the car not yet in it");
    // at the car the road runs straight on, whatever the bend
    for s in [start - 20.0, start + 5.0, start + 15.0] {
        let a = walk(&mut c, 7, s);
        assert!(a.at(zc).unwrap().1.abs() < 1e-4, "within a tenth of a pixel");
    }
}

#[test]
fn a_street_lamp_s_light_comes_on_the_road_gradually() {
    // at night, a few centimetres a frame: no lamp's light on the road that can be seen (from NEAR
    // on; the road shader's pool, along the road) jumps from one frame to the next
    let spacing = 18.0;
    let seen_at = |z: f32| (-(((NEAR as f32 - z).max(0.0) / 1.8).powi(2)) / 5.8).exp();
    let lamps = |travel: f64| -> std::collections::HashMap<i64, f32> {
        lamp_pools(7, spacing, 0.0, 1.0, travel, 0.0)
            .iter()
            .filter(|p| p[2] > 0.0)
            .map(|p| (((p[0] as f64 + travel) / spacing).round() as i64, p[2] * seen_at(p[0])))
            .collect()
    };
    let (mut was, mut seen) = (lamps(0.0), 0);
    for step in 1..40_000 {
        let now = lamps(step as f64 * 0.05);
        for i in was.keys().chain(now.keys()) {
            let (a, b) = (was.get(i).copied().unwrap_or(0.0), now.get(i).copied().unwrap_or(0.0));
            assert!((a - b).abs() < 0.02, "lamp {i} went from {a} to {b} at {} m", step as f64 * 0.05);
        }
        seen = seen.max(now.len());
        was = now;
    }
    assert!(seen >= 4, "the road is lit by several lamps: {seen}");
}

#[test]
fn a_crest_hides_the_road_behind_it_and_tilts_the_car() {
    let mut c = Course::new(7);
    let (at, len, _) = (1..1000).filter_map(|k| hill(7, k)).find(|h| h.2 > 3.0).expect("a high hill");
    // just before the top, climbing: the far side hidden behind the crest
    let a = walk(&mut c, 7, at + len * 0.35);
    assert!(a.steps.iter().any(|s| s.1 >= s.3), "some road hidden behind the crest");
    assert!(a.rows.iter().any(|r| matches!(r, Row::Road { .. })));
    // over the top, the car nose down ahead of the viewer still climbing behind it
    let over = walk(&mut c, 7, at + len * 0.5);
    assert!(over.pitch < -HEIGHT_STEP / 2.0, "pitch {}", over.pitch);
    assert!(view_of(over.pitch, 0.0) / ANGLES < 2, "over the top, nose down: less of its roof");
    // starting up it, nose up ahead of the viewer still on the level
    let up = walk(&mut c, 7, at + len * 0.1);
    assert!(view_of(up.pitch, 0.0) / ANGLES > 2, "starting up, nose up: more of its roof, pitch {}", up.pitch);
}

#[test]
fn hills_are_smooth_and_the_level_road_is_level() {
    for s in (0..200000).map(|i| i as f64 * 0.05) {
        let (h, slope) = height(7, s);
        let numeric = (height(7, s + 1e-4).0 - height(7, s - 1e-4).0) / 2e-4;
        assert!((slope - numeric).abs() < 1e-3, "slope {slope} against {numeric} at {s}");
        assert!(h.abs() <= 4.9);
    }
    let a = walk(&mut Course::new(7), 7, 0.0);
    assert_eq!((a.pitch, a.rise), (0.0, 0.0));
    // level: the rows under the horizon road, those over it sky
    for (r, row) in a.rows.iter().enumerate() {
        let y = ROWS_TOP + (r as f64 + 0.5) * row_height();
        if y > row_of(Z_END, 0.0) + 2.0 {
            assert!(matches!(row, Row::Road { .. }), "row {r} at {y}: {row:?}");
        } else if y < HORIZON - 2.0 {
            assert_eq!(*row, Row::Sky, "row {r} at {y}");
        }
    }
}

#[test]
fn the_car_turns_into_bends_and_drifts() {
    let d = PI / 180.0;
    assert_eq!(view_of(0.0, 0.0), 17, "level and straight: the middle height, straight on");
    assert_eq!(view_of(0.0, 0.8 * d), 17, "a bend too gentle to turn the car");
    assert_eq!(view_of(0.0, 2.2 * d), 18);
    assert_eq!(view_of(0.0, 4.1 * d), 19);
    assert_eq!(view_of(0.0, 5.5 * d), 20);
    assert_eq!(view_of(0.0, 9.0 * d), 20, "no further than the furthest");
    assert_eq!(view_of(0.0, -4.1 * d), 15);
    assert_eq!(view_of(HEIGHT_STEP, 0.0), 24, "nose up a step: the next height up");
    assert_eq!(view_of(-9.0, 0.0), 3, "no lower than the lowest");
    let mut c = Course::new(7);
    let start = bend_after_straight(&mut c, 0.0);
    let right = c.piece((start / PIECE) as usize).0 > 0.0;
    let a = walk(&mut c, 7, start + EASE + 5.0);
    assert_eq!(a.yaw.signum(), if right { 1.0 } else { -1.0 }, "turned into the bend: yaw {}", a.yaw);
    assert!(a.yaw.abs() <= 3.6 * PI / 180.0 + 1e-9, "a little only: {}", a.yaw);
}

#[test]
fn the_road_bends_both_ways_within_its_limits() {
    let mut c = Course::new(7);
    let (mut left, mut right, mut straight, mut most, mut step) = (0, 0, 0, 0.0f64, 0.0f64);
    let per_frame = 8.4 / 30.0;
    let mut last = c.at(0.0).1;
    let mut rights_after_left = false;
    for i in 1..(30 * 3600) {
        let now = c.at(i as f64 * per_frame).1;
        step = step.max((now - last).abs());
        most = most.max(now.abs());
        last = now;
    }
    for i in 1..c.pieces.len() {
        let b = c.pieces[i].0;
        if b < 0.0 {
            left += 1
        } else if b > 0.0 {
            right += 1
        } else {
            straight += 1
        }
        if i >= 3 && c.pieces[i - 2].0 < 0.0 && c.pieces[i - 1].0 > 0.0 && b > 0.0 && c.pieces[i - 1].0 != b {
            rights_after_left = true;
        }
    }
    assert!(left > 20 && right > 20 && straight > 20, "{left} left, {right} right, {straight} straight");
    assert!(most <= TURNED, "the backdrop turned {most}");
    assert!(step < 2.5, "a frame turns the backdrop at most {step}");
    assert!(rights_after_left, "two different right-hand bends after a left-hand one");
}

#[test]
fn wrapped_pictures_cover_the_canvas_seamlessly() {
    for offset in [0.0, 100.0, -2000.0, 5000.5, -1e-17, 1440.0 * 3.0 - 1e-13] {
        let pieces = wrapped(1440.0, offset);
        assert_eq!(pieces.first().unwrap().0, 0.0);
        assert_eq!(pieces.last().unwrap().1, CANVAS.0);
        for w in pieces.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        for (x0, x1, u0, u1) in pieces {
            assert!((0.0..=1.0).contains(&u0) && (0.0..=1.0 + 1e-9).contains(&u1) && ((u1 - u0) * 1440.0 - (x1 - x0)).abs() < 1e-6);
        }
    }
}

#[test]
fn fire_shows_only_briefly_at_changes_of_gear_on_hills() {
    let mut shows = 0;
    let (mut on, mut longest, mut run) = (false, 0, 0);
    for i in 0..(3600 * 30) {
        let s = i as f64 * 8.4 / 30.0;
        let fire = shift_age(7, s, 8.4).is_some();
        if fire {
            shows += 1;
            run += 1;
            longest = longest.max(run);
            assert!(height(7, s).1 != 0.0 || height(7, s - 8.4 * (FIRE * 1.25 + 0.07)).1 != 0.0, "only on hills, at {s}");
        } else {
            run = 0;
        }
        on |= fire;
    }
    assert!(on, "now and then");
    // two changes close together may run into one burst, never a lasting flame
    assert!(longest <= (2.5 * (FIRE * 1.25 + 0.07) * 30.0) as i32, "a flash, not a flame: {longest} frames");
    assert!(shows < 3600 * 30 / 10, "rare: {shows} frames in an hour");
}

#[test]
fn street_lamps_are_each_as_they_are() {
    let kinds: Vec<Lamp> = (0..20000).map(|i| lamp_kind(7, i)).collect();
    let share = |k: Lamp| kinds.iter().filter(|&&x| x == k).count() as f64 / kinds.len() as f64;
    for k in [Lamp::Broken, Lamp::Flickering, Lamp::Dim, Lamp::AlwaysOn] {
        assert!((share(k) - 0.05).abs() < 0.01, "{k:?}: {}", share(k));
    }
    for i in 0..2000 {
        let (night, noon) = (lamp_level(7, i, 0.0, 1.0), lamp_level(7, i, 1.0, 1.0));
        match lamp_kind(7, i) {
            Lamp::Broken => assert_eq!((night, noon), (0.0, 0.0)),
            Lamp::AlwaysOn => assert_eq!((night, noon), (1.0, 1.0)),
            Lamp::Dim => assert_eq!((night, noon), (0.4, 0.0)),
            Lamp::Normal => assert_eq!((night, noon), (1.0, 0.0)),
            Lamp::Flickering => assert_eq!(noon, 0.0),
        }
    }
    // a flickering lamp is mostly on, and off now and then
    let i = (0..).find(|&i| lamp_kind(7, i) == Lamp::Flickering).unwrap();
    let lit = (0..3000).filter(|&k| lamp_level(7, i, 0.0, k as f64 / 30.0) > 0.5).count();
    assert!((2000..2990).contains(&lit), "{lit} of 3000 frames lit");
}

#[test]
fn the_car_bumps_only_a_little() {
    let most = (0..100000).map(|i| bump_at(7, i as f64 * 0.01).abs()).fold(0.0, f64::max);
    assert!(most < 3.5 && most > 0.5, "bumps up to {most} canvas pixels");
}

#[test]
fn the_camera_adds_up() {
    // the car seen from 6 degrees above, the height its middle view is drawn from
    assert!(((1.0 / Z_CAR).atan().to_degrees() - 6.0).abs() < 0.1);
    // its rear wheels near the canvas's foot, under two thirds of its lane wide
    let foot = row_of(Z_CAR, 0.0);
    assert!((760.0..860.0).contains(&foot), "foot at {foot}");
    assert!((CAR_WIDTH / scale(Z_CAR) - 0.65).abs() < 1e-9);
    // the horizon well below the middle of the screen
    assert!(HORIZON / CANVAS.1 > 0.6);
    // the viewer's line runs straight up to the vanishing point
    for z in [3.0, 10.0, 100.0] {
        assert_eq!(road_x(z, VIEW), VANISH);
    }
}

#[test]
fn pixel_art_is_pixels() {
    let big = surface(200, 300);
    {
        let c = ctx(&big);
        c.set_source_rgb(0.2, 0.6, 0.3);
        c.arc(100.0, 150.0, 80.0, 0.0, 6.3);
        let _ = c.fill();
    }
    let mut art = pixel_art(&big, 40, 60, 6.0, true, false);
    assert_eq!((art.width(), art.height()), (40, 60));
    let d = pixels(&mut art);
    // every pixel wholly there or not: no soft edges
    assert!(d.chunks(4).all(|p| p[3] == 0 || p[3] == 255));
    // and many shades within: lit edges, leaves, an outline
    let mut shades: Vec<[u8; 3]> = d.chunks(4).filter(|p| p[3] == 255).map(|p| [p[0], p[1], p[2]]).collect();
    shades.sort();
    shades.dedup();
    assert!(shades.len() >= 5, "{} shades", shades.len());
}
