# Building Ferryx on a Manus sandbox

A Manus free-plan task sandbox is an ephemeral Ubuntu 24.04 VM that can be joined to the tailnet
over Tailscale and reached with `ssh`. It is useful when every machine in
`scripts/pick-build-host.sh` is busy and a Linux build has to happen now. It is **not** a
permanent build host: the sandbox suspends as soon as the Manus task goes idle, which drops SSH.

Verified end to end on 2026-11-05: `ferryx-cli`, `ferryx-relay` and `ferryx-account` built in
7m 10s and `ferryx-cli --version` printed `ferryx-cli 2026.1101.1`.

## Machine

| Item | Value |
| --- | --- |
| CPU | 6 vCPU — Intel Xeon @ 2.50 GHz, 1 socket / 3 cores / 2 threads per core, KVM full virt |
| RAM | 24 GB (25,004,324 kB `MemTotal`) |
| Swap | 8 GB `/swapfile` — **added by hand**, not in the base image and not in `/etc/fstab` |
| Disk | 48 GB virtio (`/dev/vda`, 52,799,397,888 bytes), single ext4 root |
| OS | Ubuntu 24.04.5 LTS, kernel 6.18.38+, x86_64 |
| GPU | none |
| Network | Tailscale userspace (`100.119.158.87`), DERP relay only, no public IP |
| Access | `ssh manus-sbx` (openssh-server + ed25519 key pushed into `~/.ssh/authorized_keys`) |

The RAM figure is real, but the disk is the binding constraint — see "Disk budget" below.

## Why a plain `cargo build` does not work

The sandbox link is fine for GitHub (6.4 MB/s measured) and useless for crates.io:
`static.crates.io` served 15–24 KB/s. The usual mirrors did no better (`rsproxy` 20 KB/s,
`ustc`/`tuna`/`sjtu` did not respond). Assume the registry is unreachable and vendor from the Mac.

## 1. Prepare the box (once per sandbox)

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl git unzip pkg-config libssl-dev cmake clang \
  libclang-dev libsqlite3-dev libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libsoup-3.0-dev \
  protobuf-compiler rsync
```

Rust. The repository pins no `rust-toolchain.toml`; `src-tauri/Cargo.toml` declares
`rust-version = "1.89"`. 1.95.0 is the toolchain every other Ferryx build host already uses, and
it is what this run built with:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.95.0 \
  --component clippy --component rustfmt
```

Zig 0.17.0 is mandatory for the pinned Ghostty terminal. Without a Zig on `PATH` the Ghostty build
step stops before Rust ever compiles a crate:

```bash
curl -fLO https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz
tar xf zig-x86_64-linux-0.17.0.tar.xz -C /opt && mv /opt/zig-x86_64-linux-0.17.0 /opt/zig
sudo ln -sf /opt/zig/zig /usr/local/bin/zig && zig version   # 0.17.0
```

Swap. Added as headroom before the first build; no OOM was observed in the two runs, and 24 GB
carried `-j6` without it. Add it anyway on a smaller sandbox:

```bash
sudo fallocate -l 8G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
```

This is not added to `/etc/fstab`, so a reboot clears it. The sandbox only suspends instead of
rebooting, so the swap survives in practice; re-run the three commands after any restart.

The submodules arrive empty and Ghostty is one of them (`src-tauri/vendor/ghostty`), which the
build needs before Zig is ever invoked:

```bash
cd ~/src/ferryx
git submodule update --init --depth 1 src-tauri/vendor/ghostty
cd src-tauri/vendor/ghostty && git log --oneline -1   # 83fb95624 Bump version to 1.4.0
```

If the box checkout is a shallow export without submodule support, clone and move it instead:

```bash
git clone --filter=blob:none https://github.com/ghostty-org/ghostty.git /tmp/ghostty
cd /tmp/ghostty && git checkout 83fb95624b0439ee660c2679b43b38c8b63769cd
rm -rf ~/src/ferryx/src-tauri/vendor/ghostty
mv /tmp/ghostty ~/src/ferryx/src-tauri/vendor/ghostty
```

Take the revision from `git submodule status` rather than the hash above; the hash is the pin
recorded by `35e91ab`, the commit this run built.

## 2. Vendor on the Mac and transfer

Vendor from **the same commit the box checks out**. A newer local `HEAD` produces a lock that
resolves to crates the older checkout's `Cargo.lock` does not mention, and the offline build fails
with a version-selection error instead of a network error.

```bash
# on the Mac, in a clean clone at the revision the box will build
git clone --depth 1 https://github.com/Indosaram/ferryx.git /tmp/ferryx-src
cd /tmp/ferryx-src/src-tauri
cargo vendor --locked /tmp/vendor-ferryx

# macOS metadata must not ride along: the archive gets ._* junk and drops checksum files
tar --no-xattrs --no-mac-metadata -czf /tmp/vendor-ferryx.tgz -C /tmp vendor-ferryx

# the box needs rsync installed; macOS openrsync has no --append-verify
rsync -a --partial --append -e "ssh -o BatchMode=yes -o ServerAliveInterval=15" \
  /tmp/vendor-ferryx.tgz manus-sbx:/tmp/
```

Unpack and point cargo at it. Both the workspace root and `src-tauri` need the config, because the
two are separate cargo invocations:

```bash
tar xzf /tmp/vendor-ferryx.tgz -C ~/vendor
for d in ~/src/ferryx ~/src/ferryx/src-tauri; do
  cat > "$d/.cargo/config.toml" <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "/home/ubuntu/vendor/vendor-ferryx"

[net]
offline = true
EOF
done
```

This overwrites the repository's own `.cargo/config.toml` (`FFMPEG_DIR`, `CMAKE_POLICY_VERSION_MINIMUM`,
`OPUS_NO_PKG`). That is intentional for a headless build — `remote-desktop` stays off, so the
FFmpeg SDK is never linked — but do not copy the box's config back into the checkout.

## 3. Build the headless binaries

```bash
cd ~/src/ferryx/src-tauri
cargo build --offline --no-default-features \
  --features hub-slack,hub-discord,hub-voice \
  --bin ferryx-cli --bin ferryx-relay --bin ferryx-account -j6
```

Output lands in `src-tauri/target/debug/`. Copy the three binaries somewhere durable before any
cleanup; they are reproducible from the vendored directory in a few minutes.

### Why the extra `--features` list

`--no-default-features` alone does not compile at `35e91ab` (and still does not at local `HEAD`):

```
error[E0433]: cannot find `omo_client` in `meta`
   --> src/hub_runtime/seed.rs:162:23
```

`crates/ferryx-hub/src/meta/mod.rs` gates `pub mod omo_client;` behind
`#[cfg(any(feature = "hub-slack", feature = "hub-discord"))]`, while `hub_runtime/seed.rs` calls
`ferryx_hub::meta::omo_client::workspace_root_from` with no gate and `hub_runtime` itself is
unconditional in `lib.rs`. Enabling the three hub features keeps the module compiled and leaves
`desktop`/`remote-desktop` off, which is the point of the headless build. Drop the workaround once
the seed call is gated.

## 4. Verify

```bash
./target/debug/ferryx-cli --version        # ferryx-cli 2026.1101.1
```

`ferryx-account` and `ferryx-relay` refuse to start without configuration:
`ferryx-account` needs `FERRYX_DEPLOYMENT_MODE` (`commercial` or `selfhost`) plus
`FERRYX_ACCOUNT_ORIGIN`, `ferryx-relay` needs `FERRYX_DEPLOYMENT_MODE`. Getting as far as a bind
attempt means the binary is healthy.

## Disk budget

A 48 GB root is enough for one repository at a time, not two.

| Artifact | Size |
| --- | --- |
| `src-tauri/target` after a debug build | ~17 GB (12 GB `deps`, 4.1 GB `incremental`) |
| `vendor/vendor-ferryx` | 1.5 GB of crate sources |
| The three debug binaries | 2.9 GB together |

A second repository's `target` fills the volume and the build dies with
`No space left on device (os error 28)` partway through `deps`. Reclaim space, in this order:

```bash
rm -rf src-tauri/target/debug/incremental     # 4.1 GB, regenerated
cp target/debug/ferryx-{cli,relay,account} ~/bin/ && rm -rf src-tauri/target
```

## Sleeping and waking

When the Manus task is idle the sandbox suspends and `ssh` times out. Memory and disk survive; the
processes do not. Wake it by sending any message to the task in the Manus web app, then reconnect
once the node is back online (observed ~30 s after one such wake). Long builds therefore belong in
`nohup` with a log file, so a suspend does not lose the output — but a suspend **does** kill the
build, and it has to be restarted (incrementally, from the last compiled crate).
