# Public auto-update release E2E audit

Captured: 2026-08-26

## Scope

This audit verifies the public release endpoint and its macOS updater asset after
the local updater implementation, signing, and notarization work. It does not
claim that the final physical in-app install/relaunch interaction was performed;
that step remains recorded below as pending user-operated E2E.

## Source and publication facts

| Requirement | Observed evidence | Result |
| --- | --- | --- |
| Date tag exists remotely | `v2026.08.26.1` resolves to `83acc75b620023b344264f8ecc0fd87477837ef3` | PASS |
| Public release exists | [Ferryx v2026.08.26.1](https://github.com/Indosaram/ferryx/releases/tag/v2026.08.26.1) is published, non-draft, and non-prerelease | PASS |
| Release source matches tag | GitHub Release `targetCommitish` is `83acc75b620023b344264f8ecc0fd87477837ef3` | PASS |
| Main contains the CTA clarification | `origin/main` is `49b8a5a`, which is after the release tag | INFORMATIONAL |

The later `49b8a5a` UI commit makes the downloaded-state **Install and Relaunch**
button primary. It is not part of the immutable `v2026.08.26.1` release source
or asset and therefore requires a separately versioned release if that UI change
must be distributed.

## Live updater manifest and assets

The live endpoint is:

```text
https://github.com/Indosaram/ferryx/releases/latest/download/latest.json
```

It returned a `2026.826.1` manifest with one `darwin-aarch64` platform entry:

```text
https://github.com/Indosaram/ferryx/releases/download/v2026.08.26.1/Ferryx_aarch64.app.tar.gz
```

The public release exposes all required updater assets:

| Asset | SHA-256 | Observed result |
| --- | --- | --- |
| `latest.json` | `2e26d6e390e6fc012657f31889fb178bccd93213c2b780e2b46340a392667e6e` | Downloaded and parsed |
| `Ferryx_aarch64.app.tar.gz` | `45dfd0e930bce0ccab84737243a438e5cc2f6234b9d886e8e4e636e6c2c6b3e7` | Downloaded; clean `Ferryx.app` archive |
| `Ferryx_aarch64.app.tar.gz.sig` | `a107c5040628a42216941c9e4dd9b73b9c82a1b6e70b03aea63f85ab5e70aaf6` | Downloaded; 416 bytes |

`latest.json` stores the decoded minisign payload in its `signature` field.
The sibling `.sig` file is the base64-wrapped encoding of that exact payload;
decoding the sibling produced a byte-for-byte match with the manifest signature.

## Public asset repair and revalidation

The first published macOS updater archive contained an AppleDouble
`._Ferryx.app` entry and could not be unpacked by `tauri-plugin-updater`. The
three coordinated public updater assets were replaced on 2026-08-26 with a
clean repack of the exact same already-notarized `2026.826.1` `Ferryx.app`:
the archive, its freshly generated matching Tauri Minisign signature, and then
`latest.json` last. The release tag and manifest version remain unchanged.

Fresh downloads of the repaired live assets passed all of the following checks:

```text
$ node scripts/assert-updater-archive-layout.mjs Ferryx_aarch64.app.tar.gz
updater archive layout OK: 22 entries

$ <Tauri Minisign-compatible verifier>
LIVE_TAURI_MINISIGN_VALID
```

The Minisign verifier decodes the public key embedded in
`83acc75b620023b344264f8ecc0fd87477837ef3:src-tauri/tauri.conf.json`, checks
the key ID `cb675cc2b59dc910`, verifies the signature over the archive's
BLAKE2b-512 prehash, and verifies the trusted-comment global signature. This
matches the verification behavior of `tauri-plugin-updater`; a raw Ed25519
check over the archive bytes would be incorrect.

## Fresh executable checks

```text
$ cargo test --manifest-path src-tauri/Cargo.toml --test updater_endpoint_contract
running 3 tests
test an_older_release_is_not_offered ... ok
test a_tampered_artifact_is_rejected_by_the_plugin ... ok
test a_newer_signed_release_is_offered_and_its_signature_accepted ... ok
test result: ok. 3 passed; 0 failed
```

The downloaded public archive was also checked directly:

```text
$ codesign --verify --deep --strict --verbose=2 Ferryx.app
... satisfies its Designated Requirement
$ xcrun stapler validate Ferryx.app
The validate action worked!
$ spctl --assess --type execute --verbose=4 Ferryx.app
... accepted
source=Notarized Developer ID
```

## Release-workflow provenance

`v2026.08.26.1` was published by the documented direct, verified local-build
upload path, not by an Actions run. GitHub has no `Release Ferryx` workflow run
for this date tag.

At the time this release was published, `.github/workflows/release.yml` was
`workflow_dispatch`-only, so `v2026.08.26.1` used the verified direct-upload
path. A subsequent updater-only source repair adds date-version tag-push
triggers for future releases and retains manual dispatch when the same tag ref
is selected. This does not alter or invalidate the verified live assets above.

## Initial physical E2E result: failed archive extraction

The disposable old app at
`/Users/indo/code/project/orca-lite-manual-update-retest/Ferryx.app` was run
against the public release on 2026-08-26. The updater reached the installation
step but displayed:

```text
Update failed: failed to unpack `._Ferryx.app` into
`.../tauri_updated_app...`
```

The screenshot is captured at:

```text
/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-26-232113-83C27009.png
```

The initial live artifact was cryptographically valid but structurally invalid
for the Tauri macOS installer: its raw tar headers contained the single-component
AppleDouble payload `._Ferryx.app`. `tauri-plugin-updater` removes the first
archive path component before extraction, causing that file to collide with its
already-created extraction root. macOS `tar -tzf` hides this metadata entry,
so `scripts/assert-updater-archive-layout.mjs` now parses raw tar headers and
rejects it reliably.

The source release workflow now removes physical `._*` files before Tauri
bundles the app and checks every generated `.app.tar.gz` before it is uploaded.

## Remaining physical E2E gate

The currently public `v2026.08.26.1` archive is the repaired archive described
above, not the archive that produced the recorded failure. A user-operated
retry from the disposable `0.1.0` app remains the only open E2E requirement:

1. Open **Settings > General > Software Update**.
2. Click **Check for Updates** and confirm `2026.826.1` is offered.
3. Click **Install and Relaunch** once; confirm download progress, automatic
   replacement, and relaunch without the `._Ferryx.app` extraction error.
4. Confirm the relaunched app reports version `2026.826.1`.

The result and a screenshot must be appended here before this public in-app
E2E audit can be marked complete.
