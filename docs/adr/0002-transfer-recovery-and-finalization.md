# ADR-0002: Transfer recovery and finalization

- Status: Accepted
- Date: 2026-11-05

## Decision

Payloads are written beside their destination with the suffix
`.grabbit.part`. A sequential pause flushes and synchronizes data before its
length becomes a SQLite checkpoint. Segmented partial files are preallocated,
so each segment flushes and synchronizes its completed range before emitting a
per-segment SQLite checkpoint. Resume starts every range at its own durable
offset. The implementation never infers downloaded bytes from a preallocated
file length, and resets stale checkpoints when the partial file is missing.

Strong ETags are preferred for representation identity. Last-Modified and
content length are fallback signals. Resume is rejected if a known validator or
length changes. Ranged responses must be HTTP 206 with the requested
Content-Range; HTTP 200 triggers a clean sequential restart rather than being
accepted as segment data.

Completion requires expected length, optional SHA-256 verification, durable
file data, and collision-safe same-filesystem finalization. A hard link creates
the destination atomically without overwriting an existing file; the partial
name is removed only after the link succeeds.

## Consequences

Sequential and segmented downloads retain only safely checkpointed bytes.
Incomplete bytes after the last segment checkpoint may be overwritten during
resume. Existing destination files always win a collision.

