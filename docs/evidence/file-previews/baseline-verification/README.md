# Independent baseline re-verification receipt — preview prerequisite `d0aee651`

Produced by adversarial verification task `st_01a0a3cf` (child "hephaestus"), parent/root session
`01a0a2ca-97b7-70f2-910f-2eb9b1cd2625`. Date 2026-09-15. Machine: darwin arm64 (Apple M4 Max).

These are **not** the setup task's baseline logs (those live untracked at
`.omo/ulw-execute/preview-baseline/`). These three logs are an *independent re-run* captured by the
verifier in the same worktree, because the setup logs record command output but no exit status.
Files here are byte-identical copies of the verifier's temporary logs (`cmp -s`, no output);
the temporaries were deleted after copying.

## Target state at capture time

| Item | Value |
| --- | --- |
| Worktree | `/Users/indo/code/project/orca-lite-wt/preview` |
| Branch | `preview` |
| HEAD | `d0aee65109c4490ec6d6ff22b21475b17e10898a` |
| Base | `c7e4414fb6a07c54355faeecf6640dfdd00c6e24` |
| Working tree | clean (`git status --porcelain` empty) before these evidence files were added |

## Exit-code receipt

All three commands were run in this worktree at HEAD `d0aee651` and their shell exit status was
captured directly (`echo "EXIT=$?"` in the verifier's shell, not embedded in the log files):

| Lane | Command (run from the worktree) | Result | Exit |
| --- | --- | --- | --- |
| UI tests | `cd ui && bun run test src/lib/fileLinkRegression.test.ts src/lib/linkRouting.test.ts src/components/NativeTerminalPane.test.tsx src/components/settings/TerminalSection.test.tsx` | `Test Files 4 passed (4)` / `Tests 206 passed (206)` | **0** |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml --lib file_link` | `test result: ok. 19 passed; 0 failed; 0 ignored; 1301 filtered out` | **0** |
| UI build | `cd ui && bun run build` (`tsc && vite build`) | `✓ built in 2.91s` | **0** |

| File | sha256 |
| --- | --- |
| `verify-ui.log` | `6985adc77db8202c171ba33261bfa21c59e1d849b65a1daeb0e62d30264c4bc6` |
| `verify-rust.log` | `6daea397228cf0b5e9d2e51ed6c47abc45a91e2ac5d513a2b614691cab615a2a` |
| `verify-build.log` | `ab73229594c26d4267a85755de69cf32dc4dd3096ef3f222b709934784d1eab9` |

Note on `verify-ui.log`: the stderr stack traces it contains are deliberate `console.error` output
from the "retries mount attach on rejection with exponential backoff" test. They are not unhandled
errors (`grep -i unhandled` matches nothing) and the run still exits 0.

## Disclosure — harmless reset in the TARGET worktree only

The target worktree's reflog contains
`c7e4414f HEAD@{2026-09-15 15:34:13 +0900}: reset: moving to HEAD`, recorded between the creation of
the `preview` worktree and the prerequisite commit. It moved HEAD to the same commit it was already
on, so no commit, file or index content was lost. It is noted here only because the setup report
(`.omo/ulw-execute/preview-setup.md`) does not mention it.

**The source repository `/Users/indo/code/project/orca-lite` was not reset.** It was observed
read-only and remained on `main` at `c7e4414f` with its foreign uncommitted work intact; this reset
entry belongs exclusively to the `orca-lite-wt/preview` worktree's own HEAD.

## Scope of this receipt

Artifact persistence only: no product or source file was edited, no test was re-run to produce this
receipt, and no commit was made. Product files are owned by the preparation worker. These evidence
files are currently untracked in the target worktree.
