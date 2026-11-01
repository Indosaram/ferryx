use std::ffi::{c_void, CString};

use ferryx_rd_mobile::{
    ferryx_rd_mobile_create, ferryx_rd_mobile_destroy, ferryx_rd_mobile_send_touch, AndroidAudioTrackPlayer,
    AndroidMediaCodecConfig, AndroidMediaCodecDecoder, IosAudioEnginePlayer, IosVideoToolboxConfig,
    IosVideoToolboxDecoder, FerryxRdMobileStatus, PowerBudgetConfig, PowerPolicyManager, ThermalState,
    TouchGestureHandler, TouchMode, TouchPhase, TouchPoint, ViewportState,
};
use ferryx_rd_proto::InputEventType;

#[test]
fn unconnected_handle_send_touch_must_not_return_ok() {
    // Given: A valid host string and default streaming TCP port 19730.
    let host = CString::new("127.0.0.1").unwrap();
    let mut handle: *mut c_void = std::ptr::null_mut();

    unsafe {
        let create_status = ferryx_rd_mobile_create(host.as_ptr(), 19730, &mut handle);
        assert_eq!(create_status, FerryxRdMobileStatus::Ok);
        assert!(!handle.is_null());

        // When: Touch input is sent through the unconnected handle.
        let touch_status =
            ferryx_rd_mobile_send_touch(handle, InputEventType::LeftMouseDown as u8, 0.5, 0.5);

        // Then: The allocated handle is destroyed and touch must not return Ok.
        let destroy_status = ferryx_rd_mobile_destroy(handle);
        assert_eq!(destroy_status, FerryxRdMobileStatus::Ok);
        assert_ne!(touch_status, FerryxRdMobileStatus::Ok);
    }
}

#[test]
fn create_with_empty_host_must_return_error() {
    // Given: An empty host C-string and null output slot.
    let empty_host = CString::new("").unwrap();
    let mut handle: *mut c_void = std::ptr::null_mut();

    // When: Creating a mobile session with an empty host.
    let status = unsafe { ferryx_rd_mobile_create(empty_host.as_ptr(), 19730, &mut handle) };

    // Then: Creation must fail.
    if status == FerryxRdMobileStatus::Ok && !handle.is_null() {
        unsafe {
            let _ = ferryx_rd_mobile_destroy(handle);
        }
    }
    assert_ne!(status, FerryxRdMobileStatus::Ok);
}

#[test]
fn create_failure_with_sentinel_must_reset_out_handle_to_null() {
    // Given: An empty host C-string and an out_handle initialized to a dangling sentinel.
    let empty_host = CString::new("").unwrap();
    let sentinel: *mut c_void = 0xdeadbeef as *mut c_void;
    let mut out_handle: *mut c_void = sentinel;

    // When: Attempting creation with an empty host.
    let status = unsafe { ferryx_rd_mobile_create(empty_host.as_ptr(), 19730, &mut out_handle) };

    // Then: Creation fails and the slot is reset to null.
    if status == FerryxRdMobileStatus::Ok && out_handle != sentinel && !out_handle.is_null() {
        unsafe {
            let _ = ferryx_rd_mobile_destroy(out_handle);
        }
    }
    assert_ne!(status, FerryxRdMobileStatus::Ok);
    assert!(out_handle.is_null());
}

#[test]
fn android_mediacodec_missing_native_backend_must_not_report_decoded() {
    // Given: AndroidMediaCodecDecoder constructed with attached surface.
    let config = AndroidMediaCodecConfig {
        surface_attached: true,
        ..Default::default()
    };
    let mut decoder = AndroidMediaCodecDecoder::new(config)
        .expect("MediaCodec stub construction must succeed without a native backend");

    // When: Nonempty NAL payload is provided.
    let dummy_nal = [0x00, 0x00, 0x00, 0x01, 0x40, 0x01];
    let res = decoder.decode_access_unit(&dummy_nal);

    // Then: The missing native backend is reported exactly and nothing is decoded.
    assert_eq!(
        res,
        Err(ferryx_rd_mobile::android::AndroidMediaError::BackendUnavailable)
    );
    assert_eq!(decoder.frames_decoded(), 0);
}

#[test]
fn android_audiotrack_missing_native_backend_must_not_report_samples_written() {
    // Given: AndroidAudioTrackPlayer initialized for stereo 48kHz.
    let mut player = AndroidAudioTrackPlayer::new(48_000, 2)
        .expect("AudioTrack stub construction must succeed without a native backend");

    // When: Nonempty PCM buffer is written.
    let pcm = [0.1f32, -0.1f32, 0.2f32, -0.2f32];
    let res = player.write_pcm(&pcm);

    // Then: The missing native backend is reported exactly and no samples are counted.
    assert_eq!(
        res,
        Err(ferryx_rd_mobile::android::AndroidMediaError::BackendUnavailable)
    );
    assert_eq!(player.samples_written, 0);
}

#[test]
fn ios_videotoolbox_missing_native_backend_must_not_report_rendered() {
    // Given: IosVideoToolboxDecoder constructed with attached Metal layer.
    let config = IosVideoToolboxConfig {
        metal_layer_attached: true,
        ..Default::default()
    };
    let mut decoder = IosVideoToolboxDecoder::new(config)
        .expect("VideoToolbox stub construction must succeed without a native backend");

    // When: Nonempty HEVC frame is submitted.
    let dummy_hevc = [0x00, 0x00, 0x00, 0x01, 0x26, 0x01];
    let res = decoder.render_frame(&dummy_hevc);

    // Then: The missing native backend is reported exactly and no frame is counted.
    assert_eq!(
        res,
        Err(ferryx_rd_mobile::ios::IosMediaError::BackendUnavailable)
    );
    assert_eq!(decoder.frames_rendered(), 0);
}

#[test]
fn ios_audioengine_missing_native_backend_must_not_report_samples_rendered() {
    // Given: Started IosAudioEnginePlayer.
    let mut engine = IosAudioEnginePlayer::default();
    let start_res = engine.start();
    assert_eq!(
        start_res,
        Err(ferryx_rd_mobile::ios::IosMediaError::BackendUnavailable)
    );
    assert!(!engine.is_running);

    // When: Nonempty PCM buffer is rendered.
    let samples = [0.1f32, -0.1f32, 0.2f32, -0.2f32];
    let rendered = engine.render_pcm(&samples);

    // Then: Rendered samples and frames played must remain 0.
    assert_eq!(rendered, 0);
    assert_eq!(engine.frames_played, 0);
}

#[test]
fn direct_touch_unknown_ended_must_not_emit_up() {
    // Given: Handler with no active touches.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

    // When: Ended is received for an unknown touch ID.
    let res = handler
        .process_touch(TouchPoint {
            id: 999,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();

    // Then: No event must be emitted.
    assert!(res.is_none());
}

#[test]
fn direct_touch_unknown_repeated_ended_must_not_emit_up() {
    // Given: Handler with no active touches.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

    // When: Ended is received repeatedly for an unknown touch ID.
    let first = handler
        .process_touch(TouchPoint {
            id: 888,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();
    let second = handler
        .process_touch(TouchPoint {
            id: 888,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();

    // Then: Neither invocation emits an event.
    assert!(first.is_none());
    assert!(second.is_none());
}

#[test]
fn direct_touch_secondary_began_must_not_emit_mouse_down() {
    // Given: Primary touch 1 began.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    let primary = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    assert_eq!(
        primary.map(|e| e.event_type),
        Some(InputEventType::LeftMouseDown)
    );

    // When: Secondary touch 2 began while touch 1 is active.
    let secondary = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // Then: Secondary touch must not emit a second LeftMouseDown.
    assert_ne!(
        secondary.map(|e| e.event_type),
        Some(InputEventType::LeftMouseDown)
    );
}

#[test]
fn direct_touch_secondary_ended_must_not_emit_mouse_up() {
    // Given: Primary touch 1 began and secondary touch 2 began.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Secondary touch 2 ends while primary touch 1 is active.
    let secondary_up = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();

    // Then: Secondary touch ending must not emit LeftMouseUp.
    assert_ne!(
        secondary_up.map(|e| e.event_type),
        Some(InputEventType::LeftMouseUp)
    );
}

#[test]
fn direct_touch_primary_ended_emits_single_mouse_up() {
    // Given: Primary touch 1 and secondary touch 2 active, then secondary ends.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    let _ = handler.process_touch(TouchPoint {
        id: 2,
        x: 600.0,
        y: 300.0,
        phase: TouchPhase::Ended,
    });

    // When: Primary touch 1 ends.
    let primary_up = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();

    // Then: Exactly one LeftMouseUp is emitted, and a duplicate ended does not emit again.
    assert_eq!(
        primary_up.map(|e| e.event_type),
        Some(InputEventType::LeftMouseUp)
    );
    let repeated = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Ended,
        })
        .unwrap();
    assert!(repeated.is_none());
}

#[test]
fn trackpad_relative_nan_input_must_be_rejected_with_err() {
    // Given: Touch began at valid coordinates in TrackpadRelative mode.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 100.0,
            y: 100.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Moved event arrives with NaN coordinates.
    let moved_nan = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::NAN,
        y: 100.0,
        phase: TouchPhase::Moved,
    });

    // Then: NaN touch input must be explicitly rejected with Err.
    assert!(moved_nan.is_err());
}

#[test]
fn trackpad_relative_infinity_input_must_be_rejected_with_err() {
    // Given: Touch began at valid coordinates in TrackpadRelative mode.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 100.0,
            y: 100.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Moved event arrives with Infinity coordinates.
    let moved_inf = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::INFINITY,
        y: 100.0,
        phase: TouchPhase::Moved,
    });

    // Then: Infinity touch input must be explicitly rejected with Err.
    assert!(moved_inf.is_err());
}

#[test]
fn trackpad_relative_delta_recovers_after_rejected_invalid_input() {
    // Given: Touch began at (100.0, 100.0), followed by rejected NaN moved event.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 100.0,
            y: 100.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    let _ = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::NAN,
        y: 100.0,
        phase: TouchPhase::Moved,
    });

    // When: Subsequent valid Moved event arrives at (120.0, 110.0).
    let next_valid = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 120.0,
            y: 110.0,
            phase: TouchPhase::Moved,
        })
        .unwrap();

    // Then: Relative delta must be computed against last valid position (100.0, 100.0).
    let evt = next_valid.expect("subsequent valid Moved must produce event");
    assert_eq!(evt.scroll_dx, 20.0);
    assert_eq!(evt.scroll_dy, 10.0);
}

#[test]
fn direct_touch_invalid_began_must_be_rejected_and_preserve_state() {
    // Given: DirectTouch handler.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

    // When: Began arrives with NaN coordinates.
    let began_nan = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::NAN,
        y: 300.0,
        phase: TouchPhase::Began,
    });

    // Then: Began with NaN must return Err, and subsequent valid Began must work cleanly.
    assert!(began_nan.is_err());
    let valid_began = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap()
        .expect("valid Began must emit event");
    assert_eq!(valid_began.event_type, InputEventType::LeftMouseDown);
    assert_eq!((valid_began.x, valid_began.y), (0.5, 0.5));
}

#[test]
fn direct_touch_cancel_with_nan_coordinates_releases_last_valid_position() {
    // Given: Direct touch begins at (400.0, 300.0) on 800x600 viewport.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    let down = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap()
        .expect("Began must emit LeftMouseDown");
    assert_eq!(down.event_type, InputEventType::LeftMouseDown);

    // When: Platform delivers Cancelled with NaN coordinates.
    let cancel = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::NAN,
        y: f32::NAN,
        phase: TouchPhase::Cancelled,
    });

    // Then: Must succeed and emit LeftMouseUp with last valid normalized coordinates.
    let up = cancel
        .expect("Cancelled must not fail with Err")
        .expect("Cancelled must emit release event");
    assert_eq!(up.event_type, InputEventType::LeftMouseUp);
    assert_eq!((up.x, up.y), (0.5, 0.5));
}

#[test]
fn direct_touch_cancel_with_infinity_coordinates_releases_last_valid_position() {
    // Given: Direct touch begins at (400.0, 300.0) on 800x600 viewport.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Platform delivers Cancelled with Infinity coordinates.
    let cancel = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::INFINITY,
        y: f32::INFINITY,
        phase: TouchPhase::Cancelled,
    });

    // Then: Must succeed and emit LeftMouseUp with last valid normalized coordinates.
    let up = cancel
        .expect("Cancelled must not fail with Err")
        .expect("Cancelled must emit release event");
    assert_eq!(up.event_type, InputEventType::LeftMouseUp);
    assert_eq!((up.x, up.y), (0.5, 0.5));
}

#[test]
fn viewport_set_zoom_nan_center_must_fail_and_preserve_state() {
    // Given: Viewport with zoom 1.0 and zero offsets.
    let mut vp = ViewportState::new(1280.0, 800.0).unwrap();

    // When: set_zoom is called with NaN center.
    let res = vp.set_zoom(2.0, f32::NAN, 400.0);

    // Then: Must return Err, and state must not change.
    assert!(res.is_err());
    assert_eq!(vp.zoom, 1.0);
    assert_eq!(vp.offset_x, 0.0);
    assert_eq!(vp.offset_y, 0.0);
}

#[test]
fn viewport_set_zoom_infinity_center_must_fail_and_preserve_state() {
    // Given: Viewport with zoom 1.0 and zero offsets.
    let mut vp = ViewportState::new(1280.0, 800.0).unwrap();

    // When: set_zoom is called with Infinity center.
    let res = vp.set_zoom(2.0, f32::INFINITY, 400.0);

    // Then: Must return Err, and state must not change.
    assert!(res.is_err());
    assert_eq!(vp.zoom, 1.0);
    assert_eq!(vp.offset_x, 0.0);
    assert_eq!(vp.offset_y, 0.0);
}

#[test]
fn viewport_transform_to_host_rejects_invalid_dimensions_zoom_offsets_table() {
    // Given: Table of concrete ViewportState values and inputs that must be rejected.
    struct InvalidViewportCase {
        name: &'static str,
        viewport: ViewportState,
        input_x: f32,
        input_y: f32,
    }

    let cases = [
        InvalidViewportCase {
            name: "nan view_width",
            viewport: ViewportState {
                view_width: f32::NAN,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "infinite view_width",
            viewport: ViewportState {
                view_width: f32::INFINITY,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "zero view_width",
            viewport: ViewportState {
                view_width: 0.0,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "negative view_width",
            viewport: ViewportState {
                view_width: -1000.0,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "nan view_height",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: f32::NAN,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "infinite view_height",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: f32::INFINITY,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "zero view_height",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 0.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "negative view_height",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: -500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "zero zoom",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "negative zoom",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: -1.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "nan zoom",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: f32::NAN,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "nan offset_x",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: f32::NAN,
                offset_y: 0.0,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "infinite offset_y",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: 0.0,
                offset_y: f32::INFINITY,
            },
            input_x: 500.0,
            input_y: 250.0,
        },
        InvalidViewportCase {
            name: "arithmetic subtraction overflow",
            viewport: ViewportState {
                view_width: 1000.0,
                view_height: 500.0,
                zoom: 1.0,
                offset_x: -f32::MAX,
                offset_y: 0.0,
            },
            input_x: f32::MAX,
            input_y: 250.0,
        },
    ];

    // When & Then: Each concrete invalid configuration must produce an error.
    for case in cases {
        let res = case.viewport.transform_to_host(case.input_x, case.input_y);
        assert!(res.is_err(), "case '{}' should return Err", case.name);
    }
}

#[test]
fn thermal_fair_must_not_raise_low_default_cap() {
    // Given: Power policy with default cap 800 kbps (below 1,000 kbps).
    let config = PowerBudgetConfig {
        default_bitrate_kbps: 800,
        default_fps: 30,
        battery_saver_bitrate_kbps: 400,
        battery_saver_fps: 15,
        thermal_throttle_bitrate_kbps: 300,
        thermal_throttle_fps: 15,
    };
    let mut manager = PowerPolicyManager::new(config);

    // When: Thermal state transitions to Fair.
    manager.set_thermal_state(ThermalState::Fair);

    // Then: Throttling must not raise bitrate above the configured 800 kbps cap.
    let (bitrate, _fps) = manager.target_limits();
    assert!(bitrate <= 800);
}

#[test]
fn thermal_fair_must_not_raise_low_battery_saver_cap() {
    // Given: Power policy with active battery saver cap 500 kbps.
    let config = PowerBudgetConfig {
        default_bitrate_kbps: 15_000,
        default_fps: 60,
        battery_saver_bitrate_kbps: 500,
        battery_saver_fps: 30,
        thermal_throttle_bitrate_kbps: 2_500,
        thermal_throttle_fps: 30,
    };
    let mut manager = PowerPolicyManager::new(config);
    manager.set_battery_saver(true);

    // When: Thermal state transitions to Fair.
    manager.set_thermal_state(ThermalState::Fair);

    // Then: Throttling must not raise bitrate above active battery saver cap (500 kbps).
    let (bitrate, _fps) = manager.target_limits();
    assert!(bitrate <= 500);
}

#[test]
fn thermal_fair_at_u32_max_produces_expected_scaled_bitrate() {
    // Given: Power policy with default bitrate set to u32::MAX.
    let config = PowerBudgetConfig {
        default_bitrate_kbps: u32::MAX,
        default_fps: 60,
        battery_saver_bitrate_kbps: u32::MAX,
        battery_saver_fps: 60,
        thermal_throttle_bitrate_kbps: u32::MAX,
        thermal_throttle_fps: 60,
    };
    let mut manager = PowerPolicyManager::new(config);

    // When: Computing limits under Fair thermal state.
    manager.set_thermal_state(ThermalState::Fair);
    let (bitrate, _fps) = manager.target_limits();

    // Then: Bitrate must match 80% of u32::MAX without overflow panic.
    assert_eq!(bitrate, 3_435_973_836);
}

#[test]
fn direct_touch_mode_change_while_dragging_releases_button() {
    // Given: DirectTouch gesture handler with an active held drag.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    let down = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap()
        .expect("Began must emit LeftMouseDown");
    assert_eq!(down.event_type, InputEventType::LeftMouseDown);

    // When: Switching to TrackpadRelative mode during the held drag.
    let release_evt = handler.set_mode(TouchMode::TrackpadRelative);

    // Then: Exactly one LeftMouseUp event is returned to release the pointer on the host.
    let release = release_evt.expect("mode change during drag must return release event");
    assert_eq!(release.event_type, InputEventType::LeftMouseUp);
    assert_eq!((release.x, release.y), (0.5, 0.5));
}

#[test]
fn direct_touch_mode_change_to_same_mode_preserves_gesture() {
    // Given: DirectTouch gesture handler with an active held drag.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Calling set_mode with the same mode.
    let release_evt = handler.set_mode(TouchMode::DirectTouch);

    // Then: No release event is returned and the gesture is preserved.
    assert!(release_evt.is_none());

    // When: Touch 1 moves.
    let move_evt = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Moved,
        })
        .unwrap()
        .expect("drag must continue after same-mode set_mode");

    // Then: LeftMouseDragged is emitted as normal.
    assert_eq!(move_evt.event_type, InputEventType::LeftMouseDragged);
    assert_eq!((move_evt.x, move_evt.y), (0.75, 0.5));
}

#[test]
fn direct_touch_secondary_cancel_preserves_primary_touch() {
    // Given: Primary touch 1 begins and secondary touch 2 begins.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    let down = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap()
        .expect("primary Began emits LeftMouseDown");
    assert_eq!(down.event_type, InputEventType::LeftMouseDown);

    let sec_began = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    assert!(sec_began.is_none());

    // When: Secondary touch 2 cancels.
    let sec_cancel = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 600.0,
            y: 300.0,
            phase: TouchPhase::Cancelled,
        })
        .unwrap();

    // Then: Secondary cancellation must emit no event and must not cancel the primary touch.
    assert!(sec_cancel.is_none());

    // When: Primary touch 1 moves.
    let drag = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 500.0,
            y: 300.0,
            phase: TouchPhase::Moved,
        })
        .unwrap()
        .expect("primary drag must continue after secondary cancel");

    // Then: LeftMouseDragged is emitted for primary touch.
    assert_eq!(drag.event_type, InputEventType::LeftMouseDragged);
    assert_eq!((drag.x, drag.y), (0.625, 0.5));
}

#[test]
fn trackpad_relative_secondary_touch_does_not_steal_primary_baseline() {
    // Given: Primary touch 1 begins at (100.0, 100.0).
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 100.0,
            y: 100.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Secondary touch 2 begins at (300.0, 300.0).
    let sec_began = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 300.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    assert!(sec_began.is_none());

    // When: Secondary touch 2 moves.
    let sec_move = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 350.0,
            y: 350.0,
            phase: TouchPhase::Moved,
        })
        .unwrap();
    // Then: Secondary touch does not emit relative move.
    assert!(sec_move.is_none());

    // When: Primary touch 1 moves to (115.0, 120.0).
    let pri_move = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 115.0,
            y: 120.0,
            phase: TouchPhase::Moved,
        })
        .unwrap()
        .expect("primary touch must emit relative move");

    // Then: Delta is computed from primary touch baseline (100.0, 100.0).
    assert_eq!(pri_move.event_type, InputEventType::RelativeMove);
    assert_eq!(pri_move.scroll_dx, 15.0);
    assert_eq!(pri_move.scroll_dy, 20.0);
}

#[test]
fn trackpad_relative_cancel_with_nan_clears_baseline_and_allows_new_owner() {
    // Given: Primary touch 1 begins at (100.0, 100.0).
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: 100.0,
            y: 100.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Primary touch 1 cancels with non-finite platform coordinates.
    let cancel_res = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::NAN,
        y: f32::NAN,
        phase: TouchPhase::Cancelled,
    });

    // Then: Cancellation must succeed and clear the baseline owner.
    assert!(cancel_res.is_ok());

    // When: A new primary touch 2 begins at (200.0, 200.0).
    let sec_began = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 200.0,
            y: 200.0,
            phase: TouchPhase::Began,
        })
        .unwrap();
    assert!(sec_began.is_none());

    // When: Touch 2 moves to (225.0, 210.0).
    let sec_move = handler
        .process_touch(TouchPoint {
            id: 2,
            x: 225.0,
            y: 210.0,
            phase: TouchPhase::Moved,
        })
        .unwrap()
        .expect("new owner must emit relative move after cancel");

    // Then: Relative move delta is computed against touch 2 baseline (200.0, 200.0).
    assert_eq!(sec_move.event_type, InputEventType::RelativeMove);
    assert_eq!(sec_move.scroll_dx, 25.0);
    assert_eq!(sec_move.scroll_dy, 10.0);
}

#[test]
fn direct_touch_cancel_after_viewport_mutated_to_nan_releases_last_valid_coordinates() {
    // Given: DirectTouch handler where primary touch began at (400.0, 300.0), emitting LeftMouseDown at (0.5, 0.5).
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
    let down = handler
        .process_touch(TouchPoint {
            id: 1,
            x: 400.0,
            y: 300.0,
            phase: TouchPhase::Began,
        })
        .unwrap()
        .expect("Began must emit LeftMouseDown");
    assert_eq!(down.event_type, InputEventType::LeftMouseDown);
    assert_eq!((down.x, down.y), (0.5, 0.5));

    // When: Mutable viewport state is corrupted with NaN dimensions and offsets.
    handler.viewport_mut().view_width = f32::NAN;
    handler.viewport_mut().offset_x = f32::NAN;

    // When: Platform delivers Cancelled for the held direct touch.
    let cancel = handler.process_touch(TouchPoint {
        id: 1,
        x: 400.0,
        y: 300.0,
        phase: TouchPhase::Cancelled,
    });

    // Then: Must succeed without recalculating from corrupted viewport, emitting LeftMouseUp with last valid coordinates.
    let up = cancel
        .expect("Cancelled must succeed even with mutated viewport")
        .expect("Cancelled must emit LeftMouseUp release event");
    assert_eq!(up.event_type, InputEventType::LeftMouseUp);
    assert_eq!((up.x, up.y), (0.5, 0.5));
}

#[test]
fn viewport_pan_finite_arithmetic_overflow_rejected() {
    // Given: Viewport with large finite offset_x.
    let mut vp = ViewportState::new(1000.0, 500.0).unwrap();
    vp.offset_x = f32::MAX;

    // When: Panning by f32::MAX which causes finite addition overflow (producing Infinity).
    vp.pan(f32::MAX, 0.0);

    // Then: Pan must reject overflow and preserve the previous finite offset.
    assert!(
        vp.offset_x.is_finite(),
        "pan must not mutate offset to infinity on overflow"
    );
    assert_eq!(vp.offset_x, f32::MAX);
}

#[test]
fn trackpad_relative_finite_delta_arithmetic_overflow_rejected() {
    // Given: TrackpadRelative touch begins at -f32::MAX.
    let vp = ViewportState::new(800.0, 600.0).unwrap();
    let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);
    handler
        .process_touch(TouchPoint {
            id: 1,
            x: -f32::MAX,
            y: 0.0,
            phase: TouchPhase::Began,
        })
        .unwrap();

    // When: Move arrives at f32::MAX, causing finite subtraction overflow (dx = f32::MAX - (-f32::MAX) -> Infinity).
    let move_overflow = handler.process_touch(TouchPoint {
        id: 1,
        x: f32::MAX,
        y: 0.0,
        phase: TouchPhase::Moved,
    });

    // Then: Finite delta arithmetic overflow must be rejected with Err.
    assert!(
        move_overflow.is_err(),
        "finite arithmetic overflow on relative delta must return Err"
    );
}
