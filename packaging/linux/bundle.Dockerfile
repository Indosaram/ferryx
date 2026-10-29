# Bundle builder for the Linux desktop release (AppImage + deb).
#
# The desktop app links WebKitGTK/GTK from the build host, so the glibc and library floor of the
# shipped bundles is the build image's: Ubuntu 22.04 (glibc 2.35, webkit2gtk-4.1). The release
# script (scripts/lib/release-platforms.mjs createLinuxBuildScript) builds this image with the
# host's Rust, Zig and Bun versions so the receipt's recorded toolchains describe both builds.
FROM ubuntu:22.04

ARG RUST_VERSION
ARG ZIG_VERSION
ARG BUN_VERSION
ARG TAURI_CLI_VERSION=2.10.1

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential ca-certificates cmake curl file git pkg-config unzip xz-utils \
      libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
      libssl-dev libasound2-dev libdbus-1-dev libxdo-dev libopus-dev \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup \
    PATH=/opt/cargo/bin:/opt/zig:/opt/bun/bin:$PATH
RUN test -n "$RUST_VERSION" && test -n "$ZIG_VERSION" && test -n "$BUN_VERSION" \
 && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | CARGO_HOME=/opt/cargo sh -s -- -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
 && CARGO_HOME=/opt/cargo cargo install tauri-cli --version "$TAURI_CLI_VERSION" --locked \
 && rm -rf /opt/cargo/registry /opt/cargo/git \
 && mkdir -p /opt/zig \
 && curl -sSfL "https://ziglang.org/download/${ZIG_VERSION}/zig-x86_64-linux-${ZIG_VERSION}.tar.xz" \
      | tar -xJ -C /opt/zig --strip-components=1 \
 && mkdir -p /opt/bun/bin \
 && curl -sSfL -o /tmp/bun.zip "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-linux-x64.zip" \
 && unzip -j /tmp/bun.zip bun-linux-x64/bun -d /opt/bun/bin \
 && rm /tmp/bun.zip \
 && chmod -R a+rX /opt
