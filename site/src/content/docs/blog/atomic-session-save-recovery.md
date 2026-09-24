---
title: "Atomic Session Saves and Crash Recovery"
description: "Session state writes must survive crashes mid-save. How atomic writes, checksums, and backup rotation keep a corrupted layout from bricking startup."
---

**Atomic Session Saves and Crash Recovery.** The invariant that makes session persistence safe — the invariant Ferryx's workspace snapshots follow — is that a save must be *all-or-nothing* — write the new state to a temporary file, `fsync` it, then atomically `rename()` it over the live file — because `rename` within a filesystem is the one operation a crash cannot tear in half. Around that core sit three supporting layers: **checksums** (detect corruption that survived, from bit rot to partial disk conditions), **backup rotation** (keep N previous known-good states so recovery has somewhere to roll back to), and a **non-fatal startup path** (a corrupt file degrades to the last good backup — or a fresh state — never a launch loop). Together they turn "app crashed during save" from data loss into a non-event.

![atomic-session-save-recovery cover](/images/blog/atomic-session-save-recovery/cover.png)

## Why torn writes corrupt state

A non-atomic save looks like: open the live file, truncate it, write the new bytes, close. Every step is individually correct and the sequence is fatal — the window between truncate and close-last-byte is real time, and a crash (power loss, SIGKILL, kernel panic) in that window leaves a half-written file: valid JSON prefix, then nothing. The next startup parses it, hits EOF mid-object, and either errors out (you have *lost your layout* — the save meant to protect it destroyed it) or, with laxer parsing, loads a partially-filled structure whose missing fields default silently — corrupted state you discover later.

The subtlety is that *most* saves work fine — crashes during a multi-millisecond write are rare — which is exactly how torn writes survive code review for years. The bug's frequency is proportional to crash rate × save rate, and a session manager saves on every significant change (pane open, layout move, session start): thousands of vulnerable windows per day of active use. Rare-per-window × high-window-count = eventually someone loses their workspace, usually during the one crash where the layout was the thing they needed to recover *from* it.

Three properties of session state make it worse than losing a document: it is *the map* (without it, sessions and worktrees still exist but you cannot find them), it is *written at the worst time* (save-on-change correlates with the churn a crash interrupts), and it is *small enough to feel safe* (a 4 KB config gets the copy-paste-in-emergency treatment; a multi-workspace tree does not). The fix is mechanical and cheap — the same pattern databases and package managers have used for decades: never let the live state and the in-progress write be the same bytes.

**The atomic pattern in full:** serialize state → write to `state.json.tmp` in the same directory (same filesystem is a hard requirement for `rename`) → `fsync` the file (push bytes to durable storage, not just page cache) → `rename(state.json.tmp, state.json)` (atomic swap — observers see old-or-new, never half) → optionally `fsync` the directory (makes the rename itself durable). The crash matrix is then trivial: crash before rename → old file intact; crash after rename → new file intact; never "half of each."

## Atomic write and checksum flow

Checksums address the failure mode atomicity cannot: corruption *without* a crash during save — disk errors, memory bit flips, a truncated backup restored by hand, an editor's interrupted sync. The flow: each saved file carries a digest (CRC32 or SHA of its content, stored alongside or embedded); every load verifies before parsing — matching digest proceeds, mismatched digest triggers recovery instead of feeding garbage to the parser.

Two placement choices matter in practice: **embed vs. sibling** (an envelope `{ "checksum": …, "state": … }` keeps the artifact self-contained — preferred when users might copy the file; a `.sha256` sibling keeps the payload plain-JSON — preferred when external tools read it) and **verify-before-parse** ordering (checksum first means corruption fails *loudly* at a known boundary, not *weirdly* somewhere inside a half-understood structure — the difference between "state file corrupt, recovering from backup" and a confusing parser error three frames deep).

Checksums pair naturally with atomicity into one pipeline: *write new state atomically, then verify on every read.* Atomicity keeps crashes from producing corruption; checksums catch everything else and — critically — distinguish "corrupt" from "merely old," which is the input the recovery path needs to choose correctly.

## Recovery from backup rotation

Rotation answers the question corruption raises: *recover from what?* Keeping only the live file means a corrupt live file has no predecessor — so the save path maintains N previous good states (`state.json.1`, `.2`, `.3`, … or timestamped), rotating on each successful save, with the rule that **rotation promotes only verified-good states** (a save whose checksum passes becomes the new live file; the old live file demotes into rotation — never the reverse).

The recovery cascade at startup, in order:

1. **Live file loads and verifies** → normal startup (the overwhelmingly common path).
2. **Live file corrupt (checksum/parse fail)** → surface it visibly (log, UI notice: "recovered from backup — last save lost") and load the newest rotation entry that verifies. Losing the last few minutes of layout churn is an acceptable trade against losing the workspace; *silently* loading an old backup without notice is not — the user deserves to know time was rolled back.
3. **All backups corrupt or absent** → start with fresh state *while preserving the corrupt file* (`state.json.corrupt-<ts>`) for forensics, and reconstruct the map from ground truth: the session registry and worktree directory are themselves enumerable — sessions relaunchable, trees listed from disk. This is the key architectural point: **the layout file is a cache over reality, not reality itself** — a design where disk state (managed worktrees, session records) can rebuild the map makes even total state loss a degraded restart instead of a catastrophe.
4. **Corruption repeats across saves** → escalate, don't loop: repeated verify-failures after successful writes indicate environment problems (failing disk, memory errors, a sync client fighting the writes) and deserve a loud alert over another silent rotation cycle.

Rotation depth trades freshness against catastrophe coverage: three to five entries covers crash-during-save plus a bad-environment day without meaningful disk cost. The whole design — atomic envelope, checksums, rotation, ground-truth rebuild — is the machinery behind the formats the [workspace snapshot post](/blog/workspace-snapshot-format/) specifies, with the user-facing recovery walk-through in [recovering a corrupted layout](/blog/recover-corrupted-layout/).

![Atomic Session Saves and Crash Recovery illustration](/images/blog/atomic-session-save-recovery/body-1.png)

## FAQ

### Why not just write the file twice instead of using rename?

Rewriting the live file (even twice) still writes *in place* — each write opens the same inode and truncates it, so a crash mid-any-write still tears the live state; duplicating the bug doesn't remove the window. The atomicity comes from `rename` specifically: within a filesystem it updates the directory entry in one indivisible step, so readers see the complete old file or the complete new file — in-place writes of any count never gain that property.

### Does fsync actually matter, or is rename enough?

It matters on real hardware: without `fsync` on the temp file before rename, the renamed file's *bytes* may still live only in page cache — a power loss after rename can leave the swap target existing but empty/short (the fsync-less variant of tearing). The full-safe order is fsync(file) → rename → fsync(dir); skipping the first risks empty-after-crash, skipping the second risks the rename itself not surviving. On modern journaled filesystems the residual risk shrinks but does not vanish — for state whose loss is costly, the complete sequence is cheap insurance.

### How big should the backup rotation be?

Three to five entries covers the realistic threats: crash-during-save (one bad generation), a corrupted batch from a bad disk day (two or three), and still leaves headroom before rolling over the oldest good state — at typical session-save sizes this is kilobytes of disk. Depth beyond ~10 rarely helps (if ten consecutive generations are suspect, the problem is environmental and step 4's escalation is the right response); depth of one is the fragile configuration — it protects against exactly one bad save.

### Can I rebuild my workspace if every state file is gone?

Yes, if the design keeps ground truth in enumerable places — and that redundancy is the real safety net: managed worktrees list from disk (`git worktree list` / the managed root), session definitions from the session registry, layouts re-derivable from workspace templates. The state file accelerates restore; it is not the sole source of truth. Walkthrough of the rebuild path (including preserving corrupt files for diagnosis) is in [recovering a corrupted layout](/blog/recover-corrupted-layout/).
