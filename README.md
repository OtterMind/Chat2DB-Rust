# Chat2DB Rust

Source-available implementation of the Chat2DB Community hybrid runtime.

Chat2DB Rust owns the product runtime in Rust, uses a native Rust path for the
current MySQL browser and SELECT slice, and retains the broader public Chat2DB
Community database compatibility layer behind a supervised Java process. The
repository is under active development and is not yet a stable end-user
release.

## Clone

The Community compatibility source is a public Git submodule pinned to an
exact commit. Clone it together with the main repository:

```bash
git clone --recurse-submodules https://github.com/OtterMind/Chat2DB-Rust.git
cd Chat2DB-Rust
```

For an existing checkout, initialize the same pinned source with:

```bash
git submodule update --init --recursive
```

## Current state

The repository has completed Stages 1 through 6, the first thirteen
independently buildable Stage 7 slices, and the first end-user Community
Console compatibility slice:

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
- one production `RuntimeHost` that opens the vault, storage, and verified
  driver catalog without starting Java, then single-flights first-use startup,
  leases each generation to active work, reaps it after three idle minutes,
  reloads drivers on the next use, and shuts down deterministically;
- secret-safe datasource CRUD, asynchronous query operations, bounded replay,
  explicit cancellation, and retained-result paging through Axum JSON/SSE and
  Tauri 2 commands/channels;
- a checked-in OpenAPI contract with generated TypeScript types and drift
  verification; and
- the pinned original Community Umi/React layout and components, served by
  Axum over historical HTTP routes on Web and bridged from `window.javaQuery`
  to Tauri IPC on desktop without a replacement UI or style fork; the pinned
  source carries one CSP-safe callback-cloning compatibility fix;
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
- strict local JDBC driver-pack discovery, hash verification, immutable
  Core/Axum/Tauri inventory, and repeatable per-generation preload; and
- a fixed Community 5.3.0 compatibility classpath that discovers real
  `IPlugin` implementations and exposes H2 plugin catalog, schema, object,
  view, foreign-key, primary-key, function, procedure, parameter, and trigger
  metadata, `CREATE SCHEMA` builder, and retained ANTLR parser operations over
  Protobuf, with every one of its 149 JARs bound to the source commit by a
  checked-in filename, byte-length, and SHA-256 lock; and
- product-owned Community DTOs and Core services exposed consistently through
  Axum and Tauri, with exact locked-classpath startup and forced-read-only
  metadata sessions; and
- an original-frontend compatibility layer for native MySQL connection testing,
  datasource CRUD/tree, database/schema/table discovery, and synchronous table
  preview over the historical `{success,data,errorCode,errorMessage}` envelope;
  and
- durable SQLite-backed Community Console create/get/list/update/delete,
  including SQL text, datasource/database/schema binding, saved status, and
  open-tab state across process restarts; and
- Community Console SELECT execution through upstream `mysql_async 0.37.0`, a
  MySQL read-only transaction, and the existing bounded Core retained-result
  path without starting Java: Web uses the historical synchronous result shape,
  while desktop maps operation lifecycle, rows, failure, and cancellation to
  the original JCEF event bus;
- a shared Web/Tauri legacy dispatcher: Axum maps the original `/api` routes,
  while desktop preserves the original JCEF correlation envelope through one
  `legacy_request` Tauri command; and
- forced-read-only table preview that ignores caller SQL, generates a bounded
  SELECT through the selected Community plugin, and pages retained results.

Runtime-tested: yes. The Stage 7M product vertical passed against a real local
MySQL 8.4 server on 2026-07-27, including plugin-built qualified table SQL,
forced-read-only execution, and retained paging of the selected table's rows.
Commit `928e62c5d775d0e81d95db7fee186db756834a72` additionally passed the
complete local repository gate and a live original-frontend legacy HTTP
vertical covering connection, datasource persistence, database/table listing,
and three-row table preview. On 2026-07-28 commits `36ecac6`, `78e92d6`, and
`c51fdff` passed formatting, strict workspace Clippy, all 509 Rust tests, 49
frontend tests, and the Community production build. A live MySQL 8.4 run then
created a Console, returned real table rows, returned a renderable SQL error,
saved edited SQL, closed it, restarted the Rust host, reopened it, and executed
the restored SQL successfully. On 2026-07-29 commits `81301c3`, `4199862`, and
`6c74421` passed 144 Core unit tests, strict Core Clippy, and a real MySQL 8.4
vertical covering native connection, database/schema/table discovery, preview,
typed Console SELECT, row truncation, active-query cancellation, retained
paging, and proof after every operation that Java remained dormant.
The complete repository `make verify` gate and the explicit real-MySQL
`native-mysql-integration` target also passed after the final compatibility fix.

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
Tauri, and the shared frontend. Stage 7M adds `community.dql-builder.v1` at tag
`225`: Java uses the selected plugin to quote the database/schema/table name and
build a row-limited SELECT without opening JDBC, then Rust validates that SQL and
executes it through the existing forced-read-only query and retained-result
path. The fixed 149-JAR classpath keeps H2 and MySQL; PostgreSQL and other
dialects do not block the MySQL preview. MySQL writes, Agent, CLI, and MCP
conformance remain outside this small read-only milestone. The current MySQL
connection, database/schema/table, preview, and supported Console SELECT routes
dispatch to `mysql_async` before Java lease acquisition; Community parser,
formatter, completion, builders, and advanced metadata remain Java-backed.

The first Console compatibility slice adds SQLite migration 3 for saved
Consoles and the historical `/api/operation/saved/*` plus
`/api/rdb/dml/execute` routes. Web waits on the Core operation and returns the
existing Community grid result. Desktop starts the same Core query, emits the
original `sql_execution_event` sequence through Tauri, and supports
`sql-cancel`. This slice intentionally supports query/SELECT execution only;
arbitrary DDL, DML, and multi-statement Console scripts are not implemented.

The Stage 5 and Stage 7G through Stage 7M custom React workbench was an
intermediate implementation and is no longer the product frontend. Commit
`928e62c5d775d0e81d95db7fee186db756834a72` deletes that replacement UI and
its styles. Current builds export the original Community frontend from the
pinned submodule; backend capabilities not yet mapped to its historical API
remain internal rather than requiring a redesigned page.
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
`ISqlBuilder`, and ANTLR parser implementations. The Stage 7C-7M product gate
repeats those calls through encrypted datasource storage and Core services,
including forced-read-only schema, object, relation, and programmability
metadata session cleanup plus datasource-free parsing, validation, formatting,
typed DML generation, namespace SQL generation, plus datasource-bound table and
column completion and table preview. H2 remains an external test driver rather
than a runtime dependency of either Java classpath.
The Stage 7J completion workbench also passes Playwright visual acceptance at
desktop `1440x1000` and mobile `390x844` viewports, with no overlapping or
out-of-bounds content, horizontal page scrolling, or text overflow.

Build and run only the fixed Community H2 compatibility gate with:

```bash
make community-h2-integration
make community-product-h2-integration
```

Prepare the pinned MySQL Connector/J pack and run the complete real MySQL
product vertical against a local server:

```bash
MYSQL_TEST_USER=root \
MYSQL_TEST_PASSWORD='<local password>' \
make community-product-mysql-integration
```

Run the native MySQL product path alone, without building Java or Connector/J:

```bash
MYSQL_TEST_USER=root \
MYSQL_TEST_PASSWORD='<local password>' \
make native-mysql-integration
```

`MYSQL_TEST_HOST` and `MYSQL_TEST_PORT` default to `127.0.0.1:3306`. The test
creates a uniquely named database, verifies driver loading, datasource CRUD,
metadata, namespace DDL, typed DML, parsing, validation, formatting, completion,
plugin-built table-preview SQL, forced-read-only query execution, and
retained-result paging, then drops the database even when verification fails.
Connector/J is downloaded from Maven Central only after its pinned byte length
and SHA-256 are verified; it remains an external driver pack and is never
embedded in the Java engine.

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

Build the Java engine, fixed Community classpath, verified MySQL driver pack,
and shared frontend, then run the Web product host with the Stage 7C-7M services
enabled:

```bash
make java community-h2-classpath mysql-driver-pack frontend
CHAT2DB_JAVA_ENGINE_JAR="$PWD/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
CHAT2DB_COMMUNITY_CLASSPATH_DIR="$PWD/target/community-h2-classpath" \
CHAT2DB_DRIVER_PACK_DIR="$PWD/target/mysql-driver-packs" \
CHAT2DB_VAULT_MASTER_KEY="$(openssl rand -base64 32)" \
cargo run -p chat2db-web
```

Run the frontend in another terminal:

```bash
cd apps/frontend
npm run dev
```

The Web API listens on `127.0.0.1:4200` by default. The pinned Community Umi
development server listens on `127.0.0.1:4210` and proxies `/api` to the Rust
runtime. Frontend commands export the exact Git tree recorded in
`scripts/community-frontend.lock.json` into `target/`; they never install into
or modify the `third_party/chat2db-community` submodule worktree.
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
  frontend/          build harness and transport tests for the pinned Community UI
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

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Contributions are accepted under the
repository license.

## Security

Do not report vulnerabilities in public issues. Follow
[`SECURITY.md`](SECURITY.md) to submit a private report.

## License

Chat2DB Rust is source-available under `LicenseRef-Chat2DB`. See
[`LICENSE`](LICENSE) for the complete terms. The pinned Chat2DB Community
submodule and other third-party components remain subject to their own license
terms and notices.
