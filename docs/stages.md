# Staged Delivery

Every stage ends in a focused commit and must leave `main` buildable. A stage is
complete only after its local gates pass; planned capabilities remain explicit
in runtime health until then.

| Stage | Deliverable | Required evidence |
| --- | --- | --- |
| 1 | Repository baseline | Rust format/Clippy/tests, Java tests/package, frontend typecheck/build, CI workflow |
| 2 | Rust-Java process protocol | Generated Protobuf in both languages, handshake, ping, version negotiation, stderr capture, process shutdown and crash tests |
| 3 | JDBC vertical slice | Dynamic H2 driver load, session lifecycle, typed query batches, backpressure, cancellation, transaction semantics, Rust integration tests |
| 4 | Product and result storage | SQLite migrations, secret boundary, datasource records, disk-backed result chunks, paging, expiry and recovery tests |
| 5 | Product transports | Shared generated frontend contract, Axum APIs/streams, Tauri 2 IPC/events, desktop/Web parity tests |
| 6 | Agent, MCP, and CLI | Provider adapters, bounded tool loop, result handles, compaction, SQL permissions, `rmcp`, local IPC attachment |
| 7 | Chat2DB compatibility estate | Existing database SPI/plugins, JDBC packs, Java ANTLR parsers, metadata/builders, per-dialect conformance |
| 8 | Packaging and release | jlink runtime, Tauri installers, signed product/engine/driver manifests, atomic update and rollback, size measurement |

## Commit policy

- One semantic commit per completed stage or independently reviewable stage
  slice.
- No generated output without its source and reproducible generation command.
- No disabled test, placeholder success response, or capability claim used to
  make a stage appear complete.
- Schema and protocol compatibility changes include fixtures from the prior
  version.
- Every database write/cancellation/crash path includes an unknown-outcome test.
