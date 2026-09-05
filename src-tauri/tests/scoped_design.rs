pub use ferryx_lib::scoped_contracts;
#[path = "../src/ferryx_scope/design/mod.rs"]
pub mod design;
use design::*;

fn identity() -> Identity { Identity { browser_id: "b".into(), webview_label: "instance-a".into(), generation: scoped_contracts::Epoch(1), operation_id: "op".into(), viewport_revision: 1 } }
fn viewport() -> Viewport { Viewport { width: 4., height: 3., dpr: 2., zoom: 1. } }
fn fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    { let mut encoder = png::Encoder::new(&mut bytes, 8, 6); encoder.set_color(png::ColorType::Rgba); encoder.set_depth(png::BitDepth::Eight);
      let pixels: Vec<u8> = (0..6).flat_map(|y| (0..8).flat_map(move |x| [x, y, 120, 255])).collect();
      encoder.write_header().unwrap().write_image_data(&pixels).unwrap(); }
    bytes
}
#[test]
fn stale_native_callback_rejected_and_completion_consumed_once() {
    let mut fence = CaptureFence::default(); let id = identity();
    fence.arm(id.clone(), viewport()); fence.invalidate();
    assert!(matches!(fence.complete(&id, viewport()), Err(DesignError::Stale)));
    fence.arm(id.clone(), viewport());
    let mut changed = id.clone(); changed.webview_label = "instance-b".into();
    assert!(fence.complete(&changed, viewport()).is_err());
    fence.complete(&id, viewport()).unwrap();
    assert!(fence.complete(&id, viewport()).is_err());
}
#[test]
fn crop_uses_selected_native_pixel_bounds() {
    let cropped = crop_png(&fixture(), Rect { x: 1., y: 0.5, width: 2., height: 1.5 }, viewport()).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(cropped)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()]; let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (4, 3));
    assert_eq!(&pixels[..4], &[2, 1, 120, 255]);
    assert_eq!(&pixels[44..48], &[5, 3, 120, 255]);
}
#[test]
fn crop_clips_edges_and_rejects_bad_scale_or_empty() {
    let rect = Rect { x: -1., y: -1., width: 3., height: 3. };
    let bytes = crop_png(&fixture(), rect, viewport()).unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (4, 4));
    assert!(crop_png(&fixture(), Rect { width: 0., ..rect }, viewport()).is_err());
    assert!(crop_png(&fixture(), rect, Viewport { zoom: 2., ..viewport() }).is_err());
    assert!(crop_png(&fixture(), Rect { x: f64::NAN, ..rect }, viewport()).is_err());
}
#[test]
fn real_webkit_viewport_crop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("viewport.png");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/features/ferryx/design/webview-proof.mjs");
    let output = std::process::Command::new("bun").arg(script).env("DESIGN_VIEWPORT_PNG", &path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let bytes = std::fs::read(&path).unwrap();
    let original = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info().unwrap();
    let scale = f64::from(original.info().width) / 320.;
    let cropped = crop_png(&bytes, Rect { x:20., y:30., width:120., height:64. }, Viewport {width:320.,height:240.,dpr:scale,zoom:1.}).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(cropped)).read_info().unwrap();
    let mut pixels = vec![0;reader.output_buffer_size()]; let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width,info.height),((120.*scale) as u32,(64.*scale) as u32));
    // Interior, away from text/antialiasing: actual CSS blue, not overlay Highlight.
    let channels = if info.color_type == png::ColorType::Rgba {4} else {3};
    let offset = (5 * info.width as usize + 5) * channels;
    assert_eq!(&pixels[offset..offset+3], &[20,100,200]);
}
