//! Native capture handoff. Only unencoded frames may be superseded; encoder
//! output continues through the session's ordered, backpressured media channel.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
#[cfg(any(target_os = "linux", test))]
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Controls {
    pub force_keyframe: bool,
    pub bitrate: Option<u32>,
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
#[derive(Debug)]
pub(super) struct Work<T> {
    pub frame: Option<T>,
    pub controls: Controls,
}

struct State<T> {
    active: bool,
    stopped: bool,
    frame: Option<T>,
    controls: Controls,
}

pub(super) struct Handoff<T> {
    state: Mutex<State<T>>,
    #[cfg(target_os = "linux")]
    audio_error: Mutex<Option<String>>,
    wake: Condvar,
    // Read-only outside stop(): native readiness loops need an atomic reference.
    cancelled: AtomicBool,
    #[cfg(target_os = "linux")]
    readiness_wake: (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ),
    #[cfg(test)]
    waiting: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl<T> Handoff<T> {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            audio_error: Mutex::new(None),
            state: Mutex::new(State {
                active: false,
                stopped: false,
                frame: None,
                controls: Controls::default(),
            }),
            wake: Condvar::new(),
            cancelled: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            readiness_wake: std::os::unix::net::UnixStream::pair()
                .expect("native readiness wake socket"),
            #[cfg(test)]
            waiting: Mutex::new(None),
        }
    }

    #[cfg(any(target_os = "linux", test))]
    pub fn cancellation(&self) -> &AtomicBool {
        &self.cancelled
    }

    #[cfg(target_os = "linux")]
    pub fn report_audio_unavailable(&self, error: String) {
        *self.audio_error.lock().expect("audio status poisoned") = Some(error);
    }

    #[cfg(target_os = "linux")]
    pub fn take_audio_error(&self) -> Option<String> {
        self.audio_error
            .lock()
            .expect("audio status poisoned")
            .take()
    }

    pub fn is_stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    #[cfg(target_os = "linux")]
    pub fn readiness_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;
        self.readiness_wake.0.as_fd()
    }

    pub fn activate(&self) {
        let mut state = self.state.lock().expect("native handoff poisoned");
        state.active = true;
        self.wake.notify_all();
    }

    fn await_activation(&self) -> bool {
        let state = self.state.lock().expect("native handoff poisoned");
        let state = self
            .wake
            .wait_while(state, |s| !s.active && !s.stopped)
            .expect("native handoff poisoned");
        !state.stopped
    }

    #[cfg(any(target_os = "windows", target_os = "linux", test))]
    pub fn publish(&self, frame: T) -> bool {
        let mut state = self.state.lock().expect("native handoff poisoned");
        if state.stopped {
            return false;
        }
        state.frame = Some(frame);
        self.wake.notify_all();
        true
    }

    #[cfg(any(target_os = "windows", target_os = "linux", test))]
    pub fn control(&self, force_keyframe: bool, bitrate: Option<u32>) -> bool {
        let mut state = self.state.lock().expect("native handoff poisoned");
        if state.stopped {
            return false;
        }
        state.controls.force_keyframe |= force_keyframe;
        if bitrate.is_some() {
            state.controls.bitrate = bitrate;
        }
        self.wake.notify_all();
        true
    }

    #[cfg(any(target_os = "windows", target_os = "linux", test))]
    pub fn next(&self) -> Option<Work<T>> {
        let mut state = self.state.lock().expect("native handoff poisoned");
        loop {
            if state.stopped {
                return None;
            }
            if state.frame.is_some() || state.controls != Controls::default() {
                return Some(Work {
                    frame: state.frame.take(),
                    controls: std::mem::take(&mut state.controls),
                });
            }
            #[cfg(test)]
            if let Some(waiting) = self.waiting.lock().unwrap().take() {
                waiting.send(()).unwrap();
            }
            state = self.wake.wait(state).expect("native handoff poisoned");
        }
    }

    /// Linearization point before submission: replace raw work only, and merge
    /// controls without losing keyframe intent across a bitrate reopen.
    #[cfg(any(target_os = "windows", target_os = "linux", test))]
    pub fn reselect(&self, work: &mut Work<T>) -> bool {
        let mut state = self.state.lock().expect("native handoff poisoned");
        if state.stopped {
            return false;
        }
        if state.frame.is_some() {
            work.frame = state.frame.take();
        }
        work.controls.force_keyframe |= state.controls.force_keyframe;
        state.controls.force_keyframe = false;
        if state.controls.bitrate.is_some() {
            work.controls.bitrate = state.controls.bitrate.take();
        }
        true
    }

    /// Capture cadence is timed, but shutdown wakes it immediately. Controls
    /// share this Condvar without shortening the next capture interval.
    pub fn pace_until(&self, deadline: Instant) {
        let mut state = self.state.lock().expect("native handoff poisoned");
        while !state.stopped {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            (state, _) = self
                .wake
                .wait_timeout(state, remaining)
                .expect("native handoff poisoned");
        }
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().expect("native handoff poisoned");
        state.stopped = true;
        state.frame = None;
        state.controls = Controls::default();
        self.cancelled.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        {
            // Idempotent shutdown wakes poll without filling a pipe.
            let _ = self.readiness_wake.1.shutdown(std::net::Shutdown::Both);
        }
        self.wake.notify_all();
    }
}

struct StopOnExit<T>(Arc<Handoff<T>>);

impl<T> Drop for StopOnExit<T> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// Startup is gated until all spawns succeed. A partial startup therefore owns
/// and reaps workers without letting any of them block on media output first.
pub(super) struct Workers<T> {
    pub handoff: Arc<Handoff<T>>,
    joins: Vec<JoinHandle<()>>,
}

impl<T: Send + 'static> Workers<T> {
    pub fn new() -> Self {
        Self {
            handoff: Arc::new(Handoff::new()),
            joins: Vec::new(),
        }
    }

    pub fn spawn(
        &mut self,
        name: &str,
        critical: bool,
        worker: impl FnOnce(Arc<Handoff<T>>) + Send + 'static,
    ) -> io::Result<()> {
        let handoff = Arc::clone(&self.handoff);
        let join = thread::Builder::new().name(name.into()).spawn(move || {
            if !handoff.await_activation() {
                return;
            }
            #[cfg(target_os = "windows")]
            let _mmcss = crate::windows_logic::MultimediaTask::register(false);
            let _stop_on_exit = critical.then(|| StopOnExit(Arc::clone(&handoff)));
            worker(handoff);
        })?;
        self.joins.push(join);
        Ok(())
    }
}

impl<T> Workers<T> {
    pub fn stop(&mut self) {
        self.handoff.stop();
        for worker in self.joins.drain(..) {
            let name = worker.thread().name().unwrap_or("native media").to_owned();
            if worker.join().is_err() {
                // Keep reaping the other workers even when one panicked.
                eprintln!("{name} worker panicked");
            }
        }
    }
}

impl<T> Drop for Workers<T> {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(super) trait Encoder<T> {
    type Output;
    type Error;
    fn selected(&mut self, _frame: &T) {}
    fn force_keyframe(&mut self);
    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<Self::Output>, Self::Error>;
    fn encode(&mut self, frame: T) -> Result<Vec<Self::Output>, Self::Error>;
}

/// Shared by both native adapters and scripted tests. A bitrate reopen can
/// drain older packets: emit those before submitting the next input.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(super) fn run_encoder<T, E: Encoder<T>>(
    handoff: &Handoff<T>,
    initialize: impl FnOnce() -> Result<E, E::Error>,
    mut emit: impl FnMut(E::Output) -> bool,
) -> Result<(), E::Error> {
    let mut encoder = initialize()?;
    while let Some(mut work) = handoff.next() {
        loop {
            if let Some(bitrate) = work.controls.bitrate.take() {
                if let Some(trace) = super::host_trace::enabled() {
                    trace.record(super::host_trace::Record {
                        event: 30,
                        value: u64::from(bitrate),
                        ..Default::default()
                    });
                }
                let packets = encoder.bitrate(bitrate)?;
                if let Some(trace) = super::host_trace::enabled() {
                    trace.record(super::host_trace::Record {
                        event: 31,
                        value: u64::from(bitrate),
                        ..Default::default()
                    });
                }
                for packet in packets {
                    if !emit(packet) {
                        handoff.stop();
                        return Ok(());
                    }
                }
            }
            if !handoff.reselect(&mut work) {
                return Ok(());
            }
            if work.controls.bitrate.is_none() {
                break;
            }
        }
        if work.controls.force_keyframe {
            encoder.force_keyframe();
        }
        if let Some(frame) = work.frame {
            encoder.selected(&frame);
            for packet in encoder.encode(frame)? {
                if !emit(packet) {
                    handoff.stop();
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
pub(super) struct AudioBlock {
    pub pcm: Vec<u8>,
    pub captured_at: Instant,
}

/// One notification represents the slot, not the block present when queued.
/// Replacing whole stereo blocks never blocks the recorder behind encoded video.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Default)]
pub(super) struct LatestAudio {
    slot: Mutex<(Option<AudioBlock>, bool)>,
}

#[cfg(any(target_os = "linux", test))]
impl LatestAudio {
    pub const MAX_AGE: Duration = Duration::from_millis(100);

    pub fn publish(&self, block: AudioBlock) -> bool {
        let mut slot = self.slot.lock().expect("native audio slot poisoned");
        slot.0 = Some(block);
        if slot.1 {
            return false;
        }
        slot.1 = true;
        true
    }

    pub fn notification_rejected(&self) {
        self.slot.lock().expect("native audio slot poisoned").1 = false;
    }

    pub fn take_fresh(&self, now: Instant) -> Option<AudioBlock> {
        let mut slot = self.slot.lock().expect("native audio slot poisoned");
        slot.1 = false;
        slot.0
            .take()
            .filter(|block| now.saturating_duration_since(block.captured_at) <= Self::MAX_AGE)
    }
}

/// Linux outputs identify accepted inputs by PTS, including packets drained by
/// bitrate reconfiguration. Never label delayed output with the newest input.
#[cfg(any(target_os = "linux", test))]
pub(super) struct FrameTimes {
    pending: std::collections::BTreeMap<i64, (Instant, Instant)>,
    next_pts: i64,
}

#[cfg(any(target_os = "linux", test))]
impl FrameTimes {
    pub fn new() -> Self {
        Self {
            pending: std::collections::BTreeMap::new(),
            next_pts: 0,
        }
    }

    pub fn submitted(&mut self, capture_at: Instant, encode_started_at: Instant) {
        self.pending
            .insert(self.next_pts, (capture_at, encode_started_at));
        self.next_pts += 1;
    }

    pub fn take(&mut self, pts: i64) -> Option<(Instant, Instant)> {
        self.pending.remove(&pts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod stalls {
        include!("native_stall_tests.rs");
    }
    use std::sync::mpsc::{self, Receiver};

    const BOUND: Duration = Duration::from_secs(2);

    fn receive<T>(rx: &Receiver<T>) -> T {
        rx.recv_timeout(BOUND).expect("bounded worker completion")
    }

    fn subscribe_wait<T>(handoff: &Handoff<T>) -> Receiver<()> {
        let (tx, rx) = mpsc::channel();
        *handoff.waiting.lock().unwrap() = Some(tx);
        rx
    }

    fn stop_bounded<T: Send + 'static>(mut workers: Workers<T>) {
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            workers.stop();
            assert!(workers.joins.is_empty());
            tx.send(()).unwrap();
        });
        receive(&rx);
        join.join().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn audio_failure_is_reported_without_stopping_video() {
        let handoff = Handoff::<u32>::new();
        handoff.activate();
        handoff.report_audio_unavailable("recorder EOF".into());
        assert!(!handoff.is_stopped());
        assert!(handoff.publish(7));
        assert_eq!(handoff.take_audio_error().as_deref(), Some("recorder EOF"));
        assert_eq!(handoff.take_audio_error(), None);
        assert_eq!(handoff.next().unwrap().frame, Some(7));
        handoff.stop();
    }

    #[test]
    fn latest_raw_survives_encoder_backpressure() {
        // Given a busy encoder, capture publishes three complete frames.
        let handoff = Handoff::new();
        for id in [2, 3, 4] {
            assert!(handoff.publish(vec![id]));
        }
        // When the encoder asks for its next input, it gets the newest capture.
        assert_eq!(handoff.next().unwrap().frame, Some(vec![4]));
    }

    #[test]
    fn newest_bitrate_and_keyframe_survive_raw_replacement() {
        let handoff = Handoff::new();
        handoff.control(true, Some(8_000_000));
        handoff.publish(1);
        handoff.control(false, Some(2_000_000));
        handoff.publish(2);
        let work = handoff.next().unwrap();
        assert_eq!(
            work.controls,
            Controls {
                force_keyframe: true,
                bitrate: Some(2_000_000)
            }
        );
        assert_eq!(work.frame, Some(2));
    }

    #[test]
    fn control_wakes_idle_encoder_without_capture() {
        let mut workers = Workers::<u8>::new();
        let waiting = subscribe_wait(&workers.handoff);
        let (done, result) = mpsc::channel();
        workers
            .spawn("idle-control", true, move |handoff| {
                done.send(handoff.next().unwrap()).unwrap();
            })
            .unwrap();
        workers.handoff.activate();
        receive(&waiting); // exact wait point, with the predicate lock held
        workers.handoff.control(true, Some(123));
        let work = receive(&result);
        assert!(work.frame.is_none());
        assert_eq!(
            work.controls,
            Controls {
                force_keyframe: true,
                bitrate: Some(123)
            }
        );
        stop_bounded(workers);
    }

    #[test]
    fn stop_clears_raw_controls_and_rejects_future_work() {
        let handoff = Handoff::new();
        let frame = Arc::new(vec![7]);
        handoff.publish(Arc::clone(&frame));
        handoff.control(true, Some(9));
        handoff.stop();
        assert_eq!(Arc::strong_count(&frame), 1);
        assert!(handoff.cancellation().load(Ordering::Acquire));
        assert!(handoff.next().is_none());
        assert!(!handoff.publish(frame));
        assert!(!handoff.control(true, Some(10)));
    }

    #[test]
    fn stop_wakes_and_reaps_idle_encoder() {
        let mut workers = Workers::<u8>::new();
        let waiting = subscribe_wait(&workers.handoff);
        let (done, result) = mpsc::channel();
        workers
            .spawn("idle-stop", true, move |handoff| {
                assert!(handoff.next().is_none());
                done.send(()).unwrap();
            })
            .unwrap();
        workers.handoff.activate();
        receive(&waiting);
        stop_bounded(workers);
        receive(&result);
    }

    #[test]
    fn failed_startup_reaps_gated_workers_without_entering_native_code() {
        let (done, result) = mpsc::channel();
        let join = thread::spawn(move || {
            let (entered, observed) = mpsc::channel();
            let start = || -> io::Result<()> {
                let mut workers = Workers::<u8>::new();
                workers.spawn("capture-before-spawn-failure", true, move |_| {
                    entered.send(()).unwrap();
                })?;
                // The same early return as `workers.spawn(...)?` on spawn error.
                Err(io::Error::other("scripted second spawn failure"))
            };
            assert!(start().is_err());
            assert!(matches!(
                observed.try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            ));
            done.send(()).unwrap();
        });
        receive(&result);
        join.join().unwrap();
    }

    #[test]
    fn critical_initialization_error_wakes_sibling() {
        let mut workers = Workers::<u8>::new();
        let waiting = subscribe_wait(&workers.handoff);
        let (release, init) = mpsc::channel();
        workers
            .spawn("encode-init-error", true, move |_| {
                receive(&init);
            })
            .unwrap();
        let (done, exited) = mpsc::channel();
        workers
            .spawn("capture-sibling", true, move |handoff| {
                assert!(handoff.next().is_none());
                done.send(()).unwrap();
            })
            .unwrap();
        workers.handoff.activate();
        receive(&waiting);
        release.send(()).unwrap();
        receive(&exited);
        stop_bounded(workers);
    }

    #[test]
    fn stop_after_consumer_drop_reaps_capture_encode_and_audio() {
        let mut workers = Workers::<u8>::new();
        let (output, consumer) = mpsc::sync_channel(1);
        let (entered, blocked) = mpsc::channel();
        for name in ["capture", "audio"] {
            workers
                .spawn(name, false, |handoff| {
                    // A cadence wait is the production cancellable wait primitive.
                    handoff.pace_until(Instant::now() + Duration::from_secs(60));
                    assert!(handoff.is_stopped());
                })
                .unwrap();
        }
        workers
            .spawn("encode", true, move |_| {
                output.send(1).unwrap();
                entered.send(()).unwrap();
                assert!(output.send(2).is_err());
            })
            .unwrap();
        workers.handoff.activate();
        receive(&blocked);
        drop(consumer);
        stop_bounded(workers);
    }

    #[test]
    fn optional_audio_exit_does_not_stop_video() {
        let mut workers = Workers::<u8>::new();
        let (done, exited) = mpsc::channel();
        workers
            .spawn("audio-eof", false, move |_| {
                done.send(()).unwrap();
            })
            .unwrap();
        workers.handoff.activate();
        receive(&exited);
        workers.joins.pop().unwrap().join().unwrap();
        assert!(!workers.handoff.is_stopped());
        stop_bounded(workers);
    }

    struct ScriptedEncoder {
        entered: mpsc::Sender<u8>,
        release: Receiver<()>,
        forced: bool,
        bitrate: u32,
    }

    impl Encoder<u8> for ScriptedEncoder {
        type Output = (u8, bool, u32);
        type Error = &'static str;
        fn force_keyframe(&mut self) {
            self.forced = true;
        }
        fn bitrate(&mut self, bitrate: u32) -> Result<Vec<Self::Output>, Self::Error> {
            self.bitrate = bitrate;
            Ok(vec![(99, false, bitrate)]) // a delayed packet drained by reopen
        }
        fn encode(&mut self, frame: u8) -> Result<Vec<Self::Output>, Self::Error> {
            self.entered.send(frame).unwrap();
            receive(&self.release);
            let forced = std::mem::take(&mut self.forced);
            Ok(vec![
                (frame, forced, self.bitrate),
                (frame, false, self.bitrate),
            ])
        }
    }

    #[test]
    fn production_encoder_loop_keeps_outputs_ordered_and_controls_latest() {
        let mut workers = Workers::new();
        let (entered, encoding) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (output, packets) = mpsc::sync_channel(1);
        workers.handoff.publish(1);
        workers
            .spawn("scripted-encode", true, move |handoff| {
                run_encoder(
                    &handoff,
                    || {
                        Ok(ScriptedEncoder {
                            entered,
                            release: released,
                            forced: false,
                            bitrate: 8,
                        })
                    },
                    |packet| output.send(packet).is_ok(),
                )
                .unwrap();
            })
            .unwrap();
        workers.handoff.activate();
        assert_eq!(receive(&encoding), 1);
        workers.handoff.publish(2);
        workers.handoff.control(true, Some(4));
        workers.handoff.publish(3);
        workers.handoff.control(false, Some(2));
        workers.handoff.publish(4);
        release.send(()).unwrap();
        assert_eq!(receive(&packets), (1, false, 8));
        assert_eq!(receive(&packets), (1, false, 8));
        assert_eq!(receive(&packets), (99, false, 2));
        assert_eq!(receive(&encoding), 4);
        release.send(()).unwrap();
        assert_eq!(receive(&packets), (4, true, 2));
        assert_eq!(receive(&packets), (4, false, 2));
        drop(packets);
        stop_bounded(workers);
    }

    #[test]
    fn audio_notification_reads_newest_whole_stereo_block() {
        let slot = LatestAudio::default();
        let now = Instant::now();
        assert!(slot.publish(AudioBlock {
            pcm: vec![1; 16],
            captured_at: now
        }));
        assert!(!slot.publish(AudioBlock {
            pcm: vec![2; 24],
            captured_at: now
        }));
        assert_eq!(slot.take_fresh(now).unwrap().pcm, vec![2; 24]);
        assert!(slot.publish(AudioBlock {
            pcm: vec![3; 8],
            captured_at: now
        }));
    }

    #[test]
    fn stale_audio_is_dropped_without_replaying_older_blocks() {
        let slot = LatestAudio::default();
        let now = Instant::now();
        slot.publish(AudioBlock {
            pcm: vec![1; 16],
            captured_at: now,
        });
        assert!(slot
            .take_fresh(now + LatestAudio::MAX_AGE + Duration::from_nanos(1))
            .is_none());
        assert!(slot.take_fresh(now).is_none());
    }

    #[test]
    fn full_compressed_queue_does_not_lose_future_audio_notifications() {
        let slot = LatestAudio::default();
        let now = Instant::now();
        assert!(slot.publish(AudioBlock {
            pcm: vec![1; 8],
            captured_at: now
        }));
        slot.notification_rejected();
        assert!(slot.publish(AudioBlock {
            pcm: vec![2; 8],
            captured_at: now
        }));
        assert_eq!(slot.take_fresh(now).unwrap().pcm, vec![2; 8]);
    }

    #[test]
    fn delayed_output_keeps_its_accepted_input_timestamps() {
        let mut times = FrameTimes::new();
        let first = Instant::now();
        let second = first + Duration::from_secs(1);
        times.submitted(first, first + Duration::from_millis(1));
        times.submitted(second, second + Duration::from_millis(1));
        assert_eq!(
            times.take(0),
            Some((first, first + Duration::from_millis(1)))
        );
        assert_eq!(
            times.take(1),
            Some((second, second + Duration::from_millis(1)))
        );
        assert!(times.take(1).is_none());
    }

    #[test]
    fn keepalive_arc_clone_shares_the_full_nv12_allocation() {
        let pixels = Arc::new(vec![128; 1920 * 1080 * 3 / 2]);
        let keepalive = Arc::clone(&pixels);
        assert!(Arc::ptr_eq(&pixels, &keepalive));
        assert_eq!(pixels.as_ptr(), keepalive.as_ptr());
    }
}
