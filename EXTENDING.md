# Extending retro-wall

Three things can be added, none of them needing retro-wall to be rebuilt:

| To add | Write | Where |
|---|---|---|
| **A measurement** (what an instrument reads) | a program that prints a number | `~/.config/retro/sources/NAME` |
| **An instrument** (how a reading is drawn) | a program that answers with drawing operations | `~/.config/retro/faces/NAME` |
| **A theme** (the whole picture) | a `theme.toml`, pictures, optionally shaders | `~/.config/retro/themes/NAME/` |

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
{"width": 172, "height": 117, "scale": 0.57, "value": 46.0, "indicator": false, "text": null,
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

A face in sh can ignore its requests and print the same drawing each time (what `--new face` writes),
or pick the numbers it needs out of the line (`examples/faces/level`); one that reads all of it is best
written in a language with a JSON parser.

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
