# Todo 30 implementation handoff

Branch: rex/t30. The integration branch was merged before daemon changes.
The prior SDK commits are preserved, including manifests, grants, crash budgets,
layout content identity and the clock example.

Added daemon registry, consent transport and Settings, plugin attach CLI and API
catalog schema, block render/view transport, event-driven scriptless iframe
rendering, drag masking, approval decision-record carry and Windows idle restart
teardown/restore. Server and protocol edits are additive and explicitly authorized.
Protocol remains v5. Each edited file was staged explicitly and committed after
reviewing the cached stat. No push or integration merge was performed.

Verification: source inspection and git diff --check only. No build, test, QA,
mutation run or remote gate was executed, per the implement-first instruction.
The llms generator ran with a source-derived catalog input to avoid building the
CLI; the generated site copy is ignored by git. Regenerate/check against the real
CLI during combined integration verification.

Acceptance on linux/mac/win:
`cargo test --manifest-path src-tauri/Cargo.toml --lib plugins::`

QA from the plan:
- HAPPY: Enable examples/plugins/clock, `ferryx-cli panes attach --plugin clock`
  -> layout gets a Plugin leaf; plugin process running (pid recorded) and killed
  on disable.
- FAILURE: Plugin requests Write to an ungranted session -> FORBIDDEN
  PANE_GRANT_REQUIRED.

Remaining risks requiring integration review/verification:
- The isolated renderer is an opaque-origin sandboxed iframe browsing context,
  not a separately allocated native Tauri child webview. It forbids scripts,
  same-origin access, top navigation, forms and popups; native-webview wording in
  the plan needs an explicit owner/reviewer verdict before todo 30 is accepted.
- Plugin-origin images/styles have a CSP allowlist but no custom resource URL
  handler; the clock only needs text. No network origin is permitted.
- Malformed installed manifests currently fail discovery/restore closed with an
  error; integration should exercise startup with a broken manifest.
- Executables run as the owner, not in an OS sandbox. Consent must be restricted
  to trusted executables; scriptless rendering is not process sandboxing.
- The existing private JSON writer's Windows rename/ACL behavior is reused;
  repeated enable/disable persistence needs the Windows acceptance leg.
- A new exhaustive client request match needed names for existing Layout/Panes
  variants; these local additive arms were included for compile completeness.
- Worktree retained for the parent to integrate; no other worktree was edited.

Plan: .omo/plans/rex-adoption-20261104.md
