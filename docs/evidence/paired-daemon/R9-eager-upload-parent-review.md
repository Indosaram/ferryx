# R9 parent evidence review

## Disposition

The optional eager-upload investigation now has reproducible fresh-client and
reused-connection failures, with actual client/server socket observations. It
does not establish a production authorization or input-limit defect, and does
not establish guaranteed rejection delivery for oversized concurrent uploads.
No blanket body drain or mutation retry is justified by this evidence.

The earlier deterministic contract-test repair remains intact. The legacy
session response regression is now repaired: parent read list_sessions and
the unchanged compatibility assertion, then independently ran the real HTTP
boundary test against the current composition. R9 is closed as an investigated
transport limitation with passing authorization/body-limit/legacy regression,
not a promise of reliable responses during arbitrary concurrent uploads.

Current command: `cargo test --locked --manifest-path src-tauri/Cargo.toml
--no-default-features --lib r3_http_boundary_contract -- --nocapture`.
`R9-parent-boundary-current.log` records one passing test, exit 0, and listener
join/refusal/root-removal receipts. The command used env -i, umask 077, private
HOME/Ferryx/agent/XDG/TMP paths and the existing private Cargo target.
Supervisor /tmp/fx-r9-parent.bFPXnL was removed with rmdir after completion.

An earlier nested-shell quoting error expanded the supervisor variable in
the outer shell, so mkdir failed before Cargo or a fixture ran (exit 2).
The empty allocated /tmp/fx-r9-parent.amKNzG was removed with rmdir. This was
an invocation failure, not behavioral RED; the corrected invocation used
single-quoted shell input. No production source was edited in this follow-up.

## Evidence inspected by the parent

- Read the complete diagnostic implementation and its private-process setup.
  The example hash still equals
  `2d5f7151fbfff64b13d544b287d53dd25046a3be85739126f8a01617299d2b4c`.
- Independently parsed every JSON event in both wire logs without parse errors.
  The final log contains 45 cases: all 9 staged cases are valid; 16 reqwest
  outcomes are valid and 2 remain transport failures.
- Inspected connection 25 in the final log: health response at sequence 368,
  prime completion at 369, complete 403 write at 371, shutdown at 373, socket
  drop at 374, and client BodyWrite/ConnectionReset at 375. The same connection
  serves both requests, proving reuse for this failure.
- Inspected the final fresh anonymous failure: complete 401 write at sequence
  427, shutdown/drop at 429/430, and BodyWrite/BrokenPipe at 431.
- Confirmed the first execution contains a fresh mirror DELETE failure at
  sequence 448 with BodyWrite/ConnectionReset. Pooling is not necessary.
- Counted 36 accepts and 36 drops in the first run, and 45 accepts and 45 drops
  in the final run. Both logs include graceful listener join, connection refusal,
  runtime join, reaped child and removed private root.
- Ran LSP diagnostics on the current example: no diagnostics.
- Read the actual related test's failure output, including exit 101, one failed
  test, and its listener/root cleanup receipt.

These are parent source and raw-artifact checks, not a third diagnostic
execution. Diagnostic exit 0 is not transport acceptance. Successful server
writes establish socket acceptance, not successful reqwest response parsing or
kernel packet/ACK ordering.

## A09 regression handoff

`remote::server::tests::r3_http_boundary_contract` failed at
`src-tauri/src/remote/server.rs:2531`. Its existing legacy `/api/v1/sessions`
assertion expects `Missing auth token`, but A09 currently returns the structured
`UNAUTHORIZED` envelope. The parent read the current compatibility assertion
and sent the exact failure to active owner `st_01a097fc`.

The instruction is to preserve approved legacy/mirror behavior in the
implementation, not weaken the existing assertion. The next invocation must
use private HOME/runtime/data/session isolation; the diagnostic's related test
invocation omitted that isolation, which remains a recorded limitation.

The 64 KiB bound and the error/status mapping were checked against the approved
plan. The plan does not require ingestion of a complete oversized upload or a
successful client HTTP response despite concurrent socket failure. Native
client, relay, valid-body, Linux/Windows and whole-plan acceptance remain open.
