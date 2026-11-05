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

The NSIS package is a current-user install with normal Start Menu integration,
downgrade protection, and GrabBit installer branding. On launch the app copies
the matching `grabbit-browser-host.exe` into `%LOCALAPPDATA%\GrabBit`. The
first-run wizard registers only the browsers the user selects, using GrabBit-
owned HKCU Native Messaging keys and manifests. It does not modify browser
policy or profiles.

### macOS

Install Xcode Command Line Tools. Apple Silicon uses
`aarch64-apple-darwin`; Intel uses `x86_64-apple-darwin`. Public distribution
requires an Apple Developer ID certificate, hardened runtime signing, and
notarization. Local unsigned builds are suitable only for development.

A DMG cannot run a post-install script when the app is copied. GrabBit performs
user-scoped browser registration on first launch and presents an in-app setup
wizard. The wizard can be skipped; direct downloads from GrabBit continue to
work without browser extensions.

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
install per-user manifests for Chrome, Brave, Edge, Chromium, and Firefox; they
do not require an unauthenticated HTTP listener. Load the unpacked extension from
`extensions/chromium/dist` or `extensions/firefox/dist` during development.

Capture mode is persisted by GrabBit and can be changed in **Settings > Browser
integration** or the extension popup. Both use the same setting through the
native host; the extension does not use a separate cached capture preference.
Choose **Off**, **Ask every time**, or **Automatic**. The browser transfer is
cancelled only after GrabBit reports that a queue job was durably created.
Browser API
timing means some downloads that start from browser-internal pages, blobs,
service workers, or POST requests cannot be captured reliably; those downloads
remain in the browser. GrabBit does not forward cookies, authentication headers,
or request bodies.

At first launch, the browser setup wizard detects supported applications, lets
the user select integrations, and installs or repairs the user-scoped native
host only for those selections. It shows the stable unpacked-extension
directory for explicit browser approval, reports the most recent real extension
PING/capture result, and configures the download directory and interception
mode. Detection is not treated as proof that an extension is installed. Until
an extension connects, installed, enabled, and permission states remain
`Unknown`.

The app bundle includes current Chromium and Firefox unpacked extension files.
On launch, GrabBit refreshes them at
`~/Library/Application Support/com.grabbit.desktop/browser-extensions/` and
updates the native-messaging host at
`~/Library/Application Support/GrabBit/grabbit-browser-host`. For a packaged
macOS install, use the `chromium` or `firefox` directory there with the
browser's **Load unpacked** command. After replacing GrabBit with a newer DMG,
reload the unpacked extension in the browser's extension manager (or restart
the browser) so it reads the updated files. Extensions loaded from a repository
or another directory are not modified by the DMG; migrate those once to the
stable application-support location. Development installs may continue to load
from `extensions/chromium/dist` or `extensions/firefox/dist`.

The beta extension requests HTTP/HTTPS page access so it can show a
user-clicked **Download with GrabBit** action on recognized media pages and
pages containing HTML video/audio. Clicking it forwards only the page URL and
opens GrabBit's authorization/format dialog. It does not forward cookies,
authorization headers, request bodies, or media stream credentials. Popup and
right-click media actions remain available when an on-page action is absent.

The desktop IPC transport uses a mode-0600 Unix socket on macOS/Linux. Windows
uses a random current-user capability pipe recorded under the user's app-data
directory; remote pipe clients are rejected and every exchange is size- and
time-bounded. No platform falls back to an unauthenticated local HTTP service.
Use the matching `uninstall-unix.sh` or `uninstall-windows.ps1` script to remove
the manifests and copied host binary.

## Upgrade, repair, and removal

- **Upgrade:** install/replace the newer app and launch it once. GrabBit
  refreshes its app-owned host and extension directory. Reload the unpacked
  extension so its service worker and content script update.
- **Repair:** use **Settings > Browser integration > Repair integration**. This
  rewrites only GrabBit's user-scoped manifests/registry keys.
- **Windows uninstall:** the NSIS hook removes GrabBit's HKCU Native Messaging
  keys, copied host, and manifests. Downloads and application data are
  preserved.
- **macOS/Linux cleanup:** run the supported uninstall script or the app's
  integration-removal command before deleting the app. Browser extensions stay
  browser-managed and must be removed in the browser UI.
- Never delete a browser profile to repair or remove GrabBit.

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
