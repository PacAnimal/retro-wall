# Changelog

Each release, newest first. A change to an interface in "What stays stable" (`EXTENDING.md`) is said
here with its new number.

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
