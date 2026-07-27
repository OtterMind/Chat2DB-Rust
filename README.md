# Chat2DB Rust

Private implementation of the Chat2DB Community hybrid runtime.

## Current state

The repository has completed Stages 1 through 6 and the first twelve independently
buildable Stage 7 slices:

- canonical Rust API contracts;
- a transport-neutral Rust application service root;
- an Axum health API bound to loopback by default;
- a Rust CLI status command;
- a React runtime-status shell;
- one Protobuf 1.0 lifecycle and JDBC contract generated in Rust and Java;
- a supervised Rust client with handshake, ping, bounded stderr capture,
  request correlation, crash reporting, and forced/clean shutdown;
- an external-driver Java 17 compatibility engine whose stdout is
  protocol-only;
- Rust driver, session, transaction, update, and credit-streaming query APIs;
- typed row batches, cancellation, deadlines, bounded results, and explicit
  unknown-outcome handling; and
- real Rust-to-Java H2 integration tests with H2 outside the engine JAR;
- process-locked SQLite storage with verified WAL, foreign keys, full
  synchronous writes, transactional migrations, and startup integrity checks;
- revisioned datasource metadata whose complete connection descriptor lives
  behind an injected, readiness-checked secret-vault boundary; and
- disk-backed Protobuf result frames with SHA-256 indexes, row/byte-bounded
  paging, a physical-byte quota, expiry, writer cleanup, and crash recovery;
- an AES-256-GCM encrypted file vault rooted in either an OS-keyring master key
  or an explicit headless master key;
- one production `RuntimeHost` that opens the vault and storage, supervises the
  Java engine, and shuts down active work deterministically;
- secret-safe datasource CRUD, asynchronous query operations, bounded replay,
  explicit cancellation, and retained-result paging through Axum JSON/SSE and
  Tauri 2 commands/channels;
- a checked-in OpenAPI contract with generated TypeScript types and drift
  verification; and
- one shared React SQL workbench with HTTP and Tauri backend adapters;
- a provider-neutral bounded agent loop with direct OpenAI, Anthropic, and
  Gemini adapters, durable sessions/messages/runs/permissions, and atomic
  context compaction;
- shared `query_database`, `inspect_query_result`, and
  `execute_database_write` tools with read-only enforcement, per-call write
  approval, bounded result previews, run-bound handles, and conservative
  unknown-outcome handling; and
- durable Agent run start, snapshot, cancellation, permission decision, and
  replay/live streaming through Axum SSE, Tauri channels, and matching
  frontend HTTP/Tauri observers;
- an authenticated owner-only local attachment started by both product hosts,
  plus a JSON CLI for datasource discovery, forced-read-only query lifecycle,
  cancellation, and retained-result paging; and
- an `rmcp` 2.2 stdio server with five bounded datasource/query tools backed by
  that same running `Application`;
- strict local JDBC driver-pack discovery, hash verification, startup preload,
  and immutable Core/Axum/Tauri inventory; and
- a fixed Community 5.3.0 compatibility classpath that discovers real
  `IPlugin` implementations and exposes H2 plugin catalog, schema, object,
  view, foreign-key, primary-key, function, procedure, parameter, and trigger
  metadata, `CREATE SCHEMA` builder, and retained ANTLR parser operations over
  Protobuf, with every one of its 149 JARs bound to the source commit by a
  checked-in filename, byte-length, and SHA-256 lock; and
- product-owned Community DTOs and Core services exposed consistently through
  Axum, Tauri, and the shared HTTP/Tauri frontend backend contract, with exact
  locked-classpath startup and forced-read-only metadata sessions; and
- an end-user Community object explorer that selects plugin, database, and
  schema scopes, preserves partial metadata from long-tail plugins, lazily opens
  relational and programmability details, inserts generated schema SQL into the
  console, and explicitly analyzes editor SQL; and
- bounded Community ANTLR syntax validation with statement summaries and source
  diagnostics exposed through Core, Axum, Tauri, and the shared React editor;
  and
- Community-compatible SQL formatting with bounded input, output, and lexical
  complexity, negotiated capability support, and one race-safe Format action
  across HTTP and Tauri; and
- retained Community SQL completion through a forced-read-only JDBC session,
  bounded UTF-16 edit ranges and suggestions, and one race-safe keyboard-driven
  completion surface across HTTP and Tauri; and
- typed Community single-row and batch INSERT plus ordered equality-predicate
  UPDATE generation, with no raw-SQL value escape hatch, no JDBC session, and a
  table-scoped editor dialog that inserts generated SQL without executing it;
  and
- closed database/schema namespace SQL generation for create, alter, drop, and
  use operations, with real H2 and MySQL Community builders, no JDBC session,
  and a race-safe editor dialog that never executes generated SQL.

Stage 6 is complete. Web and desktop own the product runtime and publish its
owner-only local endpoint; CLI and MCP attach to that host and never contact
Java directly. The current MCP surface is deliberately read-only and does not
accept JDBC bind parameters. A complete end-user Agent workspace and
CLI-started headless host remain follow-on product work. Stage 7A implements
strict local driver packs. Stage 7B pins Community source, loads its runtime in
an isolated Java classloader, and proves one real H2 SPI/ANTLR vertical slice.
Stage 7C composes those four operations into the product Core and both delivery
transports. Stage 7D adds database, table, column, and index metadata through a
separate capability. Stage 7E adds views, imported and exported foreign keys,
and primary keys through another capability. Stage 7F adds functions,
function parameters, procedures, procedure parameters, and triggers through the
same Core/Axum/Tauri/frontend boundary. Stage 7G connects all 20 fixed Community
operations to the shared React workbench through a three-pane object explorer,
partial long-tail metadata, lazy detail views, schema SQL generation, and
explicit SQL analysis. Stage 7H adds a separately negotiated SQL-validation
capability and an explicit editor Validate action without opening a JDBC
session. Stage 7I adds separately negotiated SQL formatting through the retained
Community formatter dependency, preserves Community's dialect mapping and
fallback behavior, and replaces editor SQL only while the originating SQL,
datasource, and database type are still current. Stage 7J adds the separately
negotiated `community.sql-completion.v1` capability, calls the real Community
completion service against the existing read-only JDBC session, and exposes
bounded suggestions through Core, Axum, Tauri, and the shared React editor.
Stage 7K adds the separately negotiated `community.dml-builder.v1` capability,
calls the selected plugin's real DML, value, and identifier processors without
opening a JDBC session, and exposes typed INSERT/UPDATE generation through the
same product boundary and table detail UI.
Stage 7L adds `community.namespace-builder.v1`, retains the old CREATE SCHEMA
contract, and exposes a closed database/schema DDL union through Core, Axum,
Tauri, and the shared frontend. The fixed 149-JAR classpath keeps H2 and MySQL;
PostgreSQL and other dialects do not block the first testable MySQL milestone.
Signing, downloading, updating, rollback, the remaining compatibility estate,
and full per-dialect compatibility remain Stage 7 work.

## Architecture

The final runtime is intentionally hybrid:

```text
React / TypeScript              CLI / MCP client
  -> Tauri IPC or Axum HTTP       -> JSON CLI or MCP stdio
                                  -> owner-only local attachment
                    \            /
                     -> Rust product runtime
  -> framed Protobuf IPC
  -> private Java compatibility engine
  -> Chat2DB plugins + JDBC + Java ANTLR
  -> database
```

See [`docs/architecture.md`](docs/architecture.md) for ownership,
[`docs/protocol.md`](docs/protocol.md) for the implemented 1.0 process contract,
and [`docs/driver-packs.md`](docs/driver-packs.md) for the local manifest and
startup contract.

## Build

Prerequisites:

- Rust 1.88 or newer;
- Java 17; the checked-in Maven Wrapper downloads Maven 3.9.12;
- Node.js 22.12 or newer within the Node 22 release line, and npm 10.9.7.

Run all current verification gates, including real Rust-to-Java process tests:

```bash
make verify
```

Java verification downloads H2 `2.3.232` into
`java/compat-runtime/target/test-drivers/` as an external Stage 3 test fixture.
H2 is not a runtime dependency of the compatibility engine, and the packaged
JAR integration test rejects any build that embeds `org/h2/Driver.class`.
The H2 gates cover the Stage 3 JDBC bridge, the Stage 5 product path from a
vault-backed datasource through retained-result paging and cancellation, and
the Stage 7B Community path through real `IPlugin`, `IDbMetaData`,
`ISqlBuilder`, and ANTLR parser implementations. The Stage 7C-7L product gate
repeats those calls through encrypted datasource storage and Core services,
including forced-read-only schema, object, relation, and programmability
metadata session cleanup plus datasource-free parsing, validation, formatting,
typed DML generation, namespace SQL generation, plus datasource-bound table and
column completion. H2
remains an external test driver rather than a runtime dependency of either Java
classpath.
The Stage 7J completion workbench also passes Playwright visual acceptance at
desktop `1440x1000` and mobile `390x844` viewports, with no overlapping or
out-of-bounds content, horizontal page scrolling, or text overflow.

Build and run only the fixed Community H2 compatibility gate with:

```bash
make community-h2-integration
make community-product-h2-integration
```

Those targets require a clean submodule at the fixed commit, build through the
checked-in Maven Wrapper and a repository-local Maven cache, derive archive
timestamps from the commit, exclude the H2 JDBC driver, and deterministically
remove dependency-manifest `Class-Path` entries before rejecting any JAR set
that differs from `third_party/community-h2-classpath.lock`. Run
`make community-h2-reproducibility` to compare every artifact byte across two
consecutive clean builds.

Generate or verify the external contracts:

```bash
make generate-contracts
make check-contracts
```

Build the Java engine, fixed Community classpath, and shared frontend, then run
the Web product host with the Stage 7C-7L services enabled:

```bash
make java community-h2-classpath frontend
CHAT2DB_JAVA_ENGINE_JAR="$PWD/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
CHAT2DB_COMMUNITY_CLASSPATH_DIR="$PWD/target/community-h2-classpath" \
CHAT2DB_VAULT_MASTER_KEY="$(openssl rand -base64 32)" \
cargo run -p chat2db-web
```

Run the frontend in another terminal:

```bash
cd apps/frontend
npm run dev
```

The Web API listens on `127.0.0.1:4200` by default. The Vite development server
listens on `127.0.0.1:4210` and proxies `/api` to the Rust runtime.
`CHAT2DB_DATA_DIR` selects a profile directory. Omitting
`CHAT2DB_VAULT_MASTER_KEY` selects the OS credential store. Any non-loopback
`CHAT2DB_BIND` also requires `CHAT2DB_ACCESS_TOKEN` with at least 32 bytes.

The running Web or desktop host also publishes the owner-only local endpoint
used by CLI and MCP. Point either adapter at the same profile explicitly when
the default data directory is not used:

```bash
cargo run -p chat2db-cli -- --data-dir /path/to/profile datasources
cargo run -p chat2db-mcp -- --data-dir /path/to/profile
```

MCP clients launch `chat2db-mcp` as a stdio server. Its stdout is reserved for
JSON-RPC; diagnostics use stderr. `CHAT2DB_MCP_LOG` can raise logging for the
Chat2DB target only, while dependency logs remain capped at `WARN` so SQL and
result payloads are not emitted by `rmcp` debug tracing.

## Repository layout

```text
apps/
  chat2db-cli/       Rust command-line adapter
  chat2db-desktop/   Tauri 2 desktop adapter
  chat2db-mcp/       bounded stdio MCP adapter
  chat2db-web/       Axum Web adapter
  frontend/          shared React application
contracts/openapi/   generated external HTTP contract
crates/
  chat2db-agent/     provider adapters and bounded agent/tool runtime
  chat2db-contract/  canonical DTOs and errors
  chat2db-core/      transport-neutral product services
  chat2db-engine-protocol/ generated internal wire types and frame codec
  chat2db-java-bridge/ supervised Java process client
  chat2db-local/     owner-only CLI/MCP attachment protocol
  chat2db-local-ipc-windows/ Windows named-pipe and ACL implementation
  chat2db-storage/   SQLite state, secret references, and retained results
proto/               canonical Rust-Java process schema
java/
  compat-runtime/    private Java compatibility process
docs/                architecture and staged delivery contract
scripts/             contract generation and drift checks
```
