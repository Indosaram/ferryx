# Ferryx scope implementation baseline

2026-09-05. No product changes authored yet.

## Existing adjacent UI behavior

Command: `bun run --cwd ui test src/components/BrowserPane.masking.test.tsx src/components/BrowserPane.findRace.test.tsx src/lib/agentResume.test.ts src/lib/agentResumeAffordance.test.ts src/remote/RemoteAttention.test.tsx src/remote/RemoteTerminal.contract.test.tsx`

Observed monitor mon_0ZR0D510MR4Q6SNA / bash_33: exit 0, 6 files and 113 tests passed, 4.77 seconds. This is baseline coverage only, not proof of new features. Process exited; no server/browser created.

## Provider protocol entry

Command: `/Users/indo/.local/bin/codex app-server --listen stdio://`. Sent JSON initialize request id 1 with clientInfo name ferryx_qa_probe, title Ferryx QA, version 0.1.0, experimentalApi true.

Observed result id 1, userAgent `ferryx_qa_probe/0.153.2`, platformFamily unix, platformOs macos. Remote control remained disabled. This proves only real stdio initialization, not structured turns, permissions, continuity or attachments.

Cleanup: monitor mon_8PNQV12BQWY84K3R / bash_29 killed and completion received. No provider thread was created/resumed.

## Diagnostic tooling

Built-in LSP symbols failed with daemon unreachable. Official daemon startup failed `owner_changed_during_cleanup`; dead owner PID 21195, old socket has no listener. Recovery process bash_25 exited 1. No shared metadata or global configuration was modified. Compiler/test checks are required and LSP must not be reported clean.

## Workflow recovery

Initial category routes returned 503 no available accounts and 429 Rate limit. Explicit ocx/gpt-6-astra canary succeeded; same discovery run was amended after settling. All seven report artifacts completed and lead read each, then checked decisive source contracts. No product implementation is implied by discovery completion.

## Isolated SSH fixture

Loopback sshd: 127.0.0.1:22222, fresh Ed25519 host/client keys under /tmp/ferryx-scope-ssh.Tody5Z, username indo. Password auth disabled; only fixture client key accepted. Monitor mon_82FFQDX2CRQQZSZZ / bash_38 reported listening.

Command: `ssh -F /dev/null -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/tmp/ferryx-scope-ssh.Tody5Z/known_hosts -i /tmp/ferryx-scope-ssh.Tody5Z/client_key -p 22222 indo@127.0.0.1 "printf FERRYX_SSH_QA_READY:; pwd"`. Exit 0, stdout `FERRYX_SSH_QA_READY:/Users/indo`; server logged accepted publickey and disconnection. User SSH configuration/known_hosts unchanged. This proves fixture connectivity, not Ferryx Run on or remote PTY survival.

Cleanup is pending until A2 surface verification: stop bash_38, verify port 22222 unbound, remove only /tmp/ferryx-scope-ssh.Tody5Z.
