use ferryx_lib::native_terminal::composition::{CellMetrics, LogicalBounds};
use ferryx_lib::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState,
};
use std::sync::Arc;
use std::io::BufRead;
use tauri::Manager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .init();
    let runtime = tokio::runtime::Runtime::new()?;
    tauri::async_runtime::set(runtime.handle().clone());
    let _runtime_context = runtime.enter();
    let state = Arc::new(NativeTerminalSurfaceHostState::default());
    let setup_state = Arc::clone(&state);
    let app = tauri::Builder::default()
        .setup(move |app| {
            let window = tauri::WindowBuilder::new(app, "main")
                .title("Ferryx isolated native surface QA")
                .inner_size(800.0, 480.0)
                .decorations(false)
                .visible(true)
                .center()
                .build()?;
            window.show()?;
            window.set_focus()?;
            let request = NativeTerminalBoundsRequest {
                session_id: "isolated-native-qa".into(),
                bounds: LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 480.0,
                    scale_factor: window.scale_factor()?,
                },
            };
            setup_state.prepare_session_layout(
                request.clone(),
                CellMetrics { width_px: 10, height_px: 20 },
            )?;
            let (sender, messages) = tokio::sync::mpsc::channel(8);
            let stream_task = tokio::spawn(async move {
                let _sender = sender;
                std::future::pending::<()>().await;
            });
            setup_state.attach_daemon_attachment_with_bounds(
                "isolated-native-qa",
                ferryx_lib::daemon::DaemonAttachment {
                    session_id: "isolated-native-qa".into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"\x1b[?25l\x1b[32mFerryx GPU worker QA\x1b[0m\r\nNative surface: render / resize / retire\r\n0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n".to_vec(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task,
                },
                Some(app.handle().clone()),
                Some(request.bounds),
            )?;
            setup_state.render(&window, request)?;
            if let Some(snapshot) = setup_state.snapshot_for_session("isolated-native-qa")? {
                for (index, row) in snapshot.grid.iter().enumerate() {
                    let text: String = row.iter().map(|cell| cell.text.as_str()).collect();
                    if !text.trim().is_empty() {
                        println!("QA_ROW {index}: {text}");
                    }
                }
            }
            let control_window = window.clone();
            let control_state = Arc::clone(&setup_state);
            let control_app = app.handle().clone();
            std::thread::spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(command) = line else { break };
                    let window = control_window.clone();
                    let state = Arc::clone(&control_state);
                    let app = control_app.clone();
                    let _ = control_window.run_on_main_thread(move || {
                        match command.trim() {
                            "resize" => {
                                let _ = window.set_size(tauri::LogicalSize::new(640.0, 360.0));
                                println!("QA_RESIZE_REQUESTED");
                            }
                            "detach" => {
                                state.detach_session("isolated-native-qa");
                                println!("QA_DETACHED");
                            }
                            "render" => {
                                println!("QA_RENDER {:?}", state.render_current(&window, "isolated-native-qa"));
                            }
                            "quit" => {
                                state.detach_session("isolated-native-qa");
                                app.exit(0);
                            }
                            _ => eprintln!("QA_UNKNOWN_COMMAND"),
                        }
                    });
                }
            });
            println!("QA_WINDOW_READY pid={} visible={}", std::process::id(), window.is_visible()?);
            Ok(())
        })
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    app.run(move |app, event| {
        if let tauri::RunEvent::WindowEvent { label, event, .. } = event {
            match event {
                tauri::WindowEvent::Resized(size) => {
                    if let Some(window) = app.get_window(&label) {
                        let scale = window.scale_factor().unwrap_or(1.0);
                        let request = NativeTerminalBoundsRequest {
                            session_id: "isolated-native-qa".into(),
                            bounds: LogicalBounds {
                                x: 0.0,
                                y: 0.0,
                                width: size.width as f64 / scale,
                                height: size.height as f64 / scale,
                                scale_factor: scale,
                            },
                        };
                        if let Err(error) = state.render(&window, request) {
                            eprintln!("QA_RESIZE_ERROR {error}");
                        }
                        if let Ok(Some(snapshot)) = state.snapshot_for_session("isolated-native-qa") {
                            let lines: Vec<String> = snapshot.grid.iter().map(|row| {
                                row.iter().map(|cell| cell.text.as_str()).collect::<String>()
                            }).filter(|line| !line.trim().is_empty()).collect();
                            println!("QA_RESIZED {}x{} lines={lines:?}", snapshot.cols, snapshot.rows);
                        }
                    }
                }
                tauri::WindowEvent::CloseRequested { .. } => {
                    state.detach_session("isolated-native-qa");
                    println!("QA_DETACHED");
                }
                _ => {}
            }
        }
    });
    Ok(())
}
