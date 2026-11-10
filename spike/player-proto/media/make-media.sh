#!/bin/sh
# Generates 30 s 4K HEVC 10-bit test files: MKV for Android/desktop, MP4 (hvc1) for AVPlayer.
set -e
cd "$(dirname "$0")"
ffmpeg -y -f lavfi -i testsrc2=size=3840x2160:rate=24 -f lavfi -i sine=frequency=440 -t 30 \
  -c:v libx265 -pix_fmt yuv420p10le -tag:v hvc1 -c:a aac hevc10.mkv
ffmpeg -y -i hevc10.mkv -c copy -tag:v hvc1 hevc10.mp4
