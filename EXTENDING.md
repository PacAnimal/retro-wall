# Extending retro-wall

Three things can be added, none of them needing retro-wall to be rebuilt:

| To add | Write | Where |
|---|---|---|
| **A measurement** (what an instrument reads) | a program that prints a number | `~/.config/retro/sources/NAME` |
| **An instrument** (how a reading is drawn) | a program that answers with drawing operations | `~/.config/retro/faces/NAME` |
| **A theme** (the whole picture) | a `theme.toml`, pictures, optionally shaders | `~/.config/retro/themes/NAME/` |

A new **kind of element** (a part of a scene that draws or moves in a way no kind does yet, 2D or 3D) is
Rust, built into retro-wall: see "In code: a new kind of element" at the end.

Each starts from a working example: `retro-wall --new source NAME`, `--new face NAME`, `--new theme NAME
[EXTENDS]` write one, commented, into that place. `examples/` has more to copy: a `retro.toml` with the
instruments moved and plugins added, sources (`clock`, `gpu-temp`, `headlines`), a face (`level`) and a
theme with a shader (`teal`). `retro-wall --list` shows what there is to use.

Plugins are programs: any language, run with your rights. Read one before you install it. Each runs in
a process group of its own (on Windows a job object, with no console window): when retro-wall stops
it, what it started stops too.
On Windows the place is `%APPDATA%\retro`, a plugin is an `.exe`, `.cmd` or `.bat` (its name without the
extension), and `--new` writes a `.cmd` that runs a PowerShell script beside it; a shell line in
`retro.toml` runs through `cmd /C`.

## A measurement

A source is any executable in `~/.config/retro/sources/`. Its file name is its name. retro-wall runs it
every 2 seconds; it prints its reading, 0 to 100, and exits:

```sh
#!/bin/sh
# ~/.config/retro/sources/load: the load average, per core, as a percentage
awk -v n="$(nproc)" '{ printf "%.0f\n", 100 * $1 / n }' /proc/loadavg
```

It may print JSON instead: `{"value": 87, "indicator": true}` lights the meter's lamp (the battery's
bolt); `{"text": "..."}` is for widgets that show text. A program that keeps running prints
`{"stream": true}` first, then a line whenever it has something new. A program with arguments, or
another pace, is a table in `~/.config/retro/retro.toml`:

```toml
[source.gpu]
command = ["nvidia-smi", "--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]
every = 1.0               # seconds, 0.1 to 86400
```

A source that fails, hangs or prints nothing keeps its last value, shown dimmed, and is tried again
(after 10 seconds, then less often); what went wrong is said once. It never slows the picture: each
runs on its own. While the wall is paused (on battery, the screen off), sources are not run, and a
streaming one is stopped where it is.

To show it, name it as a widget's source in `retro.toml`:

```toml
[[group]]
anchor = "top-right"
widgets = ["load"]

[widget.load]
type = "meter"            # meter, ring, bar, spark, cells, or a face of your own
source = "load"
label = "LOAD"
```

The built-in sources: `cpu`, `ram`, `swap`, `bat`, `disk`, `used` and `free` (of `/`, or of a path:
`used:/home`), and the network's traffic, `down` and `up` (on a log scale: 0 at a kilobit a second, 100
at a gigabit).

## An instrument

A face is any executable in `~/.config/retro/faces/`. Its file name is a widget type. It keeps
running: for each picture retro-wall needs, which is only when what the widget shows changes, retro-wall
writes it a line of JSON, and it answers with a line: a list of drawing operations.

It is asked:

```json
{"protocol": 1, "width": 172, "height": 117, "scale": 0.57, "value": 46.0, "indicator": false, "text": null,
 "stale": false, "settings": {"label": "CPU", "source": "cpu"}, "palette": {...},
 "colours": {"needle": "#ff4fb0", "lines": "#4fb6d6", "caption": "#4fb6d6", "fill": "#05040b"}}
```

and answers, for example:

```json
[{"op": "rect", "x": 8, "y": 8, "w": 156, "h": 90}, {"op": "stroke", "colour": "#4fb6d6"},
 {"op": "glow"}, {"op": "text", "text": "46%", "x": 164, "y": 110, "align": "right", "bold": true}]
```

| Operation | Takes |
|---|---|
| `rect` | `x`, `y`, `w`, `h` |
| `circle` | `x`, `y`, `r` |
| `arc` | `x`, `y`, `r`, `from`, `to` (degrees, clockwise from 3 o'clock) |
| `move`, `line`, `close` | `x`, `y` |
| `fill`, `stroke` | `colour` (`#rrggbb` or `#rrggbbaa`), `width` (a stroke's) |
| `text` | `text`, `x`, `y` (the baseline), `size`, `colour`, `bold`, `align` (`left`, `centre`, `right`) |
| `glow` | a halo round everything drawn so far, as the built-in instruments' lines have |
| `save`, `restore` | the drawing state |

`colours` and `palette` are the theme's, so a face looks right in every theme. `settings` is the
widget's own table: anything you put there reaches the face. Its size in the layout is `size =
[columns, rows]` in cards (the default is one card). A face that fails or does not answer in two seconds
shows ERR in its place, and is started again later. `--snapshot` waits for each face's first answer,
or its two seconds, before it draws.

`protocol` is the plugin protocol (below, "What stays stable"); every plugin also has it in its
environment, `RETRO_PROTOCOL`. A face skips what it does not know: a field or an operation can be added
without a new protocol.

A face in sh can ignore its requests and print the same drawing each time (what `--new face` writes),
or pick the numbers it needs out of the line (`examples/faces/level`); one that reads all of it is best
written in a language with a JSON parser.

## Changing a theme without making one

Every theme is made of elements, each with settings, and of tweaks. `retro.toml` can set any of them,
over whichever theme is chosen, so hiding, moving or changing a part of a theme needs no theme of your
own:

```toml
[element.block-world]     # blocky's world: its interface off, the hand on
ui = false
hand = true

[tweaks]
title_corner = "bottom-left"
vignette = 0.0
```

`retro-wall --settings NAME` lists a theme's elements, every setting each has, and every tweak. Each is
checked as a theme's would be: a setting that does not check, an element the theme's scene does not
have, or an element's `type`, is said and left out, and the rest is kept; where your settings break an
element, it keeps the theme's own. A tweak set here holds in the theme's day and night looks too.

## A theme

A theme is a directory, `~/.config/retro/themes/NAME/`, with a `theme.toml`. It starts from another
theme (`extends`), and says only what it changes:

```toml
name = "Teal"
extends = "phosphor"

[palette]
hue = 190                 # every colour turned about the colour wheel, keeping its light and shade
```

That is a whole theme. From there:

- **The palette** (`[palette]`): each colour by its role (`base`, the sky; `ramp`, six from dark to
  bright; `caption`, the instruments' lettering; ...), or `hue` and `chroma` to turn them all.
- **The tweaks** (`[tweaks]`): the core's shapes and sizes: where the hostname stands (`title_corner`),
  the vignette, the scanlines, the instruments' frames (`card_frame`). `retro-wall --settings NAME`
  lists every one, as the theme has it. Over a busy picture the network's status line can stand on
  frosted glass, `status_glass = [pad, round, blur, tint, edge, "#tint", "#edge"]` (a rounded box `pad`
  capital heights round the letters, its corners `round` of its height, 0.5 a pill, over the picture
  behind it blurred by `blur` pixels, tinted and edged; a pad of 0 draws none), and in the bottom right
  corner with `status_corner = true`, its outer edge as far in from the bottom and the right as the
  hostname stands from its own edges, within the desktop's panels.
- **The scene** (`scene = [...]`, bottom to top) and **its elements** (`[element.NAME]`, with `type =
  "KIND"` and its settings). `retro-wall --settings NAME` lists every setting of every element.
- **The instruments**: `[[group]]` and `[widget.NAME]`, as in `retro.toml` (which wins over a theme's).
- **Several monitors** (`[monitors]`): with `--all-monitors`, each piece of the header goes where its
  key says: `title` (the hostname), `widgets` (the instruments), `graph` (the network's traffic) and
  `status` (the network's status line; without a key, where the widgets are). Each takes
  `"rightmost"` (the default), `"leftmost"`, `"primary"`, `"all"`, `"none"`, or a list of monitors'
  names. The scenery, readings-driven parts included, is on every monitor. A `retro.toml`'s
  `[monitors]` wins over the theme's, key by key. One monitor driven alone shows every piece that is
  not `"none"`.

  ```toml
  [monitors]                  # the hostname on every monitor, the status line on the left, the rest on the right
  title = "all"
  status = "leftmost"
  ```
- **Day and night**: `[day.palette]`, `[night.tweaks]` and so on: the theme follows the time of day.

A theme of **your own picture** is a painting: put the picture beside `theme.toml` and name it:

```toml
name = "Harbour"
extends = "phosphor"
scene = ["picture", "mountains", "title", "instruments", "finish"]

[tweaks]
canvas = [3840.0, 2160.0]   # the picture's size: it covers the screen, cropped about crop_centre
title_corner = "bottom-left"

[element.picture]
type = "painting"
image = "harbour.png"
```

Patches of the picture can light and dim (`type = "lights"`), a lamp can flicker (`"lamp"`), a screen can
show pictures (`"screen"`), rain can fall (`"rain"`), drops can run down a window (`"glass-drops"` and
`"glass-frame"`): the neon_rain theme is all of them, and only TOML.

A painting can also **run on the readings**: parts laid over it that turn, fill, point, glow and steam,
each following a source. The copperworks theme is all of them, and only TOML. They are placed in the
pixels of `canvas_source` (scaled by `canvas_scale` onto the painting, as the patches are), and each
picture is scaled once for the screen. Every part's `source` is a source's name, or `""` for its own
idle motion; `"bat|swap"` takes the first the machine has (here the battery, or swap where there is
none). A source that fails eases back to the idle motion, and returns when it reads again.

| Kind | Draws | Its main settings |
|---|---|---|
| `rotors` | sprites turning about their pivots | `rotors = [{ image, at, size, speed, phase, pivot }]` (degrees a second, clockwise); `source`: at 100 they turn `load` times as fast, eased over `ease` seconds. The angle adds up the speed: a change never jumps |
| `tank` | a liquid filling a window from its foot, exactly empty at 0 and full at 1 | `mask` (its alpha is the window), `liquid`, `glass` and `frame` over it, `at`, `size`; `source`, or `mean` + `amplitude` sin(2 pi t / `period` + `phase`); `overscan` and `drift` (the liquid's picture moves, never past its edge), `ripple` (canvas pixels, none at empty or full), `meniscus`, `bubbles` (rising, cut where the liquid ends), `cavity` (the empty window) |
| `gauges` | a still dial and a needle about its hub | `dial`, `needle`, `needle_size`; `sweep = [from, to]` (degrees clockwise from up, at 0 and 1); `gauges = [{ at, size, phase, source }]`; with no reading `mean` + `swing` sin(`rate` t + phase); a `wobble` on top |
| `pilot-lamps` | a dark lamp, its lit lens at its brightness, a soft halo | `off`, `lens`, `halo` (colour), `halo_size`, `halo_strength`, `halo_blend` (`"screen"` or `"add"`); each of `lamps = [{ at, size, period, phase }]` breathes from `low` to full; `source` flickers them by up to `flicker`; `on = false` puts them out |
| `steam` | vents, and puffs rising from them | `vent` (or `""`), `puffs` (pictures, taken in turn), `particles`, `period`; a puff's opacity is sin(pi age)^2 x `opacity` x strength, clear at both ends of its life; `vents = [{ at, size, plume, intensity, phase, source }]`: the strength is `intensity`, from `rest` of it at 0 to all of it at 100 |

`at` is a picture's centre (a rotor's pivot), and `size` its width and height. A part in front of the
moving ones, still, is a `painting` with `image = ""` and its picture in `steady`.

**Motion of your own** is a fragment shader, drawn on the GPU every frame, costing nothing on the CPU:

```toml
[element.fog]
type = "shader"
fragment = "shaders/fog.frag"     # beside theme.toml
area = [0.0, 0.6, 1.0, 1.0]       # left, top, right, bottom, of the screen
readings = ["cpu", "disk"]        # readings.x and readings.y, 0 to 1
textures = ["noise.png"]          # tex0, repeating
fps = 30.0
```

```glsl
// given: time (the scene's seconds), resolution, area, readings, tex0..tex3, uv (0 to 1 over the area)
void main() {
    float n = texture2D(tex0, uv * 3.0 + vec2(time * 0.01, 0.0)).r;
    float a = n * 0.3 * (1.0 - uv.y) * (0.5 + readings.x);
    gl_FragColor = vec4(vec3(0.8, 0.9, 1.0) * a, a);   // premultiplied
}
```

What a theme names (pictures, SVGs, shaders, lettering, the themes it extends) is read only from a
regular file, and only up to a size for its kind (a shader 1 MB, a theme.toml 4 MB, an SVG 64 MB, a PNG
256 MB): a device, a FIFO or a larger file is said and left out.

Put its name in `scene` where it belongs: over what it covers, under what covers it. A shader that does
not compile is said, with the compiler's message, and left out.
Names that start with `retro_` are retro-wall's own: give your variables any other name.

## Sharing your theme

A theme made for everyone is welcome as a pull request. It holds:

- the theme's directory, `themes/NAME/`, with its `theme.toml` and only the pictures it uses;
- art that is your own, or that you have the right to share under this project's license (PNGs
  without text chunks or other metadata);
- a screenshot, `retro-wall --snapshot NAME.png --demo 13:37 --theme NAME`, for the README;
- the tests passing (`cargo test --release`), with the theme added to the bundled-theme tests and its
  reference picture made (`RETRO_BLESS=1`, the other pictures left as they are).

## Testing what you made

```sh
retro-wall --preview /tmp/out.png 1920x1080 --theme NAME     # the still picture
retro-wall --snapshot /tmp/out.png --theme NAME              # the live picture, one frame (fixed readings)
retro-wall --theme NAME                                      # live, here and now
```

A theme, a source or a face that has something wrong with it is said, with the reason, and left out:
a broken theme draws the default instead (the one installed, even where a theme of yours has its name),
and a broken widget shows ERR. `retro.toml` is checked item by item: a widget, a group, a source or a
setting that does not check is said and left out, and the rest is kept; where your settings break one
of the theme's widgets, it keeps the theme's own. Nothing you add can stop the wall.

## What stays stable

What you write against retro-wall keeps working across its releases. Each of these changes only with a
new number, said in the release notes:

| Interface | Its number | Within a number |
|---|---|---|
| A theme: its tables and keys, the palette's roles, the tweaks, the element kinds and their settings | `format = 1` in `theme.toml` (none is 1) | Keys, roles, tweaks, kinds and settings can be added. None is renamed or removed, and none is read in another way |
| `retro.toml`: its tables and keys | The theme format: a new format changes both | The same as a theme |
| Sources and faces: what a source prints, a face's request and its operations | `"protocol": 1` in a face's request; `RETRO_PROTOCOL` in every plugin's environment | Fields and operations can be added. A plugin skips what it does not know |
| The command line and the paths (`~/.config/retro/...`) | The version, `retro-wall --version` | Options can be added; a removed one is said for a release first |

A theme of a newer format than the retro-wall that reads it is said ("written for a newer retro-wall")
and the default is drawn in its place. The Rust inside retro-wall (`src/`) has no such promise: a kind
of element of your own is built with retro-wall, and changes with it.

## In code: a new kind of element

A theme is made of elements; each element is of a kind (`type = "KIND"`), and a kind is Rust in
`src/elements/`. A new kind is one file and one line, and every theme can then use it. Build, check and
test as CI does: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (the GL
tests need an X display; under Xvfb, `RETRO_REQUIRE_XVFB=1` makes their absence a failure).

### 1. The file and its registration

`src/elements/NAME.rs`, and in `src/elements/mod.rs` a `pub mod NAME;` and, in `MODULES`,
`module(NAME::kinds)`. `retro-wall --list` and `--settings` then show it, and themes can name it. A
module that keeps something only while a scene is built (a cache of pictures its parts share) lets it go
in a `built` function of its own: `Module { kinds: NAME::kinds, built: Some(NAME::built) }`.

A kind's name says what it draws (`northern-lights`, `road`), not the theme it was made for: a theme's
name is for `--theme` and `extends`, and a test keeps the two apart.

```rust
pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "pulse",                       // what a theme's `type =` says
        defaults: r##"colour = "#ff4fb0"
source = "cpu"
size = 0.03
rate = [0.3, 3.0]
places = [[0.25, 0.3], [0.5, 0.3]]"##,
        varies: &["places"],                 // lists that may be of any length
        check: check::<PulseSettings>,       // the settings parse and validate
        make: |_name, t, _g| Ok(vec![Box::new(Pulse::new(t))]),
    }]
}
```

### 2. Its settings

`defaults` is TOML, and it is the schema: every setting a theme (or `retro.toml`'s `[element.NAME]`)
gives is checked against its default before any element is made.

- A setting's type is its default's: a number, a string, a boolean, a list, a table.
- A string default that starts with `#` is a colour: `#rrggbb` or `#rrggbbaa`, read as `tweaks::Colour`.
- A list keeps its default's length, unless it is named in `varies` (or its default is empty).
- An unknown setting is an error that names the ones there are.

Then the kind's own check: a `#[derive(Deserialize)] #[serde(deny_unknown_fields)]` struct of the same
fields, with `impl Validate` for ranges and relations (`within`, `each`, `positive` in
`elements/mod.rs`). `make` reads it with `settings::<T>(t)`: it was checked when the theme loaded, so it
cannot fail there. The struct and the defaults must name the same fields: a test that loads a theme
using the kind catches a mismatch.

### 3. Its part: what it draws, and when

`make` returns parts (`scene::Part`). A part is still (painted once into the picture), live (drawn each
frame on the GPU), or both. The order of its calls, once per monitor:

1. `make`, when the scene is built: settings only; no GL yet.
2. `rest` (live) or `paint` (still): the still picture, drawn with cairo. The desktop's wallpaper is
   this picture, and it is what shows while the wall is paused or cannot animate, so a live part
   draws itself here too, at rest.
3. `start`, then `textures`: hand its pictures to `Uploads` (`up.add(surface, Kind::...)`), keep the
   `Tex` handles. A picture made for `rest` can be handed over here: `textures` comes after it.
4. Each frame: `state` for every part, then `draw` for the live ones, only if some `state` changed.

A live part says `live() -> true` and implements:

- `state(&mut self, g, w) -> u64`: work out this frame (from `w.clock`, the scene's seconds, and
  readings), and return a hash of what it shows (`scene::hash_f` for numbers, `scene::hash_verts` for its
  quads). The same hash, nothing is drawn. Quantise the clock to the part's frame rate, so the hash
  changes only when the picture does.
- `draw(&mut self, g, gfx, w, p)`: draw what `state` worked out.
- `interval()`: seconds between frames, `1.0 / fps`.
- ⚠️ `top(&self, g) -> i32`: the topmost screen row it can change. The live window covers only the
  rows from the smallest `top` of the live parts down; the default, `g.h`, is "none", and a live part
  that keeps it draws nowhere on the wall (while `--snapshot` may still look right). A whole-screen
  part returns 0.

The many other methods have defaults: `layer`, `area`, `passes` (several still or live passes),
`backdrop` (the picture under it, to blend with) and so on, documented on the trait in `scene.rs`.

### 4. Drawing

Coordinates are screen pixels, x right and y down, the monitor `g.w` by `g.h`; the same on the GPU,
whatever part of the screen the live window covers. Colours on the GPU are premultiplied.

| Call | Draws |
|---|---|
| `gfx.draw(tex, &quads, blend)` | quads: 32 `f32`s each, four corners of `u, v, r, g, b, a, x, y` (colour premultiplied, times the texture) |
| `gfx.image(tex, x, y, w, h, strength, blend)` | a texture over a rectangle |
| `gfx.rects(tex, &[(x0, y0, x1, y1, alpha)], rgb, blend)` | rectangles, plain or textured |
| `gfx.replace_bytes(tex, &bytes, x, y, w, h)` | new pixels for part of a texture (premultiplied B, G, R, A rows) |
| `gfx.compile`, `gfx.uniform`, `gfx.custom` | a fragment shader of the part's own (see `elements/road/`, its GLSL in `shaders.rs`) |

`util` makes the quads and pictures: `sprite_vertices` (sprites placed, sized, turned, tinted),
`image_quad`, `fill_quad`, `soft_dot` (a round glow), `surface` and `view` (a cairo picture and its
bytes), `picture` (bytes to a picture), and `paint_vertices`, which draws quads with cairo, for `rest`.
`Blend` is `Premultiplied` for most, `Opaque`, `Alpha`, `Screen` or `Add`. A texture's `Kind` says how
it is sampled: `Label` pixel for pixel, `Sprite` scaled and turned, `Pixel` sharp pixel art, `Smooth`
filtered.

Numbers the same on every start: `rng::Rng` (seed it from the element's settings and `rng::run_seed`),
`rng::lattice` (bits for a point of a grid), `util::smooth` and `util::smoothstep` (eased 0 to 1). Use
these before you write your own.

Readings: `w.reading("cpu")` is a source's value, 0 to 100, and its indicator; any source a user has
works by name. Under `--snapshot` and `--demo` readings and the clock are fixed (`w.frozen`, `w.demo`),
so a picture can be compared.

### 5. A whole 2D kind

```rust
//! pulse: discs that grow and shrink, faster as a reading rises.

use crate::elements::{Validate, check, each, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, ImageSurface};
use serde::Deserialize;
use toml::Table;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PulseSettings {
    colour: Colour,
    source: String,
    size: f64,             // a disc's radius, a share of the screen's height
    rate: [f64; 2],        // pulses a second at 0 and at 100
    places: Vec<[f64; 2]>, // centres, shares of the screen
}

impl Validate for PulseSettings {
    fn validate(&self) -> Result<(), String> {
        within("size", self.size, 0.001, 0.5)?;
        each("rate", &self.rate, 0.0, 20.0)?;
        self.places.iter().try_for_each(|p| each("places", p, 0.0, 1.0))
    }
}

pub struct Pulse {
    s: PulseSettings,
    dot: Option<ImageSurface>,
    tex: Option<Tex>,
    phase: f64,        // in turns: summed, so a change of rate never jumps
    last: Option<f64>,
    quads: Vec<f32>,
}

impl Pulse {
    fn new(t: &Table) -> Self {
        Pulse { s: settings(t), dot: None, tex: None, phase: 0.0, last: None, quads: vec![] }
    }
    fn dot(&mut self, g: &Geo) -> ImageSurface {
        let px = (4.0 * self.s.size * g.h as f64).ceil().clamp(4.0, 1024.0) as i32;
        self.dot.get_or_insert_with(|| soft_dot(px, &SOFT_DOT)).clone()
    }
    fn place(&mut self, g: &Geo) {
        let r = self.s.size * g.h as f64 * (1.0 + 0.4 * (self.phase * std::f64::consts::TAU).sin());
        let placed: Vec<_> = self.s.places.iter().map(|p| (p[0] * g.w as f64, p[1] * g.h as f64, 2.0 * r, 0.0, self.s.colour.rgb(), 0.8)).collect();
        self.quads = sprite_vertices(&placed);
    }
}

impl Part for Pulse {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        let r = 1.4 * self.s.size * g.h as f64;
        let y = self.s.places.iter().map(|p| p[1] * g.h as f64 - r).fold(g.h as f64, f64::min);
        (y.floor() as i32).clamp(0, g.h)
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.place(g);
        paint_vertices(c, &self.dot(g), &self.quads);
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.dot(g), Kind::Sprite));
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let t = (w.clock * 30.0).floor() / 30.0;
        let v = w.reading(&self.s.source).0 / 100.0;
        let rate = self.s.rate[0] + (self.s.rate[1] - self.s.rate[0]) * v;
        self.phase = (self.phase + rate * (t - self.last.unwrap_or(t)).max(0.0)).fract();
        self.last = Some(t);
        self.place(g);
        let mut h = 0;
        hash_verts(&mut h, &self.quads);
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        gfx.draw(self.tex, &self.quads, Blend::Premultiplied);
    }
}
```

Its `kinds()` is the one in step 1. A theme uses it:

```toml
extends = "phosphor"
scene = ["sky", "mountains", "beats", "title", "instruments", "finish"]

[element.beats]
type = "pulse"
places = [[0.2, 0.6], [0.5, 0.6], [0.8, 0.6]]
```

A scene starts with a still element (`sky`, a `painting`): it is the picture the live ones are drawn
over.

### 6. A 3D kind, traced on the CPU

`src/trace/` is what a world drawn in 3D shares (see "Traced scenes" in `ARCHITECTURE.md`): a camera,
rays against boxes, models of boxes on hinges (`rig::Pose`), and the machinery to move things over a
picture traced once (`sprites::Backdrop`, `sprites::Changes`). A kind of its own supplies the world and
its colours. The way it goes:

1. Once (`rest` or `textures`, whichever comes first): a `Camera` (`Camera::new(eye, yaw, pitch, fov,
   w, h)`: yaw 0 looks along +z, y is up, a negative pitch looks down), and what never moves traced
   into a picture (premultiplied B, G, R, A rows) and the depth of each pixel, kept as a `Backdrop`.
2. The backdrop up as one screen-sized texture (`Kind::Label`), drawn each frame with `gfx.image`.
3. Each frame, in `state`: each moving thing posed (`Pose::new`), the pixels it covers
   (`pose.pixels(&cam)`), left out where the backdrop hides it all (`backdrop.hides`), clipped to it
   (`overlap`); `changes.since(...)` gives the regions it left and the regions it is in now, keyed by
   anything that changes its picture (`Changes::new(n)` for n things; the first call gives all of them).
4. In `draw`: each region, the backdrop's pixels and depths there (`backdrop.crop`) with the things
   traced over them, goes up with `gfx.replace_bytes`. ⚠️ The depth test is the caller's: `Pose::trace`
   leaves out only pixels the scene is nearer than the whole model; compare each hit's `t` with the
   region's depth.
5. A picture that is the same each start (the traced backdrop) can be kept in the theme's cache:
   `svg::kept_bytes`, as `elements/blocks/` does.

```rust
//! orrery: a sun traced once, a planet moving round it, traced each frame where it moved.

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::trace::camera::{Px, overlap};
use crate::trace::rig::{Pose, RigHit};
use crate::trace::sprites::{Backdrop, Changes};
use crate::trace::{Camera, V3, dot, norm};
use crate::tweaks::Colour;
use crate::util::*;
use cairo::Context;
use serde::Deserialize;
use toml::Table;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrrerySettings {
    space: Colour,
    sun: Colour,
    planet: Colour,
    period: f64, // seconds an orbit
}

impl Validate for OrrerySettings {
    fn validate(&self) -> Result<(), String> {
        within("period", self.period, 1.0, 3600.0)
    }
}

const LIGHT: V3 = [-0.4, 0.8, -0.45];
const ORBIT: f64 = 6.0;

/// A face's colour, lit by LIGHT: B, G, R, A.
fn shade(c: Colour, h: &RigHit) -> [u8; 4] {
    let k = 0.2 + 0.8 * dot(h.normal, norm(LIGHT)).max(0.0);
    let (r, g, b) = c.rgb();
    let byte = |v: f64| (v * k * 255.0).round().clamp(0.0, 255.0) as u8;
    [byte(b), byte(g), byte(r), 255]
}

pub struct Orrery {
    s: OrrerySettings,
    cam: Option<Camera>,
    backdrop: Option<Backdrop>,
    tex: Option<Tex>,
    changes: Changes<u64>,
    angle: f64,
    dirty: Vec<Px>,
}

impl Orrery {
    fn new(t: &Table) -> Self {
        Orrery { s: settings(t), cam: None, backdrop: None, tex: None, changes: Changes::new(1), angle: 0.0, dirty: vec![] }
    }
    fn planet(&self, eye: V3) -> Pose {
        let at = [ORBIT * self.angle.cos(), 0.0, ORBIT * self.angle.sin()];
        Pose::new([([-0.4; 3], [0.4; 3], None)], at, 3.0 * self.angle, 1.0, eye)
    }
    /// The camera, and space with the sun traced into it, once.
    fn render(&mut self, g: &Geo) {
        if self.cam.is_some() {
            return;
        }
        let (w, h) = (g.w.max(1) as usize, g.h.max(1) as usize);
        let eye = [0.0, 4.5, -2.2 * ORBIT];
        let cam = Camera::new(eye, 0.0, -(4.5f64).atan2(2.2 * ORBIT), 50.0, w, h);
        let (r, gg, b) = self.s.space.rgb();
        let mut pic = [(b * 255.0) as u8, (gg * 255.0) as u8, (r * 255.0) as u8, 255].repeat(w * h);
        let mut depth = vec![f32::INFINITY; w * h];
        let sun = Pose::new([([-1.0; 3], [1.0; 3], None)], [0.0; 3], 0.6, 1.0, eye);
        sun.trace(&cam, (0, 0, w as i32, h as i32), |_, _| f32::INFINITY, |x, y, _, hit| {
            let i = y as usize * w + x as usize;
            depth[i] = hit.t as f32;
            pic[i * 4..i * 4 + 4].copy_from_slice(&shade(self.s.sun, &hit));
        });
        self.backdrop = Some(Backdrop::take((0, 0, w as i32, h as i32), w, &[&pic], |x, y| depth[y as usize * w + x as usize]));
        self.cam = Some(cam);
    }
    /// A region of the backdrop with the planet traced into it, nearer than what is there.
    fn region(&self, clip: Px) -> Vec<u8> {
        let (Some(cam), Some(bd)) = (self.cam, &self.backdrop) else { return vec![] };
        let (mut planes, near) = bd.crop(clip);
        let mut pic = planes.swap_remove(0);
        let rw = (clip.2 - clip.0) as usize;
        self.planet(cam.eye).trace(&cam, clip, |x, y| bd.depth[bd.index(x, y)], |x, y, _, hit| {
            let k = (y - clip.1) as usize * rw + (x - clip.0) as usize;
            if (hit.t as f32) < near[k] {
                pic[k * 4..k * 4 + 4].copy_from_slice(&shade(self.s.planet, &hit));
            }
        });
        pic
    }
}

impl Part for Orrery {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.render(g);
        let _ = c.set_source_surface(picture(&self.region((0, 0, g.w, g.h)), g.w, g.h), 0.0, 0.0);
        let _ = c.paint();
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.render(g);
        if let Some(bd) = &self.backdrop {
            self.tex = Some(up.add(picture(&bd.planes[0], g.w, g.h), Kind::Label));
        }
        self.changes = Changes::new(1); // the planet painted in by the first frame's changes
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.render(g);
        let t = (w.clock * 30.0).floor() / 30.0;
        self.angle = (t / self.s.period * std::f64::consts::TAU) % std::f64::consts::TAU;
        let (Some(cam), Some(bd)) = (self.cam, &self.backdrop) else { return 0 };
        let pose = self.planet(cam.eye);
        let seen = pose.pixels(&cam).filter(|&p| !bd.hides(p, pose.near() as f32)).and_then(|p| overlap(p, bd.at));
        self.dirty.extend(self.changes.since([(self.angle.to_bits(), seen)]));
        self.angle.to_bits()
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some(tex) = self.tex else { return };
        for clip in std::mem::take(&mut self.dirty) {
            gfx.replace_bytes(tex, &self.region(clip), clip.0, clip.1, clip.2 - clip.0, clip.3 - clip.1);
        }
        gfx.image(tex, 0.0, 0.0, g.w as f64, g.h as f64, 1.0, Blend::Opaque);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "orrery",
        defaults: r##"space = "#05040b"
sun = "#ffb040"
planet = "#4fb6d6"
period = 20.0"##,
        varies: &[],
        check: check::<OrrerySettings>,
        make: |_, t, _g| Ok(vec![Box::new(Orrery::new(t))]),
    }]
}
```

`elements/blocks/` is the whole of what the core can carry: a world of blocks traced on every core,
lit each frame by the time of day on the GPU, animals as rigs walking over the traced picture, the
trace kept in the cache. Start from the orrery; read `elements/blocks/` for each next thing.

### 7. Seeing it, and shipping it

```sh
retro-wall --snapshot /tmp/out.png --theme NAME --demo 13:00           # one frame, fixed readings
retro-wall --snapshot /tmp/out.png --theme NAME --frames 90 --fps 30   # out-0000.png ... : the motion
retro-wall --settings NAME                                             # every setting, as resolved
```

A theme that comes with retro-wall, `themes/NAME/`, is in its tests: its name in the list in
`tests/themes.rs` (`every_bundled_theme_has_a_test`), a `draws("NAME")` test there, and its name in
`THEMES` in `tests/gl.rs`, with its reference picture made by `RETRO_BLESS=1 cargo test --test gl`
(the others' left as they are). Its screenshot and a line in the README, as "Sharing your theme" says.
