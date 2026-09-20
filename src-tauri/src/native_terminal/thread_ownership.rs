//! Thread-ownership tokens for the native terminal render path.
//!
//! GPU acquisition currently runs inside a main-thread dispatch, where `get_current_texture`
//! blocks in `-[CAMetalLayer nextDrawable]`. The main thread is also the only thread servicing
//! WebKit IPC, so a stalled compositor stalls terminal output itself.
//!
//! The CI pattern rule cannot catch this: the acquisition lives in a different function from the
//! dispatch closure, and a lexical rule cannot follow that call. A token that cannot cross
//! threads can, because the compiler checks every call site.

use std::marker::PhantomData;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

/// Fails the build if `$t` implements `$trait`, via an ambiguous associated-item call.
///
/// Checked on every compile, so deleting the `PhantomData` that makes a token thread-bound
/// stops the crate from building rather than silently widening the contract.
macro_rules! assert_not_impl {
    ($t:ty, $trait:path) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            impl<T: ?Sized + $trait> AmbiguousIfImpl<u8> for T {}
            let _ = <$t as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

/// Proof that the holder runs on the GPU worker thread. Obtainable only inside a
/// [`GpuWorker`] job, and cannot be captured into a closure sent elsewhere.
#[derive(Debug)]
pub struct GpuThread(PhantomData<*const ()>);

/// Proof that the holder runs on the UI thread, for surface creation and native view mutation
/// that are legitimately main-thread-only.
#[derive(Debug)]
pub struct UiThread(PhantomData<*const ()>);

assert_not_impl!(GpuThread, Send);
assert_not_impl!(GpuThread, Sync);
assert_not_impl!(UiThread, Send);
assert_not_impl!(UiThread, Sync);

impl UiThread {
    /// # Safety
    /// The caller must be on the UI thread. This is the one unchecked boundary; every downstream
    /// obligation is then compiler-checked.
    pub unsafe fn assume_current() -> Self {
        Self(PhantomData)
    }
}

/// Single worker thread owning GPU submission. Jobs receive a [`GpuThread`], so work that must
/// stay off the UI thread can be typed to require one.
pub struct GpuWorker {
    jobs: Option<mpsc::Sender<Job>>,
    handle: Option<thread::JoinHandle<()>>,
}

type Job = Box<dyn FnOnce(&GpuThread) + Send + 'static>;

impl GpuWorker {
    pub fn new(name: impl Into<String>) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = thread::Builder::new().name(name.into()).spawn(move || {
            let token = GpuThread(PhantomData);
            while let Ok(job) = rx.recv() {
                job(&token);
                #[cfg(target_os = "macos")]
                // This worker has no run loop to commit implicit CAMetalLayer resize changes.
                // SAFETY: CATransaction.flush commits only the calling thread's transaction.
                unsafe {
                    let _: () = objc2::msg_send![objc2::class!(CATransaction), flush];
                }
            }
        })?;
        Ok(Self {
            jobs: Some(tx),
            handle: Some(handle),
        })
    }

    pub fn enqueue(&self, job: impl FnOnce(&GpuThread) + Send + 'static) -> bool {
        self.jobs
            .as_ref()
            .map(|tx| tx.send(Box::new(job)).is_ok())
            .unwrap_or(false)
    }

    pub fn submit(&self, job: impl FnOnce(&GpuThread) + Send + 'static) -> bool {
        self.enqueue(job)
    }

    pub fn shutdown(&mut self) {
        self.jobs = None;
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for GpuWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct LentCell<T> {
    value: Option<T>,
    retired: bool,
    disposer: Option<Box<dyn FnOnce() + Send + 'static>>,
}

/// A value lent to another thread and guaranteed to come back before its owner tears down,
/// or safely disposed along with retained target resources via safe asynchronous retirement.
///
/// The GPU leg travels to the worker while the native child view stays behind. The view may only
/// be destroyed once the leg (and the `wgpu::Surface` inside it) has ceased to exist. Rather than
/// blocking the UI thread on an arbitrary timeout, [`retire_with`] ensures that if the value is
/// lent out, the target disposer is deferred until the borrower returns the value, at which point
/// the value is dropped first, and the native platform target second.
///
/// [`retire_with`]: LentSlot::retire_with
pub struct LentSlot<T> {
    cell: Arc<(Mutex<LentCell<T>>, Condvar)>,
}

impl<T> Clone for LentSlot<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Arc::clone(&self.cell),
        }
    }
}

/// RAII borrow of a lent value that automatically returns it to [`LentSlot`] upon drop.
pub struct LentLoan<T> {
    slot: LentSlot<T>,
    value: Option<T>,
}

impl<T> LentLoan<T> {
    pub fn return_to_slot(mut self) {
        if let Some(value) = self.value.take() {
            self.slot.give_back(value);
        }
    }
}

impl<T> std::ops::Deref for LentLoan<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.value.as_ref().expect("loan value already returned")
    }
}

impl<T> std::ops::DerefMut for LentLoan<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().expect("loan value already returned")
    }
}

impl<T> Drop for LentLoan<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.slot.give_back(value);
        }
    }
}

impl<T> LentSlot<T> {
    pub fn new(value: T) -> Self {
        Self {
            cell: Arc::new((
                Mutex::new(LentCell {
                    value: Some(value),
                    retired: false,
                    disposer: None,
                }),
                Condvar::new(),
            )),
        }
    }

    pub fn lend(&self) -> Option<T> {
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        if guard.retired {
            None
        } else {
            guard.value.take()
        }
    }

    pub fn lend_loan(&self) -> Option<LentLoan<T>> {
        self.lend().map(|value| LentLoan {
            slot: self.clone(),
            value: Some(value),
        })
    }

    pub fn give_back(&self, value: T) {
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        if guard.retired {
            // The slot was retired while this value was out. Drop the value FIRST,
            // then trigger any deferred target disposer SECOND.
            let disposer = guard.disposer.take();
            drop(guard);
            drop(value);
            if let Some(disposer) = disposer {
                disposer();
            }
        } else {
            guard.value = Some(value);
            self.cell.1.notify_all();
        }
    }

    /// Retires the slot asynchronously without blocking the calling thread.
    ///
    /// If the value is currently at home, it is dropped immediately and then `disposer` is run.
    /// If the value is currently lent out to a worker, `disposer` is retained and will be executed
    /// when the loan completes, strictly AFTER the value has been dropped.
    pub fn retire_with(&self, disposer: impl FnOnce() + Send + 'static) {
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        guard.retired = true;
        if let Some(value) = guard.value.take() {
            drop(guard);
            drop(value);
            disposer();
        } else {
            guard.disposer = Some(Box::new(disposer));
        }
    }

    pub fn is_retired(&self) -> bool {
        self.cell.0.lock().expect("lent slot poisoned").retired
    }

    /// Runs `f` against the value while it is at home. `None` means it is currently lent out or retired.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        if guard.retired {
            None
        } else {
            guard.value.as_mut().map(f)
        }
    }

    /// Immediately takes the value if present at home, without waiting.
    pub fn try_reclaim(&self) -> Option<T> {
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        guard.value.take()
    }

    /// Waits up to `deadline` for the value to come home, then removes it.
    pub fn reclaim_within(&self, deadline: std::time::Duration) -> Option<T> {
        let start = std::time::Instant::now();
        let mut guard = self.cell.0.lock().expect("lent slot poisoned");
        while guard.value.is_none() && !guard.retired {
            let remaining = deadline.checked_sub(start.elapsed())?;
            let (next, _) = self
                .cell
                .1
                .wait_timeout(guard, remaining)
                .expect("lent slot poisoned");
            guard = next;
        }
        guard.value.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};

    #[test]
    fn reclaim_spends_its_deadline_waiting_for_a_value_that_is_still_lent() {
        let slot = LentSlot::new(7u32);
        let _borrowed = slot.lend().expect("value starts present");
        let budget = std::time::Duration::from_millis(80);
        let start = std::time::Instant::now();
        assert_eq!(slot.reclaim_within(budget), None, "the value is still out");
        assert!(
            start.elapsed() >= budget / 2,
            "reclaim must wait for the borrower instead of reporting an absent value at once"
        );
    }

    #[test]
    fn reclaim_returns_the_value_the_borrower_gave_back() {
        let slot = LentSlot::new(7u32);
        let borrowed = slot.lend().expect("value starts present");
        let giver = slot.clone();
        let handle = thread::spawn(move || giver.give_back(borrowed));
        assert_eq!(
            slot.reclaim_within(std::time::Duration::from_secs(5)),
            Some(7)
        );
        handle.join().expect("giver thread joins");
    }

    #[test]
    fn retired_disposer_runs_without_holding_slot_mutex() {
        let slot = LentSlot::new(7u32);
        let loan = slot.lend_loan().unwrap();
        let observer = slot.clone();
        slot.retire_with(move || {
            assert!(observer.cell.0.try_lock().is_ok());
        });
        loan.return_to_slot();
    }

    #[test]
    fn retired_frame_cannot_authorize_a_replacement_target() {
        let original = LentSlot::new(1u32);
        let completion = original.clone();
        let loan = original.lend_loan().unwrap();
        original.retire_with(|| {});
        let replacement = LentSlot::new(2u32);
        loan.return_to_slot();
        assert!(completion.is_retired());
        assert!(!replacement.is_retired());
        assert!(completion.lend_loan().is_none());
    }

    #[test]
    fn reclaim_is_immediate_when_nothing_is_lent() {
        let slot = LentSlot::new(3u32);
        assert_eq!(
            slot.reclaim_within(std::time::Duration::from_secs(5)),
            Some(3)
        );
    }

    #[test]
    fn gpu_jobs_run_off_the_calling_thread() {
        let worker = GpuWorker::new("ferryx-gpu-test").expect("worker must spawn");
        let (tx, rx) = mpsc::channel();
        let caller = thread::current().id();
        assert!(worker.submit(move |_gpu| {
            tx.send(thread::current().id())
                .expect("send worker thread id");
        }));
        let ran_on = rx.recv().expect("job must run");
        assert_ne!(
            ran_on, caller,
            "GPU work must not run on the calling thread"
        );
    }

    #[test]
    fn jobs_run_in_submission_order_on_one_thread() {
        let worker = GpuWorker::new("ferryx-gpu-order").expect("worker must spawn");
        let (tx, rx) = mpsc::channel();
        for index in 0..16 {
            let tx = tx.clone();
            assert!(worker.submit(move |_gpu| {
                tx.send((index, thread::current().id())).expect("send");
            }));
        }
        drop(tx);
        let observed: Vec<_> = rx.iter().collect();
        assert_eq!(observed.len(), 16, "every job must run");
        assert_eq!(
            observed.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            (0..16).collect::<Vec<_>>(),
            "submission order must hold so frames cannot present out of order"
        );
        let threads: std::collections::HashSet<_> = observed.iter().map(|(_, id)| *id).collect();
        assert_eq!(threads.len(), 1, "one device, one owning thread");
    }

    #[test]
    fn last_worker_owner_can_be_released_inside_its_job() {
        let worker = Arc::new(GpuWorker::new("ferryx-gpu-self-drop").unwrap());
        let last_owner = Arc::clone(&worker);
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        assert!(worker.enqueue(move |_| {
            release_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            drop(last_owner);
            done_tx.send(()).unwrap();
        }));
        drop(worker);
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn shutdown_drains_queued_jobs_before_returning() {
        // Returning while a frame is still encoding would race teardown against an in-flight
        // submission, which is a use-after-free of the surface on the platform side.
        let ran = Arc::new(AtomicUsize::new(0));
        let mut worker = GpuWorker::new("ferryx-gpu-drain").expect("worker must spawn");
        for _ in 0..32 {
            let ran = Arc::clone(&ran);
            assert!(worker.submit(move |_gpu| {
                ran.fetch_add(1, Ordering::SeqCst);
            }));
        }
        worker.shutdown();
        assert_eq!(
            ran.load(Ordering::SeqCst),
            32,
            "shutdown must join the worker, not abandon queued frames"
        );
        assert!(
            !worker.submit(|_gpu| {}),
            "a shut-down worker must reject work instead of panicking"
        );
    }

    struct DropTracker {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Drop for DropTracker {
        fn drop(&mut self) {
            self.log.lock().expect("log lock").push(self.name);
        }
    }

    #[test]
    fn retire_when_at_home_drops_value_before_target_immediately() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let slot = LentSlot::new(DropTracker {
            name: "surface",
            log: Arc::clone(&log),
        });
        let target = DropTracker {
            name: "target",
            log: Arc::clone(&log),
        };

        slot.retire_with(move || drop(target));

        assert_eq!(
            *log.lock().unwrap(),
            vec!["surface", "target"],
            "when value is at home, retire_with must drop value first, then target second"
        );
        assert!(slot.is_retired());
    }

    #[test]
    fn retire_while_lent_retains_target_without_blocking_and_drops_after_loan_returns() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let slot = LentSlot::new(DropTracker {
            name: "surface",
            log: Arc::clone(&log),
        });
        let target = DropTracker {
            name: "target",
            log: Arc::clone(&log),
        };

        let loan = slot.lend_loan().expect("value starts present");
        let (borrower_started_tx, borrower_started_rx) = mpsc::channel();
        let (retire_done_tx, retire_done_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            borrower_started_tx.send(()).unwrap();
            retire_done_rx.recv().unwrap();
            // At this point, retire_with has already run on the main thread!
            // Surface and target must NOT have been dropped yet!
            loan.return_to_slot();
            finished_tx.send(()).unwrap();
        });

        borrower_started_rx.recv().unwrap();

        // UI thread retires slot: returns without waiting for borrower.
        // If retire_with blocked on the borrower, this would deadlock because
        // the borrower is blocked on retire_done_rx.
        slot.retire_with(move || drop(target));

        // Target must NOT be dropped yet while loan is out
        assert!(
            log.lock().unwrap().is_empty(),
            "target must be retained while surface loan is still active"
        );

        // Tell worker to finish
        retire_done_tx.send(()).unwrap();
        finished_rx.recv().unwrap();
        handle.join().unwrap();

        // After loan is returned, surface was dropped first, then target second
        assert_eq!(
            *log.lock().unwrap(),
            vec!["surface", "target"],
            "after loan returns, surface must drop first, target second"
        );
        assert!(slot.is_retired());
    }
}
