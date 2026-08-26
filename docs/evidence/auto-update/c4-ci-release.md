# C4 evidence - signed release contract and latest.json manifest

Files: `.github/workflows/release.yml`, `scripts/build-latest-json.mjs`,
`scripts/build-latest-json.test.mjs`, `scripts/release-workflow.test.mjs`
Captured 2026-08-26T13:26:18.212216

## Part A - manifest generator

RED (generator absent):
```
$ node --test scripts/build-latest-json.test.mjs
# tests 6
# pass 0
# fail 6
```

GREEN (generator implemented):
```
$ node --test scripts/build-latest-json.test.mjs
# tests 6
# pass 6
# fail 0
```

Covered: universal macOS bundle mapping to both darwin targets, arch-specific bundles claiming only
their own target, windows `.nsis.zip` and linux `.AppImage.tar.gz` mapping, an unsigned artifact
being skipped with a stderr warning, notes pass-through, and an empty directory exiting non-zero so
a broken release fails loudly instead of publishing an empty manifest.

## Part B - release workflow contract

RED (workflow unmodified):
```
$ node --test scripts/release-workflow.test.mjs
not ok 1 - the bundle step receives the updater signing secrets
ok 2 - the macOS unsigned fallback never unsets the updater signing key
not ok 3 - both bundling jobs stamp the release tag into the app version
not ok 4 - signatures and updater bundles are collected as release artifacts
not ok 5 - the publish job generates latest.json into the uploaded directory
not ok 6 - release workflow never runs from a tag push
# pass 2
# fail 4
```

Assertions 2 and 6 pass in RED by design: they pin invariants that must survive the change.

GREEN (workflow updated):
```
$ node --test scripts/release-workflow.test.mjs
ok 1 - the bundle step receives the updater signing secrets
ok 2 - the macOS unsigned fallback never unsets the updater signing key
ok 3 - both bundling jobs stamp the release tag into the app version
ok 4 - signatures and updater bundles are collected as release artifacts
ok 5 - the publish job generates latest.json into the uploaded directory
ok 6 - release workflow never runs from a tag push
# pass 6
# fail 0
```

The release workflow is `workflow_dispatch` only. This prevents the tag created by the local
`gh release create --target <commit>` command from invoking GitHub Actions; the production release
build and upload are local. The workflow remains available for an explicit maintenance dispatch,
but it is not part of this release path.

YAML still parses:
```
$ python3 -c "import yaml; workflow=yaml.safe_load(open('.github/workflows/release.yml')); assert workflow['on'] == {'workflow_dispatch': None}; print('yaml-ok trigger=workflow_dispatch')"
yaml-ok trigger=workflow_dispatch
```

The repository's pre-existing CI ordering gate still passes, which matters because a new
`sync-version` step remains available to explicitly dispatched maintenance builds:
```
$ python3 script/qa/ci-step-order-gate.py
PASS: every building job has zig + submodule init before its build step
exit=0
```

## Local operator action required

The local release environment needs `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, or bundles ship unsigned and the updater rejects them. No
key material appears in this repository; only the public key is committed, in
`src-tauri/tauri.conf.json`.

VERDICT: PASS.
