# F6 reviewer availability — measured blocker (2026-11-07)

F6 requires an independent full-diff approval from a `claude-*-5-5` reviewer. No such
reviewer can be reached right now, so F6 stays open. This is provider availability, not a
product verdict. No substitute model was used, and no gate was waived.

## Probe 1 — 2026-11-07, HEAD 38d99c880

Gateway `http://100.91.254.71:18801/v1`, unauthenticated `GET /v1/models` listed
`claude-opus-5-5`, `-high`, `-low`, `-medium`, `claude-sonnet-5-5` variants,
`anthropic/claude-opus-5-5`, `cursor/claude-opus-5.5`. Direct
`POST /v1/chat/completions`:

| model | result |
|---|---|
| `claude-opus-5-5` | 503 `no available accounts for this model` |
| `claude-sonnet-5-5` | 503 same |
| `anthropic/claude-opus-5-5` | 503 same |
| `cursor/claude-opus-5.5` | `upstream_error` / `unknown` |
| `claude-opus-5-5-high` | 400 `INVALID_ARGUMENT request.labels[3].value` (gateway build bug, not a model answer) |

Other pins probed: `inferhub` (insufficient balance), `ccapi` (invalid API key),
`kiro-lb` (invalid or missing API key from off-host), `anthropic` OAuth refresh token
expired 2026-08-15.

## Probe 2 — 2026-11-07 12:47 UTC, HEAD 6aa871e3b

The same gateway now rejects unauthenticated calls (`401 invalid api key`), so this probe
used the configured `mahoquot` master key from `~/.omo/models.json`:

| model | result |
|---|---|
| `claude-opus-5-5` | 503 `no available accounts for this model` |
| `claude-sonnet-5-5` | 503 same |
| `anthropic/claude-opus-5-5` | 503 same |
| `claude-opus-5-5-high` | 400 `request.labels[6].value` (same gateway build bug) |
| `cursor/claude-opus-5.5` | 502 `upstream_error` / `unknown` |

With a valid key the whole `claude-*-5-5` set still fails, so the earlier 503s were not an
auth artefact.

## Consequence

F6 cannot be executed on the required model family. The plan keeps F6 unchecked. F6 needs
either the gateway to regain Claude capacity or an owner decision on an allowed reviewer;
neither is something this session can supply.
