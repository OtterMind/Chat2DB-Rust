# Architecture

## Status

The repository has completed the buildable baseline and the versioned
Rust-to-Java lifecycle protocol. Protobuf generation, framing, handshake, ping,
request correlation, stderr capture, process exit classification, and shutdown
are implemented and cross-language tested.

JDBC sessions, database operations, streaming rows, storage, Tauri, AI, MCP,
plugin migration, and packaging remain target components. In particular,
protocol readiness is not reported as database readiness before the Stage 3
JDBC gate passes.

## Ownership

| Area | Owner | Boundary |
| --- | --- | --- |
| Shared UI | React and TypeScript | One transport-neutral backend client |
| Desktop | Rust / Tauri 2 | Tauri commands and events; no product localhost HTTP |
| Web | Rust / Axum | JSON HTTP, SSE, and WebSocket |
| Product services | Rust | Workspace, state, policy, tasks, dashboards, and orchestration |
| Durable state | Rust | SQLite plus OS-rooted secret protection |
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

The implemented lifecycle subset is documented in [`protocol.md`](protocol.md).
It intentionally contains no placeholder database request that could report a
false success.

## Database boundary

Java remains the primary database implementation at the first complete release.
Rust does not reimplement vendor wire protocols and does not introduce parallel
native-driver behavior before the Java-backed conformance baseline passes.

The compatibility engine retains:

- dynamic driver JAR loading and classloader isolation;
- JDBC connection, statement, result, and transaction ownership;
- Chat2DB database plugins, metadata, builders, and type conversion;
- relational and existing non-relational operations;
- Java ANTLR parsing, splitting, formatting, validation, and completion.

Spring Boot, Spring Web, Spring AI, MCP, JCEF, product storage, and updater logic
do not belong in the final Java engine.

## Large result boundary

Java streams typed row batches to Rust. Rust writes retained batches to bounded
disk files and indexes chunk offsets and expiry in SQLite. UI, CLI, and MCP
clients page by an opaque result id.

AI tools receive schema, counts, truncation state, bounded samples, statistics,
and the result id. They never append an unbounded database result directly to
model history. Follow-up tools inspect or aggregate the retained result under a
new explicit budget.

## Security baseline

- Community binds to loopback by default.
- Non-loopback Web mode requires an explicit access token.
- Local CLI/MCP attachment uses an owner-only Unix-domain socket or Windows
  named pipe.
- Secrets are rooted in the operating-system credential store and are never
  persisted by Java.
- SQL write access is enforced outside prompts and scoped to the active run.
- User-provided driver JARs are treated as native-trust code.

## Packaging target

The desktop package contains the Tauri/Rust product, React assets, a private Java
compatibility JAR, a jlink-minimized Java 17 runtime, and a small signed core
driver pack. Long-tail driver packs are signed, versioned, downloaded on demand,
and independently rollback-capable.

The installed-size target is 30% to 45% below the equivalent Community package.
This is an acceptance target, not a measured current result.
