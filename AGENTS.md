# Contributor instructions

## Scope and architecture

- Follow `docs/ARCHITECTURE.md` and accepted ADRs.
- Keep HTTP transport in `http-engine`; it must not depend on Tauri or SQLite.
- Keep orchestration and destination policy in `download-core`.
- Keep SQL and migrations in `download-storage`.
- Do not add a JavaScript download engine.
- Do not add browser extensions, torrents, or multi-network aggregation in Phase 1.

## Safety and correctness

- Accept only HTTP/HTTPS and never disable TLS verification.
- Treat URLs, redirects, filenames, headers, and paths as untrusted.
- Never log credentials, cookies, auth headers, or sensitive query strings.
- Write file data durably before advancing a database checkpoint.
- Validate range status and `Content-Range`; HTTP 200 is not a valid arbitrary
  ranged-segment response.
- Preserve explicit `DownloadState` transitions and add tests for changes.

## Definition of done

Run Rust formatting, Clippy with warnings denied, workspace tests, frontend
lint/typecheck/tests/build, and relevant deterministic integration tests. Do not
claim a check passed unless it was executed successfully.

