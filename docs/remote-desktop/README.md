# Remote desktop design notes

Design and protocol notes carried over from MahoRD when its engine moved into
`crates/ferryx-rd/`. They describe the engine as it was built there, so they still use
the MahoRD names: `maho-host` is now `ferryx-rd-host`, `maho-client` is
`ferryx-rd-client`, and every `maho-*` crate is the matching `ferryx-rd-*` crate.

Inside Ferryx the daemon owns the host (`src-tauri/src/remote/remote_desktop.rs`),
pairing uses Ferryx machine grants instead of the MahoRD PIN, and the stream rides the
Ferryx end-to-end relay tunnel instead of `maho-relay`.
