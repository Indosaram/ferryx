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
            }
        })?;
        Ok(Self {
            jobs: Some(tx),
            handle: Some(handle),
        })
    }

    pub fn submit(&self, job: impl FnOnce(&GpuThread) + Send + 'static) -> bool {
        self.jobs
            .as_ref()
            .map(|tx| tx.send(Box::new(job)).is_ok())
            .unwrap_or(false)
    }

    pub fn shutdown(&mut self) {
        self.jobs = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for GpuWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};

    #[test]
    fn gpu_jobs_run_off_the_calling_thread() {
        let worker = GpuWorker::new("ferryx-gpu-test").expect("worker must spawn");
        let (tx, rx) = mpsc::channel();
        let caller = thread::current().id();
        assert!(worker.submit(move |_gpu| {
            tx.send(thread::current().id()).expect("send worker thread id");
        }));
        let ran_on = rx.recv().expect("job must run");
        assert_ne!(ran_on, caller, "GPU work must not run on the calling thread");
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
}
