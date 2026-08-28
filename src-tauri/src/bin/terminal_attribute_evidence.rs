//! Headless evidence binary that renders terminal text attributes through WGPU.
//!
//! Renders each text attribute (normal, bold, faint, italic, underline, strikethrough,
//! overline, inverse, invisible, blink) on a separate row to a PNG file for pixel inspection.

use std::env;
use std::fs;
use std::path::PathBuf;

use ferryx_lib::native_terminal::{
    CellSnapshot, CursorSnapshot, CursorVisualStyle, NativeTerminalRenderer, OffscreenFrame,
    RenderSnapshot, RendererConfig,
};

fn build_attribute_snapshot() -> RenderSnapshot {
    let cols = 60u16;
    let rows = 10u16;
    let mut grid = vec![vec![CellSnapshot::default(); cols as usize]; rows as usize];

    let row_specs: [(&str, fn(&mut CellSnapshot)); 10] = [
        ("row 0 (normal):        ", |_| {}),
        ("row 1 (bold):          ", |c| c.bold = true),
        ("row 2 (faint):         ", |c| c.faint = true),
        ("row 3 (italic):        ", |c| c.italic = true),
        ("row 4 (underline):     ", |c| c.underline = true),
        ("row 5 (strikethrough): ", |c| c.strikethrough = true),
        ("row 6 (overline):      ", |c| c.overline = true),
        ("row 7 (inverse):       ", |c| c.inverse = true),
        ("row 8 (invisible):     ", |c| c.invisible = true),
        ("row 9 (blink):         ", |c| c.blink = true),
    ];

    let sample_text = "SAMPLE";

    for (row_idx, (label, apply_attr)) in row_specs.iter().enumerate() {
        let mut col_idx = 0;

        // Render plain label starting at column 0
        for ch in label.chars() {
            let mut cell = CellSnapshot::default();
            cell.text = ch.to_string();
            grid[row_idx][col_idx] = cell;
            col_idx += 1;
        }

        // Render styled sample text
        for ch in sample_text.chars() {
            let mut cell = CellSnapshot::default();
            cell.text = ch.to_string();
            apply_attr(&mut cell);
            grid[row_idx][col_idx] = cell;
            col_idx += 1;
        }
    }

    RenderSnapshot {
        cols,
        rows,
        cursor: CursorSnapshot {
            x: 0,
            y: 0,
            visible: false,
            blinking: false,
            wide_tail: false,
            visual_style: CursorVisualStyle::Block,
        },
        grid,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/ferryx-attribute-evidence.png"));

    let abs_path = if output_path.is_absolute() {
        output_path
    } else {
        env::current_dir()?.join(output_path)
    };

    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let snapshot = build_attribute_snapshot();

    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config)?;
    let frame: OffscreenFrame = renderer.render_snapshot(&snapshot, None)?;

    frame.save_png(&abs_path)?;

    println!("Wrote attribute evidence PNG to: {}", abs_path.display());
    println!("Width: {} px", frame.width_px);
    println!("Height: {} px", frame.height_px);

    Ok(())
}
