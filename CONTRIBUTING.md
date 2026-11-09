# Contributing

Themes, measurements, instruments, fixes and support for more systems are welcome. `EXTENDING.md`
tells how to make each; `ARCHITECTURE.md` tells how the program is built.

## Reporting a problem

Open an issue with the bug form. Give:

- `retro-wall --version`, the system (Linux distribution and desktop, macOS or Windows version) and
  the GPU;
- what retro-wall said. Stop the running wall, start `retro-wall` in a terminal, and copy its output.
  The installed wall writes it to `~/.local/share/retro/retro-wall.log` (Linux, started by the
  installer), `~/Library/Logs/retro-wall.log` (macOS) or `%LOCALAPPDATA%\retro-wall\retro-wall.log`
  (Windows);
- for a picture that is wrong: `retro-wall --snapshot out.png --demo 13:37`, attached, and the theme.

## A pull request

One unit of work in each pull request: one theme, one fix, one feature. The commit message says why.

Before you open it, run what CI runs (Linux; the GL tests need Xvfb):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
RETRO_REQUIRE_XVFB=1 cargo test --release
```

Or in a container with everything the tests need:

```sh
docker build -t retro-wall-test ci/test
docker run --rm -v "$PWD:/src" -w /src -e CARGO_TARGET_DIR=/src/target/test -e RETRO_REQUIRE_XVFB=1 \
	retro-wall-test sh -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --release'
```

CI also builds for macOS and Windows, checks the dependencies (`cargo deny`) and the oldest Rust it
supports (`rust-version` in `Cargo.toml`), and installs and uninstalls in containers of the Linux
desktops. A pull request is merged when CI is green.

- **A visible change.** The GL tests compare each bundled theme with its picture in
  `tests/reference/`. Make the pictures again with `RETRO_BLESS=1 cargo test --release --test gl`, and
  commit only those your change was meant to change.
- **A new theme.** "Sharing your theme" in `EXTENDING.md`: the theme's directory, art you have the
  right to share under the GPL-3.0, the theme in the bundled-theme tests, and its reference
  picture.
- **A new kind of element.** One file in `src/elements/` and one line in `MODULES`
  (`src/elements/mod.rs`). Name the kind for what it draws, not for a theme. Use the shared helpers
  (`util`, `rng`, `elements::{within, each, positive, band, named}`) before you write your own.
- **A changed theme key, setting or plugin field.** These are interfaces: "What stays stable" in
  `EXTENDING.md`. Add; do not rename or remove. A change that cannot be an addition needs a new
  format or protocol number, and is discussed in an issue first.

## License

retro-wall is GPL-3.0-only (`LICENSE`). A contribution is under the same license. The bundled font has its own
(`fonts/LICENSE`).
