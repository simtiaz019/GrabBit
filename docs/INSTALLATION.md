# Installation and packaging

## Development

Install the platform prerequisites, then run `npm ci` and
`npm run tauri dev` from the repository root.

## Build an installer locally

```sh
npm ci
npm run tauri build
```

Tauri writes native bundles below `target/release/bundle`. Bundles are
platform-specific and must be produced on their target operating system.

### Windows 11

Install Microsoft C++ Build Tools with the Desktop development with C++
workload and ensure WebView2 is available. The release workflow produces the
configured Windows installer. Production distribution requires a Windows code
signing certificate to avoid untrusted-publisher warnings.

### macOS

Install Xcode Command Line Tools. Apple Silicon uses
`aarch64-apple-darwin`; Intel uses `x86_64-apple-darwin`. Public distribution
requires an Apple Developer ID certificate, hardened runtime signing, and
notarization. Local unsigned builds are suitable only for development.

### Linux

Install WebKitGTK 4.1, AppIndicator, librsvg, patchelf, and xdg-utils. Package
availability and names vary by distribution. The release workflow builds on
Ubuntu 22.04 and produces the Tauri-supported Linux bundles.

## Releases

Pushing a `v*` tag or manually dispatching the Release workflow builds Windows,
Linux, macOS Apple Silicon, and macOS Intel artifacts into a draft GitHub
release. Signing secrets are intentionally not embedded in the repository.

## Browser integration

Build the WebExtensions and native host:

```sh
npm run extension:build
cargo build --release --bin grabbit-browser-host
```

On macOS or Linux, run `packaging/native-messaging/install-unix.sh`. On Windows,
run `packaging/native-messaging/install-windows.ps1` in PowerShell. The scripts
install per-user manifests for Chrome, Brave, Edge, and Firefox; they do not
require an unauthenticated HTTP listener. Load the unpacked extension from
`extensions/chromium/dist` or `extensions/firefox/dist` during development.

Automatic interception is off by default. The browser transfer is cancelled
only after GrabBit reports that a queue job was durably created. Browser API
timing means some downloads that start from browser-internal pages, blobs,
service workers, or POST requests cannot be captured reliably; those downloads
remain in the browser. GrabBit does not forward cookies, authentication headers,
or request bodies.

The desktop IPC transport uses a mode-0600 Unix socket on macOS/Linux. Windows
uses a random current-user capability pipe recorded under the user's app-data
directory; remote pipe clients are rejected and every exchange is size- and
time-bounded. No platform falls back to an unauthenticated local HTTP service.
Use the matching `uninstall-unix.sh` or `uninstall-windows.ps1` script to remove
the manifests and copied host binary.

## Media tools

Install yt-dlp, FFmpeg, and FFprobe from trusted sources, then enter their
absolute executable paths in **Settings > Media tools**. GrabBit validates the
paths and invokes them directly without a shell. It never downloads or executes
tool updates silently. Review the tools' licenses and your media provider's
terms before use.

Tool processes run in a cancellable process group where the platform supports
it. Thumbnails are fetched only from credential-free public HTTPS destinations,
with DNS rebinding/redirect checks, a strict MIME allowlist, and a three-MiB
limit; remote thumbnail URLs are never handed directly to the webview.

The media dialog lists only formats reported by the extractor. `Original`
avoids an intentional conversion; MP4/WebM/MKV may remux compatible streams;
audio formats marked conversion use FFmpeg and cannot improve a lossy source.
DRM-protected, login-restricted, private-network, and unavailable sources are
rejected. Use a provider's official download/export feature when direct media
downloading is not permitted.

Safari requires the signed containing-app workflow documented in
`safari/README.md`. A normal unsigned Tauri build cannot distribute or verify a
Safari Web Extension.

