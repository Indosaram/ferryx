# Native QA-feature caller/barrier reconciliation
Windows qa_barrier --list native101,29errors44warnings. Native caller owns reconciliation with backend barrier API.
```text
error[E0599]: no method named `schedule_cancellation_receipt` found for reference `&std::sync::Arc<qa_barrier::QaBarrierChannel>` in the current scope
   --> src\native_terminal\surface_host.rs:651:13
    |
651 |     channel.schedule_cancellation_receipt(
    |     --------^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `&std::sync::Arc<qa_barrier::QaBarrierChannel>`

error[E0599]: no method named `schedule_cancellation_receipt` found for struct `std::sync::Arc<qa_barrier::QaBarrierChannel>` in the current scope
   --> src\native_terminal\surface_host.rs:706:29
    |
706 |                     channel.schedule_cancellation_receipt(
    |                     --------^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `std::sync::Arc<qa_barrier::QaBarrierChannel>`

error[E0599]: no method named `try_claim` found for struct `std::sync::Arc<qa_barrier::QaBarrierChannel>` in the current scope
   --> src\native_terminal\surface_host.rs:728:25
    |
728 |             if !channel.try_claim(crate::ipc::qa_barrier::PRESENTATION_BARRIER, &session_id, &spec.operation_id) {
    |                         ^^^^^^^^^ method not found in `std::sync::Arc<qa_barrier::QaBarrierChannel>`

error[E0599]: no method named `release_claim` found for struct `std::sync::Arc<qa_barrier::QaBarrierChannel>` in the current scope
   --> src\native_terminal\surface_host.rs:778:31
    |
778 |                 channel_clone.release_claim(
    |                 --------------^^^^^^^^^^^^^ method not found in `std::sync::Arc<qa_barrier::QaBarrierChannel>`

error[E0599]: no method named `active_presentation_generation` found for reference `&surface_host::NativeTerminalSurfaceHost` in the current scope
   --> src\native_terminal\surface_host.rs:971:38
    |
971 | ...                   host.active_presentation_generation() == Some(frame_generation)
    |                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
```
