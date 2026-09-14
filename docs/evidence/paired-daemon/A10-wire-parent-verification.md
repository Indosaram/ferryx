# Parent standalone terminal codec verification

The parent read the complete codec and harness, reconstructed their original
write scope, and read the recovery RED/GREEN command logs. The recovery changed
no codec source after the initial implementation.

From `herdr-wave2`, the parent independently compiled and executed:

```sh
rustc --edition=2021 --test src-tauri/tests/a10_terminal_wire_codec.rs \
  -L dependency=src-tauri/target/debug/deps \
  --extern serde=src-tauri/target/debug/deps/libserde-20103001e2270c27.rlib \
  --extern serde_json=src-tauri/target/debug/deps/libserde_json-bbb1dc538a584af2.rlib \
  -o src-tauri/target/a10-wire-parent-green
src-tauri/target/a10-wire-parent-green --nocapture
```

Monitor `mon_XYNWBG6JG4FNS01H` / `bash_318` completed exit 0 with
`PARENT_CODEC_EXIT=0`. All five tests passed; none failed or was ignored.
Full test output: `A10-wire-parent-green.log`. Compiler success was required
before executing the binary.

The assertions cover canonical metadata framing, retained reset bytes, raw PTY
payload separation, u64 values above JavaScript precision and at the maximum,
fragment boundaries, arbitrary bytes, malformed/truncated metadata and allocation
bounds. The controlled RED selects metadata pass-through and fails the payload
assertion; it is not a claim that an existing native proxy was exercised.

This accepts only the standalone codec. Machine attachment, controller fencing,
session ownership, replay policy, real PTYs, relay transport and native rendering
still require A09/A10 and the aggregate gate. No listener, daemon or PTY was
created by this parent run; the compiler and test process exited. Existing
private compiler artifacts remain. No Cargo overlap or source edit occurred.
