# XENOCAST_MACOS_BUILDS

Build and test ferryx on a rented XenoCI Mac mini M4 from any machine or AI
agent, using XenoCast (CLI + MCP server, https://github.com/Xeno-CI/xenocast).
This is the same Mac class that runs the `xenoci-macos` GitHub Actions precheck.

## One-time setup (per machine)

```sh
curl -fsSL https://github.com/Xeno-CI/xenocast/releases/latest/download/install.sh | sh
xenocast login        # open the link, sign in with GitHub, approve the device code
xenocast macs         # rented Macs and remaining time
```

Login is stored in the OS keychain (or an encrypted file on Linux); no tokens
live in this repo. Renting, extending and paying for a Mac is manual:
https://xenoci.com/app/store.

## Run the macOS precheck from anywhere

```sh
xenocast build --repo Indosaram/ferryx --ref <branch> --script ci.sh
```

`ci.sh` mirrors the `precheck` job in `.github/workflows/build-test.yml`:
toolchain (Rust, Bun, Zig 0.16.0), submodules, UI build, lint gates, and the
Rust unit + integration suites with the same skips. ~15-20 minutes on a lite
rental; the command's exit code is the build's exit code.

Use `--repo` mode, not a local-folder upload: uploads carry the working tree
without `.git` or dotfiles, and the fixture suites need a real checkout that
resolves `HEAD~1`.

Follow a build: `xenocast status <rb_id>`, `xenocast logs <rb_id>`,
`xenocast cancel <rb_id>`, `xenocast watch`. Results (xcarchive/ipa) via
`--artifacts` + `xenocast artifacts <rb_id> --out ./dist` (7-day retention).

## MCP for AI agents

Repo-level MCP configs are included, so agents opened in this repo get the five
`xenoci` tools (`build`, `status`, `cancel`, `logs`, `macs`) after a local
`xenocast login`:

- `.mcp.json` - Claude Code (project scope)
- `.cursor/mcp.json` - Cursor
- `.codex/config.toml` - Codex CLI

The configs call `npx -y -p github:xeno-ci/xenocast xenoci-mcp` (Node only).
For other agents or user-global registration, run `xenocast setup` on the
machine (`--agent=claude|cursor|codex|opencode|omo`); it merges the `xenoci`
server into the agent's own config without storing tokens.

## Egress allowlist and the zig package cache

XenoCI build VMs route outbound traffic through an allowlist proxy (blocked
requests return `403 Egress Blocked` with `Proxy-Authenticate: Basic
realm="xeno-ci"`). github.com, codeload, npm and rustup are allowed, but
ziglang.org, the zig community mirrors and deps.files.ghostty.org are not -
so ferryx's ghostty zig dependencies cannot be fetched during a build.

Workaround: pre-seed zig's package cache into the rental once, and let ci.sh
redirect zig's global cache to it via a `~/.local/bin/zig` wrapper. The VM's
`$HOME` persists across builds while `keep_cache` is on, so one seed lasts
the whole rental.

Seeding (once per rental; needs ~35 MB of tarballs matching the pinned
ghostty commit's build.zig.zon):

1. On any machine with network access, run `zig fetch --global-cache-dir
   ferryx-zig-global-cache <url-or-path>` for every dependency URL in
   ghostty's `build.zig.zon` (plus the zon of each dep that has one). The
   heavy, unused-in-vt deps (harfbuzz, gettext, fonts, themes, wayland,
   gobject, plasma, zf) can be skipped.
2. Tar the resulting `p/` directory and copy it to the VM, e.g. via a tiny
   one-shot `xenocast build` local-mode upload whose script untars it into
   `$HOME/ferryx-zig-global-cache` (local uploads exclude dotfiles and cap
   around 50 MB; the minimal cache is ~34 MB).
3. Rerun the normal build. ci.sh detects the cache and installs the wrapper.

If the rental expires, the cache dies with the VM - reseed on the new rental
the same way. The same applies to a VM reset (`xenocast reset` without
`--keep-cache`) and to platform reprovisioning: a wedged build whose VM comes
back with a fresh home directory needs the full reseed (zig + cache). One such
reprovision was observed on 2026-10-11 after cancelling a hung build.

## Caveats

- The build VM egress allowlist blocks ghostty's zig dependency hosts; the
  seeded cache above is required for a green build. A fresh rental needs a
  fresh seed.
- Builds consume rented-Mac time; check `xenocast macs` before long runs.
  Extensions and new orders are paid and done by a human, never by the agent.
- iOS signing, signed archive export and TestFlight upload
  (`xenocast ios archive|sign|ship`) are unverified upstream - do not use.
- Fork-PR token safety notes for wiring XenoCast into GitHub Actions with a
  `XENOCAST_TOKEN` CI token live in the upstream SETUP.md; ferryx's existing
  `xenoci-macos` runner precheck already covers PRs, so no CI token is
  configured here.
