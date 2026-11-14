# Opt-in Buildbarn cluster configuration

This is configuration infrastructure, **not a Ferryx or mhc Cargo-to-Buck2 migration**.
It reads the same external roster as `scripts/pick-build-host.sh`, renders a central
Buildbarn frontend/storage/scheduler plus one native worker/runner pair per opted-in
host, and creates isolated Buck2 transport-check cells. It never SSHes, installs,
starts a service, changes network settings or modifies the existing Cargo/Tauri build.

## What changes compared with the picker

The picker chooses one host for an entire command. Here the scheduler assigns
individual REAPI actions to any registered worker with **exactly matching**
OS/architecture/toolchain properties. Two Linux x86-64 hosts with the same toolchain
profile form one queue; a macOS arm64 worker is a separate queue. A worker's name is
an observability ID, not a property that pins a build to that machine.

A roster entry is not evidence of SDK/toolchain compatibility. Capability declarations
are explicit, not inferred from SSH targets or the picker's process-count probe.
Windows, Instabox and Salad entries remain in inventory but are not provisioned by
this native Unix configuration. Explicitly selecting an unsupported worker fails.

## Render offline

Python 3.9+ standard library only; no dependency installation required:

```sh
python3 -m unittest discover -s infra/buildbarn -p 'test_*.py' -v
python3 infra/buildbarn/configure.py \
  --hosts infra/buildbarn/hosts.example \
  --profile infra/buildbarn/profile.example.json \
  --output /tmp/ferryx-buildbarn-example
```

The example names are fictional. `127.0.0.1` is deliberately the default, suitable
only for a single-machine configuration inspection or local transport test. It does
**not** connect the example machines to one another. For a real fleet, copy the
profile outside the repository, replace its workers with actual roster names and
verified capabilities, and set the coordinator's reachable restricted-network IPv4:

```sh
python3 infra/buildbarn/configure.py \
  --profile "$HOME/.config/ferryx/buildbarn-profile.json" \
  --output "$HOME/.config/ferryx/buildbarn-rendered-v1"
```

`--hosts` defaults to `$FERRYX_BUILD_HOSTS_FILE`, otherwise
`~/.config/ferryx/build-hosts`, just like the picker. Optional disk/RAM floors and
comments are accepted. They are **not** a worker admission/load policy. Choose
concurrency conservatively for RAM and native build requirements. Output refuses
existing directories; it is written atomically with a private top-level directory
and mode-0600 files. Never commit rendered files, SSH destinations or private profiles.

## Deployment contract (manual, not executed by this renderer)

Configuration fields are based on the upstream `bare/config` and native hardlinking
worker examples at
[bb-deployments 7cd419f](https://github.com/buildbarn/bb-deployments/tree/7cd419f6526ea5d1f25b0803dee32d64f26fbdec/bare/config).
Use the corresponding binaries, not arbitrary older releases:

- `bb-storage`: source revision `ca3d8b49a5af52eb02ed473dd689d19b949c9e41`.
- `bb-scheduler`, `bb-worker`, `bb-runner`: source revision `c9ba98cf52456c400278f45a9679635daa40dcd2`.
- Native macOS workers require native macOS binaries, not Linux Docker containers.
- Buck platform API follows the [upstream Buildbarn example](https://github.com/meta/buck2/tree/main/examples/remote_execution/buildbarn).
  Pin the Buck2 binary after a successful fleet smoke run; this PR does not claim a
  tested Buck2 binary/version combination.

Before starting anything:

1. Dedicate worker capacity. **Do not use the old picker on these hosts concurrently.**
   Its caller-local `/tmp` claims do not see Buildbarn queues. Retain a full inventory
   for this renderer; give legacy picker jobs a separate `FERRYX_BUILD_HOSTS_FILE`
   excluding cluster-owned hosts. This PR does not alter the picker or its semantics.
2. Use an unprivileged, dedicated build account without SSH keys, cloud credentials,
   signing keys or access to personal files. Native runners execute submitted code
   and are **not a sandbox**. Limit trusted submitters to the owner's build clients.
3. Establish restricted authenticated network access **before** binding beyond
   loopback. Generated REAPI uses plaintext `grpc` with `allow` policies. A private
   IP is not authentication. Do not expose ports to public networks, shared LANs or
   untrusted VPN peers. Use an existing tightly ACL-restricted encrypted overlay,
   or add a reviewed mTLS gateway/configuration before broadening access. The
   renderer rejects wildcard/public coordinator IPs but cannot verify firewall/ACLs.
4. Reserve at least 40 GiB coordinator storage plus build space on workers. The
   central CAS block file is 32 GiB; worker caches/file pools are 1 GiB each. Native
   build inputs and outputs need additional disk space. Create the directories in
   `manifest.json` with mode 0700 and ownership matching the service account.

Copy only the coordinator directory to the coordinator and each worker directory
to its matching host (plus their manifest directory instructions). These commands
are an operator runbook, **not run automatically**. Run each long-lived process
in its own supervised service after installing approved matching binaries:

```sh
# Coordinator, three separate processes. bb-storage serves both storage and frontend.
bb_storage /absolute/config/coordinator/storage.json
bb_scheduler /absolute/config/coordinator/scheduler.json
bb_storage /absolute/config/coordinator/frontend.json

# Each native worker, two separate processes, same user and state paths.
bb_runner /absolute/config/workers/HOST/runner.json
bb_worker /absolute/config/workers/HOST/worker.json
```

Network matrix: build clients → coordinator 8980; workers → coordinator 8981 (CAS)
and 8983 (scheduler). Coordinator 8982 (execution) and 8984 (administration) bind
loopback only. Runner RPC is a host-local Unix socket, never a network listener.
No diagnostics or admin HTTP listener is enabled. Stop admissions/drain and wait
for running actions before stopping workers; do not kill unrelated Ferryx daemons.

## Verify real execution, without local fallback

Copy the desired generated `buck2/<os>-<arch>-<toolchain>` cell to a build client.
Each cell requests a capability queue, not a hostname. From that cell:

```sh
buck2 build //:smoke -c smoke.nonce="$(date +%s)-$$" --show-output
```

The eight actions run with `local_enabled = False`. The nonce changes action inputs
so subsequent invocations do not merely return the previous cached result. Inspect
all outputs (hostname plus marker), scheduler worker registrations and execution
logs. Fast actions can all land on one host; eight actions alone do not prove fair
load balancing or a speedup. For an acceptance check use at least two same-profile
workers, enough concurrent nontrivial actions, and record action IDs/worker IDs.
Stop all matching workers and use a fresh nonce: the build must fail or time out,
not silently run locally. These tests have **not** been run on the real fleet here.

## Before Ferryx/mhc builds use this cluster

The generated cells only check REAPI transport and scheduling. A `cargo build`
inside one Buck action still runs on one worker. Actual parallel crate actions need
Buck targets/toolchains, declared sources/outputs, dependency graphs, `build.rs`
outputs and native tools. Do not label this PR a completed distributed Rust build.

Migrate headless targets incrementally, pin the complete compiler/SDK/native library
set in each toolchain profile, and test cache invalidation. Keep Ferryx Tauri UI,
Ghostty/Zig, FFmpeg, platform SDK handling, app packaging and signing on the existing
path until explicitly ported. Never upload signing credentials to CAS. Distributed
ThinLTO is a separate compiler/linker integration, not enabled by this configuration.

## Validation scope

The standard-library test suite exercises roster validation, capability queue
matching, endpoint graph consistency, supported/unsupported host behavior, private
output handling, CLI failure paths and remote-only smoke generation. It does not
validate protobuf deserialization by actual service binaries, Buck2 interpretation,
network reachability, sandboxing, real SDK correctness or full application builds.
Those are deployment acceptance gates, not claimed passes.
