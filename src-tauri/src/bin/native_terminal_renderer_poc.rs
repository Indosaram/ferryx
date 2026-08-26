//! Ferryx Native Terminal WGPU Renderer Standalone POC.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ferryx_lib::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, NativeTerminalRenderer,
    RenderSnapshot, RendererConfig, SelectionSnapshot,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

fn blank_cell() -> CellSnapshot {
    CellSnapshot {
        text: String::new(),
        wide: CellWide::Narrow,
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        underline: false,
        inverse: false,
    }
}

fn canonical_scenario() -> (RenderSnapshot, SelectionSnapshot) {
    let cols = 80u16;
    let rows = 24u16;
    let mut grid = vec![vec![blank_cell(); cols as usize]; rows as usize];
    let accent = ColorRgb {
        r: 255,
        g: 104,
        b: 48,
    };
    let selection_bg = ColorRgb {
        r: 36,
        g: 90,
        b: 180,
    };

    for (col, text) in "Hello Ferryx Native WGPU Terminal!".chars().enumerate() {
        let mut cell = blank_cell();
        cell.text = text.to_string();
        cell.fg = Some(accent);
        cell.bold = true;
        grid[0][col] = cell;
    }

    let cjk_cells = ["東", "", "京", "", " ", "🦀", "", " ", "e\u{0301}"];
    for (col, text) in cjk_cells.into_iter().enumerate() {
        let mut cell = blank_cell();
        cell.text = text.to_string();
        cell.wide = match col {
            0 | 2 | 5 => CellWide::Wide,
            1 | 3 | 6 => CellWide::SpacerTail,
            _ => CellWide::Narrow,
        };
        cell.italic = col == 8;
        grid[1][col] = cell;
    }

    let long_line =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!@#$%^&*()-_=+[]";
    for col in 0..cols as usize {
        let mut cell = blank_cell();
        cell.text = (long_line[col % long_line.len()] as char).to_string();
        grid[2][col] = cell;
    }

    for col in 0..6 {
        grid[0][col].bg = Some(selection_bg);
    }

    (
        RenderSnapshot {
            cols,
            rows,
            cursor: CursorSnapshot {
                x: 10,
                y: 0,
                visible: true,
                blinking: false,
                wide_tail: false,
                visual_style: CursorVisualStyle::Block,
            },
            grid,
        },
        SelectionSnapshot {
            start_col: 0,
            start_row: 0,
            end_col: 5,
            end_row: 0,
        },
    )
}

fn run_headless(output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let (mut snap, sel) = canonical_scenario();
    let mut rend =
        NativeTerminalRenderer::new(RendererConfig::default()).map_err(|e| format!("{e}"))?;
    println!(
        "=== Ferryx Native WGPU Terminal Renderer POC (Headless) ===\nAdapter: {} ({:?}, {:?})",
        rend.adapter_info().name,
        rend.adapter_info().backend,
        rend.adapter_info().device_type
    );

    let (mut last_frame, iters) = (None, 50);
    let mut timings = Vec::with_capacity(iters);
    for i in 0..iters {
        snap.cursor.x = (10 + (i % 70)) as u16;
        snap.grid[0][0].text = format!("{}", i % 10);
        let t0 = Instant::now();
        let frame = rend
            .render_snapshot(&snap, Some(&sel))
            .map_err(|e| format!("{e}"))?;
        timings.push(t0.elapsed().as_micros() as f64 / 1000.0);
        last_frame = Some(frame);
    }
    timings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let f = last_frame.ok_or("No frame rendered")?;
    let st = rend.glyph_atlas_stats();
    let out = output
        .unwrap_or_else(|| PathBuf::from("bench/terminal/evidence/native-wgpu-phase2-latest.png"));
    f.save_png(&out).map_err(|e| format!("{e}"))?;

    println!(
        "Dimensions: {}x{} px ({} rows)\nDirty Stats: {} rebuilt, {} reused",
        f.width_px, f.height_px, f.rendered_row_count, f.rebuilt_row_count, f.reused_row_count
    );
    println!(
        "Glyph Atlas: {} entries, {} / {} bytes",
        st.entry_count, st.allocated_bytes, st.max_capacity_bytes
    );
    println!(
        "Frame Latency (N={iters}): p50 = {:.3} ms, p95 = {:.3} ms\nOutput Artifact: {} ({} bytes)",
        timings[iters / 2],
        timings[(iters * 95) / 100],
        out.display(),
        std::fs::metadata(&out)?.len()
    );
    Ok(())
}

type WinSetup = (
    Arc<Window>,
    NativeTerminalRenderer,
    wgpu::Surface<'static>,
    wgpu::TextureFormat,
);

#[derive(Default)]
struct WindowApp {
    window: Option<Arc<Window>>,
    renderer: Option<NativeTerminalRenderer>,
    surface: Option<wgpu::Surface<'static>>,
    format: Option<wgpu::TextureFormat>,
}

fn setup_win(el: &ActiveEventLoop) -> Result<WinSetup, Box<dyn std::error::Error>> {
    let attrs = WindowAttributes::default()
        .with_title("Ferryx Native Terminal POC")
        .with_inner_size(LogicalSize::new(800.0, 480.0));
    let w = Arc::new(el.create_window(attrs)?);
    let rend = NativeTerminalRenderer::new(RendererConfig::default())?;
    let surf = rend.create_surface(w.clone())?;
    let fmt = rend.configure_surface(
        &surf,
        w.inner_size().width.max(1),
        w.inner_size().height.max(1),
    )?;
    w.request_redraw();
    Ok((w, rend, surf, fmt))
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            match setup_win(el) {
                Ok((w, r, s, f)) => {
                    self.window = Some(w);
                    self.renderer = Some(r);
                    self.surface = Some(s);
                    self.format = Some(f);
                }
                Err(e) => {
                    eprintln!("Window init error: {e}");
                    el.exit();
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => {
                if let (Some(surf), Some(rend), Some(w)) =
                    (&self.surface, &self.renderer, &self.window)
                {
                    match rend.configure_surface(surf, sz.width.max(1), sz.height.max(1)) {
                        Ok(fmt) => {
                            self.format = Some(fmt);
                            w.request_redraw();
                        }
                        Err(e) => {
                            eprintln!("Resize error: {e}");
                            el.exit();
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let (Some(surf), Some(rend), Some(fmt), Some(w)) =
                    (&self.surface, &mut self.renderer, self.format, &self.window)
                else {
                    return;
                };
                let output = match surf.get_current_texture() {
                    Ok(o) => o,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        let sz = w.inner_size();
                        match rend.configure_surface(surf, sz.width.max(1), sz.height.max(1)) {
                            Ok(f) => {
                                self.format = Some(f);
                                w.request_redraw();
                            }
                            Err(e) => {
                                eprintln!("Surface reconfigure error: {e}");
                                el.exit();
                            }
                        }
                        return;
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        eprintln!("Surface OutOfMemory");
                        el.exit();
                        return;
                    }
                    Err(e) => {
                        eprintln!("Surface error: {e}");
                        return;
                    }
                };
                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let (snap, sel) = canonical_scenario();
                let sz = output.texture.size();
                if let Err(e) =
                    rend.render_to_surface_view(&snap, Some(&sel), &view, sz.width, sz.height, fmt)
                {
                    eprintln!("Surface render error: {e}");
                    el.exit();
                    return;
                }
                output.present();
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut window_mode = false;
    let mut output = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "Ferryx Native Terminal WGPU Renderer POC\nOptions:\n  --headless\n  --output <PATH>\n  --window\n  --help"
                );
                return Ok(());
            }
            "--headless" => {
                // Headless mode is the default when --window is not specified.
            }
            "--window" => {
                window_mode = true;
            }
            "--output" => {
                let path = args.next().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Unknown option: missing path for --output",
                    )
                })?;
                output = Some(PathBuf::from(path));
            }
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
    }

    if window_mode {
        println!("Launching Ferryx Native Terminal Renderer Window POC...");
        let el = EventLoop::new()?;
        el.set_control_flow(ControlFlow::Wait);
        el.run_app(&mut WindowApp::default())?;
    } else {
        run_headless(output)?;
    }
    Ok(())
}
