# Changelog

Each release, newest first. A change to an interface in "What stays stable" (`EXTENDING.md`) is said
here with its new number.

## v0.1.3

- **KDE Plasma under Wayland**: Plasma's icons and widgets show over the wall. The installer adds a
  Plasma wallpaper of retro-wall's (see-through where the wall is live, the still picture elsewhere) and
  a KWin script that keeps the wall below Plasma's desktop (on any KWin 6), and starts
  plasmashell again once; the uninstaller takes both out.
- **X11**: the wall keeps itself among the desktop windows rather than raising itself, so no window
  manager, or none at all, puts it over panels and menus; under Cinnamon it lies under Nemo's
  see-through desktop, whose menu showed black over the wall, and makes muffin paint it there, under
  Nemo's icons, which it hid. It asks compositors not to take it out of compositing.
- **Cinnamon**: the installer adds retro-wall to Nemo's `org.nemo.desktop ignored-desktop-handlers`,
  as a Nemo started after the wall (at login, or again) took it for another desktop and drew none; the
  uninstaller takes it out.
- The install tests check, on every desktop under X11 and Wayland, that its icons, panel, widgets and
  right-click menu stay over the wall, as over the user's own wallpaper: each one on its own, after the
  install, after an update, and after the desktop's own process starts again under the running wall.
  They also check that the wall is left where it is while nothing changes, and run a window manager
  alone (openbox), where the wall lays its own still picture under itself. Hyprland, which renders
  only through a KMS device, is tested in a throwaway virtual machine (`tests/linux/vm`), started from
  the line the installer adds to its config. The README has a table of every environment tested.

## v0.1.2

- **Wayland.** On a compositor with layer-shell (sway, Hyprland, KDE Plasma, labwc, niri, COSMIC) the
  wall is a surface on each monitor's background layer, drawn with EGL at the monitor's own pixels, at
  any scale. It lies over the compositor's wallpaper and leaves it as it is, and pauses while the
  compositor does not show it (covered, or the monitor off). GNOME, which has no layer-shell, runs it
  through XWayland. `RETRO_BACKEND=x11` or `=wayland` chooses.
- The installer brings the Wayland and EGL libraries, and under sway and Hyprland, which start no
  autostart entries, adds one marked line to the compositor's config; the uninstaller takes it out.
- `retro-wall.conf`: in a Wayland session the line of its socket (`wayland-1`) comes first, then the X
  display's.
- Where the live window covers all of a monitor (Wayland; X11 while a remote desktop client hides the
  wallpaper) and no desktop shows the still picture, no window of the still picture is made under it.
- The install tests run under Wayland on sway and KDE Plasma (layer-shell) and GNOME (XWayland), as on
  the X11 desktops; the lines that start it with sway and Hyprland are tested on their own. Hyprland
  itself is not in the tests: nested in a container it needs a GPU.

## v0.1.1

- The Windows setup program finds a world of blocks in a theme's scene as well as in its `[element]`
  tables, so a theme of your own that names `block-world` in its scene gets its settings.
- A test loads the theme the setup program writes, not only its text.
- One rule for an element's kind and a widget's type: its `type`, or with none its name.
- A release waits for the tests of its tag's commit to be listed, then to pass.

## v0.1.0

The first release.

- **Linux** (x86_64, X11: XFCE, KDE Plasma, GNOME, Cinnamon, MATE, LXDE, or a window manager alone),
  **macOS** (11 or later, Apple silicon) and **Windows** (10 and 11, x86_64). One binary each.
- **Themes:** phosphor (the default), neon_rain, night_city, aurora, winter, matrix, guybrush,
  copperworks, racing, forest, blocky.
- **Instruments:** VU meters, rings, bars, sparklines, cells, a tape deck that plays with the disks,
  the network's traffic as mountains, and a status line, laid out in groups from `retro.toml`.
- **Your own:** themes in TOML with pictures and GLSL, measurements and instruments as programs in any
  language. `retro-wall --new source|face|theme NAME` writes a working start.
- **Interfaces:** theme format 1, plugin protocol 1.
- **Installers:** `linux/install`, `macos/install`, and `retro-wall-VERSION-windows-setup.exe` for
  Windows, each with an uninstaller that puts the desktop's own wallpaper back. `SHA256SUMS` in the
  release has the checksum of each file.
- The Windows and macOS programs are not signed: see "Install" in the README.
