# Architecture

## Status

The repository has completed the buildable baseline, the versioned Rust-to-Java
process protocol, the JDBC vertical slice, and the local storage foundation. In
addition to lifecycle supervision, the implemented bridge loads external driver
JARs, owns sessions and local transactions, executes updates, and streams typed
query batches with credits, cancellation, deadlines, hard limits, and explicit
unknown outcomes. The complete database path is cross-language tested against
H2 without embedding H2 in the compatibility-engine JAR.

The implemented storage crate provides SQLite product records and disk-backed
retained results. Tauri, AI, MCP, Chat2DB plugin/parser integration, production
OS-vault integration, transport APIs, and packaging remain target components.
The bootstrap Web composition starts neither storage nor the Java supervisor,
so product health still reports both `local-storage` and `database-engine` as
`disabled`; tested components are not presented as product-wired services.

## Ownership

| Area | Owner | Boundary |
| --- | --- | --- |
| Shared UI | React and TypeScript | One transport-neutral backend client |
| Desktop | Rust / Tauri 2 | Tauri commands and events; no product localhost HTTP |
| Web | Rust / Axum | JSON HTTP, SSE, and WebSocket |
| Product services | Rust | Workspace, state, policy, tasks, dashboards, and orchestration |
| Durable state | Rust | SQLite, retained-result files, and a mandatory injected secret-vault contract |
| AI agent | Rust | Provider adapters, tool loop, limits, compaction, and cancellation |
| MCP and CLI | Rust | Adapters around the same product services and policy |
| Database compatibility | Java 17 | Existing SPI/plugins, JDBC, metadata, builders, and execution |
| SQL parsing | Java 17 | Existing Java ANTLR grammars, parser behavior, and completion |
| Rust-to-Java IPC | Shared Protobuf contract | Length-prefixed frames over private stdin/stdout |

## Process topology

```text
Desktop                         Web / Docker
React in system WebView         React in browser
  -> Tauri IPC                    -> HTTP / SSE / WebSocket
  -> Rust host                    -> Axum Rust host
          \                       /
           -> Rust application services
              -> SQLite and result store
              -> AI / MCP / CLI adapters
              -> Java process supervisor
                 -> Protobuf stdin/stdout
                 -> Java database compatibility engine
                    -> plugin registry
                    -> JDBC sessions and transactions
                    -> metadata and SQL operations
                    -> Java ANTLR parsers
                    -> vendor driver
```

Only Rust exposes product transports. Java has no listening port. JDBC
connections, statements, result sets, parser trees, and Java exceptions remain
inside the Java process.

## Contract rules

- Rust DTOs are the source of truth for frontend request, response, error, and
  event contracts.
- Desktop and Web adapters call the same application services and contain no
  business rules.
- The Java protocol is versioned and capability-negotiated.
- Every cross-process request carries request, optional session, deadline,
  cancellation, and trace identity. Responses add stream sequence and terminal
  state while echoing request and trace identity.
- Typed row batches use explicit backpressure; stdout is protocol-only and
  stderr is diagnostic-only.
- Read-only metadata/parser work may retry after a Java restart. Transactions,
  DML, and unknown-outcome operations are never replayed automatically.

The implemented lifecycle and JDBC subset is documented in
[`protocol.md`](protocol.md). Capabilities are advertised only after their
cross-language acceptance gates pass.

## Database boundary

Java remains the primary database implementation at the first complete release.
Rust does not reimplement vendor wire protocols and does not introduce parallel
native-driver behavior before the Java-backed conformance baseline passes.

Stage 3 currently implements:

- verified external JAR snapshots and per-driver classloader isolation;
- JDBC session and local-transaction ownership;
- prepared query and update execution with typed parameters;
- typed row batches with row, byte, frame, and scalar limits;
- credit flow control, cancellation, deadlines, and conservative outcomes.

The complete compatibility engine will additionally retain the existing
Chat2DB-specific estate:

- Chat2DB database plugins, metadata, builders, and type conversion;
- relational and existing non-relational operations;
- Java ANTLR parsing, splitting, formatting, validation, and completion.

The existing Chat2DB database plugins, metadata implementations, SQL builders,
non-relational operations, and Java ANTLR estate are not yet integrated into
this repository; they remain Stage 7 work.

Spring Boot, Spring Web, Spring AI, MCP, JCEF, product storage, and updater logic
do not belong in the final Java engine.

## Local storage boundary

Stage 4 implements one exclusively process-owned data directory with owner-only
permissions. Every SQLite connection verifies WAL mode, foreign keys, and
`synchronous=FULL`; startup applies an explicit transactional schema migration,
runs integrity and foreign-key checks, and performs idempotent recovery before a
`Storage` handle is exposed.

SQLite stores datasource id, display name, driver id, opaque `SecretRef`,
revision, and timestamps. The complete connection descriptor, including JDBC
URL and properties, is one `SecretValue` owned by the injected vault and never
enters SQLite. Vault creation is create-only. A durable cleanup queue plus a
single secret mutation gate covers staging, datasource CAS, commit
reconciliation, rotation, and deferred deletion. Storage startup requires the
vault readiness probe to succeed.

Java streams typed row batches to Rust under bounded row and byte budgets. Rust
persists the schema and batches as four-byte big-endian length-prefixed Protobuf
frames and indexes full-frame SHA-256 hashes, offsets, ordinals, row ranges,
completion state, and expiry in SQLite. File data is synced before its index
transaction. Completed results are immutable and page reads enforce both row
and encoded-byte budgets.

Quota accounting covers the union of SQLite-indexed bytes and physical result
files, including orphan files and unindexed tails. Active writers hold
process-local leases; explicit abort, known finish failure, and ordinary drop
reclaim them immediately, while runtime expiry can reclaim abandoned writing
records. Startup removes incomplete/expired/corrupt data, truncates valid
unindexed tails, removes orphans, and rejects an unknown result format before
mutating any result.

AI tools receive schema, counts, truncation state, bounded samples, statistics,
and the result id. They never append an unbounded database result directly to
model history. Follow-up tools inspect or aggregate the retained result under a
new explicit budget.

## Security baseline

- Community binds to loopback by default.
- Non-loopback Web mode requires an explicit access token.
- Local CLI/MCP attachment uses an owner-only Unix-domain socket or Windows
  named pipe.
- Storage requires an injected, readiness-checked credential vault and never
  persists connection descriptors in SQLite or Java. A production OS-backed
  vault adapter is not implemented yet.
- SQL write access is enforced outside prompts and scoped to the active run.
- User-provided driver JARs are treated as native-trust code.
- Java copies each supplied driver artifact into a private snapshot, verifies
  its SHA-256, rejects manifest `Class-Path`, and deletes the snapshot only
  after the driver has no remaining session lease.

## Packaging target

The desktop package contains the Tauri/Rust product, React assets, a private Java
compatibility JAR, a jlink-minimized Java 17 runtime, and a small signed core
driver pack. Long-tail driver packs are signed, versioned, downloaded on demand,
and independently rollback-capable.

The installed-size target is 30% to 45% below the equivalent Community package.
This is an acceptance target, not a measured current result.
