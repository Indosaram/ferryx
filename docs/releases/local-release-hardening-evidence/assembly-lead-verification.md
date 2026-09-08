# Lead release-assembly verification

Date: 2026-09-08. This supplements the inventory lane's initial evidence.

## Two additional copy-boundary regressions

After signature verification, mutating the source `.sig` during its real copy
could previously publish signature bytes inconsistent with `latest.json`.
The new regression changes only that copy boundary, preserves the real
filesystem copy, and requires a checksum failure with no publish directory.

The archive-layout checker previously read the mutable source path. The second
regression signs invalid archive bytes, temporarily substitutes a valid archive
only while that source-path checker runs, then restores the original bytes for
copying. The old code accepted the result. Validation now runs on the private
staged artifact after its hash check.

Fixture-name and path-normalization mistakes encountered while constructing the
tests were corrected first; they are not counted as RED evidence.

## Accepted RED

```sh
node --test --test-name-pattern='signature mutation during staging|archive layout is checked against staged bytes' scripts/build-latest-json.test.mjs
```

Monitor `mon_PTBD6B980BF919V4` / `bash_22`: exit 1, 2 tests, 0 passed,
2 failed, 0 skipped. Both failed with `Missing expected exception`.

## GREEN

The assembler checks copied signature hashes against the already-verified
signature text, including stable aliases. Archive-layout checks use the staged
path, not the original source path.

```sh
node --test scripts/build-latest-json.test.mjs scripts/release-contract.test.mjs
```

Monitor `mon_H128PJGEF3E3QQE3` / `bash_25`: exit 0, 40 tests,
40 passed, 0 failed/cancelled/skipped. This includes real CLI invocation and
filesystem fixtures, real Ed25519 verification, wrong identities, missing
inventory, path escapes, duplicates, tampering, aliases and checksum closure.

## Cleanup and scope

Temporary fixture and staging directories are removed in `finally` blocks;
failure cases assert no publishable output directory. No external release,
real signing key operation, application or daemon was invoked. A successful
fixture assembly is not a full three-machine Ferryx build certification.
