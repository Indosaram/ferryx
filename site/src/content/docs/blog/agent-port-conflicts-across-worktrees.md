---
title: "Why Parallel Agents Fight Over Ports"
description: "Five agents, five dev servers, one port range. Learn why port collisions happen across worktrees and how to partition ports per branch safely."
---

**Why Parallel Agents Fight Over Ports.** Parallel agents collide on ports because dev servers default to fixed addresses — each worktree's server asks for `:3000`, the first one gets it, and every subsequent bind fails or (worse) silently serves the *wrong* worktree's stale code. The fix is port partitioning: assign each worktree a deterministic port (derived from its slot, branch, or hash) and propagate that assignment into the environment the server reads, so "which worktree does this tab show" is answered by construction rather than by remembering what you launched first — the per-session metadata a Ferryx workspace snapshot already carries.

![agent-port-conflicts-across-worktrees cover](/images/blog/agent-port-conflicts-across-worktrees/cover.png)

## How port collisions actually happen

Two distinct failures hide under "port conflict," and they behave differently:

**Hard bind failure.** Worktree A's dev server holds `:3000`; worktree B's server tries the same port and exits — or the framework, detecting the collision, silently hops to `:3001` (common in modern dev servers' auto-increment behavior). The visible symptom is B's terminal showing either a bind error or a URL you did not ask for. The *invisible* consequence matters more: you click what you think is B's preview and land on A's stale output, then debug a bug that was fixed an hour ago in the other tree.

**Soft collision — the port is "free" but contested.** A leftover server from a killed agent still holds the port (or lingers in TIME_WAIT); a new server binds a *different* port than your muscle memory expects; proxies, hot-reload clients, and printed URLs disagree about which port is canonical. Each agent run reintroduces this residue because agents, unlike humans, do not remember to kill yesterday's server before starting today's.

Why agents trigger this more than humans: an agent starts servers the way it starts everything — by following the repo's documented command (`npm run dev`, `cargo run --example server`), which encodes *one* fixed port, written for one developer running one instance. Scale that command to five concurrent worktrees and the assumption breaks. Worse, agents may not *notice* it broke: a test hitting `localhost:3000` against the wrong worktree's server passes green while validating nothing.

Underneath both failures is the same root: **the port lives in the repo's config (a shared constant) instead of in the environment (per-run state).** Partitioning moves it.

## Partitioning ports per branch

Three partitioning schemes, in ascending order of cleverness — and one recommendation:

**Static slot assignment.** Each worktree gets a slot number (01–15) from a registry — a file, the workspace manager's session metadata, or manual convention — and ports derive from it: `3000 + slot` → worktree on slot 7 serves `:3007`. Trivial to reason about, easy to grep, and the registry doubles as the fleet inventory (which tree owns which port). Its only failure mode is slot collisions, solved the same way as ports: allocate centrally or namespace slots per workspace.

**Hash-derived ports.** Hash the worktree path (or branch name) into a range: `PORT = 3100 + (hash(path) % 400)`. Zero registry, fully deterministic — anyone can recompute any tree's port — and stable across restarts as long as paths hold. The cost: occasional hash collisions (two trees landing on one port) need a probe-and-shift fallback, and humans cannot eyeball which tree owns `:3347`.

**Registry-assigned with liveness.** The workspace layer assigns from a pool *at session start*, records the assignment, and reclaims on session end — with a liveness probe before handing a port out (nothing answers → safe to reuse). This is what purpose-built workspace managers implement: the [workspace snapshot format](/blog/workspace-snapshot-format/) describes how Ferryx serializes session metadata — including per-session ports — so restore brings the *same* port mapping back with the layout.

**Recommendation: derive, don't hardcode — and write the mapping where the agent can read it.** Whichever scheme you pick, two properties make it work with agents rather than merely with humans: (1) the port must be an *input* to the launch command (`PORT=3007 npm run dev` or an env file per worktree), not something the agent memorizes; (2) the mapping must be *discoverable* — printed at session start, stored in the session registry, or encoded in the worktree's name — because the next agent you point at that tree needs to know which port answers without asking you.

## Detection when a server binds wrong

Partitioning prevents most collisions; detection catches the residue — servers from dead runs, manually launched strays, hash collisions. Three checks, cheapest first:

1. **Assert at launch.** Wrap the dev command so it prints the *expected* port and the *actual* listener side by side at startup. A mismatch (expected `:3007`, log says `:3001` auto-incremented) is a loud, immediate signal that something else holds the port — before any browser tab lies to you.
2. **Probe before trust.** A health check that fetches a worktree-identifying marker (version string, branch name endpoint, unique asset hash) proves the server on the port is *this* tree's server, not merely *a* server. UI-driven workflows (Playwright, preview bots) should carry this assertion — it converts "wrong tree's green tests" from silent corruption into a failing check.
3. **Sweep for orphans.** On session start or workspace restore, list listeners in the dev-port range and match them against live worktrees: a port whose owning worktree no longer exists is residue to kill; a port with no listener despite an active session is a crashed server to restart. This sweep is the maintenance the registry scheme automates for you.

None of the three requires heroics — they require treating "the port map" as state you own rather than folklore you half-remember. The fleet-level framing (which run is healthy, which is blocked) sits in the [cross-provider fleet model](/blog/cross-provider-agent-fleet/); the isolation floor beneath ports — one worktree per agent so these servers have *different code* to serve — is the [worktree-per-bugfix discipline](/blog/worktree-per-bugfix/) in miniature.

![Why Parallel Agents Fight Over Ports illustration](/images/blog/agent-port-conflicts-across-worktrees/body-1.png)

## FAQ

### Why does my dev server silently pick a different port instead of failing?

Most modern dev-server frameworks treat "port in use" as a soft condition and auto-increment (`:3000` → `:3001`) to keep a solo developer unblocked. That convenience inverts into a hazard under parallel agents: the incremented port means your preview URL, proxy config, and test `baseURL` now disagree, and stale output from the original holder masquerades as fresh. Disabling auto-increment (bind strictly, fail loudly) makes collisions visible at launch instead of at trust time.

### Can I just assign fixed ports manually per worktree?

Manual assignment works up to about two or three trees, then becomes the failure mode itself: ports are remembered inconsistently, resurrected servers hold "free" ones, and a returning teammate guesses wrong. The step up from manual is a *convention* — deterministic derivation (slot or hash) or a recorded registry — so the assignment is recomputable by anyone (or any agent) rather than held in one person's head.

### How do agents know which port their server got?

Make the port an input, not an output: launch with `PORT=<derived>` from the scheme, write it into the worktree's env file at session creation, and print it in the session banner the agent reads. Agents follow what the environment tells them far more reliably than what they infer from startup logs — and a [workspace manager](/blog/multi-repo-terminal-workspace-manager/) that records per-session metadata can restore the same map after reboot alongside the layout.

### Does this problem disappear with separate machines per agent?

The collision moves rather than disappears: separate hosts have separate port spaces (so `:3000` fights end), but you inherit *reachability* bookkeeping — which machine serves which worktree, which URL the preview proxy points at. Either way the invariant is identical: the port (or host:port) must be derived state tied to the worktree's identity, never a shared constant. On one machine the [architecture overview](/docs/architecture/) shows where per-session metadata lives to hold that map.
