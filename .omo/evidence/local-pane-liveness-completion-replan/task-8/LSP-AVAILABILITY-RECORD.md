# Task 8 — explicit record: the language server was unavailable

**Plan clause satisfied:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
line 187 (Todo 8, Work): "Record unavailable LSP explicitly; remote compiler still required."

**Hole closed:** F1 audit `H-19` — `E/F1-PLAN-TO-ARTIFACT-AUDIT.md`, HOLES / Holes B, row H-19
("`Record unavailable LSP explicitly` absent from Task 8 evidence"; `grep -rin "lsp"` over
`E/task-8/**` returns no LSP-unavailable record).

**Author of this record:** the host-independent F1-hole lane (`st_01a108d2`), 2026-10-05. Nothing was
built, run or tested to produce it: it is assembled from the run's own artifacts by reading them.

**Status:** a factual evidence entry, written after the fact, that claims nothing the run did not do.

---

## 1. What was unavailable

No language-server diagnostics were obtained at any point in the Task 8 verification of this
candidate. The language server was **unavailable** in the environments the verification ran in, so
the "changed-file LSP before builds" step produced **no diagnostics artifact at all** — not an empty
result and not a clean result: no LSP run exists.

Two records inside the run say so in their own words. They are independent of each other (one is
Task 8's own pass-2 finding; the other is a Task 9 lane's disclosure in the execution ledger).

| # | Source | Verbatim |
|---|---|---|
| 1 | `E/task-8/pass2/lane-findings.md:16` — Task 8 pass 2, section "Verification environment" | "Foreign Mac helper_setup test violates requested exclusivity but separate target; untouched, exclusive-verifier-audit.md. No GUI/production/daemon mutation/product edits/commits by Task8. Shell LSP unavailable and evidence MJS TypeScript installation unavailable; evidence runner node syntax checks pass, no dependency installation." |
| 2 | `/Volumes/T9-Mac/project/ferryx/.omo/ulw-execute/local-pane-liveness-completion-state.md:1075` — Task 9 pass-5 lane disclosure | "Two disclosures from the lane: LSP unusable in that worktree (no diagnostics pass exists), and it ran read-only `ls`/`find` for navigation only — no project code executed." |

Both statements agree on the operative fact: **no diagnostics pass exists.**

## 2. In which environment

**Task 8 verification environments (record 1).** Every Task 8 gate ran against a staged source tree
on one of three remote hosts, not in the authoring worktree:

| Host | Staged source root (from `task-8/commands.jsonl` `cwd`) |
|---|---|
| mac (`maho-mac`, `I552267@CQFQ4P2LXK`) | `/Users/I552267/ferryx-pane-completion/source-<rev>` |
| linux (`omaki`, `indo`) | `/home/indo/ferryx-pane-completion/source-<rev>` |
| windows (`maho-win`, `desktop-1lapjmp\sook`) | `C:\Users\sook\ferryx-pane-completion\source-<rev>` |

The recorded toolchains on those hosts (`E/task-1/hosts.json`) contain a compiler and a bundler and
nothing that serves diagnostics:

- maho-win: `rustc 1.97.0`, `cargo 1.97.0`, `tauri-cli 2.11.4`, `git 2.55.0.windows.2`, `node v24.19.0`,
  `bun 1.4.0`, `zig 0.16.0`, `rustupDefault stable-x86_64-pc-windows-msvc`
- maho-mac: `rustc 1.92.0`, `cargo 1.92.0`, `tauri-cli 2.10.1`, `zig 0.16.0`, `git 2.54.0`, `node v22.23.1`,
  `bun 1.4.2`, `xcodebuild Xcode 27.0`
- omaki (linux): `rustc 1.98.0`, `cargo 1.98.0`, `zig 0.16.0`, `git 2.55.0`, `node v26.8.1`, `bun 1.4.0`

No `rust-analyzer`, no `typescript-language-server` and no equivalent appears in any host's recorded
toolchain. **Not established:** whether a server was installed-but-failed-to-initialize or was never
installed on each host. Record 1 says "unavailable"; record 2 says "unusable". Both mean the same
thing for this record: no diagnostics were produced.

**The authoring worktree (record 2).** Record 2's "that worktree" is the worktree the Task 9 lane was
editing (the candidate worktree family under `/Volumes/T9-Mac/project/ferryx-wt/`). It records the
same absence there: "LSP unusable in that worktree (no diagnostics pass exists)".

## 3. Negative evidence (why the absence is established, not assumed)

1. `grep -rin "lsp"` over `E/task-8/**` returns **no LSP record**. The only matches are unrelated
   false positives on other words (`helps`, `declared`, `paneHandleReach`) — no line records an LSP
   run, an LSP result or an LSP failure.
2. `task-8/commands.jsonl` holds **228 recorded command entries**; their ids are exclusively
   `ui-build`, `ui-split`, `ui-lifecycle`, `runner`, `local_split_reliability_[-list]`,
   `pane_liveness_[-list]`, `qa_barrier[-list]`, `handover[-list]`, `all-targets`, `full-lib`,
   `full-ui`, `unix-suspension[-list]`, `journal[-list]`, plus the A/B classification kinds
   (`baselineAB` 120, `candidateAssertionRecovery` 10). **Zero** entries are a language-server
   invocation.
3. No artifact anywhere under `E/` names an LSP tool call (`lsp_symbols`, `lsp_diagnostics`,
   `lsp_find_references`, `rust-analyzer`): the repository-wide search over
   `.omo/evidence/local-pane-liveness-completion-replan/` returns nothing.
4. **First-hand corroboration (2026-10-05, this lane).** While editing the two `scripts/**` files for
   the H-18 test inside worktree `C`, the language server was invoked on both files and failed to
   initialize in each case with:
   "Request initialize failed with message: Could not find a valid TypeScript installation. Please
   ensure that the \"typescript\" dependency is installed in the workspace or that a valid
   `tsserver.path` is specified. Exiting."
   That is the same absence records 1 and 2 describe, reproduced directly rather than inferred, and it
   is why this lane classified the H-20 sites by reading source instead of by asking a language
   server. It corroborates the *unavailability*; it does not establish which host or worktree each of
   the three Task 8 pass trees lacked it in.

## 4. What was used instead — the clause's second half

"remote compiler still required" was satisfied; the compiler is the substitute that carried the
type-level confidence the LSP would have carried, and it ran on the real candidate bytes:

- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` — exit 0 on all three hosts
  (linux at `70eefafe`, `P3/linux/rustK-gates.log:2`; mac at `70eefafe`, recorded in
  `P3/FINAL-VERDICT-70eefafe.md` mac table; windows at `39e722ce`, `P3/windows/rustH-windows.log:4`).
- `bun run --cwd ui build` (= `tsc && vite build`) — `P3/{mac,linux,windows}/logs/ui-build.log`
  (windows `NATIVE_EXIT=0`, `P3/windows/commands.jsonl:1`).

## 5. Non-claims

- No language-server diagnostics were produced by this run, and none is claimed anywhere in this
  record.
- No verification verdict in this run rests on LSP output. The verdicts rest on the recorded
  `cargo`/`bun` commands, their native exits and their raw logs.
- Nothing here retroactively upgrades the skipped LSP step into a pass. The step did not run; that is
  what is now recorded, as the plan required.
