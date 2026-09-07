# Receipt delta: pristine wire RED and final reapplication

The existing delta works, but its first Rust RED used an internal optional-field
scaffold. Preserve that history; do not relabel it as a pristine pre-edit RED.
Perform the explicit recovery required by the test-first contract.

1. Verify/save the current four-file owned delta and its passing source identity.
2. Restore only that owned receipt/UI delta to its recorded entry baseline using
   `apply_patch`. Preserve original D3 geometry, D2 guards and the Windows commit.
   Do not restore any file wholesale from HEAD or alter foreign changes.
3. Add a Rust test in the IPC test module, where the existing private
   `into_ipc_receipt` converter is accessible. Create/store the real resolved
   presentation state, obtain the real host receipt, convert/serialize it through
   the existing wire path, and assert JSON `effectiveScaleFactor == 2.0`.
   This compiles against the old structs: absence of a JSON field is the
   behavioral failure, so no new production field or Eq change is needed for RED.
4. Restore the new UI tests only, with production UI conversion still old. Run
   both new wire RED and fractional anchor RED on that baseline. Capture the
   source diff showing only tests changed, executed count and exact assertion.
5. Reapply the saved owned production fix forward, retaining the new wire test,
   the original host getter regression and every UI/D2 assertion.
6. Capture identical wire/UI GREEN, relevant receipt/host/geometry/lifecycle
   tests, affected normal checks/builds, source hashes and cleanup. Reuse earlier
   unchanged native/browser artifact coverage only when its exact inputs match;
   do not relabel stale results to a changed source.

New wire RED/GREEN command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal::tests::wire_receipt_reports_effective_presentation_scale -- --exact --nocapture
```

The fractional UI command remains unchanged. This is a new, honestly recorded
attempt: the earlier scaffold qualification remains historical, and the final
production reapplication occurs after the fresh baseline RED.

Write scope remains the four receipt/UI files already authorized. No staging,
commit, main edit, renderer/platform change or desktop input. Update the existing
delta report with this attempt's chronology and evidence; native GUI acceptance
remains pending.
