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
provider console if it is unreachable), stops the restore timer, restores the newest replica,
removes the marker and starts Litestream on the new primary. Point the load balancer's pool
order at the new primary, then run `demote primary` on the old one when it comes back, so it
rejoins as the standby. Never remove a marker by hand while the other host is writable.

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
