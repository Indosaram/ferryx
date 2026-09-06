# Project location QA environment

This receipt records actual environment checks performed by the lead.
It does not claim that the product registration or desktop UI has passed.

## Verified remote transport

The existing `eclipticrd-ci` VM was stopped before this session. The lead started
it using:

```sh
tart run --no-graphics --no-audio --no-clipboard eclipticrd-ci
tart ip eclipticrd-ci --wait 120
```

The address command exited 0 and returned `192.168.64.197`.
The VM lifetime belongs to monitor `mon_FTKBZ8H3M7MST4JS` / `bash_1`.
Its teardown is required after product QA.

An initial noninteractive SSH connection failed with
`Host key verification failed.` There was no matching entry in the default
known-hosts file. Host verification was not disabled.

The lead obtained the host fingerprint through the independent VM guest-agent
channel:

```sh
tart exec eclipticrd-ci /usr/bin/ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub
ssh-keyscan -T 5 -t ed25519 192.168.64.197
```

The guest command exited 0 and returned:

```text
256 SHA256:DlgEwv3RJNHLQKbfMamf4aqexcwl+dpzgxv7bZl0bn8 no comment (ED25519)
```

The lead decoded the scanned public key and calculated its SHA-256 digest using
`Bun.CryptoHasher`. It matched that fingerprint exactly. Only this verified key
was written to `/tmp/ferryx-project-location-qa.VM4GNI/known_hosts`.
No personal SSH configuration or trust file was changed.

The following real SSH invocation then exited 0:

```sh
ssh \
  -o UserKnownHostsFile=/tmp/ferryx-project-location-qa.VM4GNI/known_hosts \
  -o StrictHostKeyChecking=yes \
  -o BatchMode=yes \
  -o ConnectTimeout=5 \
  admin@192.168.64.197 \
  'printf "FERRYX_QA_HOST\n"; id -un; pwd; uname -s; command -v git'
```

Actual output:

```text
FERRYX_QA_HOST
admin
/Users/admin
Darwin
/usr/bin/git
```

This proves a usable real SSH test target and remote working directory, not the
Ferryx registration path.

## Isolated product-API fixture

The lead owns `/tmp/ferryx-project-location-qa.VM4GNI`, created with `mktemp -d`.
It contains:

- `known_hosts`: independently verified public host key.
- `ssh_hosts.json`: only host `project-location-qa-vm`, pointing to the VM as
  `admin`, port 22, agent authentication, enabled.
- `bin/ssh`: a mode-0700 wrapper forwarding every argument to real `/usr/bin/ssh`,
  adding only the explicit QA known-hosts path and strict checking.

`sh -n /tmp/ferryx-project-location-qa.VM4GNI/bin/ssh` exited 0. Prepending this
fixture `bin` directory to a QA process's PATH supplies isolated trust without a
mock SSH implementation or a production trust-bypass setting.

The fixture is not yet evidence that a product API invocation passed.

## Native desktop verification remains open

The selected `orca` executable's version-matched computer-use guide was read.
The initial runtime was absent. `orca open --json` reported PID 49997, but the
process exited before computer-use could execute. A subsequent
`ps -p 49997 -o pid=,comm=` returned no process.

Cleanup receipt: the session-started Orca PID is absent. Existing installed
Ferryx processes and existing daemons were not terminated.

The independent permission checks returned:

```text
osascript -e 'tell application "System Events" to get UI elements enabled'
false
```

```text
peekaboo permissions status --json
Screen Recording: isGranted=false
Accessibility: isGranted=false
```

The user has been asked to enable Accessibility for the terminal running this
agent. No confirmation has been received. Native picker invocation, actual
desktop Add Project actions and screenshots must remain unverified until that
gate can be executed. Browser/component evidence must not be described as a
native desktop pass.

## Required final cleanup

- Stop only the session-started `eclipticrd-ci` VM and verify it is stopped.
- Remove the session-owned QA directory after preserving nonsecret evidence.
- Close any later QA browser, debug app, listener, or test PTY and record its
  individual cleanup receipt.
- Preserve all pre-existing Ferryx processes, personal SSH files and unrelated
  working-tree edits.

## VM interruption and cleanup receipt

The VM subsequently exited with:

```text
guest has stopped the virtual machine due to error:
Error Domain=VZErrorDomain Code=1 "The virtual machine stopped unexpectedly."
```

The monitor process exited 0, but that is not a successful QA outcome.
The lead ran `tart list`, which confirmed `eclipticrd-ci` was stopped, and
removed persistent monitor `mon_FTKBZ8H3M7MST4JS`. No live VM remains from this
launch. The prior successful SSH receipt is historical transport evidence only.
Product API verification still requires a new VM launch and its own cleanup.

## Accepted real backend surface and final fixture cleanup

The backend implementation supplied a cheaper faithful surface:
`daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty`.
The lead read its complete source and executed it directly:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty \
  -- --exact --nocapture
```

Actual result: **1 passed, 0 failed, 0 ignored**, 628 filtered out, 3.81 seconds,
exit 0. This test uses real `/usr/sbin/sshd`, generated keys and an already-bound
ephemeral loopback listener, not a mock SSH process. It calls the production
registration function and daemon PTY spawn path, verifies remote `pwd` output for
new and split/restored terminals, and rejects disabled/deleted hosts and a
missing SSH startup rather than falling back locally. The test closes its PTYs,
asserts an empty session list, cancels its listener task and owns its temporary
directory through `tempfile`.

The optional VM harness was therefore dropped as redundant. No VM restart was
performed. The lead removed only the three inspected session-owned fixture files
and their empty directories. `test ! -e
/tmp/ferryx-project-location-qa.VM4GNI` exited 0. The VM had already been confirmed
stopped and its persistent monitor removed.

## Supplemental browser attempt

An in-process Vite server was used to try rendering the actual chooser/settings
modules in Bun.WebView with only a fixture Tauri transport. The initial virtual
route reached the ordinary RemoteApp fallback; it was not counted as chooser
evidence. After correcting the route and passing the existing CSS configuration
explicitly, WebView creation failed with:

```text
Failed to spawn WebView host process
```

No screenshots or browser scenario passes were captured. The Vite server was
closed and `lsof -nP -iTCP:5173 -sTCP:LISTEN` returned empty. No product HTML or
configuration files were modified for the attempt.

Native desktop acceptance remains open. The successful real SSH test above is
backend evidence only and does not prove the sidebar/picker UI.
