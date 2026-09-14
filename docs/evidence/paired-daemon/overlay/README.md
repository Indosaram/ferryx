# A03 historical overlay execution evidence

These logs were captured in
`/Users/indo/code/project/orca-lite-wt/herdr-wave0`, whose HEAD was `cd16c90`
and whose working tree included preserved changes from another session.
They establish the original behavioral REDs, controlled mutation sensitivity,
restoration, and successful execution in that overlay.

They do not establish that the clean candidate in `herdr-wave0-clean` builds or
passes. Clean-candidate build, test, owner CLI, cleanup, and review receipts must
be recorded separately before accepting an independently buildable A03 commit.
No product source from the foreign overlay is authorized by these logs.

## Interpretation

- `A03-red-admission.log` records the original real HTTP admission failures.
- Machine View and error-contract RED logs record behavioral failures.
- Each mutation pair records an intentional regression followed by restoration
  and GREEN. The off-thread and post-await revocation mutations test different
  properties.
- `A03-green-admission.log` includes an enclosing compilation failure despite
  passing focused tests; it is not a complete GREEN.
- `A03-live-owner-cli-compile-failure.log` records a successful CLI build followed
  by fixture compilation errors. Those errors are not behavioral RED.
- `A03-live-owner-cli.log` records the repaired real CLI fixture and cleanup.
- `A03-final-remote-regression.log` records 218 passing tests, the headless
  CLI/relay check, and the enclosing exit code 0 after closing an empty pager.
  The clean candidate need not have the same test count because it excludes
  foreign tests.

Original invocation paths, process IDs, and monitor identifiers are retained.
For repository whitespace checks, trailing spaces and tabs from terminal
padding were removed from the copied logs. No non-whitespace content changed;
the original overlay logs remain untouched.
No log was rerun, rewritten as a clean-candidate receipt, or used to infer
implementation of the later A04-A24 services.
