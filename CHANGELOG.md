# Changelog

Each release, newest first. A change to an interface in "What stays stable" (`EXTENDING.md`) is said
here with its new number.

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
