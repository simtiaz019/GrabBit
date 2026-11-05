# ADR-0001: Modular boundaries and narrow IPC

- Status: Accepted
- Date: 2026-11-05

## Context

GrabBit needs a reusable, testable download engine and a desktop UI without
duplicating download behavior in JavaScript. Recovery also requires strict
ordering between filesystem and database operations.

## Decision

Use a Cargo workspace with four library crates and a Tauri adapter. Domain
types live in `download-common`. `http-engine` remains transport-only and has no
Tauri or SQLite dependencies. `download-storage` owns SQLite. `download-core`
will coordinate both and enforce destination and scheduling policy. React calls
only explicitly registered Tauri commands through small typed wrappers.

Download lifecycle transitions are validated in the domain model. Completed
and cancelled jobs are terminal; paused and failed jobs re-enter through the
queue, enabling the scheduler and persistence layer to observe every retry.

The initial capability grants only Tauri core defaults. Future filesystem or
dialog access must be introduced as narrowly scoped capabilities with a review
of destination authorization.

## Consequences

The UI cannot bypass core policy. The engine is independently integration
testable with a local HTTP fixture. Some JSON contracts are mirrored in
TypeScript until schema generation is adopted; contract tests and review are
required when shared response types change.

