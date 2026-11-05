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
- [ ] M2.3 Chromium extension — implementation/build and Brave run complete; Chrome/Edge limitations are recorded below
- [ ] M2.4 Firefox extension and Safari bridge/package template — Firefox implementation/build and Safari request/reply bridge complete; browser/signing runs remain
- [x] M2.5 media detection and inspection
- [x] M2.6 media selection and processing — execution/finalization, structured byte progress, process-tree cancellation, and FFmpeg fixtures complete
- [x] M2.7 desktop UI integration — format selection, stages, browser diagnostics, and secure in-memory thumbnail proxy complete
- [x] M2.8 database and settings migration
- [x] M2.9 security and privacy controls
- [x] M2.10 deterministic tests — protocol, policy, storage, parsing, Phase 1 integration, UI, real FFmpeg/FFprobe fixtures, and fake-adapter progress/cancellation covered
- [ ] M2.11 packaging and installation — per-user install/uninstall scripts, Windows pipe, and Safari target resources complete; signed Safari and native Windows packages remain
- [ ] M2.12 release verification — macOS checks and Brave smoke test complete; unavailable browsers and native Windows/Linux/Safari runners remain

The checklist is updated only after implementation and relevant checks.

## Browser support verification matrix

| Browser | Extension build | Native manifest | Desktop IPC | Real browser smoke test (2026-11-05) |
| --- | --- | --- | --- | --- |
| Brave 155, macOS | Pass | Pass | Pass | **Pass:** pong, durable 8 MiB capture, duplicate acknowledgement, app-unavailable response, browser restart, and desktop restart |
| Google Chrome 156, macOS | Pass | Pass | Pass independently | **Blocked:** the installed production build ignores command-line unpacked-extension loading; no false smoke claim |
| Google Chrome for Testing 157, macOS | Pass; service worker starts | Requires `/Library/Google/ChromeForTesting`, which needs administrator access | Not reached | **Blocked:** test browser does not read the per-user production Chrome manifest |
| Edge, macOS | Pass | Installer tolerates absent browser | Pass by shared Chromium path | Not run: browser unavailable |
| Firefox, macOS | Pass | Pass | Pass by shared Unix path | Not run: browser unavailable |
| Chromium family / Firefox, Linux | Pass | Pass | Pass by shared bounded Unix-socket path | Not run: Linux environment unavailable |
| Chromium family / Firefox, Windows | Pass | Pass | Implemented with a random capability pipe, remote-client rejection, bounded messages, and timeouts | Not run: Windows environment unavailable |
| Safari | Resources, manifest, Swift handler, state helper, and entitlements prepared | Xcode signing required | App-group request/reply path implemented | Not run: full Xcode and signing credentials unavailable |

The Brave smoke test exercises the final release native host and final extension
origin allowlist. Automatic interception and context-menu UI still require an
interactive browser permission/UI gesture: the optional all-sites permission is
deliberately not pre-granted or bypassed by the test harness. Their policy and
acknowledgement behavior remain covered by deterministic extension tests.

Automatic capture cannot reliably reproduce POST downloads, blob URLs,
browser-internal pages, service-worker-private requests, or authenticated
downloads because GrabBit intentionally does not collect request bodies,
cookies, or authorization headers. Signed/credential-bearing query URLs are
rejected instead of being persisted in plaintext.

## Verification record

The final verification record distinguishes successful checks from environment
blockers. Apple signing, native Windows/Linux execution, unavailable browsers,
and public-network media sources are not claimed.

Successfully run on macOS arm64 on 2026-11-05:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features` (33 tests, including two real FFmpeg/FFprobe integration tests)
- `cargo check --workspace --all-targets --all-features`
- `cargo check --target x86_64-pc-windows-msvc -p browser-native-host --all-targets`
- `npm run lint`
- `npm run typecheck`
- `npm run extension:typecheck`
- `npm test` (21 tests)
- `npm run extension:build`
- `npm run build`
- `cargo build --release --bin grabbit-browser-host`
- `./safari/prepare-resources.sh`, plist/entitlement validation, and `swiftc -parse` for both Swift sources
- native-host framing and Brave browser smoke tests (approved origin, durable capture, duplicate handling, unavailable response, and restarts)
- `npm run tauri build` (ad-hoc signed `.app` and `.dmg`; notarization skipped
  because Apple credentials were unavailable)

The media fixture test generates and probes MP4, WebM, M4A, Opus, split
audio/video, merged output, HLS, and DASH locally. It also verifies SHA-256
readability, corrupt/unsupported failure paths, structured adapter progress, and
process-group cancellation. Real yt-dlp execution is blocked because yt-dlp is
not installed and GrabBit's dependency policy requires users to install and
configure external media tools explicitly.

The Windows native host cross-check passed. A full desktop cross-check stopped
in the third-party `aws-lc-sys` build because this macOS machine has no Windows
SDK headers; the named-pipe server therefore remains source- and host-client
checked but not natively verified. Linux uses the already-tested Unix transport,
but no Linux runtime was available for a native smoke test.
