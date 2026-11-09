# ADR-0003: Browser IPC and external media tools

- Status: Accepted; media-tool provisioning amended 2026-11-09
- Date: 2026-11-05

## Decision

Browser extensions communicate with a dedicated Rust Native Messaging host.
Browser manifests restrict allowed extension identities; the host independently
checks the identity supplied at launch and validates a versioned, size-bounded
JSON envelope. The host forwards requests to a per-user local IPC endpoint
owned by the running desktop application. No HTTP listener is exposed.

Captured direct downloads enter the existing `DownloadService::add` path. The
desktop acknowledges a capture only after its queue record has been committed.
Idempotency keys prevent duplicate browser submissions.

Media support is implemented in a Tauri-independent `media-engine` crate. Its
adapter invokes `yt-dlp`, FFmpeg, and FFprobe executables without a shell. Tool
output is bounded and parsed from machine-readable JSON. The adapter accepts
only HTTP/HTTPS sources and refuses loopback, link-local, and private literal IP
addresses.

Official installers bundle version-pinned media executables and a Deno
JavaScript runtime for yt-dlp's YouTube challenge solver as Tauri resources.
A reproducible preparation script selects artifacts by target triple, fetches
them from pinned upstream releases, verifies immutable SHA-256 values and binary
architecture, runs native version and encoder smoke checks, and stages license,
build, and corresponding-source information. Verification failure fails the
release build. GrabBit never downloads executables during application startup.

Runtime resolution is per tool: a valid explicit advanced override wins,
followed by the bundled executable, then an optional system-installed fallback.
Empty or invalid overrides never mask a valid bundled executable.

The browser extension never sends cookies, authorization headers, or page DOM
content. GrabBit does not implement DRM, paywall, signature, or access-control
circumvention. Users are responsible for confirming authorization.

## Consequences

Browser capture requires the desktop app to be running. Failure leaves the
browser download intact. Bundled media tools increase installer size and make
their pinned versions, checksums, licenses, build configuration, and source
links release inputs. Settings report each resolved tool's version and
bundled/system/custom source. Safari distribution also depends on Apple signing
and a containing Xcode application target.
