//! Owned Kitty image snapshots. No borrowed foreign handle survives capture.
use super::{
    error::NativeTerminalError,
    sys::{
        ffi::ghostty_terminal_get,
        kitty::*,
        types::{GhosttyTerminal, GHOSTTY_NO_VALUE},
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Immutable straight-alpha RGBA pixels, shared across placements and frames.
#[derive(Debug, PartialEq, Eq)]
pub struct TerminalImage {
    pub id: u32,
    /// Process-unique libghostty generation, including across screens/terminals.
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePlacementSnapshot {
    pub image: Arc<TerminalImage>,
    pub placement_id: u32,
    pub z: i32,
    /// Signed viewport-relative cell origin; partially scrolled images are negative.
    pub viewport_col: i32,
    pub viewport_row: i32,
    pub offset_x: u32,
    pub offset_y: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Source rectangle in original image pixels (x, y, width, height).
    pub source: [u32; 4],
}

#[derive(Default)]
pub struct ImageCache {
    generation: u64,
    images: HashMap<u32, Arc<TerminalImage>>,
}

struct IteratorGuard(PlacementIterator);
impl Drop for IteratorGuard {
    fn drop(&mut self) {
        // SAFETY: This independently owned iterator is freed exactly once.
        unsafe { ghostty_kitty_graphics_placement_iterator_free(self.0) }
    }
}

fn check(result: i32) -> Result<(), NativeTerminalError> {
    NativeTerminalError::from_c_result(result, "Kitty graphics snapshot")
}

// These query wrappers remain private: callers pair each C key with its exact
// header-documented initialized output type. All handles are capture-local.
unsafe fn image_get<T: Default>(image: Image, key: i32) -> Result<T, NativeTerminalError> {
    let mut out = T::default();
    check(unsafe { ghostty_kitty_graphics_image_get(image, key, (&mut out as *mut T).cast()) })?;
    Ok(out)
}
unsafe fn placement_get<T: Default>(
    iter: PlacementIterator,
    key: i32,
) -> Result<T, NativeTerminalError> {
    let mut out = T::default();
    check(unsafe { ghostty_kitty_graphics_placement_get(iter, key, (&mut out as *mut T).cast()) })?;
    Ok(out)
}

impl ImageCache {
    pub fn capture(
        &mut self,
        terminal: GhosttyTerminal,
    ) -> Result<Vec<ImagePlacementSnapshot>, NativeTerminalError> {
        let mut graphics: Graphics = std::ptr::null_mut();
        // SAFETY: Terminal is exclusively accessed for this capture. Each output
        // has the exact C ABI type and borrowed handles never escape this scope.
        let result = unsafe {
            ghostty_terminal_get(terminal, GRAPHICS, (&mut graphics as *mut Graphics).cast())
        };
        if result == GHOSTTY_NO_VALUE {
            self.images.clear();
            self.generation = 0;
            return Ok(Vec::new());
        }
        check(result)?;
        let mut generation = 0u64;
        check(unsafe {
            ghostty_kitty_graphics_get(graphics, GENERATION, (&mut generation as *mut u64).cast())
        })?;
        if generation == 0 {
            self.images.clear();
            self.generation = 0;
            return Ok(Vec::new());
        }
        let changed = self.generation != generation;
        let mut iter = std::ptr::null_mut();
        check(unsafe {
            ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut iter)
        })?;
        let guard = IteratorGuard(iter);
        check(unsafe {
            ghostty_kitty_graphics_get(
                graphics,
                PLACEMENT_ITERATOR,
                (&mut iter as *mut PlacementIterator).cast(),
            )
        })?;
        let mut placements = Vec::new();
        let mut retained = HashSet::new();
        while NativeTerminalError::decode_c_bool(
            unsafe { ghostty_kitty_graphics_placement_next(guard.0) },
            "kitty placement next",
        )? {
            let id: u32 = unsafe { placement_get(iter, PLACEMENT_IMAGE_ID)? };
            let raw_image = unsafe { ghostty_kitty_graphics_image(graphics, id) };
            if raw_image.is_null() {
                continue;
            }
            retained.insert(id);
            if changed || !self.images.contains_key(&id) {
                let image_generation: u64 = unsafe { image_get(raw_image, IMAGE_GENERATION)? };
                if !self
                    .images
                    .get(&id)
                    .is_some_and(|i| i.generation == image_generation)
                {
                    let width: u32 = unsafe { image_get(raw_image, IMAGE_WIDTH)? };
                    let height: u32 = unsafe { image_get(raw_image, IMAGE_HEIGHT)? };
                    let format: i32 = unsafe { image_get(raw_image, IMAGE_FORMAT)? };
                    let len: usize = unsafe { image_get(raw_image, IMAGE_LEN)? };
                    let mut ptr: *const u8 = std::ptr::null();
                    let result = unsafe {
                        ghostty_kitty_graphics_image_get(
                            raw_image,
                            IMAGE_DATA,
                            (&mut ptr as *mut *const u8).cast(),
                        )
                    };
                    if result == GHOSTTY_NO_VALUE {
                        self.images.remove(&id);
                        continue;
                    }
                    check(result)?;
                    let channels = match format {
                        0 => 3,
                        1 => 4,
                        3 => 2,
                        4 => 1,
                        _ => return Err(NativeTerminalError::ForeignErrorCode(format)),
                    };
                    let pixels = (width as usize)
                        .checked_mul(height as usize)
                        .ok_or(NativeTerminalError::LimitExceeded)?;
                    if ptr.is_null()
                        || pixels == 0
                        || pixels > super::png_decoder::IMAGE_LIMIT / 4
                        || pixels.checked_mul(channels) != Some(len)
                    {
                        return Err(NativeTerminalError::InvalidValue(
                            "Invalid Kitty image payload dimensions".into(),
                        ));
                    }
                    // SAFETY: Validated non-null decoded foreign buffer and exact
                    // format length; copied before any mutating terminal call.
                    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
                    let rgba = match channels {
                        4 => data.to_vec(),
                        3 => data
                            .chunks_exact(3)
                            .flat_map(|p| [p[0], p[1], p[2], 255])
                            .collect(),
                        2 => data
                            .chunks_exact(2)
                            .flat_map(|p| [p[0], p[0], p[0], p[1]])
                            .collect(),
                        _ => data.iter().flat_map(|&p| [p, p, p, 255]).collect(),
                    };
                    self.images.insert(
                        id,
                        Arc::new(TerminalImage {
                            id,
                            generation: image_generation,
                            width,
                            height,
                            rgba,
                        }),
                    );
                }
            }
            let mut info = RenderInfo {
                size: std::mem::size_of::<RenderInfo>(),
                ..Default::default()
            };
            check(unsafe {
                ghostty_kitty_graphics_placement_render_info(iter, raw_image, terminal, &mut info)
            })?;
            if !NativeTerminalError::decode_c_bool(info.viewport_visible, "kitty viewport visible")?
            {
                continue;
            }
            let Some(image) = self.images.get(&id) else {
                continue;
            };
            placements.push(ImagePlacementSnapshot {
                image: Arc::clone(image),
                placement_id: unsafe { placement_get(iter, PLACEMENT_ID)? },
                z: unsafe { placement_get(iter, PLACEMENT_Z)? },
                viewport_col: info.viewport_col,
                viewport_row: info.viewport_row,
                offset_x: unsafe { placement_get(iter, PLACEMENT_X_OFFSET)? },
                offset_y: unsafe { placement_get(iter, PLACEMENT_Y_OFFSET)? },
                pixel_width: info.pixel_width,
                pixel_height: info.pixel_height,
                source: [
                    info.source_x,
                    info.source_y,
                    info.source_width,
                    info.source_height,
                ],
            });
        }
        self.images.retain(|id, _| retained.contains(id));
        self.generation = generation;
        placements.sort_by_key(|p| (p.z, p.image.id, p.placement_id));
        Ok(placements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::{NativeTerminal, TerminalEngine};

    #[test]
    fn kitty_snapshots_share_pixels_and_outlive_replacement_and_terminal() {
        let mut terminal = NativeTerminal::new(8, 6).unwrap();
        terminal.resize(8, 6, 8, 8).unwrap();
        terminal
            .feed(b"\x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,c=1,r=1,C=1;/wAA\x1b\\")
            .unwrap();
        let first = terminal.render_snapshot().unwrap();
        terminal.feed(b"text").unwrap();
        let second = terminal.render_snapshot().unwrap();
        assert!(Arc::ptr_eq(&first.images[0].image, &second.images[0].image));
        terminal
            .feed(b"\x1b_Ga=p,i=1,p=2,c=1,r=1,C=1\x1b\\")
            .unwrap();
        let placements = terminal.render_snapshot().unwrap();
        assert_eq!(placements.images.len(), 2);
        assert!(Arc::ptr_eq(
            &placements.images[0].image,
            &placements.images[1].image
        ));
        terminal
            .feed(b"\x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,c=1,r=1,C=1;AAD/\x1b\\")
            .unwrap();
        let replaced = terminal.render_snapshot().unwrap();
        assert_ne!(
            first.images[0].image.generation,
            replaced.images[0].image.generation
        );
        drop(terminal);
        assert_eq!(first.images[0].image.rgba, [255, 0, 0, 255]);
        assert_eq!(replaced.images[0].image.rgba, [0, 0, 255, 255]);
        assert!(serde_json::to_value(&first)
            .unwrap()
            .get("images")
            .is_none());
    }

    #[test]
    fn kitty_query_and_png_failure_remain_protocol_responses() {
        let mut terminal = NativeTerminal::new(8, 6).unwrap();
        terminal.resize(8, 6, 8, 8).unwrap();
        terminal
            .feed(b"\x1b_Ga=q,f=24,s=1,v=1,i=12;/wAA\x1b\\")
            .unwrap();
        let replies: Vec<u8> = terminal
            .buffered_pty_writes()
            .iter()
            .flat_map(|r| r.data.iter().copied())
            .collect();
        assert!(replies.windows(7).any(|w| w == b"i=12;OK"));
        terminal
            .feed(b"\x1b_Ga=T,f=100,i=13;bm90IGEgcG5n\x1b\\")
            .unwrap();
        assert!(terminal.render_snapshot().unwrap().images.is_empty());
    }
}
