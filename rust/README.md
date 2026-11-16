# Inkboard 0.6.0 (Rust)

The library screen, floating editor toolbars, style panel, menus, dialogs, text/icon drawing and TV remote focus are rendered by Rust. Their layout follows the Flutter app: menu/undo at top left, styles at top right, zoom at bottom left, tools at bottom centre. There are no Kotlin screen or toolbar implementations. A small Android host supplies the Surface, raw input, IME, complex-script text rasterisation, ML Kit and system installation APIs.

## Included

- Pen, handwriting-to-text pen, smart-shape pen, selection/move/resize, marquee selection, eraser, arrows, rectangles and ellipses.
- Automatic shape tidying and handwriting-to-typed-text conversion, with separate persisted settings. English and Hindi. There is no keyboard: board text comes from handwriting or from Claude.
- 36 board fonts (31 bundled open-licence families, see `assets/licenses`) with a font picker that previews each one; colour, width and fill controls.
- Export as PNG to Pictures/Inkboard.
- Share a board with a 6-digit code. Devices on the same network connect directly; other networks go through the relay (`server/relay.mjs`).
- Claude connector: the panel is a Streamable HTTP MCP server (`/mcp/<key>` on port 18763, and `<relay>/d/<id>/mcp/<key>` from anywhere). Claude can read the open board, add and update shapes, text and diagrams, delete with confirmation and get a PNG picture of the board. Suggested models: Claude Opus 5.5 for diagrams, Claude Sonnet 5.5 for quick edits.
- Palm rejection, bounded multi-touch forwarding (five contacts), historical/unbuffered input.
- 1920-pixel maximum drawing-buffer side by default for 4K panels. Full-resolution mode is optional. Only dirty screen rectangles are copied while writing.
- Atomic saved board files, rename/delete, thumbnails, undo/redo, zoom/fit. Leaving a dirty board waits for the write; a failed write keeps the board open. Backgrounding preserves a stroke already in progress.
- GitHub OTA checks for `rust-v*` releases, with package/version/signing-certificate validation and Android confirmation before installation.
- Android API 24+, ARM 32-bit, ARM64, x86 and x86_64 in the same APK.

The package is `com.canvasstudio.rust`, separate from the Flutter app. Earlier Rust boards remain in the same app-private `files/boards` directory. Flutter boards are not automatically imported. The app is shown as Inkboard; the package name is unchanged so over-the-air updates keep working.

## Relay

`server/relay.mjs` (Node 18+) holds nothing on disk. Panels long-poll `/_dev/poll` with their id and secret; requests to `/d/<id>/...` are queued for that panel, and `/find?code=` maps a share code to the panel sharing it. It listens on 127.0.0.1:18780 and is published over HTTPS (Tailscale Funnel at `https://inkboard.taila6db10.ts.net`, the address built into the app).

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

This is a temporary tunnel. The Rust updater uses GitHub releases tagged `rust-v*`. Publication of `rust-v0.5.0` is pending: GitHub returned HTTP 403 for the supplied token, so a token with Contents write access is required to finish that upload.
