# A14 parent rejection and recovery

Status: NOT ACCEPTED. The first four-node completion means child termination,
not working native integration.

Parent recovered and read all four original node prompts from:
`.omo/senpi-task/dag/runs/dag_eee987ec-f28d-4b90-948b-8466717d9c74.json`
in the primary repository. Native/UI producers, native integration and one
aggregate verifier are the original topology. Their exact scopes remain
binding; A15 is not included.

## Independently established defects

- The required A14-native-integration-report.md does not exist.
- Parent read ipc/paired_host.rs: it contains inventory commands only, no
  paired_host_operation. The UI adapter invokes that missing command.
- Parent read complete client.rs, projects.rs, module declarations, UI adapter
  and service credential-capture/generation helpers. Native map_result does
  not validate requested workspace/worktree on worktree responses, requested
  workspace filtering on Sessions, or nested session machine identity on
  completed Operation outcomes. CreateSession validates only machine identity.
  These require applicable captured-owner validation and real HTTP regression
  evidence before native adoption. Predecessor epochs must remain original,
  not be forced to the gateway capability epoch.
- Operation capability requirements differ between native route and UI.
- Actual aggregate-results.json is Rust exit 101, UI tests exit 1, UI build
  exit 0, headless check exit 0, diff check exit 0. It is not a green manifest.
- Parent read actual Rust error: in-flight A10 machine_input_fixture.rs uses
  unavailable reqwest Response.json at lines 39/42. Its active owner has the
  exact diagnostic. Parent's own A12 lib gate independently hit this error.
- Parent read UI failure output: selected runtime exposes unusable localStorage.
  The verifier reports Node 25 versus supported Node 22 PATH difference.
  The rerun must explicitly select a supported runtime, preserve all tests,
  and establish this with a single actual command run.

The aggregate verifier's report correctly returns FAIL. Full producer source
and reverse-scope audit remains open; no blanket validation is inferred from
that report. Parent has not rerun A14's Rust gate while the known fixture
compile blocker remains active. Parent's A12 wire parity now passes 2,221
tests, resolving the earlier metadata decoder mismatch.

## Recovery performed

Same-run send to native-integration returned node_not_continuable.
Same-run retry returned node_not_retryable because the node was completed.
Parent amended the SAME run, generation 2, changing only native-integration
and downstream aggregate-verifier prompts. Completed native/UI producers
were retained, not restarted.

Replacement integration child: st_01a09938. It owns the originally missing
typed command chain, actual two-host native IPC/relay registration and restart
proof, and the minimal native provenance corrections above. Parent sent the
exact findings after reading the relevant implementation. Metadata and A10
sections remain outside its writes. Proxy capability stays OFF.

The aggregate verifier follows that integration, preserving initial FAIL logs,
running supported-runtime UI tests, actual native relay tests and build gates.
There is no per-producer approval gate. Parent must still inspect its source
and independently exercise all deliverables before accepting this packet.

No commit, deployment, release, canonical daemon, user PTY or desktop action
was performed. Work remains uncommitted in the isolated continuation tree.

## Parent followthrough

Parent verified `/Users/indo/.local/bin/node --version` is v22.22.3 and reran
the exact three-file UI command with that directory first in the isolated PATH.
`A12-session-metadata-parent-a14-ui.log` records 19 adapter, 13 host-store and
78 contract tests: 110 passed, zero failed, exit 0. Cleanup receipt is 0.
No storage code or assertions were changed to obtain this pass.

Parent also read client_tests.rs, projects_tests.rs and the complete UI adapter
test file. Native source coverage gaps already sent to the integration owner
remain open; passing UI mocks do not discharge them.

The A10 Response.json error is corrected in the actual fixture. A subsequent
parent backend aggregate encountered the integration owner's newly added
PairedHostOperation enum before its server/client match arms had landed.
`A12-session-metadata-parent-backend-repaired.log` records two E0004 errors,
exit 101; no tests executed. Cleanup receipt is 0. The owner was notified and
must signal stable compile readiness before the next Rust aggregate.

Fresh LSP requests on both A08 repair files timed out after 3,000 ms.
This is unavailable diagnostics, not a clean result. A08 source inspection is
complete for the changed failure scenario/probe, but independent runtime
acceptance remains pending the stable combined source.
