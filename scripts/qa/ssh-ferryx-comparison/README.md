# Production SSH bridge comparison

This is a standalone QA binary that depends on the production `ferryx` Rust
library and calls its real `BridgeConnection::spawn`, `handshake`, and `close`
methods. It performs no helper installation or daemon startup. It requires an
already-running isolated SYSTEM-token inbox `sshd` fixture and an explicitly
provided isolated helper executable path.

The fixture contract is the `manifest.json` written by
`scripts/qa/ssh-causal-comparison/run_windows_experiment.py start`. The host
must be `127.0.0.1`, and `token_context` must be `SYSTEM`. The client profile
uses that manifest's pinned `known_hosts`; SSH forwarding is disabled. The
harness creates uniquely named `known_hosts-ferryx-causal-<pid>` and
`config-ferryx-causal-<pid>` files in the fixture user's `.ssh` directory. It
does not edit the default config or known_hosts files.

The fixture already verifies access to the local `sook` account. The production
helper binary at its ordinary installed path is only executed with the unique
`FERRYX_SSH_TEST_HELPER_ROOT` supplied below. Its start/bridge process is
launched through the private SYSTEM sshd session; the harness never invokes
`helper_setup::ensure_started`, installs a binary, or uses the ordinary helper
root. The local machine may have an unrelated pre-existing helper daemon; this
experiment does not connect to or signal it.

## Remote compile and run

Copy this directory and the source checkout needed by its Cargo path dependency
to maho-win. Use a unique private Cargo target directory and execute from the
harness directory:

```powershell
$env:CARGO_TARGET_DIR = "$env:TEMP\ferryx-ssh-causal-target-$([guid]::NewGuid().ToString('N'))"
$env:FERRYX_SSH_TEST_HELPER_ROOT = "C:\ProgramData\Ferryx\ssh-causal-helper-$([guid]::NewGuid().ToString('N'))"
$env:FERRYX_SSH_TEST_HELPER = 'C:\Users\sook\.ferryx\bin\ferryx-remote-helper.exe'
cargo run --manifest-path .\scripts\qa\ssh-ferryx-comparison\Cargo.toml --release -- .\evidence\manifest.json .\evidence\bridge-trials.json
```

The process writes complete JSON evidence and exits nonzero unless all 20
sequential connections and all five waves of four concurrent connections
complete with matching control/reader handshakes and explicit close. Each
trial has a 20-second wall-clock timeout. The fixture sshd remains running for
separate cleanup via its own explicit `cleanup --evidence` command.
