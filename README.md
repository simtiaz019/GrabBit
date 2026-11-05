<p align="center">
  <img src="public/branding/logo.png" alt="GrabBit" width="360">
</p>

# GrabBit

GrabBit is a cross-platform HTTP/HTTPS download manager built with Rust, Tokio,
reqwest, SQLite, Tauri 2, React, TypeScript, Vite, and Tailwind CSS.

The official raster branding masters are in `public/branding/`. Platform bundle
icons generated from `logo-icon.png` are in `src-tauri/icons/`; the original
application identifier remains `com.grabbit.desktop` so existing app data paths
and installations are not migrated.

## Features

- URL metadata inspection with safe filenames and redirects
- Sequential and validated multi-part range downloads (1–32 segments)
- Bounded retries, transfer concurrency, memory use, and cancellation
- Pause, resume, cancel, retry, persistent queue, and history
- SQLite crash reconciliation and resource identity validation
- Optional SHA-256 verification and non-overwriting finalization
- Live progress, speed, ETA, filters, details, settings, and light/dark themes
- Windows, macOS, and Linux CI/release configuration
- Secure native-messaging capture for Chromium-family browsers and Firefox
- Authorized, non-DRM media inspection through configured yt-dlp/FFmpeg tools
- Real source-format selection, audio extraction, remux/conversion, and media job recovery

GrabBit never captures cookies or authorization headers and does not bypass DRM,
paywalls, or access controls. Media downloads are for content the user owns or
is authorized to download. Torrents and multi-network aggregation remain out of
scope.

## Run

Install [Tauri's platform prerequisites](docs/INSTALLATION.md), then:

```sh
npm ci
npm run tauri dev
```

Select **Add download**, enter an HTTP/HTTPS URL, inspect its metadata, choose a
download folder, and start the transfer. GrabBit never silently overwrites an
existing destination or partial file.

## Test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo check --workspace --all-targets --all-features
npm run lint
npm run typecheck
npm test
npm run build
```

The Rust suite uses controlled local HTTP fixtures and verifies exact file bytes
and SHA-256, ranges, range fallback, malformed responses, changed validators,
SQLite recovery, integrity checks, collision handling, and end-to-end service
finalization. It does not rely on public download URLs.

## Package

```sh
npm run tauri build
```

See [installation and packaging](docs/INSTALLATION.md),
[architecture](docs/ARCHITECTURE.md), and
[recovery guarantees](docs/adr/0002-transfer-recovery-and-finalization.md).

Phase 2 implementation status, browser support, and honest platform limitations
are tracked in [the Phase 2 audit](docs/PHASE2_AUDIT.md).
