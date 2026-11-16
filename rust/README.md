# Canvas Studio Rust 0.5.0

The library screen, floating editor toolbars, style panel, menus, dialogs, text/icon drawing and TV remote focus are rendered by Rust. Their layout follows the Flutter app: menu/undo at top left, styles at top right, zoom at bottom left, tools at bottom centre. There are no Kotlin screen or toolbar implementations. A small Android host supplies the Surface, raw input, IME, complex-script text rasterisation, ML Kit and system installation APIs.

## Included

- Pen, selection/move/resize, marquee selection, eraser, arrows, rectangles and ellipses.
- Automatic shape tidying and handwriting-to-typed-text conversion, with separate persisted settings. English and Hindi. No keyboard toolbar button; existing text can be edited by double-tapping or selecting it.
- Five board fonts, colour, width and fill controls.
- Palm rejection, bounded multi-touch forwarding (five contacts), historical/unbuffered input.
- 1920-pixel maximum drawing-buffer side by default for 4K panels. Full-resolution mode is optional. Only dirty screen rectangles are copied while writing.
- Atomic saved board files, rename/delete, thumbnails, undo/redo, zoom/fit. Leaving a dirty board waits for the write; a failed write keeps the board open. Backgrounding preserves a stroke already in progress.
- GitHub OTA checks for `rust-v*` releases, with package/version/signing-certificate validation and Android confirmation before installation.
- Android API 24+, ARM 32-bit, ARM64, x86 and x86_64 in the same APK.

The package is `com.canvasstudio.rust`, separate from the Flutter app. Earlier Rust boards remain in the same app-private `files/boards` directory. Flutter boards are not automatically imported. Collaboration/Claude MCP and PNG sharing currently remain in the Flutter implementation; they have not been migrated into this Rust UI release.

## Build

Build host: `ssh addypy` (LAN) or `ssh addypy-ts` (Tailscale).
Source checkout: `/home/ubuntu/canvas-rust`.

```sh
cargo test -p canvas_core
bash app-android/build.sh
```

The script cross-compiles four Android ABIs and signs/packages `out/canvas-studio-rust.apk`. Keep the existing release keystore and `app-android/key.properties` private; neither belongs in git. Future Rust updates must use that key and an increased Android version code.

## Validation

48 Rust tests pass, including font clipping, UI navigation, saved-board reopen, backgrounding during a stroke, and disk-write failure recovery. The signed release APK was installed and launched on a 1920×1080 Android TV emulator; the library, editor, drawing, menu and Hindi labels were visually checked. The host has no KVM acceleration. A 400 ms emulator stroke reported approximately 69 ms mean sample-to-present time; this is a software-emulator observation, not a latency measurement of the BIGVUE panel. Real-panel latency and ML Kit recognition quality still require testing on that device.

## Download

Cloudflare: https://annotated-remedies-property-binding.trycloudflare.com/canvas-studio-rust.apk

This is a temporary tunnel. GitHub release `rust-v0.5.0` provides the durable download used by the Rust updater.
