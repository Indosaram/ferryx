//! PipeWire/PulseAudio system-monitor capture for Linux.
//!
//! `pw-record` is preferred on PipeWire. `parec` is the compatibility fallback
//! and works with PipeWire's PulseAudio server as well as native PulseAudio.
//! Both commands are configured for 48 kHz, stereo, interleaved native-endian
//! `f32`, matching the v3 media contract exactly.
//!
//! A specific PipeWire monitor node can be supplied through
//! `MAHO_AUDIO_MONITOR`. Otherwise PipeWire captures its default output sink
//! via `stream.capture.sink`; for PulseAudio, the backend resolves the default sink and appends
//! `.monitor`. On Arch/Omarchy install `pipewire-audio` (and normally
//! `pipewire-pulse`). Runtime QA must confirm that the selected node is the
//! system-output monitor rather than a microphone.

use std::{
    env,
    io::{self, Read},
    process::{Child, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use ferryx_rd_proto::{AudioFragment, AudioFragmentHeader, MAX_AUDIO_FRAGMENT_BYTES};
use thiserror::Error;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const BYTES_PER_SAMPLE: usize = size_of::<f32>();
/// How long a recorder may take to deliver its first PCM before it is judged.
const STARTUP_READY_TIMEOUT: Duration = Duration::from_millis(1500);

enum Readiness {
    Data,
    Quiet,
}

pub const BYTES_PER_SAMPLE_FRAME: usize = CHANNELS as usize * BYTES_PER_SAMPLE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    PipeWire,
    PulseAudio,
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("audio capture process reached EOF")]
    Eof,
    #[error("audio capture cancelled")]
    Cancelled,
    #[error("install PipeWire (`pw-record`) or PulseAudio (`parec`) capture tools")]
    Unavailable,
    #[error("audio capture command failed: {0}")]
    Command(String),
    #[error("audio I/O failed: {0}")]
    Io(#[from] io::Error),
}

pub struct LinuxAudioCapture {
    backend: AudioBackend,
    child: Child,
    stdout: ChildStdout,
    pending: Vec<u8>,
    read_buffer: Vec<u8>,
    #[cfg(test)]
    before_poll: Option<std::sync::mpsc::Sender<()>>,
}

impl LinuxAudioCapture {
    pub fn start() -> Result<Self, AudioError> {
        Self::open()
    }

    pub fn open() -> Result<Self, AudioError> {
        Self::open_cancellable(&AtomicBool::new(false))
    }

    pub fn open_cancellable(stop: &AtomicBool) -> Result<Self, AudioError> {
        if stop.load(Ordering::Acquire) {
            return Err(AudioError::Cancelled);
        }
        // Try PipeWire first, then PulseAudio. A recorder only counts as
        // started once it delivers PCM; one that exits, hits EOF, or fails
        // before data falls through to the next backend instead of leaving the
        // session silently without audio. A recorder that stays alive but
        // quiet is kept as a last resort so an idle graph is not a failure.
        let mut quiet: Option<Self> = None;
        let mut last_error: Option<AudioError> = None;
        for backend in [AudioBackend::PipeWire, AudioBackend::PulseAudio] {
            let command = match backend {
                AudioBackend::PipeWire => pipewire_command(),
                AudioBackend::PulseAudio => pulse_command(stop),
            };
            let command = match command {
                Ok(Some(command)) => command,
                Ok(None) => continue,
                Err(AudioError::Cancelled) => return Err(AudioError::Cancelled),
                Err(error) => {
                    tracing::warn!(?backend, %error, "audio backend setup failed");
                    last_error = Some(error);
                    continue;
                }
            };
            let mut capture = match Self::spawn_cancellable(backend, command, stop) {
                Ok(capture) => capture,
                Err(AudioError::Cancelled) => return Err(AudioError::Cancelled),
                Err(error) => {
                    tracing::warn!(?backend, %error, "audio recorder failed to start");
                    last_error = Some(error);
                    continue;
                }
            };
            match capture.await_ready(stop, STARTUP_READY_TIMEOUT) {
                Ok(Readiness::Data) => return Ok(capture),
                Ok(Readiness::Quiet) => {
                    tracing::warn!(?backend, "audio recorder produced no data during startup");
                    quiet.get_or_insert(capture);
                }
                Err(AudioError::Cancelled) => return Err(AudioError::Cancelled),
                Err(error) => {
                    tracing::warn!(?backend, %error, "audio recorder failed during startup");
                    last_error = Some(error);
                }
            }
        }
        match quiet {
            Some(capture) => Ok(capture),
            None => Err(last_error.unwrap_or(AudioError::Unavailable)),
        }
    }

    /// Waits until the recorder delivers its first bytes. Early exit, EOF, or
    /// an I/O error is a failure (the child is reaped); running out of time
    /// with a live child reports `Quiet`.
    fn await_ready(
        &mut self,
        stop: &AtomicBool,
        timeout: Duration,
    ) -> Result<Readiness, AudioError> {
        use rustix::event::{poll, PollFd, PollFlags, Timespec};
        let deadline = Instant::now() + timeout;
        let result = (|| loop {
            if stop.load(Ordering::Acquire) {
                return Err(AudioError::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return match self.child.try_wait()? {
                    Some(status) => Err(AudioError::Command(format!(
                        "audio recorder exited with {status}"
                    ))),
                    None => Ok(Readiness::Quiet),
                };
            }
            let slice = Timespec::try_from(remaining.min(Duration::from_millis(50)))
                .expect("bounded duration");
            let mut fds = [PollFd::new(&self.stdout, PollFlags::IN)];
            match poll(&mut fds, Some(&slice)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io::Error::from(error).into()),
            }
            if fds[0].revents().is_empty() {
                continue;
            }
            let mut chunk = [0_u8; 4096];
            let count = self.stdout.read(&mut chunk)?;
            if count == 0 {
                return Err(match self.child.try_wait()? {
                    Some(status) => {
                        AudioError::Command(format!("audio recorder exited with {status}"))
                    }
                    None => AudioError::Eof,
                });
            }
            self.pending.extend_from_slice(&chunk[..count]);
            return Ok(Readiness::Data);
        })();
        if result.is_err() {
            self.shutdown()?;
        }
        result
    }

    fn spawn(backend: AudioBackend, mut command: Command) -> Result<Self, AudioError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AudioError::Command("capture stdout was unavailable".into()))?;
        Ok(Self {
            backend,
            child,
            stdout,
            pending: Vec::with_capacity(BYTES_PER_SAMPLE_FRAME * 2),
            read_buffer: Vec::new(),
            #[cfg(test)]
            before_poll: None,
        })
    }

    pub fn backend(&self) -> AudioBackend {
        self.backend
    }

    fn spawn_cancellable(
        backend: AudioBackend,
        command: Command,
        stop: &AtomicBool,
    ) -> Result<Self, AudioError> {
        if stop.load(Ordering::Acquire) {
            return Err(AudioError::Cancelled);
        }
        let mut capture = Self::spawn(backend, command)?;
        if stop.load(Ordering::Acquire) {
            capture.shutdown()?;
            return Err(AudioError::Cancelled);
        }
        Ok(capture)
    }

    fn query_source(
        &mut self,
        stop: &AtomicBool,
        deadline: Instant,
    ) -> Result<Option<String>, AudioError> {
        use rustix::event::{poll, PollFd, PollFlags, Timespec};
        let result = (|| {
            let mut bytes = Vec::new();
            let mut eof = false;
            loop {
                if stop.load(Ordering::Acquire) {
                    return Err(AudioError::Cancelled);
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "pactl startup deadline expired",
                    )
                    .into());
                }
                let status = self.child.try_wait()?;
                if eof {
                    if let Some(status) = status {
                        if !status.success() {
                            return Err(AudioError::Command(format!("pactl exited with {status}")));
                        }
                        let source = String::from_utf8(bytes)
                            .map_err(|error| AudioError::Command(error.to_string()))?;
                        let source = source.trim();
                        if source.is_empty() {
                            return Err(AudioError::Command(
                                "pactl returned no default sink".into(),
                            ));
                        }
                        return Ok(Some(format!("{source}.monitor")));
                    }
                }
                let duration = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50));
                let timeout = Timespec::try_from(duration).expect("bounded duration");
                let mut fds = [PollFd::new(&self.stdout, PollFlags::IN)];
                #[cfg(test)]
                if let Some(ready) = self.before_poll.take() {
                    ready.send(()).expect("test receiver alive");
                }
                // EOF may precede child exit. An empty poll avoids spinning on HUP.
                let ready = match poll(if eof { &mut [] } else { &mut fds }, Some(&timeout)) {
                    Ok(count) => count > 0,
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(io::Error::from(error).into()),
                };
                if ready {
                    let mut chunk = [0; 512];
                    let count = self.stdout.read(&mut chunk)?;
                    eof = count == 0;
                    if bytes.len() + count > 4096 {
                        return Err(AudioError::Command(
                            "pactl output exceeds 4096 bytes".into(),
                        ));
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
            }
        })();
        self.shutdown()?;
        result
    }

    /// Reads complete stereo sample frames into `output`. Short pipe reads are
    /// retained so a partial f32/stereo frame never shifts channel alignment.
    pub fn read_interleaved_f32(&mut self, output: &mut [f32]) -> Result<usize, AudioError> {
        let stop = AtomicBool::new(false);
        loop {
            let count =
                self.read_interleaved_f32_cancellable(output, &stop, Duration::from_millis(50))?;
            if count > 0 || output.len() < CHANNELS as usize {
                return Ok(count);
            }
        }
    }

    /// Zero means no complete frame in this readiness slice. EOF and cancellation
    /// are terminal errors; the calling worker owns this object without a mutex.
    pub fn read_interleaved_f32_cancellable(
        &mut self,
        output: &mut [f32],
        stop: &AtomicBool,
        poll_timeout: Duration,
    ) -> Result<usize, AudioError> {
        if stop.load(Ordering::Acquire) {
            self.shutdown()?;
            return Err(AudioError::Cancelled);
        }
        let sample_capacity = output.len() - (output.len() % CHANNELS as usize);
        if sample_capacity == 0 {
            return Ok(0);
        }
        let byte_capacity = sample_capacity * BYTES_PER_SAMPLE;
        if self.pending.len() < BYTES_PER_SAMPLE_FRAME {
            use rustix::event::{poll, PollFd, PollFlags, Timespec};
            let timeout = Timespec::try_from(poll_timeout.min(Duration::from_millis(50)))
                .expect("bounded poll duration");
            let mut fds = [PollFd::new(&self.stdout, PollFlags::IN)];
            #[cfg(test)]
            if let Some(ready) = self.before_poll.take() {
                ready.send(()).expect("test receiver alive");
            }
            match poll(&mut fds, Some(&timeout)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => return Ok(0),
                Err(error) => return Err(io::Error::from(error).into()),
            }
            let ready = !fds[0].revents().is_empty();
            if stop.load(Ordering::Acquire) {
                self.shutdown()?;
                return Err(AudioError::Cancelled);
            }
            if !ready {
                return Ok(0);
            }
            self.read_buffer
                .resize(byte_capacity.max(BYTES_PER_SAMPLE_FRAME), 0);
            let read = self.stdout.read(&mut self.read_buffer)?;
            if read == 0 {
                self.shutdown()?;
                return Err(AudioError::Eof);
            }
            self.pending.extend_from_slice(&self.read_buffer[..read]);
        }

        let complete_bytes = self.pending.len().min(byte_capacity);
        let complete_bytes = complete_bytes - (complete_bytes % BYTES_PER_SAMPLE_FRAME);
        for (sample, bytes) in output[..complete_bytes / BYTES_PER_SAMPLE]
            .iter_mut()
            .zip(self.pending[..complete_bytes].chunks_exact(BYTES_PER_SAMPLE))
        {
            *sample = f32::from_ne_bytes(bytes.try_into().expect("one native f32"));
        }
        self.pending.drain(..complete_bytes);
        Ok(complete_bytes / BYTES_PER_SAMPLE)
    }

    /// Bound teardown even if a recorder is stuck in an uninterruptible syscall.
    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_some() {
            return Ok(());
        }
        if let Err(error) = self.child.kill() {
            if self.child.try_wait()?.is_none() {
                return Err(error);
            }
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "audio child did not exit after kill",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for LinuxAudioCapture {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::warn!(%error, "failed to reap Linux audio recorder");
        }
    }
}

/// Splits one PCM block into the exact v3 UDP fragmentation contract.
pub fn fragment_audio(frame_id: u32, pcm: &[u8]) -> Vec<AudioFragment> {
    if pcm.is_empty() {
        return Vec::new();
    }
    let count = pcm.len().div_ceil(MAX_AUDIO_FRAGMENT_BYTES);
    if count > u16::MAX as usize {
        return Vec::new();
    }
    pcm.chunks(MAX_AUDIO_FRAGMENT_BYTES)
        .enumerate()
        .map(|(index, data)| AudioFragment {
            header: AudioFragmentHeader {
                frame_id,
                fragment_index: index as u16,
                fragment_count: count as u16,
            },
            data: data.to_vec(),
        })
        .collect()
}

fn pipewire_command() -> Result<Option<Command>, AudioError> {
    if !command_available("pw-record") {
        return Ok(None);
    }
    let mut command = Command::new("pw-record");
    command.args([
        "--raw",
        "--rate",
        "48000",
        "--channels",
        "2",
        "--format",
        "f32",
    ]);
    if let Some(target) = env::var_os("MAHO_AUDIO_MONITOR") {
        command.arg("--target").arg(target);
    } else {
        command.args(["--properties", r#"{"stream.capture.sink":true}"#]);
    }
    command.arg("-");
    Ok(Some(command))
}

fn pulse_command(stop: &AtomicBool) -> Result<Option<Command>, AudioError> {
    if !command_available("parec") {
        return Ok(None);
    }
    let source = match env::var("MAHO_AUDIO_MONITOR").ok() {
        Some(source) => Some(source),
        None => default_pulse_monitor_source(stop)?,
    };
    let mut command = Command::new("parec");
    command.args([
        "--raw",
        "--format=float32ne",
        "--rate=48000",
        "--channels=2",
    ]);
    if let Some(source) = source {
        command.arg(format!("--device={source}"));
    }
    Ok(Some(command))
}

fn command_available(name: &str) -> bool {
    // Split into owned directories first: `split_paths` borrows its argument,
    // so the borrowed iterator cannot outlive the owned `PATH` value.
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    command_available_in(name, env::split_paths(&paths).collect::<Vec<_>>())
}

fn command_available_in<I>(name: &str, directories: I) -> bool
where
    I: IntoIterator,
    I::Item: AsRef<std::path::Path>,
{
    use std::os::unix::fs::PermissionsExt;

    directories
        .into_iter()
        .map(|directory| directory.as_ref().join(name))
        .any(|candidate| {
            candidate.is_file()
                && candidate
                    .metadata()
                    .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
}

fn default_pulse_monitor_source(stop: &AtomicBool) -> Result<Option<String>, AudioError> {
    let mut command = Command::new("pactl");
    command.arg("get-default-sink");
    let mut query = LinuxAudioCapture::spawn_cancellable(AudioBackend::PulseAudio, command, stop)?;
    query.query_source(stop, Instant::now() + Duration::from_secs(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh_capture(script: &str) -> LinuxAudioCapture {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        LinuxAudioCapture::spawn(AudioBackend::PipeWire, command).unwrap()
    }

    #[test]
    fn startup_readiness_distinguishes_data_failure_and_quiet() {
        let stop = AtomicBool::new(false);
        let timeout = Duration::from_secs(5);

        let mut data = sh_capture("printf 'abcd'; sleep 30");
        assert!(matches!(
            data.await_ready(&stop, timeout),
            Ok(Readiness::Data)
        ));
        assert_eq!(data.pending, b"abcd");
        data.shutdown().unwrap();

        let mut failed = sh_capture("exit 3");
        // EOF may be observed before the exit status is reapable.
        assert!(matches!(
            failed.await_ready(&stop, timeout),
            Err(AudioError::Command(_) | AudioError::Eof)
        ));
        assert!(failed.child.try_wait().unwrap().is_some());

        let mut quiet = sh_capture("sleep 30");
        assert!(matches!(
            quiet.await_ready(&stop, Duration::from_millis(100)),
            Ok(Readiness::Quiet)
        ));
        quiet.shutdown().unwrap();

        let cancelled = AtomicBool::new(true);
        let mut pending = sh_capture("sleep 30");
        assert!(matches!(
            pending.await_ready(&cancelled, timeout),
            Err(AudioError::Cancelled)
        ));
        assert!(pending.child.try_wait().unwrap().is_some());
    }

    #[test]
    fn audio_eof_is_terminal() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        let mut capture = LinuxAudioCapture::spawn(AudioBackend::PipeWire, command).unwrap();
        assert!(
            capture.read_interleaved_f32(&mut [0.0; 2]).is_err(),
            "EOF must be terminal, not an empty successful read"
        );
    }

    #[test]
    fn fake_pactl_cancel_is_gated_and_reaped() {
        use std::sync::{mpsc, Arc};
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let gate = child.stdin.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let mut query = LinuxAudioCapture {
            backend: AudioBackend::PulseAudio,
            stdout: child.stdout.take().unwrap(),
            child,
            pending: Vec::new(),
            read_buffer: Vec::new(),
            before_poll: Some(ready_tx),
        };
        let worker = std::thread::spawn(move || {
            let result = query.query_source(&worker_stop, Instant::now() + Duration::from_secs(10));
            done_tx
                .send((
                    matches!(result, Err(AudioError::Cancelled)),
                    query.child.try_wait().unwrap().is_some(),
                ))
                .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stop.store(true, Ordering::Release);
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        drop(gate);
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok((true, true)),
            "pactl query must cancel and reap while stdout remains open"
        );
    }

    #[test]
    fn command_available_requires_executable_permission() {
        use std::fs;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("parec");
        fs::write(&executable, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let blocked = directory.path().join("pactl");
        fs::write(&blocked, b"#!/bin/sh\n").unwrap();

        assert!(command_available_in("parec", [directory.path()]));
        assert!(!command_available_in("pactl", [directory.path()]));
        assert!(!command_available_in(
            "parec",
            [directory.path().join("empty")]
        ));
    }

    #[test]
    fn pending_audio_read_cancels_and_reaps_child() {
        use std::sync::{mpsc, Arc};
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // Keep stdin open: cat cannot produce bytes or EOF before cancellation.
        let input = child.stdin.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let mut capture = LinuxAudioCapture {
            backend: AudioBackend::PipeWire,
            stdout: child.stdout.take().unwrap(),
            child,
            pending: Vec::new(),
            read_buffer: Vec::new(),
            before_poll: Some(ready_tx),
        };
        let worker = std::thread::spawn(move || {
            let result = loop {
                match capture.read_interleaved_f32_cancellable(
                    &mut [0.0; 2],
                    &worker_stop,
                    Duration::from_secs(60),
                ) {
                    Ok(0) => continue,
                    result => break result,
                }
            };
            let reaped = capture.child.try_wait().unwrap().is_some();
            done_tx
                .send((matches!(result, Err(AudioError::Cancelled)), reaped))
                .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stop.store(true, Ordering::Release);
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        drop(input);
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok((true, true)),
            "pending read must cancel and reap without stdout EOF"
        );
    }

    #[test]
    fn audio_read_buffer_is_reused_and_pcm_is_stereo_aligned() {
        use std::io::Write;
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut capture = LinuxAudioCapture {
            backend: AudioBackend::PipeWire,
            stdout: child.stdout.take().unwrap(),
            child,
            pending: Vec::new(),
            read_buffer: Vec::new(),
            before_poll: None,
        };
        let samples = [1.0_f32, -0.5];
        let bytes: Vec<u8> = samples.iter().flat_map(|x| x.to_ne_bytes()).collect();
        let mut buffer_address = None;
        for _ in 0..3 {
            input.write_all(&bytes).unwrap();
            let mut output = [0.0; 3];
            assert_eq!(capture.read_interleaved_f32(&mut output).unwrap(), 2);
            assert_eq!(&output[..2], &samples);
            assert_eq!(output[2], 0.0);
            if let Some(address) = buffer_address {
                assert_eq!(capture.read_buffer.as_ptr(), address);
            }
            buffer_address = Some(capture.read_buffer.as_ptr());
        }
        drop(input);
        assert!(matches!(
            capture.read_interleaved_f32(&mut [0.0; 2]),
            Err(AudioError::Eof)
        ));
        assert!(capture.child.try_wait().unwrap().is_some());
    }

    #[test]
    fn pactl_query_bounds_output_and_deadline_and_parses_source() {
        let stop = AtomicBool::new(false);
        for (script, expected) in [
            ("printf 'sink-name\\n'", Some("sink-name.monitor")),
            ("printf '%05000d' 0", None),
        ] {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            let mut query = LinuxAudioCapture::spawn(AudioBackend::PulseAudio, command).unwrap();
            let result = query.query_source(&stop, Instant::now() + Duration::from_secs(2));
            match expected {
                Some(source) => assert_eq!(result.unwrap().as_deref(), Some(source)),
                None => assert!(matches!(result, Err(AudioError::Command(_)))),
            }
            assert!(query.child.try_wait().unwrap().is_some());
        }
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        let mut query = LinuxAudioCapture::spawn(AudioBackend::PulseAudio, command).unwrap();
        assert!(matches!(query.query_source(&stop, Instant::now()),
            Err(AudioError::Io(error)) if error.kind() == io::ErrorKind::TimedOut));
        assert!(query.child.try_wait().unwrap().is_some());
        assert!(matches!(
            LinuxAudioCapture::open_cancellable(&AtomicBool::new(true)),
            Err(AudioError::Cancelled)
        ));
    }

    #[test]
    fn fragments_audio_at_wire_limit() {
        let pcm = vec![7_u8; MAX_AUDIO_FRAGMENT_BYTES * 2 + 3];
        let fragments = fragment_audio(42, &pcm);
        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[0].header.frame_id, 42);
        assert_eq!(fragments[0].header.fragment_index, 0);
        assert_eq!(fragments[2].header.fragment_index, 2);
        assert_eq!(fragments[2].header.fragment_count, 3);
        assert_eq!(fragments[2].data, vec![7; 3]);
        assert_eq!(
            fragments
                .into_iter()
                .flat_map(|fragment| fragment.data)
                .collect::<Vec<_>>(),
            pcm
        );
    }

    #[test]
    fn empty_audio_does_not_create_invalid_zero_count_fragment() {
        assert!(fragment_audio(1, &[]).is_empty());
    }
}
