# Phase 0 player prototype (throwaway)

Proves a native video player can render under Dioxus controls drawn in a transparent webview.
Plan: `reelhouse/plan-00-player-prototype/` in the documentation repo. Never merged to `main`.

## Toolchain (macOS)

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer   # avoids sudo xcode-select
export ANDROID_HOME=$HOME/Library/Android/sdk
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android
cargo install dioxus-cli --version 0.7.10 --locked   # /opt/homebrew/bin/dx is a Deno tool; use ~/.cargo/bin/dx
brew install --cask android-commandlinetools
brew install mpv
sdkmanager --sdk_root=$ANDROID_HOME "platform-tools" "emulator" "platforms;android-35" "build-tools;35.0.0" \
  "ndk;27.2.12479018" "system-images;android-35;google_apis;arm64-v8a" "system-images;android-34;google-tv;arm64-v8a"
```

## Test media

```sh
./media/make-media.sh                       # hevc10.mkv + hevc10.mp4
npx -y http-server@14.1.1 media -p 8765 -a 0.0.0.0 --cors -s   # needs Range requests; AVPlayer refuses python http.server
```

HLS stream: https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8 (sends CORS headers; Apple's bipbop stream does not, so browsers cannot load it)
