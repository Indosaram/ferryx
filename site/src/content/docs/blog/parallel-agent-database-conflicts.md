---
title: "Parallel Agents and Database Collisions"
description: "Parallel agents sharing one dev database overwrite each other's migrations and fixtures. Isolate schemas or containers per worktree with this pattern."
---

**Parallel Agents and Database Collisions.** Parallel agents sharing one dev database collide at three levels — migrations stepping on each other's schema history, seed/fixture runs overwriting shared tables, and two transactions writing the same rows with last-writer-wins semantics — and the durable fix is *database-per-worktree*: give every agent its own schema, database, or container, provisioned automatically from the worktree's lifecycle and advertised through its environment, so isolation is a property of the checkout rather than a rule everyone remembers — the same lifecycle Ferryx uses to bind each session to its managed worktree.

![parallel-agent-database-conflicts cover](/images/blog/parallel-agent-database-conflicts/cover.png)

## Shared database failure modes

Agents hit the shared database harder than humans do because they *reset* more often. Three recurring failures, in the order they usually appear:

**Migration races.** Agent A runs `migrate` adding column `x`; agent B, in its own branch, runs a *different* migration adding column `y` — both against the same schema history table. Depending on your migration tool's locking, you get either a failed migration (loud, annoying) or two partially-applied heads (quiet, dangerous: each agent's green tests ran against a schema neither branch actually describes). Branch-shaped migrations assume branch-shaped databases; one shared schema violates the assumption structurally.

**Seed and fixture stomping.** Test suites and demo-data scripts typically `truncate` + `insert` — correct against *their* database, destructive against a shared one. Agent A's test run wipes rows agent B's server was serving, and B's symptoms (500s, empty lists) have no local cause: the cause lives in another worktree's test command. These bugs are famously hard to attribute because every tree's code is innocent.

**Write contention at the row level.** Two agents exercising the same entity — same user, same cart, same job row — interleave transactions. Lost updates, unique-constraint violations that only reproduce under concurrency, and dirty reads make both agents' observations unreliable: each is debugging against a database the other is mutating underneath them.

The common structure: **shared mutable state without per-actor fencing.** Worktrees isolate files and git state perfectly; the database is the one shared resource most setups forget to fence along the same boundary — which is why database collisions are the *second* fight (after git locks) every parallel fleet discovers.

## Schema or container isolation

Two isolation granularities, both correct; pick by tooling weight:

**Schema-per-worktree (lightweight).** One physical server, one namespace per worktree: `app_wt_authfix`, `app_wt_pagination`. Provisioning is one `CREATE SCHEMA` (or `CREATE DATABASE`) plus env pointing the app at it; cost is near zero; cleanup drops the namespace. Works when your stack respects schema boundaries cleanly (Postgres schemas, MySQL databases) and migrations are schema-scoped. The failure mode to verify: tools that hardcode the public schema or cross-schema search paths — test one real migration round-trip before committing the team.

**Container-per-worktree (heavyweight, hermetic).** Each worktree's compose file brings up its own Postgres/Redis/whatever on its own ports, volume scoped to the tree. Full version pinning per branch (agent A runs Postgres 14 for the legacy migration; agent B runs 16), complete teardown with `docker compose down -v`, and zero cross-talk by construction. The costs: disk and RAM multiply by concurrent trees, first-boot migrations add latency, and port partitioning (the [ports problem](/blog/agent-port-conflicts-across-worktrees/)) reappears for the database's own port — solve it with the same derivation scheme.

**The hybrid most fleets land on:** one long-lived *scratch* server with schema-per-worktree for everyday iteration (fast, cheap), plus container isolation reserved for branches that must pin engine versions or run destructive tooling (upgrades, down migrations). The deciding question per worktree: *does this branch need a different database than the trunk?* If yes, container; if no, schema.

## Wiring env vars per worktree

Isolation exists only if every process in the worktree *reads* the right connection string — the wiring step is where fleets leak:

1. **Derive the connection string from worktree identity.** Host, port, and database name all keyed off the slot/slug (`DATABASE_URL=postgres://localhost:5433/app_wt_<slug>`), the same derivation used for app ports — one scheme, two consumers.
2. **Write it where the app actually reads it.** Per-worktree `.env` (never committed — the `.env.example` documents the keys, the worktree's ignored `.env` holds values) or exported vars in the session's shell profile. Agents launching `npm run dev` inherit it without knowing it is special.
3. **Assert identity at startup.** The app (or a preflight check) prints the database name/banner on boot — a one-line "connected to `app_wt_authfix`" makes a mis-wired tree obvious in the first second, before tests assert against the wrong data.
4. **Provision from the worktree lifecycle, not from a wiki page.** If creating a tree requires a human to follow setup docs, agents will skip it. Tying provisioning (run the schema create, write the env file) to worktree/session creation is what makes isolation the default path — the pattern a [workspace manager](/blog/multi-repo-terminal-workspace-manager/) can own, and what Ferryx's managed-worktree lifecycle hooks are shaped to carry.
5. **Teardown with the tree.** Dropped schemas and removed volumes on worktree removal keep disk honest and prevent yesterday's `app_wt_tmp` from becoming tomorrow's mystery leak.

The payoff is attribution: when an agent's tests fail, the database is *its* database, so the bug is in its branch — the property worktrees already gave you for files, extended across the last shared resource. Repo-level isolation mechanics are covered in the [same-repo two-agent pattern](/blog/same-repo-two-agents/), and the fleet view of which run owns which resources is the [cross-provider fleet inventory](/blog/cross-provider-agent-fleet/).

![Parallel Agents and Database Collisions illustration](/images/blog/parallel-agent-database-conflicts/body-1.png)

## FAQ

### Isn't a database per worktree overkill for quick tasks?

For one-off branches that never touch data paths, yes — skip it. The trigger for spinning one up should be the agent *running tests or servers that touch the DB*, not the mere existence of a worktree. The cheap default is the scratch schema (near-zero cost), reserving container isolation for branches that need version pinning — so "overkill" is opt-in by branch requirement, not the baseline.

### How do migrations work if each worktree has its own database?

Each worktree runs its own migrations against its *own* schema on its own schedule — which is precisely the point: branch A's half-finished migration cannot corrupt branch B's history. Merge-time migrations get resolved the same way code conflicts do (one branch's migration wins, the other rebases), so the shared trunk's database always receives a linear sequence even though branches developed in parallel.

### What about Redis, RabbitMQ, and other stateful services?

Same fence, same patterns: Redis trivially (logical DB indexes or container-per-tree), brokers less so (virtual hosts or per-tree containers with partitioned ports). The decision rule is unchanged — shared mutable state gets a per-actor namespace — and the services that cannot namespace cleanly are the ones deserving container isolation by default.

### Can Ferryx provision database isolation automatically when I open a worktree?

The lifecycle hook is exactly where it belongs: worktree/session creation already runs provisioning steps (the managed [worktree model](/use-cases/git-worktree-workflow/) creates directory, branch, and env), and adding schema creation + env wiring extends the same step — no schema-aware feature is required, just provisioning tied to tree birth. The fleet registry then records which namespace each session owns, for teardown on removal.
