# Staged Delivery

Every stage ends in a focused commit and must leave `main` buildable. A stage is
complete only after its local gates pass; planned capabilities remain explicit
in runtime health until then.

| Stage | Status | Deliverable | Required evidence |
| --- | --- | --- | --- |
| 1 | Complete | Repository baseline | Rust format/Clippy/tests, Java tests/package, frontend typecheck/build, CI workflow |
| 2 | Complete | Rust-Java process protocol | Generated Protobuf in both languages, handshake, ping, version negotiation, stderr capture, process shutdown and crash tests |
| 3 | Complete | JDBC vertical slice | Dynamic H2 driver load, session lifecycle, typed query batches, backpressure, cancellation, transaction semantics, Rust integration tests |
| 4 | Complete | Product and result storage foundation | SQLite migration/integrity gates, mandatory vault boundary, revisioned datasource records, durable result frames, bounded paging/quota, expiry, writer cleanup and recovery tests |
| 5 | Complete | Product transports | Generated OpenAPI/TypeScript contract, Axum JSON/SSE, Tauri 2 commands/channels, shared SQL workbench, product H2 tests |
| 6 | In progress | Agent, MCP, and CLI | Implemented provider adapters, durable bounded tool loop, SQL tools/permissions, result handles, compaction, and Web/Tauri run transports; pending `rmcp` and local CLI/MCP attachment |
| 7 | Planned | Chat2DB compatibility estate | Existing database SPI/plugins, JDBC packs, Java ANTLR parsers, metadata/builders, per-dialect conformance |
| 8 | Planned | Packaging and release | jlink runtime, Tauri installers, signed product/engine/driver manifests, atomic update and rollback, size measurement |

Stage 3 completion means the versioned Rust-Java bridge can load an external
JDBC driver, own sessions and local transactions, execute updates, and stream
typed query batches under explicit limits, credits, deadlines, and
cancellation. Stage 5 now composes that bridge into the Web and desktop product
hosts; the CLI remains a status-only adapter and does not own a product runtime.

Stage 4 completion means `chat2db-storage` owns a process-locked SQLite schema,
datasource revisions and secret references, persistent secret-cleanup intents,
and immutable completed result files indexed by full-frame hashes. Result pages
have row and encoded-byte limits, quota accounting includes indexed and physical
files, active writers hold leases, and startup rejects unknown result formats
before mutation. Stage 5 adds the production vault adapters and composes storage
into Web and desktop; the status-only CLI still reports optional components as
disabled.

Stage 5 completion means Rust DTOs generate a checked-in OpenAPI document and
TypeScript types, one React SQL workbench selects either an HTTP/SSE or Tauri
command/channel adapter, and both transports expose the same datasource, query,
operation, cancellation, and retained-result services. Query submission is
asynchronous, event replay is cursor-based and bounded, disconnecting a stream
does not cancel work, and Web API fallbacks remain JSON rather than SPA HTML.
The Web and desktop bootstrap paths fail closed unless durable storage, a
production secret vault, and the Java engine start successfully. Real H2 product
tests cover secret-backed datasource execution, retained paging, explicit
cancellation, and Java session release. Managed driver-pack installation and
the existing Chat2DB plugin/ANTLR compatibility estate remain Stage 7, so the
production hosts currently require drivers to be provisioned by a future driver
manager; H2 is loaded only by the product integration fixture.

The implemented Stage 6 slice provides direct OpenAI, Anthropic, and Gemini
adapters; a provider-neutral bounded Agent loop; durable session, message, run,
permission, and compaction state; and Core-owned start, snapshot, replay,
cancellation, terminal commit, and shutdown behavior. Datasource-bound runs can
query through a forced read-only JDBC session, inspect retained results through
run-bound expiring handles, and execute a write only after a fresh approval
bound to the exact tool call and argument digest. Axum exposes the lifecycle as
JSON plus replay/live SSE; Tauri exposes matching commands and independent
channels; the frontend has matching HTTP/Tauri observers with bounded recovery.
Stage 6 is not complete until `rmcp` and the owner-only local CLI/MCP attachment
path use these same services and policies.

## Commit policy

- One semantic commit per completed stage or independently reviewable stage
  slice.
- No generated output without its source and reproducible generation command.
- No disabled test, placeholder success response, or capability claim used to
  make a stage appear complete.
- Schema and protocol compatibility changes include fixtures from the prior
  version.
- Every database write/cancellation/crash path includes an unknown-outcome test.
