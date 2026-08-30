# Windows Atomic History Correction

Corrected branch: `windows-atomic-history`

Corrected evidence head before this report: `97f33fa`

The original local `main` branch was not rewritten. A separate local-only branch was created from `9266644`, immediately before the mixed commit `dc17931`.

## Atomic split

The mixed commit was replaced by two independent commits:

- `b59a6b1` `fix(windows): normalize Git worktree paths`
  - `src-tauri/src/worktree/git.rs`
- `7ffa7a4` `test(native-terminal): cover Windows child compositor`
  - `src-tauri/tests/native_terminal_surface_host_contract.rs`

Every corrected commit includes:

```text
Ultraworked with [omo](https://github.com/code-yeongyu/oh-my-openagent)
Co-authored-by: sisyphus-dev-ai <sisyphus-dev-ai@users.noreply.github.com>
```

## Corrected implementation history

- `b59a6b1` fix(windows): normalize Git worktree paths
- `7ffa7a4` test(native-terminal): cover Windows child compositor
- `7de71cc` fix(build): restore native-terminal feature-off compilation
- `6ecd743` fix(windows): read native terminal clipboard
- `cb2358d` fix(native-terminal): repair search on child surfaces
- `d70f434` fix(remote): resolve bundled SPA assets
- `74008c7` fix(windows): enforce exclusive daemon locking
- `9cb363e` test(qa): verify Windows PTY output and cwd
- `41a2461` test(windows): compare launcher paths portably
- `91eb17c` test(qa): parse Windows PTY cwd output
- `a317255` test(qa): honor Windows E2E repo root

## Tree identity proof

The previously native-Windows-tested implementation commit and the corrected atomic implementation commit have the same Git tree object:

```text
76ac81a tree = 96b6443049c198fe58c04780ed8f7804740a040b
a317255 tree = 96b6443049c198fe58c04780ed8f7804740a040b
```

Therefore every production file, QA script, and generated-resource input at `a317255` is byte-identical to the implementation that passed the native Windows build, focused tests, live PTY/CWD probes, duplicate-daemon edge, and baseline comparison.

## Corrected-branch verification

- UI production build: PASS.
- Focused UI Vitest suite: PASS, 23 tests.
- `cargo check --no-default-features`: PASS.
- CLI launcher tests: PASS, 13 tests.
- Git verbatim path test: PASS.
- Native terminal surface contract: PASS, 17 tests.
- QA script syntax and self-test: PASS.
- Stale old-hash scan of corrected evidence: no matches.
- Attribution scan of every corrected commit: PASS.

## Windows checkout

The clean isolated checkout `C:\Users\sook\ferryx-ulw-01a04fcf` was moved to branch `windows-atomic-history` at corrected head `97f33fa`, with clean status and 10.96 GB free. No user dirty-checkout process or file was modified.

## Verdict

The explicit atomic-commit requirement is satisfied on `windows-atomic-history` without destructive rewriting of `main` or any published reference.
