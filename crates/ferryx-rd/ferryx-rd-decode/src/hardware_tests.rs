use super::*;

fn fixture() -> Vec<u8> {
    include_str!("../tests/fixtures/black_16x16.hevc.hex")
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

#[test]
fn explicit_software_decode_reports_actual_backend() {
    let data = fixture();
    let mut decoder = HevcDecoder::new_software(&data).unwrap();
    let mut frames = decoder.decode(&data, 7).unwrap();
    frames.extend(decoder.flush().unwrap());
    assert_eq!(decoder.acceleration(), HardwareAcceleration::Software);
    assert_eq!(frames.len(), 1);
    assert_nv12(&frames[0], 16, 16);
}

#[test]
fn default_decoder_downloads_hardware_or_falls_back_to_nv12() {
    let data = fixture();
    let mut decoder = HevcDecoder::new(&data).unwrap();
    let mut frames = decoder.decode(&data, 7).unwrap();
    frames.extend(decoder.flush().unwrap());
    assert_eq!(frames.len(), 1);
    assert_nv12(&frames[0], 16, 16);
    eprintln!("actual decoded pixel backend: {:?}", decoder.acceleration());
}

#[cfg(all(target_os = "windows", feature = "d3d11va"))]
#[test]
fn d3d11va_decodes_real_hevc_without_software_fallback() {
    use ffmpeg_next::ffi;
    ffmpeg_next::init().unwrap();
    // Only device creation failure is an environmental skip. A decoder that
    // silently falls back after a successful probe must fail this proof.
    unsafe {
        let mut device = std::ptr::null_mut();
        let result = ffi::av_hwdevice_ctx_create(
            &mut device,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        );
        if result < 0 {
            eprintln!(
                "SKIP D3D11VA: device creation failed: {}",
                ffmpeg_next::Error::from(result)
            );
            return;
        }
        ffi::av_buffer_unref(&mut device);
    }
    let data = fixture();
    let mut decoder = HevcDecoder::new(&data).unwrap();
    let mut frames = decoder.decode(&data, 7).unwrap();
    frames.extend(decoder.flush().unwrap());
    assert_eq!(frames.len(), 1);
    assert_eq!(decoder.acceleration(), HardwareAcceleration::D3d11va);
    assert_nv12(&frames[0], 16, 16);
}

fn assert_nv12(frame: &Nv12Frame, width: u32, height: u32) {
    assert_eq!((frame.width, frame.height), (width, height));
    assert_eq!(frame.y_plane.len(), (width * height) as usize);
    assert_eq!(frame.uv_plane.len(), (width * height / 2) as usize);
    assert_eq!(frame.y_stride, width as usize);
    assert_eq!(frame.uv_stride, width as usize);
}

#[cfg(all(target_os = "macos", feature = "videotoolbox"))]
#[test]
fn videotoolbox_decodes_real_hevc_to_nv12() {
    use ffmpeg_next::{codec, encoder, format::Pixel, frame, Dictionary, Packet};
    ffmpeg_next::init().unwrap();
    let codec = encoder::find_by_name("hevc_videotoolbox").unwrap();
    let mut encoder = codec::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap();
    encoder.set_width(128);
    encoder.set_height(128);
    encoder.set_format(Pixel::NV12);
    encoder.set_time_base((1, 30));
    encoder.set_frame_rate(Some((30, 1)));
    encoder.set_gop(1);
    encoder.set_max_b_frames(0);
    let mut options = Dictionary::new();
    options.set("realtime", "1");
    options.set("allow_sw", "0");
    let mut encoder = encoder.open_with(options).unwrap();
    let mut frame = frame::Video::new(Pixel::NV12, 128, 128);
    frame.data_mut(0).fill(16);
    frame.data_mut(1).fill(128);
    frame.set_pts(Some(0));
    encoder.send_frame(&frame).unwrap();
    encoder.send_eof().unwrap();
    let mut packet = Packet::empty();
    encoder.receive_packet(&mut packet).unwrap();
    // VideoToolbox's raw HEVC packets use Annex-B; the media API uses AVCC.
    let bytes = packet.data().unwrap();
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let length = if bytes[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        starts.push((i, i + length));
        i += length;
    }
    let mut data = Vec::new();
    for (index, &(_, start)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map_or(bytes.len(), |&(end, _)| end);
        data.extend_from_slice(&((end - start) as u32).to_be_bytes());
        data.extend_from_slice(&bytes[start..end]);
    }
    let mut decoder = HevcDecoder::from_keyframe(&data).unwrap();
    let mut frames = decoder.decode(&data, 0).unwrap();
    frames.extend(decoder.flush().unwrap());
    assert_eq!(frames.len(), 1);
    assert_eq!(decoder.acceleration(), HardwareAcceleration::VideoToolbox);
    assert_nv12(&frames[0], 128, 128);
}
