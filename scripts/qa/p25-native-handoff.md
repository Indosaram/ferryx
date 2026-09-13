# P25 runtime-owner handoff (st_01a09a0b -> st_01a099f8)

No PowerShell interpreter is installed on the Darwin child host. Before changing
PowerShell wrappers, run the staged source-boundary test on native Windows:

```
powershell.exe -NoProfile -File scripts/qa/helper-wrapper-contracts.ps1
```

This harness mocks tar/cargo/bun functions, owns a random TEMP directory and
archive, and does not build or launch a daemon/SSH/native GUI. Current wrappers
must fail archive-preservation assertions (and zero-test/failed-survival cleanup
where reached). Capture output, source hashes, exit code and cleanup receipt.
Relay the RED log to the lead. After RED, authorize the child wrapper fix or
apply only the exact wrapper diff handed off in impl-p25.md, then run identical
assertions GREEN. Do not use these wrappers against real source archives until
this contract passes.

Native survival acceptance additionally needs a verified helper debug executable
and runtime-owner exclusive use of owned roots. Helper protocol is 1; daemon
protocol is 3. Run direct helper survival only after identity/cleanup repair.
For SSH detached startup/bridge tests, provide an owned OS process handle/start
identity facility across the same local/SSH account namespace, plus private
pinned SSH configuration and known-host files. Numeric endpoint PID text is
not sufficient permission to kill. Preserve roots on missing proof/failure.
No SSH service/trust provisioning, installed replacement, or user daemon action
is authorized by this handoff.
