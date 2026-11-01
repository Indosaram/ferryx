# Encoder recovery: on-demand intra refresh (PERF-TRANSPORT-4)

Status: **not implemented. Blocked.** Every host recovers from loss by forcing an IDR
(`force_key_frame`). Nothing in this repository implements on-demand intra
refresh, and no code path enables periodic intra refresh. This document records
why, so the work is not re-attempted with a no-op or a periodic substitute.

## Goal

When the receiver reports loss, the host would send a short intra-refresh wave instead of an IDR.
Each frame in the wave intra-codes one band of the picture, and the
stream is clean again after N frames. This spreads the IDR's bitrate spike
across those N frames. A refresh only qualifies if it meets both of these requirements:

1. It is **on-demand**: triggered by a specific recovery request, not running on a fixed schedule.
2. **Old receivers keep working**: any client that does not understand
   refresh frames must still get an IDR.

## Linux (`clients/rust/maho-host/src/encode_linux.rs`)

The Linux backends are FFmpeg `h264_nvenc`/`hevc_nvenc`, `h264_vaapi`/`hevc_vaapi`, and
the `libx264` fallback. All of them go through the libavcodec API (`ffmpeg-next`).
The facts below were checked against FFmpeg tag `n7.1` and nv-codec-headers
`n12.2.72.0`.

### NVENC: refresh is periodic only

- `intra-refresh` is described as "Use Periodic Intra Refresh instead of IDR frames"
  in both encoders:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc_h264.c#L210 and
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc_hevc.c#L197
- With that option set, `nvenc.c` sets `intraRefreshPeriod = gopLength` and
  `intraRefreshCnt = gopLength - 1`, then forces
  `gopLength = NVENC_INFINITE_GOPLENGTH`. This happens at encoder init for H.264, HEVC
  and AV1:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L1245-L1251,
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L1376-L1382,
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L1515-L1519
- `base_video_context` maps `keyframe_interval == 0` (infinite GOP) to
  `set_gop(i32::MAX)`, so the refresh period would be about 2^31 frames and a wave
  would never actually run. A finite period would refresh continuously, which is
  not on-demand. **Neither option is acceptable, so `intra-refresh` stays off.**
- The NVENC SDK does support on-demand refresh, through the per-frame field
  `forceIntraRefreshWithFrameCnt` in `NV_ENC_PIC_PARAMS_H264/HEVC/AV1`:
  https://github.com/FFmpeg/nv-codec-headers/blob/n12.2.72.0/include/ffnvcodec/nvEncodeAPI.h#L2282
  (also L2327 and L2374).
- **FFmpeg never sets that field.** The per-frame setup only fills in slice mode and
  SEI data:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L2291-L2335.
  The only per-frame control is `encodePicFlags`, which can be `NV_ENC_PIC_FLAG_FORCEIDR`,
  `NV_ENC_PIC_FLAG_FORCEINTRA` or 0, chosen from `AVFrame.pict_type`:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L2829-L2834
- Mid-stream reconfiguration (`reconfig_encoder`) only covers display aspect ratio and bitrate:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/nvenc.c#L2683-L2773
- `nvenc.c` never calls `nvEncInvalidateRefFrames`, and no LTR controls are exposed.

### VAAPI: no intra refresh at all

- `vaapi_encode.c`, `vaapi_encode_h264.c` and `vaapi_encode_h265.c` never send
  the libva rolling-intra-refresh misc parameter (`VAEncMiscParameterTypeRIR`).
  The only intra setting is `intra_period = intra_idr_period = gop_size`:
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/vaapi_encode_h264.c#L322-L323 and
  https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/vaapi_encode_h265.c#L347-L348
- No AVOption controls intra refresh for either VAAPI encoder.

### libx264 fallback

libx264's `intra-refresh` option is also "Use Periodic Intra Refresh instead of IDR
frames" (it maps to `b_intra_refresh`, tied to keyint):
https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/libx264.c#L1531 and
https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/libx264.c#L1217.
The only per-frame control is `pict_type`, mapped to an IDR or a keyframe:
https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavcodec/libx264.c#L519.
It has the same problem as NVENC.

## macOS (`clients/rust/maho-host/src/encode_vt.rs`)

Checked against the MacOSX 27.0 SDK headers
(`VideoToolbox.framework/Headers/VTCompressionProperties.h`). Apple documentation:
https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_enableltr and
https://developer.apple.com/documentation/videotoolbox/kvtencodeframeoptionkey_forceltrrefresh

- **VideoToolbox has no intra-refresh property or frame option.** A
  case-insensitive search for `IntraRefresh` in the VideoToolbox headers finds nothing. The only
  per-frame recovery key the host uses is `kVTEncodeFrameOptionKey_ForceKeyFrame`.
- The closest feature is long-term reference (LTR) recovery: `kVTCompressionPropertyKey_EnableLTR`
  (macOS 12+), `kVTEncodeFrameOptionKey_ForceLTRRefresh` and
  `kVTEncodeFrameOptionKey_AcknowledgedLTRTokens`, with tokens returned via
  `kVTSampleAttachmentKey_RequireLTRAcknowledgementToken`. The header says
  that a forced LTR refresh encodes a P frame from an *acknowledged* LTR, "or an
  IDR if no LTR frames have been acknowledged". This is not intra refresh. It
  needs a receiver-to-host acknowledgement channel that carries LTR tokens, and the
  protocol has no such channel today. It is also not something `encode_vt.rs`
  can do on its own.

## Missing negotiation and receiver gate

Even with an encoder trigger, sending a refresh wave in place of an IDR would
break current receivers:

1. **No wire marker.** `FrameHeader` (`clients/rust/maho-proto/src/media.rs`) is
   16 bytes: `frame_id u32`, `width u16`, `height u16`, `is_key_frame u8` at offset
   8, `total_chunks u16`, a padding `u8` at offset 11 (written as 0, ignored on
   decode), and `total_size u32`. Nothing marks a frame as "refresh wave start" or
   "refresh complete".
2. **No capability bit.** `Capabilities` (`clients/rust/maho-proto/src/handshake.rs`)
   defines bits 0-15 (`STREAM_CONFIGURATION` through `RTT_PROBE`). No bit says
   "receiver accepts intra-refresh recovery". `docs/protocol-v3.md` section 5.2
   requires unknown bits to be ignored, so a new bit is the compatible way to
   negotiate this.
3. **Receiver gate.** `FrameQueue::push` (`clients/rust/maho-app/src/frame_queue.rs`)
   sets `recovering = true` after a discontinuity or overflow. While
   `state.recovering && !key`, it drops every frame as `QueueDependent`, and only
   clears `recovering` when a frame with `is_key_frame == 1` arrives. A refresh wave
   is made of non-key frames, so a current client would throw away the entire wave. It
   would also re-request a keyframe after `idr_defer`. Test
   `upstream_gap_suppresses_dependents_until_keyframe` in
   `clients/rust/maho-app/src/bin/maho_client.rs` covers this behavior. The macOS host's
   startup gate (`awaiting_keyframe` in `clients/rust/maho-host/src/session.rs`)
   likewise only opens on `is_key_frame`.
4. **Decoder init.** The client only builds its decoder from a keyframe
   (`HevcDecoder::from_keyframe_auto`), and VPS/SPS/PPS are prepended only on
   keyframes (`docs/protocol-v3.md`).

## What unblocking requires

All of the following, as one negotiated feature:

- **Encoder trigger.** On NVENC, either call the NVENC API directly or carry an FFmpeg patch
  that sets `forceIntraRefreshWithFrameCnt` per frame, with
  `enableIntraRefresh` configured and a capability probe for
  `NV_ENC_CAPS_SUPPORT_INTRA_REFRESH`. On VAAPI, either use libva directly or carry an FFmpeg patch
  that sends `VAEncMiscParameterTypeRIR`, with a driver capability probe. VideoToolbox would need
  an LTR-token acknowledgement design instead.
- **Protocol.** A new `Capabilities` bit, plus a frame-level marker for the start
  and end of a refresh wave (for example, assigning the padding byte at offset 11 of
  `FrameHeader`), documented in `docs/protocol-v3.md`.
- **Receiver.** `FrameQueue` would leave `recovering` when the marked refresh wave completes,
  without requiring `is_key_frame`. That is valid only after the capability is
  negotiated. The receiver must keep falling back to IDR requests when the wave does not complete.
- **Host policy.** Use refresh only when both peers advertise the bit and the
  encoder probe succeeds. In every other case, `force_key_frame` must keep producing an IDR.

Until all of these exist, `force_key_frame` stays an IDR on every platform, and
encoder intra refresh stays off.
