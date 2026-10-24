# Security

## Release provenance

Ferryx releases are built on operator-controlled machines, not on hosted CI. Every build uses one
40-character source commit, recorded in the release's `provenance.json`:

| Host | Builds |
| --- | --- |
| `macbook` (macOS, release coordinator) | universal updater archive, `Ferryx_universal.dmg`, `ferryx-cli-darwin-universal`; also assembles, signs and publishes the release |
| `omaki` (Linux x86_64) | `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `ferryx-cli-linux-amd64` |
| `maho-win` (Windows x64) | `Ferryx_x64.msix`, and the NSIS installer when it is shipped |

Each GitHub release carries:

- `SHA256SUMS`: the SHA-256 of every other asset.
- `SHA256SUMS.minisig`: a minisign signature of `SHA256SUMS`, made with the same key that signs app
  updates (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`).
- `ferryx-src-tauri.cdx.json` and `ferryx-ui.cdx.json`: CycloneDX SBOMs for the Rust app and the UI.
- `provenance.json`: the commit, tag, Ghostty pin, `sourceDateEpoch`, and per host the toolchain
  versions it reported (node, bun, zig, rust) and the assets it built.

The reproducible build inputs are that commit, the committed `src-tauri/Cargo.lock` and
`ui/bun.lock`, the Ghostty pin and the recorded toolchain versions.

To verify a download, put it in one directory with `SHA256SUMS` and `SHA256SUMS.minisig`, then run:

```bash
minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm
shasum -a 256 -c SHA256SUMS --ignore-missing
```

On Linux, `sha256sum -c SHA256SUMS --ignore-missing` works the same way. Both commands must exit 0.
If either one fails, do not run the file. The full operator procedure is in
[docs/releases/LOCAL_RELEASE_RUNBOOK.md](docs/releases/LOCAL_RELEASE_RUNBOOK.md#release-provenance).
