# Ferryx account service

The account service is how two of your own machines meet without copying a PIN. A machine enrolls
the daemon identity it already has; the account hands out one-time pairing capabilities, and the
target daemon keeps issuing the device token exactly as before.

This document does not switch the account domain. `ferryx.dev` is not configured here; the origin is
whatever you set in `FERRYX_ACCOUNT_ORIGIN`.

## Binary

`ferryx-account` runs the HTTP service. It is a separate process from `ferryx-relay` and from the
desktop daemon.

```bash
FERRYX_ACCOUNT_ORIGIN=https://account.example \
FERRYX_ACCOUNT_DATA_DIR=~/.ferryx/account \
FERRYX_MAIL_DIR=/var/lib/ferryx/mail \
ferryx-account
```

It prints `FERRYX_ACCOUNT_READY <addr>` on stdout once bound.

## Configuration

| Variable | Required | Meaning |
| --- | --- | --- |
| `FERRYX_ACCOUNT_ORIGIN` | yes | Public origin of this service. Must be `https`, or `http` on loopback. A `ferryx.dev` host is refused unless `FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV=1`. |
| `FERRYX_ACCOUNT_DATA_DIR` | no | Store directory, default `~/.ferryx/account`. Holds `account-store.json` at mode 0600. |
| `FERRYX_MAIL_DIR` | no | Where the file mailer writes magic links. Development only. |
| `FERRYX_ACCOUNT_BIND` | no | Listen address, default `127.0.0.1:43822`. |
| `FERRYX_ACCOUNT_LOGIN_PER_HOUR` | no | Login requests per email per hour, default 5. |
| `FERRYX_ACCOUNT_MAX_BODY_BYTES` | no | Request body limit, default 4096. |

If `FERRYX_ACCOUNT_ORIGIN` is unset the service refuses to start with `ACCOUNT_ORIGIN_UNSET`.

## Mail

The service writes a magic link through a `Mailer`. The shipped implementation is `FileMailer`, which
writes the link to `FERRYX_MAIL_DIR` — suitable for development and for operator scripts that pick the
file up and deliver it themselves. A production transport is deliberately not named in the code:
supply a `Mailer` behind whatever provider you choose. When delivery fails, the endpoint answers
`503 MAIL_FAILED` for every address and no login code is stored.

## Endpoints

| Method | Path | Notes |
| --- | --- | --- |
| `POST` | `/api/account/v1/login/request` | `{"email": "..."}`. Always `202` when the mail went out, whether or not the address exists. |
| `POST` | `/api/account/v1/login/consume` | `{"code": "..."}`. Creates the account on first success, returns an opaque session bearer in the body only. |
| `POST` | `/api/account/v1/logout` | Bearer-authenticated. Revokes that session. |
| `POST` | `/api/account/v1/enrollment-codes` | Bearer-authenticated. Issues a single-use enrollment code bound to the account and origin, 10 minutes. |
| `GET` | `/api/account/v1/machines` | Bearer-authenticated. Lists the machines owned by that account. |
| `POST` | `/api/account/v1/machines/enroll/challenge` | `{"machineId": "..."}`. Returns a nonce to sign. |
| `POST` | `/api/account/v1/machines/enroll` | Signed with the machine's existing Ed25519 key. Consumes the enrollment code. |
| `POST` | `/api/account/v1/machines/{machineRecordId}/grants` | Bearer-authenticated. Returns a one-time pairing capability, never a device token. |

The service stores only hashes of login codes, pairing capabilities, and session bearers. Machine
private keys never reach it.

## Enrolling a machine

Sign in on a machine you already use, issue a code, then run this on the machine that should join. It
works on a headless Linux or Windows host and needs no inbound SSH and no running GUI:

```bash
ferryx-cli account enroll --code <code> [--origin https://account.example]
```

The command signs the challenge with the daemon identity already present in the canonical remote
directory, writes `account-enrollment.json` beside `identity.json` at mode 0600, and prints the
account id, machine record id, relay origin, and enrollment epoch. It never rewrites `identity.json`,
never clears `remote-auth.json`, and never signals a running terminal session.

`--origin` overrides `FERRYX_ACCOUNT_ORIGIN` for that one command.

## What replaced the PIN

`ferryx pair generate` and the Remote Access pairing-code button no longer issue credentials. They
answer `ACCOUNT_LOGIN_REQUIRED` (the CLI exits 2) and point at account enrollment. The pairing
exchange itself still exists and still works: it is what redeems an account-issued capability into a
device token on the target machine.

## Relay traffic

Reachability is unchanged: local network, Tailscale, relay, and a saved SSH forward all remain ways to
reach a daemon. The account design runs one end-to-end handshake before any gateway byte so the relay
forwards ciphertext and cannot read terminal content or the pairing capability.

The handshake itself lives in `src-tauri/src/remote/attach_crypto.rs` and is covered by tests that
splice a recording proxy between the two ends: the recorded bytes never contain the planted payload,
an unbound device key is rejected, a substituted machine key cannot complete the handshake, and a
plaintext client cannot attach. Connecting that handshake to the relay and desktop paths is the part
still in progress, so do not describe a live account attach as encrypted until that wiring lands.
