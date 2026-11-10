#!/bin/sh
# Build, ad-hoc sign (dx 0.7.10 leaves the Swift framework unsigned), install and launch on a booted simulator.
# Usage: ./sim-ios.sh [autoplay-url]
set -e
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer PATH=$HOME/.cargo/bin:$PATH
cd "$(dirname "$0")"
MEDIA_BASE=${MEDIA_BASE:-http://localhost:8765} dx build --platform ios --features mobile
APP=target/dx/player-proto/debug/ios/PlayerProto.app
for f in "$APP"/Frameworks/*; do codesign --force --sign - "$f"; done
codesign --force --sign - "$APP"
xcrun simctl install booted "$APP"
xcrun simctl terminate booted dev.reelhouse.proto 2>/dev/null || true
SIMCTL_CHILD_PROTO_AUTOPLAY="$1" xcrun simctl launch booted dev.reelhouse.proto
