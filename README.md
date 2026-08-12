# MinnowSnap

MinnowSnap is a desktop screen capture tool built with Rust and GPUI. It focuses on fast capture, lightweight editing, and local post-processing for screenshots.

![MinnowSnap screenshot](.github/assets/screenshot.png)

## Features

- Fast screen capture
- Simple annotation
- Local OCR
- QR code detection
- Copy, save, and pin

## Build

- Default build: `cargo build -p minnow-app --release`
- Portable build: `cargo build -p minnow-app --features portable --release`

Portable builds store app-internal data next to the executable in `data/`:

- `data/config.toml`
- `data/logs/`
- `data/temp/`
- `data/ocr_models/`

### macOS app bundle

On macOS, prefer building a proper `.app` bundle rather than running the bare
binary. Capturing the screen requires the **Screen Recording** permission, and
macOS tracks that permission per app identity. When you run the loose binary
from a terminal, the permission attaches to the *terminal* instead of
MinnowSnap — so screenshots come back showing only the desktop wallpaper.

Build the bundle with [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle):

```sh
cargo install cargo-bundle
# Run from the crate directory so the bundled Info.plist extras resolve.
cd crates/minnow-app
cargo bundle --release --format osx
```

The bundle is written to `target/release/bundle/osx/MinnowSnap.app`. Launch it
once and grant Screen Recording access when prompted (or via **System Settings →
Privacy & Security → Screen & System Audio Recording**), then relaunch the app.

## TODO

- improvement long capture

## Credits

- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)
- [xcap](https://github.com/nashaofu/xcap)
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR)
- [And more...](crates/minnow-app/Cargo.toml)
