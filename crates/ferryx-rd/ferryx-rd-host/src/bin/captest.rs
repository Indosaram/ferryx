//! Standalone capture diagnostics: connect to the compositor and take N
//! screenshots in a row, printing timing for each. Run with WAYLAND_DISPLAY
//! and XDG_RUNTIME_DIR set to the active session.
//!
//! Usage: captest [count] [interval_ms]
//! (Linux only — uses the wlroots/Hyprland screencopy backend.)

fn main() {
    #[cfg(target_os = "linux")]
    run();
    #[cfg(not(target_os = "linux"))]
    eprintln!("captest is Linux-only");
}

#[cfg(target_os = "linux")]
fn run() {
    use ferryx_rd_host::capture_linux::{CaptureConfig, LinuxCapture};
    use std::time::{Duration, Instant};

    let args: Vec<String> = std::env::args().collect();
    let count: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(5);
    let interval_ms: u64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(300);
    let output_name: Option<String> = args.get(3).cloned();

    println!("captest: {count} captures, {interval_ms}ms interval, output {output_name:?}");
    let config = CaptureConfig {
        output_name,
        ..CaptureConfig::default()
    };
    println!("config: {config:?}");
    let started = Instant::now();
    let mut capture = match LinuxCapture::connect(config) {
        Ok(capture) => capture,
        Err(error) => {
            eprintln!("connect failed: {error}");
            std::process::exit(1);
        }
    };
    println!(
        "connected in {:?}, output: {:?}",
        started.elapsed(),
        capture.output_info()
    );

    for index in 0..count {
        let begin = Instant::now();
        match capture.capture_frame() {
            Ok(frame) => {
                println!(
                    "[{index}] ok in {:?}: {}x{} stride {} bgra {} bytes damage {:?}",
                    begin.elapsed(),
                    frame.width,
                    frame.height,
                    frame.stride,
                    frame.bgra.len(),
                    frame.damage,
                );
            }
            Err(error) => {
                println!("[{index}] FAILED in {:?}: {error}", begin.elapsed());
            }
        }
        if index + 1 < count {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    println!("done in {:?}", started.elapsed());
}
