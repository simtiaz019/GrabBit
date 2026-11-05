# GrabBit architecture

GrabBit is a Tauri desktop shell around a Rust application service. React is a
presentation client; it never downloads or writes payload files.

## Dependency direction

```text
React UI -> Tauri commands -> download-core
                              /           \
                    http-engine       download-storage
                              \           /
                              download-common

Browser extension -> Native Messaging host -> per-user IPC -> Tauri adapter
Tauri commands -> download-core -> media-engine -> yt-dlp / FFmpeg / FFprobe
```

- `download-common`: serializable domain types, errors, and state rules.
- `http-engine`: HTTP metadata and payload transport. It cannot depend on Tauri,
  React, SQLite, or destination policy.
- `download-storage`: SQLite jobs, settings, and checkpoints plus migration logic.
- `download-core`: scheduler and application services coordinating transport,
  durable storage, and destination policy.
- `src-tauri`: narrow, typed commands adapting the UI to application services.
- `src`: React views and bridge wrappers; no independent download engine.
- `browser-native-host`: bounded native framing, extension identity checks, and
  forwarding only; it cannot write download payloads or execute arbitrary tools.
- `media-engine`: Tauri-independent inspection and media-tool process adapter;
  `download-core` retains queue, destination, and finalization policy.

## Trust boundaries

URLs, redirects, response headers, filenames, and payloads are untrusted. The
core service will own URL-scheme and destination validation. The engine will
retain byte-level HTTP validation. The storage layer will only checkpoint bytes
after durable file writes. Tauri commands expose purpose-built operations, not
arbitrary filesystem access; M1 grants only `core:default` to the main window.

## Recovery invariant

A persisted checkpoint must never exceed bytes known to be durably written.
Resource validators (strong ETag, or Last-Modified where appropriate) bind
partial bytes to a remote representation. If validation fails, old and new
bytes are never merged. This invariant is documented now and implemented/tested
in later milestones.

## Runtime flow

The Tauri adapter initializes one `DownloadService`, which opens SQLite,
reconciles interrupted jobs, and owns a bounded global scheduler. Purpose-built
commands probe URLs, create jobs, and change lifecycle state. The service emits
throttled progress snapshots; React renders them but never handles payloads.
Browser capture uses one SQLite-backed `off` / `ask` / `automatic` mode.
Extensions read and update that setting through the native host and fail closed
when the desktop IPC service is unavailable.
`http-engine` probes with HEAD and a ranged GET fallback. Transfers use bounded
stream chunks, validated HTTP 206 ranges, up to 32 segments, per-segment
retries, and cancellation tokens. `download-core` owns partial paths, resource
identity checks, optional SHA-256 validation, and non-overwriting finalization.
SQLite stores queue/history/settings and crash-recovery state.

## Recovery guarantee

Only bytes flushed and synchronized to disk may advance a durable checkpoint.
See ADR-0002 for pause, validator, preallocation, and finalization behavior.
