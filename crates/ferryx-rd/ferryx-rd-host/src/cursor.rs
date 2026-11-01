//! Native cursor snapshots. Shape pixels are straight-alpha RGBA, top-to-bottom.
use ferryx_rd_proto::CursorShape;
#[cfg(any(target_os = "windows", test))]
use ferryx_rd_proto::CursorUpdate;

#[derive(Default)]
pub(super) struct ShapeCache {
    last: Option<CursorShape>,
}

impl ShapeCache {
    pub(super) fn update(&mut self, mut shape: CursorShape) -> (u32, Option<CursorShape>) {
        if let Some(last) = &self.last {
            if last.width == shape.width
                && last.height == shape.height
                && last.hotspot_x == shape.hotspot_x
                && last.hotspot_y == shape.hotspot_y
                && last.rgba == shape.rgba
            {
                return (last.shape_id, None);
            }
        }
        static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        // IDs survive capture restarts and concurrent sessions: a receiver must
        // never associate a delayed position with a different shape.
        shape.shape_id = NEXT_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .max(1);
        self.last = Some(shape.clone());
        (shape.shape_id, Some(shape))
    }
}

fn straight_alpha(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            if let Some(straight) = (u32::from(*channel) * 255 + alpha / 2).checked_div(alpha) {
                *channel = straight.min(255) as u8;
            }
        }
    }
}

/// Linux cursors are premultiplied RGBA/BGRA. Keep the existing 128px wire
/// limit by sampling larger (up to 256px) accessibility cursors at half size.
#[cfg(target_os = "linux")]
pub(super) fn linux_shape(
    width: u32,
    height: u32,
    stride: i32,
    hotspot: (i32, i32),
    bgra: bool,
    opaque: bool,
    pixels: &[u8],
) -> Option<CursorShape> {
    if width == 0
        || height == 0
        || width > 256
        || height > 256
        || stride != (width * 4) as i32
        || hotspot.0 < 0
        || hotspot.1 < 0
        || hotspot.0 as u32 >= width
        || hotspot.1 as u32 >= height
        || pixels.len() != width as usize * height as usize * 4
    {
        return None;
    }
    let divisor = if width > 128 || height > 128 { 2 } else { 1 };
    let out_width = width.div_ceil(divisor);
    let out_height = height.div_ceil(divisor);
    let mut rgba = Vec::with_capacity((out_width * out_height * 4) as usize);
    for y in 0..out_height {
        for x in 0..out_width {
            let offset = ((y * divisor * width + x * divisor) * 4) as usize;
            let mut pixel: [u8; 4] = pixels[offset..offset + 4].try_into().ok()?;
            if bgra {
                pixel.swap(0, 2);
            }
            if opaque {
                pixel[3] = 255;
            }
            rgba.extend_from_slice(&pixel);
        }
    }
    straight_alpha(&mut rgba);
    Some(CursorShape {
        shape_id: 0,
        width: out_width as u16,
        height: out_height as u16,
        hotspot_x: (hotspot.0 as u32 / divisor) as u16,
        hotspot_y: (hotspot.1 as u32 / divisor) as u16,
        rgba,
    })
}

#[cfg(any(target_os = "macos", test))]
fn image_geometry(
    logical: (f64, f64),
    pixels: (usize, usize),
    hotspot: (f64, f64),
) -> Option<(u16, u16, u16, u16)> {
    if !logical.0.is_finite()
        || !logical.1.is_finite()
        || logical.0 <= 0.0
        || logical.1 <= 0.0
        || pixels.0 == 0
        || pixels.1 == 0
        || pixels.0 > 128
        || pixels.1 > 128
    {
        return None;
    }
    let (width, height) = (pixels.0 as u16, pixels.1 as u16);
    let x = (hotspot.0 * pixels.0 as f64 / logical.0).round().max(0.0) as u16;
    let y = (hotspot.1 * pixels.1 as f64 / logical.1).round().max(0.0) as u16;
    Some((width, height, x.min(width - 1), y.min(height - 1)))
}

#[cfg(target_os = "macos")]
pub(super) fn mac_shape(cache: &mut ShapeCache) -> Option<(u32, Option<CursorShape>)> {
    use core_graphics::geometry::CGRect;
    use objc2_app_kit::NSCursor;
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits: usize,
            stride: usize,
            space: *mut c_void,
            info: u32,
        ) -> *mut c_void;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGContextDrawImage(context: *mut c_void, rect: CGRect, image: *const c_void);
        fn CGContextTranslateCTM(context: *mut c_void, x: f64, y: f64);
        fn CGContextScaleCTM(context: *mut c_void, x: f64, y: f64);
        fn CGContextRelease(context: *mut c_void);
    }
    objc2::rc::autoreleasepool(|_| unsafe {
        // This getter does not create an event tap and needs no Input Monitoring
        // permission. It is deprecated, but still the only system-wide AppKit getter.
        #[allow(deprecated)]
        let cursor = NSCursor::currentSystemCursor()?;
        let hotspot = cursor.hotSpot();
        let image = cursor.image();
        let size = image.size();
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return None;
        }
        let cg = image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)?;
        let image_ptr = (&*cg as *const objc2_core_graphics::CGImage).cast();
        let pixel_width = CGImageGetWidth(image_ptr);
        let pixel_height = CGImageGetHeight(image_ptr);
        // Wire dimensions are physical pixels of the selected CGImage
        // representation, not NSImage points. Hotspots use the same scale.
        let (width, height, hotspot_x, hotspot_y) = image_geometry(
            (size.width, size.height),
            (pixel_width, pixel_height),
            (hotspot.x, hotspot.y),
        )?;
        let mut rgba = vec![0; usize::from(width) * usize::from(height) * 4];
        let space = CGColorSpaceCreateDeviceRGB();
        if space.is_null() {
            return None;
        }
        // Big endian byte order + premultiplied last means RGBA bytes.
        let context = CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width.into(),
            height.into(),
            8,
            usize::from(width) * 4,
            space,
            (4 << 12) | 1,
        );
        CGColorSpaceRelease(space);
        if context.is_null() {
            return None;
        }
        CGContextTranslateCTM(context, 0.0, f64::from(height));
        CGContextScaleCTM(context, 1.0, -1.0);
        CGContextDrawImage(
            context,
            CGRect::new(
                &core_graphics::geometry::CGPoint::new(0.0, 0.0),
                &core_graphics::geometry::CGSize::new(width.into(), height.into()),
            ),
            image_ptr,
        );
        CGContextRelease(context);
        straight_alpha(&mut rgba);
        Some(cache.update(CursorShape {
            shape_id: 0,
            width,
            height,
            hotspot_x: hotspot_x.min(width - 1),
            hotspot_y: hotspot_y.min(height - 1),
            rgba,
        }))
    })
}

#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
struct HandleCache {
    shapes: ShapeCache,
    handles: std::collections::HashMap<usize, CursorShape>,
    handle: Option<usize>,
    shape_id: Option<u32>,
}

#[cfg(any(target_os = "windows", test))]
impl HandleCache {
    fn sample(
        &mut self,
        handle: usize,
        capture: impl FnOnce() -> Option<CursorShape>,
    ) -> (Option<u32>, Option<CursorShape>) {
        if self.handle == Some(handle) {
            return (self.shape_id, None);
        }
        self.handle = Some(handle);
        self.shape_id = None;
        let native = self.handles.get(&handle).cloned().or_else(capture);
        let Some(native) = native else {
            self.shapes = ShapeCache::default();
            return (None, None);
        };
        if self.handles.len() >= 64 {
            self.handles.clear();
        }
        self.handles.insert(handle, native.clone());
        let (id, changed) = self.shapes.update(native);
        self.shape_id = Some(id);
        (Some(id), changed)
    }
}

#[cfg(any(target_os = "windows", test))]
fn position(x: f64, y: f64, origin_x: f64, origin_y: f64, width: f64, height: f64) -> CursorUpdate {
    let x = (x - origin_x) / width;
    let y = (y - origin_y) / height;
    if width > 0.0 && height > 0.0 && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y) {
        CursorUpdate {
            x: x as f32,
            y: y as f32,
            cursor_type: 1,
        }
    } else {
        CursorUpdate {
            x: -1.0,
            y: -1.0,
            cursor_type: 0,
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Default)]
pub(super) struct WindowsCursor {
    cache: HandleCache,
}

#[cfg(target_os = "windows")]
impl WindowsCursor {
    pub fn sample(
        &mut self,
        origin_x: i32,
        origin_y: i32,
        width: u32,
        height: u32,
    ) -> (CursorUpdate, Option<u32>, Option<CursorShape>) {
        use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO, CURSOR_SHOWING};
        let mut info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let hidden = CursorUpdate {
            x: -1.0,
            y: -1.0,
            cursor_type: 0,
        };
        if unsafe { GetCursorInfo(&mut info) }.is_err() || info.flags.0 & CURSOR_SHOWING.0 == 0 {
            return (hidden, self.cache.shape_id, None);
        }
        let (shape_id, shape) = self.cache.sample(info.hCursor.0 as usize, || unsafe {
            windows_shape(info.hCursor)
        });
        let position = position(
            f64::from(info.ptScreenPos.x),
            f64::from(info.ptScreenPos.y),
            f64::from(origin_x),
            f64::from(origin_y),
            f64::from(width),
            f64::from(height),
        );
        (position, shape_id, shape)
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_shape(
    cursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR,
) -> Option<CursorShape> {
    use windows::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};
    // GetIconInfo returns owned bitmap copies; always release both, even when
    // rejecting an oversized/accessibility cursor.
    let mut icon = ICONINFO::default();
    unsafe { GetIconInfo(HICON(cursor.0), &mut icon) }.ok()?;
    let result = (|| unsafe {
        let bitmap = if !icon.hbmColor.0.is_null() {
            icon.hbmColor
        } else {
            icon.hbmMask
        };
        let mut metadata = BITMAP::default();
        if GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut metadata as *mut BITMAP).cast()),
        ) == 0
        {
            return None;
        }
        let width = metadata.bmWidth;
        let height = metadata.bmHeight / if icon.hbmColor.0.is_null() { 2 } else { 1 };
        if !(1..=128).contains(&width) || !(1..=128).contains(&height) {
            return None;
        }
        let dc = GetDC(None);
        if dc.0.is_null() {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut rgba = vec![0; width as usize * height as usize * 4];
        let ok = if !icon.hbmColor.0.is_null() {
            GetDIBits(
                dc,
                icon.hbmColor,
                0,
                height as u32,
                Some(rgba.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            ) == height
        } else {
            false
        };
        // A classic or monochrome cursor has no alpha. Render onto a transparent
        // DIB and derive its opacity from the AND mask below.
        let rendered = if !ok || rgba.chunks_exact(4).all(|p| p[3] == 0) {
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok();
            if let Some(dib) = dib {
                let memory_dc = CreateCompatibleDC(Some(dc));
                if !memory_dc.0.is_null() && !bits.is_null() {
                    let old = SelectObject(memory_dc, dib.into());
                    std::ptr::write_bytes(bits, 0, rgba.len());
                    let drawn = DrawIconEx(
                        memory_dc,
                        0,
                        0,
                        HICON(cursor.0),
                        width,
                        height,
                        0,
                        None,
                        DI_NORMAL,
                    )
                    .is_ok();
                    if drawn {
                        let len = rgba.len();
                        rgba.copy_from_slice(std::slice::from_raw_parts(bits.cast::<u8>(), len));
                    }
                    SelectObject(memory_dc, old);
                    let _ = DeleteDC(memory_dc);
                    let _ = DeleteObject(dib.into());
                    drawn
                } else {
                    if !memory_dc.0.is_null() {
                        let _ = DeleteDC(memory_dc);
                    }
                    let _ = DeleteObject(dib.into());
                    false
                }
            } else {
                false
            }
        } else {
            true
        };
        let _ = ReleaseDC(None, dc);
        if !rendered {
            return None;
        }
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        // DrawIconEx populates alpha for modern cursors. For legacy cursors,
        // derive alpha from the monochrome AND bitmap (top half for mono).
        if rgba.chunks_exact(4).all(|p| p[3] == 0) {
            let mask_height = if icon.hbmColor.0.is_null() {
                height * 2
            } else {
                height
            };
            let mut mask_info = info;
            mask_info.bmiHeader.biHeight = -mask_height;
            let mut mask = vec![0u8; width as usize * mask_height as usize * 4];
            let dc = GetDC(None);
            let rows = GetDIBits(
                dc,
                icon.hbmMask,
                0,
                mask_height as u32,
                Some(mask.as_mut_ptr().cast()),
                &mut mask_info,
                DIB_RGB_COLORS,
            );
            let _ = ReleaseDC(None, dc);
            if rows != mask_height {
                return None;
            }
            for (pixel, and) in rgba.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                pixel[3] = if and[0] == 0 || pixel[..3].iter().any(|v| *v != 0) {
                    255
                } else {
                    0
                };
            }
        }
        straight_alpha(&mut rgba);
        Some(CursorShape {
            shape_id: 0,
            width: width as u16,
            height: height as u16,
            hotspot_x: (icon.xHotspot as u16).min(width as u16 - 1),
            hotspot_y: (icon.yHotspot as u16).min(height as u16 - 1),
            rgba,
        })
    })();
    unsafe {
        if !icon.hbmColor.0.is_null() {
            let _ = DeleteObject(icon.hbmColor.into());
        }
        if !icon.hbmMask.0.is_null() {
            let _ = DeleteObject(icon.hbmMask.into());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pixels_alpha_and_accessibility_geometry() {
        let shape = linux_shape(1, 1, 4, (0, 0), true, false, &[16, 32, 64, 128]).unwrap();
        assert_eq!(shape.rgba, vec![128, 64, 32, 128]);
        let pixels = vec![255; 256 * 256 * 4];
        let shape = linux_shape(256, 256, 1024, (255, 255), false, false, &pixels).unwrap();
        assert_eq!(
            (shape.width, shape.height, shape.hotspot_x, shape.hotspot_y),
            (128, 128, 127, 127)
        );
        let mut cache = ShapeCache::default();
        let shape = cache.update(shape).1.unwrap();
        assert!(ferryx_rd_proto::WireCodec::encode(&shape).is_ok());
        assert!(linux_shape(257, 256, 1028, (0, 0), false, false, &pixels).is_none());
    }
    #[test]
    fn shape_ids_only_change_with_content() {
        let mut cache = ShapeCache::default();
        let shape = CursorShape {
            shape_id: 0,
            width: 1,
            height: 1,
            hotspot_x: 0,
            hotspot_y: 0,
            rgba: vec![1, 2, 3, 255],
        };
        let (first, changed) = cache.update(shape.clone());
        assert!(changed.is_some());
        assert_eq!(cache.update(shape.clone()), (first, None));
        let mut next = shape.clone();
        next.rgba[0] = 2;
        let second = cache.update(next).0;
        assert_ne!(second, first);
        // A -> B -> A must republish A: a bounded client cache may have
        // evicted its earlier image even though the native handle is cached.
        let (returned_id, returned) = cache.update(shape.clone());
        assert_ne!(returned_id, first);
        assert_ne!(returned_id, second);
        assert_eq!(returned.unwrap().rgba, shape.rgba);
    }
    #[test]
    fn handle_cache_reuses_pixels_but_republishes_on_return() {
        let shape = |value| CursorShape {
            shape_id: 0,
            width: 1,
            height: 1,
            hotspot_x: 0,
            hotspot_y: 0,
            rgba: vec![value, 0, 0, 255],
        };
        let mut cache = HandleCache::default();
        let (first, pixels) = cache.sample(10, || Some(shape(1)));
        assert!(pixels.is_some());
        assert_eq!(
            cache.sample(10, || panic!("unchanged handle must not recapture")),
            (first, None)
        );
        let (second, _) = cache.sample(20, || Some(shape(2)));
        let (returned, pixels) = cache.sample(10, || panic!("cached handle must not recapture"));
        assert_ne!(returned, first);
        assert_ne!(returned, second);
        assert_eq!(pixels.unwrap().rgba, shape(1).rgba);
        assert_eq!(cache.sample(30, || None), (None, None));
        assert!(cache.sample(10, || panic!("retained pixels")).1.is_some());
    }

    #[test]
    fn handle_cache_is_bounded() {
        let mut cache = HandleCache::default();
        for handle in 0..130 {
            cache.sample(handle, || {
                Some(CursorShape {
                    shape_id: 0,
                    width: 1,
                    height: 1,
                    hotspot_x: 0,
                    hotspot_y: 0,
                    rgba: vec![1, 0, 0, 255],
                })
            });
            assert!(cache.handles.len() <= 64);
        }
    }

    #[test]
    fn mapping_accounts_for_negative_origin_and_display_edges() {
        assert_eq!(
            position(-960.0, 540.0, -1920.0, 0.0, 1920.0, 1080.0),
            CursorUpdate {
                x: 0.5,
                y: 0.5,
                cursor_type: 1
            }
        );
        assert_eq!(
            position(-1920.0, 0.0, -1920.0, 0.0, 1920.0, 1080.0).cursor_type,
            1
        );
        for (x, y, width) in [
            (0.0, 0.0, 1920.0),
            (-1921.0, 0.0, 1920.0),
            (-960.0, 1080.0, 1920.0),
            (-960.0, 0.0, 0.0),
            (f64::NAN, 0.0, 1920.0),
        ] {
            assert_eq!(
                position(x, y, -1920.0, 0.0, width, 1080.0),
                CursorUpdate {
                    x: -1.0,
                    y: -1.0,
                    cursor_type: 0
                }
            );
        }
    }

    #[test]
    fn stream_legacy_fallback_keeps_nine_byte_positions() {
        use ferryx_rd_proto::WireCodec;
        let cursor = position(50.0, 25.0, 0.0, 0.0, 100.0, 100.0);
        for negotiated in [false, true] {
            for available in [None, Some(7)] {
                let id = negotiated.then_some(available.unwrap_or(0));
                let bytes = cursor.encode_with_shape(id).unwrap();
                assert_eq!(bytes.len(), if negotiated { 13 } else { 9 });
                assert_eq!(
                    CursorUpdate::decode_with_shape(&bytes, negotiated).unwrap(),
                    (cursor, id)
                );
                if !negotiated {
                    assert_eq!(bytes, cursor.encode().unwrap());
                }
            }
        }
    }

    #[test]
    fn retina_image_dimensions_and_hotspots_share_pixel_scale() {
        assert_eq!(
            image_geometry((16.0, 24.0), (32, 48), (3.0, 5.0)),
            Some((32, 48, 6, 10))
        );
        assert_eq!(
            image_geometry((16.0, 24.0), (16, 24), (3.0, 5.0)),
            Some((16, 24, 3, 5))
        );
        assert_eq!(image_geometry((16.0, 24.0), (129, 48), (3.0, 5.0)), None);
        assert_eq!(image_geometry((0.0, 24.0), (32, 48), (3.0, 5.0)), None);
    }

    #[test]
    fn alpha_is_unpremultiplied_without_overflow() {
        let mut pixels = [64, 32, 0, 128, 255, 1, 2, 255];
        straight_alpha(&mut pixels);
        assert_eq!(pixels, [128, 64, 0, 128, 255, 1, 2, 255]);
    }
}
