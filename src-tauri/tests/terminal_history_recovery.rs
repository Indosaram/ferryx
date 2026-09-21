#![cfg(feature = "native-terminal")]

use base64::Engine;
use ferryx_lib::native_terminal::{
    NativeTerminal, NativeTerminalRenderer, RendererConfig, ScrollViewport, TerminalEngine,
};
use ferryx_lib::terminal::TerminalOutputHub;

#[test]
fn multipart_recovery_matches_live_after_interleaved_text_and_resize() {
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("multipart-order"));
    hub.record_initial_size("multipart-order", 40, 24);
    let mut live = NativeTerminal::new(40, 24).unwrap();
    live.resize(40, 24, 8, 16).unwrap();
    let first = b"OLDEST\r\n\x1b_Ga=T,f=24,s=2,v=1,i=902,c=2,r=1,q=2,m=1;/wAA\x1b\\";
    live.feed(first).unwrap();
    hub.publish("multipart-order", first.to_vec()).unwrap();
    live.resize(20, 24, 8, 16).unwrap();
    hub.record_resize("multipart-order", 20, 24).unwrap();
    let middle = b"INTERLEAVED-TEXT-WRAPS-PAST-TWENTY\r\n\x1b[5;4H";
    live.feed(middle).unwrap();
    hub.publish("multipart-order", middle.to_vec()).unwrap();
    let snapshot = hub.subscribe_with_sequence("multipart-order", None).unwrap().snapshot;
    let mut restored = NativeTerminal::new(40, 24).unwrap();
    for segment in snapshot.history_segments {
        restored.resize(segment.cols.unwrap(), segment.rows.unwrap(), 8, 16).unwrap();
        restored.feed(&segment.bytes).unwrap();
    }
    let tail = b"\x1b_Gm=0;AP8A\x1b\\AFTER";
    live.feed(tail).unwrap();
    restored.feed(tail).unwrap();
    let expected = live.render_snapshot().unwrap();
    let actual = restored.render_snapshot().unwrap();
    for row in 0..24 {
        assert_eq!(actual.row_text(row), expected.row_text(row), "row {row}");
    }
    assert_eq!(actual.images.len(), 1);
    assert_eq!(expected.images.len(), 1);
    assert_eq!(actual.images[0].viewport_col, expected.images[0].viewport_col);
    assert_eq!(actual.images[0].viewport_row, expected.images[0].viewport_row);
    assert_eq!(actual.images[0].image.rgba, expected.images[0].image.rgba);
}

#[test]
fn fresh_terminal_recovers_old_text_and_image_after_large_image_output() {
    // Given: a live session whose image traffic exceeds the transport ring.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("image-history"));
    let mut live = NativeTerminal::new(80, 24).expect("live terminal");
    live.resize(80, 24, 8, 16).expect("metrics");
    let mut stream = b"\x1b[?25lOLDEST-TEXT-MUST-SURVIVE\r\n".to_vec();
    let pixels = [255, 0, 0].repeat(512 * 512);
    for (index, part) in pixels.chunks(3072).enumerate() {
        let more = u8::from((index + 1) * 3072 < pixels.len());
        let header = if index == 0 {
            format!("a=T,f=24,s=512,v=512,i=901,c=12,r=6,C=1,q=2,m={more}")
        } else {
            format!("m={more}")
        };
        stream.extend_from_slice(
            format!("\x1b_G{header};{}\x1b\\", base64::prelude::BASE64_STANDARD.encode(part)).as_bytes(),
        );
    }
    stream.extend_from_slice(b"\x1b[7B\r\n");
    for line in 0..60 {
        stream.extend_from_slice(format!("AFTER-IMAGE-{line:02}\r\n").as_bytes());
    }
    assert!(stream.len() > 512 * 1024);
    for chunk in stream.chunks(8191) {
        live.feed(chunk).expect("live output");
        hub.publish("image-history", chunk.to_vec()).expect("publish");
    }

    // When: a new GUI terminal reconstructs itself from the surviving daemon.
    let attachment = hub.subscribe_with_sequence("image-history", None).expect("attach");
    let mut restored = NativeTerminal::new(80, 24).expect("new GUI terminal");
    restored.resize(80, 24, 8, 16).expect("restored metrics");
    restored.feed(&attachment.snapshot.history).expect("replay");
    live.scroll_viewport(ScrollViewport::Top).expect("live top");
    restored.scroll_viewport(ScrollViewport::Top).expect("restored top");
    let expected = live.render_snapshot().expect("live snapshot");
    let actual = restored.render_snapshot().expect("restored snapshot");

    // Then: the oldest text and actual image pixels survive reconstruction.
    assert!(actual.row_text(0).contains("OLDEST-TEXT-MUST-SURVIVE"));
    for row in 0..24 {
        assert_eq!(actual.row_text(row), expected.row_text(row), "row {row}");
    }
    assert_eq!(actual.images.len(), 1);
    let mut renderer = NativeTerminalRenderer::new(RendererConfig {
        cell_width_px: 8,
        cell_height_px: 16,
        ..Default::default()
    }).expect("GPU renderer");
    let frame = renderer.render_snapshot(&actual, None).expect("render restored image");
    let offset = ((32 * frame.width_px + 8) * 4) as usize;
    assert_eq!(&frame.pixels[offset..offset + 4], &[255, 0, 0, 255]);
    if let Some(directory) = std::env::var_os("FERRYX_IMAGE_EVIDENCE_DIR") {
        frame.save_png(std::path::PathBuf::from(directory).join("history-recovered.png"))
            .expect("save recovery evidence");
    }
}

#[test]
fn reconnect_inside_discarded_osc_consumes_live_payload_without_visible_base64() {
    let hub = TerminalOutputHub::new(64);
    drop(hub.register_session("oversized-osc"));
    hub.publish("oversized-osc", b"BEFORE\r\n\x1b]1337;File=inline=1:".to_vec()).unwrap();
    hub.publish("oversized-osc", vec![b'Q'; 4096]).unwrap();
    let snapshot = hub.subscribe_with_sequence("oversized-osc", None).unwrap().snapshot;
    let mut restored = NativeTerminal::new(80, 24).unwrap();
    restored.feed(&snapshot.history).unwrap();
    restored.feed(b"QUJDQUJDQUJD\x07AFTER\r\n").unwrap();
    let actual = restored.render_snapshot().unwrap();
    let text: String = (0..24).map(|row| actual.row_text(row)).collect();
    assert!(text.contains("BEFORE"));
    assert!(text.contains("AFTER"));
    assert!(!text.contains("QUJD"), "payload leaked into text: {text}");
}
