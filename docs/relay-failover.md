# Hosted relay: warm standby, failover and status page

`relay.ferryx.dev` runs on two hosts:

| Role | Host | Account database | Litestream |
|------|------|------------------|------------|
| Primary | omarchy (`ferryx-relay.service`, behind the `omaki-relay` Cloudflare Tunnel) | writable | `replicate` to the object store |
| Warm standby | owner-provided VPS (same `ferryx-relay.service` unit) | read-only replica, refreshed every minute | disabled |

The account database is `<FERRYX_ACCOUNT_DATA_DIR>/account-store.sqlite3`
(`src-tauri/src/account/store_sqlite/mod.rs`, `STORE_SQLITE_FILENAME`). Never use
`relay.checka.cc`; the tooling rejects it.

## One writer, always

Exactly one host writes account data. The standby's data directory carries the marker file
`standby.readonly`. While it exists, `ferryx-relay`:

- refuses every non-GET account request with HTTP 503 and the typed body
  `{"code":"ACCOUNT_STORE_READ_ONLY","details":{"role":"standby"}}`;
- refuses every store write (`mutate_transaction`, `save`) with the same code, which also covers
  GET handlers that write;
- does not start the commercial suspension and billing notice sweepers;
- answers `GET /healthz` with `{"ok":true,"role":"standby"}` (the primary says `"primary"`).

Relay tunnels (control, data, browser sessions) work on the standby, so paired machines keep
working through a failover. On a commercial standby the entitlement check behind every tunnel
evaluates the replica without persisting the grace transition (and sends no transition notice);
the primary persists it once it is back. Sign-in, enrollment, grants and billing changes wait for the primary
or a promotion. The marker is created before the first restore and removed only by
`promote-standby`.

## Supervisor lease on the relay

The relay decides which machine of an account runs the supervisor. The decision is a lease
row per account in the account database (schema v3): `supervisor_leases` (holder, fence,
`expires_at`, last checkpoint id, released flag, reservation), `supervisor_lease_fences`
(the highest fence ever issued, never lowered) and `supervisor_relay_epoch` (one row, `0`
until the first promotion). Lease rows are never deleted, only marked released. The rules live
in `src-tauri/src/account/supervisor_lease.rs`:

- `TTL_SECS = 120`: every grant and renew is valid for 120 s of relay time.
- `RENEW_SECS = 20`: the holder renews every 20 s.
- `GRACE_SECS = 30`: after expiry, another machine still cannot take the lease for 30 s.
- A fence is `(relay_epoch << 32) | counter`. A new holder always gets a fence above every
  fence recorded for the account; the same holder keeps its fence within one epoch.
- Another machine gets a free lease (released, past `expires_at + GRACE_SECS`, or from an older
  epoch) only if it presents the last recorded checkpoint id, or the lease was released to it
  in this epoch, or it asks with `force`.

Daemons send the requests over their authenticated control channel. The relay takes the
machine and account from the control socket's key, never from the message. Requests are
`SupervisorLeaseAcquire`, `SupervisorLeaseRenew`, `SupervisorLeaseRelease` and
`SupervisorLeaseQuery`; the one reply is `SupervisorLeaseAck` with `outcome`
`granted | renewed | released | refused | unavailable` and a `reasonCode`. A relay that serves
leases lists `supervisorLeaseV1` in the `capabilities` of its control challenge.

| relay | acquire / release | renew | query |
|-------|-------------------|-------|-------|
| primary | decided in one write transaction | extends by `TTL_SECS` | current holder |
| standby (`standby.readonly`) | `unavailable`, `ACCOUNT_STORE_READ_ONLY` | answered while the replicated row still names the same holder, fence and epoch and is not released | current holder |
| replica that predates schema v3, or no account store | `unavailable`, `LEASE_UNSUPPORTED` | same | same |

On the standby the renewed expiry is kept in memory with the standby's own clock and is never
written; it is dropped as soon as the replicated row names another holder or fence. So while
the primary is down, the current holder keeps its lease through the standby, but no other
machine can take it over, and nothing is released. A move or a takeover waits for the primary or
for a promotion. A read-only store never runs the v3 migration.

In detail, a standby relay (`ferryx-relay` with `standby.readonly` in its data directory)
answers lease requests like this (`answer_standby_lease` in `src-tauri/src/remote/relay_server.rs`):

- `SupervisorLeaseAcquire` and `SupervisorLeaseRelease` get outcome `unavailable` with
  `reasonCode` `ACCOUNT_STORE_READ_ONLY`, without reading the store. This includes a forced
  acquire.
- `SupervisorLeaseRenew` re-reads the replicated row on every request. It answers `renewed`,
  with an expiry of the standby's `now + TTL_SECS`, only if the row is not released, names the
  asking machine and the presented fence, and that fence is in the replica's current epoch.
  Otherwise it answers `refused` with `LEASE_NOT_HOLDER`. The checkpoint id a renew carries is
  not recorded.
- `SupervisorLeaseQuery` reports the replicated holder. Its expiry is the later of the
  replicated `expires_at` and the in-memory renewal.

What this means for daemons while only the standby relay is up:

- A holder's clean `ferryx hub stop` still publishes its final checkpoint, but its release is
  not acknowledged. The holder has stopped renewing, so its lease runs out on its own (TTL plus
  grace). No standby daemon can acquire it until the primary is back or promoted.
- `ferryx hub move-here` (also with `--force`) cannot acquire the lease. A move that staged an
  import discards it, and the target keeps its own database.
- A standby daemon does not take over automatically, because acquire is unavailable.
- After a promotion, the epoch bump makes the old holder's fence unrenewable; the holder
  re-acquires and gets a new-epoch fence (see "Promotion and failback").

Refusal codes in `reasonCode`: `LEASE_HELD` (another machine holds a live or in-grace lease),
`LEASE_CHECKPOINT_MISMATCH` (the lease is free but the caller's checkpoint is not the last
recorded one), `LEASE_NOT_HOLDER` (renew with a wrong holder, fence or epoch, or a released
row), `LEASE_MACHINE_NOT_ENROLLED`, `LEASE_FENCE_EXHAUSTED`, `LEASE_STORE_ERROR`.

### Deploy order for the lease

1. Upgrade the standby relay first. Its store is read-only, so the new binary does not migrate
   it; until the replica carries the v3 tables it answers `LEASE_UNSUPPORTED`.
2. Upgrade the primary. Its first start migrates the account database to v3, and Litestream
   carries the new tables to the standby.
3. Then upgrade the daemons (see `docs/supervisor/unified-cutover.md`).

Never downgrade a relay below the lease version once daemons have seen `supervisorLeaseV1`:
those daemons stay on standby against a relay without the capability instead of starting their
own supervisor.

## Tooling

`scripts/relay-failover.mjs` prints a JSON plan for each command and changes nothing unless
`--apply` is given. Copy `scripts/relay-failover.example.json` to a private path, fill in
`standby.ssh` once the owner provides the VPS, and keep the file out of git.

```bash
bun scripts/relay-failover.mjs deploy-standby  --config ~/ferryx-relay-failover.json   # dry run
bun scripts/relay-failover.mjs deploy-standby  --config ~/ferryx-relay-failover.json --apply
bun scripts/relay-failover.mjs cloudflare      --config ~/ferryx-relay-failover.json --apply
bun scripts/relay-failover.mjs drill           --config ~/ferryx-relay-failover.json --apply
bun scripts/relay-failover.mjs promote-standby --config ~/ferryx-relay-failover.json --apply
bun scripts/relay-failover.mjs demote primary  --config ~/ferryx-relay-failover.json --apply
```

With `standby.ssh` unset, every standby plan prints `blockedOn: STANDBY_HOST_MISSING` and
`--apply` refuses to run. `bun test scripts/relay-failover.test.mjs` pins the plans against
`scripts/fixtures/relay-failover/`; regenerate a fixture with the dry run when a plan changes
on purpose.

Secrets never enter a plan. Before `deploy-standby`, create on both hosts:

- `/etc/ferryx/litestream.env`: `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY` for
  the R2 bucket named in the config;
- `/etc/ferryx/relay.env` on the standby: a copy of the primary's private relay environment
  (machine tokens, mail and Lemon Squeezy keys).

`deploy-standby` also copies the account signing key and the relay machine key registry from
the primary, so grants and control-channel keys verify on either host.

## Cloudflare failover

Used: **Cloudflare Load Balancing** (`cloudflare.mode = "load-balancer"`). One HTTPS monitor
checks `GET /healthz` every 15 s (one retry, 5 s timeout, so the primary is marked unhealthy in
about 30-40 s) and expects `"ok":true`. The primary pool (the omarchy tunnel
origin) is preferred with `steering_policy: off`; the standby pool (`relay-2.ferryx.dev`) takes
traffic only while the primary pool is unhealthy, and is also the fallback pool.

A Tunnel with two connectors is refused by the tooling (`CLOUDFLARE_MODE_UNSUPPORTED`):
replicas of one tunnel spread requests across connectors, which would send account writes to
the read-only standby half the time.

`cloudflare --apply` needs `CLOUDFLARE_API_TOKEN` (Load Balancing edit), `CLOUDFLARE_ACCOUNT_ID`
and `CLOUDFLARE_ZONE_ID` in the environment. The DNS record for `relay.ferryx.dev` is replaced
by the load balancer of the same name; `relay-1.ferryx.dev` and `relay-2.ferryx.dev` stay as
direct names for monitoring.

## Daemon-side failover

Daemons follow `relay.ferryx.dev`, so DNS failover needs no client change. A daemon can also
list extra relay origins in `FERRYX_RELAY_FALLBACK_URLS` (comma-separated, HTTPS only). After a
failed control connection it tries the next origin; after a healthy session ends it returns to
its configured relay. Control and data channels always use the same origin.

## Failover drill

Schedule a window, then run `drill --apply`. maho-win polls
`https://relay.ferryx.dev/healthz` every 5 s while omarchy's relay is stopped for 120 s. The
result lands in `drill.evidenceDir` (`drill-probe.txt`, `drill-summary.json` with sample count,
failures and longest outage). The drill passes only if every sample, including those taken while
the primary was stopped, returned 200.

The failure scenario is checked by `deploy-standby` itself (`standby-refuses-writes`): an
enrollment POST straight to the standby must return `ACCOUNT_STORE_READ_ONLY`.

## Promotion and failback

`promote-standby` fences the old primary (stops its relay and Litestream; power it off in the
provider console if it is unreachable), stops the restore timer and restores the newest
replica. On the standby it then runs, in this order:

1. `stop-relay`: stops `ferryx-relay.service`. A running relay checks the marker on every
   store call and would turn writable the instant the marker goes.
2. `remove-marker`: removes `standby.readonly`.
3. `bump-lease-epoch`: `ferryx-relay bump-lease-epoch` (with `FERRYX_ACCOUNT_DATA_DIR` set to
   the account data directory) raises the supervisor lease epoch in one write transaction.
4. `start-replication`: starts Litestream and the relay.

The epoch bump is what keeps fences safe across a promotion. The restored replica can lag the
old primary, so it may not know the newest fence. After the bump every fence the new primary
issues is in a higher epoch than anything the old primary issued. A lease row from the old epoch
counts as free for acquire and cannot be renewed once it has expired, so a restored row cannot
revive an old holder. The current holder re-acquires and gets a new-epoch fence; executing
daemons then refuse any send stamped with an older fence.

Point the load balancer's pool order at the new primary, then run `demote primary` on the old
one when it comes back, so it rejoins as the standby. Never remove a marker by hand while the
other host is writable, and never start a promoted relay before `bump-lease-epoch` succeeded.

## Status page

`status-page/worker.js` is a Cloudflare Worker on `status.ferryx.dev`. A cron trigger probes
`/healthz` on `relay.ferryx.dev`, `relay-1.ferryx.dev` and `relay-2.ferryx.dev` every minute and
stores per-host daily counts in D1; the page shows current state, role and 90-day uptime per
host, and `/api/status` returns the same data as JSON. Deploy (owner action):

```bash
bunx wrangler d1 create ferryx-status          # put the id into status-page/wrangler.jsonc
bunx wrangler deploy -c status-page/wrangler.jsonc
```

`bun test status-page/` covers the probe, the aggregation and the rendered page.
