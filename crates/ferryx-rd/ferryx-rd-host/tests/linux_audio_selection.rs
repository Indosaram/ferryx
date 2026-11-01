#![cfg(target_os = "linux")]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::{atomic::AtomicBool, mpsc},
    time::Duration,
};

use ferryx_rd_host::audio_linux::{AudioBackend, AudioError, LinuxAudioCapture};

// Each case re-execs itself so PATH and the monitor override are process-local.
// The recorder speaks the actual argv/stdout seam, not a command-builder clone.
fn selection_case(case: &str) {
    if env::var("MAHO_AUDIO_SELECTION_CHILD").as_deref() == Ok(case) {
        let result = LinuxAudioCapture::open_cancellable(&AtomicBool::new(false));
        if matches!(case, "pulse-empty" | "pulse-failed") {
            assert!(matches!(result, Err(AudioError::Command(_))));
            return;
        }
        let mut capture = result.unwrap();
        let expected = if case.starts_with("pulse") {
            AudioBackend::PulseAudio
        } else {
            AudioBackend::PipeWire
        };
        assert_eq!(capture.backend(), expected);
        let mut samples = [0.0; 2];
        assert_eq!(capture.read_interleaved_f32(&mut samples).unwrap(), 2);
        assert_eq!(samples, [1.0, -0.5]);
        assert!(matches!(
            capture.read_interleaved_f32(&mut samples),
            Err(AudioError::Eof)
        ));
        return;
    }

    // Given: isolated recorder tools and distinct default/override targets.
    let directory = tempfile::tempdir().unwrap();
    let args_file = directory.path().join("recorder-args");
    let tool = if case.starts_with("pulse") {
        "parec"
    } else {
        "pw-record"
    };
    let executable = directory.path().join(tool);
    let pcm: String = [1.0_f32, -0.5]
        .iter()
        .flat_map(|sample| sample.to_ne_bytes())
        .map(|byte| format!("\\{byte:03o}"))
        .collect();
    fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >\"$MAHO_AUDIO_ARGS\"\nprintf '{pcm}'\n"),
    )
    .unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    let pactl = directory.path().join("pactl");
    let reply = match case {
        "pulse-empty" => "printf '\\n'",
        "pulse-failed" | "pulse-override" => "exit 7",
        _ => "printf 'desktop-sink\\n'",
    };
    fs::write(
        &pactl,
        format!("#!/bin/sh\n[ \"$1\" = get-default-sink ] || exit 9\n{reply}\n"),
    )
    .unwrap();
    fs::set_permissions(pactl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(env::current_exe().unwrap());
    child
        .args([
            "--exact",
            &format!("audio_selection_{}", case.replace('-', "_")),
            "--nocapture",
        ])
        .env("MAHO_AUDIO_SELECTION_CHILD", case)
        .env("MAHO_AUDIO_ARGS", &args_file)
        .env("PATH", directory.path())
        .env_remove("MAHO_AUDIO_MONITOR");
    if case.ends_with("override") {
        child.env("MAHO_AUDIO_MONITOR", "chosen-output.monitor");
    }

    // When: the public acquisition entry point selects and launches a recorder.
    let (done_tx, done_rx) = mpsc::channel();
    let mut child = child.spawn().unwrap();
    let pid = child.id();
    let worker = std::thread::spawn(move || done_tx.send(child.wait().unwrap()).unwrap());
    // Child completion is the signal; timeout only bounds a broken regression.
    let status = done_rx.recv_timeout(Duration::from_secs(10));
    if status.is_err() {
        let killed = Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap();
        assert!(
            killed.success(),
            "timed-out acquisition child must be killed"
        );
    }
    worker.join().unwrap();
    let status = status.expect("acquisition child exceeded deadline");
    assert!(status.success(), "acquisition child exited {status}");

    // Then: default selection cannot become the default microphone.
    if matches!(case, "pulse-empty" | "pulse-failed") {
        assert!(
            !args_file.exists(),
            "invalid sink must not launch a recorder"
        );
        return;
    }
    let args = fs::read_to_string(args_file).unwrap();
    let args: Vec<_> = args.lines().collect();
    if tool == "pw-record" {
        assert!(args.contains(&"--raw"));
        for option in [
            ["--rate", "48000"],
            ["--channels", "2"],
            ["--format", "f32"],
        ] {
            assert!(args.windows(2).any(|pair| pair == option));
        }
        assert_eq!(args.last(), Some(&"-"), "raw PCM must be written to stdout");
        if case == "pipewire-default" {
            let properties = args
                .windows(2)
                .find(|pair| matches!(pair[0], "--properties" | "-P"))
                .map(|pair| serde_json::from_str::<serde_json::Value>(pair[1]).unwrap());
            assert_eq!(
                properties
                    .as_ref()
                    .and_then(|p| p["stream.capture.sink"].as_bool()),
                Some(true),
                "default pw-record must capture the output sink, not an Audio/Source"
            );
        } else {
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--target", "chosen-output.monitor"]));
        }
    } else {
        let expected = if case == "pulse-override" {
            "--device=chosen-output.monitor"
        } else {
            "--device=desktop-sink.monitor"
        };
        for option in [
            "--raw",
            "--format=float32ne",
            "--rate=48000",
            "--channels=2",
        ] {
            assert!(args.contains(&option));
        }
        assert!(
            args.contains(&expected),
            "Pulse capture must name the output monitor: {args:?}"
        );
    }
}

#[test]
fn audio_selection_pipewire_default() {
    selection_case("pipewire-default");
}

#[test]
fn audio_selection_pipewire_override() {
    selection_case("pipewire-override");
}

#[test]
fn audio_selection_pulse_default() {
    selection_case("pulse-default");
}

#[test]
fn audio_selection_pulse_override() {
    selection_case("pulse-override");
}

#[test]
fn audio_selection_pulse_empty() {
    selection_case("pulse-empty");
}

#[test]
fn audio_selection_pulse_failed() {
    selection_case("pulse-failed");
}
