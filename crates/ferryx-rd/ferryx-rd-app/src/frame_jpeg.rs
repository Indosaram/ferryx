//! Converts decoded NV12 frames into JPEG images for presenters that cannot show NV12.

use std::io::Cursor;

use ferryx_rd_decode::Nv12Frame;
use image::{codecs::jpeg::JpegEncoder, ColorType};

/// Converts a strided NV12 frame to packed RGB8. Odd dimensions are allowed: the last
/// odd row/column reuses the chroma sample of its pair. Returns `None` when a plane is
/// shorter than its declared stride and dimensions require.
pub fn nv12_frame_to_rgb(frame: &Nv12Frame) -> Option<Vec<u8>> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let chroma_w = w.div_ceil(2) * 2;
    let chroma_h = h.div_ceil(2);
    if frame.y_stride < w
        || frame.uv_stride < chroma_w
        || frame.y_plane.len() < frame.y_stride.checked_mul(h - 1)?.checked_add(w)?
        || frame.uv_plane.len()
            < frame
                .uv_stride
                .checked_mul(chroma_h - 1)?
                .checked_add(chroma_w)?
    {
        return None;
    }
    let mut rgb = Vec::with_capacity(w.checked_mul(h)?.checked_mul(3)?);
    for y in 0..h {
        let y_row = &frame.y_plane[y * frame.y_stride..y * frame.y_stride + w];
        let uv_row = &frame.uv_plane[(y / 2) * frame.uv_stride..];
        for (x, luma) in y_row.iter().enumerate() {
            let uv = (x / 2) * 2;
            let y_val = f32::from(*luma);
            let u_val = f32::from(uv_row[uv]) - 128.0;
            let v_val = f32::from(uv_row[uv + 1]) - 128.0;
            rgb.push((y_val + 1.5748 * v_val).clamp(0.0, 255.0) as u8);
            rgb.push((y_val - 0.1873 * u_val - 0.4681 * v_val).clamp(0.0, 255.0) as u8);
            rgb.push((y_val + 1.8556 * u_val).clamp(0.0, 255.0) as u8);
        }
    }
    Some(rgb)
}

/// Encodes a decoded NV12 frame as a baseline JPEG with `quality` (1..=100).
pub fn nv12_frame_to_jpeg(frame: &Nv12Frame, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = nv12_frame_to_rgb(frame).ok_or_else(|| "invalid nv12 frame".to_string())?;
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(Cursor::new(&mut output), quality.clamp(1, 100))
        .encode(&rgb, frame.width, frame.height, ColorType::Rgb8.into())
        .map_err(|error| format!("failed to encode jpeg: {error}"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, y_stride: usize, uv_stride: usize, luma: u8) -> Nv12Frame {
        let chroma_h = (height as usize).div_ceil(2);
        Nv12Frame {
            width,
            height,
            y_stride,
            uv_stride,
            y_plane: vec![luma; y_stride * height as usize],
            uv_plane: vec![128; uv_stride * chroma_h],
            timestamp_ms: 0,
        }
    }

    #[test]
    fn neutral_chroma_yields_gray_and_honours_stride() {
        let rgb = nv12_frame_to_rgb(&frame(3, 3, 8, 8, 100)).unwrap();
        assert_eq!(rgb.len(), 3 * 3 * 3);
        assert!(rgb.iter().all(|value| *value == 100));
    }

    #[test]
    fn short_planes_are_rejected() {
        let mut bad = frame(4, 4, 4, 4, 0);
        bad.uv_plane.truncate(3);
        assert!(nv12_frame_to_rgb(&bad).is_none());
        assert!(nv12_frame_to_rgb(&frame(4, 4, 2, 4, 0)).is_none());
    }

    #[test]
    fn jpeg_output_has_soi_and_eoi_markers() {
        let jpeg = nv12_frame_to_jpeg(&frame(16, 16, 16, 16, 60), 70).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xFF, 0xD9]);
    }
}
