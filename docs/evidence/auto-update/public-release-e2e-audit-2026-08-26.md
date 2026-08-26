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
| `latest.json` | `dc89444ec43a85a63e51f6b52df3ba8aaa36a97ad86073e2216a43a8962f0e6a` | Downloaded and parsed |
| `Ferryx_aarch64.app.tar.gz` | `fcb5932f0192fc7918edddf98b8809a97309278a80427343a46b5b0e6b5808a4` | Downloaded; contains `Ferryx.app` |
| `Ferryx_aarch64.app.tar.gz.sig` | `660f69816abe77b7aedb43a97e27bfa015c3d2a69e81aea03fc67b5ecbeaf1b5` | Downloaded; 404 bytes |

`latest.json` stores the decoded minisign payload in its `signature` field.
The sibling `.sig` file is the base64-wrapped encoding of that exact payload;
decoding the sibling produced a byte-for-byte match with the manifest signature.

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

## Pending physical E2E

The disposable old app at
`/Users/indo/code/project/orca-lite-manual-update-retest/Ferryx.app` remains
version `0.1.0`. Complete the documented user-operated sequence in
`docs/AUTO_UPDATE_MANUAL_E2E.md`:

1. Open **Settings > General > Software Update** in that old app.
2. Select **Check for Updates** and confirm `2026.826.1` is offered.
3. Select the highlighted **Install and Relaunch** control and wait for its signed
   download, install, and automatic relaunch.
4. Confirm the same app bundle reopens at `2026.826.1`.

Only that observed install/relaunch result can close the in-app E2E gate.
