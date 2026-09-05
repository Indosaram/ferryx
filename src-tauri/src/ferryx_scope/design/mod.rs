//! Native viewport capture and generation fence. Call PNG work on a blocking worker.
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub mod native;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub browser_id: String,
    pub webview_label: String,
    pub generation: crate::scoped_contracts::Epoch,
    pub operation_id: String,
    pub viewport_revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport { pub width: f64, pub height: f64, pub dpr: f64, pub zoom: f64 }
#[derive(Debug, thiserror::Error)]
pub enum DesignError {
    #[error("TARGET_EXPIRED: browser or viewport changed")] Stale,
    #[error("INVALID_GEOMETRY: empty, nonfinite or inconsistent scale")] Geometry,
    #[error("INVALID_PNG: {0}")] Png(String),
    #[error("CAPTURE_UNSUPPORTED: {0}")] Unsupported(String),
    #[error("CAPTURE_FAILED: {0}")] Capture(String),
    #[error("TIMEOUT: native snapshot did not complete")] Timeout,
}
pub type Result<T> = std::result::Result<T, DesignError>;
#[derive(Default)]
pub struct CaptureFence { current: Option<(Identity, Viewport)> }
impl CaptureFence {
    pub fn arm(&mut self, identity: Identity, viewport: Viewport) { self.current = Some((identity, viewport)); }
    pub fn invalidate(&mut self) { self.current = None; }
    pub fn complete(&mut self, identity: &Identity, viewport: Viewport) -> Result<()> {
        if self.current.as_ref() != Some(&(identity.clone(), viewport)) { return Err(DesignError::Stale); }
        self.current = None;
        Ok(())
    }
}
pub fn crop_png(bytes: &[u8], selection: Rect, viewport: Viewport) -> Result<Vec<u8>> {
    let Rect { x, y, width, height } = selection;
    if ![x,y,width,height,viewport.width,viewport.height,viewport.dpr,viewport.zoom].iter().all(|v| v.is_finite())
        || width <= 0. || height <= 0. || viewport.width <= 0. || viewport.height <= 0. || viewport.dpr <= 0. || viewport.zoom <= 0. { return Err(DesignError::Geometry); }
    if bytes.len() > crate::scoped_contracts::ATTACHMENT_MAX_FILE_BYTES as usize { return Err(DesignError::Png("file exceeds limit".into())); }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| DesignError::Png(e.to_string()))?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || u64::from(info.width) * u64::from(info.height) > 32 * 1024 * 1024 { return Err(DesignError::Geometry); }
    let sx = f64::from(info.width) / viewport.width;
    let sy = f64::from(info.height) / viewport.height;
    // DPR includes page zoom on some engines. Adapter must supply the measured effective
    // device scale and visual zoom separately, not multiply page zoom twice.
    let expected = viewport.dpr * viewport.zoom;
    if (sx - expected).abs() > 1. / viewport.width || (sy - expected).abs() > 1. / viewport.height { return Err(DesignError::Geometry); }
    let left = (x.max(0.) * sx).floor().min(f64::from(info.width)) as u32;
    let top = (y.max(0.) * sy).floor().min(f64::from(info.height)) as u32;
    let right = ((x + width).min(viewport.width) * sx).ceil().max(0.) as u32;
    let bottom = ((y + height).min(viewport.height) * sy).ceil().max(0.) as u32;
    if right <= left || bottom <= top { return Err(DesignError::Geometry); }
    let mut decoded = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut decoded).map_err(|e| DesignError::Png(e.to_string()))?;
    let channels = match frame.color_type { png::ColorType::Rgba => 4, png::ColorType::Rgb => 3, png::ColorType::GrayscaleAlpha => 2, png::ColorType::Grayscale => 1, _ => return Err(DesignError::Png("unsupported pixel format".into())) };
    let mut cropped = Vec::with_capacity(((right-left)*(bottom-top)) as usize * channels);
    for row in top..bottom {
        let start = (row as usize * frame.width as usize + left as usize) * channels;
        cropped.extend_from_slice(&decoded[start..start + (right-left) as usize * channels]);
    }
    let mut output = Vec::new();
    { let mut encoder = png::Encoder::new(&mut output, right-left, bottom-top); encoder.set_color(frame.color_type); encoder.set_depth(png::BitDepth::Eight);
      encoder.write_header().map_err(|e| DesignError::Png(e.to_string()))?.write_image_data(&cropped).map_err(|e| DesignError::Png(e.to_string()))?; }
    if output.len() > crate::scoped_contracts::ATTACHMENT_MAX_FILE_BYTES as usize { return Err(DesignError::Png("crop exceeds limit".into())); }
    Ok(output)
}
