# Cross-compile retro-wall for x86_64-pc-windows-gnu with the whole cairo stack linked statically: the
# exe needs no DLL that Windows does not have.
#
#   docker build -t rwall-win ci/windows
#
# Arch for the compiler: its mingw-w64 gcc matches the one MSYS2 builds its libraries with. The
# libraries, and the C runtime they were built against, are MSYS2's mingw64 (msvcrt) packages, pinned
# in packages.lock and checked against the MSYS2 keyring.
FROM archlinux:latest

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN pacman -Syu --noconfirm --needed mingw-w64-gcc mingw-w64-binutils mingw-w64-crt mingw-w64-headers \
      mingw-w64-winpthreads gcc pkgconf rustup curl zstd gnupg \
 && pacman -Scc --noconfirm \
 && rustup default stable && rustup target add x86_64-pc-windows-gnu

# Library packages go to /opt/msys2. The runtime packages (crt, headers, winpthreads, gcc) replace
# Arch's crt and gcc runtime libraries: MSYS2's static libraries need its crt (strndup, msvcrt imports).
COPY packages.lock msys2.gpg /tmp/msys2/
RUN set -eu; cd /tmp/msys2; \
    export GNUPGHOME=/tmp/msys2/gpg; mkdir -m 700 $GNUPGHOME; gpg -q --import msys2.gpg; \
    mkdir -p /opt/msys2 rt; \
    while read sum file; do \
      curl -sSfO https://repo.msys2.org/mingw/mingw64/$file; \
      curl -sSfO https://repo.msys2.org/mingw/mingw64/$file.sig; \
      echo "$sum  $file" | sha256sum -c -; \
      gpg -q --verify $file.sig $file; \
      case $file in \
        mingw-w64-x86_64-crt-*|mingw-w64-x86_64-headers-*|mingw-w64-x86_64-winpthreads-*|mingw-w64-x86_64-gcc-[0-9]*) \
          tar -C rt --zstd -xf $file mingw64 ;; \
        *) tar -C /opt/msys2 --zstd -xf $file mingw64 ;; \
      esac; \
      rm -f $file $file.sig; \
    done < packages.lock; \
    v=$(x86_64-w64-mingw32-gcc -dumpfullversion); G=/usr/lib/gcc/x86_64-w64-mingw32/$v; \
    M=rt/mingw64/lib/gcc/x86_64-w64-mingw32/$v; \
    [ -d "$M" ] || { echo "gcc version mismatch: Arch $v, MSYS2 $(ls rt/mingw64/lib/gcc/x86_64-w64-mingw32)"; exit 1; }; \
    cp -f $M/*.a $M/*.o $G/; \
    # no import libraries for the gcc runtime: Rust's -Bdynamic would take the DLL
    rm -f $G/*.dll.a $G/libgcc_s.a; \
    rm -rf rt/mingw64/lib/gcc rt/mingw64/include/c++; \
    rm -rf /usr/x86_64-w64-mingw32/lib /usr/x86_64-w64-mingw32/include; \
    cp -a rt/mingw64/lib rt/mingw64/include /usr/x86_64-w64-mingw32/; \
    rm -f /usr/x86_64-w64-mingw32/lib/libwinpthread.dll.a /usr/x86_64-w64-mingw32/lib/libpthread.dll.a \
          /usr/x86_64-w64-mingw32/lib/libstdc++.dll.a /usr/x86_64-w64-mingw32/lib/libgcc_s.a; \
    cd /; rm -rf /tmp/msys2; \
    # the .pc files point at the sysroot; no DLLs or import libraries, so only static archives link
    sed -i 's|^prefix=/mingw64|prefix=/opt/msys2/mingw64|' /opt/msys2/mingw64/lib/pkgconfig/*.pc; \
    rm -rf /opt/msys2/mingw64/bin /opt/msys2/mingw64/share/doc /opt/msys2/mingw64/share/man; \
    find /opt/msys2/mingw64/lib -name '*.dll.a' -delete

# crt-static and -static link libgcc, libstdc++ and winpthread in. The link group closes what the .pc
# files leave out: libstdc++ (graphite2), iconv and advapi32 (libintl), and the crt they need.
ENV PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_PATH= \
    PKG_CONFIG_LIBDIR=/opt/msys2/mingw64/lib/pkgconfig \
    SYSTEM_DEPS_LINK=static \
    CARGO_BUILD_TARGET=x86_64-pc-windows-gnu \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-static -C link-arg=-Wl,--start-group -C link-arg=-lstdc++ -C link-arg=-lwinpthread -C link-arg=-liconv -C link-arg=-ladvapi32 -C link-arg=-lmingwex -C link-arg=-lmsvcrt -C link-arg=-lkernel32 -C link-arg=-Wl,--end-group"
