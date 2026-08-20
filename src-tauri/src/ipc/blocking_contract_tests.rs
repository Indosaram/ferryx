use super::run_blocking;

#[tokio::test]
async fn blocking_ipc_helper_runs_operation_off_async_caller_thread() {
    let caller_thread = std::thread::current().id();
    let worker_thread = run_blocking(|| Ok(std::thread::current().id()))
        .await
        .expect("blocking operation");
    assert_ne!(caller_thread, worker_thread);
}
