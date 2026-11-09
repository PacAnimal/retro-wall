//! The lights in the towers' windows: when each comes on and goes off, and the windows drawn lit.

use super::*;

// --- the lights ------------------------------------------------------------------------------------

/// Windows still fading, in draw order (lower index underneath where reflections overlap).
#[derive(Default)]
pub struct Fading(std::collections::BTreeSet<usize>);

impl Fading {
    pub(super) fn new() -> Self {
        Fading::default()
    }
    pub(super) fn add(&mut self, k: usize) {
        self.0.insert(k);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct When(f64, usize);
impl Eq for When {}
impl PartialOrd for When {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for When {
    // Reversed so BinaryHeap pops the earliest (when, window) first.
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Which of a skyline's windows are lit at clock c.
pub struct Lights {
    pub(super) period: Vec<f64>,
    pub(super) phase: Vec<f64>,
    pub(super) level: Vec<f64>,
    pub(super) dims: Vec<bool>,
    pub(super) storeys: Vec<Vec<usize>>,
    pub(super) floor: Vec<usize>,
    pub(super) houses: Vec<Vec<usize>>,
    pub(super) house: Vec<usize>,
    pub(super) wanted: Vec<bool>,
    pub(super) seed: i64,
    pub(super) c: Option<f64>,
    pub(super) version: u64,
    pub(super) lit: Vec<bool>,
    pub(super) own: Vec<(bool, bool)>,
    pub(super) changed: Vec<f64>,
    pub(super) dimmed: f64,
    pub(super) heap: BinaryHeap<When>,
    pub(super) log: Vec<(u64, usize)>,
    pub(super) since_full: u64,
}

pub(super) const DIMMING: f64 = 0.35;
pub(super) const DIM: f64 = 1.0;
pub(super) const FILL: f64 = 0.75;
pub(super) const MOST: f64 = 0.8;

/// A window of the windows' list: (x0, y0, x1, y1, colour, (row, building), floor, shown, mirrored).
#[derive(Clone)]
pub struct Win {
    pub(super) x0: i64,
    pub(super) y0: i64,
    pub(super) y1: i64,
    pub(super) colour: Rgb,
    pub(super) building: (usize, usize),
    pub(super) storey: i64,
    pub(super) mirrored: Option<(i64, i64)>,
}

impl Lights {
    pub(super) fn new(windows: &[Win], lit: f64, period: (f64, f64), seed: i64) -> Lights {
        let n = windows.len();
        let mut rng = Rng::new(seed);
        let periods: Vec<f64> = (0..n).map(|_| rng.range(period.0, period.1)).collect();
        let phase: Vec<f64> = (0..n).map(|_| rng.f64()).collect();
        let mut levels: HashMap<(usize, usize), f64> = HashMap::new();
        let mut dimming: HashMap<(usize, usize), bool> = HashMap::new();
        let mut floors: HashMap<(usize, usize, i64), f64> = HashMap::new();
        let (mut level, mut dims) = (vec![], vec![]);
        let mut storeys: Vec<((usize, usize, i64), Vec<usize>)> = vec![];
        let mut storey_at: HashMap<(usize, usize, i64), usize> = HashMap::new();
        let mut houses: Vec<Vec<usize>> = vec![];
        let mut house_at: HashMap<(usize, usize), usize> = HashMap::new();
        for (i, w) in windows.iter().enumerate() {
            let building = w.building;
            let floor = (building.0, building.1, w.storey);
            let si = *storey_at.entry(floor).or_insert_with(|| {
                storeys.push((floor, vec![]));
                storeys.len() - 1
            });
            storeys[si].1.push(i);
            let hi = *house_at.entry(building).or_insert_with(|| {
                houses.push(vec![]);
                houses.len() - 1
            });
            houses[hi].push(i);
            if let std::collections::hash_map::Entry::Vacant(e) = levels.entry(building) {
                e.insert(lit * rng.range(0.2, 1.8));
                dimming.insert(building, rng.f64() < DIMMING);
            }
            floors.entry(floor).or_insert_with(|| *rng.pick(&[0.15, 0.5, 1.0, 1.0, 1.4, 2.0]));
            dims.push(dimming[&building]);
            #[allow(clippy::manual_clamp)] // max then min: a NaN is 0, where clamp would keep it
            level.push((levels[&building] * floors[&floor]).max(0.0).min(0.95));
        }
        let storeys: Vec<Vec<usize>> = storeys
            .into_iter()
            .map(|(_, mut ws)| {
                ws.sort_by_key(|&i| windows[i].x0);
                ws
            })
            .collect();
        let mut floor = vec![0; n];
        for (k, ws) in storeys.iter().enumerate() {
            for &i in ws {
                floor[i] = k;
            }
        }
        let houses: Vec<Vec<usize>> = houses
            .into_iter()
            .map(|mut ws| {
                ws.sort_by(|&a, &b| chance(&[seed + 2, a as i64]).total_cmp(&chance(&[seed + 2, b as i64])));
                ws
            })
            .collect();
        let mut house = vec![0; n];
        for (b, ws) in houses.iter().enumerate() {
            for &i in ws {
                house[i] = b;
            }
        }
        Lights {
            period: periods,
            phase,
            level,
            dims,
            storeys,
            floor,
            houses,
            house,
            wanted: vec![false; n],
            seed,
            c: None,
            version: 0,
            lit: vec![false; n],
            own: vec![(false, false); n],
            changed: vec![f64::NEG_INFINITY; n],
            dimmed: f64::NEG_INFINITY,
            heap: BinaryHeap::new(),
            log: vec![],
            since_full: 0,
        }
    }

    pub(super) fn on(&self, i: usize, c: f64) -> (bool, bool) {
        let count = (c / self.period[i] + self.phase[i]).floor() as i64;
        (chance(&[self.seed, i as i64, count]) < self.level[i], chance(&[self.seed + 1, i as i64, count]) < FILL)
    }

    /// Storey k's windows wanted lit: their own lights, and dark ones filled in between lit ones.
    /// Whether any changed.
    pub(super) fn fill(&mut self, k: usize) -> bool {
        let ws = &self.storeys[k];
        let dark = ws.iter().filter(|&&i| !self.own[i].0).count();
        let mut moved = false;
        for (j, &i) in ws.iter().enumerate() {
            let (mut lit, fill) = self.own[i];
            if !lit && fill {
                lit = (ws.len() >= 3 && dark == 1) || (0 < j && j < ws.len() - 1 && self.own[ws[j - 1]].0 && self.own[ws[j + 1]].0);
            }
            moved |= lit != self.wanted[i];
            self.wanted[i] = lit;
        }
        moved
    }

    /// Building b's windows lit: those wanted, up to MOST of them. At a clock `when`, changes are logged.
    pub(super) fn cap(&mut self, b: usize, when: Option<f64>) {
        let ws = &self.houses[b];
        let mut room = (MOST * ws.len() as f64).floor() as i64;
        for &i in ws {
            let lit = self.wanted[i] && room > 0;
            room -= lit as i64;
            if let Some(when) = when.filter(|_| lit != self.lit[i]) {
                self.version += 1;
                self.log.push((self.version, i));
                if self.dims[i] {
                    self.changed[i] = when;
                    self.dimmed = self.dimmed.max(when);
                }
            }
            self.lit[i] = lit;
        }
    }

    pub(super) fn brightness(&self, i: usize, c: f64) -> f64 {
        if !self.dims[i] {
            return if self.lit[i] { 1.0 } else { 0.0 };
        }
        let p = ((c - self.changed[i]) / DIM).min(1.0);
        if self.lit[i] { p } else { 1.0 - p }
    }

    pub(super) fn dimming(&self, c: f64) -> bool {
        c - self.dimmed < DIM
    }

    pub(super) fn at(&mut self, c: f64) -> u64 {
        let n = self.lit.len();
        if self.c.is_none() || c < self.c.unwrap() {
            for i in 0..n {
                self.own[i] = self.on(i, c);
            }
            for k in 0..self.storeys.len() {
                self.fill(k);
            }
            for b in 0..self.houses.len() {
                self.cap(b, None);
            }
            self.changed.fill(f64::NEG_INFINITY);
            self.dimmed = f64::NEG_INFINITY;
            self.heap.clear();
            for i in 0..n {
                let (p, f) = (self.period[i], self.phase[i]);
                self.heap.push(When((((c / p + f).floor() + 1.0) - f) * p, i));
            }
            self.version += 1;
            self.log.clear();
            self.since_full = self.version;
        } else {
            while let Some(&When(when, i)) = self.heap.peek() {
                if when > c {
                    break;
                }
                self.heap.pop();
                let own = self.on(i, when + 1e-6);
                if own != self.own[i] {
                    self.own[i] = own;
                    if self.fill(self.floor[i]) {
                        self.cap(self.house[i], Some(when));
                    }
                }
                self.heap.push(When(when + self.period[i], i));
            }
            if self.log.len() > 4096 {
                let cut = self.log.len() - 2048;
                self.since_full = self.log[cut].0 - 1;
                self.log.drain(..cut);
            }
        }
        self.c = Some(c);
        self.version
    }

    /// The changes (version, window) after `version`, or None when they are no longer all logged.
    pub(super) fn since(&self, version: Option<u64>) -> Option<&[(u64, usize)]> {
        let v = version?;
        if v < self.since_full {
            return None;
        }
        let from = self.log.partition_point(|e| e.0 <= v);
        Some(&self.log[from..])
    }
}

/// Vertex array of only the lit windows' rectangles, updated as lights change.
pub struct Lit {
    pub(super) built: Option<u64>,
    pub(super) premultiplied: bool,
    pub(super) quads: Vec<Option<[f32; 32]>>,
    pub(super) data: Vec<f32>,
    pub(super) at: HashMap<usize, usize>,
    pub(super) order: Vec<usize>,
    pub(super) fading: Fading,
}

impl Lit {
    pub(super) fn new(quads: &[Option<[f64; 32]>], premultiplied: bool) -> Lit {
        let quads = quads.iter().map(|q| q.map(|q| q.map(|v| v as f32))).collect();
        Lit { built: None, premultiplied, quads, data: vec![], at: HashMap::new(), order: vec![], fading: Fading::new() }
    }

    pub(super) fn put(&mut self, i: usize, level: f64) {
        if level > 0.0 && !self.at.contains_key(&i) {
            self.at.insert(i, self.order.len());
            self.order.push(i);
            self.data.extend_from_slice(&self.quads[i].unwrap());
        } else if level <= 0.0 && self.at.contains_key(&i) {
            let place = self.at.remove(&i).unwrap();
            let last = self.order.pop().unwrap();
            let n = self.data.len();
            if last != i {
                self.data.copy_within(n - 32.., place * 32);
                self.order[place] = last;
                self.at.insert(last, place);
            }
            self.data.truncate(n - 32);
            return;
        }
        if level > 0.0 {
            let q = self.quads[i].unwrap();
            let k = self.at[&i] * 32;
            for corner in (0..32).step_by(8) {
                if self.premultiplied {
                    for j in 2..6 {
                        self.data[k + corner + j] = (q[corner + j] as f64 * level) as f32;
                    }
                } else {
                    self.data[k + corner + 5] = (q[corner + 5] as f64 * level) as f32;
                }
            }
        }
    }

    /// Window i's light changed.
    pub(super) fn changed(&mut self, lights: &Lights, i: usize) {
        if self.quads.get(i).is_some_and(|q| q.is_some()) {
            if lights.dims[i] {
                self.fading.add(i);
            } else {
                self.put(i, if lights.lit[i] { 1.0 } else { 0.0 });
            }
        }
    }

    pub(super) fn current(&mut self, lights: &Lights, c: f64) -> &[f32] {
        match lights.since(self.built) {
            None => (0..self.quads.len()).for_each(|i| self.changed(lights, i)),
            Some(log) => log.iter().for_each(|&(_, i)| self.changed(lights, i)),
        }
        let mut fading = std::mem::take(&mut self.fading);
        fading.0.retain(|&i| {
            let level = lights.brightness(i, c);
            self.put(i, level);
            level != 0.0 && level != 1.0
        });
        self.fading = fading;
        self.built = Some(lights.version);
        &self.data
    }
}

/// A Windows element's windows, built once and shared with the reflections.
pub struct Bundle {
    pub(super) windows: Vec<Win>,
    pub(super) quads: Vec<Option<[f64; 32]>>,
    pub(super) lights: RefCell<Lights>,
}

/// For each column, the top of the highest building of `skylines` standing in it (or `floor`).
pub(super) fn heights(skylines: &[&SkylineData], width: i64, floor: i64) -> Vec<i64> {
    let mut top = vec![floor; width as usize];
    for data in skylines {
        for b in &data.buildings {
            for &(a, bb, t) in &b.tiers {
                for x in a.max(0)..bb.min(width) {
                    if t < top[x as usize] {
                        top[x as usize] = t;
                    }
                }
            }
        }
    }
    top
}

/// The widest run of columns x0..x1 at height y not hidden by nearer buildings.
pub(super) fn shown(x0: i64, x1: i64, y: i64, top: &[i64]) -> Option<(i64, i64)> {
    let len = top.len() as i64;
    let (mut best, mut start): (Option<(i64, i64)>, Option<i64>) = (None, None);
    for x in x0.max(0)..len.min(x1) + 1 {
        if x < len.min(x1) && top[x as usize] > y {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(s) = start {
            if best.is_none_or(|b| x - s > b.1 - b.0) {
                best = Some((s, x));
            }
            start = None;
        }
    }
    best
}

/// Windows, rectangles and lights of the Windows element `name`, built on first use, then shared.
pub(super) fn bundle(name: &str, g: &Geo, s: &mut Shared) -> Option<Rc<Bundle>> {
    if let Some(b) = s.get_named::<Rc<Bundle>>(name) {
        return Some(b.clone());
    }
    let e = g.th.scene.iter().find(|e| e.name == name && e.kind == "windows")?;
    let st: WindowsSettings = settings(&e.settings);
    let colours: Vec<Rgb> = st.colours.iter().map(|c| c.rgb()).collect();
    let alpha0 = st.alpha;
    let layers: Vec<&SkylineData> = st.skylines.iter().filter_map(|n| s.get_named::<SkylineData>(n)).collect();
    let (w, floor) = (g.w as i64, g.th.horizon_y(g.h).ceil() as i64 + 1);
    let mut rng = Rng::new(layers.iter().map(|d| d.windows.len() as i64).sum());
    let weights: Vec<f64> = (0..colours.len()).map(|k| 0.5f64.powi(k as i32)).collect();
    let (mut windows, mut quads) = (vec![], vec![]);
    let mut lamps: HashMap<(usize, usize, i64), Rgb> = HashMap::new();
    for (k, data) in layers.iter().enumerate() {
        let top = heights(&layers[k + 1..], w, floor);
        let alpha = alpha0 * (1.0 - 0.45 * data.haze);
        for &(x0, y0, x1, y1, b, storey, building) in &data.windows {
            let sk = (k, building, storey);
            lamps.entry(sk).or_insert_with(|| colours[rng.weighted(&weights)]);
            let colour = if rng.f64() < 0.88 { lamps[&sk] } else { colours[rng.weighted(&weights)] };
            let whole = shown(x0, x1, y1 - 1, &top) == Some((x0, x1));
            let mirrored = shown(x0, x1, y0, &top) == Some((x0, x1));
            windows.push(Win { x0, y0, y1, colour, building: (k, building), storey, mirrored: mirrored.then_some((x0, x1)) });
            quads.push(whole.then(|| fill_quad(x0 as f64, y0 as f64, x1 as f64, y1 as f64, colour, alpha * b)));
        }
    }
    let seed: i64 = name.chars().map(|c| c as i64).sum();
    let lights = Lights::new(&windows, st.lit, (st.period[0], st.period[1]), seed);
    let b = Rc::new(Bundle { windows, quads, lights: RefCell::new(lights) });
    s.put_named(name, b.clone());
    Some(b)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowsSettings {
    pub(super) skylines: Vec<String>,
    pub(super) lit: f64,
    pub(super) period: [f64; 2],
    pub(super) colours: Vec<Colour>,
    pub(super) alpha: f64,
}

impl Validate for WindowsSettings {
    fn validate(&self) -> Result<(), String> {
        within("lit", self.lit, 0.0, 1.0)?;
        positive_pair("period", self.period)?;
        each("period", &self.period, 1.0, 86400.0)?;
        if self.colours.is_empty() {
            return Err("colours is empty: it needs at least one".into());
        }
        within("alpha", self.alpha, 0.0, 1.0)
    }
}

/// The skylines' windows lighting up and going dark (live), in one draw call.
pub struct Windows {
    pub(super) name: String,
    pub(super) bundle: Option<Rc<Bundle>>,
    pub(super) glass: Option<Lit>,
}

impl Part for Windows {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.bundle = bundle(&self.name, g, s);
    }
    fn top(&self, g: &Geo) -> i32 {
        self.bundle.as_ref().and_then(|b| b.windows.iter().map(|w| w.y0).min()).map_or(g.h, |y| y as i32)
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let b = self.bundle.as_ref().unwrap();
        let mut l = b.lights.borrow_mut();
        let c = w.clock;
        let mut h = 0xcbf2_9ce4_8422_2325;
        mix(&mut h, l.at(c));
        opt(&mut h, l.dimming(c).then(|| (c * FPS).round() as i64));
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, w: &World, _p: f64) {
        let b = self.bundle.clone().unwrap();
        let glass = self.glass.get_or_insert_with(|| Lit::new(&b.quads, false));
        let data = glass.current(&b.lights.borrow(), w.clock);
        gfx.draw(None, data, Blend::Alpha);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let Some(b) = bundle(&self.name, g, s) else { return };
        let mut l = b.lights.borrow_mut();
        l.at(0.0);
        for (q, &on) in b.quads.iter().zip(l.lit.iter()) {
            if let (true, Some(q)) = (on, q) {
                paint_fills(c, q);
            }
        }
        l.c = None;
    }
}
