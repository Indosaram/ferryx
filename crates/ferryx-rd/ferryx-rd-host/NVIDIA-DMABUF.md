# PERF-LINUX-HOST-6 implementation / validation contract

The render node selected by MAHO_VAAPI_DEVICE (default /dev/dri/renderD128)
selects the import backend through its sysfs driver symlink: nvidia uses CUDA,
all other drivers keep the existing VAAPI pipeline. The NVENC codec is opened
with AV_PIX_FMT_CUDA and an NV12 CUDA frames context, including on bitrate
reopen. Unsupported interop, absent shaders/filters or a failed first real
capture import abort initialization and invoke the existing CPU capture/upload
fallback. No wire changes or new capability bits are needed.

## Why Vulkan rather than direct DRM -> CUDA

FFmpeg 7.1 source:
- https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavutil/hwcontext_cuda.c
  cuda_device_derive accepts Vulkan only, not DRM; the CUDA context has no
  map_to/map_from implementation.
- https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavutil/hwcontext_vulkan.c
  vulkan_device_derive selects a Vulkan adapter from the DRM device;
  vulkan_map_from_drm imports the descriptor; vulkan_transfer_data_to_cuda
  imports external memory/semaphores and uses cuMemcpy2DAsync device-to-device.
  CUDA cannot import multiplane Vulkan images, so disable_multiplane=1 is set.
- https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavfilter/vf_scale_vulkan.c
  supports RGB -> NV12 conversion, scaling and full-range output.
- https://github.com/FFmpeg/FFmpeg/blob/n7.1/libavutil/hwcontext_cuda.h
  defines the public AVCUDADeviceContext prefix used to synchronize the stream.

This is a GPU-only path, NOT literal zero-copy: Vulkan processing and the CUDA
transfer perform device-side work. It removes CPU readback/conversion/upload.
CUDA stream completion is explicitly awaited before capture-slot reuse and
NVENC submission. Capture descriptors carry their real GBM modifier and current
source dimensions; filter contexts rebuild for changed geometry/orientation.

NVIDIA live validation is required: no NVIDIA GPU is available on the assigned
AMD Omarchy machine or this macOS workstation. Compile and deterministic unit
tests cannot prove modifier support, color correctness, scaling/orientation,
NVENC interop, or loaded-system lifetime behavior on NVIDIA hardware. Do not
mark this row fully done based on these gates. No GPU stress is authorized on
Omarchy.
