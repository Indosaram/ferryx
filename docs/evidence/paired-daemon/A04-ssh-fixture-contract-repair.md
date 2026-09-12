# A04 SSH fixture contract repair

Status: corrected fixture is under parent execution; not a passing result.

The actual isolated OpenSSH test failed with exit 101 at
`daemon/remote_ssh_gateway_qa.rs:79`, waiting for output after sending a raw binary
WebSocket input frame. The helper PID 26061 was reaped and the parent independently
confirmed that PID and `/tmp/fxQ6Bn1C` were absent.
`A04-extracted-real-ssh-failure.log` contains the full failure output.

The parent inspected `remote/server.rs:1192-1434`: SSH sessions deliberately ignore
binary input and accept `ClientControlMessage::RemoteWrite` with the generation
chosen by the client. The server sends a `RemoteStatus` text frame before terminal
output. Both that production handler and the original SSH fixture have no diff
against the A04 base `a2534ff4`; the mismatch predates this extraction. This source
comparison does not claim that an unmodified baseline runtime was executed.

The parent extended the A04 test-only scope to repair this existing regression
fixture. It now reads the actual Connected status, sends a typed RemoteWrite text
frame using that advertised generation, and retains the original output assertion
and bounded deadline. No production generation fence was weakened or bypassed.
This is a prerequisite to meaningful SSH preservation evidence, not a new SSH
feature or a replacement for paired-daemon transport.

The corrected actual SSH run is monitored as `mon_NSYFWAC4EWJ1YN1M`
(`bash_242`). Final output, process/root cleanup and independent review remain
required. The additional changed test file must be included explicitly in the
final source manifest and commit scope.
