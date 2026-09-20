use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use super::composition::{LogicalBounds, SurfaceCompositionLayout};
use super::surface_host::{NativeTerminalSurfaceReceipt, SessionRenderInput};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublishedFrame {
    pub generation: u64,
    pub attachment_epoch: u64,
    pub layout: SurfaceCompositionLayout,
    pub logical_bounds: LogicalBounds,
    pub input: SessionRenderInput,
}

/// Receipt of a frame the GPU actually presented, tagged with the frame and attachment it
/// belongs to.
///
/// The native host defers every frame, so a bounds IPC cannot learn from its own return value
/// whether the surface was painted. This is the completion signal it waits on instead, and the
/// tags are what keep a late or detached completion from acknowledging a different attachment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PresentedFrame {
    pub generation: u64,
    pub attachment_epoch: u64,
    pub receipt: NativeTerminalSurfaceReceipt,
}

#[derive(Debug)]
pub struct SnapshotSlot {
    ready: Mutex<Option<Arc<PublishedFrame>>>,
    presented: tokio::sync::watch::Sender<Option<PresentedFrame>>,
    generation: AtomicU64,
    attached: AtomicBool,
    attachment_epoch: AtomicU64,
}

impl Default for SnapshotSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotSlot {
    pub fn new() -> Self {
        Self {
            ready: Mutex::new(None),
            presented: tokio::sync::watch::channel(None).0,
            generation: AtomicU64::new(0),
            attached: AtomicBool::new(false),
            attachment_epoch: AtomicU64::new(1),
        }
    }

    pub fn set_attached(&self, attached: bool) -> u64 {
        // Held across the whole transition so a presentation cannot be validated against the old
        // epoch and then land in the channel after this attachment is gone.
        let mut ready = self.ready.lock();
        let epoch = self.attachment_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.attached.store(attached, Ordering::SeqCst);
        if !attached {
            *ready = None;
        }
        // The previous attachment's presentation must never satisfy a waiter that belongs to this
        // one, and clearing it also wakes anybody parked on the old attachment.
        self.presented.send_replace(None);
        epoch
    }

    pub fn is_attached(&self) -> bool {
        self.attached.load(Ordering::Acquire)
    }

    pub fn current_epoch(&self) -> u64 {
        self.attachment_epoch.load(Ordering::Acquire)
    }

    pub fn is_attached_with_epoch(&self, expected_epoch: u64) -> bool {
        self.is_attached() && self.current_epoch() == expected_epoch
    }

    pub(crate) fn publish(
        &self,
        layout: SurfaceCompositionLayout,
        logical_bounds: LogicalBounds,
        input: SessionRenderInput,
    ) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let attachment_epoch = self.current_epoch();
        let frame = Arc::new(PublishedFrame {
            generation,
            attachment_epoch,
            layout,
            logical_bounds,
            input,
        });
        *self.ready.lock() = Some(frame);
        generation
    }

    pub(crate) fn consume(&self) -> Option<Arc<PublishedFrame>> {
        if !self.is_attached() {
            return None;
        }
        let frame = self.ready.lock().clone()?;
        if frame.attachment_epoch == self.current_epoch() {
            Some(frame)
        } else {
            None
        }
    }

    /// Records a frame the GPU presented. Rejected (returning `false`) when the attachment the
    /// frame was rendered for is gone, so a detached or superseded completion cannot mark the
    /// current attachment painted.
    ///
    /// The validity check and the publish share [`Self::set_attached`]'s lock: without that, a
    /// detach landing between them would push a retired frame into the channel after the channel
    /// had already been cleared, and the next attachment's waiter would read it as its own.
    pub(crate) fn publish_presentation(
        &self,
        generation: u64,
        attachment_epoch: u64,
        receipt: NativeTerminalSurfaceReceipt,
    ) -> bool {
        let _guard = self.ready.lock();
        if !self.is_attached_with_epoch(attachment_epoch) {
            return false;
        }
        self.presented.send_replace(Some(PresentedFrame {
            generation,
            attachment_epoch,
            receipt,
        }));
        true
    }

    pub(crate) fn subscribe_presentations(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<PresentedFrame>> {
        self.presented.subscribe()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::composition::PhysicalBounds;

    fn mock_input() -> SessionRenderInput {
        use crate::native_terminal::cursor::{CursorSnapshot, CursorVisualStyle};
        use crate::native_terminal::snapshot::RenderSnapshot;
        SessionRenderInput {
            snapshot: RenderSnapshot {
                cols: 80,
                rows: 24,
                cursor: CursorSnapshot {
                    x: 0,
                    y: 0,
                    visible: false,
                    blinking: false,
                    wide_tail: false,
                    visual_style: CursorVisualStyle::Block,
                },
                grid: Vec::new(),
                images: Vec::new(),
            },
            selection: None,
            scrollbar_overlay: None,
            attention_frame: false,
            synchronized_output: false,
        }
    }

    #[test]
    fn publish_and_consume_updates_generation_and_leaves_frame() {
        let slot = SnapshotSlot::new();
        slot.set_attached(true);
        let layout = SurfaceCompositionLayout {
            cols: 80,
            rows: 24,
            physical_bounds: PhysicalBounds {
                x: 0,
                y: 0,
                width: 800,
                height: 480,
            },
        };
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        assert_eq!(slot.generation(), 0);
        assert!(slot.consume().is_none());

        let gen = slot.publish(layout, bounds, mock_input());
        assert_eq!(gen, 1);
        assert_eq!(slot.generation(), 1);

        let consumed = slot.consume().expect("frame available");
        assert_eq!(consumed.generation, 1);
        assert_eq!(consumed.attachment_epoch, slot.current_epoch());

        // Triple-buffered: consequent consume still returns the published frame
        let consumed2 = slot.consume().expect("frame still available");
        assert_eq!(consumed2.generation, 1);

        // Detaching clears the frame and invalidates consume
        slot.set_attached(false);
        assert!(slot.consume().is_none());
        assert!(!slot.is_attached_with_epoch(consumed.attachment_epoch));
    }

    #[test]
    fn epoch_change_invalidates_stale_frame() {
        let slot = SnapshotSlot::new();
        slot.set_attached(true);
        let epoch1 = slot.current_epoch();

        let layout = SurfaceCompositionLayout {
            cols: 80,
            rows: 24,
            physical_bounds: PhysicalBounds {
                x: 0,
                y: 0,
                width: 800,
                height: 480,
            },
        };
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };
        slot.publish(layout, bounds, mock_input());

        assert!(slot.is_attached_with_epoch(epoch1));
        assert!(slot.consume().is_some());

        // Reattach creates a new epoch
        let epoch2 = slot.set_attached(true);
        assert_ne!(epoch1, epoch2);

        // Prior frame had epoch1, so consume rejects it as stale until next publish
        assert!(slot.consume().is_none());

        slot.publish(layout, bounds, mock_input());
        let frame2 = slot.consume().expect("frame available for epoch 2");
        assert_eq!(frame2.attachment_epoch, epoch2);
    }
}
