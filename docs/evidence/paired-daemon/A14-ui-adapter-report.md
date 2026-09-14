# A14 UI adapter report

## Outcome

Implemented the assigned typed desktop operation adapter and 19 passing adapter tests. Full A14 and the full user goal remain incomplete. No visible frontend change, mounting, proxy enablement, Rust edit, metadata edit, relay edit, commit, release, deployment, daemon/PTY access, or desktop interaction occurred in this lane.

Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.
Execution: macOS arm64; filesystem commands wrapped in a synchronous shell `run_blocking` function because no run_blocking tool/executable was provided.

## Exported command/type contract

`createPairedDaemonProjectAdapter(context, dependencies?)` captures explicit immutable `{ hostId, generation }`. Generation is parsed as canonical u64 decimal. Production dependencies use Tauri `invoke`, `isTauri`, and the sanitized `remoteHostStore`; injected dependencies are for isolated tests. No active selection lookup routes an operation. Native inventory must be ready, and the captured host must still have matching generation, machine identity, and a paired machine grant before dispatch and after completion (including rejection).

Command: `paired_host_operation`, arguments `{ request: { hostId, generation, operation } }`. Unknown IPC response is parsed as `{ hostId, generation, result: { kind, data } }`. Discriminants and provenance must match. Rust alone generates desktop IDs. `registerProject` and `projects` parse canonical `daemon:<64 lowercase hex>` IDs, preserve `remoteWorkspaceId` and remote metadata, and require matching paired target host. Project-list unavailable IDs are desktop IDs. Worktrees/sessions and journal outcomes remain remote machine DTOs, matching the native producer's actual `OperationResult::Operation(m::Operation)`; a journal project is not itself an adopted desktop reference.

Exported types: `PairedHostContext`, `WorktreeIdentity`, `RegisterProjectRequest`, `UnregisterProjectRequest`, `CreateWorktreeRequest`, `DeleteWorktreeRequest`, `CreateSessionRequest`, `CloseSessionRequest`, `PairedHostOperation`, `PairedHostOperationRequest`, `PairedHostInvoke`, `PairedDaemonProjectAdapter`; error class `PairedOperationError`.

Wrapper -> operation fields (all additionally capture context):

| Wrapper/kind | Fields | Result |
| --- | --- | --- |
| capabilities | none | validated machine capabilities |
| directories | path optional, includeHidden boolean | directory listing |
| projects | none | mapped project-list envelope |
| registerProject | request { requestId, repoPath } | mapped project |
| unregisterProject | workspaceId, request { requestId, expectedRevision } | null |
| worktrees | workspaceId | remote worktree list |
| worktreeStatus | workspaceId, worktree { wsId, slug } | deletion/status preview |
| createWorktree | request { requestId, workspaceId, worktree, baseRef? } | remote worktree |
| deleteWorktree | request { requestId, workspaceId, worktree, deleteBranch, expectedRevision } | null |
| sessions | workspaceId optional | remote session list |
| session | sessionId, daemonEpoch | running/exited/expired detail |
| createSession | existing machine request DTO: requestId, workspaceId, worktree, cols, rows, inheritFromSessionId, cwdRelative, startup | remote session |
| closeSession | sessionId, request { requestId, daemonEpoch } | null |
| operation | requestId | validated journal state/outcome |

Startup supports shell and typed agentResume/providerSession. DeleteWorktreeRequest excludes create-only baseRef.

Capability negotiation must precede operations. Unknown capabilities fail closed. Requirements are directoryBrowseV1, machineWorkspaceV1, managedWorktreesV1, or terminalCreateV1 by operation; known terminalStreamV1 and machineEventsV1 are accepted but do not enable the proxy. `operation` requires machineWorkspaceV1 in this adapter. No automatic mutation retry occurs; callers explicitly reconcile the original request ID. Native ClientError `{ code, machineError, requestId, ambiguous }` and existing `{ error }` machine envelopes are supported. Structured machine code/retryability/request ID/details and ambiguity survive; arbitrary native strings/HTML/exceptions become PAIRED_HOST_UNAVAILABLE. Error.message is the machine code, not free-text body. Native is responsible for sanitizing structured machine details and remote Git metadata.

## Verification

1. Behavioral RED: temporarily omitted the post-await current-generation/grant check, then ran `bun run --cwd ui test src/lib/pairedDaemonProject.test.ts`. Exit 1: 3 failed / 15 passed. Re-pair, forget, revoke tests each observed incorrectly resolved registrations. Restored the fence immediately. Raw output: A14-ui-adapter-red.log.
2. Required combined run: `bun run --cwd ui test src/lib/pairedDaemonProject.test.ts src/lib/pairedDaemonContracts.test.ts src/state/remoteHostStore.test.ts`. Exit 1: adapter 19/19 passed; host store 13/13 passed; contracts 77/78 passed; aggregate 109 passed, 1 failed. Raw output: A14-ui-adapter-green.log (filename names the intended GREEN run, not an all-green claim).
3. External/read-only failure: contracts session roundtrip expected `agentType: "omo"` and `title: "agent terminal"` in the currently shared fixture, but its existing decoder drops those fields. The metadata fixture appeared during concurrent work; this lane did not edit either dependency. No failure suppression or retry. Metadata owner must resolve this before aggregate acceptance.
4. `bun run --cwd ui build`: exit 0, TypeScript and Vite passed, 1884 modules transformed. Raw output: A14-ui-adapter-build.log. After excluding create-only baseRef from DeleteWorktreeRequest, the same build passed again (exit 0), recorded in A14-ui-adapter-build-final.log.
5. LSP: final test file clean. Adapter clean before final native error alignment; subsequent fresh-diagnostics requests timed out. Final full tsc succeeded. Exact limitation in A14-ui-adapter-lsp.log.
6. Scoped `git diff --check` exit 0 (new untracked files are not covered by git diff); build parses both files.

Async tests use explicit entered/release promise gates, handlers installed before releasing responses, and a 2000ms test timeout for late-response scenarios. No sleeps, polling, timing-luck assertions, real credentials, network, filesystem fixture roots, or process spawns. Equal-path tests exercise the injected typed native command boundary for two distinct captured owners. Native command registration/HTTP/relay transport and real native UI are not verified by these mocks and belong to integration, not this lane.

## Cleanup and limitations

No owned persistent processes, sockets, temporary daemons, PTYs, or remote resources were created. Build generated its normal ignored `ui/dist` artifacts and test runner caches; these were left intact to preserve concurrent work. No destructive cleanup. Only the two assigned UI files and this lane's evidence were deliberately written. Dependencies, existing work, metadata and relay code remain untouched by this lane. Paired proxy capability stays OFF; the adapter has no feature-store writes. The library is intentionally not mounted, so no real user-visible entry point exists to exercise. Transport cancellation, redirect policy, authentication, durable registration/journaling and final canonical ID provenance rely on the independent native lane. Full aggregate acceptance remains blocked by the read-only contracts test and final LSP freshness limitation.

## Changed files and SHA-256

- `ui/src/lib/pairedDaemonProject.ts`: `9bf51ced8bd2db11e002d2667517ead9b985417f1e9dd7027acad93169254a3e`
- `ui/src/lib/pairedDaemonProject.test.ts`: `d98d5ee0f26bcc1517e00b3b900fe7798aa980b298f4372ef00b3aef8b872678`
- `docs/evidence/paired-daemon/A14-ui-adapter-build-final.log`: `9390cfe007f27b84f7ca4750c8226a965ed66374b2216638ce222f56e4b834f8`
- `docs/evidence/paired-daemon/A14-ui-adapter-build.log`: `089b2d2f5b4e6762e74f4f3f2fb314106bdf0b205b716921d79fd2a4121bde02`
- `docs/evidence/paired-daemon/A14-ui-adapter-green.log`: `5568c2f57f530888855efb7e0d2a460c71d230fe288d67e7f67c8664c1ec7696`
- `docs/evidence/paired-daemon/A14-ui-adapter-lsp.log`: `5a82eb9b578086ee9a73254a849d52760e08cb1cdefc838946fdebfc1809b673`
- `docs/evidence/paired-daemon/A14-ui-adapter-red.log`: `026935b643a793bc221507791fdbe3d7197b2da6e8db8a05e1bc0fc331390079`

This report is also a new owned file; its hash is returned separately.
