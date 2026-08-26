# C2 evidence - release tag becomes the app version

Command: `node --test scripts/sync-version.test.mjs`
Files: `scripts/sync-version.mjs`, `scripts/sync-version.test.mjs`
Captured 2026-08-26T13:26:18.212216

Why this exists: `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` both pinned `0.1.0` while
`.github/workflows/release.yml` publishes only `vYYYY.MM.DD[.N]` tags and never injected the tag.
An installed app always reported `0.1.0`, so updater version comparison could never succeed.

## RED (script moved aside, test unchanged)

```
$ mv scripts/sync-version.mjs /tmp/sync-version.mjs.hidden
$ node --test scripts/sync-version.test.mjs
# tests 6
# pass 1
# fail 5
```

The single passing case is the "missing --tag reports usage" test, which cannot fail without the
script present; the five substantive cases all failed.

## GREEN (script restored)

```
$ node --test scripts/sync-version.test.mjs
# tests 6
# pass 6
# fail 0
```

The test operates only on `fs.mkdtempSync` copies; the real manifests stay at `0.1.0` in the repo
and are rewritten by CI at release time.

VERDICT: PASS.


## Release tag -> valid updater SemVer correction

A release-readiness probe found that preserving the old date tag verbatim is invalid for Cargo/Tauri:
`cargo metadata` rejects `2026.08.26.1` (four numeric components and leading-zero numeric
identifiers). The release tag remains human-readable (`vYYYY.MM.DD[.N]`), but the bundled app and
`latest.json` now use the deterministic, monotonic valid SemVer mapping `YYYY.MMDD.revision`:

| Tag | App / manifest version |
| --- | --- |
| `v2026.08.26` | `2026.826.0` |
| `v2026.08.26.1` | `2026.826.1` |
| `v2026.09.01` | `2026.901.0` |

The test suite now has 13 passing cases, including monotonically increasing same-day, next-day,
next-month, and next-year releases. The release workflow derives `MANIFEST_VERSION` through the same
script, so the app version and `latest.json` cannot drift.
