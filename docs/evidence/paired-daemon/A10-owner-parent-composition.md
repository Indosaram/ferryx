# A10 owner HTTP composition: partial, not GREEN

Parent integrated routed list/detail and owner HTTP close in session_api.rs.
Read work runs in an owned async task retaining Admission while nested blocking
reads drain, with revocation/deadline selection outside and checks before/after.
Close validates/authenticates first, forwards the original body and bearer only
through the validated predecessor transport before any replacement journal work,
and returns the owner's status/body. Existing token parser is reused.
HOST_UNAVAILABLE maps to 503, unsupported machine owners to 422, changed ownership
to 409. No fallback to Local/SSH was added.

## Direct failing-first evidence

Parent attempts initially failed fixture isolation, not the target behavior:
A10-owner-parent-RED.log used the wrong supervisor prefix;
A10-owner-parent-runtime-RED.log used runtime mode755 instead of700;
A10-owner-parent-authority-RED.log encountered concurrent socket signature E0061.
These are preserved, not counted as behavioral RED.

Parent then executed the already-built handover test binary with correct private
prefix and umask077. A10-owner-parent-built-RED.log records original PID76841,
original CWD/epoch and routed generation2 resize103x37, but replacement HTTP
detail returned expired rather than running. Test failed with exit101 after
its cleanup. HTTP adapter source remained unchanged from that binary until
the parent composition.

## Misleading exit0 detected

A10-owner-parent-GREEN.log compiled and exited0 after composition, showing
PID77838 and replacement HTTP running with the original target, plus routed
generation2 and original CWD. It contains NO final test result or cleanup.
It is NOT accepted as GREEN.

Source investigation found machine lifecycle retirement calls
HandoverManager::check_retirement_if_empty, then retire spawns cleanup and
std::process::exit(0). Closing the last machine PTY in this two-owner-object,
single-process fixture can terminate libtest before journal completion,
response assertions and cleanup. /tmp/a10-owner-parent.YmuzgZ retained the
fixture and a machine journal temporary file after exit.

Task st_01a09869 now owns a separate-process final-close regression and minimal
retirement ordering repair, retaining real production retirement rather than
adding a sentinel PTY or disabling it. Parent keeps session_api ownership.
The tracked requirement is:
`A10 followthrough: preserve final close during owner retirement`.

## Regression evidence after adapter composition

- A10-owner-journal-regression.log: six HTTP contention/revocation/actual
  deadline tests passed, exit0, with held permits and full drainage.
- A10-owner-session-regression.log: one real HTTP/PTY lifecycle scenario passed,
  exit0. Replay preserves target/PID, changed digest conflicts, roots/CWD match,
  explicit close reaps, listener and private root clean up.
- session_api LSP: no errors.

These regressions do not prove final predecessor retirement, old-peer refusal,
separate-process handover, or saturated output. All remain required.
Output socket integration is assigned to st_01a0986b in a disjoint server
machine-socket scope; retirement worker must not edit that scope.

All changes are uncommitted. No canonical daemon, desktop, release or commit
operation occurred. Retained failed supervisor artifacts await exact owned
cleanup; neither exit0 nor an empty process-name search substitutes for it.
