Generated using FFmpeg libx265, testsrc2=size=32x32:rate=30, 16 frames, preset ultrafast.
Parameters: pools=1:frame-threads=1:bframes=0:ref=1:keyint=8:min-keyint=8:scenecut=0:open-gop=0:repeat-headers=1:aud=1.
Each line in hevc-continuity.hex is one access unit, NAL units prefixed with four-byte big-endian lengths, hex encoded. Non-VCL SEI encoder metadata was removed. Frames 0 and 8 contain real IDR slices; all others are dependent pictures. Tests need no FFmpeg executable, only the production decoder library.
