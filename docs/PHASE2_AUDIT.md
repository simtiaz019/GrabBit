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
- [ ] M2.2 native messaging host and desktop IPC — complete on macOS/Linux; secure Windows named-pipe transport remains
- [ ] M2.3 Chromium extension — implementation/build complete; real Chrome, Brave, and Edge runs remain
- [ ] M2.4 Firefox extension and Safari bridge/package template — Firefox implementation/build complete; browser run and signed Safari reply channel remain
- [x] M2.5 media detection and inspection
- [ ] M2.6 media selection and processing — execution/finalization complete; byte progress and deterministic FFmpeg fixture coverage remain
- [ ] M2.7 desktop UI integration — real format selection and stages complete; secure thumbnail proxy remains
- [x] M2.8 database and settings migration
- [x] M2.9 security and privacy controls
- [ ] M2.10 deterministic tests — protocol, policy, storage, parsing, Phase 1 integration, and UI covered; real tool fixtures remain
- [ ] M2.11 packaging and installation — per-user manifests/scripts complete; Windows IPC and signed Safari packaging remain
- [ ] M2.12 release verification — runnable checks recorded below; browser/platform smoke tests remain

The checklist is updated only after implementation and relevant checks.

## Browser support verification matrix

| Browser | Extension build | Native manifest | Desktop IPC | Real browser smoke test |
| --- | --- | --- | --- | --- |
| Chrome / Brave / Edge, macOS/Linux | Implemented | Implemented | Implemented | Not run |
| Firefox, macOS/Linux | Implemented | Implemented | Implemented | Not run |
| Chrome / Brave / Edge / Firefox, Windows | Implemented | Implemented | Blocked: current-user named pipe | Not run |
| Safari | Shared resources and Swift validation scaffold | Xcode signing required | Reply channel incomplete | Not run |

Automatic capture cannot reliably reproduce POST downloads, blob URLs,
browser-internal pages, service-worker-private requests, or authenticated
downloads because GrabBit intentionally does not collect request bodies,
cookies, or authorization headers. Signed/credential-bearing query URLs are
rejected instead of being persisted in plaintext.

## Verification record

The final verification record is populated only with commands successfully run
in the working tree. Apple signing, real-browser smoke tests, Windows IPC, and
public-network media sources are not CI claims.

Successfully run on macOS arm64 on 2026-11-05:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features` (29 tests)
- `cargo check --workspace --all-targets --all-features`
- `npm run lint`
- `npm run typecheck`
- `npm run extension:typecheck`
- `npm test` (21 tests)
- `npm run extension:build`
- `npm run build`
- `cargo build --release --bin grabbit-browser-host`
- native-host framing smoke test (approved origin; app-unavailable fallback)
- `npm run tauri build` (ad-hoc signed `.app` and `.dmg`; notarization skipped
  because Apple credentials were unavailable)
