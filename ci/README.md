# Release builds

The CI workflows use these; they also work by hand. The macOS and Windows builds link cairo in
statically, so the binary needs no library that the system does not have.

## Linux (x86_64)

```sh
docker run --rm -v "$PWD:/src" -w /src ubuntu:22.04 ci/linux/build   # target/release/retro-wall
```

- Built against Ubuntu 22.04's glibc (2.35): the binary runs there, on Debian 12 and on every newer
  system. One built on a newer system needs that system's glibc or newer.
- cairo is the system's (libcairo2); the installer installs it where it is missing.

## macOS (arm64)

```sh
brew install meson ninja pkgconf             # build tools only
ci/macos/build-cairo-static ~/cairo-static   # pixman, libpng, cairo from source, checked by sha256
CAIRO_STATIC=~/cairo-static ci/macos/cargo build --release
otool -L target/release/retro-wall           # only /usr/lib and /System/Library
```

- cairo has its Quartz surface and fonts; no FreeType, fontconfig, X11 or glib.
- `MACOSX_DEPLOYMENT_TARGET` (11.0 by default) must be the same for both scripts.
- arm64 only. An x86_64 or universal binary needs the prefix built again with `-arch x86_64`, the
  x86_64-apple-darwin target, and `lipo`.
- The binary has an ad hoc signature. A downloaded copy is quarantined: `xattr -d
  com.apple.quarantine retro-wall`, or sign and notarize it.

## Windows (x86_64, cross-built)

```sh
docker build -t rwall-win ci/windows
ci/windows/cargo build --release             # target/x86_64-pc-windows-gnu/release/retro-wall.exe
```

- The image is Arch's mingw-w64 gcc with MSYS2's mingw64 (msvcrt) libraries and C runtime. The
  packages are pinned in `packages.lock` and checked against `msys2.gpg`, the MSYS2 keyring.
- **The image will not build when Arch's gcc and MSYS2's differ** ("gcc version mismatch"): both
  roll. Run `ci/windows/update-packages`, then build again; if they still differ, wait for both to
  have the same release.
- No import library of the gcc runtime is left in the image: Rust links with `-Bdynamic` and would
  take the DLL. A project's own `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS` replaces the image's,
  which close the link: add to them.
