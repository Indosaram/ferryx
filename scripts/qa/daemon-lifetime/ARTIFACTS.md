# Run desktop scenarios without rebuilding the QA binaries

The default `--all` / `--only` invocation still builds and tests in one pass. To separate the two, run `--build-only` on the authorized native builder. Use a clean checkout, an unused UTC run ID and a **new directory outside the repository and run roots**. The example is the command on the builder; use the project's build wrapper to launch it. It never launches Ferryx or connects to a desktop.

```sh
QA=scripts/qa/daemon-lifetime/run.mjs
RUN=$(date -u +%Y%m%dt%H%M%Sz)
SET="$HOME/code/ferryx-wt/.evidence/qa-sets/$RUN"
bun "$QA" --platform mac --run-id "$RUN" --build-only "$SET" \
  --v1-worktree "$V1_WORKTREE"
```

Record `runId` and `manifestSha256` from the **trusted producer's output**. Transfer `artifact-manifest.json` and the complete `artifacts/` directory together; preserve executable permissions, macOS bundle structure, and Windows DLLs. The producing checkout, OS, architecture, and optional historical revisions are part of the identity. `receipts/build.json` records build results, not scenario results. A source-tree hash, a release bundle or an older `build-summary.json` is not a substitute for the manifest and its independently recorded digest.

Linux producers need `patchelf` and the FFmpeg SDK used by the build. They copy the versioned libraries into each linked executable's `ffmpeg/` directory and set relative lookup paths on those private copies before hashing. The consumer needs the normal platform libraries, but neither `patchelf` nor the builder's FFmpeg SDK.

On the matching native target use the **same clean commit** and the baked `--run-id`. The scenario invocation does not install dependencies or call Cargo, even when verification fails. Pin the full `--v1-rev` for a set built with `--v1-worktree` (required by Q08b); `--baseline-worktree` and `--baseline-rev` work analogously for Q13. Omit these pairs when the set has no corresponding variant.

```sh
bun "$QA" --platform mac --run-id "$RUN" --reuse-artifacts "$SET" \
  --manifest-sha256 "$PIN" --v1-rev "$V1_SHA" \
  --self-check --evidence-dir "$EVIDENCE/verify"

FERRYX_QA_MAC_ACCOUNT="$QA_LOGIN" bun "$QA" --platform mac --run-id "$RUN" \
  --reuse-artifacts "$SET" --manifest-sha256 "$PIN" --v1-rev "$V1_SHA" \
  --only Q10,Q11,Q14 --evidence-dir "$EVIDENCE/attempt-1"
```

`--self-check` with `--reuse-artifacts` checks the copied payload without starting a GUI, daemon, or desktop scenario. For an actual run use an isolated desktop; on macOS, log in as the dedicated QA user. Use separate evidence directories for each attempt. The signed macOS bundle identifier embeds the run ID: attempts using one set must be sequential and the preceding run must have torn down its runtime directories and jobs. The runner works on private copies, so Q07's on-disk replacement never changes the saved set. A missing pin, wrong commit/platform/run ID, incomplete variant, failed signature check, or changed payload is a refusal, not a request to rebuild or a desktop PASS.
