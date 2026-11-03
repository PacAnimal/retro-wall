# The wall in a browser, for development: see tools/browser/run.
FROM rust:1-trixie
RUN apt-get update -q && apt-get install -y -q --no-install-recommends \
        libcairo2-dev libgl1 libgl1-mesa-dri libxcomposite1 libxdamage1 xvfb x11vnc novnc websockify \
        xfdesktop4 xfwm4 xfconf thunar mousepad dbus dbus-x11 \
    && rm -rf /var/lib/apt/lists/*
