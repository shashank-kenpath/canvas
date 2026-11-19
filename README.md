# Canvas Studio — native Android app

Team boards (live multi-device collaboration on Cloudflare, [`cloud/`](cloud/README.md)) and native macOS/Windows/Linux apps ([`rust/desktop`](rust/README.md#desktop-macos-windows-linux)) are part of the Rust app. The Rust Android port is now in [`rust/`](rust/README.md), with its library, editor controls, menus and dialogs rendered in Rust. Download for macOS, Windows, Android and Linux: **https://whiteboard.kenpathlabs.com**. The Flutter implementation below remains available alongside it.

Version 2 is a native Flutter implementation. The editor uses Flutter CustomPainter, Canvas, TextPainter and native gestures. No WebView, React runtime, JavaScript editor or tldraw SDK is packaged into this APK.

The fetched tldraw repository at `upstream/tldraw` is retained as a reference for interaction concepts. Its React renderer is not a native Flutter renderer; this is a smaller native implementation, not a full port of the tldraw SDK. The previous WebView prototype source remains in `editor/` for reference and is excluded from the native build.

## Features and saving

- Pen, rectangle, ellipse, arrow, text and eraser.
- Select and move shapes, choose colors/line weights/fill, undo/redo.
- Pan, pinch zoom, fit drawing, optional dot grid.
- Adaptive toolbars and board grids for phones and tablets.
- Named boards with thumbnails, rename and delete.
- **Save button**, autosave after edits, save on app backgrounding, and awaited save before leaving a board. Save failures appear in the editor and prevent leaving through Back until saving succeeds.
- Atomic JSON file writes to the application's private documents directory, under `native_boards`. Reopening the app restores the library and drawings. Uninstalling or clearing app data removes those files.
- Native Android PNG sharing (exports the entire drawing).
- Canvas Studio logo in the library and custom adaptive Android launcher icons.
- In-app update checks (automatic on library startup and a manual **App updates** button), download progress/cancel, SHA-256 and file-size validation, Android install permission and installer confirmation. Native code also verifies package name, newer version code, and the same signing certificate before installation.

## Writing, recognition and large displays (2.2)

- tldraw-style layout: floating tool bar at the bottom, style panel, zoom controls and a Share button.
- Writing is drawn directly from raw pointer events, with the finished board cached as an image, so a stroke costs the same on a full board as on an empty one. A resting palm is ignored while a stroke is in progress.
- **Shape recognition:** draw a rough box, circle, line or arrow and hold the pen still for about half a second; it snaps to a clean shape. Undo brings the ink back.
- **Handwriting to text:** select ink and tap *Convert handwriting to text*, or turn on automatic conversion in the menu. English and Hindi, chosen in the menu. Recognition runs on the device with Google ML Kit; each language model (about 20 MB) is downloaded once, which needs internet and Google Play services.
- Select several shapes with a drag, move, resize, restyle, delete.

## Sharing and Claude

Open a board and tap **Share → Start sharing**. The board is uploaded to the collaboration server and kept in sync (about once a second) while it is open.

- **Another device:** on the board list tap **Join shared board** and enter the 6-digit code (valid 10 minutes) or paste the invite link.
- **Claude Code:** run the command shown in the Share panel: `claude mcp add --transport http canvas-studio <MCP URL>`.
- **Claude Desktop:** add the MCP URL as a custom connector, or open `<server>/pair` on the computer, enter the board code and copy the ready-made config.

Claude gets five tools: `get_board`, `list_shapes`, `add_shapes`, `update_shape`, `delete_shapes`. Its edits appear on every device viewing the board.

Anyone holding the code, invite link or MCP URL can edit that board; there are no accounts. Edits merge per shape, last write wins, so two people changing the same shape at once keep only the later change.

The server is `server/` (Node 22, SQLite). It also serves the APK downloads and update feed. On the Ubuntu host it runs as the user service `canvas-studio-server` on port 18763 with data in `/home/ubuntu/canvas-studio/data`. Run its tests with `npm ci && node --test test/*.test.mjs`. The app's default server address is set at build time with `--dart-define=COLLAB_SERVER_URL=https://...`.

Undo history and camera position are session-only. No shape rotation, images, or tldraw-file compatibility yet.

## Build on Addypy Ubuntu

Location: `/home/ubuntu/canvas-studio`, accessed with `ssh addypy-ts` over Tailscale.

```
bash build-android.sh
```

Flutter 3.50.0 at `/home/ubuntu/tools/flutter`; Android SDK at `/home/ubuntu/Android/sdk`; JDK at `/home/ubuntu/jdk/jdk-21.0.13+9`.

The script builds a release-mode universal APK and smaller APKs by ABI. No Node build or tldraw license key is involved. Android API 24+ is required. APKs currently use the host's development signing key for direct installation; Play Store distribution needs your own upload signing configuration.

## Verification

Flutter analysis and 39 tests pass: geometry/hit testing, undo/redo, shape recognition, handwriting model handling (mocked), pen input and palm rejection, pinch zoom, sharing and joining (mocked server), serialization, file save/reopen/overwrite/delete, native drawing plus save/reopen, phone/tablet layouts, update metadata validation, downgrade filtering, download integrity/cancellation and install-permission sequencing. Android native installer code compiles into the release APK; on-device installer interaction still needs hardware testing. Six server tests cover rooms, merging, board codes and the MCP endpoint. Not yet verified on real hardware: writing latency on the 65" panel and ML Kit recognition quality.

Public download: https://annotated-remedies-property-binding.trycloudflare.com/canvas-studio-native.apk

Temporary Cloudflare tunnel on Ubuntu, served by user services `canvas-studio-server` and `canvas-studio-public-tunnel`. Link depends on the running host/tunnel. The directory exposes APK downloads and their checksums, not the source project.

## Publishing future updates

Increment both version name and integer build code in `app/pubspec.yaml` (currently `2.2.1+5`). Run the build script, then `python3 publish_native.py` on Ubuntu. The publisher checks the signing certificate against the existing APK, creates an immutable versioned APK, and atomically publishes `update.json` after the downloads are ready. Keep the same application ID and signing key to preserve installed boards. Do not share the signing key.

The update feed currently uses the temporary public tunnel above. For durable updates and sharing, use a permanent HTTPS host: set `UPDATE_MANIFEST_URL` and `COLLAB_SERVER_URL` during builds and `PUBLIC_BASE_URL` while publishing to that host. The APK download must use the same HTTPS origin as its feed. A stopped or changed tunnel makes the current feed unreachable; users receive a retry message. The update UI never silently installs an APK.

Editable logo source: `branding/logo.svg`. Native Flutter mark: `app/lib/brand_logo.dart`. Rebuild PNG launcher assets with `python3 branding/generate_icons.py --help`.
