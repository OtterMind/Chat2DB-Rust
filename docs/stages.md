# Staged Delivery

Every stage ends in a focused commit and must leave `main` buildable. A stage is
complete only after its local gates pass; planned capabilities remain explicit
in runtime health until then.

| Stage | Status | Deliverable | Required evidence |
| --- | --- | --- | --- |
| 1 | Complete | Repository baseline | Rust format/Clippy/tests, Java tests/package, frontend typecheck/build, CI workflow |
| 2 | Complete | Rust-Java process protocol | Generated Protobuf in both languages, handshake, ping, version negotiation, stderr capture, process shutdown and crash tests |
| 3 | Complete | JDBC vertical slice | Dynamic H2 driver load, session lifecycle, typed query batches, backpressure, cancellation, transaction semantics, Rust integration tests |
| 4 | Planned | Product and result storage | SQLite migrations, secret boundary, datasource records, disk-backed result chunks, paging, expiry and recovery tests |
| 5 | Planned | Product transports | Shared generated frontend contract, Axum APIs/streams, Tauri 2 IPC/events, desktop/Web parity tests |
| 6 | Planned | Agent, MCP, and CLI | Provider adapters, bounded tool loop, result handles, compaction, SQL permissions, `rmcp`, local IPC attachment |
| 7 | Planned | Chat2DB compatibility estate | Existing database SPI/plugins, JDBC packs, Java ANTLR parsers, metadata/builders, per-dialect conformance |
| 8 | Planned | Packaging and release | jlink runtime, Tauri installers, signed product/engine/driver manifests, atomic update and rollback, size measurement |

Stage 3 completion means the versioned Rust-Java bridge can load an external
JDBC driver, own sessions and local transactions, execute updates, and stream
typed query batches under explicit limits, credits, deadlines, and
cancellation. It does not mean the bootstrap product runtime composes that
bridge: `chat2db-core` and the Web adapter do not yet start the Java supervisor,
so `database-engine` remains `disabled` until product integration is delivered.

## Commit policy

- One semantic commit per completed stage or independently reviewable stage
  slice.
- No generated output without its source and reproducible generation command.
- No disabled test, placeholder success response, or capability claim used to
  make a stage appear complete.
- Schema and protocol compatibility changes include fixtures from the prior
  version.
- Every database write/cancellation/crash path includes an unknown-outcome test.
