# Manual desktop QA script (user-performed)

OS-level automation of the user's desktop is prohibited by standing instruction,
so the criteria below cannot be closed by this session. They need a human at the
machine. Everything here runs against the **isolated continuation worktree**, not
the canonical checkout, and never restarts the user's running daemon.

## Launch (exactly this, nothing else)

```bash
cd /Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8
bun tauri dev
```

Debug only. Do not build a release bundle, do not replace `/Applications/Ferryx.app`,
and do not kill `ferryx --daemon` — it owns every live PTY.

Two paired machines are needed for the multi-host rows. If only one machine is
available, run the single-host rows and mark the rest "not attempted" rather than
guessing.

## Rows to check

### AC01 - Add Project offers three kinds
1. Open Add Project. Confirm **Local**, **SSH**, and **Paired Daemon** are all offered.
2. Choose Paired Daemon, pair a machine, and browse its home directory.
3. Confirm browsing needs **no SSH credentials** and no remote desktop GUI.
4. Try a directory outside the permitted set; it must be refused, not silently empty.

### AC03 - Coexistence and disambiguation
1. Register a Local project, an SSH project, and a paired-daemon project at the same time.
2. Use the **same folder name** (and ideally the same absolute path) on two different machines.
3. Confirm the Sidebar keeps them as separate rows, each with a **machine label** and a
   **connection status**, and that selecting one never switches the other.

### AC04 - Tabs, splits, and the native pane
1. In a paired-daemon project open several tabs and split panes.
2. Exercise: focus follow, search, copy and paste text, keyboard shortcuts, native menus.
3. Confirm each new shell starts in the **selected remote root or managed worktree**,
   not in a local path.
4. Drag a pane/tab; the native surface must not go black or unmount.

### AC06 - Remote agents
1. Launch a coding agent inside a remote shell.
2. Confirm it actually runs on the remote machine (check its `hostname`/paths).
3. Confirm the agent's activity indicator and provider-session reference are attributed to
   the correct **host and pane**.
4. Confirm no remote path is probed as if it were local (no spurious "missing folder" errors).

### AC07 - Lifecycle is non-destructive
Do each of these while at least two sessions are live on different machines, and after each
one confirm **no other session died and no shell was recreated**:
1. Switch projects.
2. Change the selected machine.
3. Open and close Settings.
4. Close a renderer window.
5. Drop the relay connection (turn off networking briefly).
Then reconnect: it must **attach to the original target** or clearly report it expired.

### AC11 - Offline behavior
1. Take a paired machine offline.
2. Confirm its projects and layout snapshots **stay visible**, marked unavailable.
3. Confirm an empty/failed inventory is **not** shown as an authoritative empty list.
4. Confirm no operation silently falls back to Local or SSH.

### AC12 - Compatibility and rollback
1. Run a mixed set of versions across relay / remote daemon / local daemon / desktop /
   legacy mobile client.
2. Roll back one component and confirm the rest keep working.
3. Confirm **no unrelated developer or user daemon is terminated** at any point.

## What to report back

For each row: pass / fail / not attempted, plus what you saw when it failed.
Screenshots help for AC03 (labels and status) and AC04 (splits and focus).
This file is the checklist of record; results will be appended to the final report.
