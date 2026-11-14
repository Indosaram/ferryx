# Runtime validation scope

`buildbarn-runtime.yml` is an isolated Linux CI integration test, not an installer.
It builds official Buildbarn sources at fixed revisions and downloads an official,
dated Buck2 release with a fixed archive checksum. It uses the existing renderer
without changing the emitted JSON or Buck rules. All network listeners are on
127.0.0.1; runners use temporary Unix sockets. There are no deployment credentials
or connections to the user's inventory. Native runners execute commands without
sandboxing, so run this only on disposable CI runners, not a shared workstation.

The harness checks:

1. Storage, scheduler, frontend and native runner start from the generated JSON.
2. Worker A alone executes eight Buck2 remote-only actions, and all materialized
   outputs contain a newly generated nonce.
3. After A stops, worker B alone does the same with a different nonce.
4. After B stops, a third nonce fails with no workers. A timeout of the harness is
   a test failure, not a successful negative result. Coordinator processes must
   remain alive and no third-nonce output may appear.

Each build disables remote cache reads, and each nonce changes action commands.
The generated execution platform disables local execution, reinforced by the
`--remote-only` CLI flag. Logs and `result.json` are uploaded even after failure.
Only harness-owned process groups and the isolated cell's Buck daemon are stopped.
Job and subprocess time limits bound failures. The runner is discarded afterward.

This verifies configuration parsing and a real CAS/scheduler/runner transport
path, not Rust compilation, Cargo/Tauri migration, physical multi-machine routing,
macOS compatibility, throughput, fairness, or production security. Two workers
on the same host necessarily report the same hostname; testing them separately
ensures each can execute rather than inferring worker identity from that hostname.
A passing Python unit suite alone is not evidence that this integration passed.

## Upstream pins

- [bb-storage](https://github.com/buildbarn/bb-storage/tree/ca3d8b49a5af52eb02ed473dd689d19b949c9e41)
- [bb-remote-execution](https://github.com/buildbarn/bb-remote-execution/tree/c9ba98cf52456c400278f45a9679635daa40dcd2)
- Both pinned `.bazelversion` files specify Bazel 9.1.0. The workflow downloads
  its official Linux x86_64 binary and checks SHA256
  `a667454f3f4f8878df8199136b82c199f6ada8477b337fae3b1ef854f01e4e2f`.
  Upstream Bazel modules provision the Go SDK from `go.mod` (Go 1.27.3).
- Build each checkout as its own Bazel root module, preserving all upstream
  dependency overrides. Plain `go build` is not equivalent: storage's
  [`MODULE.bazel`](https://github.com/buildbarn/bb-storage/blob/ca3d8b49a5af52eb02ed473dd689d19b949c9e41/MODULE.bazel)
  applies the upstream
  [`service-registrar.diff`](https://github.com/buildbarn/bb-storage/blob/ca3d8b49a5af52eb02ed473dd689d19b949c9e41/patches/org_golang_google_genproto_googleapis_bytestream/service-registrar.diff)
  needed by `bb_storage` to compile. No local upstream patches are added.
  The initial CI run exposed this exact type mismatch before the integration
  harness started. The workflow now builds the official `//cmd/...` targets and
  obtains binary paths using `bazel cquery --output=files`.
- Cold Bazel dependency/toolchain compilation has a 30-minute step limit and
  two concurrent build jobs; the overall job remains bounded at 45 minutes.
  Build logs are retained as artifacts even if compilation fails.
- [Buck2 2026-11-01](https://github.com/meta/buck2/releases/tag/2026-11-01), source
  `e8edae33cd7cbc6d042c58bb55a2a900ac6ad489`, official Linux x86_64 `.zst` archive
  SHA256 `a2f7830c240c682c32826ae0b30bf8864d264e866475abaafcc0857d8133cd53`.
- Buck2 flags were checked in
  [`app/buck2_client_ctx/src/common/build.rs`](https://github.com/meta/buck2/blob/e8edae33cd7cbc6d042c58bb55a2a900ac6ad489/app/buck2_client_ctx/src/common/build.rs).
- The generated 32 GiB CAS file is sparse at creation: pinned storage uses
  [`Ftruncate`](https://github.com/buildbarn/bb-storage/blob/ca3d8b49a5af52eb02ed473dd689d19b949c9e41/pkg/blockdevice/new_block_device_from_file_unix.go).
  The smoke writes tiny outputs but still needs address space and metadata memory.

Locally test only the Python harness and renderer without installing binaries:

```sh
cd infra/buildbarn
python3 -m unittest test_configure test_runtime_smoke
```

To run the integration after explicitly provisioning the pinned binaries on an
isolated host, supply a directory containing `bb_storage`, `bb_scheduler`,
`bb_runner`, `bb_worker` and `buck2`:

```sh
python3 infra/buildbarn/runtime_smoke.py --bin-dir /path/to/bin --evidence /path/to/logs
```

Final-output materialization is explicitly forced with `--materializations all`,
verified in pinned [`commands/build.rs`](https://github.com/meta/buck2/blob/e8edae33cd7cbc6d042c58bb55a2a900ac6ad489/app/buck2_client/src/commands/build.rs).
This bypasses configuration-dependent defaults (including agent-specific skips).
The negative check establishes fail-closed behavior after workers are removed;
it does not yet classify the server status code from log text. Successful prior
identical commands, live coordinators, a cold nonce, no outputs and a nonzero exit
reduce ambiguity, but diagnosing a failure's exact RPC cause requires its logs.
