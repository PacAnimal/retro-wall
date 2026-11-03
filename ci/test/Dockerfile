# The tests' environment: build dependencies, rustfmt, clippy, Xvfb, Xorg's dummy driver, Composite and Damage.
FROM rust:1-slim-trixie
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libcairo2-dev libx11-dev libfontconfig-dev \
      libgl-dev libxext-dev libxrandr-dev \
      xvfb xauth libgl1-mesa-dri libgl1 x11-xserver-utils x11-apps mesa-utils xserver-xorg-core \
      xserver-xorg-video-dummy tigervnc-standalone-server fonts-dejavu-core libxcomposite1 libxdamage1 \
 && rm -rf /var/lib/apt/lists/* \
 && rustup component add rustfmt clippy
