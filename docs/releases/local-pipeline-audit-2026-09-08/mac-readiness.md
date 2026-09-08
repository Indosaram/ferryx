# Mac release prerequisite and readiness audit

Date: 2026-09-08. Executable recovery of the Mac readiness audit. This report supplements the lead probe evidence in probe-evidence.md without repeating established tool probes. No compilation, code signing, notarization, packaging, or app launch was executed.

## Verified prerequisites and machine state

- Host architecture (historic lead probe): Apple Silicon arm64 (observed via uname -m).
- Toolchains (historic lead probe): Bun 1.4.0, Node v22.22.3, Cargo/rustc 1.92.0, Zig 0.16.0, tauri-cli 2.10.1, Xcode 26.6 (build 17F113).
- Compilation targets (historic lead probe): rustup target list --installed confirms both aarch64-apple-darwin and x86_64-apple-darwin for universal binary builds.
- Signing identity (historic lead probe): security find-identity -v -p codesigning confirms valid "Developer ID Application: Indo Yoon (5DUM8WPB4C)".
- Working tree revision (newly executed check): git rev-parse HEAD resolves to commit 5d5499806a1b207849778f488b4e3e7b821a751b.
- Available disk space (historic vs newly executed check): df -h . reported 15Gi initially, 11Gi during lead inspection, and 10Gi on re-check on /System/Volumes/Data.

## Source configuration references

- Updater configuration: src-tauri/tauri.conf.json:34-43 specifies updater endpoint https://github.com/Indosaram/ferryx/releases/latest/download/latest.json and plugins.updater.pubkey.
- Contract test coverage: src-tauri/tests/updater_config_contract.rs:9,22-32 asserts plugins.updater.pubkey matches UPDATER_PUBKEY.
- Bundle configuration: src-tauri/tauri.conf.json:50-57,104-105 configures bundle targets (dmg, app), minimumSystemVersion 10.15, and hardenedRuntime true.
- macOS dev runner: src-tauri/tauri.macos.conf.json:3 specifies ../scripts/macos-dev-runner.sh, isolated from cross-platform base config.
- Manifest target mapping: scripts/build-latest-json.mjs:24-28 maps Ferryx.app.tar.gz to darwin-aarch64 and darwin-x86_64.
- Historical release precedent: docs/releases/v2026.09.05.1.md and v2026.09.06.1.md record universal builds with lipo verification and notarytool/stapler validation.

## Critical boundaries and unproven readiness

- Public-key equality does not establish private-key usability or installed-app key: String equality between tauri.conf.json and test constants verifies only repository config consistency. It does not prove operator access to or passphrase readiness for the private minisign key (TAURI_SIGNING_PRIVATE_KEY), nor does it prove what public key is baked into any currently installed app binary.
- Signing and notarization were not exercised: Certificate presence in the keychain establishes identity enrollment, but codesign binary signing, private key access permissions, and keychain prompt authorization were not tested. Apple notarization credentials (notarytool API key or keychain profile) were not exercised.
- Credential search boundary: Do not declare a missing credential from an unsearched path. Release signing keys, App Store Connect API keys, or keychain passwords may reside in operator-controlled paths, environment variables, or secure stores outside repository inspection boundaries.
- Disk capacity is a snapshot, not a certified build threshold: The 11Gi snapshot (and current 10Gi) represents an acute build-space risk, not a certified minimum threshold. Past release v2026.09.05.1.md noted 162 GB of stale build cache cleared from src-tauri/target; universal macOS compilation of Rust and vendored Ghostty VT slices requires substantial headroom.
- No build or runtime certification: No compilation, lipo slice verification, packaging, code signing, notarization, installation, or daemon execution was performed. Toolchain presence alone is not release certification.
