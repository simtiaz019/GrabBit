# Phase 2 architecture audit and checklist

## Audit

GrabBit Phase 1 is a small Cargo workspace with clean dependency direction.
`http-engine` owns validated HTTP transport, `download-core` owns scheduling and
destination policy, `download-storage` owns SQLite, and `src-tauri` only adapts
typed commands/events. The React application consumes real service data and has
no payload writer. These boundaries are retained.

Reusable Phase 1 components:

- `DownloadService::add` is the single durable queue-creation path for captured
  direct HTTP/HTTPS downloads.
- `HttpEngine::probe` and range validation remain the direct-file transport.
- `DownloadStorage` migrations, queue/history, settings, and recovery are
  extended instead of replaced.
- Tauri's managed `DownloadService`, progress events, destination picker, and
  existing list/details UI remain the desktop integration points.
- Existing filename, destination, TLS, checkpoint, validator, and finalization
  rules continue to apply to ordinary downloads.

Phase 2 adds a versioned, current-user local IPC boundary. Browser extensions
talk only to the native messaging host. The host authenticates the extension
identity, validates and bounds a shared request envelope, then forwards it over
the user-scoped IPC channel. It never accepts commands or arbitrary paths.

Automatic browser capture uses one desktop-owned `off` / `ask` / `automatic`
setting. The extension asks the desktop for that setting for every interception
decision and fails closed when GrabBit is unavailable. Chrome reports an
absolute destination in `DownloadItem.filename`; the extension reduces that
untrusted value to a basename before sending it through the native protocol.
This avoids the protocol path-separator rejection that previously prevented
automatic captures even though manual URL captures worked.

Media inspection/execution is isolated in `media-engine`. It invokes configured
`yt-dlp`, FFmpeg, and FFprobe executables directly with argument arrays, a
cleared environment, bounded output, cancellation, and timeouts. It does not
contain provider-specific access-control bypasses and never receives cookies or
browser authentication data from the extension.

Safari requires an Apple-signed containing application and Safari Web Extension
target. The repository can provide the bridge sources, entitlements template,
and packaging documentation; signing and an actual Safari run require external
Apple credentials and cannot be asserted by CI.

## Milestone checklist

- [x] M2.1 repository and architecture audit
- [x] M2.2 native messaging host and desktop IPC — bounded Unix socket and current-user Windows named-pipe implementations complete; Windows requires native verification
- [ ] M2.3 Chromium extension — implementation/build complete; Brave passed once but repeatability and Chrome/Edge limitations remain
- [ ] M2.4 Firefox extension and Safari bridge/package template — Firefox implementation/build and Safari request/reply bridge complete; browser/signing runs remain
- [x] M2.5 media detection and inspection
- [x] M2.6 media selection and processing — execution/finalization, structured byte progress, process-tree cancellation, and FFmpeg fixtures complete
- [x] M2.7 desktop UI integration — format selection, stages, browser diagnostics, and secure in-memory thumbnail proxy complete
- [x] M2.8 database and settings migration
- [x] M2.9 security and privacy controls
- [x] M2.10 deterministic tests — protocol, policy, storage, parsing, Phase 1 integration, UI, real FFmpeg/FFprobe fixtures, and fake-adapter progress/cancellation covered
- [ ] M2.11 packaging and installation — per-user install/uninstall scripts, Windows pipe, and Safari target resources complete; signed Safari and native Windows packages remain
- [ ] M2.12 release verification — macOS checks and Brave smoke test complete; unavailable browsers and native Windows/Linux/Safari runners remain

## Beta functional repair (2026-11-06)

- [x] Manual HTTP baseline covers queue creation, transfer, pause, durable
  checkpoint, resume, SHA-256 verification, exact bytes, and completion.
- [x] Packaged apps copy the user-scoped native host on launch; the first-run
  wizard installs/repairs manifests only for the browsers the user selects.
- [x] Browser discovery reports detected, registered, executable-valid, and
  unknown extension states separately without reading browser profiles.
- [x] A skippable eight-step wizard guides detection, selection, repair,
  browser approval, connectivity, and capture preferences.
- [x] Settings exposes versions, real recent PING/capture state, per-browser
  registration, repair, and sanitized diagnostics.
- [x] WebExtensions provide a user-clicked on-page media action for recognized
  media pages and HTML video/audio, plus popup/context-menu fallbacks.
- [x] Windows NSIS is current-user scoped and removes only GrabBit-owned HKCU
  registrations and host files during uninstall.
- [ ] Native Windows install/upgrade/uninstall verification requires the
  Windows release runner; macOS cannot substitute for that result.
- [ ] Store-signed Chromium/Firefox and Apple-signed Safari distribution remain
  external requirements. Browser approval is intentionally not bypassed.

The checklist is updated only after implementation and relevant checks.

## Browser support verification matrix

| Browser | Extension build | Native manifest | Desktop IPC | Real browser smoke test (2026-11-05) |
| --- | --- | --- | --- | --- |
| Brave 155, macOS | Pass | Pass | Pass | **Inconsistent:** one full packaged run passed all capture/fallback/restart checks; later reruns timed out before automatic capture, with no browser download event recorded |
| Google Chrome 156, macOS | Pass | Pass | Pass independently | **Blocked:** final isolated-profile run ignored the requested unpacked extension and opened its URL as `chrome-error://chromewebdata/`; the unrelated installed extension was the only service worker. The user's manually loaded extension connects, but the live profile was not modified or stopped |
| Google Chrome for Testing 157, macOS | Pass; service worker starts | Requires `/Library/Google/ChromeForTesting`, which needs administrator access | Not reached | **Blocked:** test browser does not read the per-user production Chrome manifest |
| Edge, macOS | Pass | Installer tolerates absent browser | Pass by shared Chromium path | Not run: browser unavailable |
| Firefox, macOS | Pass | Pass | Pass by shared Unix path | Not run: browser unavailable |
| Chromium family / Firefox, Linux | Pass | Pass | Pass by shared bounded Unix-socket path | Not run: Linux environment unavailable |
| Chromium family / Firefox, Windows | Pass | Pass | Implemented with a random capability pipe, remote-client rejection, bounded messages, and timeouts | Not run: Windows environment unavailable |
| Safari | Resources, manifest, Swift handler, state helper, and entitlements prepared | Xcode signing required | App-group request/reply path implemented | Not run: full Xcode and signing credentials unavailable |

The installed-bundle Brave smoke test launches a copy of the app extracted from
the DMG and loads the extension that app copied from its bundle resources into
the stable application-support directory. It uses a temporary HOME, profile,
downloads directory, and app-data directory; the harness seeds the matching
per-user Native Messaging manifest inside that temporary HOME. This covers
release resource sync, the packaged native host, extension origin allowlist,
and real `downloads.onCreated` interception. It does not exercise the app's
interactive browser-registration wizard.

Automatic capture cannot reliably reproduce POST downloads, blob URLs,
browser-internal pages, service-worker-private requests, or authenticated
downloads because GrabBit intentionally does not collect request bodies,
cookies, or authorization headers. Signed/credential-bearing query URLs are
rejected instead of being persisted in plaintext.

## Verification record

The final verification record distinguishes successful checks from environment
blockers. Apple signing, native Windows/Linux execution, unavailable browsers,
and public-network media sources are not claimed.

Successfully rerun on macOS arm64 on 2026-11-06:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check --workspace`
- `cargo test --workspace` (39 tests, including the pause/resume/SHA-256 and
  local FFmpeg/FFprobe integration tests)
- `npm run lint`
- `npm run typecheck`
- `npm run extension:typecheck`
- `npm test` (29 tests)
- `npm run extension:build`
- `npm run build`
- `npm run tauri build -- --bundles dmg` (ad-hoc signed `.app` and `.dmg`;
  notarization skipped because Apple credentials were unavailable)
- Installed-bundle Brave smoke test for automatic capture/cancellation, Ask
  choices, site exclusions, redirect/ISO naming, Content-Disposition, unknown
  length, duplicate prevention, cancellation failure, app-unavailable fallback,
  and browser/desktop restarts passed once. Later reruns were not repeatable:
  the last stopped after manual capture when the browser produced no automatic
  download event.

The media fixture test generates and probes MP4, WebM, M4A, Opus, split
audio/video, merged output, HLS, and DASH locally. It also verifies SHA-256
readability, corrupt/unsupported failure paths, structured adapter progress, and
process-group cancellation. The on-page content action is covered by a DOM test
that confirms its click forwards only the page URL; the UI event-to-dialog path
is covered by a frontend test. Real provider/YouTube and full extension-to-
installed-app media handoff testing were not performed; yt-dlp and authorized
provider credentials were not used.

Google Chrome's installed-bundle smoke test was attempted with an isolated
profile. Chrome ignored the requested unpacked extension and opened its page as
`chrome-error://chromewebdata/`; an unrelated extension service worker was the
only worker visible. Chrome behavior therefore remains blocked on this runner.
Native Windows, Linux, Firefox, Edge, and signed Safari installer/browser runs
were not available. Windows registry-key mapping has a Windows-only unit test,
but PowerShell parsing, Windows compilation, and execution remain unverified
here.
