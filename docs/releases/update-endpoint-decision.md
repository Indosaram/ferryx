# Update endpoint decision: GitHub Releases vs Cloudflare R2

Status: decided 2026-11-04. **Decision: stay on GitHub Releases.**

The in-app updater fetches `latest.json` (and, since todo 18, `latest.json.minisig`) from
`https://github.com/Indosaram/ferryx/releases/latest/download/`. This note compares that endpoint
with serving the same files from a Cloudflare R2 bucket behind a custom domain.

## Measurements

Five sequential `curl -L` fetches of the live stable manifest from the operator's Mac (Seoul) on
2026-11-04 12:18 UTC; every response was HTTP 200:

| Run | Time to first byte (s) | Total (s) |
| --- | --- | --- |
| 1 (cold) | 0.918 | 0.918 |
| 2 | 0.197 | 0.197 |
| 3 | 0.197 | 0.197 |
| 4 | 0.107 | 0.107 |
| 5 | 0.207 | 0.207 |

The `/latest/download/` path answers with a redirect to `objects.githubusercontent.com`, so each
check costs two requests. R2 was not measured because no bucket exists; for comparison, a public
R2 custom domain is a single Cloudflare edge request, typically below 0.1 s warm.

## Comparison

| Criterion | GitHub Releases | Cloudflare R2 |
| --- | --- | --- |
| Latency | 0.1-0.2 s warm and about 0.9 s cold (measured above); one redirect | One edge request, likely faster when warm; the update check runs at most once an hour, so the gain is not user-visible |
| Availability | Shares fate with github.com; the manifest and artifacts sit on the same host, so the manifest can never point at missing artifacts | Separate failure domain; the manifest can drift from the release artifacts unless both are uploaded together |
| Cost | Free for public releases, no egress charge | Storage plus Class B operations; egress free; a new paid account to keep alive |
| Operator steps | None beyond `release-local.mjs publish` (one `gh release` upload) | A second upload target and credential in the local runbook, plus a cache purge for `latest.json` on every release and every halt |
| Halt latency | Takes effect as soon as the re-signed `latest.json` is uploaded to the latest release | Same, plus the edge cache TTL unless purged |
| Trust | The client verifies `latest.json.minisig` with the updater key, so the host is not trusted with update policy | Same |

## Decision

Stay on GitHub Releases. The measured latency is well under a second for an hourly background
check, availability is tied to the release artifacts that have to come from GitHub anyway, and R2
adds a paid account, a second credential and an extra runbook step without changing the security
model (the signed manifest already makes the host untrusted). Revisit if GitHub manifest fetches
fail or exceed 2 s for users, or if artifacts move off GitHub.

Uploads stay in the local runbook (`docs/releases/LOCAL_RELEASE_RUNBOOK.md`);
`scripts/release-workflow-policy.mjs` is unchanged and hosted workflows still never publish.
