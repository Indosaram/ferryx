# Contributing to Ferryx

Thanks for helping. Before you open a pull request:

1. **Sign the CLA.** Every contributor signs the [Contributor License Agreement](CLA.md) once. When
   you open your first pull request, a bot asks you to comment
   `I have read the CLA Document and I hereby sign the CLA`. The pull request cannot be merged until
   you do. The CLA lets the maintainer change Ferryx's license in the future, including to
   commercial terms; you keep the copyright in your work.
2. **Read `AGENTS.md`.** It lists the project conventions and the patterns that are not accepted.
3. **Describe the user-visible result** in the pull request and how you checked it.

Ferryx is source-available under the license in [`LICENSE`](LICENSE). It is not OSI open source.

## Build prerequisites

- [Bun](https://bun.sh/) 1.1+ (the only JavaScript runtime the repository uses; Node.js is not needed)
- [Rust](https://www.rust-lang.org/) stable (at least 1.89, the `rust-version` in `src-tauri/Cargo.toml`)
- [Zig](https://ziglang.org/download/) 0.16.0 (required for the pinned Ghostty build)
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/) as a Cargo subcommand, since `cargo tauri dev` runs it:
  `cargo install tauri-cli --version "^2" --locked`

On Linux, install the GTK 3, WebKitGTK 4.1, and other system libraries required by
[Tauri](https://v2.tauri.app/start/prerequisites/) before compiling the desktop app. The
headless CLI does not need them:
`cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features --bin ferryx-cli`.

Clone and install:

```bash
git clone https://github.com/Indosaram/ferryx.git
cd ferryx
git submodule update --init --recursive
bun install
bun install --cwd ui
bun install --cwd site
```

## Tests

JavaScript and TypeScript run on Bun only: use `bun install`, `bun run`, `bun test`, and `bunx`,
never `node`, `npx`, or `npm`.

```bash
# Frontend build and tests
bun run --cwd ui build
bun run --cwd ui test

# Rust core
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib

# Site and docs
BASE_URL=/ bun run --cwd site build
```

Contributions are accepted under the [Ferryx License](LICENSE) and require the
[Contributor License Agreement](CLA.md) described above.
