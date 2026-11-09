# Architecture

One binary, `retro-wall`. A frame is drawn on the GPU from textures baked once; the CPU does almost
nothing between frames.

## Pieces

| Module | Does |
|---|---|
| `config.rs`, `tweaks.rs` | Themes and `retro.toml`: the theme format (`THEME_FORMAT`; a newer one is refused), `extends` chains, palette roles (and hue turning), tweaks, `[day]`/`[night]` looks, layout. Every setting checked against its kind's defaults; every tweak against its range (`tweaks::LIMITS`). A theme that loads cannot stop the wall; a user's theme that does not load gives way to the installed default. `retro.toml` is checked item by item: what does not check is said and left out |
| `elements/` | The element kinds a scene is made of, one module or directory per family, each listed once in `MODULES` (`elements/mod.rs`). Each declares its settings (TOML defaults), checks them, and builds its parts. `elements/instruments/` has one file per widget type and a table of them (`TYPES`). A new kind, step by step, with a 2D and a 3D one whole: `EXTENDING.md`, "In code: a new kind of element" |
| `scene.rs` | `Part` trait; the scene compiler: still passes painted once into layers, runs of layers between live passes merged into one texture, empty runs dropped. `compose()` draws the still picture. A part draws in the look its `Geo` carries; it adds its pictures to `Uploads` and keeps their handles |
| `gfx.rs` | The renderer: one vertex stream a frame, a draw per run of quads with the same texture, blend, clip and mask (a second texture's alpha, a tank's window). Small filtered pictures go up packed onto pages (`Uploads::add_packed`), so many of them are one draw: a draw costs a small processor's driver far more than its pixels. New pixels of a texture's size go into the storage it has. A table a shader reads every frame can go up as a vec4 array uniform (`Gfx::uniform_array`, where `uniform_room` allows), far cheaper for a driver than a texture changed each frame. OpenGL 3.3 core, or 2.1 where the driver has no more |
| `live.rs` | The frame loop: one window per monitor driven (one, or `--all-monitors` as they come and go), the header placed by `[monitors]`, pausing (battery, screen off, VNC viewer), covered-monitor detection, `--snapshot`. The look is chosen here and handed to each monitor's scene |
| `platform/` | Everything system-specific, behind `Platform` (the machine: power, counters, addresses, the desktop), `Display` (monitors, windows and GL contexts, events, coverage, screen off) and `Desktop` (a desktop environment's wallpaper settings). `platform/linux/`: X11 and GLX, `/proc` and `/sys`, the desktops' own tools; `platform/macos/`: Cocoa and NSOpenGL, Mach, sysctl and IOKit, NSWorkspace. `platform/unix.rs`: what Linux and macOS read alike through POSIX. Each platform's `impl Platform` takes the machine's readings from its `system` module (`readings_from_system!`). `platform/windows/`: Win32 and WGL (windows behind the desktop icons, in Explorer's WorkerW), the system's counters, the wallpaper through SystemParametersInfo |
| `trace/` | Scenes traced on the CPU, for an element that draws a world of its own in 3D (`elements/blocks/` is one): the camera, rays and boxes, models of hinged boxes, and what moves over a picture traced once. See "Traced scenes" |
| `still.rs` | The still picture, handed to the desktop or shown in a window of our own |
| `sources/`, `plugins.rs` | Readings: built-in sources on a thread; plugin sources and faces as child processes speaking JSON lines |
| `svg.rs`, `path.rs`, `title.rs`, `util.rs` | SVG layers and what a theme works out once (with an exact cache in `$XDG_CACHE_HOME/retro-wall`, on Windows `%LOCALAPPDATA%\retro-wall\cache`), SVG path data, the hostname lettering, shared drawing helpers |
| `files.rs` | Every file a theme or the config names is opened here: regular files only, each kind within its size |
| `font.rs` | The typeface, DejaVu Sans Mono, carried in the binary: written to the cache once and registered for the process (`platform::register_font`) before anything draws |
| `cli.rs` | The command line, `--new`, `--settings`, `--list`, `--preview`, `--desktop` (the installer's save and restore of the desktop's wallpaper settings) |

## Traced scenes

An element that draws a world in 3D traces it once on the CPU, then lights and moves it each frame. `trace/`
holds what such an element shares, so a new one is a world and its models, not a renderer:

- `camera`: the eye, the ray through a pixel, where a point falls on the screen (`bounds`, `pixels`).
- `boxes`: a ray and a box, exactly (`slab`), and quickly with the ray's reciprocal (`Ray::enters`) on a
  box grown by `PAD`, which never misses a box the exact test finds.
- `rig`: a model of boxes, each on an optional hinge, placed and turned in the world. `Pose::new` once a
  frame; `Pose::trace` into a region gives each pixel it is seen in. Only the pixels whose middles lie in
  a part's outline on the screen (the hull of its box's corners, row by row) are traced, only those
  parts there, and none where the scene is nearer than the model can be (behind a house).
- `sprites`: what moves over the traced picture. `Backdrop` keeps the picture and its depths under it;
  `Changes` gives the regions to paint again, where something moved from and to, merged. A pixel is the
  backdrop and what is traced over it, so a region painted again is what painting all of it would make.
  A thing hidden where it is (all of its pixels behind the scene) changes nothing.
- `rows`: a picture filled row by row on every core.

What is the same each start (the traced picture, its depths, pictures made at many sizes) goes in the
theme's cache: `svg::kept_bytes`, keyed by the screen size, a tag of the settings it was made with, the
theme file and the build; `svg::kept_read` and `svg::keep` read and write it apart, so it is read while
something else is made. `util::pack_surfaces` keeps a list of pictures. The still picture works out only
what shows: nothing under what is opaque. It is handed to the desktop as a PNG made by `util::write_png`,
quickly: the same pixels as cairo's, in a fraction of the time.

## A frame

1. Each live part's `state()` returns a hash of what it shows now, once, before any part draws; nothing
   changed, nothing presented.
2. Changed: the plan runs bottom to top. Runs are one textured quad set each; live passes append quads.
3. `Gfx::flush` uploads the vertex stream once and issues the draws; one buffer swap.

Frames are paced by the quickest part's `interval()` (30 fps for most, 60 while the tape spools), each
monitor on its own, and stop while its picture is covered or the wall is paused. With nothing to draw,
the loop sleeps on the display connection until the next check (at most 2 s) or an event.

## Rendering

- **OpenGL 3.3 core** by default: vertex array object, GLSL 330.
- **OpenGL 2.1 fallback** (GLSL 1.20, the GLES 2 model): taken when the driver will not make a 3.3
  core context (Intel Ironlake), or with `RETRO_GL=2.1`.
- **Software** (llvmpipe): either profile; `--snapshot` and the tests run on it under Xvfb. The live
  wall refuses to animate on a software renderer unless `--software`. On Linux, a display whose GLX is
  software only (Xephyr, Xvnc) is first asked of a window of one pixel: with VirtualGL installed, the
  wall runs itself again through `vglrun -d /dev/dri/card0`, so the GPU draws and VirtualGL hands the
  frames over (`RETRO_VIRTUALGL` marks that run).

A theme's `shader` element is written once in GLSL 1.20 and runs on both profiles.

## Memory

- A picture stays on the CPU only while it is used again. A part hands its pictures to `Uploads` once
  the still picture is drawn (`Part::textures`); caches kept while a scene is built are let go when it
  is (`elements::built`).
- A layer's texture holds only the rectangle where it has anything drawn, within its area
  (`Uploads::add_layer`); textures go up in strips of about a megabyte, so a driver's staging buffers
  stay small.
- The allocator: glibc's mmap threshold is fixed, so a screen-sized picture is given back when it is
  freed, and the heap is trimmed after a monitor's scene is built. On macOS a large picture has a
  mapping of its own: the system allocator keeps a large block it is given back in the footprint.

## Windows

- **One exe**, target `x86_64-pc-windows-gnu`: cairo and its libraries linked in statically, only system
  DLLs imported. A GUI program: no console window. Its output goes to the console it was started from,
  or to whatever it was redirected to, else to `%LOCALAPPDATA%\retro-wall\retro-wall.log`.
- **Windows**: one per monitor, children of the WorkerW that Explorer puts behind the desktop icons when
  Progman is sent `0x052C`. On Windows 11 24H2 and later that WorkerW is a child of Progman: the windows
  are then children of Progman, layered, just below the icons (`SHELLDLL_DefView`). Without Explorer, a
  top-level window at the bottom, moved into the desktop when Explorer comes. Explorer restarting
  (`TaskbarCreated`) makes the windows again; each keeps its GL context, so nothing is uploaded again.
- **WGL**: 3.3 core through `WGL_ARB_create_context`, else a legacy context of 2.1 or more. Swap interval 0:
  DWM composes.
- **Older than 2.1** (Windows' own GDI Generic, OpenGL 1.1, all a remote session without a GPU gets): no
  live window. `Display::still_window` shows the still picture over the whole monitor, its header the
  hostname only (frozen instruments would show untrue readings), drawn with
  `glDrawPixels` (OpenGL 1.0) and drawn again by the platform on every expose. It never animates.
- **Per-monitor DPI aware (v2)**: monitors and windows in physical pixels.
- **The font**: cairo's toy font API on Windows finds families through DirectWrite's system collection
  only, which never holds a font added at run time (`AddFontResourceEx`, private or not). So
  `platform::register_font` opens the file with FreeType (linked in with cairo) and `util::select_mono`
  sets that face. The glyphs are FreeType's, as on Linux. Before a font is registered, the toy API's
  lookup is used.
- **Paths**: `%APPDATA%\retro` for what `~/.config/retro` holds; `%LOCALAPPDATA%\retro-wall` for the
  program, its themes, `cache\` and `wallpaper\` (the still pictures).
- **The still picture**: one for every monitor, centred on the theme's base colour, through
  `SystemParametersInfo`. `--desktop save|restore` keeps the picture, `WallpaperStyle`, `TileWallpaper`
  and the desktop colour. A slideshow or Spotlight is not saved.
- **`--pause-screen-off`**: the console display off (`GUID_CONSOLE_DISPLAY_STATE`), or the session locked.

## Built-in measurements

`sources::BUILTINS` is a table. A measurement is one file in `sources/` with a `Builtin` (its name, pace,
snapshot value and reader, real or made up for `--demo`) and one line in the table. What it reads of the
machine goes through `Platform`.

## Plugins

Programs, in any language, so a plugin needs no toolchain and cannot crash the wall. They speak a numbered protocol (`plugins::PROTOCOL`, in each face request and in `RETRO_PROTOCOL`):

- **sources** (`~/.config/retro/sources/NAME`): print a reading; polled or streaming.
- **faces** (`~/.config/retro/faces/NAME`): read a JSON request a line, answer a list of drawing
  operations; asked only when the reading changes.
- **themes** (`~/.config/retro/themes/NAME/`): TOML, pictures, optional GLSL.

See `EXTENDING.md`.
